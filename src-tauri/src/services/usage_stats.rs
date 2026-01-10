//! 使用统计服务
//!
//! 提供使用量数据的聚合查询功能

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use chrono::{Local, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;

/// 使用量汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_requests: u64,
    pub total_cost: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub success_rate: f32,
}

/// 每日统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStats {
    pub date: String,
    pub request_count: u64,
    pub total_cost: String,
    pub total_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

/// Provider 统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStats {
    pub provider_id: String,
    pub provider_name: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    pub success_rate: f32,
    pub avg_latency_ms: u64,
}

/// 模型统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    pub model: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    pub avg_cost_per_request: String,
}

/// 请求日志过滤器
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilters {
    pub app_type: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<u16>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
}

/// 分页请求日志响应
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedLogs {
    pub data: Vec<RequestLogDetail>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

/// 请求日志详情
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogDetail {
    pub request_id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub app_type: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub total_cost_usd: String,
    pub is_streaming: bool,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub created_at: i64,
}

impl Database {
    /// 获取使用量汇总
    pub fn get_usage_summary(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);

        let (where_clause, params_vec) = if start_date.is_some() || end_date.is_some() {
            let mut conditions = Vec::new();
            let mut params = Vec::new();

            if let Some(start) = start_date {
                conditions.push("created_at >= ?");
                params.push(start);
            }
            if let Some(end) = end_date {
                conditions.push("created_at <= ?");
                params.push(end);
            }

            (format!("WHERE {}", conditions.join(" AND ")), params)
        } else {
            (String::new(), Vec::new())
        };

        let sql = format!(
            "SELECT 
                COUNT(*) as total_requests,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as total_cache_read_tokens,
                COALESCE(SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
             FROM proxy_request_logs
             {where_clause}"
        );

        let result = conn.query_row(&sql, rusqlite::params_from_iter(params_vec), |row| {
            let total_requests: i64 = row.get(0)?;
            let total_cost: f64 = row.get(1)?;
            let total_input_tokens: i64 = row.get(2)?;
            let total_output_tokens: i64 = row.get(3)?;
            let total_cache_creation_tokens: i64 = row.get(4)?;
            let total_cache_read_tokens: i64 = row.get(5)?;
            let success_count: i64 = row.get(6)?;

            let success_rate = if total_requests > 0 {
                (success_count as f32 / total_requests as f32) * 100.0
            } else {
                0.0
            };

            Ok(UsageSummary {
                total_requests: total_requests as u64,
                total_cost: format!("{total_cost:.6}"),
                total_input_tokens: total_input_tokens as u64,
                total_output_tokens: total_output_tokens as u64,
                total_cache_creation_tokens: total_cache_creation_tokens as u64,
                total_cache_read_tokens: total_cache_read_tokens as u64,
                success_rate,
            })
        })?;

        Ok(result)
    }

    /// 获取每日趋势（滑动窗口，<=24h 按小时，>24h 按天，窗口与汇总一致）
    pub fn get_daily_trends(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<Vec<DailyStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let end_ts = end_date.unwrap_or_else(|| Local::now().timestamp());
        let mut start_ts = start_date.unwrap_or_else(|| end_ts - 24 * 60 * 60);

        if start_ts >= end_ts {
            start_ts = end_ts - 24 * 60 * 60;
        }

        let duration = end_ts - start_ts;
        let bucket_seconds: i64 = if duration <= 24 * 60 * 60 {
            60 * 60
        } else {
            24 * 60 * 60
        };
        let mut bucket_count: i64 = if duration <= 0 {
            1
        } else {
            ((duration as f64) / bucket_seconds as f64).ceil() as i64
        };

        // 固定 24 小时窗口为 24 个小时桶，避免浮点误差
        if bucket_seconds == 60 * 60 {
            bucket_count = 24;
        }

        if bucket_count < 1 {
            bucket_count = 1;
        }

        let sql = "
            SELECT 
                CAST((created_at - ?1) / ?3 AS INTEGER) as bucket_idx,
                COUNT(*) as request_count,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens,
                COALESCE(SUM(input_tokens), 0) as total_input_tokens,
                COALESCE(SUM(output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(cache_read_tokens), 0) as total_cache_read_tokens
            FROM proxy_request_logs
            WHERE created_at >= ?1 AND created_at <= ?2
            GROUP BY bucket_idx
            ORDER BY bucket_idx ASC";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![start_ts, end_ts, bucket_seconds], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                DailyStats {
                    date: String::new(),
                    request_count: row.get::<_, i64>(1)? as u64,
                    total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                    total_tokens: row.get::<_, i64>(3)? as u64,
                    total_input_tokens: row.get::<_, i64>(4)? as u64,
                    total_output_tokens: row.get::<_, i64>(5)? as u64,
                    total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
                    total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
                },
            ))
        })?;

        let mut map: HashMap<i64, DailyStats> = HashMap::new();
        for row in rows {
            let (mut bucket_idx, stat) = row?;
            if bucket_idx < 0 {
                continue;
            }
            if bucket_idx >= bucket_count {
                bucket_idx = bucket_count - 1;
            }
            map.insert(bucket_idx, stat);
        }

        let mut stats = Vec::with_capacity(bucket_count as usize);
        for i in 0..bucket_count {
            let bucket_start_ts = start_ts + i * bucket_seconds;
            let bucket_start = Local
                .timestamp_opt(bucket_start_ts, 0)
                .single()
                .unwrap_or_else(Local::now);

            let date = bucket_start.format("%Y-%m-%dT%H:%M:%S").to_string();

            if let Some(mut stat) = map.remove(&i) {
                stat.date = date;
                stats.push(stat);
            } else {
                stats.push(DailyStats {
                    date,
                    request_count: 0,
                    total_cost: "0.000000".to_string(),
                    total_tokens: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_cache_creation_tokens: 0,
                    total_cache_read_tokens: 0,
                });
            }
        }

        Ok(stats)
    }

    /// 获取 Provider 统计
    pub fn get_provider_stats(&self) -> Result<Vec<ProviderStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let sql = "SELECT 
                l.provider_id,
                p.name as provider_name,
                COUNT(*) as request_count,
                COALESCE(SUM(l.input_tokens + l.output_tokens), 0) as total_tokens,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count,
                COALESCE(AVG(l.latency_ms), 0) as avg_latency
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             GROUP BY l.provider_id, l.app_type
             ORDER BY total_cost DESC";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let request_count: i64 = row.get(2)?;
            let success_count: i64 = row.get(5)?;
            let success_rate = if request_count > 0 {
                (success_count as f32 / request_count as f32) * 100.0
            } else {
                0.0
            };

            Ok(ProviderStats {
                provider_id: row.get(0)?,
                provider_name: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| "Unknown".to_string()),
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(3)? as u64,
                total_cost: format!("{:.6}", row.get::<_, f64>(4)?),
                success_rate,
                avg_latency_ms: row.get::<_, f64>(6)? as u64,
            })
        })?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取模型统计
    pub fn get_model_stats(&self) -> Result<Vec<ModelStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let sql = "SELECT 
                model,
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost
             FROM proxy_request_logs
             GROUP BY model
             ORDER BY total_cost DESC";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let request_count: i64 = row.get(1)?;
            let total_cost: f64 = row.get(3)?;
            let avg_cost = if request_count > 0 {
                total_cost / request_count as f64
            } else {
                0.0
            };

            Ok(ModelStats {
                model: row.get(0)?,
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(2)? as u64,
                total_cost: format!("{total_cost:.6}"),
                avg_cost_per_request: format!("{avg_cost:.6}"),
            })
        })?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取请求日志列表（分页）
    pub fn get_request_logs(
        &self,
        filters: &LogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedLogs, AppError> {
        let conn = lock_conn!(self.conn);

        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref app_type) = filters.app_type {
            conditions.push("l.app_type = ?");
            params.push(Box::new(app_type.clone()));
        }
        if let Some(ref provider_name) = filters.provider_name {
            conditions.push("p.name LIKE ?");
            params.push(Box::new(format!("%{provider_name}%")));
        }
        if let Some(ref model) = filters.model {
            conditions.push("l.model LIKE ?");
            params.push(Box::new(format!("%{model}%")));
        }
        if let Some(status) = filters.status_code {
            conditions.push("l.status_code = ?");
            params.push(Box::new(status as i64));
        }
        if let Some(start) = filters.start_date {
            conditions.push("l.created_at >= ?");
            params.push(Box::new(start));
        }
        if let Some(end) = filters.end_date {
            conditions.push("l.created_at <= ?");
            params.push(Box::new(end));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // 获取总数
        let count_sql = format!(
            "SELECT COUNT(*) FROM proxy_request_logs l 
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}"
        );
        let count_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let total: u32 = conn.query_row(&count_sql, count_params.as_slice(), |row| {
            row.get::<_, i64>(0).map(|v| v as u32)
        })?;

        // 获取数据
        let offset = page * page_size;
        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));

        let sql = format!(
            "SELECT l.request_id, l.provider_id, p.name as provider_name, l.app_type, l.model,
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}
             ORDER BY l.created_at DESC
             LIMIT ? OFFSET ?"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(RequestLogDetail {
                request_id: row.get(0)?,
                provider_id: row.get(1)?,
                provider_name: row.get(2)?,
                app_type: row.get(3)?,
                model: row.get(4)?,
                input_tokens: row.get::<_, i64>(5)? as u32,
                output_tokens: row.get::<_, i64>(6)? as u32,
                cache_read_tokens: row.get::<_, i64>(7)? as u32,
                cache_creation_tokens: row.get::<_, i64>(8)? as u32,
                input_cost_usd: row.get(9)?,
                output_cost_usd: row.get(10)?,
                cache_read_cost_usd: row.get(11)?,
                cache_creation_cost_usd: row.get(12)?,
                total_cost_usd: row.get(13)?,
                is_streaming: row.get::<_, i64>(14)? != 0,
                latency_ms: row.get::<_, i64>(15)? as u64,
                first_token_ms: row.get::<_, Option<i64>>(16)?.map(|v| v as u64),
                duration_ms: row.get::<_, Option<i64>>(17)?.map(|v| v as u64),
                status_code: row.get::<_, i64>(18)? as u16,
                error_message: row.get(19)?,
                created_at: row.get(20)?,
            })
        })?;

        let mut logs = Vec::new();
        let mut provider_cache = HashMap::new();
        let mut pricing_cache = HashMap::new();

        for row in rows {
            let mut log = row?;
            Self::maybe_backfill_log_costs(
                &conn,
                &mut log,
                &mut provider_cache,
                &mut pricing_cache,
            )?;
            logs.push(log);
        }

        Ok(PaginatedLogs {
            data: logs,
            total,
            page,
            page_size,
        })
    }

    /// 获取单个请求详情
    pub fn get_request_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);

        let result = conn.query_row(
            "SELECT l.request_id, l.provider_id, p.name as provider_name, l.app_type, l.model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    is_streaming, latency_ms, first_token_ms, duration_ms,
                    status_code, error_message, created_at
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE l.request_id = ?",
            [request_id],
            |row| {
                Ok(RequestLogDetail {
                    request_id: row.get(0)?,
                    provider_id: row.get(1)?,
                    provider_name: row.get(2)?,
                    app_type: row.get(3)?,
                    model: row.get(4)?,
                    input_tokens: row.get::<_, i64>(5)? as u32,
                    output_tokens: row.get::<_, i64>(6)? as u32,
                    cache_read_tokens: row.get::<_, i64>(7)? as u32,
                    cache_creation_tokens: row.get::<_, i64>(8)? as u32,
                    input_cost_usd: row.get(9)?,
                    output_cost_usd: row.get(10)?,
                    cache_read_cost_usd: row.get(11)?,
                    cache_creation_cost_usd: row.get(12)?,
                    total_cost_usd: row.get(13)?,
                    is_streaming: row.get::<_, i64>(14)? != 0,
                    latency_ms: row.get::<_, i64>(15)? as u64,
                    first_token_ms: row.get::<_, Option<i64>>(16)?.map(|v| v as u64),
                    duration_ms: row.get::<_, Option<i64>>(17)?.map(|v| v as u64),
                    status_code: row.get::<_, i64>(18)? as u16,
                    error_message: row.get(19)?,
                    created_at: row.get(20)?,
                })
            },
        );

        match result {
            Ok(mut detail) => {
                let mut provider_cache = HashMap::new();
                let mut pricing_cache = HashMap::new();
                Self::maybe_backfill_log_costs(
                    &conn,
                    &mut detail,
                    &mut provider_cache,
                    &mut pricing_cache,
                )?;
                Ok(Some(detail))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 检查 Provider 使用限额
    pub fn check_provider_limits(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, AppError> {
        let conn = lock_conn!(self.conn);

        // 获取 provider 的限额设置
        let (limit_daily, limit_monthly) = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = ? AND app_type = ?",
                params![provider_id, app_type],
                |row| {
                    let meta_str: String = row.get(0)?;
                    Ok(meta_str)
                },
            )
            .ok()
            .and_then(|meta_str| serde_json::from_str::<serde_json::Value>(&meta_str).ok())
            .map(|meta| {
                let daily = meta
                    .get("limitDailyUsd")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                let monthly = meta
                    .get("limitMonthlyUsd")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                (daily, monthly)
            })
            .unwrap_or((None, None));

        // 计算今日使用量
        let daily_usage: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs
             WHERE provider_id = ? AND app_type = ?
               AND date(datetime(created_at, 'unixepoch', 'localtime')) = date('now', 'localtime')",
                params![provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        // 计算本月使用量
        let monthly_usage: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs
             WHERE provider_id = ? AND app_type = ?
               AND strftime('%Y-%m', datetime(created_at, 'unixepoch', 'localtime')) = strftime('%Y-%m', 'now', 'localtime')",
                params![provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let daily_exceeded = limit_daily
            .map(|limit| daily_usage >= limit)
            .unwrap_or(false);
        let monthly_exceeded = limit_monthly
            .map(|limit| monthly_usage >= limit)
            .unwrap_or(false);

        Ok(ProviderLimitStatus {
            provider_id: provider_id.to_string(),
            daily_usage: format!("{daily_usage:.6}"),
            daily_limit: limit_daily.map(|l| format!("{l:.2}")),
            daily_exceeded,
            monthly_usage: format!("{monthly_usage:.6}"),
            monthly_limit: limit_monthly.map(|l| format!("{l:.2}")),
            monthly_exceeded,
        })
    }
}

/// Provider 限额状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimitStatus {
    pub provider_id: String,
    pub daily_usage: String,
    pub daily_limit: Option<String>,
    pub daily_exceeded: bool,
    pub monthly_usage: String,
    pub monthly_limit: Option<String>,
    pub monthly_exceeded: bool,
}

#[derive(Clone)]
struct PricingInfo {
    input: rust_decimal::Decimal,
    output: rust_decimal::Decimal,
    cache_read: rust_decimal::Decimal,
    cache_creation: rust_decimal::Decimal,
}

impl Database {
    fn maybe_backfill_log_costs(
        conn: &Connection,
        log: &mut RequestLogDetail,
        provider_cache: &mut HashMap<(String, String), rust_decimal::Decimal>,
        pricing_cache: &mut HashMap<String, PricingInfo>,
    ) -> Result<(), AppError> {
        let total_cost = rust_decimal::Decimal::from_str(&log.total_cost_usd)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let has_cost = total_cost > rust_decimal::Decimal::ZERO;
        let has_usage = log.input_tokens > 0
            || log.output_tokens > 0
            || log.cache_read_tokens > 0
            || log.cache_creation_tokens > 0;

        if has_cost || !has_usage {
            return Ok(());
        }

        let pricing = match Self::get_model_pricing_cached(conn, pricing_cache, &log.model)? {
            Some(info) => info,
            None => return Ok(()),
        };
        let multiplier = Self::get_cost_multiplier_cached(
            conn,
            provider_cache,
            &log.provider_id,
            &log.app_type,
        )?;

        let million = rust_decimal::Decimal::from(1_000_000u64);
        let input_cost = rust_decimal::Decimal::from(log.input_tokens as u64) * pricing.input
            / million
            * multiplier;
        let output_cost = rust_decimal::Decimal::from(log.output_tokens as u64) * pricing.output
            / million
            * multiplier;
        let cache_read_cost = rust_decimal::Decimal::from(log.cache_read_tokens as u64)
            * pricing.cache_read
            / million
            * multiplier;
        let cache_creation_cost = rust_decimal::Decimal::from(log.cache_creation_tokens as u64)
            * pricing.cache_creation
            / million
            * multiplier;
        let total_cost = input_cost + output_cost + cache_read_cost + cache_creation_cost;

        log.input_cost_usd = format!("{input_cost:.6}");
        log.output_cost_usd = format!("{output_cost:.6}");
        log.cache_read_cost_usd = format!("{cache_read_cost:.6}");
        log.cache_creation_cost_usd = format!("{cache_creation_cost:.6}");
        log.total_cost_usd = format!("{total_cost:.6}");

        conn.execute(
            "UPDATE proxy_request_logs
             SET input_cost_usd = ?1,
                 output_cost_usd = ?2,
                 cache_read_cost_usd = ?3,
                 cache_creation_cost_usd = ?4,
                 total_cost_usd = ?5
             WHERE request_id = ?6",
            params![
                log.input_cost_usd,
                log.output_cost_usd,
                log.cache_read_cost_usd,
                log.cache_creation_cost_usd,
                log.total_cost_usd,
                log.request_id
            ],
        )
        .map_err(|e| AppError::Database(format!("更新请求成本失败: {e}")))?;

        Ok(())
    }

    fn get_cost_multiplier_cached(
        conn: &Connection,
        cache: &mut HashMap<(String, String), rust_decimal::Decimal>,
        provider_id: &str,
        app_type: &str,
    ) -> Result<rust_decimal::Decimal, AppError> {
        let key = (provider_id.to_string(), app_type.to_string());
        if let Some(multiplier) = cache.get(&key) {
            return Ok(*multiplier);
        }

        let meta_json: Option<String> = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = ? AND app_type = ?",
                params![provider_id, app_type],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::Database(format!("查询 provider meta 失败: {e}")))?;

        let multiplier = meta_json
            .and_then(|meta| serde_json::from_str::<Value>(&meta).ok())
            .and_then(|value| value.get("costMultiplier").cloned())
            .and_then(|val| {
                val.as_str()
                    .and_then(|s| rust_decimal::Decimal::from_str(s).ok())
            })
            .unwrap_or(rust_decimal::Decimal::ONE);

        cache.insert(key, multiplier);
        Ok(multiplier)
    }

    fn get_model_pricing_cached(
        conn: &Connection,
        cache: &mut HashMap<String, PricingInfo>,
        model: &str,
    ) -> Result<Option<PricingInfo>, AppError> {
        if let Some(info) = cache.get(model) {
            return Ok(Some(info.clone()));
        }

        let row = find_model_pricing_row(conn, model)?;
        let Some((input, output, cache_read, cache_creation)) = row else {
            return Ok(None);
        };

        let pricing = PricingInfo {
            input: rust_decimal::Decimal::from_str(&input)
                .map_err(|e| AppError::Database(format!("解析输入价格失败: {e}")))?,
            output: rust_decimal::Decimal::from_str(&output)
                .map_err(|e| AppError::Database(format!("解析输出价格失败: {e}")))?,
            cache_read: rust_decimal::Decimal::from_str(&cache_read)
                .map_err(|e| AppError::Database(format!("解析缓存读取价格失败: {e}")))?,
            cache_creation: rust_decimal::Decimal::from_str(&cache_creation)
                .map_err(|e| AppError::Database(format!("解析缓存写入价格失败: {e}")))?,
        };

        cache.insert(model.to_string(), pricing.clone());
        Ok(Some(pricing))
    }
}

pub(crate) fn find_model_pricing_row(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    // 清洗模型名称：去前缀(/)、去后缀(:)、@ 替换为 -
    // 例如 moonshotai/gpt-5.2-codex@low:v2 → gpt-5.2-codex-low
    let cleaned = model_id
        .rsplit_once('/')
        .map_or(model_id, |(_, r)| r)
        .split(':')
        .next()
        .unwrap_or(model_id)
        .trim()
        .replace('@', "-");

    // 精确匹配清洗后的名称
    let exact = conn
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing
             WHERE model_id = ?1",
            [&cleaned],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| AppError::Database(format!("查询模型定价失败: {e}")))?;

    if exact.is_none() {
        log::warn!("模型 {model_id}（清洗后: {cleaned}）未找到定价信息，成本将记录为 0");
    }

    Ok(exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_usage_summary() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试数据
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["req1", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["req2", "p1", "claude", "claude-3", 200, 100, "0.02", 150, 200, 2000],
            )?;
        }

        let summary = db.get_usage_summary(None, None)?;
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.success_rate, 100.0);

        Ok(())
    }

    #[test]
    fn test_get_model_stats() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试数据
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req1",
                    "p1",
                    "claude",
                    "claude-3-sonnet",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    1000
                ],
            )?;
        }

        let stats = db.get_model_stats()?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "claude-3-sonnet");
        assert_eq!(stats[0].request_count, 1);

        Ok(())
    }

    #[test]
    fn test_model_pricing_matching() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        // 准备额外定价数据，覆盖前缀/后缀清洗场景
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                "claude-haiku-4.5",
                "Claude Haiku 4.5",
                "1.0",
                "2.0",
                "0.0",
                "0.0"
            ],
        )?;

        // 测试精确匹配（seed_model_pricing 已预置 claude-sonnet-4-5-20250929）
        let result = find_model_pricing_row(&conn, "claude-sonnet-4-5-20250929")?;
        assert!(
            result.is_some(),
            "应该能精确匹配 claude-sonnet-4-5-20250929"
        );

        // 清洗：去除前缀和冒号后缀
        let result = find_model_pricing_row(&conn, "anthropic/claude-haiku-4.5")?;
        assert!(
            result.is_some(),
            "带前缀的模型 anthropic/claude-haiku-4.5 应能匹配到 claude-haiku-4.5"
        );
        let result = find_model_pricing_row(&conn, "moonshotai/kimi-k2-0905:exa")?;
        assert!(
            result.is_some(),
            "带前缀+冒号后缀的模型应清洗后匹配到 kimi-k2-0905"
        );

        // 清洗：@ 替换为 -（seed_model_pricing 已预置 gpt-5.2-codex-low）
        let result = find_model_pricing_row(&conn, "gpt-5.2-codex@low")?;
        assert!(
            result.is_some(),
            "带 @ 分隔符的模型 gpt-5.2-codex@low 应能匹配到 gpt-5.2-codex-low"
        );

        // 测试不存在的模型
        let result = find_model_pricing_row(&conn, "unknown-model-123")?;
        assert!(result.is_none(), "不应该匹配不存在的模型");

        Ok(())
    }
}
