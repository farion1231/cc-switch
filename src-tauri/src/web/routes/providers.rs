use crate::app_config::AppType;
use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse, Provider},
};
use axum::{
    extract::{Multipart, Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use rusqlite::params;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Deserialize)]
struct CreateProviderRequest {
    provider: Provider,
    app: Option<String>,
}

#[derive(Deserialize)]
struct UpdateProviderRequest {
    provider: Provider,
    app: Option<String>,
    #[serde(rename = "originalId")]
    original_id: Option<String>,
}
use indexmap::IndexMap;
use rusqlite::{Connection, Result as SqliteResult};
use serde_json::{json, Value};

const DEFAULT_APP_TYPE: &str = "claude";

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/", get(list_providers))
        .route("/", post(create_provider))
        .route("/import-opencode-live", post(import_opencode_live))
        .route("/opencode-live-ids", get(get_opencode_live_ids))
        .route("/openclaw-live-ids", get(get_openclaw_live_ids))
        .route("/hermes-live-ids", get(get_hermes_live_ids))
        .route(
            "/claude-desktop-default-routes",
            get(get_claude_desktop_default_routes),
        )
        .route("/claude-desktop-status", get(get_claude_desktop_status))
        .route("/import-openclaw-live", post(import_openclaw_live))
        .route("/import-hermes-live", post(import_hermes_live))
        .route(
            "/import-claude-desktop-from-claude",
            post(import_claude_desktop_from_claude),
        )
        .route(
            "/ensure-claude-desktop-official",
            post(ensure_claude_desktop_official),
        )
        .route("/:id", get(get_provider))
        .route("/:id", put(update_provider))
        .route("/:id", delete(delete_provider))
        .route("/:id/remove-from-live", post(remove_from_live_config))
        .route("/:id/switch", post(switch_provider))
        .route("/:id/endpoints", get(get_custom_endpoints))
        .route("/:id/endpoints", post(add_custom_endpoint))
        .route("/:id/endpoints/:url", delete(remove_custom_endpoint))
        .route("/current", get(get_current_provider))
        .route("/sort", post(update_sort_order))
        .route("/import-default", post(import_default_config))
        .route("/import-upload", post(import_from_upload))
        .route("/fetch-models", post(fetch_models_for_config))
        .route("/usage/test", post(test_usage_script))
        .route("/usage/query", post(query_provider_usage))
        .route("/usage/balance", post(query_balance))
        .route("/usage/coding-plan", post(query_coding_plan_quota))
}

#[derive(Deserialize)]
struct CredentialQuotaRequest {
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: String,
}

/// Query official balance for a base_url/api_key (web mirror of `get_balance`).
async fn query_balance(
    State((_state, _ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<CredentialQuotaRequest>,
) -> Json<ApiResponse<crate::provider::UsageResult>> {
    match crate::services::balance::get_balance(&req.base_url, &req.api_key).await {
        Ok(result) => Json(ApiResponse::success(result)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

/// Query coding-plan quota (web mirror of `get_coding_plan_quota`).
async fn query_coding_plan_quota(
    State((_state, _ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<CredentialQuotaRequest>,
) -> Json<ApiResponse<crate::services::subscription::SubscriptionQuota>> {
    match crate::services::coding_plan::get_coding_plan_quota(
        &req.base_url,
        &req.api_key,
        None,
        None,
    )
    .await
    {
        Ok(quota) => Json(ApiResponse::success(quota)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

#[derive(Deserialize)]
struct TestUsageScriptRequest {
    #[serde(rename = "providerId")]
    provider_id: String,
    app: String,
    #[serde(rename = "scriptCode")]
    script_code: String,
    timeout: Option<u64>,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    #[serde(rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    #[serde(rename = "templateType")]
    template_type: Option<String>,
}

/// Test a usage-query script (web mirror of the Tauri `testUsageScript` command).
async fn test_usage_script(
    State((state, _ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<TestUsageScriptRequest>,
) -> Json<ApiResponse<crate::provider::UsageResult>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let app_type = match AppType::from_str(&req.app) {
        Ok(t) => t,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };

    match crate::services::provider::ProviderService::test_usage_script(
        &desktop,
        app_type,
        &req.provider_id,
        &req.script_code,
        req.timeout.unwrap_or(10),
        req.api_key.as_deref(),
        req.base_url.as_deref(),
        req.access_token.as_deref(),
        req.user_id.as_deref(),
        req.template_type.as_deref(),
    )
    .await
    {
        Ok(result) => Json(ApiResponse::success(result)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct QueryUsageRequest {
    #[serde(rename = "providerId")]
    provider_id: String,
    app: String,
}

/// Query a provider's usage via its saved script (web mirror of `queryProviderUsage`).
///
/// Covers the script-based templates (custom/general/newapi/balance). The
/// Copilot-OAuth and coding-plan native paths are desktop-only and are not
/// surfaced here.
async fn query_provider_usage(
    State((state, _ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<QueryUsageRequest>,
) -> Json<ApiResponse<crate::provider::UsageResult>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let app_type = match AppType::from_str(&req.app) {
        Ok(t) => t,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };

    match crate::services::provider::ProviderService::query_usage_with_templates(
        &desktop,
        app_type,
        &req.provider_id,
    )
    .await
    {
        Ok(result) => Json(ApiResponse::success(result)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
struct FetchModelsRequest {
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(rename = "isFullUrl")]
    is_full_url: Option<bool>,
    #[serde(rename = "modelsUrl")]
    models_url: Option<String>,
    #[serde(rename = "customUserAgent")]
    custom_user_agent: Option<String>,
}

/// Fetch a provider's available model list (OpenAI-compatible `/v1/models`).
///
/// Web mirror of the Tauri `fetch_models_for_config` command — the frontend
/// `fetchModelsForConfig` routes here when running outside Tauri.
async fn fetch_models_for_config(
    State((_state, _ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<FetchModelsRequest>,
) -> Json<ApiResponse<Vec<crate::services::model_fetch::FetchedModel>>> {
    // Mirror the desktop command: invalid UA is silently ignored, never blocks.
    let user_agent = crate::provider::parse_custom_user_agent(req.custom_user_agent.as_deref())
        .ok()
        .flatten();

    match crate::services::model_fetch::fetch_models(
        &req.base_url,
        &req.api_key,
        req.is_full_url.unwrap_or(false),
        req.models_url.as_deref(),
        user_agent,
    )
    .await
    {
        Ok(models) => Json(ApiResponse::success(models)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn import_opencode_live(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<usize>> {
    let providers = match crate::opencode_config::get_typed_providers() {
        Ok(v) => v,
        Err(e) => {
            return Json(ApiResponse::error(format!(
                "Failed to read OpenCode live config: {e}"
            )))
        }
    };

    if providers.is_empty() {
        return Json(ApiResponse::success(0));
    }

    let result: Result<usize, String> = state.with_db(|db: &Connection| {
        let mut stmt = db
            .prepare("SELECT id FROM providers WHERE app_type = 'opencode'")
            .map_err(|e| e.to_string())?;

        let existing_iter = stmt
            .query_map([], |row| row.get::<usize, String>(0))
            .map_err(|e| e.to_string())?;

        let mut existing_ids = std::collections::HashSet::new();
        for id in existing_iter {
            if let Ok(v) = id {
                existing_ids.insert(v);
            }
        }

        let mut imported = 0usize;
        for (id, config) in providers {
            if existing_ids.contains(&id) {
                continue;
            }

            let settings_config = match serde_json::to_value(&config) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let name = config.name.clone().unwrap_or_else(|| id.clone());
            let created_at = chrono::Utc::now().timestamp().to_string();
            let meta = serde_json::json!({ "live_config_managed": true }).to_string();

            db.execute(
                "INSERT INTO providers (id, name, settings_config, website_url, category, created_at, sort_index, notes, is_partner, meta, icon, icon_color, in_failover_queue, app_type, is_current)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                [
                    &id,
                    &name,
                    &serde_json::to_string(&settings_config).unwrap_or_default(),
                    "",
                    "custom",
                    &created_at,
                    "0",
                    "Imported from OpenCode live config",
                    "0",
                    &meta,
                    "",
                    "",
                    "0",
                    "opencode",
                    "0",
                ],
            )
            .map_err(|e| e.to_string())?;

            imported += 1;
        }

        Ok(imported)
    });

    match result {
        Ok(imported) => {
            if imported > 0 {
                crate::web::handlers::ws::broadcast_event(
                    &ws_state,
                    "provider.imported",
                    json!({ "app": "opencode" }),
                );
            }
            Json(ApiResponse::success(imported))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to import OpenCode providers: {e}"
        ))),
    }
}

async fn get_opencode_live_ids() -> Json<ApiResponse<Vec<String>>> {
    match crate::opencode_config::get_typed_providers() {
        Ok(providers) => Json(ApiResponse::success(
            providers.into_iter().map(|(id, _)| id).collect(),
        )),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to read OpenCode live ids: {e}"
        ))),
    }
}

async fn get_openclaw_live_ids() -> Json<ApiResponse<Vec<String>>> {
    match crate::openclaw_config::get_typed_providers() {
        Ok(providers) => Json(ApiResponse::success(
            providers.into_iter().map(|(id, _)| id).collect(),
        )),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to read OpenClaw live ids: {e}"
        ))),
    }
}

async fn get_hermes_live_ids() -> Json<ApiResponse<Vec<String>>> {
    match crate::hermes_config::get_providers() {
        Ok(providers) => Json(ApiResponse::success(
            providers.into_iter().map(|(id, _)| id).collect(),
        )),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to read Hermes live ids: {e}"
        ))),
    }
}

async fn get_claude_desktop_default_routes(
) -> Json<ApiResponse<Vec<crate::claude_desktop_config::ClaudeDesktopDefaultRoute>>> {
    Json(ApiResponse::success(
        crate::claude_desktop_config::default_proxy_routes(),
    ))
}

async fn get_claude_desktop_status(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<crate::claude_desktop_config::ClaudeDesktopStatus>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    let proxy_running = desktop.proxy_service.is_running().await;
    match crate::claude_desktop_config::get_status(desktop.db.as_ref(), proxy_running) {
        Ok(status) => Json(ApiResponse::success(status)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn import_openclaw_live(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<usize>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match crate::services::provider::import_openclaw_providers_from_live(&desktop) {
        Ok(imported) => {
            if imported > 0 {
                crate::web::handlers::ws::broadcast_event(
                    &ws_state,
                    "provider.imported",
                    json!({ "app": "openclaw" }),
                );
            }
            Json(ApiResponse::success(imported))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to import OpenClaw providers: {e}"
        ))),
    }
}

async fn import_hermes_live(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<usize>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match crate::services::provider::import_hermes_providers_from_live(&desktop) {
        Ok(imported) => {
            if imported > 0 {
                crate::web::handlers::ws::broadcast_event(
                    &ws_state,
                    "provider.imported",
                    json!({ "app": "hermes" }),
                );
            }
            Json(ApiResponse::success(imported))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to import Hermes providers: {e}"
        ))),
    }
}

async fn import_claude_desktop_from_claude(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<usize>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match crate::commands::import_claude_desktop_providers_from_claude_impl(&desktop) {
        Ok(imported) => {
            if imported > 0 {
                crate::web::handlers::ws::broadcast_event(
                    &ws_state,
                    "provider.imported",
                    json!({ "app": "claude-desktop" }),
                );
            }
            Json(ApiResponse::success(imported))
        }
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn ensure_claude_desktop_official(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<bool>> {
    let desktop = match state.desktop() {
        Ok(d) => d,
        Err(e) => return Json(ApiResponse::error(e)),
    };
    match desktop.db.ensure_official_seed_by_id(
        crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
        AppType::ClaudeDesktop,
    ) {
        Ok(changed) => Json(ApiResponse::success(changed)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

async fn remove_from_live_config(
    Path(id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let app = payload
        .get("app")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let result = match app {
        "opencode" => crate::opencode_config::remove_provider(&id).map(|_| true),
        "openclaw" => crate::openclaw_config::remove_provider(&id).map(|_| true),
        _ => Err(crate::error::AppError::Config(format!(
            "remove-from-live not supported for app: {app}"
        ))),
    };

    match result {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to remove provider from live config: {e}"
        ))),
    }
}

async fn import_from_upload(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    mut multipart: Multipart,
) -> Json<ApiResponse<bool>> {
    let app = params
        .get("app")
        .cloned()
        .unwrap_or_else(|| DEFAULT_APP_TYPE.to_string());

    // Try to get the uploaded file
    let mut file_content: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("config") {
            file_name = field.file_name().map(|s| s.to_string());
            if let Ok(data) = field.bytes().await {
                file_content = Some(data.to_vec());
            }
            break;
        }
    }

    let (content, name) = match (file_content, file_name) {
        (Some(c), Some(n)) => (c, n),
        _ => {
            return Json(ApiResponse::error("No config file uploaded".to_string()));
        }
    };

    // Parse the config based on app type
    let settings_config: serde_json::Value = match app.as_str() {
        "claude" => match serde_json::from_slice::<serde_json::Value>(&content) {
            Ok(v) => v,
            Err(e) => {
                return Json(ApiResponse::error(format!(
                    "Failed to parse Claude settings.json: {}",
                    e
                )));
            }
        },
        "codex" => {
            // Codex uses auth.json format
            match serde_json::from_slice::<serde_json::Value>(&content) {
                Ok(auth) => {
                    serde_json::json!({ "auth": auth, "config": "" })
                }
                Err(e) => {
                    return Json(ApiResponse::error(format!(
                        "Failed to parse Codex auth.json: {}",
                        e
                    )));
                }
            }
        }
        "gemini" => {
            // Try to parse as JSON first (settings.json)
            if let Ok(settings) = serde_json::from_slice::<serde_json::Value>(&content) {
                serde_json::json!({ "env": {}, "config": settings })
            } else {
                // Try to parse as .env file
                let content_str = String::from_utf8_lossy(&content);
                let mut env_map = serde_json::Map::new();
                for line in content_str.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        env_map.insert(
                            key.trim().to_string(),
                            serde_json::Value::String(value.trim().to_string()),
                        );
                    }
                }
                serde_json::json!({ "env": env_map, "config": {} })
            }
        }
        _ => {
            return Json(ApiResponse::error(format!("Unsupported app type: {}", app)));
        }
    };

    // Create provider from uploaded config
    let result: Result<bool, String> = state.with_db(|db: &Connection| {
        // Check if providers already exist
        let mut stmt = db.prepare(
            "SELECT COUNT(*) FROM providers WHERE app_type = ?1"
        ).map_err(|e| e.to_string())?;

        let count: i64 = stmt.query_row([&app], |row| row.get(0))
            .map_err(|e| e.to_string())?;

        if count > 0 {
            return Ok(false); // Providers already exist, skip import
        }

        let provider_id = format!("imported-{app}-{}", chrono::Utc::now().timestamp());
        let provider_name = format!("Imported {} Config", app);

        db.execute(
            "INSERT OR REPLACE INTO providers (id, name, settings_config, website_url, category, created_at, sort_index, notes, is_partner, meta, icon, icon_color, in_failover_queue, app_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            [
                &provider_id,
                &provider_name,
                &serde_json::to_string(&settings_config).unwrap_or_default(),
                "",
                "custom",
                &chrono::Utc::now().timestamp().to_string(),
                "0",
                &format!("Imported from {}", name),
                "0",
                "{}",
                "",
                "",
                "0",
                &app,
            ]
        ).map_err(|e| e.to_string())?;

        // Set as current provider
        db.execute(
            "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
            [&provider_id, &app]
        ).map_err(|e| e.to_string())?;

        Ok(true)
    });

    match result {
        Ok(true) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "provider.imported",
                json!({ "app": app }),
            );
            Json(ApiResponse::success(true))
        }
        Ok(false) => Json(ApiResponse::error(
            "Providers already exist for this app. Delete existing providers first to import."
                .to_string(),
        )),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to import provider: {}",
            e
        ))),
    }
}

async fn import_default_config(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    body: Option<Json<serde_json::Value>>,
) -> Json<ApiResponse<bool>> {
    let body_app = body
        .as_ref()
        .and_then(|Json(v)| v.get("app").and_then(|x| x.as_str()))
        .map(|s| s.to_string());

    let app = params
        .get("app")
        .cloned()
        .or(body_app)
        .unwrap_or_else(|| DEFAULT_APP_TYPE.to_string());

    // Read config from remote server's filesystem
    let settings_config = match app.as_str() {
        "claude" => {
            let settings_path = dirs::home_dir()
                .map(|h| h.join(".claude/settings.json"))
                .filter(|p| p.exists());

            match settings_path {
                Some(path) => match tokio::fs::read_to_string(&path).await {
                    Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(v) => v,
                        Err(e) => {
                            return Json(ApiResponse::error(format!(
                                "Failed to parse Claude settings.json: {}",
                                e
                            )));
                        }
                    },
                    Err(e) => {
                        return Json(ApiResponse::error(format!(
                            "Failed to read Claude settings.json: {}",
                            e
                        )));
                    }
                },
                None => {
                    return Json(ApiResponse::error(
                        "Claude settings.json not found at ~/.claude/settings.json".to_string(),
                    ));
                }
            }
        }
        "codex" => {
            let auth_path = dirs::home_dir()
                .map(|h| h.join(".codex/auth.json"))
                .filter(|p| p.exists());

            match auth_path {
                Some(path) => match tokio::fs::read_to_string(&path).await {
                    Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(auth) => serde_json::json!({ "auth": auth, "config": "" }),
                        Err(e) => {
                            return Json(ApiResponse::error(format!(
                                "Failed to parse Codex auth.json: {}",
                                e
                            )));
                        }
                    },
                    Err(e) => {
                        return Json(ApiResponse::error(format!(
                            "Failed to read Codex auth.json: {}",
                            e
                        )));
                    }
                },
                None => {
                    return Json(ApiResponse::error(
                        "Codex auth.json not found at ~/.codex/auth.json".to_string(),
                    ));
                }
            }
        }
        "gemini" => {
            let home = dirs::home_dir();
            let env_path = home.as_ref().map(|h| h.join(".gemini/.env"));
            let settings_path = home.as_ref().map(|h| h.join(".gemini/settings.json"));

            let env_data = if let Some(ref path) = env_path {
                if path.exists() {
                    tokio::fs::read_to_string(path).await.ok()
                } else {
                    None
                }
            } else {
                None
            };

            let settings_data = if let Some(ref path) = settings_path {
                if path.exists() {
                    tokio::fs::read_to_string(path)
                        .await
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                } else {
                    None
                }
            } else {
                None
            };

            if env_data.is_none() && settings_data.is_none() {
                return Json(ApiResponse::error(
                    "Gemini config not found at ~/.gemini/.env or ~/.gemini/settings.json"
                        .to_string(),
                ));
            }

            let env_obj = if let Some(content) = env_data {
                let mut env_map = serde_json::Map::new();
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        env_map.insert(
                            key.trim().to_string(),
                            serde_json::Value::String(value.trim().to_string()),
                        );
                    }
                }
                serde_json::Value::Object(env_map)
            } else {
                serde_json::json!({})
            };

            serde_json::json!({
                "env": env_obj,
                "config": settings_data.unwrap_or_else(|| serde_json::json!({}))
            })
        }
        _ => {
            return Json(ApiResponse::error(format!("Unsupported app type: {}", app)));
        }
    };

    let result: Result<bool, String> = state.with_db(|db: &Connection| {
        let mut stmt = db
            .prepare("SELECT COUNT(*) FROM providers WHERE app_type = ?1")
            .map_err(|e| e.to_string())?;

        let count: i64 = stmt
            .query_row([&app], |row| row.get(0))
            .map_err(|e| e.to_string())?;

        if count > 0 {
            return Ok(false); // Do not fail hard when providers already exist
        }

        let provider_id = format!("default-{app}");
        let provider_name = format!("Default {} Config", app);

        db.execute(
            "INSERT OR REPLACE INTO providers (id, name, settings_config, website_url, category, created_at, sort_index, notes, is_partner, meta, icon, icon_color, in_failover_queue, app_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            [
                &provider_id,
                &provider_name,
                &serde_json::to_string(&settings_config).unwrap_or_default(),
                "",
                "custom",
                &chrono::Utc::now().timestamp().to_string(),
                "0",
                &format!("Imported from remote server {} config", app),
                "0",
                "{}",
                "",
                "",
                "0",
                &app,
            ],
        )
        .map_err(|e| e.to_string())?;

        db.execute(
            "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
            [&provider_id, &app],
        )
        .map_err(|e| e.to_string())?;

        Ok(true)
    });

    match result {
        Ok(true) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "provider.imported",
                json!({ "app": app }),
            );
            Json(ApiResponse::success(true))
        }
        Ok(false) => Json(ApiResponse::success(false)),
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to import provider: {}",
            e
        ))),
    }
}

fn row_to_provider(row: &rusqlite::Row) -> SqliteResult<Provider> {
    let settings_config: String = row.get(2)?;
    let meta: Option<String> = row.get(9)?;

    Ok(Provider {
        id: row.get(0)?,
        name: row.get(1)?,
        settings_config: serde_json::from_str(&settings_config).unwrap_or_default(),
        website_url: row.get(3)?,
        category: row.get(4)?,
        created_at: row.get(5)?,
        sort_index: row.get(6)?,
        notes: row.get(7)?,
        is_partner: row.get(8)?,
        meta: meta.and_then(|m: String| serde_json::from_str(&m).ok()),
        icon: row.get(10)?,
        icon_color: row.get(11)?,
        in_failover_queue: row.get(12)?,
    })
}

async fn list_providers(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<ApiResponse<IndexMap<String, Provider>>> {
    let app = params.get("app").cloned().unwrap_or_default();

    let result: Result<IndexMap<String, Provider>, String> = state.with_db(|db: &Connection| {
        let mut stmt = db.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, is_partner, meta, icon, icon_color, in_failover_queue
             FROM providers
             WHERE app_type = ?1
             ORDER BY sort_index ASC, created_at ASC"
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map([&app], row_to_provider)
            .map_err(|e| e.to_string())?;

        let mut providers = IndexMap::new();
        for row in rows {
            if let Ok(provider) = row {
                providers.insert(provider.id.clone(), provider);
            }
        }

        Ok(providers)
    });

    match result {
        Ok(providers) => Json(ApiResponse::success(providers)),
        Err(e) => Json(ApiResponse::error(format!("Database error: {}", e))),
    }
}

async fn get_provider(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Option<Provider>>> {
    let result: Option<Provider> = state.with_db(|db: &Connection| {
        let mut stmt = db.prepare(
            "SELECT id, name, settings_config, website_url, category, created_at, sort_index, notes, is_partner, meta, icon, icon_color, in_failover_queue
             FROM providers
             WHERE id = ?1"
        ).ok()?;

        stmt.query_row([&id], row_to_provider).ok()
    });

    Json(ApiResponse::success(result))
}

async fn create_provider(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<CreateProviderRequest>,
) -> Json<ApiResponse<String>> {
    let app_type = req.app.as_deref().unwrap_or(DEFAULT_APP_TYPE).to_string();
    let provider = req.provider;
    let result: Result<(), String> = state.with_db(|db: &Connection| {
        let created_at = provider.created_at.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let sort_index = provider.sort_index;
        let is_partner: i32 = if provider.is_partner.unwrap_or(false) { 1 } else { 0 };
        let in_failover: i32 = if provider.in_failover_queue.unwrap_or(false) { 1 } else { 0 };

        db.execute(
            "INSERT INTO providers (id, name, settings_config, website_url, category, created_at, sort_index, notes, is_partner, meta, icon, icon_color, in_failover_queue, app_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                provider.id,
                provider.name,
                serde_json::to_string(&provider.settings_config).unwrap_or_default(),
                provider.website_url.unwrap_or_default(),
                provider.category.unwrap_or_default(),
                created_at,
                sort_index,
                provider.notes.unwrap_or_default(),
                is_partner,
                serde_json::to_string(&provider.meta).unwrap_or_default(),
                provider.icon.unwrap_or_default(),
                provider.icon_color.unwrap_or_default(),
                in_failover,
                app_type,
            ]
        ).map_err(|e| e.to_string())?;

        Ok(())
    });

    match result {
        Ok(_) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "provider.created",
                json!({ "id": provider.id, "app": app_type }),
            );
            Json(ApiResponse::success(provider.id))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to create provider: {}",
            e
        ))),
    }
}

async fn update_provider(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> Json<ApiResponse<bool>> {
    let app_type = req.app.as_deref().unwrap_or(DEFAULT_APP_TYPE).to_string();
    let provider = req.provider;
    let original_id = req.original_id.unwrap_or(id.clone());
    let updated_id = provider.id.clone();
    let result: Result<(), String> = state.with_db(|db: &Connection| {
        let sort_index = provider.sort_index;
        let is_partner: i32 = if provider.is_partner.unwrap_or(false) {
            1
        } else {
            0
        };
        let in_failover: i32 = if provider.in_failover_queue.unwrap_or(false) {
            1
        } else {
            0
        };

        db.execute(
            "UPDATE providers SET
                name = ?2, settings_config = ?3, website_url = ?4, category = ?5,
                sort_index = ?6, notes = ?7, is_partner = ?8, meta = ?9,
                icon = ?10, icon_color = ?11, in_failover_queue = ?12
             WHERE id = ?1 AND app_type = ?13",
            params![
                original_id,
                provider.name,
                serde_json::to_string(&provider.settings_config).unwrap_or_default(),
                provider.website_url.unwrap_or_default(),
                provider.category.unwrap_or_default(),
                sort_index,
                provider.notes.unwrap_or_default(),
                is_partner,
                serde_json::to_string(&provider.meta).unwrap_or_default(),
                provider.icon.unwrap_or_default(),
                provider.icon_color.unwrap_or_default(),
                in_failover,
                app_type,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    });

    match result {
        Ok(_) => {
            if let Err(e) = sync_updated_provider_runtime_state(&state, &app_type, &updated_id) {
                return Json(ApiResponse::error(format!(
                    "Saved in database but failed to sync runtime config: {}",
                    e
                )));
            }

            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "provider.updated",
                json!({ "id": updated_id, "previousId": original_id, "app": app_type }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to update provider: {}",
            e
        ))),
    }
}

fn sync_updated_provider_runtime_state(
    state: &AppState,
    app: &str,
    provider_id: &str,
) -> Result<(), String> {
    let app_type = AppType::from_str(app).map_err(|e| e.to_string())?;

    let current_settings = state.with_db(|db: &Connection| -> Result<Option<Value>, String> {
        let mut stmt = db
            .prepare(
                "SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2 AND is_current = 1",
            )
            .map_err(|e| e.to_string())?;

        let config_str: Option<String> = stmt
            .query_row([provider_id, app], |row| row.get(0))
            .ok();

        let Some(config_str) = config_str else {
            return Ok(None);
        };

        let settings = serde_json::from_str::<Value>(&config_str).map_err(|e| e.to_string())?;
        Ok(Some(settings))
    })?;

    let Some(settings) = current_settings else {
        // Edited provider is not currently active; DB-only update is expected.
        return Ok(());
    };

    match app_type {
        AppType::Claude => {
            let sanitized = crate::services::provider::sanitize_claude_settings_for_live(&settings);
            let path = crate::config::get_claude_settings_path();
            crate::config::write_json_file(&path, &sanitized).map_err(|e| e.to_string())?;
        }
        AppType::Codex => {
            let settings_obj = settings
                .as_object()
                .ok_or_else(|| "Codex settings_config must be an object".to_string())?;
            let auth = settings_obj
                .get("auth")
                .ok_or_else(|| "Codex settings_config.auth is missing".to_string())?;
            let config_text = settings_obj.get("config").and_then(|v| v.as_str());
            crate::codex_config::write_codex_live_atomic(auth, config_text)
                .map_err(|e| e.to_string())?;
        }
        AppType::Gemini => {
            let env_map =
                crate::gemini_config::json_to_env(&settings).map_err(|e| e.to_string())?;
            crate::gemini_config::write_gemini_env_atomic(&env_map).map_err(|e| e.to_string())?;

            if let Some(config_value) = settings.get("config") {
                if config_value.is_object() {
                    let settings_path = crate::gemini_config::get_gemini_settings_path();
                    crate::config::write_json_file(&settings_path, config_value)
                        .map_err(|e| e.to_string())?;
                } else if !config_value.is_null() {
                    return Err("Gemini settings_config.config must be object or null".to_string());
                }
            }
        }
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => {
            // Additive-mode apps are managed in their own live files and don't have exclusive "current" live overwrite.
        }
        AppType::ClaudeDesktop => {
            // Web mode keeps the provider record in sync, but does not rewrite the local Claude Desktop profile here.
        }
    }

    Ok(())
}

async fn delete_provider(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<ApiResponse<bool>> {
    let app = params
        .get("app")
        .cloned()
        .unwrap_or_else(|| DEFAULT_APP_TYPE.to_string());
    let result: Result<(), String> = state.with_db(|db: &Connection| {
        // Remove any child rows first so the delete works regardless of whether
        // the desktop `provider_endpoints` table (with its FK) exists on this
        // shared file. `execute` on a non-existent table errors, so ignore that.
        let _ = db.execute(
            "DELETE FROM provider_endpoints WHERE provider_id = ?1 AND app_type = ?2",
            [&id, &app],
        );
        db.execute(
            "DELETE FROM providers WHERE id = ?1 AND app_type = ?2",
            [&id, &app],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    });

    match result {
        Ok(_) => {
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "provider.deleted",
                json!({ "id": id }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to delete provider: {}",
            e
        ))),
    }
}

async fn switch_provider(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<ApiResponse<bool>> {
    let app = params
        .get("app")
        .cloned()
        .unwrap_or_else(|| DEFAULT_APP_TYPE.to_string());

    let result: Result<(), String> = state.with_db(|db: &Connection| {
        db.execute("BEGIN TRANSACTION", []).ok();

        let res: Result<usize, rusqlite::Error> = db
            .execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
                [&app],
            )
            .and_then(|_| {
                db.execute(
                    "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
                    [&id, &app],
                )
            });

        match res {
            Ok(_) => {
                db.execute("COMMIT", []).ok();
                Ok(())
            }
            Err(e) => {
                db.execute("ROLLBACK", []).ok();
                Err(e.to_string())
            }
        }
    });

    match result {
        Ok(_) => {
            if let Err(e) = sync_switched_provider_runtime_state(&state, &app, &id) {
                return Json(ApiResponse::error(format!(
                    "Switched in database but failed to sync runtime config: {}",
                    e
                )));
            }

            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "provider.switched",
                json!({ "id": id, "app": app }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to switch provider: {}",
            e
        ))),
    }
}

fn sync_switched_provider_runtime_state(
    state: &AppState,
    app: &str,
    provider_id: &str,
) -> Result<(), String> {
    let app_type = AppType::from_str(app).map_err(|e| e.to_string())?;

    // Keep device-level current provider in sync with DB selection.
    crate::settings::set_current_provider(&app_type, Some(provider_id))
        .map_err(|e| e.to_string())?;

    // In web/api-only mode, Claude switch must also write live settings immediately.
    if app_type == AppType::Claude {
        let settings_config = state.with_db(|db: &Connection| {
            let mut stmt = db
                .prepare("SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2")
                .map_err(|e| e.to_string())?;

            let config_str: String = stmt
                .query_row([provider_id, app], |row| row.get(0))
                .map_err(|e| e.to_string())?;

            serde_json::from_str::<serde_json::Value>(&config_str).map_err(|e| e.to_string())
        })?;

        let sanitized =
            crate::services::provider::sanitize_claude_settings_for_live(&settings_config);
        let path = crate::config::get_claude_settings_path();
        crate::config::write_json_file(&path, &sanitized).map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn get_current_provider(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<ApiResponse<Option<String>>> {
    let app = params.get("app").cloned().unwrap_or_default();

    let result: Option<String> = state.with_db(|db: &Connection| {
        let mut stmt = db
            .prepare("SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1")
            .ok()?;

        stmt.query_row([&app], |row: &rusqlite::Row| row.get::<usize, String>(0))
            .ok()
    });

    Json(ApiResponse::success(result))
}

async fn get_custom_endpoints(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Vec<crate::web::models::CustomEndpoint>>> {
    let result: Vec<crate::web::models::CustomEndpoint> = state.with_db(|db: &Connection| {
        let mut stmt = match db.prepare("SELECT meta FROM providers WHERE id = ?1") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let meta_str: String = match stmt.query_row([&id], |row: &rusqlite::Row| row.get(0)) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let meta: Option<crate::web::models::ProviderMeta> = serde_json::from_str(&meta_str).ok();

        meta.and_then(|m| m.custom_endpoints)
            .map(|endpoints| endpoints.into_values().collect::<Vec<_>>())
            .unwrap_or_default()
    });

    Json(ApiResponse::success(result))
}

async fn add_custom_endpoint(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(_id): Path<String>,
    Json(_endpoint): Json<crate::web::models::CustomEndpoint>,
) -> Json<ApiResponse<bool>> {
    Json(ApiResponse::success(true))
}

async fn remove_custom_endpoint(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path((_id, _url)): Path<(String, String)>,
) -> Json<ApiResponse<bool>> {
    Json(ApiResponse::success(true))
}

async fn update_sort_order(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(_updates): Json<Vec<serde_json::Value>>,
) -> Json<ApiResponse<bool>> {
    Json(ApiResponse::success(true))
}
