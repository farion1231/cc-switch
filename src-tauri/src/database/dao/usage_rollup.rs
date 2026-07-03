//! Usage rollup DAO
//!
//! Aggregates proxy_request_logs into daily rollups and prunes old detail rows.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::usage_stats::effective_usage_log_filter;
use chrono::{Duration, Local, TimeZone};

/// Compute the rollup/prune cutoff aligned to a local-day boundary.
///
/// Anything strictly older than the returned timestamp will be aggregated into
/// `usage_daily_rollups` and deleted from `proxy_request_logs`. Aligning to the
/// next local midnight after `(now - retain_days)` guarantees that the youngest
/// rollup row always represents a *complete* local day. Without this alignment
/// the cutoff falls mid-day, leaving the day half-rolled-up and half-pruned —
/// which would silently under-count any range query that touches that day
/// after `compute_rollup_date_bounds` trims partial-coverage rollup days.
fn compute_local_midnight_cutoff(
    now: chrono::DateTime<Local>,
    retain_days: i64,
) -> Result<i64, AppError> {
    let target_day = now
        .checked_sub_signed(Duration::days(retain_days))
        .ok_or_else(|| AppError::Database("rollup cutoff overflow".to_string()))?
        .date_naive();

    // Use the *next* day's midnight so anything before it has fully been bucketed.
    let next_day = target_day
        .succ_opt()
        .ok_or_else(|| AppError::Database("rollup cutoff next-day overflow".to_string()))?;
    let naive_midnight = next_day
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| AppError::Database("rollup cutoff midnight overflow".to_string()))?;

    let local_dt = match Local.from_local_datetime(&naive_midnight) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(earliest, _) => earliest,
        chrono::LocalResult::None => {
            // DST gap: fall back to one hour later, which always exists.
            let bumped = naive_midnight + Duration::hours(1);
            match Local.from_local_datetime(&bumped) {
                chrono::LocalResult::Single(dt) => dt,
                chrono::LocalResult::Ambiguous(earliest, _) => earliest,
                chrono::LocalResult::None => {
                    return Err(AppError::Database(
                        "rollup cutoff fell into DST gap".to_string(),
                    ))
                }
            }
        }
    };

    Ok(local_dt.timestamp())
}

impl Database {
    /// Aggregate proxy_request_logs older than `retain_days` into usage_daily_rollups,
    /// then delete the aggregated detail rows.
    /// Returns the number of deleted detail rows.
    pub fn rollup_and_prune(&self, retain_days: i64) -> Result<u64, AppError> {
        let cutoff = compute_local_midnight_cutoff(Local::now(), retain_days)?;
        let conn = lock_conn!(self.conn);

        // Check if there are any rows to process
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE created_at < ?1",
                [cutoff],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        if count == 0 {
            return Ok(0);
        }

        // 剪枝是不可逆的：明细一旦汇总删除，0 成本行就永远失去按 pricing_model
        // 补价重算的机会（启动序列里 seed 定价先于 rollup、但启动回填在 rollup
        // 之后；周期任务同理）。所以剪枝前先尽力回填一次。失败仅告警不阻断——
        // 否则一行损坏的定价数据会永久卡死日志清理。
        // 注意必须在 SAVEPOINT 之外调用：回填内部自己开顶层事务。
        if let Err(e) = Self::backfill_missing_usage_costs_on_conn(&conn, None) {
            log::warn!("Pre-prune cost backfill failed, pruning anyway: {e}");
        }

        // Use a savepoint for atomicity
        conn.execute("SAVEPOINT rollup_prune;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = Self::do_rollup_and_prune(&conn, cutoff);

        match result {
            Ok(deleted) => {
                conn.execute("RELEASE rollup_prune;", [])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                if deleted > 0 {
                    log::info!(
                        "Rolled up and pruned {deleted} proxy_request_logs (retain={retain_days}d)"
                    );
                    // 归档触发了表结构变化，前端 30 天前的统计可能跟着变，
                    // 通知一次让 UsageDashboard 重拉数据
                    crate::usage_events::notify_log_recorded();
                }
                Ok(deleted)
            }
            Err(e) => {
                conn.execute("ROLLBACK TO rollup_prune;", []).ok();
                conn.execute("RELEASE rollup_prune;", []).ok();
                Err(e)
            }
        }
    }

    fn do_rollup_and_prune(conn: &rusqlite::Connection, cutoff: i64) -> Result<u64, AppError> {
        // Aggregate old logs, merging with any pre-existing rollup rows via LEFT JOIN.
        let effective_filter = effective_usage_log_filter("l");
        // request_model 维度保留路由接管的「客户端别名 → 真实模型」映射，
        // pricing_model 维度保留写入时的计价基准（request 计价模式下与 model 分叉），
        // api_key_id 维度区分同一 provider 下不同 key 的用量（v12 引入）。
        // 明细行的这三列可能为 NULL（历史/手工数据），归一为 ''。
        let aggregation_sql = format!(
            "INSERT OR REPLACE INTO usage_daily_rollups
                (date, app_type, provider_id, api_key_id, model, request_model, pricing_model,
                 request_count, success_count,
                 input_tokens, output_tokens,
                 cache_read_tokens, cache_creation_tokens,
                 total_cost_usd, avg_latency_ms)
            SELECT
                d, a, p, kid, m, rm, pm,
                COALESCE(old.request_count, 0) + new_req,
                COALESCE(old.success_count, 0) + new_succ,
                COALESCE(old.input_tokens, 0) + new_in,
                COALESCE(old.output_tokens, 0) + new_out,
                COALESCE(old.cache_read_tokens, 0) + new_cr,
                COALESCE(old.cache_creation_tokens, 0) + new_cc,
                CAST(COALESCE(CAST(old.total_cost_usd AS REAL), 0) + new_cost AS TEXT),
                CASE WHEN COALESCE(old.request_count, 0) + new_req > 0
                    THEN (COALESCE(old.avg_latency_ms, 0) * COALESCE(old.request_count, 0)
                          + new_lat * new_req)
                         / (COALESCE(old.request_count, 0) + new_req)
                    ELSE 0 END
            FROM (
                SELECT
                    date(l.created_at, 'unixepoch', 'localtime') as d,
                    l.app_type as a, l.provider_id as p,
                    COALESCE(l.api_key_id, '') as kid,
                    l.model as m,
                    COALESCE(l.request_model, '') as rm,
                    COALESCE(l.pricing_model, '') as pm,
                    COUNT(*) as new_req,
                    SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as new_succ,
                    COALESCE(SUM(l.input_tokens), 0) as new_in,
                    COALESCE(SUM(l.output_tokens), 0) as new_out,
                    COALESCE(SUM(l.cache_read_tokens), 0) as new_cr,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as new_cc,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as new_cost,
                    COALESCE(AVG(l.latency_ms), 0) as new_lat
                FROM proxy_request_logs l
                WHERE l.created_at < ?1 AND {effective_filter}
                GROUP BY d, a, p, kid, m, rm, pm
            ) agg
            LEFT JOIN usage_daily_rollups old
                ON old.date = agg.d AND old.app_type = agg.a
                AND old.provider_id = agg.p AND old.api_key_id = agg.kid
                AND old.model = agg.m
                AND old.request_model = agg.rm AND old.pricing_model = agg.pm"
        );

        conn.execute(&aggregation_sql, [cutoff])
            .map_err(|e| AppError::Database(format!("Rollup aggregation failed: {e}")))?;

        // INSERT uses the effective-log filter to exclude duplicate session rows.
        // DELETE intentionally prunes all old details so those duplicates are discarded.
        let deleted = conn
            .execute(
                "DELETE FROM proxy_request_logs WHERE created_at < ?1",
                [cutoff],
            )
            .map_err(|e| AppError::Database(format!("Pruning old logs failed: {e}")))?;

        Ok(deleted as u64)
    }

    /// 读取某把 API key 在 `range_days` 内的用量聚合（明细粒度）。
    ///
    /// 注：明细表只保留 30 天内数据（30 天前的已被 rollup_and_prune 折叠并删除）。
    /// 这个接口只读明细——UI 在展示"最近 N 天"时足够；超过 30 天的数据需要走
    /// rollup 表合并（不在本接口范围内，留作 follow-up）。
    pub fn get_usage_by_key(
        &self,
        app_type: &str,
        provider_id: &str,
        key_id: &str,
        range_days: u32,
    ) -> Result<KeyUsage, AppError> {
        let now = chrono::Utc::now().timestamp();
        let since = now - (range_days as i64) * 86400;
        let conn = lock_conn!(self.conn);

        // 明细按 day 聚合
        let sql = format!(
            "SELECT
                 date(created_at, 'unixepoch', 'localtime') as d,
                 COUNT(*) as req,
                 SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END) as succ,
                 COALESCE(SUM(input_tokens), 0) as in_t,
                 COALESCE(SUM(output_tokens), 0) as out_t,
                 COALESCE(SUM(cache_read_tokens), 0) as cr_t,
                 COALESCE(SUM(cache_creation_tokens), 0) as cc_t,
                 COALESCE(SUM(CAST(total_cost_usd AS REAL)), 0) as cost
             FROM proxy_request_logs
             WHERE app_type = ?1 AND provider_id = ?2 AND api_key_id = ?3
               AND created_at >= ?4
             GROUP BY d
             ORDER BY d DESC"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let mut daily: Vec<KeyUsageDay> = Vec::new();
        let rows = stmt
            .query_map(
                rusqlite::params![app_type, provider_id, key_id, since],
                |row| {
                    Ok(KeyUsageDay {
                        date: row.get(0)?,
                        request_count: row.get::<_, i64>(1)? as u32,
                        success_count: row.get::<_, i64>(2)? as u32,
                        input_tokens: row.get(3)?,
                        output_tokens: row.get(4)?,
                        cache_read_tokens: row.get(5)?,
                        cache_creation_tokens: row.get(6)?,
                        total_cost_usd: format!("{:.6}", row.get::<_, f64>(7)?),
                    })
                },
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        for r in rows {
            daily.push(r.map_err(|e| AppError::Database(e.to_string()))?);
        }

        let total = KeyUsageTotal::from_days(&daily);
        Ok(KeyUsage {
            key_id: key_id.to_string(),
            daily,
            total,
        })
    }

    /// 读取某个 provider 所有 key 的并行聚合。
    /// 返回 Vec 长度等于该 provider 的 key 数（已 order by sort_index）。
    /// 任何 key 在 range 内没有请求时也出现在结果中，全 0。
    pub fn get_usage_by_provider_keys(
        &self,
        app_type: &str,
        provider_id: &str,
        range_days: u32,
    ) -> Result<Vec<KeyUsageSummary>, AppError> {
        let keys = self.list_api_keys(provider_id, app_type)?;
        let mut out = Vec::with_capacity(keys.len());
        for k in &keys {
            let usage = self.get_usage_by_key(app_type, provider_id, &k.id, range_days)?;
            out.push(KeyUsageSummary {
                key_id: k.id.clone(),
                label: k.label.clone(),
                is_active: k.is_active,
                enabled: k.enabled,
                cooldown_until: k.cooldown_until,
                total: usage.total,
            });
        }
        Ok(out)
    }
}

/// One day of usage for a single key. `total_cost_usd` 是固定精度字符串，
/// 与 `proxy_request_logs.total_cost_usd` / `usage_daily_rollups.total_cost_usd` 口径一致。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyUsageDay {
    pub date: String,
    pub request_count: u32,
    pub success_count: u32,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_cost_usd: String,
}

/// 单 key 的总用量。`success_rate` 是 success / request 浮点百分比（0.0-100.0）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyUsageTotal {
    pub request_count: u32,
    pub success_count: u32,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_cost_usd: String,
    pub success_rate: f32,
}

impl KeyUsageTotal {
    fn from_days(days: &[KeyUsageDay]) -> Self {
        let mut total = Self {
            request_count: 0,
            success_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_cost_usd: "0".to_string(),
            success_rate: 0.0,
        };
        let mut cost: f64 = 0.0;
        for d in days {
            total.request_count = total.request_count.saturating_add(d.request_count);
            total.success_count = total.success_count.saturating_add(d.success_count);
            total.input_tokens = total.input_tokens.saturating_add(d.input_tokens);
            total.output_tokens = total.output_tokens.saturating_add(d.output_tokens);
            total.cache_read_tokens =
                total.cache_read_tokens.saturating_add(d.cache_read_tokens);
            total.cache_creation_tokens =
                total.cache_creation_tokens.saturating_add(d.cache_creation_tokens);
            cost += d.total_cost_usd.parse::<f64>().unwrap_or(0.0);
        }
        total.total_cost_usd = format!("{:.6}", cost);
        if total.request_count > 0 {
            total.success_rate =
                (total.success_count as f32 / total.request_count as f32) * 100.0;
        }
        total
    }
}

/// 完整返回值：daily 数组 + 折叠后的 total。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyUsage {
    pub key_id: String,
    pub daily: Vec<KeyUsageDay>,
    pub total: KeyUsageTotal,
}

/// `get_usage_by_provider_keys` 的行项：包含 key 元数据 + total，
/// 供前端 ProviderCard 的 per-key 列表直接展示。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyUsageSummary {
    pub key_id: String,
    pub label: String,
    pub is_active: bool,
    pub enabled: bool,
    pub cooldown_until: i64,
    pub total: KeyUsageTotal,
}

#[cfg(test)]
mod tests {
    use super::compute_local_midnight_cutoff;
    use crate::database::Database;
    use crate::error::AppError;
    use chrono::{Local, TimeZone};

    fn local_dt(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> chrono::DateTime<Local> {
        match Local.with_ymd_and_hms(year, month, day, hour, minute, second) {
            chrono::LocalResult::Single(dt) => dt,
            chrono::LocalResult::Ambiguous(earliest, _) => earliest,
            chrono::LocalResult::None => panic!("invalid local datetime in test fixture"),
        }
    }

    #[test]
    fn cutoff_is_aligned_to_local_midnight_after_target_day() -> Result<(), AppError> {
        // now = 2026-04-16 14:32:17 local; retain_days = 30
        // target day = 2026-03-17; cutoff should be 2026-03-18 00:00 local.
        let now = local_dt(2026, 4, 16, 14, 32, 17);
        let cutoff_ts = compute_local_midnight_cutoff(now, 30)?;
        let cutoff_dt = Local.timestamp_opt(cutoff_ts, 0).single().unwrap();
        let expected = local_dt(2026, 3, 18, 0, 0, 0);
        assert_eq!(cutoff_dt, expected);
        Ok(())
    }

    #[test]
    fn cutoff_at_local_midnight_now_still_lands_on_midnight() -> Result<(), AppError> {
        // If `now` is itself local midnight, the math should not introduce drift.
        let now = local_dt(2026, 4, 16, 0, 0, 0);
        let cutoff_ts = compute_local_midnight_cutoff(now, 7)?;
        let cutoff_dt = Local.timestamp_opt(cutoff_ts, 0).single().unwrap();
        // (2026-04-16 - 7d) = 2026-04-09; cutoff = 2026-04-10 00:00 local.
        let expected = local_dt(2026, 4, 10, 0, 0, 0);
        assert_eq!(cutoff_dt, expected);
        Ok(())
    }

    #[test]
    fn test_rollup_and_prune() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400; // 40 days ago
        let recent_ts = now - 5 * 86400; // 5 days ago

        {
            let conn = crate::database::lock_conn!(db.conn);
            for i in 0..5 {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at
                    ) VALUES (?1, 'p1', 'claude', 'claude-3', 100, 50, '0.01', 100, 200, ?2)",
                    rusqlite::params![format!("old-{i}"), old_ts + i as i64],
                )?;
            }
            for i in 0..3 {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at
                    ) VALUES (?1, 'p1', 'claude', 'claude-3', 200, 100, '0.02', 150, 200, ?2)",
                    rusqlite::params![format!("recent-{i}"), recent_ts + i as i64],
                )?;
            }
        }

        let deleted = db.rollup_and_prune(30)?;
        assert_eq!(deleted, 5);

        // Verify rollup data
        let conn = crate::database::lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT request_count FROM usage_daily_rollups WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 5);

        // Verify recent logs untouched
        let remaining: i64 =
            conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining, 3);
        Ok(())
    }

    #[test]
    fn test_rollup_uses_effective_usage_logs() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?1, 'openai', 'codex', 'gpt-5.4', 'gpt-5.4', 100, 20, 10, 0, '0.10', 100, 200, ?2, 'proxy')",
                rusqlite::params!["codex-proxy-old", old_ts],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?1, '_codex_session', 'codex', 'gpt-5.4', 'gpt-5.4', 100, 20, 10, 0, '0.10', 0, 200, ?2, 'codex_session')",
                rusqlite::params!["codex-session-old-dup", old_ts + 60],
            )?;
        }

        let deleted = db.rollup_and_prune(30)?;
        assert_eq!(deleted, 2);

        let conn = crate::database::lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT provider_id, request_count, input_tokens, output_tokens, cache_read_tokens
             FROM usage_daily_rollups WHERE app_type = 'codex'",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(rows.len(), 1);
        let (provider_id, request_count, input_tokens, output_tokens, cache_read_tokens) = &rows[0];
        assert_eq!(provider_id, "openai");
        assert_eq!(*request_count, 1);
        assert_eq!(*input_tokens, 100);
        assert_eq!(*output_tokens, 20);
        assert_eq!(*cache_read_tokens, 10);

        let remaining: i64 =
            conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining, 0);

        Ok(())
    }

    #[test]
    fn test_rollup_preserves_request_model_dimension() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            // 路由接管行：model 是真实上游模型，request_model 是客户端别名。
            // 同 model 下两个不同别名必须各自成行，prune 后映射关系仍可审计。
            for (i, request_model) in [
                ("a", "claude-sonnet-4-6"),
                ("b", "claude-sonnet-4-6"),
                ("c", "claude-haiku-4-5"),
            ] {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at
                    ) VALUES (?1, 'p1', 'claude', 'kimi-k2', ?2, 100, 50, '0.01', 100, 200, ?3)",
                    rusqlite::params![format!("takeover-{i}"), request_model, old_ts],
                )?;
            }
        }

        let deleted = db.rollup_and_prune(30)?;
        assert_eq!(deleted, 3);

        let conn = crate::database::lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT request_model, request_count FROM usage_daily_rollups
             WHERE model = 'kimi-k2' ORDER BY request_model",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(
            rows,
            vec![
                ("claude-haiku-4-5".to_string(), 1),
                ("claude-sonnet-4-6".to_string(), 2),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_rollup_preserves_pricing_model_dimension() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            // request 计价模式下 pricing_model 与 model 分叉，必须各自成行
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('pm-a', 'p1', 'claude', 'kimi-k2', 'claude-sonnet-4-6', 'kimi-k2',
                          100, 50, '0.01', 100, 200, ?1)",
                rusqlite::params![old_ts],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('pm-b', 'p1', 'claude', 'kimi-k2', 'claude-sonnet-4-6', 'claude-sonnet-4-6',
                          100, 50, '0.30', 100, 200, ?1)",
                rusqlite::params![old_ts],
            )?;
        }

        let deleted = db.rollup_and_prune(30)?;
        assert_eq!(deleted, 2);

        let conn = crate::database::lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT pricing_model, total_cost_usd FROM usage_daily_rollups
             WHERE model = 'kimi-k2' ORDER BY pricing_model",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "claude-sonnet-4-6");
        assert_eq!(rows[1].0, "kimi-k2");
        Ok(())
    }

    #[test]
    fn test_rollup_backfills_costs_before_pruning() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            // >30 天的 0 成本行：pricing_model（gpt-5.5）在 seed 定价表中有价。
            // 剪枝是不可逆的，rollup 必须先回填再汇总，否则按 0 永久入账。
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('prune-backfill', 'p1', 'codex', 'gpt-5.5', 'gpt-5.5', 'gpt-5.5',
                          1000000, 0, '0', 100, 200, ?1)",
                rusqlite::params![old_ts],
            )?;
        }

        let deleted = db.rollup_and_prune(30)?;
        assert_eq!(deleted, 1);

        let conn = crate::database::lock_conn!(db.conn);
        let total_cost: f64 = conn.query_row(
            "SELECT CAST(total_cost_usd AS REAL) FROM usage_daily_rollups
             WHERE model = 'gpt-5.5'",
            [],
            |row| row.get(0),
        )?;
        // gpt-5.5 input $5/M × 1M tokens，回填后再汇总
        assert!(
            (total_cost - 5.0).abs() < 1e-6,
            "expected backfilled cost 5.0, got {total_cost}"
        );
        Ok(())
    }

    #[test]
    fn test_rollup_noop_when_no_old_data() -> Result<(), AppError> {
        let db = Database::memory()?;
        assert_eq!(db.rollup_and_prune(30)?, 0);
        Ok(())
    }

    #[test]
    fn test_rollup_merges_with_existing() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            let date_str = Local
                .timestamp_opt(old_ts, 0)
                .single()
                .expect("old timestamp should be a valid local datetime")
                .format("%Y-%m-%d")
                .to_string();
            conn.execute(
                "INSERT INTO usage_daily_rollups
                    (date, app_type, provider_id, model, request_count, success_count,
                     input_tokens, output_tokens, total_cost_usd, avg_latency_ms)
                 VALUES (?1, 'claude', 'p1', 'claude-3', 10, 10, 1000, 500, '0.10', 100)",
                [&date_str],
            )?;
            for i in 0..3 {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at
                    ) VALUES (?1, 'p1', 'claude', 'claude-3', 100, 50, '0.01', 200, 200, ?2)",
                    rusqlite::params![format!("merge-{i}"), old_ts + i as i64],
                )?;
            }
        }

        let deleted = db.rollup_and_prune(30)?;
        assert_eq!(deleted, 3);

        let conn = crate::database::lock_conn!(db.conn);
        let (count, input): (i64, i64) = conn.query_row(
            "SELECT request_count, input_tokens FROM usage_daily_rollups
             WHERE app_type = 'claude' AND provider_id = 'p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(count, 13, "10 existing + 3 new");
        assert_eq!(input, 1300, "1000 existing + 300 new");
        Ok(())
    }

    // ─────── per-key 用量查询 ───────

    fn insert_log(
        conn: &rusqlite::Connection,
        req_id: &str,
        key_id: Option<&str>,
        ts: i64,
        status: i64,
        in_t: i64,
        out_t: i64,
        cost: f64,
    ) {
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, api_key_id,
                input_tokens, output_tokens, total_cost_usd,
                latency_ms, status_code, created_at, data_source
             ) VALUES (?1, 'p1', 'claude', 'kimi-k2', ?2, ?3, ?4, ?5, 100, ?6, ?7, 'proxy')",
            rusqlite::params![
                req_id,
                key_id,
                in_t,
                out_t,
                format!("{cost}"),
                status,
                ts,
            ],
        )
        .expect("insert log");
    }

    fn seed_provider(db: &Database) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR IGNORE INTO providers (id, app_type, name, settings_config)
             VALUES ('p1', 'claude', 'P1', '{}')",
            [],
        )
        .expect("seed provider");
        drop(conn);
        Ok(())
    }

    #[test]
    fn get_usage_by_key_aggregates_only_that_key() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db)?;

        // 为 key-A 和 key-B 各写两条成功 + 一条 429 失败的行
        let now = chrono::Utc::now().timestamp();
        {
            let conn = crate::database::lock_conn!(db.conn);
            for key in ["key-A", "key-B"] {
                for i in 0..3 {
                    let status = if i == 2 { 429 } else { 200 };
                    insert_log(
                        &conn,
                        &format!("log-{key}-{i}"),
                        Some(key),
                        now - i * 3600,
                        status,
                        100,
                        50,
                        0.01,
                    );
                }
            }
        }

        let usage = db.get_usage_by_key("claude", "p1", "key-A", 7)?;
        assert_eq!(usage.total.request_count, 3);
        assert_eq!(usage.total.success_count, 2);
        // 100 + 50 tokens × 3 行 = 300 + 150
        assert_eq!(usage.total.input_tokens, 300);
        assert_eq!(usage.total.output_tokens, 150);
        // success_rate ≈ 66.67%
        assert!((usage.total.success_rate - 66.66666).abs() < 0.1);
        assert!((usage.total.total_cost_usd.parse::<f64>().unwrap() - 0.03).abs() < 1e-6);

        // key-B 是独立聚合：被 key-A 测试隔离
        let usage_b = db.get_usage_by_key("claude", "p1", "key-B", 7)?;
        assert_eq!(usage_b.total.request_count, 3);
        Ok(())
    }

    #[test]
    fn get_usage_by_provider_keys_returns_one_summary_per_key() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db)?;

        // 写两把 key，每把各一行
        let now = chrono::Utc::now().timestamp();
        {
            let conn = crate::database::lock_conn!(db.conn);
            insert_log(&conn, "log-1", Some("k-1"), now, 200, 100, 50, 0.01);
            insert_log(&conn, "log-2", Some("k-2"), now, 500, 200, 80, 0.02);
            // 还有一行没绑 key（api_key_id NULL）：v11 历史数据
            insert_log(&conn, "log-3", None, now, 200, 50, 25, 0.005);
        }

        // 没插入 api_keys 时返回空 Vec（list_api_keys → 空）
        let empty = db.get_usage_by_provider_keys("claude", "p1", 7)?;
        assert!(empty.is_empty());

        // 插入两把 key
        let now = chrono::Utc::now().timestamp();
        let key1 = crate::database::dao::api_keys::ProviderApiKey {
            id: "k-1".to_string(),
            provider_id: "p1".to_string(),
            app_type: "claude".to_string(),
            label: "Primary".to_string(),
            api_key: "sk-1".to_string(),
            tags: vec![],
            notes: String::new(),
            enabled: true,
            sort_index: 0,
            is_active: true,
            cooldown_until: 0,
            failure_count: 0,
            last_used_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        };
        let mut key2 = key1.clone();
        key2.id = "k-2".to_string();
        key2.label = "Backup".to_string();
        key2.is_active = false;
        key2.sort_index = 1;
        db.insert_api_key(&key1)?;
        db.insert_api_key(&key2)?;

        let summaries = db.get_usage_by_provider_keys("claude", "p1", 7)?;
        assert_eq!(summaries.len(), 2, "two keys both surfaced");
        // 按 sort_index 排：k-1 在前
        assert_eq!(summaries[0].key_id, "k-1");
        assert_eq!(summaries[1].key_id, "k-2");
        assert!(summaries[0].is_active);
        assert!(!summaries[1].is_active);
        assert_eq!(summaries[0].label, "Primary");
        Ok(())
    }

    #[test]
    fn get_usage_by_key_returns_empty_for_unknown_key() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db)?;
        let usage = db.get_usage_by_key("claude", "p1", "nope", 7)?;
        assert_eq!(usage.total.request_count, 0);
        assert_eq!(usage.daily.len(), 0);
        assert_eq!(usage.total.success_rate, 0.0);
        Ok(())
    }

    #[test]
    fn get_usage_by_key_respects_range_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db)?;

        // 8 天前一条 + 1 天前一条；range=3 应该只看 1 天前那条
        let now = chrono::Utc::now().timestamp();
        {
            let conn = crate::database::lock_conn!(db.conn);
            insert_log(&conn, "old", Some("k1"), now - 8 * 86400, 200, 100, 50, 0.01);
            insert_log(&conn, "recent", Some("k1"), now - 86400, 200, 100, 50, 0.01);
        }

        let usage_3d = db.get_usage_by_key("claude", "p1", "k1", 3)?;
        assert_eq!(usage_3d.total.request_count, 1);
        let usage_14d = db.get_usage_by_key("claude", "p1", "k1", 14)?;
        assert_eq!(usage_14d.total.request_count, 2);
        Ok(())
    }
}
