//! Capsule Runtime Server
//!
//! Main entry point for the Capsule runtime daemon.

use axum::{
    routing::get,
    Router,
};
use capsule_runtime::{session::SessionManager, websocket::ws_handler};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "capsule_runtime=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Capsule Runtime");

    // Create session manager
    let sessions = Arc::new(SessionManager::new());

    // CORS for local development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_check))
        .layer(cors)
        .with_state(sessions);

    // Bind and serve
    let bind_addr = "0.0.0.0:3001";
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    info!("Runtime listening on http://{}", bind_addr);
    info!("WebSocket endpoint: ws://{}/ws", bind_addr);

    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "ok"
}
