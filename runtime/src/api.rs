//! Capsule Runtime - REST API handlers

use crate::docker::ContainerConfig;
use crate::session::SessionManager;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const MAX_EXEC_TIMEOUT: u64 = 300; // 5 minutes
const DEFAULT_EXEC_TIMEOUT: u64 = 60;
const MAX_OUTPUT_BYTES: usize = 1_048_576; // 1MB

// ── Request/Response types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub credentials: Option<HashMap<String, String>>,
    pub agent: Option<String>,
    pub image: Option<String>,
    pub enable_dind: Option<bool>,
    pub repo: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct ExecRequest {
    pub command: Vec<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ExecResponse {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize)]
pub struct SessionStatusResponse {
    pub session_id: String,
    pub state: String,
    pub kind: String,
    pub uptime_seconds: u64,
}

// ── Auth middleware ──────────────────────────────────────────────────────────

pub fn check_auth(required_token: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let Some(required) = required_token else {
        return Ok(());
    };

    let header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match header {
        Some(t) if t == required => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

pub async fn create_session(
    State((sessions, api_token, _)): State<(Arc<SessionManager>, Option<String>, Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>)>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    if let Err(status) = check_auth(&api_token, &headers) {
        return status.into_response();
    }

    let mut env_vars = req.credentials.unwrap_or_default();

    if let Some(agent) = &req.agent {
        env_vars.insert("CAPSULE_AGENT".to_string(), agent.clone());
    }

    if let Some(repo) = &req.repo {
        env_vars.insert("CAPSULE_REPO".to_string(), repo.clone());
    }

    let config = ContainerConfig {
        image: req.image,
        env_vars,
        enable_dind: req.enable_dind.unwrap_or(false),
    };

    match sessions.create_headless_session(config).await {
        Ok(session) => {
            let guard = session.read().await;
            let resp = CreateSessionResponse {
                session_id: guard.id.clone(),
                state: "active".to_string(),
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn get_session(
    State((sessions, api_token, _)): State<(Arc<SessionManager>, Option<String>, Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>)>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = check_auth(&api_token, &headers) {
        return status.into_response();
    }

    let Some(session) = sessions.get_session(&session_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let guard = session.read().await;
    let state = match guard.state {
        crate::session::SessionState::Active => "active",
        crate::session::SessionState::Disconnected { .. } => "disconnected",
        crate::session::SessionState::Terminated => "terminated",
    };

    let kind = if guard.is_interactive() {
        "interactive"
    } else {
        "headless"
    };

    let resp = SessionStatusResponse {
        session_id: guard.id.clone(),
        state: state.to_string(),
        kind: kind.to_string(),
        uptime_seconds: guard.created_at.elapsed().as_secs(),
    };

    Json(resp).into_response()
}

pub async fn exec_in_session(
    State((sessions, api_token, _)): State<(Arc<SessionManager>, Option<String>, Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>)>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> impl IntoResponse {
    if let Err(status) = check_auth(&api_token, &headers) {
        return status.into_response();
    }

    if req.command.is_empty() {
        return (StatusCode::BAD_REQUEST, "command must not be empty").into_response();
    }

    let timeout_secs = req.timeout.unwrap_or(DEFAULT_EXEC_TIMEOUT).min(MAX_EXEC_TIMEOUT);
    let timeout = Duration::from_secs(timeout_secs);

    match sessions
        .exec_in_session(&session_id, req.command, timeout)
        .await
    {
        Ok(result) => {
            let stdout = truncate_output(result.stdout);
            let stderr = truncate_output(result.stderr);

            let resp = ExecResponse {
                exit_code: result.exit_code,
                stdout,
                stderr,
            };
            Json(resp).into_response()
        }
        Err(crate::session::SessionError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn delete_session(
    State((sessions, api_token, _)): State<(Arc<SessionManager>, Option<String>, Arc<tokio::sync::RwLock<Option<crate::observal::ObservalClient>>>)>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = check_auth(&api_token, &headers) {
        return status.into_response();
    }

    if sessions.get_session(&session_id).await.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }

    sessions.remove_session(&session_id).await;
    StatusCode::NO_CONTENT.into_response()
}

fn truncate_output(s: String) -> String {
    if s.len() > MAX_OUTPUT_BYTES {
        let mut truncated = s[..MAX_OUTPUT_BYTES].to_string();
        truncated.push_str("\n... [output truncated at 1MB]");
        truncated
    } else {
        s
    }
}
