//! Capsule Runtime Library
//!
//! Core runtime components for the Capsule ephemeral coding environment.

pub mod pty;
pub mod session;
pub mod websocket;

pub use pty::{spawn_pty, PtyCommand, PtyError};
pub use session::{Session, SessionManager, SessionState};
pub use websocket::{ClientMessage, ServerMessage};
