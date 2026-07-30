use cc_switch_protocol::protocol::MAX_FRAME_BYTES;
use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection, OptionalExtension};
use std::collections::HashMap;

use crate::CoreError;

use super::model::*;
use super::sql::fresh_input_sql;
use super::UsageQueryConnection;

pub(crate) fn summary(
    source: &impl UsageQueryConnection,
    scope: &UsageScope,
) -> Result<UsageSummary, CoreError> {
    source.with_usage_connection(|connection| {
        let filter = QueryFilter::from_scope(scope, "l");
        let fresh = fresh_input_sql("l");
        let sql = format!(
            "SELECT COUNT(*), COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM({fresh}), 0), COALESCE(SUM(l.output_tokens), 0),
                    COALESCE(SUM(l.cache_creation_tokens), 0),
                    COALESCE(SUM(l.cache_read_tokens), 0),
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0)
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {}",
            filter.where_clause()
        );
        let detail = connection
            .query_row(&sql, params_from_iter(filter.params.iter()), summary_from_row)
            .map_err(CoreError::from)?;
        let rollup = rollup_summary(connection, scope)?;
        Ok(merge_summary(detail, rollup))
    })
}

pub(crate) fn summary_by_app(
    source: &impl UsageQueryConnection,
    scope: &UsageScope,
) -> Result<Vec<UsageSummaryByApp>, CoreError> {
    source.with_usage_connection(|connection| {
        let filter = QueryFilter::from_scope_without_app(scope, "l");
        let fresh = fresh_input_sql("l");
        let app = folded_app_type("l.app_type");
        let sql = format!(
            "SELECT {app}, COUNT(*), COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM({fresh}), 0), COALESCE(SUM(l.output_tokens), 0),
                    COALESCE(SUM(l.cache_creation_tokens), 0), COALESCE(SUM(l.cache_read_tokens), 0),
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0)
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {} GROUP BY {app}",
            filter.where_clause()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
            Ok(UsageSummaryByApp {
                app_type: row.get(0)?,
                summary: summary_from_offset(row, 1)?,
            })
        })?;
        let mut values = rows.collect::<Result<Vec<_>, _>>()?;
        for rollup in rollup_summaries_by_app(connection, scope)? {
            if let Some(existing) = values
                .iter_mut()
                .find(|item| item.app_type == rollup.app_type)
            {
                existing.summary = merge_summary(existing.summary.clone(), rollup.summary);
            } else {
                values.push(rollup);
            }
        }
        values.sort_by(|left, right| {
            right
                .summary
                .real_total_tokens
                .cmp(&left.summary.real_total_tokens)
        });
        Ok(values)
    })
}

pub(crate) fn trends(
    source: &impl UsageQueryConnection,
    scope: &UsageScope,
) -> Result<Vec<DailyStats>, CoreError> {
    source.with_usage_connection(|connection| {
        let end = scope.end_date.unwrap_or_else(|| Local::now().timestamp());
        let mut start = scope.start_date.unwrap_or(end - SECONDS_PER_DAY);
        if start >= end {
            start = end - SECONDS_PER_DAY;
        }

        let bounded_scope = UsageScope {
            start_date: Some(start),
            end_date: Some(end),
            app_type: scope.app_type.clone(),
            provider_name: scope.provider_name.clone(),
            model: scope.model.clone(),
        };
        if end - start <= SECONDS_PER_DAY {
            hourly_trends(connection, &bounded_scope, start, end)
        } else {
            daily_trends(connection, &bounded_scope, start, end)
        }
    })
}

const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * SECONDS_PER_HOUR;

/// 短窗口沿用桌面 Dashboard 的小时桶，并显式补齐无请求时段以保持图表横轴稳定。
fn hourly_trends(
    connection: &Connection,
    scope: &UsageScope,
    start: i64,
    end: i64,
) -> Result<Vec<DailyStats>, CoreError> {
    let filter = QueryFilter::from_scope(scope, "l");
    let fresh = fresh_input_sql("l");
    let sql = format!(
        "SELECT CAST((l.created_at - {start}) / {SECONDS_PER_HOUR} AS INTEGER),
                COUNT(*), COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh} + l.output_tokens), 0), COALESCE(SUM({fresh}), 0),
                COALESCE(SUM(l.output_tokens), 0), COALESCE(SUM(l.cache_creation_tokens), 0),
                COALESCE(SUM(l.cache_read_tokens), 0)
         FROM proxy_request_logs l
         LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
         {} GROUP BY 1 ORDER BY 1",
        filter.where_clause()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        Ok((row.get::<_, i64>(0)?, daily_from_row(row, 1)?))
    })?;
    let mut buckets = rows.collect::<Result<HashMap<_, _>, _>>()?;
    let bucket_count = ((end - start + SECONDS_PER_HOUR - 1) / SECONDS_PER_HOUR).max(1);
    let mut values = Vec::with_capacity(bucket_count as usize);
    for index in 0..bucket_count {
        let mut item = buckets.remove(&index).unwrap_or_else(empty_daily_stats);
        item.date = local_datetime(start + index * SECONDS_PER_HOUR)?.to_rfc3339();
        values.push(item);
    }
    Ok(values)
}

/// 长窗口按本地自然日合并明细与 rollup；rollup 过滤器会排除两个不完整边界日。
fn daily_trends(
    connection: &Connection,
    scope: &UsageScope,
    start: i64,
    end: i64,
) -> Result<Vec<DailyStats>, CoreError> {
    let filter = QueryFilter::from_scope(scope, "l");
    let fresh = fresh_input_sql("l");
    let sql = format!(
        "SELECT date(l.created_at, 'unixepoch', 'localtime'), COUNT(*),
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh} + l.output_tokens), 0), COALESCE(SUM({fresh}), 0),
                COALESCE(SUM(l.output_tokens), 0), COALESCE(SUM(l.cache_creation_tokens), 0),
                COALESCE(SUM(l.cache_read_tokens), 0)
         FROM proxy_request_logs l
         LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
         {} GROUP BY 1 ORDER BY 1",
        filter.where_clause()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        Ok((row.get::<_, String>(0)?, daily_from_row(row, 1)?))
    })?;
    let mut buckets = rows.collect::<Result<HashMap<_, _>, _>>()?;
    for rollup in rollup_trends(connection, scope)? {
        if let Some(existing) = buckets.get_mut(&rollup.date) {
            merge_daily(existing, rollup);
        } else {
            buckets.insert(rollup.date.clone(), rollup);
        }
    }

    let start_day = local_datetime(start)?.date_naive();
    let end_day = local_datetime(end)?.date_naive();
    let bucket_count = end_day.signed_duration_since(start_day).num_days() + 1;
    let mut values = Vec::with_capacity(bucket_count as usize);
    let mut day = start_day;
    for _ in 0..bucket_count {
        let key = day.format("%Y-%m-%d").to_string();
        let mut item = buckets.remove(&key).unwrap_or_else(empty_daily_stats);
        item.date = local_day_start(day)?;
        values.push(item);
        day = day
            .succ_opt()
            .ok_or_else(|| CoreError::InvalidUsageRange("本地日期超出可表示范围".to_string()))?;
    }
    Ok(values)
}

fn daily_from_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<DailyStats> {
    Ok(DailyStats {
        date: String::new(),
        request_count: row.get::<_, i64>(offset)? as u64,
        total_cost: format!("{:.6}", row.get::<_, f64>(offset + 1)?),
        total_tokens: row.get::<_, i64>(offset + 2)? as u64,
        total_input_tokens: row.get::<_, i64>(offset + 3)? as u64,
        total_output_tokens: row.get::<_, i64>(offset + 4)? as u64,
        total_cache_creation_tokens: row.get::<_, i64>(offset + 5)? as u64,
        total_cache_read_tokens: row.get::<_, i64>(offset + 6)? as u64,
    })
}

fn empty_daily_stats() -> DailyStats {
    DailyStats {
        date: String::new(),
        request_count: 0,
        total_cost: "0.000000".to_string(),
        total_tokens: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_cache_creation_tokens: 0,
        total_cache_read_tokens: 0,
    }
}

fn local_datetime(timestamp: i64) -> Result<chrono::DateTime<Local>, CoreError> {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .ok_or_else(|| CoreError::InvalidUsageRange(format!("无法解析本地时间戳 {timestamp}")))
}

fn local_day_start(day: NaiveDate) -> Result<String, CoreError> {
    let naive = day
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| CoreError::InvalidUsageRange(format!("无法构造本地日期 {day}")))?;
    let datetime = Local
        .from_local_datetime(&naive)
        .earliest()
        .ok_or_else(|| CoreError::InvalidUsageRange(format!("本地日期不存在 {day}")))?;
    Ok(datetime.to_rfc3339())
}

pub(crate) fn provider_stats(
    source: &impl UsageQueryConnection,
    scope: &UsageScope,
) -> Result<Vec<ProviderStats>, CoreError> {
    source.with_usage_connection(|connection| {
        let filter = QueryFilter::from_scope(scope, "l");
        let fresh = fresh_input_sql("l");
        let provider_name = provider_name_sql("l", "p");
        let sql = format!(
            "SELECT l.provider_id, l.app_type, {provider_name}, COUNT(*),
                    COALESCE(SUM({fresh} + l.output_tokens), 0),
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0),
                    COALESCE(AVG(l.latency_ms), 0)
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {} GROUP BY l.provider_id, l.app_type ORDER BY 6 DESC",
            filter.where_clause()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
            let requests = row.get::<_, i64>(3)?;
            let successes = row.get::<_, i64>(6)?;
            Ok(ProviderStatsByApp {
                app_type: row.get(1)?,
                stats: ProviderStats {
                    provider_id: row.get(0)?,
                    provider_name: row.get(2)?,
                    request_count: requests as u64,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                    total_cost: format!("{:.6}", row.get::<_, f64>(5)?),
                    success_rate: percentage(successes, requests),
                    avg_latency_ms: row.get::<_, f64>(7)? as u64,
                },
            })
        })?;
        let mut values = rows.collect::<Result<Vec<_>, _>>()?;
        for rollup in rollup_provider_stats(connection, scope)? {
            if let Some(existing) = values
                .iter_mut()
                .find(|item| {
                    item.app_type == rollup.app_type
                        && item.stats.provider_id == rollup.stats.provider_id
                })
            {
                merge_provider(&mut existing.stats, rollup.stats);
            } else {
                values.push(rollup);
            }
        }
        values.sort_by(|left, right| {
            cost(&right.stats.total_cost).total_cmp(&cost(&left.stats.total_cost))
        });
        Ok(values.into_iter().map(|item| item.stats).collect())
    })
}

pub(crate) fn model_stats(
    source: &impl UsageQueryConnection,
    scope: &UsageScope,
) -> Result<Vec<ModelStats>, CoreError> {
    source.with_usage_connection(|connection| {
        let filter = QueryFilter::from_scope(scope, "l");
        let fresh = fresh_input_sql("l");
        let model = effective_model("l");
        let sql = format!(
            "SELECT {model}, COUNT(*), COALESCE(SUM({fresh} + l.output_tokens), 0),
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {} GROUP BY {model} ORDER BY 4 DESC",
            filter.where_clause()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
            let requests = row.get::<_, i64>(1)?;
            let cost = row.get::<_, f64>(3)?;
            Ok(ModelStats {
                model: row.get(0)?,
                request_count: requests as u64,
                total_tokens: row.get::<_, i64>(2)? as u64,
                total_cost: format!("{cost:.6}"),
                avg_cost_per_request: format!(
                    "{:.6}",
                    if requests > 0 {
                        cost / requests as f64
                    } else {
                        0.0
                    }
                ),
            })
        })?;
        let mut values = rows.collect::<Result<Vec<_>, _>>()?;
        for rollup in rollup_model_stats(connection, scope)? {
            if let Some(existing) = values.iter_mut().find(|item| item.model == rollup.model) {
                merge_model(existing, rollup);
            } else {
                values.push(rollup);
            }
        }
        values.sort_by(|left, right| cost(&right.total_cost).total_cmp(&cost(&left.total_cost)));
        Ok(values)
    })
}

pub(crate) fn logs(
    source: &impl UsageQueryConnection,
    filters: &LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, CoreError> {
    source.with_usage_connection(|connection| {
        let filter = QueryFilter::from_log_filters(filters, "l");
        let join = "LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type";
        let count_sql = format!(
            "SELECT COUNT(*) FROM proxy_request_logs l {join} {}",
            filter.where_clause()
        );
        let total =
            connection.query_row(&count_sql, params_from_iter(filter.params.iter()), |row| {
                row.get::<_, i64>(0)
            })? as u32;
        let provider_name = provider_name_sql("l", "p");
        let sql = format!(
            "{} {join} {} ORDER BY l.created_at DESC LIMIT ? OFFSET ?",
            detail_select(&provider_name),
            filter.where_clause()
        );
        let mut params = filter.params;
        params.push(SqlValue::Integer(page_size as i64));
        params.push(SqlValue::Integer((page as u64 * page_size as u64) as i64));
        let mut statement = connection.prepare(&sql)?;
        let rows =
            statement.query_map(params_from_iter(params.iter()), row_to_request_log_detail)?;
        let result = PaginatedLogs {
            data: rows.collect::<Result<Vec<_>, _>>()?,
            total,
            page,
            page_size,
        };
        let actual = serde_json::to_vec(&result)?.len();
        if actual > MAX_FRAME_BYTES {
            return Err(CoreError::PayloadTooLarge {
                actual,
                limit: MAX_FRAME_BYTES,
            });
        }
        Ok(result)
    })
}

pub(crate) fn detail(
    source: &impl UsageQueryConnection,
    request_id: &str,
) -> Result<Option<RequestLogDetail>, CoreError> {
    source.with_usage_connection(|connection| {
        let provider_name = provider_name_sql("l", "p");
        let sql = format!(
            "{} LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE l.request_id = ?1",
            detail_select(&provider_name)
        );
        connection
            .query_row(&sql, [request_id], row_to_request_log_detail)
            .optional()
            .map_err(CoreError::from)
    })
}

pub(crate) fn data_sources(
    source: &impl UsageQueryConnection,
) -> Result<Vec<DataSourceSummary>, CoreError> {
    source.with_usage_connection(|connection| {
        let effective_filter = effective_usage_log_filter("l");
        let sql = format!(
            "SELECT COALESCE(l.data_source, 'proxy'), COUNT(*),
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs l WHERE {effective_filter}
             GROUP BY COALESCE(l.data_source, 'proxy') ORDER BY 2 DESC"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok(DataSourceSummary {
                data_source: row.get(0)?,
                request_count: row.get::<_, i64>(1)? as u32,
                total_cost_usd: format!("{:.6}", row.get::<_, f64>(2)?),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

pub(crate) fn pricing(
    source: &impl UsageQueryConnection,
) -> Result<Vec<ModelPricingInfo>, CoreError> {
    source.with_usage_connection(|connection| {
        let mut statement = connection.prepare(
            "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing ORDER BY display_name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ModelPricingInfo {
                model_id: row.get(0)?,
                display_name: row.get(1)?,
                input_cost_per_million: row.get(2)?,
                output_cost_per_million: row.get(3)?,
                cache_read_cost_per_million: row.get(4)?,
                cache_creation_cost_per_million: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

fn rollup_summary(connection: &Connection, scope: &UsageScope) -> Result<UsageSummary, CoreError> {
    let filter = RollupFilter::from_scope(scope, "r", true);
    let fresh = fresh_input_sql("r");
    let sql = format!(
        "SELECT COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh}), 0), COALESCE(SUM(r.output_tokens), 0),
                COALESCE(SUM(r.cache_creation_tokens), 0), COALESCE(SUM(r.cache_read_tokens), 0),
                COALESCE(SUM(r.success_count), 0)
         FROM usage_daily_rollups r
         LEFT JOIN providers p ON r.provider_id = p.id AND r.app_type = p.app_type
         {}",
        filter.where_clause()
    );
    connection
        .query_row(
            &sql,
            params_from_iter(filter.params.iter()),
            summary_from_row,
        )
        .map_err(CoreError::from)
}

fn rollup_summaries_by_app(
    connection: &Connection,
    scope: &UsageScope,
) -> Result<Vec<UsageSummaryByApp>, CoreError> {
    let filter = RollupFilter::from_scope(scope, "r", false);
    let fresh = fresh_input_sql("r");
    let app = folded_app_type("r.app_type");
    let sql = format!(
        "SELECT {app}, COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0), COALESCE(SUM({fresh}), 0),
                COALESCE(SUM(r.output_tokens), 0), COALESCE(SUM(r.cache_creation_tokens), 0),
                COALESCE(SUM(r.cache_read_tokens), 0), COALESCE(SUM(r.success_count), 0)
         FROM usage_daily_rollups r
         LEFT JOIN providers p ON r.provider_id = p.id AND r.app_type = p.app_type
         {} GROUP BY {app}",
        filter.where_clause()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        Ok(UsageSummaryByApp {
            app_type: row.get(0)?,
            summary: summary_from_offset(row, 1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn rollup_trends(
    connection: &Connection,
    scope: &UsageScope,
) -> Result<Vec<DailyStats>, CoreError> {
    let filter = RollupFilter::from_scope(scope, "r", true);
    let fresh = fresh_input_sql("r");
    let sql = format!(
        "SELECT r.date, COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh} + r.output_tokens), 0), COALESCE(SUM({fresh}), 0),
                COALESCE(SUM(r.output_tokens), 0), COALESCE(SUM(r.cache_creation_tokens), 0),
                COALESCE(SUM(r.cache_read_tokens), 0)
         FROM usage_daily_rollups r
         LEFT JOIN providers p ON r.provider_id = p.id AND r.app_type = p.app_type
         {} GROUP BY r.date ORDER BY r.date",
        filter.where_clause()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        Ok(DailyStats {
            date: row.get(0)?,
            request_count: row.get::<_, i64>(1)? as u64,
            total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
            total_tokens: row.get::<_, i64>(3)? as u64,
            total_input_tokens: row.get::<_, i64>(4)? as u64,
            total_output_tokens: row.get::<_, i64>(5)? as u64,
            total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
            total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn rollup_provider_stats(
    connection: &Connection,
    scope: &UsageScope,
) -> Result<Vec<ProviderStatsByApp>, CoreError> {
    let filter = RollupFilter::from_scope(scope, "r", true);
    let fresh = fresh_input_sql("r");
    let provider_name = provider_name_sql("r", "p");
    let sql = format!(
        "SELECT r.provider_id, r.app_type, {provider_name}, COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM({fresh} + r.output_tokens), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                COALESCE(SUM(r.success_count), 0),
                CASE WHEN SUM(r.request_count) > 0
                     THEN SUM(r.avg_latency_ms * r.request_count) / SUM(r.request_count) ELSE 0 END
         FROM usage_daily_rollups r
         LEFT JOIN providers p ON r.provider_id = p.id AND r.app_type = p.app_type
         {} GROUP BY r.provider_id, r.app_type",
        filter.where_clause()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        let requests = row.get::<_, i64>(3)?;
        let successes = row.get::<_, i64>(6)?;
        Ok(ProviderStatsByApp {
            app_type: row.get(1)?,
            stats: ProviderStats {
                provider_id: row.get(0)?,
                provider_name: row.get(2)?,
                request_count: requests as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                total_cost: format!("{:.6}", row.get::<_, f64>(5)?),
                success_rate: percentage(successes, requests),
                avg_latency_ms: row.get::<_, i64>(7)? as u64,
            },
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Provider DTO 不暴露 app_type，但合并明细与 rollup 时必须保留该键，防止跨应用同 ID 串行。
struct ProviderStatsByApp {
    app_type: String,
    stats: ProviderStats,
}

fn rollup_model_stats(
    connection: &Connection,
    scope: &UsageScope,
) -> Result<Vec<ModelStats>, CoreError> {
    let filter = RollupFilter::from_scope(scope, "r", true);
    let fresh = fresh_input_sql("r");
    let model = effective_model("r");
    let sql = format!(
        "SELECT {model}, COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM({fresh} + r.output_tokens), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0)
         FROM usage_daily_rollups r
         LEFT JOIN providers p ON r.provider_id = p.id AND r.app_type = p.app_type
         {} GROUP BY {model}",
        filter.where_clause()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(filter.params.iter()), |row| {
        let requests = row.get::<_, i64>(1)?;
        let total_cost = row.get::<_, f64>(3)?;
        Ok(ModelStats {
            model: row.get(0)?,
            request_count: requests as u64,
            total_tokens: row.get::<_, i64>(2)? as u64,
            total_cost: format!("{total_cost:.6}"),
            avg_cost_per_request: format!(
                "{:.6}",
                if requests > 0 {
                    total_cost / requests as f64
                } else {
                    0.0
                }
            ),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn merge_summary(left: UsageSummary, right: UsageSummary) -> UsageSummary {
    let requests = left.total_requests + right.total_requests;
    let successes = success_count(&left) + success_count(&right);
    let input = left.total_input_tokens + right.total_input_tokens;
    let output = left.total_output_tokens + right.total_output_tokens;
    let cache_creation = left.total_cache_creation_tokens + right.total_cache_creation_tokens;
    let cache_read = left.total_cache_read_tokens + right.total_cache_read_tokens;
    let cacheable = input + cache_creation + cache_read;
    UsageSummary {
        total_requests: requests,
        total_cost: format!("{:.6}", cost(&left.total_cost) + cost(&right.total_cost)),
        total_input_tokens: input,
        total_output_tokens: output,
        total_cache_creation_tokens: cache_creation,
        total_cache_read_tokens: cache_read,
        success_rate: percentage(successes as i64, requests as i64),
        real_total_tokens: input + output + cache_creation + cache_read,
        cache_hit_rate: if cacheable > 0 {
            cache_read as f64 / cacheable as f64
        } else {
            0.0
        },
    }
}

fn success_count(summary: &UsageSummary) -> u64 {
    ((summary.success_rate as f64 / 100.0) * summary.total_requests as f64).round() as u64
}

fn merge_daily(target: &mut DailyStats, incoming: DailyStats) {
    target.request_count += incoming.request_count;
    target.total_cost = format!(
        "{:.6}",
        cost(&target.total_cost) + cost(&incoming.total_cost)
    );
    target.total_tokens += incoming.total_tokens;
    target.total_input_tokens += incoming.total_input_tokens;
    target.total_output_tokens += incoming.total_output_tokens;
    target.total_cache_creation_tokens += incoming.total_cache_creation_tokens;
    target.total_cache_read_tokens += incoming.total_cache_read_tokens;
}

fn merge_provider(target: &mut ProviderStats, incoming: ProviderStats) {
    let old_requests = target.request_count;
    let total_requests = old_requests + incoming.request_count;
    let successes = ((target.success_rate as f64 / 100.0) * old_requests as f64
        + (incoming.success_rate as f64 / 100.0) * incoming.request_count as f64)
        .round() as i64;
    let latency_sum =
        target.avg_latency_ms * old_requests + incoming.avg_latency_ms * incoming.request_count;
    target.request_count = total_requests;
    target.total_tokens += incoming.total_tokens;
    target.total_cost = format!(
        "{:.6}",
        cost(&target.total_cost) + cost(&incoming.total_cost)
    );
    target.success_rate = percentage(successes, total_requests as i64);
    // 平均延迟必须按请求数加权；空集合通过 checked_div 明确回落为 0，避免维护时引入除零分支偏差。
    target.avg_latency_ms = latency_sum.checked_div(total_requests).unwrap_or(0);
}

fn merge_model(target: &mut ModelStats, incoming: ModelStats) {
    target.request_count += incoming.request_count;
    target.total_tokens += incoming.total_tokens;
    let total_cost = cost(&target.total_cost) + cost(&incoming.total_cost);
    target.total_cost = format!("{total_cost:.6}");
    target.avg_cost_per_request = format!(
        "{:.6}",
        if target.request_count > 0 {
            total_cost / target.request_count as f64
        } else {
            0.0
        }
    );
}

fn cost(value: &str) -> f64 {
    value.parse().unwrap_or(0.0)
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSummary> {
    summary_from_offset(row, 0)
}

fn summary_from_offset(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<UsageSummary> {
    let requests = row.get::<_, i64>(offset)?;
    let input = row.get::<_, i64>(offset + 2)? as u64;
    let output = row.get::<_, i64>(offset + 3)? as u64;
    let cache_creation = row.get::<_, i64>(offset + 4)? as u64;
    let cache_read = row.get::<_, i64>(offset + 5)? as u64;
    let successes = row.get::<_, i64>(offset + 6)?;
    let cacheable = input + cache_creation + cache_read;
    Ok(UsageSummary {
        total_requests: requests as u64,
        total_cost: format!("{:.6}", row.get::<_, f64>(offset + 1)?),
        total_input_tokens: input,
        total_output_tokens: output,
        total_cache_creation_tokens: cache_creation,
        total_cache_read_tokens: cache_read,
        success_rate: percentage(successes, requests),
        real_total_tokens: input + output + cache_creation + cache_read,
        cache_hit_rate: if cacheable > 0 {
            cache_read as f64 / cacheable as f64
        } else {
            0.0
        },
    })
}

fn percentage(numerator: i64, denominator: i64) -> f32 {
    if denominator > 0 {
        numerator as f32 / denominator as f32 * 100.0
    } else {
        0.0
    }
}

/// 详情 SELECT 固定返回 26 列；列顺序与桌面 RequestLogDetail 完全一致，修改时必须同步 mapper。
fn detail_select(provider_name: &str) -> String {
    format!(
        "SELECT l.request_id, l.provider_id, {provider_name}, l.app_type, l.model,
                l.request_model, l.cost_multiplier, l.input_tokens, l.output_tokens,
                l.cache_read_tokens, l.cache_creation_tokens, l.input_cost_usd,
                l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd,
                l.total_cost_usd, l.is_streaming, l.latency_ms, l.first_token_ms,
                l.duration_ms, l.status_code, l.error_message, l.created_at,
                COALESCE(l.data_source, 'proxy'), l.pricing_model, l.input_token_semantics
         FROM proxy_request_logs l"
    )
}

fn row_to_request_log_detail(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestLogDetail> {
    Ok(RequestLogDetail {
        request_id: row.get(0)?,
        provider_id: row.get(1)?,
        provider_name: row.get(2)?,
        app_type: row.get(3)?,
        model: row.get(4)?,
        request_model: row.get(5)?,
        cost_multiplier: row
            .get::<_, Option<String>>(6)?
            .unwrap_or_else(|| "1".to_string()),
        input_tokens: row.get::<_, i64>(7)? as u32,
        output_tokens: row.get::<_, i64>(8)? as u32,
        cache_read_tokens: row.get::<_, i64>(9)? as u32,
        cache_creation_tokens: row.get::<_, i64>(10)? as u32,
        input_cost_usd: row.get(11)?,
        output_cost_usd: row.get(12)?,
        cache_read_cost_usd: row.get(13)?,
        cache_creation_cost_usd: row.get(14)?,
        total_cost_usd: row.get(15)?,
        is_streaming: row.get::<_, i64>(16)? != 0,
        latency_ms: row.get::<_, i64>(17)? as u64,
        first_token_ms: row.get::<_, Option<i64>>(18)?.map(|value| value as u64),
        duration_ms: row.get::<_, Option<i64>>(19)?.map(|value| value as u64),
        status_code: row.get::<_, i64>(20)? as u16,
        error_message: row.get(21)?,
        created_at: row.get(22)?,
        data_source: row.get(23)?,
        pricing_model: row.get(24)?,
        input_token_semantics: row.get(25)?,
    })
}

fn provider_name_sql(log_alias: &str, provider_alias: &str) -> String {
    format!(
        "COALESCE({provider_alias}.name, CASE {log_alias}.provider_id
         WHEN '_session' THEN 'Claude (Session)'
         WHEN '_codex_session' THEN 'Codex (Session)'
         WHEN '_gemini_session' THEN 'Gemini (Session)'
         WHEN '_opencode_session' THEN 'OpenCode (Session)'
         WHEN '_grok_session' THEN 'Grok Build (Session)'
         ELSE {log_alias}.provider_id END)"
    )
}

fn effective_model(alias: &str) -> String {
    format!("COALESCE(NULLIF({alias}.pricing_model, ''), {alias}.model)")
}

fn folded_app_type(column: &str) -> String {
    format!("CASE WHEN {column} = 'claude-desktop' THEN 'claude' ELSE {column} END")
}

struct QueryFilter {
    conditions: Vec<String>,
    params: Vec<SqlValue>,
}

impl QueryFilter {
    fn from_scope(scope: &UsageScope, alias: &str) -> Self {
        Self::build(scope, alias, true)
    }

    fn from_scope_without_app(scope: &UsageScope, alias: &str) -> Self {
        Self::build(scope, alias, false)
    }

    fn build(scope: &UsageScope, alias: &str, include_app: bool) -> Self {
        let mut filter = Self {
            conditions: vec![effective_usage_log_filter(alias)],
            params: Vec::new(),
        };
        if let Some(start) = scope.start_date {
            filter.push(format!("{alias}.created_at >= ?"), start.into());
        }
        if let Some(end) = scope.end_date {
            filter.push(format!("{alias}.created_at <= ?"), end.into());
        }
        if include_app {
            if let Some(app) = &scope.app_type {
                filter.push(
                    format!("{} = ?", folded_app_type(&format!("{alias}.app_type"))),
                    app.clone().into(),
                );
            }
        }
        filter.push_provider_model(
            alias,
            scope.provider_name.as_deref(),
            scope.model.as_deref(),
        );
        filter
    }

    fn from_log_filters(filters: &LogFilters, alias: &str) -> Self {
        let scope = UsageScope {
            start_date: filters.start_date,
            end_date: filters.end_date,
            app_type: filters.app_type.clone(),
            provider_name: filters.provider_name.clone(),
            model: filters.model.clone(),
        };
        let mut filter = Self::from_scope(&scope, alias);
        if let Some(status) = filters.status_code {
            filter.push(format!("{alias}.status_code = ?"), (status as i64).into());
        }
        filter
    }

    fn push_provider_model(
        &mut self,
        alias: &str,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) {
        if let Some(provider_name) = provider_name {
            self.push(
                format!("{} = ?", provider_name_sql(alias, "p")),
                provider_name.to_string().into(),
            );
        }
        if let Some(model) = model {
            self.push(
                format!("{} = ?", effective_model(alias)),
                model.to_string().into(),
            );
        }
    }

    fn push(&mut self, condition: String, value: SqlValue) {
        self.conditions.push(condition);
        self.params.push(value);
    }

    fn where_clause(&self) -> String {
        if self.conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.conditions.join(" AND "))
        }
    }
}

/// 日汇总表只能代表完整的本地自然日；边界落在半天时必须排除该日，
/// 否则会把边界日的 rollup 与仍保留的请求明细重复计入 Dashboard。
struct RollupFilter {
    conditions: Vec<String>,
    params: Vec<SqlValue>,
}

impl RollupFilter {
    fn from_scope(scope: &UsageScope, alias: &str, include_app: bool) -> Self {
        let mut filter = Self {
            conditions: Vec::new(),
            params: Vec::new(),
        };

        if let Some(start) = scope.start_date {
            filter.conditions.push(format!(
                "{alias}.date >= CASE
                    WHEN strftime('%H:%M:%S', ?, 'unixepoch', 'localtime') = '00:00:00'
                    THEN date(?, 'unixepoch', 'localtime')
                    ELSE date(?, 'unixepoch', 'localtime', '+1 day')
                 END"
            ));
            filter.push_timestamp_triplet(start);
        }
        if let Some(end) = scope.end_date {
            filter.conditions.push(format!(
                "{alias}.date <= CASE
                    WHEN strftime('%H:%M', ?, 'unixepoch', 'localtime') = '23:59'
                    THEN date(?, 'unixepoch', 'localtime')
                    ELSE date(?, 'unixepoch', 'localtime', '-1 day')
                 END"
            ));
            filter.push_timestamp_triplet(end);
        }
        if include_app {
            if let Some(app) = &scope.app_type {
                filter.push(
                    format!("{} = ?", folded_app_type(&format!("{alias}.app_type"))),
                    app.clone().into(),
                );
            }
        }
        if let Some(provider_name) = &scope.provider_name {
            filter.push(
                format!("{} = ?", provider_name_sql(alias, "p")),
                provider_name.clone().into(),
            );
        }
        if let Some(model) = &scope.model {
            filter.push(
                format!("{} = ?", effective_model(alias)),
                model.clone().into(),
            );
        }
        filter
    }

    /// SQLite 需要在同一条件中分别判断时刻和生成日期，因此一个时间戳按占位符顺序绑定三次。
    fn push_timestamp_triplet(&mut self, timestamp: i64) {
        self.params.extend([
            SqlValue::Integer(timestamp),
            SqlValue::Integer(timestamp),
            SqlValue::Integer(timestamp),
        ]);
    }

    fn push(&mut self, condition: String, value: SqlValue) {
        self.conditions.push(condition);
        self.params.push(value);
    }

    fn where_clause(&self) -> String {
        if self.conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.conditions.join(" AND "))
        }
    }
}

const SESSION_PROXY_DEDUP_WINDOW_SECONDS: i64 = 10 * 60;

fn effective_usage_log_filter(log_alias: &str) -> String {
    let data_source = format!("COALESCE({log_alias}.data_source, 'proxy')");
    let proxy_source = "COALESCE(proxy_dedup.data_source, 'proxy')";
    format!(
        "NOT (
            {data_source} IN ('session_log', 'codex_session', 'gemini_session', 'opencode_session')
            AND EXISTS (
                SELECT 1 FROM proxy_request_logs proxy_dedup
                WHERE {proxy_source} = 'proxy'
                  AND proxy_dedup.app_type = {log_alias}.app_type
                  AND proxy_dedup.status_code >= 200 AND proxy_dedup.status_code < 300
                  AND proxy_dedup.input_tokens = {log_alias}.input_tokens
                  AND proxy_dedup.output_tokens = {log_alias}.output_tokens
                  AND proxy_dedup.cache_read_tokens = {log_alias}.cache_read_tokens
                  AND (
                      proxy_dedup.cache_creation_tokens = {log_alias}.cache_creation_tokens
                      OR (
                          {log_alias}.cache_creation_tokens = 0
                          AND {data_source} IN ('codex_session', 'gemini_session', 'opencode_session')
                      )
                  )
                  AND proxy_dedup.created_at BETWEEN
                      {log_alias}.created_at - {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                      AND {log_alias}.created_at + {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                  AND (
                      LOWER(proxy_dedup.model) = LOWER({log_alias}.model)
                      OR LOWER(proxy_dedup.model) = 'unknown'
                      OR LOWER({log_alias}.model) = 'unknown'
                  )
            )
        )"
    )
}
