//! Bearer token 鉴权中间件。
//!
//! token 取自共享 `RestConfig`（`Arc<RwLock<RestConfig>>`），运行期可热更新。
//! 空配置时：仅 `127.0.0.1` 直连允许无 token（降级），`0.0.0.0` 必须有 token
//! （`set_rest_config` 已在写入时强制）。中间件只校验"有 token 时请求须带对"。

use std::sync::Arc;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::RwLock;

use crate::rest::config::RestConfig;
use crate::rest::error::RestError;

/// 共享的 REST 配置（token 在此 RwLock 中，热更新）。
pub type SharedRestConfig = Arc<RwLock<RestConfig>>;

/// 从 `Authorization` 头取 bearer token。
fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::trim))
}

/// 鉴权中间件。`State` 为 `SharedRestConfig`。
pub async fn auth_middleware(
    axum::extract::State(cfg): axum::extract::State<SharedRestConfig>,
    req: Request,
    next: Next,
) -> Result<Response, RestError> {
    let token = cfg.read().await.token.clone();
    if token.trim().is_empty() {
        // 无 token：放行（绑定地址已在 set_rest_config 时限制 0.0.0.0 必须有 token）
        return Ok(next.run(req).await);
    }
    match extract_bearer(req.headers()) {
        Some(t) if t == token => Ok(next.run(req).await),
        _ => Err(RestError::Unauthorized),
    }
}
