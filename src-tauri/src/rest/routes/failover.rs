//! Phase 2d — failover 簇。
//!
//! `set_auto_failover_enabled` 命令用 `AppHandle`（emit + tray refresh），
//! 保持 `#[tauri::command]`。REST 路由复用 `_impl`（含 P1 切换 + rollback，
//! 不含 emit / tray refresh）。

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::rest::error::RestError;
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new().route("/control/v1/failover/auto", axum::routing::put(set_auto))
}

#[derive(Deserialize)]
struct SetAutoBody {
    #[serde(rename = "appType")]
    app_type: String,
    enabled: bool,
}

async fn set_auto(
    State(state): State<AppState>,
    Json(body): Json<SetAutoBody>,
) -> Result<impl IntoResponse, RestError> {
    crate::commands::failover::set_auto_failover_enabled_impl(&state, body.app_type, body.enabled)
        .await
        .map_err(|e| RestError::App(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
