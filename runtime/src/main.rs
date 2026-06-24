//! Capsule Runtime Server

use axum::{
    Router,
    body::Body,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use capsule_runtime::{
    api, free_keys::FreeKeyPool, observal::ObservalClient, session::SessionManager,
    websocket::ws_handler,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

type AppState = (
    Arc<SessionManager>,
    Option<String>,
    Arc<RwLock<Option<ObservalClient>>>,
    Arc<FreeKeyPool>,
);

#[tokio::main]
async fn main() {
    // Load .env from project root (sibling to runtime/)
    let _ = dotenvy::from_filename("../.env").or_else(|_| dotenvy::dotenv());

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

    // Provider key pool (must be created before Observal init to configure insights)
    let free_keys = Arc::new(FreeKeyPool::from_env());
    if free_keys.is_empty() {
        info!(
            "Provider key pool empty (set CAPSULE_FREE_KEYS_OPENROUTER, OPENROUTER_API_KEY, ANTHROPIC_API_KEY, or CAPSULE_ANTHROPIC_API_KEY)"
        );
    }

    // Initialize Observal client (optional — runs without it)
    let observal: Arc<RwLock<Option<ObservalClient>>> = Arc::new(RwLock::new(None));
    if let Some(mut client) = ObservalClient::from_env() {
        info!("Observal integration enabled ({})", client.api_url());
        let observal_clone = Arc::clone(&observal);
        let free_keys_for_observal = Arc::clone(&free_keys);
        tokio::spawn(async move {
            // Wait for Observal to be ready (it boots slower than us)
            for attempt in 1..=30 {
                if client.is_healthy().await {
                    break;
                }
                if attempt == 30 {
                    tracing::warn!(
                        "Observal not reachable after 30 attempts, continuing without it"
                    );
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            match client.authenticate_admin().await {
                Ok(()) => {
                    info!("Observal admin authenticated, integration active");
                    client.configure_insights(&free_keys_for_observal).await;
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

    let state: AppState = (
        Arc::clone(&sessions),
        api_token,
        Arc::clone(&observal),
        free_keys,
    );

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
        .route("/observal/{session_id}/", get(observal_proxy_root))
        .route("/observal/{session_id}/{*path}", get(observal_proxy))
        // Observal API proxy (Web UI makes calls to /api/v1/*)
        .route(
            "/api/v1/{*path}",
            get(observal_api_proxy).post(observal_api_proxy),
        )
        // Observal static assets (referenced by the Web UI HTML with absolute paths)
        .route("/assets/{*path}", get(observal_assets))
        .route("/fonts/{*path}", get(observal_assets_fonts))
        .route("/observal-logo.svg", get(observal_root_asset))
        .route("/favicon.ico", get(observal_root_asset_favicon))
        .layer(cors)
        .with_state(state);

    let bind_addr = std::env::var("CAPSULE_BIND").unwrap_or_else(|_| "0.0.0.0:3001".into());
    let bind_addr = bind_addr.as_str();
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    info!("Runtime listening on http://{}", bind_addr);
    info!("WebSocket endpoint: ws://{}/ws", bind_addr);

    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "ok"
}

async fn export_workspace(
    State((sessions, _, _, _)): State<AppState>,
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

async fn observal_proxy_root(
    State((sessions, _, observal, _)): State<AppState>,
    Path(session_id): Path<String>,
    req: Request<Body>,
) -> impl IntoResponse {
    observal_proxy_handler(sessions, observal, session_id, String::new(), req).await
}

async fn observal_proxy(
    State((sessions, _, observal, _)): State<AppState>,
    Path((session_id, path)): Path<(String, String)>,
    req: Request<Body>,
) -> impl IntoResponse {
    observal_proxy_handler(sessions, observal, session_id, path, req).await
}

async fn observal_proxy_handler(
    sessions: Arc<SessionManager>,
    observal: Arc<RwLock<Option<ObservalClient>>>,
    session_id: String,
    path: String,
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

    let is_html_page = path.is_empty()
        || path == "traces"
        || path == "sessions"
        || path == "agents"
        || path == "login";

    let target_url = if path.is_empty() {
        format!("{}/", observal_web_url)
    } else {
        format!("{}/{}", observal_web_url, path)
    };

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
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let headers = resp.headers().clone();
            let body_bytes = resp.bytes().await.unwrap_or_default();

            // For HTML pages, inject scripts to:
            // 1. Rewrite URL so React Router sees "/"
            // 2. Copy auth tokens from parent window (same origin) into iframe storage
            // 3. Decode JWT to extract role and set user metadata in localStorage
            let final_body = if is_html_page {
                let html = String::from_utf8_lossy(&body_bytes);
                let inject = r#"<script>
window.history.replaceState(null, "", "/");
try {
  var t = window.parent.sessionStorage.getItem("observal_access_token");
  var r = window.parent.sessionStorage.getItem("observal_refresh_token");
  if (t) {
    sessionStorage.setItem("observal_access_token", t);
    localStorage.setItem("observal_access_token", t);
    try {
      var p = JSON.parse(atob(t.split(".")[1]));
      if (p.role) localStorage.setItem("observal_user_role", p.role);
      if (p.sub) localStorage.setItem("observal_user_email", p.sub);
    } catch(e2) {}
  }
  if (r) {
    sessionStorage.setItem("observal_refresh_token", r);
    localStorage.setItem("observal_refresh_token", r);
  }
} catch(e) {}
</script>"#;
                let modified = html.replacen("<head>", &format!("<head>{}", inject), 1);
                modified.into_bytes()
            } else {
                body_bytes.to_vec()
            };

            let mut response = (status, final_body).into_response();
            for (name, value) in headers.iter() {
                if name == header::TRANSFER_ENCODING
                    || name == header::CONNECTION
                    || name == "content-security-policy"
                    || name == "x-frame-options"
                    || name == header::CONTENT_LENGTH
                {
                    continue;
                }
                response.headers_mut().insert(name.clone(), value.clone());
            }
            response
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("Observal proxy error: {}", e),
        )
            .into_response(),
    }
}

async fn observal_api_proxy(
    Path(path): Path<String>,
    req: Request<Body>,
) -> axum::response::Response {
    let observal_api_url =
        std::env::var("OBSERVAL_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let target_url = format!("{}/api/v1/{}", observal_api_url, path);

    let client = reqwest::Client::new();
    let mut proxy_req = client.request(req.method().clone(), &target_url);

    for (name, value) in req.headers() {
        if name == header::HOST {
            continue;
        }
        if let Ok(v) = value.to_str() {
            proxy_req = proxy_req.header(name.as_str(), v);
        }
    }

    let body_bytes = axum::body::to_bytes(req.into_body(), 10_485_760)
        .await
        .unwrap_or_default();
    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes);
    }

    match proxy_req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
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
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("Observal API proxy error: {}", e),
        )
            .into_response(),
    }
}

async fn observal_assets(Path(path): Path<String>) -> impl IntoResponse {
    proxy_static_to_observal(format!("/assets/{}", path)).await
}

async fn observal_assets_fonts(Path(path): Path<String>) -> impl IntoResponse {
    proxy_static_to_observal(format!("/fonts/{}", path)).await
}

async fn observal_root_asset() -> impl IntoResponse {
    proxy_static_to_observal("/observal-logo.svg".to_string()).await
}

async fn observal_root_asset_favicon() -> impl IntoResponse {
    proxy_static_to_observal("/favicon.ico".to_string()).await
}

async fn proxy_static_to_observal(path: String) -> axum::response::Response {
    let observal_web_url =
        std::env::var("OBSERVAL_WEB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
    let target_url = format!("{}{}", observal_web_url, path);

    let client = reqwest::Client::new();
    match client.get(&target_url).send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
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
        Err(e) => (StatusCode::BAD_GATEWAY, format!("Asset proxy error: {}", e)).into_response(),
    }
}
