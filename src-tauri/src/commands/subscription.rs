use std::str::FromStr;
use tauri::State;

use crate::app_config::AppType;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// 成功时把结果写入 `UsageCache` 并通知托盘刷新，让前端/托盘两路刷新共享缓存。
#[tauri::command]
pub async fn get_subscription_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    tool: String,
) -> Result<SubscriptionQuota, String> {
    let quota = crate::services::subscription::get_subscription_quota(&tool).await?;
    if quota.success {
        if let Ok(app_type) = AppType::from_str(&tool) {
            state.usage_cache.put_subscription(app_type, quota.clone());
            crate::tray::schedule_tray_refresh(&app);
        }
    }
    Ok(quota)
}
