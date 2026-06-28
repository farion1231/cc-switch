pub mod handlers;
pub mod middleware;
pub mod models;
pub mod routes;

use axum::{http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use handlers::ws::WsState;
use models::app_state::AppState;

/// Get the path to the web assets directory.
///
/// Tries, in order:
/// 1. The `TAURI_RESOURCE_DIR/web-dist` path used by bundled Tauri apps.
/// 2. `current_dir/web-dist` (convenient when running from `src-tauri`).
/// 3. `current_dir/src-tauri/web-dist` (convenient when running from the repo root).
/// 4. The `web-dist` directory next to the executable (`src-tauri/web-dist`
///    for debug builds in the standard layout).
/// 5. `../dist` for Vite development builds.
fn get_web_assets_path() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(resource_dir) = std::env::var("TAURI_RESOURCE_DIR") {
        candidates.push(PathBuf::from(resource_dir).join("web-dist"));
    }

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("web-dist"));
        candidates.push(current_dir.join("src-tauri").join("web-dist"));
    }

    if let Ok(exe_path) = std::env::current_exe() {
        // exe: .../src-tauri/target/{debug,release}/cc-switch
        // web-dist sibling: .../src-tauri/web-dist
        if let Some(exe_dir) = exe_path.parent() {
            if let Some(target_dir) = exe_dir.parent() {
                if let Some(src_tauri_dir) = target_dir.parent() {
                    candidates.push(src_tauri_dir.join("web-dist"));
                }
            }
        }
    }

    candidates.push(PathBuf::from("../dist"));

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    // Last resort: return a deterministic path so the failure mode is obvious.
    candidates.last().cloned().unwrap_or_else(|| PathBuf::from("web-dist"))
}

pub fn create_router(state: Arc<AppState>, ws_state: Arc<WsState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any) // Required for local/remote browser access
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_credentials(false);

    let shared_state = (state, ws_state);

    let protected_routes = Router::new()
        .nest("/providers", routes::providers::routes())
        .nest("/settings", routes::settings::routes())
        .nest("/mcp", routes::mcp::routes())
        .nest("/prompts", routes::prompts::routes())
        .nest("/skills", routes::skills::routes())
        .nest("/sessions", routes::sessions::routes())
        .nest("/proxy", routes::proxy::routes())
        .nest("/hermes", routes::hermes::routes())
        .nest("/openclaw", routes::openclaw::routes())
        .nest("/workspace", routes::workspace::routes())
        .layer(axum::middleware::from_fn(middleware::auth_middleware))
        .with_state(shared_state.clone());
    let api_routes = Router::new()
        .nest("/auth", routes::auth::routes())
        .nest("/logs", routes::logs::routes())
        .merge(protected_routes);
    let ws_route = Router::new()
        .route("/ws", get(handlers::ws::ws_handler))
        .route("/ws/terminal", get(handlers::terminal::terminal_ws_handler))
        .with_state(shared_state.clone());
    // Get the web assets path
    let assets_path = get_web_assets_path();
    let index_path = assets_path.join("index.html");

    tracing::info!(
        "serving web assets from {:?} (index: {:?}, exists: {})",
        assets_path,
        index_path,
        index_path.exists()
    );

    Router::new()
        .nest("/api/v1", api_routes)
        .route("/health", get(health_check))
        .merge(ws_route)
        .nest_service("/assets", ServeDir::new(assets_path.join("assets")))
        .fallback_service(ServeFile::new(index_path))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

pub fn create_app(db_path: &str) -> Result<Router, rusqlite::Error> {
    let state = Arc::new(AppState::new(db_path)?);
    let (tx, _rx) = broadcast::channel(100);
    let ws_state = Arc::new(WsState::new(tx));
    Ok(create_router(state, ws_state))
}

async fn health_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}
