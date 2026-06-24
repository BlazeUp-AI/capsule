//! Capsule Runtime - Session management

use crate::docker::{ContainerConfig, ContainerManager, DockerError, ExecResult};
use crate::pty::{spawn_docker_pty_with_env, PtyCommand, PtyError};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

const RECONNECT_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(90);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60); // 30 minutes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Disconnected { since: Instant },
    Terminated,
}

#[derive(Debug)]
pub enum SessionKind {
    Interactive {
        pty_cmd_tx: mpsc::Sender<PtyCommand>,
        pty_output_rx: Option<mpsc::Receiver<Vec<u8>>>,
    },
    Headless,
}

pub struct Session {
    pub id: String,
    pub state: SessionState,
    pub container_id: String,
    pub kind: SessionKind,
    pub last_activity: Instant,
    pub last_pty_activity: Instant,
    pub created_at: Instant,
}

impl Session {
    pub fn take_output_rx(&mut self) -> Option<mpsc::Receiver<Vec<u8>>> {
        match &mut self.kind {
            SessionKind::Interactive { pty_output_rx, .. } => pty_output_rx.take(),
            SessionKind::Headless => None,
        }
    }

    pub fn put_output_rx(&mut self, rx: mpsc::Receiver<Vec<u8>>) {
        if let SessionKind::Interactive { pty_output_rx, .. } = &mut self.kind {
            *pty_output_rx = Some(rx);
        }
    }

    pub async fn write(&self, data: &[u8]) -> Result<(), mpsc::error::SendError<PtyCommand>> {
        match &self.kind {
            SessionKind::Interactive { pty_cmd_tx, .. } => {
                pty_cmd_tx.send(PtyCommand::Write(data.to_vec())).await
            }
            SessionKind::Headless => Ok(()),
        }
    }

    pub async fn resize(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(), mpsc::error::SendError<PtyCommand>> {
        match &self.kind {
            SessionKind::Interactive { pty_cmd_tx, .. } => {
                pty_cmd_tx.send(PtyCommand::Resize { cols, rows }).await
            }
            SessionKind::Headless => Ok(()),
        }
    }

    pub async fn shutdown(&self) {
        if let SessionKind::Interactive { pty_cmd_tx, .. } = &self.kind {
            let _ = pty_cmd_tx.send(PtyCommand::Shutdown).await;
        }
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn touch_pty(&mut self) {
        self.last_pty_activity = Instant::now();
        self.last_activity = Instant::now();
    }

    pub fn is_stale(&self) -> bool {
        matches!(self.state, SessionState::Active)
            && self.last_activity.elapsed() > HEARTBEAT_TIMEOUT
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, SessionState::Active)
            && self.last_pty_activity.elapsed() > IDLE_TIMEOUT
    }

    pub fn is_reconnectable(&self) -> bool {
        match self.state {
            SessionState::Disconnected { since } => since.elapsed() < RECONNECT_TIMEOUT,
            _ => false,
        }
    }

    pub fn is_interactive(&self) -> bool {
        matches!(self.kind, SessionKind::Interactive { .. })
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
    #[error("Operation not supported for headless session")]
    NotInteractive,
}

pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<RwLock<Session>>>>,
    container_manager: ContainerManager,
    warm_pool: Mutex<Vec<String>>, // pre-started container IDs
}

impl SessionManager {
    pub fn new() -> Result<Self, DockerError> {
        let container_manager = ContainerManager::new()?;
        Ok(Self {
            sessions: RwLock::new(HashMap::new()),
            container_manager,
            warm_pool: Mutex::new(Vec::new()),
        })
    }

    /// Pre-warm a container for fast session assignment
    pub async fn prewarm_one(&self) {
        let config = ContainerConfig::default();
        let id = Uuid::new_v4().to_string();
        match self.container_manager.create_container(&id, &config).await {
            Ok(container_id) => {
                self.warm_pool.lock().await.push(container_id.clone());
                info!(container_id = %container_id, "Pre-warmed container ready");
            }
            Err(e) => {
                warn!(error = %e, "Failed to pre-warm container");
            }
        }
    }

    /// Take a pre-warmed container if available
    async fn take_warm_container(&self) -> Option<String> {
        self.warm_pool.lock().await.pop()
    }

    pub async fn create_session(
        &self,
        cols: u16,
        rows: u16,
        config: ContainerConfig,
    ) -> Result<Arc<RwLock<Session>>, SessionError> {
        let id = Uuid::new_v4().to_string();
        let now = Instant::now();

        // Try to use a pre-warmed container, otherwise create fresh
        let container_id = if config.image.is_none() && !config.enable_dind {
            if let Some(warm_id) = self.take_warm_container().await {
                info!(container_id = %warm_id, "Assigned pre-warmed container");
                warm_id
            } else {
                self.container_manager.create_container(&id, &config).await?
            }
        } else {
            self.container_manager.create_container(&id, &config).await?
        };

        // Seed repo if requested (non-fatal)
        if let Some(repo) = config.env_vars.get("CAPSULE_REPO").cloned() {
            self.seed_repo(&container_id, &repo).await;
        }

        // Pass user env vars via docker exec -e flags
        let env_pairs: Vec<(String, String)> = config.env_vars.into_iter().collect();
        let (pty_cmd_tx, pty_output_rx) =
            spawn_docker_pty_with_env(container_id.clone(), cols, rows, &env_pairs)?;

        let session = Session {
            id: id.clone(),
            state: SessionState::Active,
            container_id,
            kind: SessionKind::Interactive {
                pty_cmd_tx,
                pty_output_rx: Some(pty_output_rx),
            },
            last_activity: now,
            last_pty_activity: now,
            created_at: now,
        };

        info!(session_id = %id, "Interactive session created");

        let session = Arc::new(RwLock::new(session));
        self.sessions
            .write()
            .await
            .insert(id.clone(), Arc::clone(&session));

        Ok(session)
    }

    pub async fn create_headless_session(
        &self,
        config: ContainerConfig,
    ) -> Result<Arc<RwLock<Session>>, SessionError> {
        let id = Uuid::new_v4().to_string();
        let now = Instant::now();

        let container_id = self.container_manager.create_container(&id, &config).await?;

        // Seed repo if requested (non-fatal)
        if let Some(repo) = config.env_vars.get("CAPSULE_REPO") {
            self.seed_repo(&container_id, repo).await;
        }

        let session = Session {
            id: id.clone(),
            state: SessionState::Active,
            container_id,
            kind: SessionKind::Headless,
            last_activity: now,
            last_pty_activity: now,
            created_at: now,
        };

        info!(session_id = %id, "Headless session created");

        let session = Arc::new(RwLock::new(session));
        self.sessions
            .write()
            .await
            .insert(id.clone(), Arc::clone(&session));

        Ok(session)
    }

    async fn seed_repo(&self, container_id: &str, repo_url: &str) {
        info!(repo = %repo_url, "Cloning repo into workspace");
        let result = self
            .container_manager
            .exec_command(
                container_id,
                vec![
                    "git".to_string(),
                    "clone".to_string(),
                    repo_url.to_string(),
                    ".".to_string(),
                ],
                Duration::from_secs(120),
            )
            .await;

        match result {
            Ok(r) if r.exit_code == 0 => info!("Repo cloned successfully"),
            Ok(r) => warn!(exit_code = r.exit_code, stderr = %r.stderr, "Repo clone failed"),
            Err(e) => warn!(error = %e, "Repo clone error"),
        }
    }

    pub async fn reconnect_session(&self, id: &str) -> Result<Arc<RwLock<Session>>, SessionError> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(id).ok_or(SessionError::NotFound)?;

        let mut guard = session.write().await;

        if !guard.is_reconnectable() {
            return Err(SessionError::NotReconnectable);
        }

        match &guard.kind {
            SessionKind::Interactive { pty_output_rx, .. } => {
                if pty_output_rx.is_none() {
                    return Err(SessionError::AlreadyConnected);
                }
            }
            SessionKind::Headless => return Err(SessionError::NotInteractive),
        }

        guard.state = SessionState::Active;
        guard.touch();
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
            guard.state = SessionState::Disconnected {
                since: Instant::now(),
            };
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

        if let Err(e) = self
            .container_manager
            .remove_container(&session_guard.container_id)
            .await
        {
            error!(session_id = %id, error = %e, "Failed to remove container");
        }

        info!(session_id = %id, "Session removed");
    }

    /// Execute a command in a session's container
    pub async fn exec_in_session(
        &self,
        id: &str,
        command: Vec<String>,
        timeout: Duration,
    ) -> Result<ExecResult, SessionError> {
        let session = self
            .sessions
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or(SessionError::NotFound)?;

        let container_id = session.read().await.container_id.clone();
        let result = self
            .container_manager
            .exec_command(&container_id, command, timeout)
            .await?;
        Ok(result)
    }

    /// Clean up sessions that are idle, expired, or stale
    pub async fn cleanup_expired_sessions(&self) {
        let session_ids: Vec<String> = { self.sessions.read().await.keys().cloned().collect() };

        let mut stale_ids = Vec::new();
        let mut expired_ids = Vec::new();
        let mut idle_ids = Vec::new();

        for id in session_ids {
            let Some(session) = self.sessions.read().await.get(&id).cloned() else {
                continue;
            };

            let guard = session.read().await;

            if guard.is_idle() {
                idle_ids.push(id);
            } else if guard.is_stale() {
                stale_ids.push(id);
            } else if let SessionState::Disconnected { since } = guard.state {
                if since.elapsed() >= RECONNECT_TIMEOUT {
                    expired_ids.push(id);
                }
            }
        }

        for id in stale_ids {
            let Some(session) = self.sessions.read().await.get(&id).cloned() else {
                continue;
            };
            let mut guard = session.write().await;
            if guard.is_stale() {
                info!(session_id = %id, "Marking stale session as disconnected");
                guard.state = SessionState::Disconnected {
                    since: Instant::now(),
                };
            }
        }

        for id in idle_ids {
            info!(session_id = %id, "Removing idle session (30 min no I/O)");
            self.remove_session(&id).await;
        }

        for id in expired_ids {
            info!(session_id = %id, "Removing expired session");
            self.remove_session(&id).await;
        }
    }

    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }

    pub async fn export_workspace(&self, session_id: &str) -> Result<Vec<u8>, SessionError> {
        let session = self
            .sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or(SessionError::NotFound)?;

        let container_id = session.read().await.container_id.clone();
        let data = self.container_manager.export_workspace(&container_id).await?;
        Ok(data)
    }

    pub fn container_manager(&self) -> &ContainerManager {
        &self.container_manager
    }
}
