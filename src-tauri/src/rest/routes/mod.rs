//! REST 控制面路由 + handler（Phase 1）。
//!
//! handler 通过 `services/*` 调用业务逻辑，绝不直接读写数据库（R-003）。
//! switch 调 `ProviderService::switch`（与 `commands/provider.rs` 同路径），
//! 保证 live 写入 + 事件 emit +（代理在跑时）`current_providers` 同步（R-004）。

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use serde::Deserialize;

use crate::rest::auth::SharedRestConfig;
use crate::rest::error::RestError;
use crate::rest::service::RestState;
use crate::services::{ProviderService, SwitchResult};
use crate::store::AppState;

mod common;
mod db;
// config.rs 簇已迁至 #[command_api]（get_config_dir），由 rest_registry 自动挂载
mod failover;
mod global_proxy;
mod misc;
mod pi;
mod profile;
mod provider;
mod proxy;
mod rest_config;
mod settings;
mod skill;

/// axum 共享状态 = `RestState`（复合：AppState + SkillService + CopilotAuthManager）。
pub fn router(state: RestState, config: SharedRestConfig) -> axum::Router {
    // 需鉴权的控制面 API
    let api = axum::Router::new()
        // Phase 1
        // get_providers / get_current_provider 已迁移到 #[command_api]，由
        // rest_registry::build_router() 自动挂载
        .route("/control/v1/providers/switch", post(switch_provider))
        .route("/control/v1/proxy/status", get(get_proxy_status))
        .route("/control/v1/proxy/restart", post(restart_proxy))
        // Phase 2 簇
        .merge(provider::routes())
        .merge(proxy::routes())
        .merge(global_proxy::routes())
        .merge(failover::routes())
        .merge(profile::routes())
        .merge(settings::routes())
        // config 簇已迁至 #[command_api]（get_config_dir），由 rest_registry 自动挂载
        .merge(misc::routes())
        .merge(skill::routes())
        .merge(pi::routes())
        .merge(rest_config::routes())
        .merge(db::routes())
        // config 簇已迁至 #[command_api]（get_config_dir），由 rest_registry 自动挂载
        // #[command_api] 宏生成的路由（inventory 自动收集）
        .merge(crate::rest_registry::build_router())
        .with_state(state)
        .layer(axum::middleware::from_fn_with_state(
            config,
            crate::rest::auth::auth_middleware,
        ))
        .layer(tower_http::cors::CorsLayer::permissive());

    // 公开路由（文档 + web 控制台 + 路由发现）：不走 auth 中间件
    axum::Router::new()
        .route(
            "/control/v1/openapi.json",
            get(crate::rest::openapi::openapi_json),
        )
        .route("/control/v1/docs", get(crate::rest::openapi::swagger_ui))
        .route(
            "/control/v1/__routes",
            get(|| async { axum::Json(crate::rest_registry::route_table()) }),
        )
        .merge(crate::rest::web::routes())
        .merge(api)
}

use common::parse_app;

// 注：get_providers 已迁移到 #[command_api]（commands/provider.rs），
// 由 rest_registry::build_router() 经 inventory 自动挂载——不再此处手写。

// 注：get_providers / get_current_provider 已迁移到 #[command_api]
// （commands/provider.rs），由 rest_registry::build_router() 经 inventory 自动挂载。
// get_current_provider 的信封 `{current:"..."}` 随迁移消失——REST 现返 raw 字符串。

/// `POST /control/v1/providers/switch`  body `{"app":"claude","provider_id":"<id>"}`
async fn switch_provider(
    State(state): State<AppState>,
    Json(body): Json<SwitchBody>,
) -> Result<impl IntoResponse, RestError> {
    let app_type = parse_app(&body.app)?;
    let id = body.provider_id.clone();
    let result =
        tokio::task::spawn_blocking(move || ProviderService::switch(&state, app_type, &id))
            .await
            .map_err(|e| RestError::App(format!("任务执行失败: {e}")))??;
    Ok(Json::<SwitchResult>(result))
}

#[derive(Deserialize)]
struct SwitchBody {
    app: String,
    provider_id: String,
}

/// `GET /control/v1/proxy/status`
async fn get_proxy_status(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    let status = state
        .proxy_service
        .get_status()
        .await
        .map_err(RestError::App)?;
    Ok(Json(status))
}

/// `POST /control/v1/proxy/restart`（幂等：运行中 stop+start；未运行直接 start）
async fn restart_proxy(State(state): State<AppState>) -> Result<impl IntoResponse, RestError> {
    // stop() 在未运行时返回 Err，忽略——幂等语义。
    let _ = state.proxy_service.stop().await;
    let info = state.proxy_service.start().await.map_err(RestError::App)?;
    Ok(Json(info))
}
