//! Capsule Runtime Library

pub mod api;
pub mod docker;
pub mod pty;
pub mod session;
pub mod websocket;

pub use docker::{ContainerConfig, ContainerManager, DockerError, ExecResult};
pub use pty::{spawn_docker_pty, spawn_docker_pty_with_env, spawn_local_pty, PtyCommand, PtyError};
pub use session::{Session, SessionError, SessionKind, SessionManager, SessionState};
pub use websocket::{ClientMessage, ServerMessage};
