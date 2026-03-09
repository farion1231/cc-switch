use serde_json::json;

use crate::deeplink::DeepLinkImportRequest;
use crate::error::AppError;
use crate::store::AppState;

use super::support::{convert, fresh_core_state, map_core_err};

pub fn legacy_parse_deeplink(url: &str) -> Result<DeepLinkImportRequest, AppError> {
    crate::deeplink::parse_deeplink_url(url).map_err(|e| AppError::Message(e.to_string()))
}

pub fn parse_deeplink(url: &str) -> Result<DeepLinkImportRequest, AppError> {
    let parsed = cc_switch_core::parse_deeplink_url(url).map_err(map_core_err)?;
    convert(parsed)
}

pub fn legacy_merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    crate::deeplink::parse_and_merge_config(&request).map_err(|e| AppError::Message(e.to_string()))
}

pub fn merge_deeplink_config(
    request: DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    let request = convert(request)?;
    let merged = cc_switch_core::parse_and_merge_config(&request).map_err(map_core_err)?;
    convert(merged)
}

pub fn legacy_import_provider(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    crate::deeplink::import_provider_from_deeplink(state, request)
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn import_provider(request: DeepLinkImportRequest) -> Result<String, AppError> {
    let state = fresh_core_state()?;
    let request = convert(request)?;
    cc_switch_core::import_provider_from_deeplink(&state, request).map_err(map_core_err)
}

pub fn legacy_import_prompt(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    crate::deeplink::import_prompt_from_deeplink(state, request)
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn import_prompt(request: DeepLinkImportRequest) -> Result<String, AppError> {
    let state = fresh_core_state()?;
    let request = convert(request)?;
    cc_switch_core::import_prompt_from_deeplink(&state, request).map_err(map_core_err)
}

pub fn legacy_import_mcp(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<serde_json::Value, AppError> {
    let result = crate::deeplink::import_mcp_from_deeplink(state, request)
        .map_err(|e| AppError::Message(e.to_string()))?;
    Ok(json!({
        "type": "mcp",
        "importedCount": result.imported_count,
        "importedIds": result.imported_ids,
        "failed": result.failed
    }))
}

pub fn import_mcp(request: DeepLinkImportRequest) -> Result<serde_json::Value, AppError> {
    let state = fresh_core_state()?;
    let request = convert(request)?;
    let result = cc_switch_core::import_mcp_from_deeplink(&state, request).map_err(map_core_err)?;
    Ok(json!({
        "type": "mcp",
        "importedCount": result.imported_count,
        "importedIds": result.imported_ids,
        "failed": result.failed
    }))
}

pub fn legacy_import_skill(
    state: &AppState,
    request: DeepLinkImportRequest,
) -> Result<String, AppError> {
    crate::deeplink::import_skill_from_deeplink(state, request)
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn import_skill(request: DeepLinkImportRequest) -> Result<String, AppError> {
    let state = fresh_core_state()?;
    let request = convert(request)?;
    cc_switch_core::import_skill_from_deeplink(&state, request).map_err(map_core_err)
}
