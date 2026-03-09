//! 使用统计相关命令

use crate::bridges::usage as usage_bridge;
use crate::error::AppError;
use crate::services::usage_stats::*;
use crate::store::AppState;
use tauri::State;

/// 获取使用量汇总
#[tauri::command]
pub fn get_usage_summary(
    _state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<UsageSummary, AppError> {
    usage_bridge::get_usage_summary(start_date, end_date)
}

/// 获取每日趋势
#[tauri::command]
pub fn get_usage_trends(
    _state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<DailyStats>, AppError> {
    usage_bridge::get_usage_trends(start_date, end_date)
}

/// 获取 Provider 统计
#[tauri::command]
pub fn get_provider_stats(_state: State<'_, AppState>) -> Result<Vec<ProviderStats>, AppError> {
    usage_bridge::get_provider_stats()
}

/// 获取模型统计
#[tauri::command]
pub fn get_model_stats(_state: State<'_, AppState>) -> Result<Vec<ModelStats>, AppError> {
    usage_bridge::get_model_stats()
}

/// 获取请求日志列表
#[tauri::command]
pub fn get_request_logs(
    _state: State<'_, AppState>,
    filters: LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, AppError> {
    usage_bridge::get_request_logs(filters, page, page_size)
}

/// 获取单个请求详情
#[tauri::command]
pub fn get_request_detail(
    _state: State<'_, AppState>,
    request_id: String,
) -> Result<Option<RequestLogDetail>, AppError> {
    usage_bridge::get_request_detail(&request_id)
}

/// 获取模型定价列表
#[tauri::command]
pub fn get_model_pricing(_state: State<'_, AppState>) -> Result<Vec<ModelPricingInfo>, AppError> {
    usage_bridge::get_model_pricing()
}

/// 更新模型定价
#[tauri::command]
pub fn update_model_pricing(
    _state: State<'_, AppState>,
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<(), AppError> {
    usage_bridge::update_model_pricing(ModelPricingInfo {
        model_id,
        display_name,
        input_cost_per_million: input_cost,
        output_cost_per_million: output_cost,
        cache_read_cost_per_million: cache_read_cost,
        cache_creation_cost_per_million: cache_creation_cost,
    })
}

/// 检查 Provider 使用限额
#[tauri::command]
pub fn check_provider_limits(
    _state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    usage_bridge::check_provider_limits(&provider_id, &app_type)
}

/// 删除模型定价
#[tauri::command]
pub fn delete_model_pricing(
    _state: State<'_, AppState>,
    model_id: String,
) -> Result<(), AppError> {
    usage_bridge::delete_model_pricing(&model_id)
}

/// 模型定价信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingInfo {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
}
