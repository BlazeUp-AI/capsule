//! Capsule Runtime - Session management
//!
//! Handles session lifecycle and state.

use crate::pty::{spawn_pty, PtyCommand, PtyError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

/// Session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Disconnected,
    Terminated,
}

/// A runtime session with PTY
pub struct Session {
    pub id: String,
    pub state: SessionState,
    pub pty_cmd_tx: mpsc::Sender<PtyCommand>,
    pub pty_output_rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl Session {
    /// Create a new session with a PTY
    pub fn new(cols: u16, rows: u16) -> Result<Self, PtyError> {
        let id = Uuid::new_v4().to_string();
        let (pty_cmd_tx, pty_output_rx) = spawn_pty(cols, rows)?;

        info!(session_id = %id, "Session created");

        Ok(Self {
            id,
            state: SessionState::Active,
            pty_cmd_tx,
            pty_output_rx: Some(pty_output_rx),
        })
    }

    /// Take the output receiver (can only be done once)
    pub fn take_output_rx(&mut self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.pty_output_rx.take()
    }

    /// Send input to the PTY
    pub async fn write(&self, data: &[u8]) -> Result<(), mpsc::error::SendError<PtyCommand>> {
        self.pty_cmd_tx.send(PtyCommand::Write(data.to_vec())).await
    }

    /// Resize the PTY
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), mpsc::error::SendError<PtyCommand>> {
        self.pty_cmd_tx.send(PtyCommand::Resize { cols, rows }).await
    }

    /// Shutdown the PTY
    pub async fn shutdown(&self) {
        let _ = self.pty_cmd_tx.send(PtyCommand::Shutdown).await;
    }
}

/// Global session manager
#[derive(Default)]
pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<RwLock<Session>>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new session
    pub async fn create_session(&self, cols: u16, rows: u16) -> Result<Arc<RwLock<Session>>, PtyError> {
        let session = Session::new(cols, rows)?;
        let id = session.id.clone();
        let session = Arc::new(RwLock::new(session));

        self.sessions.write().await.insert(id.clone(), Arc::clone(&session));
        info!(session_id = %id, "Session registered");

        Ok(session)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<Arc<RwLock<Session>>> {
        self.sessions.read().await.get(id).cloned()
    }

    /// Remove a session
    pub async fn remove_session(&self, id: &str) {
        let Some(session) = self.sessions.write().await.remove(id) else {
            warn!(session_id = %id, "Attempted to remove non-existent session");
            return;
        };
        session.read().await.shutdown().await;
        info!(session_id = %id, "Session removed");
    }

    /// List all session IDs
    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }
}
