use crate::services::subscription::SubscriptionQuota;

#[tauri::command]
pub async fn get_coding_plan_quota(
    base_url: String,
    api_key: String,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
) -> Result<SubscriptionQuota, String> {
    crate::services::coding_plan::get_coding_plan_quota(
        &base_url,
        &api_key,
        access_key_id.as_deref(),
        secret_access_key.as_deref(),
    )
    .await
}
