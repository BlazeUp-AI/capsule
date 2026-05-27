//! Capsule Runtime - Session management

use crate::docker::{ContainerManager, DockerError};
use crate::pty::{spawn_docker_pty, PtyCommand, PtyError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Disconnected,
    Terminated,
}

pub struct Session {
    pub id: String,
    pub state: SessionState,
    pub container_id: String,
    pub pty_cmd_tx: mpsc::Sender<PtyCommand>,
    pub pty_output_rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl Session {
    pub fn take_output_rx(&mut self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.pty_output_rx.take()
    }

    pub async fn write(&self, data: &[u8]) -> Result<(), mpsc::error::SendError<PtyCommand>> {
        self.pty_cmd_tx.send(PtyCommand::Write(data.to_vec())).await
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), mpsc::error::SendError<PtyCommand>> {
        self.pty_cmd_tx.send(PtyCommand::Resize { cols, rows }).await
    }

    pub async fn shutdown(&self) {
        let _ = self.pty_cmd_tx.send(PtyCommand::Shutdown).await;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Docker error: {0}")]
    Docker(#[from] DockerError),
    #[error("PTY error: {0}")]
    Pty(#[from] PtyError),
}

pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<RwLock<Session>>>>,
    container_manager: ContainerManager,
}

impl SessionManager {
    pub fn new() -> Result<Self, DockerError> {
        let container_manager = ContainerManager::new()?;
        Ok(Self {
            sessions: RwLock::new(HashMap::new()),
            container_manager,
        })
    }

    pub async fn create_session(&self, cols: u16, rows: u16) -> Result<Arc<RwLock<Session>>, SessionError> {
        let id = Uuid::new_v4().to_string();

        // Create Docker container
        let container_id = self.container_manager.create_container(&id).await?;

        // Spawn PTY connected to container
        let (pty_cmd_tx, pty_output_rx) = spawn_docker_pty(container_id.clone(), cols, rows)?;

        let session = Session {
            id: id.clone(),
            state: SessionState::Active,
            container_id,
            pty_cmd_tx,
            pty_output_rx: Some(pty_output_rx),
        };

        info!(session_id = %id, "Session created");

        let session = Arc::new(RwLock::new(session));
        self.sessions.write().await.insert(id.clone(), Arc::clone(&session));
        info!(session_id = %id, "Session registered");

        Ok(session)
    }

    pub async fn get_session(&self, id: &str) -> Option<Arc<RwLock<Session>>> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn remove_session(&self, id: &str) {
        let Some(session) = self.sessions.write().await.remove(id) else {
            warn!(session_id = %id, "Attempted to remove non-existent session");
            return;
        };

        let session_guard = session.read().await;
        session_guard.shutdown().await;

        // Remove the container
        if let Err(e) = self.container_manager.remove_container(&session_guard.container_id).await {
            error!(session_id = %id, error = %e, "Failed to remove container");
        }

        info!(session_id = %id, "Session removed");
    }

    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }
}
