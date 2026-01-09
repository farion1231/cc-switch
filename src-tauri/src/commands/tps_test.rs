//! TPS（Token Per Second）测试命令

use crate::app_config::AppType;
use crate::error::AppError;
use crate::services::tps_test::{TpsTestResult, TpsTestService};
use crate::store::AppState;
use tauri::State;

/// TPS 测试（单个供应商）
#[tauri::command]
pub async fn tps_test_provider(
    state: State<'_, AppState>,
    app_type: AppType,
    provider_id: String,
) -> Result<TpsTestResult, AppError> {
    let config = state.db.get_stream_check_config()?;
    let providers = state.db.get_all_providers(app_type.as_str())?;
    let provider = providers
        .get(&provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

    Ok(TpsTestService::test_once(&app_type, provider, config.timeout_secs).await)
}

