use crate::deeplink::{
    import_mcp_from_deeplink, import_prompt_from_deeplink, import_provider_from_deeplink,
    import_skill_from_deeplink, parse_deeplink_url, DeepLinkImportRequest,
};
use crate::store::AppState;
use std::sync::{Mutex, OnceLock};
use tauri::State;

fn pending_deeplink() -> &'static Mutex<Option<DeepLinkImportRequest>> {
    static PENDING_DEEPLINK: OnceLock<Mutex<Option<DeepLinkImportRequest>>> = OnceLock::new();
    PENDING_DEEPLINK.get_or_init(|| Mutex::new(None))
}

fn with_pending_deeplink<R>(f: impl FnOnce(&mut Option<DeepLinkImportRequest>) -> R) -> R {
    let mut guard = pending_deeplink()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

pub fn cache_pending_deeplink(request: DeepLinkImportRequest) {
    with_pending_deeplink(|pending| *pending = Some(request));
}

#[tauri::command]
pub fn take_pending_deeplink() -> Option<DeepLinkImportRequest> {
    with_pending_deeplink(Option::take)
}

/// Parse a deep link URL and return the parsed request for frontend confirmation
#[tauri::command]
pub fn parse_deeplink(url: String) -> Result<DeepLinkImportRequest, String> {
    log::info!("Parsing deep link URL: {url}");
    parse_deeplink_url(&url).map_err(|e| e.to_string())
}

/// Merge configuration from Base64/URL into a deep link request
/// This is used by the frontend to show the complete configuration in the confirmation dialog
#[tauri::command]
pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, String> {
    log::info!("Merging config for deep link request: {:?}", request.name);
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| e.to_string())
}

/// Import a provider from a deep link request (legacy, kept for compatibility)
#[tauri::command]
pub fn import_from_deeplink(
    state: State<AppState>,
    request: DeepLinkImportRequest,
) -> Result<String, String> {
    log::info!(
        "Importing provider from deep link: {:?} for app {:?}",
        request.name,
        request.app
    );

    let provider_id = import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;

    log::info!("Successfully imported provider with ID: {provider_id}");

    Ok(provider_id)
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
            let provider_id =
                import_provider_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "provider",
                "id": provider_id
            }))
        }
        "prompt" => {
            let prompt_id =
                import_prompt_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "prompt",
                "id": prompt_id
            }))
        }
        "mcp" => {
            let result = import_mcp_from_deeplink(&state, request).map_err(|e| e.to_string())?;
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
                import_skill_from_deeplink(&state, request).map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "type": "skill",
                "key": skill_key
            }))
        }
        _ => Err(format!("Unsupported resource type: {}", request.resource)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn sample_request(name: &str) -> DeepLinkImportRequest {
        DeepLinkImportRequest {
            version: "v1".to_string(),
            resource: "provider".to_string(),
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn take_pending_deeplink_consumes_cached_request() {
        let _guard = test_guard();
        let _ = take_pending_deeplink();
        let request = sample_request("cached");

        cache_pending_deeplink(request.clone());

        let consumed = take_pending_deeplink().expect("pending deeplink should be cached");
        assert_eq!(consumed.name.as_deref(), Some("cached"));
        assert!(take_pending_deeplink().is_none());
    }

    #[test]
    fn cache_pending_deeplink_keeps_latest_request() {
        let _guard = test_guard();
        let _ = take_pending_deeplink();
        let first = sample_request("first");
        let latest = sample_request("latest");

        cache_pending_deeplink(first);
        cache_pending_deeplink(latest.clone());

        assert_eq!(
            take_pending_deeplink().and_then(|request| request.name),
            Some("latest".to_string())
        );
    }
}
