use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, DeepLinkImportRequest, PendingDeepLinkEntry,
};
use crate::error::AppError;
use crate::store::AppState;
use tauri::State;

/// Read the deep link waiting for the frontend without clearing it (see `PendingDeepLink`)
#[tauri::command]
pub fn peek_pending_deeplink() -> Option<PendingDeepLinkEntry> {
    crate::deeplink::peek_pending_deeplink()
}

/// Clear the pending deep link once the frontend has shown it; ignored if `id` is stale
#[tauri::command]
pub fn ack_pending_deeplink(id: u64) {
    crate::deeplink::ack_pending_deeplink(id);
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[tauri::command]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request)
}

/// 下载深链接 `iconUrl` 指向的图标，返回核对过类型的 data URI；任何失败都返回 None，
/// 确认框退回内置图标或首字母（原因只写日志，见 `deeplink::icon_fetch`）
#[tauri::command]
pub async fn fetch_deeplink_icon(url: String) -> Option<String> {
    crate::deeplink::fetch_deeplink_icon(&url).await
}

/// Import resource from a deep link request (unified handler)
#[tauri::command]
pub async fn import_from_deeplink_unified(
    state: State<'_, AppState>,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, String> {
    log::info!("Importing {} resource from deep link", request.resource);

    match request.resource.as_str() {
        "provider" => {
            let provider_id = import_provider_from_deeplink(&state, request)?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id = import_prompt_from_deeplink(&state, request)?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request)?;
            // Add type field to the result
            Ok(serde_json::json!({
                "type": "mcp",
                "importedCount": result.imported_count,
                "importedIds": result.imported_ids,
                "failed": result.failed
            }))
        }
        "skill" => {
            let skill_key = import_skill_from_deeplink(&state, request)?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
}
