//! 镜像 commands/proxy.rs —— 代理配置 + 接管 + 熔断簇。

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;

use crate::proxy::types::AppProxyConfig;
use crate::proxy::{CircuitBreakerConfig, ProxyConfig, ProxyTakeoverStatus};
use crate::rest::error::RestError;
use crate::store::AppState;

pub fn routes() -> axum::Router<crate::rest::service::RestState> {
    axum::Router::new()
        .route("/control/v1/proxy/config", get(get_proxy_config))
        .route(
            "/control/v1/proxy/config",
            axum::routing::put(update_proxy_config),
        )
        .route("/control/v1/proxy/config/:app", get(get_app_proxy_config))
        .route(
            "/control/v1/proxy/config/:app",
            axum::routing::put(update_app_proxy_config),
        )
        .route("/control/v1/proxy/takeover", get(get_takeover_status))
        .route(
            "/control/v1/proxy/takeover/:app",
            post(set_takeover_for_app),
        )
        .route("/control/v1/proxy/health/:app", get(get_provider_health))
        .route(
            "/control/v1/proxy/circuit-breaker/config",
            get(get_cb_config),
        )
        .route(
            "/control/v1/proxy/circuit-breaker/config",
            axum::routing::put(update_cb_config),
        )
        .route(
            "/control/v1/proxy/circuit-breaker/reset",
            post(reset_circuit_breaker),
        )
        .route("/control/v1/proxy/circuit-breaker/stats", get(get_cb_stats))
}

async fn get_proxy_config(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let cfg = state.proxy_service.get_config().await?;
    Ok(Json(cfg))
}

async fn update_proxy_config(
    State(state): State<AppState>,
    Json(config): Json<ProxyConfig>,
) -> Result<impl IntoResponse, RestError> {
    let addr_changed = state.proxy_service.update_config(&config).await?;
    if addr_changed {
        crate::mode::controller::resync_routes(&state).await?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_takeover_status(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, RestError> {
    let status: ProxyTakeoverStatus = state.proxy_service.get_takeover_status().await?;
    Ok(Json(status))
}

async fn set_takeover_for_app(
    State(state): State<AppState>,
    axum::extract::Path(app_type): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<ToggleQuery>,
) -> Result<impl IntoResponse, RestError> {
    let app = super::common::parse_app(&app_type)?;
    if !app.supports_local_proxy() {
        return Err(RestError::Bad(format!("{} 不支持本地路由", app.as_str())));
    }
    if q.enabled {
        crate::mode::controller::enter(&state, &app, false).await?;
    } else {
        crate::mode::controller::exit(&state, &app).await?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
struct ToggleQuery {
    enabled: bool,
}

async fn get_provider_health(
    State(state): State<AppState>,
    axum::extract::Path(app_type): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<HealthQuery>,
) -> Result<impl IntoResponse, RestError> {
    let app = super::common::parse_app(&app_type)?;
    if !app.supports_local_proxy() {
        return Err(RestError::Bad(format!("{} 不支持本地路由", app.as_str())));
    }
    let health = state
        .db
        .get_provider_health(&q.provider_id, app_type.as_str())
        .await?;
    Ok(Json(health))
}

#[derive(serde::Deserialize)]
struct HealthQuery {
    #[serde(rename = "providerId")]
    provider_id: String,
}

async fn get_cb_config(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let cfg: CircuitBreakerConfig = state.db.get_circuit_breaker_config().await?;
    Ok(Json(cfg))
}

async fn update_cb_config(
    State(state): State<AppState>,
    Json(config): Json<CircuitBreakerConfig>,
) -> Result<impl IntoResponse, RestError> {
    state.db.update_circuit_breaker_config(&config).await?;
    state
        .proxy_service
        .update_circuit_breaker_configs(config)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
struct ResetBody {
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(rename = "appType")]
    app_type: String,
}

async fn reset_circuit_breaker(
    State(state): State<AppState>,
    Json(body): Json<ResetBody>,
) -> Result<impl IntoResponse, RestError> {
    let app = super::common::parse_app(&body.app_type)?;
    if !app.supports_local_proxy() {
        return Err(RestError::Bad(format!("{} 不支持本地路由", app.as_str())));
    }
    // 1. 重置数据库健康状态
    state
        .db
        .update_provider_health(&body.provider_id, app.as_str(), true, None)
        .await?;
    // 2. 重置内存熔断器
    state
        .proxy_service
        .reset_provider_circuit_breaker(&body.provider_id, app.as_str())
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// F-10: 补全 `get_circuit_breaker_stats` 镜像。
///
/// 命令侧为 stub（恒返回 `Ok(None)`，需访问运行中代理内存态），REST 镜像
/// 同样语义——当前返回 null，与 command parity。
async fn get_cb_stats(
    State(state): State<AppState>,
    Query(q): Query<CbStatsQuery>,
) -> Result<impl IntoResponse, RestError> {
    let _ = super::common::parse_app(&q.app)?;
    // 命令侧 stub：恒 None。这里镜像同样语义，避免与 command 出现 shape 偏差。
    let _ = (&state, &q.provider_id);
    Ok(Json(serde_json::Value::Null))
}

#[derive(serde::Deserialize)]
struct CbStatsQuery {
    #[serde(rename = "provider_id")]
    provider_id: String,
    app: String,
}

/// F-11: per-app 代理配置镜像（`get_proxy_config_for_app` / `update_proxy_config_for_app`）。
/// 区别于 `/proxy/config`（整体 ProxyConfig）。
async fn get_app_proxy_config(
    State(state): State<AppState>,
    axum::extract::Path(app_type): axum::extract::Path<String>,
) -> Result<impl IntoResponse, RestError> {
    let app = super::common::parse_app(&app_type)?;
    let cfg = state.db.get_proxy_config_for_app(app.as_str()).await?;
    Ok(Json(cfg))
}

async fn update_app_proxy_config(
    State(state): State<AppState>,
    axum::extract::Path(app_type): axum::extract::Path<String>,
    Json(config): Json<AppProxyConfig>,
) -> Result<impl IntoResponse, RestError> {
    let app = super::common::parse_app(&app_type)?;
    let _ = app;
    state.db.update_proxy_config_for_app(config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
