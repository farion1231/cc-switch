use crate::provider::UsageResult;

#[tauri::command]
pub async fn get_balance(base_url: String, api_key: String) -> Result<UsageResult, String> {
    let api_key = crate::services::api_key_vars::resolve::resolve_value(&api_key)
        .map_err(|e| e.to_string())?
        .into_owned();
    crate::services::balance::get_balance(&base_url, &api_key).await
}
