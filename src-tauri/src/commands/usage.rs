//! 使用统计相关命令

use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::*;
use crate::store::AppState;
use rust_decimal::Decimal;
use serde::Serialize;
use std::str::FromStr;
use tauri::State;

/// 获取使用量汇总
#[tauri::command]
pub fn get_usage_summary(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    source: Option<String>,
) -> Result<UsageSummary, AppError> {
    state
        .db
        .get_usage_summary(start_date, end_date, app_type.as_deref(), source.as_deref())
}

/// 获取按 app_type 拆分的使用量汇总
#[tauri::command]
pub fn get_usage_summary_by_app(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    source: Option<String>,
) -> Result<Vec<UsageSummaryByApp>, AppError> {
    state
        .db
        .get_usage_summary_by_app(start_date, end_date, source.as_deref())
}

/// 获取每日趋势
#[tauri::command]
pub fn get_usage_trends(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    source: Option<String>,
) -> Result<Vec<DailyStats>, AppError> {
    state
        .db
        .get_daily_trends(start_date, end_date, app_type.as_deref(), source.as_deref())
}

/// 获取 Provider 统计
#[tauri::command]
pub fn get_provider_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    source: Option<String>,
) -> Result<Vec<ProviderStats>, AppError> {
    state
        .db
        .get_provider_stats(start_date, end_date, app_type.as_deref(), source.as_deref())
}

/// 获取模型统计
#[tauri::command]
pub fn get_model_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    source: Option<String>,
) -> Result<Vec<ModelStats>, AppError> {
    state
        .db
        .get_model_stats(start_date, end_date, app_type.as_deref(), source.as_deref())
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
    let model_id = model_id.trim().to_string();
    let display_name = display_name.trim().to_string();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            "模型 ID 不能为空",
            "Model ID is required",
        ));
    }
    if display_name.is_empty() {
        return Err(AppError::localized(
            "usage.displayNameRequired",
            "显示名称不能为空",
            "Display name is required",
        ));
    }

    for (label, value) in [
        ("input_cost", &input_cost),
        ("output_cost", &output_cost),
        ("cache_read_cost", &cache_read_cost),
        ("cache_creation_cost", &cache_creation_cost),
    ] {
        let parsed = Decimal::from_str(value.trim()).map_err(|e| {
            AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格无效: {value} - {e}"),
                format!("{label} price is invalid: {value} - {e}"),
            )
        })?;
        if parsed < Decimal::ZERO {
            return Err(AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格必须为非负数: {value}"),
                format!("{label} price must be non-negative: {value}"),
            ));
        }
    }

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                model_id,
                display_name,
                input_cost.trim(),
                output_cost.trim(),
                cache_read_cost.trim(),
                cache_creation_cost.trim()
            ],
        )
        .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;
    }

    if let Err(e) = db.backfill_missing_usage_costs_for_model(&model_id) {
        log::warn!("模型定价更新后回填历史用量成本失败 (model_id={model_id}): {e}");
    }

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

    // 同步 OpenCode 使用数据
    match crate::services::session_usage_opencode::sync_opencode_usage(&state.db) {
        Ok(opencode_result) => {
            result.imported += opencode_result.imported;
            result.skipped += opencode_result.skipped;
            result.files_scanned += opencode_result.files_scanned;
            result.errors.extend(opencode_result.errors);
        }
        Err(e) => {
            result.errors.push(format!("OpenCode 同步失败: {e}"));
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRemoteUsageDataResult {
    pub data_source: String,
    pub deleted_request_logs: usize,
    pub deleted_rollups: usize,
    pub deleted_sync_states: usize,
}

/// Deletes locally stored usage data for one remote source.
#[tauri::command]
pub fn delete_remote_usage_data(
    state: State<'_, AppState>,
    data_source: String,
) -> Result<DeleteRemoteUsageDataResult, AppError> {
    delete_remote_usage_data_for_source(&state.db, data_source)
}

fn delete_remote_usage_data_for_source(
    db: &Database,
    data_source: String,
) -> Result<DeleteRemoteUsageDataResult, AppError> {
    let data_source = data_source.trim().to_string();
    let host_alias = data_source
        .strip_prefix("remote:")
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "usage.sourceFilter.deleteInvalidSource",
                "只能删除远端用量数据",
                "Only remote usage data can be deleted",
            )
        })?;

    let sync_prefix = format!("remote://{host_alias}/");
    let sync_prefix_len = sync_prefix.len() as i64;
    let conn = crate::database::lock_conn!(db.conn);

    let deleted_request_logs = conn
        .execute(
            "DELETE FROM proxy_request_logs WHERE COALESCE(data_source, 'proxy') = ?1",
            rusqlite::params![data_source],
        )
        .map_err(|e| AppError::Database(format!("删除远端用量明细失败: {e}")))?;

    let deleted_rollups = conn
        .execute(
            "DELETE FROM usage_daily_rollups WHERE data_source = ?1",
            rusqlite::params![data_source],
        )
        .map_err(|e| AppError::Database(format!("删除远端用量聚合失败: {e}")))?;

    let deleted_sync_states = conn
        .execute(
            "DELETE FROM session_log_sync WHERE substr(file_path, 1, ?1) = ?2",
            rusqlite::params![sync_prefix_len, sync_prefix],
        )
        .map_err(|e| AppError::Database(format!("删除远端同步状态失败: {e}")))?;

    Ok(DeleteRemoteUsageDataResult {
        data_source,
        deleted_request_logs,
        deleted_rollups,
        deleted_sync_states,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;

    #[test]
    fn delete_remote_usage_data_removes_only_selected_host() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            for (request_id, source) in [
                ("remote-pjlab-1", "remote:pjlab"),
                ("remote-jd-1", "remote:jd"),
            ] {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        latency_ms, status_code, created_at, data_source
                    ) VALUES (?1, ?2, 'codex', 'gpt-5.4', 0, 200, 1000, ?3)",
                    rusqlite::params![request_id, "_remote:codex:test", source],
                )?;
            }
            for source in ["remote:pjlab", "remote:jd"] {
                conn.execute(
                    "INSERT INTO usage_daily_rollups (
                        date, app_type, provider_id, data_source, model
                    ) VALUES ('2026-05-22', 'codex', '_remote:codex:test', ?1, 'gpt-5.4')",
                    rusqlite::params![source],
                )?;
            }
            for path in [
                "remote://pjlab/codex/session.jsonl",
                "remote://jd/codex/session.jsonl",
            ] {
                conn.execute(
                    "INSERT INTO session_log_sync (
                        file_path, last_modified, last_line_offset, last_synced_at
                    ) VALUES (?1, 1, 2, 3)",
                    rusqlite::params![path],
                )?;
            }
        }

        let result = delete_remote_usage_data_for_source(&db, "remote:pjlab".to_string())?;
        assert_eq!(result.deleted_request_logs, 1);
        assert_eq!(result.deleted_rollups, 1);
        assert_eq!(result.deleted_sync_states, 1);

        let conn = lock_conn!(db.conn);
        let pjlab_logs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'remote:pjlab'",
            [],
            |row| row.get(0),
        )?;
        let jd_logs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'remote:jd'",
            [],
            |row| row.get(0),
        )?;
        let pjlab_sync: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path LIKE 'remote://pjlab/%'",
            [],
            |row| row.get(0),
        )?;
        let jd_sync: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path LIKE 'remote://jd/%'",
            [],
            |row| row.get(0),
        )?;

        assert_eq!(pjlab_logs, 0);
        assert_eq!(jd_logs, 1);
        assert_eq!(pjlab_sync, 0);
        assert_eq!(jd_sync, 1);
        Ok(())
    }
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
