use crate::services::subscription::SubscriptionQuota;

/// 查询官方订阅额度
///
/// 读取 CLI 工具已有的 OAuth 凭据并调用官方 API 获取使用额度。
/// 不需要 AppState（不访问数据库），直接读文件 + 发 HTTP。
#[tauri::command]
pub async fn get_subscription_quota(tool: String) -> Result<SubscriptionQuota, String> {
    crate::services::subscription::get_subscription_quota(&tool).await
}
