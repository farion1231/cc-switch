use crate::provider::UsageResult;

#[tauri::command]
pub async fn get_balance(
    base_url: String,
    api_key: String,
    // 所属供应商的代理（用量设置里测试时传入）；启用全局代理时忽略
    proxy_url: Option<String>,
) -> Result<UsageResult, String> {
    crate::proxy::http_client::scope_provider_proxy(
        proxy_url.as_deref(),
        crate::services::balance::get_balance(&base_url, &api_key),
    )
    .await
}
