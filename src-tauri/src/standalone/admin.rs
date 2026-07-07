//! 管理 API（Chunk 4 实现）。
//!
//! 占位：返回空 Router，使 standalone 模块在当前阶段可编译。

use crate::proxy::server::ProxyState;
use axum::Router;

pub fn build_admin_router() -> Router<ProxyState> {
    Router::new()
}
