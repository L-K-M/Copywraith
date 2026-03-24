mod api;
mod search;
mod storage;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use storage::Storage;

/// Fallback admin HTML embedded in the binary, used when ui/dist is not found.
const FALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><title>Copywraith Admin</title></head>
<body style="font-family:sans-serif;text-align:center;padding:40px">
<h2>Copywraith Server</h2>
<p>The admin UI has not been built yet.</p>
<p>Run <code>cd server/ui && npm install && npm run build</code> to build it.</p>
<p><a href="/api/health">API Health Check</a></p>
</body>
</html>"#;

pub struct AppState {
    pub storage: Storage,
    pub admin_api_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminConfigResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "copywraith_server=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let data_dir = std::env::var("COPYWRAITH_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"));

    std::fs::create_dir_all(&data_dir)?;

    let storage = Storage::new(&data_dir)?;
    let admin_api_key = std::env::var("COPYWRAITH_ADMIN_API_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if admin_api_key.is_some() {
        tracing::info!("Admin API key loaded from environment");
    }

    let state = Arc::new(AppState {
        storage,
        admin_api_key,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Resolve UI dist directory: check env var, then common relative paths
    let ui_dir = resolve_ui_dir();

    let app = if let Some(ref dist_path) = ui_dir {
        tracing::info!("Serving admin UI from {}", dist_path.display());
        let index_file = dist_path.join("index.html");
        Router::new()
            .nest("/api", api::router())
            .route("/admin-config", get(admin_config))
            .fallback_service(
                ServeDir::new(dist_path).fallback(ServeFile::new(index_file)),
            )
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(state)
    } else {
        tracing::warn!("Admin UI dist directory not found; serving fallback page");
        Router::new()
            .route("/", get(fallback_ui))
            .nest("/api", api::router())
            .route("/admin-config", get(admin_config))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(state)
    };

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3742);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Copywraith server listening on {}", addr);
    tracing::info!("Admin UI available at http://localhost:{}/", port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Try to find the built UI dist directory.
/// Checks: $COPYWRAITH_UI_DIR, then ./server/ui/dist, then ./ui/dist.
fn resolve_ui_dir() -> Option<PathBuf> {
    // Explicit env override
    if let Ok(dir) = std::env::var("COPYWRAITH_UI_DIR") {
        let p = PathBuf::from(dir);
        if p.join("index.html").exists() {
            return Some(p);
        }
    }

    // Running from repo root: cargo run -p copywraith-server
    let candidates = ["server/ui/dist", "ui/dist"];
    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.join("index.html").exists() {
            return Some(p);
        }
    }

    None
}

/// Fallback when the UI dist hasn't been built
async fn fallback_ui() -> axum::response::Html<&'static str> {
    axum::response::Html(FALLBACK_HTML)
}

async fn admin_config(State(state): State<Arc<AppState>>) -> Json<AdminConfigResponse> {
    Json(AdminConfigResponse {
        api_key: state.admin_api_key.clone(),
    })
}
