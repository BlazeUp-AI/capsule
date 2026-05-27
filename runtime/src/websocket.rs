//! Capsule Runtime - WebSocket handler

use crate::session::{Session, SessionManager, SessionState};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
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
    Error { message: String },
    Pong,
}

// ── WebSocket handler ──────────────────────────────────────────────────────────

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(sessions): State<Arc<SessionManager>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, sessions))
}

async fn handle_socket(socket: WebSocket, sessions: Arc<SessionManager>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Wait for initial resize to get correct terminal dimensions
    let (cols, rows) = match wait_for_initial_size(&mut ws_receiver).await {
        Some(size) => size,
        None => {
            warn!("Client disconnected before sending initial size");
            return;
        }
    };
    info!(cols, rows, "Got initial terminal size");

    // Create session with correct size
    let session = match sessions.create_session(cols, rows).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create session: {}", e);
            let msg = ServerMessage::Error { message: e.to_string() };
            let _ = ws_sender
                .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
                .await;
            return;
        }
    };

    let session_id = session.read().await.id.clone();
    info!(session_id = %session_id, "Session created");

    // Send connected confirmation
    let connected_msg = ServerMessage::Connected { session_id: session_id.clone() };
    if ws_sender
        .send(Message::Text(serde_json::to_string(&connected_msg).unwrap().into()))
        .await
        .is_err()
    {
        return;
    }

    run_session(session.clone(), ws_sender, ws_receiver).await;

    // Cleanup
    session.write().await.state = SessionState::Disconnected;
    sessions.remove_session(&session_id).await;
    info!(session_id = %session_id, "Session cleaned up");
}

async fn wait_for_initial_size(
    ws_receiver: &mut futures::stream::SplitStream<WebSocket>,
) -> Option<(u16, u16)> {
    let timeout = tokio::time::Duration::from_secs(5);

    loop {
        let msg = match tokio::time::timeout(timeout, ws_receiver.next()).await {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(_))) | Ok(None) => return None,
            Err(_) => return Some((80, 24)), // timeout, use defaults
        };

        let Message::Text(text) = msg else { continue };
        let Ok(ClientMessage::Resize { cols, rows }) = serde_json::from_str(&text) else { continue };
        return Some((cols, rows));
    }
}

async fn run_session(
    session: Arc<RwLock<Session>>,
    mut ws_sender: futures::stream::SplitSink<WebSocket, Message>,
    mut ws_receiver: futures::stream::SplitStream<WebSocket>,
) {
    let mut pty_output_rx = match session.write().await.take_output_rx() {
        Some(rx) => rx,
        None => return,
    };

    let session_for_receiver = Arc::clone(&session);

    // Task: PTY -> WebSocket (binary frames for terminal data)
    let pty_to_ws = tokio::spawn(async move {
        while let Some(data) = pty_output_rx.recv().await {
            // Send terminal output as raw binary - no JSON overhead
            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
    });

    // Task: WebSocket -> PTY
    let ws_to_pty = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if !handle_ws_message(msg, &session_for_receiver).await {
                break;
            }
        }
    });

    tokio::select! {
        _ = pty_to_ws => {},
        _ = ws_to_pty => {},
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
            // Binary from client = raw terminal input
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
        ClientMessage::Ping => true, // Just keep connection alive
    }
}
