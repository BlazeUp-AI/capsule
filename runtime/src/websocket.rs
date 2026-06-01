//! Capsule Runtime - WebSocket handler

use crate::docker::ContainerConfig;
use crate::session::{Session, SessionError, SessionManager};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

// ── Message types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    TerminalInput { data: String },
    Resize { cols: u16, rows: u16 },
    Ping,
    #[serde(rename = "session_config")]
    SessionConfig {
        credentials: Option<HashMap<String, String>>,
        agent: Option<String>,
        image: Option<String>,
        enable_dind: Option<bool>,
        repo: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Connected { session_id: String },
    Reconnected { session_id: String },
    SessionState { state: String, detail: String },
    Error { message: String },
    Pong,
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    session_id: Option<String>,
}

// ── WebSocket handler ──────────────────────────────────────────────────────────

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State((sessions, _, observal)): State<(Arc<SessionManager>, Option<String>, Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>)>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, query.session_id, sessions, observal))
}

async fn handle_socket(
    socket: WebSocket,
    session_id: Option<String>,
    sessions: Arc<SessionManager>,
    observal: Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let (session, is_reconnect) = match session_id {
        Some(id) => match resolve_reconnect(&id, &sessions, &mut ws_sender).await {
            Some(s) => s,
            None => return,
        },
        None => match resolve_new_session(&sessions, &mut ws_sender, &mut ws_receiver, &observal).await {
            Some(s) => s,
            None => return,
        },
    };

    let session_id = session.read().await.id.clone();

    // Send connected/reconnected confirmation
    let confirm_msg = if is_reconnect {
        info!(session_id = %session_id, "Client reconnected");
        ServerMessage::Reconnected { session_id: session_id.clone() }
    } else {
        info!(session_id = %session_id, "Session created");
        ServerMessage::Connected { session_id: session_id.clone() }
    };

    if ws_sender
        .send(Message::Text(serde_json::to_string(&confirm_msg).unwrap().into()))
        .await
        .is_err()
    {
        return;
    }

    // Run the session and get back the output receiver when done
    let output_rx = run_session(session.clone(), ws_sender, ws_receiver).await;

    // Mark session as disconnected (keeps container alive for reconnect)
    if let Some(rx) = output_rx {
        sessions.mark_disconnected(&session_id, rx).await;
    }
}

// ── Session resolution helpers ────────────────────────────────────────────────

/// Resolve a reconnect request, sending an error and returning None if it fails.
async fn resolve_reconnect(
    id: &str,
    sessions: &Arc<SessionManager>,
    ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>,
) -> Option<(Arc<RwLock<Session>>, bool)> {
    match sessions.reconnect_session(id).await {
        Ok(s) => Some((s, true)),
        Err(SessionError::NotFound) => {
            warn!(session_id = %id, "Reconnect failed: session not found");
            send_error(ws_sender, "Session not found").await;
            None
        }
        Err(SessionError::NotReconnectable) => {
            warn!(session_id = %id, "Reconnect failed: session expired");
            send_error(ws_sender, "Session expired").await;
            None
        }
        Err(e) => {
            error!(session_id = %id, error = %e, "Reconnect failed");
            send_error(ws_sender, &e.to_string()).await;
            None
        }
    }
}

/// Wait for initial resize + optional config, create a new session. Returns None on failure.
async fn resolve_new_session(
    sessions: &Arc<SessionManager>,
    ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    ws_receiver: &mut futures::stream::SplitStream<WebSocket>,
    observal: &Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>,
) -> Option<(Arc<RwLock<Session>>, bool)> {
    let (cols, rows, mut config) = match wait_for_init_messages(ws_receiver).await {
        Some(result) => result,
        None => {
            warn!("Client disconnected before sending initial messages");
            return None;
        }
    };
    info!(cols, rows, "Creating session");

    send_state(ws_sender, "provisioning", "creating container").await;

    // Provision Observal user (non-blocking — if it fails, session still works)
    let session_id_preview = Uuid::new_v4().to_string();
    let observal_guard = observal.read().await;
    if let Some(client) = observal_guard.as_ref() {
        match client.provision_session_user(&session_id_preview).await {
            Ok(tokens) => {
                config
                    .env_vars
                    .insert("OBSERVAL_TOKEN".to_string(), tokens.access_token.clone());
                config.env_vars.insert(
                    "OBSERVAL_SERVER_URL".to_string(),
                    client.api_url().to_string(),
                );
                info!("Observal user provisioned for session");
            }
            Err(e) => {
                warn!(error = %e, "Failed to provision Observal user, continuing without telemetry");
            }
        }
    }
    drop(observal_guard);

    match sessions.create_session(cols, rows, config).await {
        Ok(s) => Some((s, false)),
        Err(e) => {
            error!("Failed to create session: {}", e);
            send_error(ws_sender, &e.to_string()).await;
            None
        }
    }
}

/// Send an error message over the WebSocket.
async fn send_error(ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>, message: &str) {
    let msg = ServerMessage::Error {
        message: message.into(),
    };
    let _ = ws_sender
        .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
        .await;
}

/// Send a session state update for progressive loading.
async fn send_state(
    ws_sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    state: &str,
    detail: &str,
) {
    let msg = ServerMessage::SessionState {
        state: state.into(),
        detail: detail.into(),
    };
    let _ = ws_sender
        .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
        .await;
}


/// Wait for resize (required) and optional session_config messages.
/// Returns (cols, rows, ContainerConfig). Times out to defaults after 5s.
async fn wait_for_init_messages(
    ws_receiver: &mut futures::stream::SplitStream<WebSocket>,
) -> Option<(u16, u16, ContainerConfig)> {
    let timeout = tokio::time::Duration::from_secs(5);
    let mut cols = 80u16;
    let mut rows = 24u16;
    let mut got_resize = false;
    let mut config = ContainerConfig::default();

    loop {
        let msg = match tokio::time::timeout(timeout, ws_receiver.next()).await {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(_))) | Ok(None) => return None,
            Err(_) => break, // timeout — use defaults
        };

        let Message::Text(text) = msg else { continue };
        let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else {
            continue;
        };

        match client_msg {
            ClientMessage::Resize { cols: c, rows: r } => {
                cols = c;
                rows = r;
                got_resize = true;
                // If we already have resize but no config after 500ms, proceed
                if got_resize {
                    // Give a short window for config to arrive
                    let brief = tokio::time::Duration::from_millis(500);
                    match tokio::time::timeout(brief, ws_receiver.next()).await {
                        Ok(Some(Ok(Message::Text(t)))) => {
                            if let Ok(ClientMessage::SessionConfig {
                                credentials,
                                agent,
                                image,
                                enable_dind,
                                repo,
                            }) = serde_json::from_str(&t)
                            {
                                let mut env_vars = credentials.unwrap_or_default();
                                if let Some(a) = agent {
                                    env_vars.insert("CAPSULE_AGENT".to_string(), a);
                                }
                                if let Some(r) = repo {
                                    env_vars.insert("CAPSULE_REPO".to_string(), r);
                                }
                                config = ContainerConfig {
                                    image,
                                    env_vars,
                                    enable_dind: enable_dind.unwrap_or(false),
                                };
                            }
                        }
                        _ => {} // No config — use defaults
                    }
                    break;
                }
            }
            ClientMessage::SessionConfig {
                credentials,
                agent,
                image,
                enable_dind,
                repo,
            } => {
                let mut env_vars = credentials.unwrap_or_default();
                if let Some(a) = agent {
                    env_vars.insert("CAPSULE_AGENT".to_string(), a);
                }
                if let Some(r) = repo {
                    env_vars.insert("CAPSULE_REPO".to_string(), r);
                }
                config = ContainerConfig {
                    image,
                    env_vars,
                    enable_dind: enable_dind.unwrap_or(false),
                };
                if got_resize {
                    break;
                }
            }
            _ => continue,
        }
    }

    Some((cols, rows, config))
}

/// Run session and return the output receiver when done (for reconnect support)
async fn run_session(
    session: Arc<RwLock<Session>>,
    mut ws_sender: futures::stream::SplitSink<WebSocket, Message>,
    mut ws_receiver: futures::stream::SplitStream<WebSocket>,
) -> Option<tokio::sync::mpsc::Receiver<Vec<u8>>> {
    let mut pty_output_rx = session.write().await.take_output_rx()?;

    let session_for_receiver = Arc::clone(&session);

    // Shared flag to signal shutdown
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let shutdown_for_ws = Arc::clone(&shutdown);

    // Task: PTY -> WebSocket
    let pty_to_ws = tokio::spawn(async move {
        loop {
            tokio::select! {
                data = pty_output_rx.recv() => match data {
                    Some(data) => {
                        if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                _ = shutdown.notified() => break,
            }
        }
        pty_output_rx
    });

    // Task: WebSocket -> PTY
    while let Some(Ok(msg)) = ws_receiver.next().await {
        if !handle_ws_message(msg, &session_for_receiver).await {
            break;
        }
    }

    // Signal PTY task to stop and get the receiver back
    shutdown_for_ws.notify_one();

    match pty_to_ws.await {
        Ok(rx) => Some(rx),
        Err(_) => None,
    }
}

// ── Message dispatch ───────────────────────────────────────────────────────────

async fn handle_ws_message(msg: Message, session: &Arc<RwLock<Session>>) -> bool {
    // Update activity timestamp
    session.write().await.touch();
    
    match msg {
        Message::Text(text) => {
            let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else {
                return true;
            };
            handle_client_message(client_msg, session).await
        }
        Message::Binary(data) => {
            session.read().await.write(&data).await.is_ok()
        }
        Message::Close(_) => false,
        _ => true,
    }
}

async fn handle_client_message(msg: ClientMessage, session: &Arc<RwLock<Session>>) -> bool {
    match msg {
        ClientMessage::TerminalInput { data } => {
            session.write().await.touch_pty();
            session.read().await.write(data.as_bytes()).await.is_ok()
        }
        ClientMessage::Resize { cols, rows } => {
            let _ = session.read().await.resize(cols, rows).await;
            true
        }
        ClientMessage::Ping => true,
        ClientMessage::SessionConfig { .. } => true, // ignored after init
    }
}
