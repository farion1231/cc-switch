use axum::{
    extract::{Query, State},
    routing::{get, post, put},
    Json, Router,
};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::web::{
    handlers::ws::WsState,
    models::{app_state::AppState, ApiResponse},
};

pub fn routes() -> Router<(Arc<AppState>, Arc<WsState>)> {
    Router::new()
        .route("/status", get(get_proxy_status))
        .route("/start", post(start_proxy))
        .route("/stop", post(stop_proxy))
        .route("/stop-with-restore", post(stop_proxy_with_restore))
        .route("/restart", post(restart_proxy))
        .route("/switch", post(switch_proxy_provider))
        .route("/takeover", get(get_takeover_status))
        .route("/takeover", post(set_takeover))
        .route("/config", get(get_proxy_config))
        .route("/config", post(update_proxy_config))
        .route("/config/global", get(get_global_config))
        .route("/config/global", post(update_global_config))
        .route("/config/app", get(get_app_config))
        .route("/config/app", post(update_app_config))
        .route("/default-cost-multiplier", get(get_default_cost_multiplier))
        .route("/default-cost-multiplier", put(set_default_cost_multiplier))
        .route("/pricing-model-source", get(get_pricing_model_source))
        .route("/pricing-model-source", put(set_pricing_model_source))
}

#[derive(Deserialize)]
struct SwitchProxyProviderRequest {
    #[serde(rename = "appType")]
    app_type: String,
    #[serde(rename = "providerId")]
    provider_id: String,
}

#[derive(Deserialize)]
struct StringValueRequest {
    value: String,
}

fn get_setting_string(db: &Connection, key: &str) -> Option<String> {
    db.prepare("SELECT value FROM settings WHERE key = ?1")
        .ok()?
        .query_row([key], |row| row.get::<_, String>(0))
        .ok()
}

async fn get_proxy_status(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<serde_json::Value>> {
    let running: bool = state.with_db(|db: &Connection| {
        get_setting_string(db, "proxy_running")
            .map(|v| v == "1")
            .unwrap_or(false)
    });

    let port: i64 = state.with_db(|db: &Connection| {
        get_setting_string(db, "proxy_port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    });

    let takeover_status: HashMap<String, bool> = state.with_db(|db: &Connection| {
        get_setting_string(db, "proxy_takeover_status")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    });

    Json(ApiResponse::success(json!({
        "running": running,
        "port": port,
        "takeover_status": takeover_status
    })))
}

async fn start_proxy(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<bool>> {
    let result = state
        .with_db(|db: &Connection| {
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_running', '1')",
                [],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(&ws_state, "proxy.started", json!({}));
    }

    Json(ApiResponse::success(result))
}

async fn stop_proxy(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<bool>> {
    let result = state
        .with_db(|db: &Connection| {
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_running', '0')",
                [],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(&ws_state, "proxy.stopped", json!({}));
    }

    Json(ApiResponse::success(result))
}

async fn stop_proxy_with_restore(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<bool>> {
    let result = state
        .with_db(|db: &Connection| {
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_running', '0')",
                [],
            )
            .ok()?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_takeover_status', '{}')",
                [],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(
            &ws_state,
            "proxy.stopped",
            json!({ "restored": true }),
        );
    }

    Json(ApiResponse::success(result))
}

async fn switch_proxy_provider(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(payload): Json<SwitchProxyProviderRequest>,
) -> Json<ApiResponse<bool>> {
    let result: Result<(), String> = state.with_db(|db: &Connection| {
        let category: Option<String> = db
            .prepare("SELECT category FROM providers WHERE id = ?1 AND app_type = ?2")
            .map_err(|e| e.to_string())?
            .query_row([&payload.provider_id, &payload.app_type], |row| row.get(0))
            .ok();

        let Some(category) = category else {
            return Err(format!("Provider not found: {}", payload.provider_id));
        };

        if category == "official" {
            return Err("Cannot switch to official provider during proxy takeover".to_string());
        }

        db.execute("BEGIN TRANSACTION", []).ok();
        let res = db
            .execute(
                "UPDATE providers SET is_current = 0 WHERE app_type = ?1",
                [&payload.app_type],
            )
            .and_then(|_| {
                db.execute(
                    "UPDATE providers SET is_current = 1 WHERE id = ?1 AND app_type = ?2",
                    [&payload.provider_id, &payload.app_type],
                )
            })
            .and_then(|_| {
                db.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    rusqlite::params![
                        format!("proxy_current_provider_{}", payload.app_type),
                        &payload.provider_id
                    ],
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
            crate::web::handlers::ws::broadcast_event(
                &ws_state,
                "proxy.provider_switched",
                json!({ "app": payload.app_type, "id": payload.provider_id }),
            );
            Json(ApiResponse::success(true))
        }
        Err(e) => Json(ApiResponse::error(format!(
            "Failed to switch proxy provider: {e}"
        ))),
    }
}

async fn restart_proxy(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<bool>> {
    let result = state
        .with_db(|db: &Connection| {
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_running', '1')",
                [],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(&ws_state, "proxy.restarted", json!({}));
    }

    Json(ApiResponse::success(result))
}

async fn get_takeover_status(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<HashMap<String, bool>>> {
    let status: HashMap<String, bool> = state.with_db(|db: &Connection| {
        get_setting_string(db, "proxy_takeover_status")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    });

    Json(ApiResponse::success(status))
}

async fn set_takeover(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(payload): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let app = payload.get("app").and_then(|v| v.as_str()).unwrap_or("");
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result = state
        .with_db(|db: &Connection| {
            let status_str = get_setting_string(db, "proxy_takeover_status");
            let mut status: HashMap<String, bool> = status_str
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            status.insert(app.to_string(), enabled);

            let status_str = serde_json::to_string(&status).ok()?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_takeover_status', ?1)",
                [&status_str],
            )
            .ok()?;

            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(
            &ws_state,
            "proxy.takeover_changed",
            json!({ "app": app, "enabled": enabled }),
        );
    }

    Json(ApiResponse::success(result))
}

async fn get_proxy_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<serde_json::Value>> {
    let config: serde_json::Value = state.with_db(|db: &Connection| {
        get_setting_string(db, "proxy_config")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_else(|| json!({}))
    });

    Json(ApiResponse::success(config))
}

async fn update_proxy_config(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(config): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let result = state
        .with_db(|db: &Connection| {
            let config_str = serde_json::to_string(&config).ok()?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_config', ?1)",
                [&config_str],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(&ws_state, "proxy.config_updated", json!({}));
    }

    Json(ApiResponse::success(result))
}

async fn get_global_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
) -> Json<ApiResponse<serde_json::Value>> {
    let config: serde_json::Value = state.with_db(|db: &Connection| {
        get_setting_string(db, "proxy_global_config")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_else(|| json!({}))
    });

    Json(ApiResponse::success(config))
}

async fn update_global_config(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Json(config): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let result = state
        .with_db(|db: &Connection| {
            let config_str = serde_json::to_string(&config).ok()?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('proxy_global_config', ?1)",
                [&config_str],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(
            &ws_state,
            "proxy.global_config_updated",
            json!({}),
        );
    }

    Json(ApiResponse::success(result))
}

async fn get_app_config(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<ApiResponse<serde_json::Value>> {
    let app = params.get("app").cloned().unwrap_or_default();

    let config: serde_json::Value = state.with_db(|db: &Connection| {
        let key = format!("proxy_app_config_{}", app);
        match db.prepare("SELECT value FROM settings WHERE key = ?1") {
            Ok(mut stmt) => stmt
                .query_row([&key], |row| row.get::<_, String>(0))
                .ok()
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_else(|| json!({})),
            Err(_) => json!({}),
        }
    });

    Json(ApiResponse::success(config))
}

async fn update_app_config(
    State((state, ws_state)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<HashMap<String, String>>,
    Json(config): Json<serde_json::Value>,
) -> Json<ApiResponse<bool>> {
    let app = params.get("app").cloned().unwrap_or_default();

    let result = state
        .with_db(|db: &Connection| {
            let key = format!("proxy_app_config_{}", app);
            let config_str = serde_json::to_string(&config).ok()?;
            db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                rusqlite::params![key, config_str],
            )
            .ok()?;
            Some(true)
        })
        .unwrap_or(false);

    if result {
        crate::web::handlers::ws::broadcast_event(
            &ws_state,
            "proxy.app_config_updated",
            json!({ "app": app }),
        );
    }

    Json(ApiResponse::success(result))
}

fn app_param(params: &HashMap<String, String>) -> String {
    params
        .get("app")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "claude".to_string())
}

fn default_cost_multiplier_key(app: &str) -> String {
    format!("proxy_default_cost_multiplier_{app}")
}

fn pricing_model_source_key(app: &str) -> String {
    format!("proxy_pricing_model_source_{app}")
}

async fn get_default_cost_multiplier(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<ApiResponse<String>> {
    let app = app_param(&params);
    let value = state.with_db(|db: &Connection| {
        get_setting_string(db, &default_cost_multiplier_key(&app))
            .unwrap_or_else(|| "1".to_string())
    });

    Json(ApiResponse::success(value))
}

async fn set_default_cost_multiplier(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<StringValueRequest>,
) -> Json<ApiResponse<bool>> {
    if payload
        .value
        .trim()
        .parse::<f64>()
        .map(|value| value < 0.0)
        .unwrap_or(true)
    {
        return Json(ApiResponse::error("Invalid multiplier".to_string()));
    }

    let app = app_param(&params);
    let result = state.with_db(|db: &Connection| {
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            rusqlite::params![default_cost_multiplier_key(&app), payload.value.trim()],
        )
        .is_ok()
    });

    Json(ApiResponse::success(result))
}

async fn get_pricing_model_source(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<ApiResponse<String>> {
    let app = app_param(&params);
    let value = state.with_db(|db: &Connection| {
        get_setting_string(db, &pricing_model_source_key(&app))
            .unwrap_or_else(|| "response".to_string())
    });

    Json(ApiResponse::success(value))
}

async fn set_pricing_model_source(
    State((state, _)): State<(Arc<AppState>, Arc<WsState>)>,
    Query(params): Query<HashMap<String, String>>,
    Json(payload): Json<StringValueRequest>,
) -> Json<ApiResponse<bool>> {
    let value = payload.value.trim();
    if value != "request" && value != "response" {
        return Json(ApiResponse::error(
            "Invalid pricing model source".to_string(),
        ));
    }

    let app = app_param(&params);
    let result = state.with_db(|db: &Connection| {
        db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            rusqlite::params![pricing_model_source_key(&app), value],
        )
        .is_ok()
    });

    Json(ApiResponse::success(result))
}
