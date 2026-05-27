//! Capsule Runtime Server

use axum::{extract::{Path, State}, http::{header, StatusCode}, response::IntoResponse, routing::get, Router};
use capsule_runtime::{session::SessionManager, websocket::ws_handler};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "capsule_runtime=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Capsule Runtime");

    let sessions = match SessionManager::new() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("Failed to initialize session manager: {}", e);
            tracing::error!("Is Docker running?");
            std::process::exit(1);
        }
    };

    // Spawn cleanup task
    let sessions_for_cleanup = Arc::clone(&sessions);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            sessions_for_cleanup.cleanup_expired_sessions().await;
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_check))
        .route("/api/export/{session_id}", get(export_workspace))
        .layer(cors)
        .with_state(sessions);

    let bind_addr = "0.0.0.0:3001";
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    info!("Runtime listening on http://{}", bind_addr);
    info!("WebSocket endpoint: ws://{}/ws", bind_addr);

    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "ok"
}

async fn export_workspace(
    State(sessions): State<Arc<SessionManager>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match sessions.export_workspace(&session_id).await {
        Ok(data) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/x-tar"),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"workspace.tar\""),
            ],
            data,
        ).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            format!("Export failed: {}", e),
        ).into_response(),
    }
}
