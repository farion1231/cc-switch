//! Custom endpoints management
//!
//! Handles CRUD operations for provider custom endpoints.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::settings::CustomEndpoint;
use crate::store::AppState;

/// Get custom endpoints list for a provider
pub fn get_custom_endpoints(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<Vec<CustomEndpoint>, AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let Some(provider) = providers.get(provider_id) else {
        return Ok(vec![]);
    };
    let Some(meta) = provider.meta.as_ref() else {
        return Ok(vec![]);
    };
    if meta.custom_endpoints.is_empty() {
        return Ok(vec![]);
    }

    let mut result: Vec<_> = meta.custom_endpoints.values().cloned().collect();
    result.sort_by_key(|ep| std::cmp::Reverse(ep.added_at));
    Ok(result)
}

/// Add a custom endpoint to a provider
pub fn add_custom_endpoint(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    let normalized = url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err(AppError::localized(
            "provider.endpoint.url_required",
            "URL 不能为空",
            "URL cannot be empty",
        ));
    }

    state
        .db
        .add_custom_endpoint(app_type.as_str(), provider_id, &normalized)?;
    Ok(())
}

/// Remove a custom endpoint from a provider
pub fn remove_custom_endpoint(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    let normalized = url.trim().trim_end_matches('/').to_string();
    state
        .db
        .remove_custom_endpoint(app_type.as_str(), provider_id, &normalized)?;
    Ok(())
}

/// Update endpoint last used timestamp
pub fn update_endpoint_last_used(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
    url: String,
) -> Result<(), AppError> {
    let normalized = url.trim().trim_end_matches('/').to_string();

    // 端点表是这份数据的权威来源，直接写 `last_used` 列。以前是先把值塞进
    // ProviderMeta.custom_endpoints 再 save_provider，可 save_provider 写库前会把
    // custom_endpoints 从 meta 里摘掉（端点走独立表），于是这个时间戳从来没落过盘，
    // 读完永远是 None。
    state
        .db
        .set_endpoint_last_used(app_type.as_str(), provider_id, &normalized, now_millis())
}

/// Get current timestamp in milliseconds
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
