//! Capsule Runtime Server

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use capsule_runtime::{api, session::SessionManager, websocket::ws_handler};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

type AppState = (Arc<SessionManager>, Option<String>);

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

    let api_token: Option<String> = std::env::var("CAPSULE_API_TOKEN").ok();
    if api_token.is_some() {
        info!("API token authentication enabled");
    } else {
        info!("API token authentication disabled (CAPSULE_API_TOKEN not set)");
    }

    // Verify container image and pre-warm one container
    let sessions_for_warmup = Arc::clone(&sessions);
    tokio::spawn(async move {
        info!("Verifying container image availability...");
        let start = std::time::Instant::now();
        match sessions_for_warmup
            .container_manager()
            .docker()
            .inspect_image("capsule-runtime:latest")
            .await
        {
            Ok(img) => {
                let size_mb = img.size.unwrap_or(0) / 1_048_576;
                info!(
                    elapsed_ms = start.elapsed().as_millis(),
                    size_mb, "Container image ready"
                );
                // Pre-warm one container for instant first session
                sessions_for_warmup.prewarm_one().await;
            }
            Err(_) => {
                tracing::warn!(
                    "Container image 'capsule-runtime:latest' not found. \
                     Run: docker build -t capsule-runtime docker/"
                );
            }
        }
    });

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

    let state: AppState = (Arc::clone(&sessions), api_token);

    let app = Router::new()
        // WebSocket (uses SessionManager directly)
        .route("/ws", get(ws_handler))
        // Health
        .route("/health", get(health_check))
        // Export (legacy endpoint)
        .route("/api/export/{session_id}", get(export_workspace))
        // REST API
        .route("/api/sessions", post(api::create_session))
        .route("/api/sessions/{session_id}", get(api::get_session))
        .route("/api/sessions/{session_id}", delete(api::delete_session))
        .route(
            "/api/sessions/{session_id}/exec",
            post(api::exec_in_session),
        )
        .layer(cors)
        .with_state(state);

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
    State((sessions, _)): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match sessions.export_workspace(&session_id).await {
        Ok(data) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/x-tar"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"workspace.tar\"",
                ),
            ],
            data,
        )
            .into_response(),
        Err(e) => (StatusCode::NOT_FOUND, format!("Export failed: {}", e)).into_response(),
    }
}
