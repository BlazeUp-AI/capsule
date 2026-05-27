//! Capsule Runtime - Session management

use crate::docker::{ContainerManager, DockerError};
use crate::pty::{spawn_docker_pty, PtyCommand, PtyError};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

const RECONNECT_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Disconnected { since: Instant },
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

    pub fn put_output_rx(&mut self, rx: mpsc::Receiver<Vec<u8>>) {
        self.pty_output_rx = Some(rx);
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

    pub fn is_reconnectable(&self) -> bool {
        match self.state {
            SessionState::Disconnected { since } => since.elapsed() < RECONNECT_TIMEOUT,
            _ => false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Docker error: {0}")]
    Docker(#[from] DockerError),
    #[error("PTY error: {0}")]
    Pty(#[from] PtyError),
    #[error("Session not found")]
    NotFound,
    #[error("Session not reconnectable")]
    NotReconnectable,
    #[error("Session already has active connection")]
    AlreadyConnected,
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

        let container_id = self.container_manager.create_container(&id).await?;
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

        Ok(session)
    }

    pub async fn reconnect_session(&self, id: &str) -> Result<Arc<RwLock<Session>>, SessionError> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(id).ok_or(SessionError::NotFound)?;
        
        let mut guard = session.write().await;
        
        if !guard.is_reconnectable() {
            return Err(SessionError::NotReconnectable);
        }
        
        if guard.pty_output_rx.is_none() {
            // Output receiver is still held by another connection
            return Err(SessionError::AlreadyConnected);
        }
        
        guard.state = SessionState::Active;
        info!(session_id = %id, "Session reconnected");
        
        drop(guard);
        Ok(Arc::clone(session))
    }

    pub async fn get_session(&self, id: &str) -> Option<Arc<RwLock<Session>>> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn mark_disconnected(&self, id: &str, output_rx: mpsc::Receiver<Vec<u8>>) {
        if let Some(session) = self.sessions.read().await.get(id) {
            let mut guard = session.write().await;
            guard.state = SessionState::Disconnected { since: Instant::now() };
            guard.put_output_rx(output_rx);
            info!(session_id = %id, "Session marked disconnected (reconnectable for 10 min)");
        }
    }

    pub async fn remove_session(&self, id: &str) {
        let Some(session) = self.sessions.write().await.remove(id) else {
            warn!(session_id = %id, "Attempted to remove non-existent session");
            return;
        };

        let session_guard = session.read().await;
        session_guard.shutdown().await;

        if let Err(e) = self.container_manager.remove_container(&session_guard.container_id).await {
            error!(session_id = %id, error = %e, "Failed to remove container");
        }

        info!(session_id = %id, "Session removed");
    }

    /// Clean up sessions that have been disconnected for too long
    pub async fn cleanup_expired_sessions(&self) {
        let expired: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions
                .iter()
                .filter_map(|(id, session)| {
                    let guard = session.blocking_read();
                    match guard.state {
                        SessionState::Disconnected { since } if since.elapsed() >= RECONNECT_TIMEOUT => {
                            Some(id.clone())
                        }
                        _ => None,
                    }
                })
                .collect()
        };

        for id in expired {
            info!(session_id = %id, "Cleaning up expired session");
            self.remove_session(&id).await;
        }
    }

    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }
}
