//! 会话级供应商绑定
//!
//! 目标：同一会话内只在第一次请求时做跨供应商意图路由，
//! 后续轮次继续使用第一次选定的供应商，避免对话中途切换。

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::settings;
use crate::database::Database;

/// 根据会话绑定和当前设置，决定本次请求应该使用的供应商列表。
///
/// 目前实现策略：
/// - 只在启用意图路由时生效；
/// - Session ID 由上层根据 `RequestContext` 提供；
/// - 当前版本仅用于 Claude，后续可扩展到其他 AppType。
pub fn resolve_session_bound_providers<'a>(
    _app_type: AppType,
    session_id: &str,
    providers: &'a [Provider],
    _db: &Database,
) -> Result<&'a [Provider], AppError> {
    // 目前只是预留扩展点：
    // - 未来可以在此根据 session_id 读取/写入“首选供应商”缓存；
    // - 目前会话绑定由 ClaudeIntentRouter 基于是否为“首轮请求”来控制，
    //   这里保持透传行为，以免引入新的状态持久化。
    let _ = (session_id, _db); // silence unused warnings for now
    Ok(providers)
}

