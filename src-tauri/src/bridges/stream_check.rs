use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::stream_check::{
    StreamCheckConfig, StreamCheckResult, StreamCheckService as LegacyStreamCheckService,
};
use crate::store::AppState;

use super::support::{convert, fresh_core_state, with_core_state};

pub fn legacy_get_config(state: &AppState) -> Result<StreamCheckConfig, AppError> {
    state.db.get_stream_check_config()
}

pub fn get_config() -> Result<StreamCheckConfig, AppError> {
    with_core_state(cc_switch_core::StreamCheckService::get_config).and_then(convert)
}

pub fn legacy_save_config(state: &AppState, config: &StreamCheckConfig) -> Result<(), AppError> {
    state.db.save_stream_check_config(config)
}

pub fn save_config(config: StreamCheckConfig) -> Result<(), AppError> {
    let config = convert(config)?;
    with_core_state(|state| cc_switch_core::StreamCheckService::save_config(state, &config))
}

pub async fn legacy_check_provider(
    state: &AppState,
    app_type: AppType,
    provider_id: &str,
) -> Result<StreamCheckResult, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers
        .get(provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;
    let result =
        LegacyStreamCheckService::check_with_retry(&app_type, provider, &config, None, None, None)
            .await?;
    let _ = state
        .db
        .save_stream_check_log(provider_id, &provider.name, app_type.as_str(), &result);
    Ok(result)
}

pub async fn check_provider(
    app_type: AppType,
    provider_id: &str,
) -> Result<StreamCheckResult, AppError> {
    let app_type = super::support::to_core_app_type(app_type);
    let state = fresh_core_state()?;
    let result = cc_switch_core::StreamCheckService::check_provider(&state, app_type, provider_id)
        .await
        .map_err(super::support::map_core_err)?;
    convert(result)
}

pub async fn legacy_check_all_providers(
    state: &AppState,
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let allowed_ids = if proxy_targets_only {
        let mut ids = std::collections::HashSet::new();
        if let Ok(Some(current_id)) = state.db.get_current_provider(app_type.as_str()) {
            ids.insert(current_id);
        }
        if let Ok(queue) = state.db.get_failover_queue(app_type.as_str()) {
            for item in queue {
                ids.insert(item.provider_id);
            }
        }
        Some(ids)
    } else {
        None
    };

    let mut results = Vec::new();
    for (id, provider) in providers {
        if let Some(ids) = &allowed_ids {
            if !ids.contains(&id) {
                continue;
            }
        }

        let result = LegacyStreamCheckService::check_with_retry(
            &app_type, &provider, &config, None, None, None,
        )
        .await
        .unwrap_or_else(|error| StreamCheckResult {
            status: crate::services::stream_check::HealthStatus::Failed,
            success: false,
            message: error.to_string(),
            response_time_ms: None,
            http_status: None,
            model_used: String::new(),
            tested_at: chrono::Utc::now().timestamp(),
            retry_count: 0,
        });

        let _ = state
            .db
            .save_stream_check_log(&id, &provider.name, app_type.as_str(), &result);
        results.push((id, result));
    }

    Ok(results)
}

pub async fn check_all_providers(
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    let app_type = super::support::to_core_app_type(app_type);
    let state = fresh_core_state()?;
    let result = cc_switch_core::StreamCheckService::check_all_providers(
        &state,
        app_type,
        proxy_targets_only,
    )
    .await
    .map_err(super::support::map_core_err)?;
    convert(result)
}
