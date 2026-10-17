//! Phase 2d — failover 簇。
//!
//! 大部分路由已迁移到 `#[command_api]`（`commands/failover.rs`），由
//! `rest_registry::build_router()` 经 inventory 自动挂载。此处仅保留
//! `set_auto_failover_enabled`——其命令用 `AppHandle`（emit 事件 + 托盘刷新），
//! 暂不迁移，手写 REST handler 保留简化版（不含 P1 切换/事件）。

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::rest::error::RestError;
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new().route("/control/v1/failover/auto", axum::routing::put(set_auto))
}

fn require_failover_app(app_type: &str) -> Result<crate::app_config::AppType, RestError> {
    let app = super::common::parse_app(app_type)?;
    if !matches!(
        app,
        crate::app_config::AppType::Claude
            | crate::app_config::AppType::Codex
            | crate::app_config::AppType::Gemini
            | crate::app_config::AppType::GrokBuild
    ) {
        return Err(RestError::Bad(format!(
            "{app_type} 不支持故障转移",
            app_type = app_type
        )));
    }
    Ok(app)
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
    let app = require_failover_app(&body.app_type)?;
    let mut config = state.db.get_proxy_config_for_app(&body.app_type).await?;
    if body.enabled && !crate::mode::current::is_proxy(&app) {
        return Err(RestError::Bad(
            "应用未进入代理模式，无法启用自动故障转移".to_string(),
        ));
    }
    config.auto_failover_enabled = body.enabled;
    state.db.update_proxy_config_for_app(config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
