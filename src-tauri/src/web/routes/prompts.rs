use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;

use crate::app_config::AppType;
use crate::prompt::Prompt as DesktopPrompt;
use crate::services::prompt::PromptService;
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

use crate::web::models::Prompt as WebPrompt;

/// HTTP body for `POST /prompts/import`.
#[derive(Deserialize)]
struct ImportBody {
    app: String,
}

/// Query parameters for app-scoped endpoints.
#[derive(Deserialize)]
struct AppQuery {
    app: Option<String>,
}

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/", get(list_prompts))
        .route("/", post(create_prompt))
        .route("/import", post(import_prompt))
        .route("/current-content", get(get_current_content))
        .route("/:id", get(get_prompt))
        .route("/:id", put(update_prompt))
        .route("/:id", delete(delete_prompt))
        .route("/:id/activate", post(activate_prompt))
}

fn parse_app(app: &str) -> Result<AppType, String> {
    AppType::from_str(app).map_err(|e| e.to_string())
}

fn resolve_app(query: &AppQuery) -> Result<AppType, String> {
    let app = query.app.as_deref().unwrap_or("claude");
    parse_app(app)
}

fn desktop_state(state: &AppState) -> Result<Arc<crate::store::AppState>, String> {
    state.desktop()
}

/// Convert a desktop `Prompt` (with `app_type` field) into the web `Prompt` model
/// the frontend expects. We strip `app_type` to match the frontend `Prompt` interface.
fn desktop_to_web(p: DesktopPrompt) -> WebPrompt {
    WebPrompt {
        id: p.id,
        name: p.name,
        content: p.content,
        description: p.description,
        enabled: p.enabled,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

/// Convert a web `Prompt` (no `app_type`) into the desktop `Prompt` shape used by
/// the DAO/service layer.
fn web_to_desktop(p: WebPrompt, app_type: &str) -> DesktopPrompt {
    DesktopPrompt {
        id: p.id,
        app_type: app_type.to_string(),
        name: p.name,
        content: p.content,
        description: p.description,
        enabled: p.enabled,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

/// Convert a JSON `serde_json::Value` (or a free-form prompt body from the client) into
/// a `WebPrompt`. The client may or may not include `app_type`; we accept the JSON
/// object and look up the app via path/query string.
fn parse_web_prompt(value: serde_json::Value) -> Result<WebPrompt, String> {
    serde_json::from_value(value).map_err(|e| e.to_string())
}

async fn list_prompts(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(query): Query<AppQuery>,
) -> Json<ApiResponse<Vec<WebPrompt>>> {
    let app = match resolve_app(&query) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match desktop.db.get_prompts(app.as_str()) {
        Ok(prompts) => {
            let list: Vec<WebPrompt> = prompts.into_values().map(desktop_to_web).collect();
            Json(ApiResponse::success(list))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_prompt(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Option<WebPrompt>>> {
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match desktop.db.get_prompt_by_id(&id) {
        Ok(Some(p)) => Json(ApiResponse::success(Some(desktop_to_web(p)))),
        Ok(None) => Json(ApiResponse::success(None)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn create_prompt(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<serde_json::Value>,
) -> Json<ApiResponse<String>> {
    // Try to extract an app_type hint from the body. Frontend doesn't send it,
    // but we accept it if present.
    let app_hint = body
        .get("app_type")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("appType").and_then(|v| v.as_str()))
        .unwrap_or("claude");
    let app = match parse_app(app_hint) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let web_prompt = match parse_web_prompt(body) {
        Ok(p) => p,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let id = web_prompt.id.clone();
    let desktop_prompt = web_to_desktop(web_prompt, app.as_str());
    match PromptService::upsert_prompt(&desktop, app, &id, desktop_prompt) {
        Ok(_) => Json(ApiResponse::success(id)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn update_prompt(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    // Look up the existing prompt to find its app_type. Fallback to claude on miss.
    let app = match desktop.db.get_prompt_by_id(&id) {
        Ok(Some(p)) => match parse_app(&p.app_type) {
            Ok(a) => a,
            Err(_) => AppType::Claude,
        },
        _ => AppType::Claude,
    };
    let web_prompt = match parse_web_prompt(body) {
        Ok(mut p) => {
            p.id = id.clone();
            p
        }
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop_prompt = web_to_desktop(web_prompt, app.as_str());
    match PromptService::upsert_prompt(&desktop, app, &id, desktop_prompt) {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn delete_prompt(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<bool>> {
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let app = match desktop.db.get_prompt_by_id(&id) {
        Ok(Some(p)) => match parse_app(&p.app_type) {
            Ok(a) => a,
            Err(_) => AppType::Claude,
        },
        _ => AppType::Claude,
    };
    match PromptService::delete_prompt(&desktop, app, &id) {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn activate_prompt(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<bool>> {
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let app = match desktop.db.get_prompt_by_id(&id) {
        Ok(Some(p)) => match parse_app(&p.app_type) {
            Ok(a) => a,
            Err(_) => AppType::Claude,
        },
        _ => AppType::Claude,
    };
    match PromptService::enable_prompt(&desktop, app, &id) {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn import_prompt(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(body): Json<ImportBody>,
) -> Json<ApiResponse<String>> {
    let app = match parse_app(&body.app) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let desktop = match desktop_state(&state) {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match PromptService::import_from_file(&desktop, app) {
        Ok(id) => Json(ApiResponse::success(id)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn get_current_content(Query(query): Query<AppQuery>) -> Json<ApiResponse<Option<String>>> {
    let app = match resolve_app(&query) {
        Ok(a) => a,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match PromptService::get_current_file_content(app) {
        Ok(content) => Json(ApiResponse::success(content)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::broadcast;

    /// Each test gets its own on-disk directory so they can run in parallel
    /// without stepping on each other.
    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir(label: &str) -> std::path::PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        temp_dir().join(format!(
            "cc-switch-prompt-route-{}-{}-{}-{}",
            label,
            std::process::id(),
            n,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ))
    }

    fn test_ws_state() -> Arc<WsState> {
        let (tx, _rx) = broadcast::channel(16);
        Arc::new(WsState::new(tx))
    }

    fn test_state(dir: &std::path::Path) -> Arc<AppState> {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).unwrap();
        let db_path = dir.join("cc-switch.db");
        Arc::new(AppState::new(db_path.to_str().unwrap()).unwrap())
    }

    /// Resolve a temp dir for fake `~/.claude/CLAUDE.md` so the prompt-file
    /// helpers (which read from `get_home_dir()`) find our test data.
    fn setup_test_home(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", dir);
        std::env::set_var("HOME", dir);
    }

    #[tokio::test]
    #[serial]
    async fn list_prompts_returns_imported_claude_prompts() {
        let dir = test_dir("list-import");
        let state = test_state(&dir);
        let ws = test_ws_state();
        let home = dir.clone();
        setup_test_home(&home);

        std::fs::write(home.join(".claude").join("CLAUDE.md"), "# Hello Claude").unwrap();

        // Import from file.
        let import = import_prompt(
            State((state.clone(), ws.clone())),
            Json(ImportBody {
                app: "claude".to_string(),
            }),
        )
        .await;
        assert!(import.0.success, "import failed: {:?}", import.0.error);

        // List should now return the imported prompt.
        let list = list_prompts(
            State((state.clone(), ws.clone())),
            Query(AppQuery {
                app: "claude".to_string(),
            }),
        )
        .await;
        assert!(list.0.success, "list failed: {:?}", list.0.error);
        let prompts = list.0.data.unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].content, "# Hello Claude");
    }

    #[tokio::test]
    #[serial]
    async fn create_and_activate_prompt_writes_to_file() {
        let dir = test_dir("create-activate");
        let state = test_state(&dir);
        let ws = test_ws_state();
        let home = dir.clone();
        setup_test_home(&home);

        // Make sure the file does not pre-exist with the new content
        // (write_text_file overwrites, but a stale file would muddy the assertion).
        let target = home.join(".claude").join("CLAUDE.md");
        let _ = std::fs::remove_file(&target);

        let body = serde_json::json!({
            "id": "p1",
            "name": "Default",
            "content": "# New Prompt",
            "description": null,
            "enabled": false,
        });

        let created = create_prompt(State((state.clone(), ws.clone())), Json(body)).await;
        assert!(created.0.success, "create failed: {:?}", created.0.error);

        let activated =
            activate_prompt(State((state.clone(), ws.clone())), Path("p1".to_string())).await;
        assert!(
            activated.0.success,
            "activate failed: {:?}",
            activated.0.error
        );

        let content = get_current_content(Query(AppQuery {
            app: "claude".to_string(),
        }))
        .await;
        assert_eq!(content.0.data.unwrap(), Some("# New Prompt".to_string()));

        let file_content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(file_content, "# New Prompt");
    }
}
