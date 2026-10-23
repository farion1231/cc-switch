use crate::provider::UsageResult;
use cc_command_api::command_api;

#[command_api]
pub async fn get_balance(base_url: String, api_key: String) -> Result<UsageResult, String> {
    crate::services::balance::get_balance(&base_url, &api_key).await
}
