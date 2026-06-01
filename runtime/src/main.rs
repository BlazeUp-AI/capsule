//! Capsule Runtime Server

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Router,
};
use capsule_runtime::{api, observal::ObservalClient, session::SessionManager, websocket::ws_handler};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

type AppState = (Arc<SessionManager>, Option<String>, Arc<RwLock<Option<ObservalClient>>>);

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

    // Initialize Observal client (optional — runs without it)
    let observal: Arc<RwLock<Option<ObservalClient>>> = Arc::new(RwLock::new(None));
    if let Some(mut client) = ObservalClient::from_env() {
        info!("Observal integration enabled ({})", client.api_url());
        let observal_clone = Arc::clone(&observal);
        tokio::spawn(async move {
            // Wait for Observal to be ready (it boots slower than us)
            for attempt in 1..=30 {
                if client.is_healthy().await {
                    break;
                }
                if attempt == 30 {
                    tracing::warn!("Observal not reachable after 30 attempts, continuing without it");
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            match client.authenticate_admin().await {
                Ok(()) => {
                    info!("Observal admin authenticated, integration active");
                    *observal_clone.write().await = Some(client);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to authenticate with Observal");
                }
            }
        });
    } else {
        info!("Observal integration disabled (OBSERVAL_API_URL not set)");
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

    let state: AppState = (Arc::clone(&sessions), api_token, Arc::clone(&observal));

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
        // File API
        .route("/api/sessions/{session_id}/files", get(api::list_files))
        .route(
            "/api/sessions/{session_id}/files/content",
            get(api::read_file),
        )
        .route(
            "/api/sessions/{session_id}/files/content",
            put(api::write_file),
        )
        // Observal dashboard proxy
        .route("/observal/{session_id}/{*path}", get(observal_proxy))
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
    State((sessions, _, _)): State<AppState>,
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

async fn observal_proxy(
    State((sessions, _, observal)): State<AppState>,
    Path((session_id, path)): Path<(String, String)>,
    req: Request<Body>,
) -> impl IntoResponse {
    // Validate session exists
    if sessions.get_session(&session_id).await.is_none() {
        return (StatusCode::NOT_FOUND, "Session not found").into_response();
    }

    let observal_web_url =
        std::env::var("OBSERVAL_WEB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

    // Determine if this is an insights request needing elevation
    let is_insights = path.starts_with("api/v1/insights");

    let target_url = format!("{}/{}", observal_web_url, path);

    let client = reqwest::Client::new();
    let mut proxy_req = client.request(req.method().clone(), &target_url);

    // Forward relevant headers
    for (name, value) in req.headers() {
        if name == header::HOST || name == header::AUTHORIZATION {
            continue;
        }
        if let Ok(v) = value.to_str() {
            proxy_req = proxy_req.header(name.as_str(), v);
        }
    }

    // Set auth: use admin token for insights, pass through user token otherwise
    if is_insights {
        let guard = observal.read().await;
        if let Some(client_ref) = guard.as_ref() {
            if let Some(admin_token) = client_ref.admin_token_value() {
                proxy_req = proxy_req.bearer_auth(admin_token);
            }
        }
    } else if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(v) = auth.to_str() {
            proxy_req = proxy_req.header("Authorization", v);
        }
    }

    // Forward body
    let body_bytes = axum::body::to_bytes(req.into_body(), 10_485_760)
        .await
        .unwrap_or_default();
    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes);
    }

    match proxy_req.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let headers = resp.headers().clone();
            let body = resp.bytes().await.unwrap_or_default();

            let mut response = (status, body.to_vec()).into_response();
            for (name, value) in headers.iter() {
                if name == header::TRANSFER_ENCODING || name == header::CONNECTION {
                    continue;
                }
                response.headers_mut().insert(name.clone(), value.clone());
            }
            response
        }
        Err(e) => {
            (StatusCode::BAD_GATEWAY, format!("Observal proxy error: {}", e)).into_response()
        }
    }
}
