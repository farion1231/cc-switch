//! 镜像 commands/pi.rs —— pi 集成当前状态（只读）。

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;

use crate::rest::error::RestError;
use crate::services::pi_state::PiStateService;
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new().route(
        "/control/v1/pi-current-state",
        get(get_pi_current_state_handler),
    )
}

/// `GET /control/v1/pi-current-state` → PiCurrentState（只读，pi 集成状态）。
async fn get_pi_current_state_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, RestError> {
    let st = tokio::task::spawn_blocking(move || PiStateService::current(&state))
        .await
        .map_err(|e| RestError::App(format!("读取 pi 状态失败: {e}")))?
        .map_err(|e| RestError::App(e.to_string()))?;
    Ok(Json(st))
}
