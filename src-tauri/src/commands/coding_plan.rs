use crate::services::subscription::SubscriptionQuota;

#[tauri::command]
pub async fn get_coding_plan_quota(
    base_url: String,
    api_key: String,
) -> Result<SubscriptionQuota, String> {
    crate::services::coding_plan::get_coding_plan_quota(&base_url, &api_key).await
}
