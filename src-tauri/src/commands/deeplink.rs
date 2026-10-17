use cc_command_api::command_api;
use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_deeplink_url, DeepLinkImportRequest, PendingDeepLinkEntry,
};
use crate::error::AppError;
use crate::store::AppState;

/// Read the deep link waiting for the frontend without clearing it (see `PendingDeepLink`)
#[command_api]
pub fn peek_pending_deeplink() -> Option<PendingDeepLinkEntry> {
    crate::deeplink::peek_pending_deeplink()
}

/// Clear the pending deep link once the frontend has shown it; ignored if `id` is stale
#[command_api]
pub fn ack_pending_deeplink(id: u64) {
    crate::deeplink::ack_pending_deeplink(id);
}

/// Parse a deep link URL and return the parsed request for frontend confirmation
#[command_api]
pub fn parse_deeplink(url: String) -> Result<DeepLinkImportRequest, AppError> {
    log::info!("Parsing deep link URL: {}", crate::url_for_log(&url));
    parse_deeplink_url(&url).map_err(|e| AppError::Config(e.to_string()))
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[command_api]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| AppError::Config(e.to_string()))
}

/// 下载深链接 `iconUrl` 指向的图标，返回核对过类型的 data URI；任何失败都返回 None，
/// 确认框退回内置图标或首字母（原因只写日志，见 `deeplink::icon_fetch`）
#[command_api]
pub async fn fetch_deeplink_icon(url: String) -> Option<String> {
    crate::deeplink::fetch_deeplink_icon(&url).await
}

/// Import a provider from a deep link request (legacy, kept for compatibility)
#[command_api]
pub fn import_from_deeplink(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    log::info!(
        "Importing provider from deep link: {:?} for app {:?}",
        request.name,
        request.app
    );

    let provider_id = import_provider_from_deeplink(&state, request).map_err(|e| AppError::Config(e.to_string()))?;

    log::info!("Successfully imported provider with ID: {provider_id}");

    Ok(provider_id)
}

/// Import resource from a deep link request (unified handler)
#[command_api]
pub async fn import_from_deeplink_unified(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, AppError> {
    log::info!("Importing {} resource from deep link", request.resource);

    match request.resource.as_str() {
        "provider" => {
            let provider_id =
                import_provider_from_deeplink(&state, request).map_err(|e| AppError::Config(e.to_string()))?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(&state, request).map_err(|e| AppError::Config(e.to_string()))?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request).map_err(|e| AppError::Config(e.to_string()))?;
            // Add type field to the result
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key =
                import_skill_from_deeplink(&state, request).map_err(|e| AppError::Config(e.to_string()))?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(AppError::Config(format!("Unsupported resource type: {}", request.resource))),
    }
}
