//! 使用统计相关命令

use crate::database::Database;
use crate::error::AppError;
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
) -> Result<UsageSummary, AppError> {
    state
        .db
        .get_usage_summary(start_date, end_date, app_type.as_deref())
}

/// 获取每日趋势
#[tauri::command]
pub fn get_usage_trends(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<Vec<DailyStats>, AppError> {
    state
        .db
        .get_daily_trends(start_date, end_date, app_type.as_deref())
}

/// 获取 Provider 统计
#[tauri::command]
pub fn get_provider_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<Vec<ProviderStats>, AppError> {
    state
        .db
        .get_provider_stats(start_date, end_date, app_type.as_deref())
}

/// 获取模型统计
#[tauri::command]
pub fn get_model_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<Vec<ModelStats>, AppError> {
    state
        .db
        .get_model_stats(start_date, end_date, app_type.as_deref())
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
    let db = state.db.clone();
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
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    state.db.check_provider_limits(&provider_id, &app_type)
}

/// 删除模型定价
#[tauri::command]
pub fn delete_model_pricing(state: State<'_, AppState>, model_id: String) -> Result<(), AppError> {
    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);

    conn.execute(
        "DELETE FROM model_pricing WHERE model_id = ?1",
        rusqlite::params![model_id],
    )
    .map_err(|e| AppError::Database(format!("删除模型定价失败: {e}")))?;

    log::info!("已删除模型定价: {model_id}");
    Ok(())
}

/// 手动回填历史记录中缺失的成本
#[tauri::command]
pub fn backfill_missing_usage_costs(
    state: State<'_, AppState>,
) -> Result<BackfillUsageCostsResult, AppError> {
    backfill_missing_usage_costs_for_db(state.db.as_ref())
}

fn backfill_missing_usage_costs_for_db(
    db: &Database,
) -> Result<BackfillUsageCostsResult, AppError> {
    let backfilled_cost_rows = db.backfill_missing_usage_costs()?;
    Ok(BackfillUsageCostsResult {
        backfilled_cost_rows,
    })
}

/// 手动触发会话日志同步
#[tauri::command]
pub fn sync_session_usage(
    state: State<'_, AppState>,
) -> Result<crate::services::session_usage::SessionSyncResult, AppError> {
    // 同步 Claude 会话日志
    let mut result = crate::services::session_usage::sync_claude_session_logs(&state.db)?;

    // 同步 Codex 使用数据
    match crate::services::session_usage_codex::sync_codex_usage(&state.db) {
        Ok(codex_result) => {
            result.imported += codex_result.imported;
            result.skipped += codex_result.skipped;
            result.files_scanned += codex_result.files_scanned;
            result.errors.extend(codex_result.errors);
        }
        Err(e) => {
            result.errors.push(format!("Codex 同步失败: {e}"));
        }
    }

    // 同步 Gemini 使用数据
    match crate::services::session_usage_gemini::sync_gemini_usage(&state.db) {
        Ok(gemini_result) => {
            result.imported += gemini_result.imported;
            result.skipped += gemini_result.skipped;
            result.files_scanned += gemini_result.files_scanned;
            result.errors.extend(gemini_result.errors);
        }
        Err(e) => {
            result.errors.push(format!("Gemini 同步失败: {e}"));
        }
    }

    Ok(result)
}

/// 获取数据来源分布
#[tauri::command]
pub fn get_usage_data_sources(
    state: State<'_, AppState>,
) -> Result<Vec<crate::services::session_usage::DataSourceSummary>, AppError> {
    crate::services::session_usage::get_data_source_breakdown(&state.db)
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

/// 历史成本回填结果
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillUsageCostsResult {
    pub backfilled_cost_rows: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{lock_conn, Database};
    use rusqlite::params;

    #[allow(clippy::too_many_arguments)]
    fn insert_usage_log(
        db: &Database,
        request_id: &str,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_creation_tokens: i64,
        total_cost_usd: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, created_at, data_source
            ) VALUES (?1, '_codex_session', 'codex', ?2, ?2, ?3, ?4, ?5, ?6,
                      '0', '0', '0', '0', ?7, 100, 200, 1000, 'codex_session')",
            params![
                request_id,
                model,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                total_cost_usd
            ],
        )?;
        Ok(())
    }

    fn insert_model_pricing(
        db: &Database,
        model_id: &str,
        input_cost: &str,
        output_cost: &str,
        cache_read_cost: &str,
        cache_creation_cost: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million,
                output_cost_per_million, cache_read_cost_per_million,
                cache_creation_cost_per_million
            ) VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
            params![
                model_id,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost
            ],
        )?;
        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_for_db_reprices_all_known_zero_cost_models(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;

        insert_model_pricing(&db, "priced-model-a", "5", "30", "0.5", "5")?;
        insert_model_pricing(&db, "priced-model-b", "2", "10", "0", "0")?;

        insert_usage_log(
            &db,
            "priced-model-a-zero-cost",
            "priced-model-a",
            1_000_000,
            100_000,
            200_000,
            50_000,
            "0",
        )?;
        insert_usage_log(
            &db,
            "priced-model-a-existing-cost",
            "priced-model-a",
            1_000_000,
            100_000,
            200_000,
            50_000,
            "123.000000",
        )?;
        insert_usage_log(
            &db,
            "priced-model-b-zero-cost",
            "priced-model-b",
            2_000_000,
            0,
            0,
            0,
            "0",
        )?;
        insert_usage_log(
            &db,
            "unknown-model-zero-cost",
            "unknown-new-model",
            1_000_000,
            100_000,
            200_000,
            50_000,
            "0",
        )?;

        let result = backfill_missing_usage_costs_for_db(&db)?;

        assert_eq!(result.backfilled_cost_rows, 2);

        let conn = lock_conn!(db.conn);
        let priced_model_a_cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'priced-model-a-zero-cost'",
            [],
            |row| row.get(0),
        )?;
        let existing_cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'priced-model-a-existing-cost'",
            [],
            |row| row.get(0),
        )?;
        let priced_model_b_cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'priced-model-b-zero-cost'",
            [],
            |row| row.get(0),
        )?;
        let other_model_cost: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'unknown-model-zero-cost'",
            [],
            |row| row.get(0),
        )?;

        assert_eq!(priced_model_a_cost, "7.350000");
        assert_eq!(priced_model_b_cost, "4.000000");
        assert_eq!(existing_cost, "123.000000");
        assert_eq!(other_model_cost, "0");

        Ok(())
    }
}
