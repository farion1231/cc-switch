use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::services::workspace::WorkspaceService;
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route(
            "/file/:filename",
            get(read_workspace_file).put(write_workspace_file),
        )
        .route("/daily-memory", get(list_daily_memory_files))
        .route("/daily-memory/search", get(search_daily_memory_files))
        .route(
            "/daily-memory/:filename",
            get(read_daily_memory_file)
                .put(write_daily_memory_file)
                .delete(delete_daily_memory_file),
        )
        .route("/directory", get(get_directory_path))
}

#[derive(Deserialize)]
struct WriteBody {
    content: String,
}

#[derive(Deserialize)]
struct SubdirQuery {
    subdir: String,
}

async fn read_workspace_file(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(filename): Path<String>,
) -> Json<ApiResponse<Option<String>>> {
    match WorkspaceService::read_workspace_file(&filename).await {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn write_workspace_file(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(filename): Path<String>,
    Json(body): Json<WriteBody>,
) -> Json<ApiResponse<bool>> {
    match WorkspaceService::write_workspace_file(&filename, &body.content).await {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn list_daily_memory_files(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Vec<crate::services::workspace::DailyMemoryFileInfo>>> {
    match WorkspaceService::list_daily_memory_files().await {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn read_daily_memory_file(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(filename): Path<String>,
) -> Json<ApiResponse<Option<String>>> {
    match WorkspaceService::read_daily_memory_file(&filename).await {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn write_daily_memory_file(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(filename): Path<String>,
    Json(body): Json<WriteBody>,
) -> Json<ApiResponse<bool>> {
    match WorkspaceService::write_daily_memory_file(&filename, &body.content).await {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn delete_daily_memory_file(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(filename): Path<String>,
) -> Json<ApiResponse<bool>> {
    match WorkspaceService::delete_daily_memory_file(&filename).await {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    query: Option<String>,
}

async fn search_daily_memory_files(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(query): Query<SearchQuery>,
) -> Json<ApiResponse<Vec<crate::services::workspace::DailyMemorySearchResult>>> {
    let q = query.query.as_deref().unwrap_or("");
    match WorkspaceService::search_daily_memory_files(q).await {
        Ok(v) => Json(ApiResponse::success(v)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn get_directory_path(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(query): Query<SubdirQuery>,
) -> (StatusCode, Json<ApiResponse<String>>) {
    let path = match query.subdir.as_str() {
        "memory" => WorkspaceService::memory_directory(),
        "workspace" => WorkspaceService::workspace_directory(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error(format!(
                    "Invalid subdir: '{}'. Allowed values are 'workspace' or 'memory'.",
                    query.subdir
                ))),
            );
        }
    };
    (
        StatusCode::OK,
        Json(ApiResponse::success(path.to_string_lossy().to_string())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestEnv;
    use crate::web::models::app_state::AppState;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    fn test_ws_state() -> Arc<WsState> {
        Arc::new(WsState::new(broadcast::channel(16).0))
    }

    fn test_state(env: &TestEnv) -> Arc<AppState> {
        let db_path = env.home_path().join("cc-switch.db");
        Arc::new(AppState::new(db_path.to_str().unwrap()).unwrap())
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_read_and_write_workspace_file() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();

        let written = write_workspace_file(
            State((state.clone(), ws.clone())),
            Path("AGENTS.md".to_string()),
            Json(WriteBody {
                content: "# agents".to_string(),
            }),
        )
        .await;
        assert!(written.0.success, "write failed: {:?}", written.0.error);

        let read = read_workspace_file(
            State((state.clone(), ws.clone())),
            Path("AGENTS.md".to_string()),
        )
        .await;
        assert!(read.0.success, "read failed: {:?}", read.0.error);
        assert_eq!(read.0.data.unwrap(), Some("# agents".to_string()));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_daily_memory_roundtrip() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();
        let filename = "2026-06-18.md".to_string();

        let written = write_daily_memory_file(
            State((state.clone(), ws.clone())),
            Path(filename.clone()),
            Json(WriteBody {
                content: "# Daily notes\nHello world".to_string(),
            }),
        )
        .await;
        assert!(written.0.success, "write failed: {:?}", written.0.error);

        let read =
            read_daily_memory_file(State((state.clone(), ws.clone())), Path(filename.clone()))
                .await;
        assert!(read.0.success, "read failed: {:?}", read.0.error);
        assert_eq!(
            read.0.data.unwrap(),
            Some("# Daily notes\nHello world".to_string())
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_list_daily_memory_files() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();
        let filename = "2026-06-18.md".to_string();

        let written = write_daily_memory_file(
            State((state.clone(), ws.clone())),
            Path(filename.clone()),
            Json(WriteBody {
                content: "# Daily notes".to_string(),
            }),
        )
        .await;
        assert!(written.0.success, "write failed: {:?}", written.0.error);

        let list = list_daily_memory_files(State((state.clone(), ws.clone()))).await;
        assert!(list.0.success, "list failed: {:?}", list.0.error);
        let files = list.0.data.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, filename);
        assert_eq!(files[0].date, "2026-06-18");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_search_daily_memory_files() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();
        let filename = "2026-06-18.md".to_string();

        let written = write_daily_memory_file(
            State((state.clone(), ws.clone())),
            Path(filename.clone()),
            Json(WriteBody {
                content: "# Daily notes\nHello world".to_string(),
            }),
        )
        .await;
        assert!(written.0.success, "write failed: {:?}", written.0.error);

        let search = search_daily_memory_files(
            State((state.clone(), ws.clone())),
            Query(SearchQuery {
                query: Some("Hello".to_string()),
            }),
        )
        .await;
        assert!(search.0.success, "search failed: {:?}", search.0.error);
        let results = search.0.data.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].filename, filename);
        assert_eq!(results[0].match_count, 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_delete_daily_memory_file() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();
        let filename = "2026-06-18.md".to_string();

        let written = write_daily_memory_file(
            State((state.clone(), ws.clone())),
            Path(filename.clone()),
            Json(WriteBody {
                content: "# Daily notes".to_string(),
            }),
        )
        .await;
        assert!(written.0.success, "write failed: {:?}", written.0.error);

        let deleted =
            delete_daily_memory_file(State((state.clone(), ws.clone())), Path(filename.clone()))
                .await;
        assert!(deleted.0.success, "delete failed: {:?}", deleted.0.error);

        let read =
            read_daily_memory_file(State((state.clone(), ws.clone())), Path(filename.clone()))
                .await;
        assert!(
            read.0.success,
            "read after delete failed: {:?}",
            read.0.error
        );
        assert_eq!(read.0.data.unwrap(), None);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_directory_path_returns_distinct_paths() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();

        let workspace = get_directory_path(
            State((state.clone(), ws.clone())),
            Query(SubdirQuery {
                subdir: "workspace".to_string(),
            }),
        )
        .await;
        assert_eq!(workspace.0, StatusCode::OK);
        assert!(workspace.1 .0.success);
        let ws_path = workspace.1 .0.data.unwrap();
        assert!(ws_path.contains("workspace"));
        assert!(!ws_path.contains("memory"));

        let memory = get_directory_path(
            State((state.clone(), ws.clone())),
            Query(SubdirQuery {
                subdir: "memory".to_string(),
            }),
        )
        .await;
        assert_eq!(memory.0, StatusCode::OK);
        assert!(memory.1 .0.success);
        let mem_path = memory.1 .0.data.unwrap();
        assert!(mem_path.contains("memory"));
        assert!(mem_path.contains("workspace"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn workspace_web_routes_directory_path_rejects_invalid_subdir() {
        let env = TestEnv::new();
        let state = test_state(&env);
        let ws = test_ws_state();

        let result = get_directory_path(
            State((state.clone(), ws.clone())),
            Query(SubdirQuery {
                subdir: "invalid".to_string(),
            }),
        )
        .await;
        assert_eq!(result.0, StatusCode::BAD_REQUEST);
        assert!(!result.1 .0.success);
        assert!(result.1 .0.error.unwrap().contains("Invalid subdir"));
    }
}
