use crate::app_config::AppType;
use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;

use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse, Settings},
};

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/", get(list_settings))
        .route("/", put(update_settings))
        .route("/app-config-path", get(get_app_config_path))
        .route("/common-config/:app_type", get(get_common_config_snippet))
        .route("/common-config/:app_type", put(set_common_config_snippet))
        .route(
            "/common-config/extract",
            post(extract_common_config_snippet),
        )
        .route("/rectifier-config", get(get_rectifier_config))
        .route("/rectifier-config", put(set_rectifier_config))
        .route("/optimizer-config", get(get_optimizer_config))
        .route("/optimizer-config", put(set_optimizer_config))
        .route("/log-config", get(get_log_config))
        .route("/log-config", put(set_log_config))
}

#[derive(Deserialize)]
struct SetCommonConfigSnippetRequest {
    snippet: String,
}

#[derive(Deserialize)]
struct ExtractCommonConfigSnippetRequest {
    #[serde(rename = "appType")]
    app_type: String,
    #[serde(rename = "settingsConfig")]
    settings_config: Option<String>,
}

fn invalid_json_format_error(error: serde_json::Error) -> String {
    let lang = crate::settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    match lang.as_str() {
        "en" => format!("Invalid JSON format: {error}"),
        "ja" => format!("JSON形式が無効です: {error}"),
        _ => format!("无效的 JSON 格式: {error}"),
    }
}

fn invalid_toml_format_error(error: toml_edit::TomlError) -> String {
    let lang = crate::settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    match lang.as_str() {
        "en" => format!("Invalid TOML format: {error}"),
        "ja" => format!("TOML形式が無効です: {error}"),
        _ => format!("无效的 TOML 格式: {error}"),
    }
}

fn validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), String> {
    if snippet.trim().is_empty() {
        return Ok(());
    }

    match app_type {
        "claude" | "gemini" | "omo" | "omo-slim" => {
            serde_json::from_str::<serde_json::Value>(snippet)
                .map_err(invalid_json_format_error)?;
        }
        "codex" => {
            snippet
                .parse::<toml_edit::DocumentMut>()
                .map_err(invalid_toml_format_error)?;
        }
        _ => {}
    }

    Ok(())
}

fn get_setting_value(db: &Connection, key: &str) -> Result<Option<String>, String> {
    let mut stmt = db
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([key]).map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        row.get::<usize, String>(0)
            .map(Some)
            .map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}

fn set_setting_value(db: &Connection, key: &str, value: &str) -> Result<(), String> {
    db.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn delete_setting_value(db: &Connection, key: &str) -> Result<(), String> {
    db.execute(
        "DELETE FROM settings WHERE key = ?1",
        rusqlite::params![key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn config_snippet_key(app_type: &str) -> String {
    format!("common_config_{app_type}")
}

fn config_snippet_cleared_key(app_type: &str) -> String {
    format!("common_config_{app_type}_cleared")
}

fn default_settings() -> Settings {
    Settings {
        show_in_tray: true,
        minimize_to_tray_on_close: true,
        enable_claude_plugin_integration: None,
        skip_claude_onboarding: None,
        launch_on_startup: None,
        silent_startup: None,
        enable_local_proxy: None,
        language: None,
        visible_apps: None,
        claude_config_dir: None,
        codex_config_dir: None,
        gemini_config_dir: None,
        opencode_config_dir: None,
        openclaw_config_dir: None,
        current_provider_claude: None,
        current_provider_codex: None,
        current_provider_gemini: None,
        skill_sync_method: None,
        skill_storage_location: None,
        webdav_sync: None,
        preferred_terminal: None,
        first_run_notice_confirmed: None,
        proxy_confirmed: None,
        usage_confirmed: None,
        stream_check_confirmed: None,
        enable_failover_toggle: None,
        preserve_codex_official_auth_on_switch: None,
        failover_confirmed: None,
        auto_sync_confirmed: None,
        common_config_confirmed: None,
    }
}

async fn list_settings(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<Settings>> {
    let stored: Option<String> = state.with_db(|db: &Connection| {
        let mut stmt = db
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .ok()?;
        stmt.query_row([&"app_settings"], |row: &rusqlite::Row| {
            row.get::<usize, String>(0)
        })
        .ok()
    });

    if let Some(json_str) = stored {
        match serde_json::from_str::<Settings>(&json_str) {
            Ok(settings) => Json(ApiResponse::success(settings)),
            Err(e) => {
                eprintln!(
                    "Failed to parse app_settings from DB: {}. Returning defaults.",
                    e
                );
                Json(ApiResponse::success(default_settings()))
            }
        }
    } else {
        Json(ApiResponse::success(default_settings()))
    }
}

async fn update_settings(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(settings): Json<Settings>,
) -> Json<ApiResponse<bool>> {
    match serde_json::to_string(&settings) {
        Ok(json_str) => {
            let res = state.with_db(|db: &Connection| {
                db.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    rusqlite::params!["app_settings", json_str],
                )
                .map_err(|e| e.to_string())
            });

            match res {
                Ok(_) => {
                    crate::web::handlers::ws::broadcast_event(
                        &ws_state,
                        "settings.changed",
                        json!({}),
                    );
                    Json(ApiResponse::success(true))
                }
                Err(e) => Json(ApiResponse::error(format!("Database error: {}", e))),
            }
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to serialize settings: {}",
            e
        ))),
    }
}

async fn get_app_config_path(
    State((_state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<serde_json::Value>> {
    let path = if let Ok(home_override) = std::env::var("CC_SWITCH_TEST_HOME") {
        let trimmed = home_override.trim();
        if !trimmed.is_empty() {
            std::path::PathBuf::from(trimmed).join(".cc-switch")
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".cc-switch")
        }
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".cc-switch")
    };

    Json(ApiResponse::success(
        json!({ "path": path.to_string_lossy().to_string() }),
    ))
}

async fn get_common_config_snippet(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(app_type): Path<String>,
) -> Json<ApiResponse<Option<String>>> {
    let result =
        state.with_db(|db: &Connection| get_setting_value(db, &config_snippet_key(&app_type)));

    match result {
        Ok(snippet) => Json(ApiResponse::success(snippet)),
        Err(error) => Json(ApiResponse::error(format!(
            "Failed to load common config: {error}"
        ))),
    }
}

async fn set_common_config_snippet(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Path(app_type): Path<String>,
    Json(req): Json<SetCommonConfigSnippetRequest>,
) -> Json<ApiResponse<bool>> {
    if let Err(error) = validate_common_config_snippet(&app_type, &req.snippet) {
        return Json(ApiResponse::error(error));
    }

    let snippet = req.snippet;
    let is_cleared = snippet.trim().is_empty();
    let result = state.with_db(|db: &Connection| {
        if is_cleared {
            delete_setting_value(db, &config_snippet_key(&app_type))?;
            set_setting_value(db, &config_snippet_cleared_key(&app_type), "true")?;
        } else {
            set_setting_value(db, &config_snippet_key(&app_type), &snippet)?;
            delete_setting_value(db, &config_snippet_cleared_key(&app_type))?;
        }

        Ok::<(), String>(())
    });

    match result {
        Ok(_) => Json(ApiResponse::success(true)),
        Err(error) => Json(ApiResponse::error(format!(
            "Failed to save common config: {error}"
        ))),
    }
}

async fn extract_common_config_snippet(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(req): Json<ExtractCommonConfigSnippetRequest>,
) -> Json<ApiResponse<String>> {
    let app = match AppType::from_str(&req.app_type) {
        Ok(app) => app,
        Err(error) => return Json(ApiResponse::error(error.to_string())),
    };

    let settings_config = if let Some(settings_config) =
        req.settings_config.filter(|value| !value.trim().is_empty())
    {
        match serde_json::from_str::<serde_json::Value>(&settings_config) {
            Ok(value) => value,
            Err(error) => return Json(ApiResponse::error(invalid_json_format_error(error))),
        }
    } else {
        let result = state.with_db(|db: &Connection| {
            let mut stmt = db
                .prepare(
                    "SELECT settings_config FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
                )
                .map_err(|e| e.to_string())?;

            let config_text = stmt
                .query_row([req.app_type.as_str()], |row| row.get::<usize, String>(0))
                .map_err(|e| e.to_string())?;

            serde_json::from_str::<serde_json::Value>(&config_text).map_err(invalid_json_format_error)
        });

        match result {
            Ok(value) => value,
            Err(error) => {
                return Json(ApiResponse::error(format!(
                    "Failed to extract common config from current provider: {error}"
                )))
            }
        }
    };

    match crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
        app,
        &settings_config,
    ) {
        Ok(snippet) => Json(ApiResponse::success(snippet)),
        Err(error) => Json(ApiResponse::error(error.to_string())),
    }
}

// ============================================================================
// Rectifier / Optimizer / Log config (stored as JSON in the settings table,
// mirroring the desktop DAO keys so both backends stay interchangeable).
// ============================================================================

async fn get_rectifier_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<crate::proxy::types::RectifierConfig>> {
    let cfg = state.with_db(
        |db: &Connection| match get_setting_value(db, "rectifier_config") {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
            _ => crate::proxy::types::RectifierConfig::default(),
        },
    );
    Json(ApiResponse::success(cfg))
}

async fn set_rectifier_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(config): Json<crate::proxy::types::RectifierConfig>,
) -> Json<ApiResponse<bool>> {
    let json = match serde_json::to_string(&config) {
        Ok(j) => j,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };
    match state.with_db(|db: &Connection| set_setting_value(db, "rectifier_config", &json)) {
        Ok(()) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn get_optimizer_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<crate::proxy::types::OptimizerConfig>> {
    let cfg = state.with_db(
        |db: &Connection| match get_setting_value(db, "optimizer_config") {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
            _ => crate::proxy::types::OptimizerConfig::default(),
        },
    );
    Json(ApiResponse::success(cfg))
}

async fn set_optimizer_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(config): Json<crate::proxy::types::OptimizerConfig>,
) -> Json<ApiResponse<bool>> {
    // Validate cache_ttl: only allow known values (matches desktop command).
    match config.cache_ttl.as_str() {
        "5m" | "1h" => {}
        other => {
            return Json(ApiResponse::error(format!(
                "Invalid cache_ttl value: '{other}'. Allowed values: '5m', '1h'"
            )))
        }
    }
    let json = match serde_json::to_string(&config) {
        Ok(j) => j,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };
    match state.with_db(|db: &Connection| set_setting_value(db, "optimizer_config", &json)) {
        Ok(()) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}

async fn get_log_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<crate::proxy::types::LogConfig>> {
    let cfg = state.with_db(
        |db: &Connection| match get_setting_value(db, "log_config") {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
            _ => crate::proxy::types::LogConfig::default(),
        },
    );
    Json(ApiResponse::success(cfg))
}

async fn set_log_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(config): Json<crate::proxy::types::LogConfig>,
) -> Json<ApiResponse<bool>> {
    let json = match serde_json::to_string(&config) {
        Ok(j) => j,
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };
    match state.with_db(|db: &Connection| set_setting_value(db, "log_config", &json)) {
        Ok(()) => Json(ApiResponse::success(true)),
        Err(e) => Json(ApiResponse::error(e)),
    }
}
