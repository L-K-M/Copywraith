mod api;
mod search;
mod storage;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::response::Html;
use axum::routing::get;
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use storage::Storage;

/// Embedded admin UI (compiled into the binary)
const ADMIN_HTML: &str = include_str!("admin.html");

pub struct AppState {
    pub storage: Storage,
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
    let state = Arc::new(AppState { storage });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(admin_ui))
        .nest("/api", api::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

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

/// Serve the embedded admin UI
async fn admin_ui() -> Html<&'static str> {
    Html(ADMIN_HTML)
}
