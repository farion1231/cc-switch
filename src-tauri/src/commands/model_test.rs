//! 模型测试相关命令

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::model_test::{
    ModelTestConfig, ModelTestLog, ModelTestResult, ModelTestService,
};
use crate::store::AppState;
use tauri::State;

/// 测试单个供应商的模型可用性
#[tauri::command]
pub async fn test_provider_model(
    state: State<'_, AppState>,
    app_type: AppType,
    provider_id: String,
) -> Result<ModelTestResult, AppError> {
    // 获取测试配置
    let config = state.db.get_model_test_config()?;

    // 获取供应商
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers
        .get(&provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

    // 执行测试
    let result = ModelTestService::test_provider(&app_type, provider, &config).await?;

    // 记录日志
    let _ = state.db.save_model_test_log(
        &provider_id,
        &provider.name,
        app_type.as_str(),
        &result.model_used,
        &config.test_prompt,
        &result,
    );

    Ok(result)
}

/// 批量测试所有供应商
#[tauri::command]
pub async fn test_all_providers_model(
    state: State<'_, AppState>,
    app_type: AppType,
    proxy_targets_only: bool,
) -> Result<Vec<(String, ModelTestResult)>, AppError> {
    let config = state.db.get_model_test_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;

    let mut results = Vec::new();

    for (id, provider) in providers {
        // 如果只测试代理目标，跳过非代理目标
        if proxy_targets_only && !provider.is_proxy_target.unwrap_or(false) {
            continue;
        }

        match ModelTestService::test_provider(&app_type, &provider, &config).await {
            Ok(result) => {
                // 记录日志
                let _ = state.db.save_model_test_log(
                    &id,
                    &provider.name,
                    app_type.as_str(),
                    &result.model_used,
                    &config.test_prompt,
                    &result,
                );
                results.push((id, result));
            }
            Err(e) => {
                let error_result = ModelTestResult {
                    success: false,
                    message: e.to_string(),
                    response_time_ms: None,
                    http_status: None,
                    model_used: String::new(),
                    tested_at: chrono::Utc::now().timestamp(),
                };
                results.push((id, error_result));
            }
        }
    }

    Ok(results)
}

/// 获取模型测试配置
#[tauri::command]
pub fn get_model_test_config(state: State<'_, AppState>) -> Result<ModelTestConfig, AppError> {
    state.db.get_model_test_config()
}

/// 保存模型测试配置
#[tauri::command]
pub fn save_model_test_config(
    state: State<'_, AppState>,
    config: ModelTestConfig,
) -> Result<(), AppError> {
    state.db.save_model_test_config(&config)
}

/// 获取模型测试日志
#[tauri::command]
pub fn get_model_test_logs(
    state: State<'_, AppState>,
    app_type: Option<String>,
    provider_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ModelTestLog>, AppError> {
    state.db.get_model_test_logs(
        app_type.as_deref(),
        provider_id.as_deref(),
        limit.unwrap_or(50),
    )
}

/// 清理旧的测试日志
#[tauri::command]
pub fn cleanup_model_test_logs(
    state: State<'_, AppState>,
    keep_count: Option<u32>,
) -> Result<u64, AppError> {
    state.db.cleanup_model_test_logs(keep_count.unwrap_or(100))
}
