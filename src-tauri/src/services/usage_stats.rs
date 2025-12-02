//! 使用统计服务
//!
//! 提供使用量数据的聚合查询功能

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// 使用量汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub total_requests: u64,
    pub total_cost: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub success_rate: f32,
}

/// 每日统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub date: String,
    pub request_count: u64,
    pub total_cost: String,
    pub total_tokens: u64,
}

/// Provider 统计
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct ModelStats {
    pub model: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    pub avg_cost_per_request: String,
}

/// 请求日志过滤器
#[derive(Debug, Clone, Default)]
pub struct LogFilters {
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<u16>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
}

/// 请求日志详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogDetail {
    pub request_id: String,
    pub provider_id: String,
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
    pub latency_ms: u64,
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
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens,
                COALESCE(SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
             FROM proxy_request_logs
             {where_clause}"
        );

        let result = conn.query_row(&sql, rusqlite::params_from_iter(params_vec), |row| {
            let total_requests: i64 = row.get(0)?;
            let total_cost: f64 = row.get(1)?;
            let total_tokens: i64 = row.get(2)?;
            let success_count: i64 = row.get(3)?;

            let success_rate = if total_requests > 0 {
                (success_count as f32 / total_requests as f32) * 100.0
            } else {
                0.0
            };

            Ok(UsageSummary {
                total_requests: total_requests as u64,
                total_cost: format!("{:.6}", total_cost),
                total_input_tokens: 0,
                total_output_tokens: 0,
                success_rate,
            })
        })?;

        Ok(result)
    }

    /// 获取每日趋势
    pub fn get_daily_trends(
        &self,
        days: u32,
    ) -> Result<Vec<DailyStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let sql = "SELECT 
                date(created_at, 'unixepoch') as date,
                COUNT(*) as request_count,
                COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens
             FROM proxy_request_logs
             WHERE created_at >= strftime('%s', 'now', ?)
             GROUP BY date
             ORDER BY date DESC";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([format!("-{} days", days)], |row| {
            Ok(DailyStats {
                date: row.get(0)?,
                request_count: row.get::<_, i64>(1)? as u64,
                total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                total_tokens: row.get::<_, i64>(3)? as u64,
            })
        })?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
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
                provider_name: row.get::<_, Option<String>>(1)?.unwrap_or_else(|| "Unknown".to_string()),
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
                total_cost: format!("{:.6}", total_cost),
                avg_cost_per_request: format!("{:.6}", avg_cost),
            })
        })?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取请求日志列表
    pub fn get_request_logs(
        &self,
        filters: &LogFilters,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RequestLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref provider_id) = filters.provider_id {
            conditions.push("provider_id = ?");
            params.push(Box::new(provider_id.clone()));
        }
        if let Some(ref model) = filters.model {
            conditions.push("model = ?");
            params.push(Box::new(model.clone()));
        }
        if let Some(status) = filters.status_code {
            conditions.push("status_code = ?");
            params.push(Box::new(status as i64));
        }
        if let Some(start) = filters.start_date {
            conditions.push("created_at >= ?");
            params.push(Box::new(start));
        }
        if let Some(end) = filters.end_date {
            conditions.push("created_at <= ?");
            params.push(Box::new(end));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        params.push(Box::new(limit as i64));
        params.push(Box::new(offset as i64));

        let sql = format!(
            "SELECT request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    latency_ms, status_code, error_message, created_at
             FROM proxy_request_logs
             {where_clause}
             ORDER BY created_at DESC
             LIMIT ? OFFSET ?"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(RequestLogDetail {
                request_id: row.get(0)?,
                provider_id: row.get(1)?,
                app_type: row.get(2)?,
                model: row.get(3)?,
                input_tokens: row.get::<_, i64>(4)? as u32,
                output_tokens: row.get::<_, i64>(5)? as u32,
                cache_read_tokens: row.get::<_, i64>(6)? as u32,
                cache_creation_tokens: row.get::<_, i64>(7)? as u32,
                input_cost_usd: row.get(8)?,
                output_cost_usd: row.get(9)?,
                cache_read_cost_usd: row.get(10)?,
                cache_creation_cost_usd: row.get(11)?,
                total_cost_usd: row.get(12)?,
                latency_ms: row.get::<_, i64>(13)? as u64,
                status_code: row.get::<_, i64>(14)? as u16,
                error_message: row.get(15)?,
                created_at: row.get(16)?,
            })
        })?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }

        Ok(logs)
    }

    /// 获取单个请求详情
    pub fn get_request_detail(&self, request_id: &str) -> Result<Option<RequestLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);

        let result = conn.query_row(
            "SELECT request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    latency_ms, status_code, error_message, created_at
             FROM proxy_request_logs
             WHERE request_id = ?",
            [request_id],
            |row| {
                Ok(RequestLogDetail {
                    request_id: row.get(0)?,
                    provider_id: row.get(1)?,
                    app_type: row.get(2)?,
                    model: row.get(3)?,
                    input_tokens: row.get::<_, i64>(4)? as u32,
                    output_tokens: row.get::<_, i64>(5)? as u32,
                    cache_read_tokens: row.get::<_, i64>(6)? as u32,
                    cache_creation_tokens: row.get::<_, i64>(7)? as u32,
                    input_cost_usd: row.get(8)?,
                    output_cost_usd: row.get(9)?,
                    cache_read_cost_usd: row.get(10)?,
                    cache_creation_cost_usd: row.get(11)?,
                    total_cost_usd: row.get(12)?,
                    latency_ms: row.get::<_, i64>(13)? as u64,
                    status_code: row.get::<_, i64>(14)? as u16,
                    error_message: row.get(15)?,
                    created_at: row.get(16)?,
                })
            },
        );

        match result {
            Ok(detail) => Ok(Some(detail)),
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
        let (limit_daily, limit_monthly) = conn.query_row(
            "SELECT meta FROM providers WHERE id = ? AND app_type = ?",
            params![provider_id, app_type],
            |row| {
                let meta_str: String = row.get(0)?;
                Ok(meta_str)
            },
        ).ok().and_then(|meta_str| {
            serde_json::from_str::<serde_json::Value>(&meta_str).ok()
        }).and_then(|meta| {
            let daily = meta.get("limitDailyUsd")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok());
            let monthly = meta.get("limitMonthlyUsd")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok());
            Some((daily, monthly))
        }).unwrap_or((None, None));

        // 计算今日使用量
        let daily_usage: f64 = conn.query_row(
            "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs
             WHERE provider_id = ? AND app_type = ?
               AND date(created_at, 'unixepoch') = date('now')",
            params![provider_id, app_type],
            |row| row.get(0),
        ).unwrap_or(0.0);

        // 计算本月使用量
        let monthly_usage: f64 = conn.query_row(
            "SELECT COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0)
             FROM proxy_request_logs
             WHERE provider_id = ? AND app_type = ?
               AND strftime('%Y-%m', created_at, 'unixepoch') = strftime('%Y-%m', 'now')",
            params![provider_id, app_type],
            |row| row.get(0),
        ).unwrap_or(0.0);

        let daily_exceeded = limit_daily.map(|limit| daily_usage >= limit).unwrap_or(false);
        let monthly_exceeded = limit_monthly.map(|limit| monthly_usage >= limit).unwrap_or(false);

        Ok(ProviderLimitStatus {
            provider_id: provider_id.to_string(),
            daily_usage: format!("{:.6}", daily_usage),
            daily_limit: limit_daily.map(|l| format!("{:.2}", l)),
            daily_exceeded,
            monthly_usage: format!("{:.6}", monthly_usage),
            monthly_limit: limit_monthly.map(|l| format!("{:.2}", l)),
            monthly_exceeded,
        })
    }
}

/// Provider 限额状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderLimitStatus {
    pub provider_id: String,
    pub daily_usage: String,
    pub daily_limit: Option<String>,
    pub daily_exceeded: bool,
    pub monthly_usage: String,
    pub monthly_limit: Option<String>,
    pub monthly_exceeded: bool,
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
                params!["req1", "p1", "claude", "claude-3-sonnet", 100, 50, "0.01", 100, 200, 1000],
            )?;
        }

        let stats = db.get_model_stats()?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "claude-3-sonnet");
        assert_eq!(stats[0].request_count, 1);

        Ok(())
    }
}
