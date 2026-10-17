//! REST 路由注册器（仅 `rest_api` feature）。
//!
//! `#[command_api(rest = ...)]` 宏为每个命令 `inventory::submit!` 一个 [`RouteReg`]，
//! 此模块用 `inventory::iter` 收集全部 → 自动建 `axum::Router`。
//!
//! **E2 论点**:命令一解耦即自动出现在 REST 路由表，无 parity drift——
//! 对侧 `rest/routes/` 下 11 个手写路由文件镜像 11 个 command 文件，
//! 经此注册器后，凡挂 `#[command_api]` 的命令零行手写即上路由表。
//!
//! 集成现状（渐进迁移）：
//! - `get_providers` 已迁移到宏 → 由本注册器自动挂载
//! - 其余路由仍由 `rest/routes/*.rs` 手写 `.merge()` 挂载
//! - 两套共存于同一 axum Router（见 `rest/routes/mod.rs` 的 `.merge(build_router())`）

#[cfg(feature = "rest_api")]
pub struct RouteReg {
    #[allow(dead_code)]
    pub method: &'static str,
    #[allow(dead_code)]
    pub path: &'static str,
    #[allow(dead_code)]
    pub mount: fn(
        axum::Router<crate::rest::service::RestState>,
    ) -> axum::Router<crate::rest::service::RestState>,
}

#[cfg(feature = "rest_api")]
inventory::collect!(RouteReg);

/// 路由元数据（供 `GET /control/v1/__routes` 发现端点 dump 给 web shim 自动建表）。
#[cfg(feature = "rest_api")]
#[derive(serde::Serialize)]
pub struct RouteMeta {
    pub cmd: &'static str,
    pub method: &'static str,
    /// axum 路径形式（`:id` 动态段）
    pub path: &'static str,
    /// path 参名（用于 shim 从 args 抽取填入 URL）
    #[serde(rename = "pathParams")]
    pub path_params: &'static [&'static str],
}

#[cfg(feature = "rest_api")]
inventory::collect!(RouteMeta);

/// 收集所有 `#[command_api]` 提交的路由，建一个 axum Router（state = RestState）。
///
/// 该 Router 与 `rest/routes/*.rs` 手写路由 `.merge()` 共存；
/// 迁移完成的命令从此处自动挂载，未迁移的仍走手写文件。
#[cfg(feature = "rest_api")]
#[allow(dead_code)] // 由 rest/routes/mod.rs 的 .merge() 调用
pub fn build_router() -> axum::Router<crate::rest::service::RestState> {
    let mut r = axum::Router::new();
    for reg in inventory::iter::<RouteReg> {
        r = (reg.mount)(r);
    }
    r
}

/// dump 全部 `#[command_api]` 路由元数据（按 cmd 排序，供 web shim 自动建表）。
#[cfg(feature = "rest_api")]
pub fn route_table() -> Vec<RouteMeta> {
    let mut v: Vec<&RouteMeta> = inventory::iter::<RouteMeta>().collect();
    v.sort_by_key(|m| m.cmd);
    v.into_iter()
        .map(|m| RouteMeta {
            cmd: m.cmd,
            method: m.method,
            path: m.path,
            path_params: m.path_params,
        })
        .collect()
}
