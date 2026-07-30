//! 使用统计相关命令

use crate::error::AppError;
use crate::services::usage_stats::*;
use crate::store::AppState;
pub use cc_switch_core::ModelPricingInfo;
use cc_switch_core::{
    DataSourceSummary, HeadlessState, OperationCancellation, PricingUpdate, SessionSyncResult,
    UsageService,
};
use tauri::State;

/// 获取使用量汇总
#[tauri::command]
pub fn get_usage_summary(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<UsageSummary, AppError> {
    state.db.get_usage_summary(
        start_date,
        end_date,
        app_type.as_deref(),
        provider_name.as_deref(),
        model.as_deref(),
    )
}

/// 获取按 app_type 拆分的使用量汇总
#[tauri::command]
pub fn get_usage_summary_by_app(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<UsageSummaryByApp>, AppError> {
    state.db.get_usage_summary_by_app(
        start_date,
        end_date,
        provider_name.as_deref(),
        model.as_deref(),
    )
}

/// 获取每日趋势
#[tauri::command]
pub fn get_usage_trends(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<DailyStats>, AppError> {
    state.db.get_daily_trends(
        start_date,
        end_date,
        app_type.as_deref(),
        provider_name.as_deref(),
        model.as_deref(),
    )
}

/// 获取 Provider 统计
#[tauri::command]
pub fn get_provider_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<ProviderStats>, AppError> {
    state.db.get_provider_stats(
        start_date,
        end_date,
        app_type.as_deref(),
        provider_name.as_deref(),
        model.as_deref(),
    )
}

/// 获取模型统计
#[tauri::command]
pub fn get_model_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
) -> Result<Vec<ModelStats>, AppError> {
    state.db.get_model_stats(
        start_date,
        end_date,
        app_type.as_deref(),
        provider_name.as_deref(),
        model.as_deref(),
    )
}

/// 获取请求日志列表
#[tauri::command]
pub fn get_request_logs(
    state: State<'_, AppState>,
    filters: LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, AppError> {
    state.db.get_request_logs(&filters, page, page_size)
}

/// 获取单个请求详情
#[tauri::command]
pub fn get_request_detail(
    state: State<'_, AppState>,
    request_id: String,
) -> Result<Option<RequestLogDetail>, AppError> {
    state.db.get_request_detail(&request_id)
}

/// 获取模型定价列表
#[tauri::command]
pub fn get_model_pricing(state: State<'_, AppState>) -> Result<Vec<ModelPricingInfo>, AppError> {
    state.db.ensure_model_pricing_seeded()?;
    let conn = crate::database::lock_conn!(state.db.conn);
    UsageService::pricing(&*conn).map_err(map_core_usage_error)
}

/// 更新模型定价；写入与历史成本回填仍由桌面写侧服务维护。
#[tauri::command]
pub fn update_model_pricing(
    state: State<'_, AppState>,
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<(), AppError> {
    let conn = crate::database::lock_conn!(state.db.conn);
    UsageService::update_pricing_on_connection(
        &conn,
        PricingUpdate {
            model_id,
            display_name,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
        },
    )
    .map_err(map_core_usage_error)
}

/// 检查 Provider 使用限额
#[tauri::command]
pub fn check_provider_limits(
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    let conn = crate::database::lock_conn!(state.db.conn);
    UsageService::limits_on_connection(&conn, &provider_id, &app_type).map_err(map_core_usage_error)
}

/// 删除模型定价
#[tauri::command]
pub fn delete_model_pricing(state: State<'_, AppState>, model_id: String) -> Result<(), AppError> {
    let conn = crate::database::lock_conn!(state.db.conn);
    UsageService::delete_pricing_on_connection(&conn, &model_id).map_err(map_core_usage_error)
}

/// 手动触发会话日志同步
#[tauri::command]
pub async fn sync_session_usage(state: State<'_, AppState>) -> Result<SessionSyncResult, AppError> {
    // 仍持有桌面进程级互斥锁，避免自动同步与手动同步同时扫描；实际文件和 SQLite
    // 业务统一交给 Core，远端 Agent 因此得到相同的来源集合与写入语义。
    let _db_lifetime = state.db.clone();
    let _guard = crate::services::session_sync::session_sync_mutex()
        .lock()
        .await;
    tauri::async_runtime::spawn_blocking(move || -> Result<SessionSyncResult, AppError> {
        let core =
            HeadlessState::open(crate::config::get_home_dir()).map_err(map_core_usage_error)?;
        UsageService::sync_sessions(&core, &OperationCancellation::active())
            .map_err(map_core_usage_error)
    })
    .await
    .map_err(|error| AppError::Message(format!("会话用量同步任务失败: {error}")))?
    .inspect(|result| {
        if result.imported > 0 {
            crate::usage_events::notify_log_recorded();
        }
    })
}

/// Codex reset 成功后，无论重导是否导入新行或返回错误，都必须通知前端刷新。
/// 调用方应只在 reset 成功后调用，避免把未发生的数据变更误报为重建完成。
fn finish_codex_rebuild(
    result: Result<SessionSyncResult, AppError>,
) -> Result<SessionSyncResult, AppError> {
    crate::usage_events::notify_log_recorded();
    result
}

/// 备份数据库后，仅重建 Codex session 用量。锁覆盖 backup → reset → import
/// 整个序列，避免后台同步在清理和重导之间插入数据。
#[tauri::command]
pub async fn rebuild_codex_usage(
    state: State<'_, AppState>,
) -> Result<SessionSyncResult, AppError> {
    let _db_lifetime = state.db.clone();
    let _guard = crate::services::session_sync::session_sync_mutex()
        .lock()
        .await;
    tauri::async_runtime::spawn_blocking(move || -> Result<SessionSyncResult, AppError> {
        // Core 内部固定执行 backup -> reset -> import，并在每个破坏性边界检查取消；
        // 桌面层只负责异步调度和刷新事件，不能重新拆开该序列。
        let core =
            HeadlessState::open(crate::config::get_home_dir()).map_err(map_core_usage_error)?;
        let result = UsageService::rebuild_codex(&core, &OperationCancellation::active())
            .map_err(map_core_usage_error);
        finish_codex_rebuild(result)
    })
    .await
    .map_err(|error| AppError::Message(format!("Codex 用量重建任务失败: {error}")))?
}

/// 获取数据来源分布
#[tauri::command]
pub fn get_usage_data_sources(
    state: State<'_, AppState>,
) -> Result<Vec<DataSourceSummary>, AppError> {
    let conn = crate::database::lock_conn!(state.db.conn);
    UsageService::data_sources(&*conn).map_err(map_core_usage_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rebuild_notifies_when_reimport_is_empty() {
        crate::usage_events::take_test_notify_count();

        let result = finish_codex_rebuild(Ok(SessionSyncResult::default())).expect("空重导应成功");

        assert_eq!(result.imported, 0);
        assert_eq!(crate::usage_events::take_test_notify_count(), 1);
    }

    #[test]
    fn codex_rebuild_notifies_when_reimport_fails_after_reset() {
        crate::usage_events::take_test_notify_count();

        let result = finish_codex_rebuild(Err(AppError::Message(
            "synthetic reimport failure".to_string(),
        )));

        assert!(result.is_err());
        assert_eq!(crate::usage_events::take_test_notify_count(), 1);
    }
}
