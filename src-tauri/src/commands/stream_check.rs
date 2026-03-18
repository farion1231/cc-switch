//! 流式健康检查命令

use crate::app_config::AppType;
use crate::commands::copilot::CopilotAuthState;
use crate::error::AppError;
use crate::services::stream_check::{
    HealthStatus, StreamCheckConfig, StreamCheckResult, StreamCheckService,
};
use crate::store::AppState;
use std::collections::HashSet;
use tauri::State;

/// 流式健康检查（单个供应商）
#[tauri::command]
pub async fn stream_check_provider(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    app_type: AppType,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    let config = state.db.get_stream_check_config()?;

    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers
        .get(&provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

    let auth_override = resolve_copilot_auth_override(provider, &copilot_state).await?;
    let result =
        StreamCheckService::check_with_retry(&app_type, provider, &config, auth_override).await?;

    // 记录日志
    let _ =
        state
            .db
            .save_stream_check_log(&provider_id, &provider.name, app_type.as_str(), &result);

    Ok(result)
}

/// 批量流式健康检查
#[tauri::command]
pub async fn stream_check_all_providers(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, StreamCheckResult)>, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;

    let mut results = Vec::new();
    let allowed_ids: Option<HashSet<String>> = if proxy_targets_only {
        let mut ids = HashSet::new();
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

    for (id, provider) in providers {
        if let Some(ids) = &allowed_ids {
            if !ids.contains(&id) {
                continue;
            }
        }

        let auth_override = resolve_copilot_auth_override(&provider, &copilot_state).await?;
        let result =
            StreamCheckService::check_with_retry(&app_type, &provider, &config, auth_override)
                .await
                .unwrap_or_else(|e| StreamCheckResult {
                    status: HealthStatus::Failed,
                    success: false,
                    message: e.to_string(),
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

/// 获取流式检查配置
#[tauri::command]
pub fn get_stream_check_config(state: State<'_, AppState>) -> Result<StreamCheckConfig, AppError> {
    state.db.get_stream_check_config()
}

/// 保存流式检查配置
#[tauri::command]
pub fn save_stream_check_config(
    state: State<'_, AppState>,
    config: StreamCheckConfig,
) -> Result<(), AppError> {
    state.db.save_stream_check_config(&config)
}

async fn resolve_copilot_auth_override(
    provider: &crate::provider::Provider,
    copilot_state: &State<'_, CopilotAuthState>,
) -> Result<Option<crate::proxy::providers::AuthInfo>, AppError> {
    let is_copilot = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("github_copilot")
        || provider
            .settings_config
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .map(|url| url.contains("githubcopilot.com"))
            .unwrap_or(false);

    if !is_copilot {
        return Ok(None);
    }

    let auth_manager = copilot_state.0.read().await;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.github_account_id.clone());

    let token = match account_id.as_deref() {
        Some(id) => auth_manager
            .get_valid_token_for_account(id)
            .await
            .map_err(|e| AppError::Message(format!("GitHub Copilot 认证失败: {e}")))?,
        None => auth_manager
            .get_valid_token()
            .await
            .map_err(|e| AppError::Message(format!("GitHub Copilot 认证失败: {e}")))?,
    };

    Ok(Some(crate::proxy::providers::AuthInfo::new(
        token,
        crate::proxy::providers::AuthStrategy::GitHubCopilot,
    )))
}
