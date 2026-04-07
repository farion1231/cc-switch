use crate::error::AppError;
use crate::services::usage_stats::{
    DailyStats, LogFilters, ModelStats, PaginatedLogs, ProviderLimitStatus, ProviderStats,
    RequestLogDetail, UsageSummary,
};
use crate::store::AppState;

use super::support::{convert, with_core_state};

fn normalize_epoch(value: Option<i64>) -> Option<i64> {
    value.map(|item| {
        if item.abs() < 100_000_000_000 {
            item * 1000
        } else {
            item
        }
    })
}

fn normalize_filters(mut filters: LogFilters) -> LogFilters {
    filters.start_date = normalize_epoch(filters.start_date);
    filters.end_date = normalize_epoch(filters.end_date);
    filters
}

pub fn legacy_get_usage_summary(
    state: &AppState,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<UsageSummary, AppError> {
    state.db.get_usage_summary(start_date, end_date)
}

pub fn get_usage_summary(
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<UsageSummary, AppError> {
    let summary = with_core_state(|state| {
        cc_switch_core::UsageService::get_detailed_summary(
            &state.db,
            normalize_epoch(start_date),
            normalize_epoch(end_date),
        )
    })?;
    convert(summary)
}

pub fn legacy_get_usage_trends(
    state: &AppState,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<DailyStats>, AppError> {
    state.db.get_daily_trends(start_date, end_date)
}

pub fn get_usage_trends(
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<DailyStats>, AppError> {
    let trends = with_core_state(|state| {
        cc_switch_core::UsageService::get_trends(
            &state.db,
            normalize_epoch(start_date),
            normalize_epoch(end_date),
        )
    })?;
    convert(trends)
}

pub fn legacy_get_provider_stats(state: &AppState) -> Result<Vec<ProviderStats>, AppError> {
    state.db.get_provider_stats()
}

pub fn get_provider_stats() -> Result<Vec<ProviderStats>, AppError> {
    let stats =
        with_core_state(|state| cc_switch_core::UsageService::get_provider_stats(&state.db))?;
    convert(stats)
}

pub fn legacy_get_model_stats(state: &AppState) -> Result<Vec<ModelStats>, AppError> {
    state.db.get_model_stats()
}

pub fn get_model_stats() -> Result<Vec<ModelStats>, AppError> {
    let stats = with_core_state(|state| cc_switch_core::UsageService::get_model_stats(&state.db))?;
    convert(stats)
}

pub fn legacy_get_request_logs(
    state: &AppState,
    filters: LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, AppError> {
    state.db.get_request_logs(&filters, page, page_size)
}

pub fn get_request_logs(
    filters: LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, AppError> {
    let filters = convert::<_, cc_switch_core::UsageLogFilters>(normalize_filters(filters))?;
    let logs = with_core_state(|state| {
        cc_switch_core::UsageService::get_logs(&state.db, &filters, page, page_size)
    })?;
    convert(logs)
}

pub fn legacy_get_request_detail(
    state: &AppState,
    request_id: &str,
) -> Result<Option<RequestLogDetail>, AppError> {
    state.db.get_request_detail(request_id)
}

pub fn get_request_detail(request_id: &str) -> Result<Option<RequestLogDetail>, AppError> {
    let detail = with_core_state(|state| {
        cc_switch_core::UsageService::get_request_detail(&state.db, request_id)
    })?;
    convert(detail)
}

pub fn legacy_get_model_pricing(
    state: &AppState,
) -> Result<Vec<crate::commands::ModelPricingInfo>, AppError> {
    let db = state.db.clone();
    state.db.ensure_model_pricing_seeded()?;
    let conn = crate::database::lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         ORDER BY display_name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(crate::commands::ModelPricingInfo {
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
    Ok(pricing)
}

pub fn get_model_pricing() -> Result<Vec<crate::commands::ModelPricingInfo>, AppError> {
    let pricing =
        with_core_state(|state| cc_switch_core::UsageService::get_model_pricing(&state.db))?;
    convert(pricing)
}

pub fn legacy_update_model_pricing(
    state: &AppState,
    pricing: crate::commands::ModelPricingInfo,
) -> Result<(), AppError> {
    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "INSERT OR REPLACE INTO model_pricing (
            model_id, display_name, input_cost_per_million, output_cost_per_million,
            cache_read_cost_per_million, cache_creation_cost_per_million
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            pricing.model_id,
            pricing.display_name,
            pricing.input_cost_per_million,
            pricing.output_cost_per_million,
            pricing.cache_read_cost_per_million,
            pricing.cache_creation_cost_per_million
        ],
    )
    .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;
    Ok(())
}

pub fn update_model_pricing(pricing: crate::commands::ModelPricingInfo) -> Result<(), AppError> {
    let pricing = convert(pricing)?;
    with_core_state(|state| cc_switch_core::UsageService::update_model_pricing(&state.db, pricing))
}

pub fn legacy_delete_model_pricing(state: &AppState, model_id: &str) -> Result<(), AppError> {
    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "DELETE FROM model_pricing WHERE model_id = ?1",
        rusqlite::params![model_id],
    )
    .map_err(|e| AppError::Database(format!("删除模型定价失败: {e}")))?;
    Ok(())
}

pub fn delete_model_pricing(model_id: &str) -> Result<(), AppError> {
    with_core_state(|state| cc_switch_core::UsageService::delete_model_pricing(&state.db, model_id))
}

pub fn legacy_check_provider_limits(
    state: &AppState,
    provider_id: &str,
    app_type: &str,
) -> Result<ProviderLimitStatus, AppError> {
    state.db.check_provider_limits(provider_id, app_type)
}

pub fn check_provider_limits(
    provider_id: &str,
    app_type: &str,
) -> Result<ProviderLimitStatus, AppError> {
    let status = with_core_state(|state| {
        cc_switch_core::UsageService::check_provider_limits(&state.db, provider_id, app_type)
    })?;
    convert(status)
}
