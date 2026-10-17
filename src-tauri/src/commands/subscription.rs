use std::str::FromStr;

use cc_command_api::command_api;
use crate::app_config::AppType;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// `Ok`（成功或确定性失败）写入 `UsageCache`；失败快照写入后
/// `format_subscription_summary` 会通过 `success=false` 守卫返回
/// `None`，避免长期滞留旧配额数字。
/// `Err`（瞬时传输失败）不写快照：保留上一份缓存，与前端
/// react-query reject 保留上次 data 的语义一致。
#[command_api(state = "state")]
pub async fn get_subscription_quota(
    state: &AppState,
    tool: String,
) -> Result<SubscriptionQuota, String> {
    let inner = crate::services::subscription::get_subscription_quota(&tool).await;
    if let Ok(snapshot) = &inner {
        if let Ok(app_type) = AppType::from_str(&tool) {
            state
                .usage_cache
                .put_subscription(app_type, snapshot.clone());
        }
    }
    inner
}
