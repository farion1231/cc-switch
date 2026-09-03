//! 使用统计相关命令

use crate::error::AppError;
use crate::services::model_pricing::{ModelPricingInfo, ModelsDevSyncConfig, ModelsDevSyncState};
use crate::services::usage_stats::*;
use crate::store::AppState;
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
    log::info!("获取模型定价列表");
    state.db.ensure_model_pricing_seeded()?;
    crate::services::model_pricing::sync_local_model_pricing(&state.db)?;

    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);

    // 检查表是否存在
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )
        .unwrap_or(false);

    if !table_exists {
        log::error!("model_pricing 表不存在,可能需要重启应用以触发数据库迁移");
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         ORDER BY display_name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(ModelPricingInfo {
            model_id: row.get(0)?,
            display_name: row.get(1)?,
            input_cost_per_million: row.get(2)?,
            output_cost_per_million: row.get(3)?,
            cache_read_cost_per_million: row.get(4)?,
            cache_creation_cost_per_million: row.get(5)?,
        })
    })?;

    let mut pricing = Vec::new();
    for row in rows {
        pricing.push(row?);
    }

    log::info!("成功获取 {} 条模型定价数据", pricing.len());
    Ok(pricing)
}

/// 更新模型定价
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
    crate::services::model_pricing::update_model_pricing(
        &state.db,
        ModelPricingInfo {
            model_id,
            display_name,
            input_cost_per_million: input_cost,
            output_cost_per_million: output_cost,
            cache_read_cost_per_million: cache_read_cost,
            cache_creation_cost_per_million: cache_creation_cost,
        },
    )?;
    Ok(())
}

/// 批量更新模型定价（models.dev 自动同步仅触发一次历史成本回填）
#[tauri::command]
pub fn update_model_pricing_batch(
    state: State<'_, AppState>,
    entries: Vec<ModelPricingInfo>,
) -> Result<usize, AppError> {
    crate::services::model_pricing::update_model_pricing_batch(&state.db, entries)
}

#[tauri::command]
pub fn get_models_dev_sync_config(
    state: State<'_, AppState>,
) -> Result<ModelsDevSyncState, AppError> {
    crate::services::model_pricing::get_models_dev_sync_state(&state.db)
}

#[tauri::command]
pub fn save_models_dev_sync_config(
    state: State<'_, AppState>,
    config: ModelsDevSyncConfig,
) -> Result<(), AppError> {
    crate::services::model_pricing::save_models_dev_sync_config(&state.db, config)
}

#[tauri::command]
pub fn record_models_dev_sync_result(
    state: State<'_, AppState>,
    synced_at: Option<i64>,
    error: Option<String>,
) -> Result<(), AppError> {
    crate::services::model_pricing::record_models_dev_sync_result(&state.db, synced_at, error)
}

/// 检查 Provider 使用限额
#[tauri::command]
pub fn check_provider_limits(
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    state.db.check_provider_limits(&provider_id, &app_type)
}

/// 删除模型定价
#[tauri::command]
pub fn delete_model_pricing(state: State<'_, AppState>, model_id: String) -> Result<(), AppError> {
    crate::services::model_pricing::delete_model_pricing(&state.db, &model_id)?;
    log::info!("已删除模型定价: {model_id}");
    Ok(())
}

/// 手动触发会话日志同步
#[tauri::command]
pub async fn sync_session_usage(
    state: State<'_, AppState>,
) -> Result<crate::services::session_usage::SessionSyncResult, AppError> {
    let db = state.db.clone();
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    tauri::async_runtime::spawn_blocking(move || {
        crate::services::session_usage::sync_all_unlocked(&db)
    })
    .await
    .map_err(|error| AppError::Message(format!("会话用量同步任务失败: {error}")))
}

/// Codex reset 成功后，无论重导是否导入新行或返回错误，都必须通知前端刷新。
/// 调用方应只在 reset 成功后调用，避免把未发生的数据变更误报为重建完成。
fn finish_codex_rebuild(
    result: Result<crate::services::session_usage::SessionSyncResult, AppError>,
) -> Result<crate::services::session_usage::SessionSyncResult, AppError> {
    crate::usage_events::notify_log_recorded();
    result
}

/// 备份数据库后，仅重建 Codex session 用量。锁覆盖 backup → reset → import
/// 整个序列，避免后台同步在清理和重导之间插入数据。
#[tauri::command]
pub async fn rebuild_codex_usage(
    state: State<'_, AppState>,
) -> Result<crate::services::session_usage::SessionSyncResult, AppError> {
    let db = state.db.clone();
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    tauri::async_runtime::spawn_blocking(move || {
        db.backup_database_file()?;
        db.reset_codex_usage()?;
        let result = crate::services::session_usage_codex::sync_codex_usage(&db);
        finish_codex_rebuild(result)
    })
    .await
    .map_err(|error| AppError::Message(format!("Codex 用量重建任务失败: {error}")))?
}

/// 获取数据来源分布
#[tauri::command]
pub fn get_usage_data_sources(
    state: State<'_, AppState>,
) -> Result<Vec<crate::services::session_usage::DataSourceSummary>, AppError> {
    crate::services::session_usage::get_data_source_breakdown(&state.db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rebuild_notifies_when_reimport_is_empty() {
        crate::usage_events::take_test_notify_count();

        let result = finish_codex_rebuild(Ok(
            crate::services::session_usage::SessionSyncResult::default(),
        ))
        .expect("空重导应成功");

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
