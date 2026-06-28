use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::session_manager;
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

/// Session management routes.
///
/// These mirror the desktop `session_manager` Tauri commands. Sessions are NOT
/// stored in the database — they are scanned from each AI tool's on-disk session
/// logs (e.g. `~/.claude/projects/**.jsonl`, Codex/Gemini/OpenCode/OpenClaw/
/// Hermes stores). In Docker the host home must be mounted (HOST_HOME) so these
/// files are visible. The previous implementation read an always-empty bespoke
/// `sessions` SQL table, which is why session management appeared broken.
pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/", get(list_sessions))
        .route("/messages", get(get_session_messages))
        .route("/delete", post(delete_session))
}

async fn list_sessions(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<session_manager::SessionMeta>>> {
    let sessions = match tokio::task::spawn_blocking(session_manager::scan_sessions).await {
        Ok(s) => s,
        Err(e) => return Json(ApiResponse::error(format!("Failed to scan sessions: {e}"))),
    };
    Json(ApiResponse::success(sessions))
}

#[derive(Deserialize)]
struct MessagesQuery {
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(rename = "sourcePath")]
    source_path: String,
}

async fn get_session_messages(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(q): Query<MessagesQuery>,
) -> Json<ApiResponse<Vec<session_manager::SessionMessage>>> {
    let result = tokio::task::spawn_blocking(move || {
        session_manager::load_messages(&q.provider_id, &q.source_path)
    })
    .await;

    match result {
        Ok(Ok(messages)) => Json(ApiResponse::success(messages)),
        Ok(Err(e)) => Json(ApiResponse::error(e)),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to load session messages: {e}"
        ))),
    }
}

#[derive(Deserialize)]
struct DeleteSessionBody {
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "sourcePath")]
    source_path: String,
}

async fn delete_session(
    State((_state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<DeleteSessionBody>,
) -> Json<ApiResponse<bool>> {
    let DeleteSessionBody {
        provider_id,
        session_id,
        source_path,
    } = body;
    let session_id_for_event = session_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        session_manager::delete_session(&provider_id, &session_id, &source_path)
    })
    .await;

    match result {
        Ok(Ok(true)) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "session.deleted",
                json!({ "id": session_id_for_event }),
            );
            Json(ApiResponse::success(true))
        }
        Ok(Ok(false)) => Json(ApiResponse::success(false)),
        Ok(Err(e)) => Json(ApiResponse::error(format!("Failed to delete session: {e}"))),
        Err(e) => Json(ApiResponse::error(format!("Failed to delete session: {e}"))),
    }
}
