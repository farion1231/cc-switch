//! 使用统计相关命令

use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::*;
use tauri::State;

/// 获取使用量汇总
#[tauri::command]
pub fn get_usage_summary(
    db: State<'_, Database>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<UsageSummary, AppError> {
    db.get_usage_summary(start_date, end_date)
}

/// 获取每日趋势
#[tauri::command]
pub fn get_usage_trends(
    db: State<'_, Database>,
    days: u32,
) -> Result<Vec<DailyStats>, AppError> {
    db.get_daily_trends(days)
}

/// 获取 Provider 统计
#[tauri::command]
pub fn get_provider_stats(db: State<'_, Database>) -> Result<Vec<ProviderStats>, AppError> {
    db.get_provider_stats()
}

/// 获取模型统计
#[tauri::command]
pub fn get_model_stats(db: State<'_, Database>) -> Result<Vec<ModelStats>, AppError> {
    db.get_model_stats()
}

/// 获取请求日志列表
#[tauri::command]
pub fn get_request_logs(
    db: State<'_, Database>,
    provider_id: Option<String>,
    model: Option<String>,
    status_code: Option<u16>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    limit: u32,
    offset: u32,
) -> Result<Vec<RequestLogDetail>, AppError> {
    let filters = LogFilters {
        provider_id,
        model,
        status_code,
        start_date,
        end_date,
    };
    db.get_request_logs(&filters, limit, offset)
}

/// 获取单个请求详情
#[tauri::command]
pub fn get_request_detail(
    db: State<'_, Database>,
    request_id: String,
) -> Result<Option<RequestLogDetail>, AppError> {
    db.get_request_detail(&request_id)
}

/// 获取模型定价列表
#[tauri::command]
pub fn get_model_pricing(db: State<'_, Database>) -> Result<Vec<ModelPricingInfo>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    
    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         ORDER BY display_name"
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

    Ok(pricing)
}

/// 更新模型定价
#[tauri::command]
pub fn update_model_pricing(
    db: State<'_, Database>,
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<(), AppError> {
    let conn = crate::database::lock_conn!(db.conn);

    conn.execute(
        "INSERT OR REPLACE INTO model_pricing (
            model_id, display_name, input_cost_per_million, output_cost_per_million,
            cache_read_cost_per_million, cache_creation_cost_per_million
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            model_id,
            display_name,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost
        ],
    )
    .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;

    Ok(())
}

/// 检查 Provider 使用限额
#[tauri::command]
pub fn check_provider_limits(
    db: State<'_, Database>,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    db.check_provider_limits(&provider_id, &app_type)
}

/// 模型定价信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelPricingInfo {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
}
