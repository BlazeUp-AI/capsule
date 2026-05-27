//! Capsule Runtime - WebSocket handler

use crate::session::{Session, SessionError, SessionManager};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ── Message types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    TerminalInput { data: String },
    Resize { cols: u16, rows: u16 },
    Ping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Connected { session_id: String },
    Reconnected { session_id: String },
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
    State(sessions): State<Arc<SessionManager>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, query.session_id, sessions))
}

async fn handle_socket(
    socket: WebSocket,
    session_id: Option<String>,
    sessions: Arc<SessionManager>,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Try to reconnect or create new session
    let (session, is_reconnect) = match session_id {
        Some(id) => {
            match sessions.reconnect_session(&id).await {
                Ok(s) => (s, true),
                Err(SessionError::NotFound) => {
                    warn!(session_id = %id, "Reconnect failed: session not found");
                    let msg = ServerMessage::Error { message: "Session not found".into() };
                    let _ = ws_sender.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await;
                    return;
                }
                Err(SessionError::NotReconnectable) => {
                    warn!(session_id = %id, "Reconnect failed: session expired");
                    let msg = ServerMessage::Error { message: "Session expired".into() };
                    let _ = ws_sender.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await;
                    return;
                }
                Err(e) => {
                    error!(session_id = %id, error = %e, "Reconnect failed");
                    let msg = ServerMessage::Error { message: e.to_string() };
                    let _ = ws_sender.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await;
                    return;
                }
            }
        }
        None => {
            // Wait for initial resize to get terminal dimensions
            let (cols, rows) = match wait_for_initial_size(&mut ws_receiver).await {
                Some(size) => size,
                None => {
                    warn!("Client disconnected before sending initial size");
                    return;
                }
            };
            info!(cols, rows, "Got initial terminal size");

            match sessions.create_session(cols, rows).await {
                Ok(s) => (s, false),
                Err(e) => {
                    error!("Failed to create session: {}", e);
                    let msg = ServerMessage::Error { message: e.to_string() };
                    let _ = ws_sender.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await;
                    return;
                }
            }
        }
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

async fn wait_for_initial_size(
    ws_receiver: &mut futures::stream::SplitStream<WebSocket>,
) -> Option<(u16, u16)> {
    let timeout = tokio::time::Duration::from_secs(5);

    loop {
        let msg = match tokio::time::timeout(timeout, ws_receiver.next()).await {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(_))) | Ok(None) => return None,
            Err(_) => return Some((80, 24)),
        };

        let Message::Text(text) = msg else { continue };
        let Ok(ClientMessage::Resize { cols, rows }) = serde_json::from_str(&text) else { continue };
        return Some((cols, rows));
    }
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
                data = pty_output_rx.recv() => {
                    match data {
                        Some(data) => {
                            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = shutdown.notified() => {
                    break;
                }
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
            session.read().await.write(data.as_bytes()).await.is_ok()
        }
        ClientMessage::Resize { cols, rows } => {
            info!(cols, rows, "Resize");
            let _ = session.read().await.resize(cols, rows).await;
            true
        }
        ClientMessage::Ping => true,
    }
}
