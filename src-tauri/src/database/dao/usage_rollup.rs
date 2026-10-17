//! Usage rollup DAO
//!
//! Aggregates proxy_request_logs into daily rollups and prunes old detail rows.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::sql_helpers::{fresh_input_sql, INPUT_TOKEN_SEMANTICS_FRESH};
use crate::services::usage_stats::effective_usage_log_filter;
use chrono::{Duration, Local, TimeZone};

/// Rolled-up request identities are kept this long (see `usage_rollup_ledger`).
/// Older rollup totals stay, but can no longer be de-duplicated across devices.
pub(crate) const ROLLUP_LEDGER_RETENTION_DAYS: i64 = 365;

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

/// Whether this database holds any cross-device usage state (a remote cache,
/// inherited history or identity ledger), i.e. whether the materialized
/// cross-device subtraction depends on local rollups at all.
fn has_usage_sync_state(conn: &rusqlite::Connection) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM usage_rollup_ledger)
             OR EXISTS(SELECT 1 FROM usage_sync_remote_partitions)
             OR EXISTS(SELECT 1 FROM usage_sync_inherited)",
        [],
        |row| row.get(0),
    )
    .map_err(|e| AppError::Database(format!("Reading usage sync state failed: {e}")))
}

impl Database {
    /// Whether rollups record per-request identities (the ledger used by
    /// cross-device usage sync): only after usage sync was enabled on this
    /// device, in automatic or manual mode.
    pub(crate) fn usage_identity_ledger_enabled(&self) -> bool {
        #[cfg(test)]
        if let Some(enabled) = *self
            .usage_sync_enabled_override
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            return enabled;
        }
        crate::settings::get_usage_sync_settings().enabled
    }

    /// Test stand-in for enabling / disabling usage sync on this device.
    #[cfg(test)]
    pub(crate) fn set_usage_sync_enabled_for_test(&self, enabled: bool) {
        *self
            .usage_sync_enabled_override
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(enabled);
    }

    /// Aggregate proxy_request_logs older than `retain_days` into usage_daily_rollups,
    /// then delete the aggregated detail rows.
    /// Returns the number of deleted detail rows.
    pub fn rollup_and_prune(&self, retain_days: i64) -> Result<u64, AppError> {
        let cutoff = compute_local_midnight_cutoff(Local::now(), retain_days)?;
        // Read before taking the connection lock (settings have their own lock).
        let sync_enabled = self.usage_identity_ledger_enabled();
        let conn = lock_conn!(self.conn);
        // Identities are recorded once usage sync is enabled, and also while
        // it is off on a database whose identity already published (see
        // `identity_takes_part_in_sync`). Installations that never published
        // record nothing. On a read error, record (the safe side).
        let record_identities = sync_enabled
            || crate::services::usage_sync::store::identity_takes_part_in_sync(&conn)
                .unwrap_or(true);

        // Identity-ledger retention runs on every call, also on an idle device
        // with nothing to roll up, so the 365-day bound holds everywhere.
        let expired = conn
            .execute(
                "DELETE FROM usage_rollup_ledger WHERE date < date('now', 'localtime', ?1)",
                [format!("-{ROLLUP_LEDGER_RETENTION_DAYS} days")],
            )
            .map_err(|e| AppError::Database(format!("Rollup ledger retention failed: {e}")))?;
        if expired > 0 {
            if let Err(e) = crate::services::usage_sync::store::refresh_ledger_suppression(&conn) {
                log::warn!("Refreshing cross-device ledger suppression failed: {e}");
            }
        }

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

        let result = Self::do_rollup_and_prune(&conn, cutoff, record_identities);

        match result {
            Ok(deleted) => {
                conn.execute("RELEASE rollup_prune;", [])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                if deleted > 0 {
                    log::info!(
                        "Rolled up and pruned {deleted} proxy_request_logs (retain={retain_days}d)"
                    );
                    // Only databases that take part in usage sync need the
                    // cross-device state re-derived; for everyone else it is empty.
                    // The rollup is already committed here: on a read error, refresh.
                    let refresh = record_identities || has_usage_sync_state(&conn).unwrap_or(true);
                    if refresh {
                        if let Err(e) =
                            crate::services::usage_sync::store::refresh_ledger_suppression(&conn)
                        {
                            log::warn!("Refreshing cross-device ledger suppression failed: {e}");
                        }
                    }
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

    fn do_rollup_and_prune(
        conn: &rusqlite::Connection,
        cutoff: i64,
        record_identities: bool,
    ) -> Result<u64, AppError> {
        // Aggregate old logs, merging with any pre-existing rollup rows via LEFT JOIN.
        let effective_filter = effective_usage_log_filter("l");
        let fresh_detail_input = fresh_input_sql("l");
        let fresh_old_input = fresh_input_sql("old");
        // request_model 维度保留路由接管的「客户端别名 → 真实模型」映射，
        // pricing_model 维度保留写入时的计价基准（request 计价模式下与 model 分叉）；
        // 明细行的这两列可能为 NULL（历史/手工数据），归一为 ''。
        let aggregation_sql = format!(
            "INSERT OR REPLACE INTO usage_daily_rollups
                (date, app_type, provider_id, model, request_model, pricing_model,
                 request_count, success_count,
                 input_tokens, output_tokens,
                 cache_read_tokens, cache_creation_tokens,
                 input_token_semantics, total_cost_usd, avg_latency_ms)
            SELECT
                d, a, p, m, rm, pm,
                COALESCE(old.request_count, 0) + new_req,
                COALESCE(old.success_count, 0) + new_succ,
                COALESCE({fresh_old_input}, 0) + new_in,
                COALESCE(old.output_tokens, 0) + new_out,
                COALESCE(old.cache_read_tokens, 0) + new_cr,
                COALESCE(old.cache_creation_tokens, 0) + new_cc,
                {INPUT_TOKEN_SEMANTICS_FRESH},
                CAST(COALESCE(CAST(old.total_cost_usd AS REAL), 0) + new_cost AS TEXT),
                CASE WHEN COALESCE(old.request_count, 0) + new_req > 0
                    THEN (COALESCE(old.avg_latency_ms, 0) * COALESCE(old.request_count, 0)
                          + new_lat * new_req)
                         / (COALESCE(old.request_count, 0) + new_req)
                    ELSE 0 END
            FROM (
                SELECT
                    date(l.created_at, 'unixepoch', 'localtime') as d,
                    l.app_type as a, l.provider_id as p, l.model as m,
                    COALESCE(l.request_model, '') as rm,
                    COALESCE(l.pricing_model, '') as pm,
                    COUNT(*) as new_req,
                    SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as new_succ,
                    COALESCE(SUM({fresh_detail_input}), 0) as new_in,
                    COALESCE(SUM(l.output_tokens), 0) as new_out,
                    COALESCE(SUM(l.cache_read_tokens), 0) as new_cr,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as new_cc,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as new_cost,
                    COALESCE(AVG(l.latency_ms), 0) as new_lat
                FROM proxy_request_logs l
                WHERE l.created_at < ?1 AND {effective_filter}
                GROUP BY d, a, p, m, rm, pm
            ) agg
            LEFT JOIN usage_daily_rollups old
                ON old.date = agg.d AND old.app_type = agg.a
                AND old.provider_id = agg.p AND old.model = agg.m
                AND old.request_model = agg.rm AND old.pricing_model = agg.pm"
        );

        conn.execute(&aggregation_sql, [cutoff])
            .map_err(|e| AppError::Database(format!("Rollup aggregation failed: {e}")))?;

        // Usage sync only: record identities before the detail rows go, in the
        // same savepoint, so a failure here keeps the detail rows.
        if record_identities {
            Self::record_rollup_identities(conn, cutoff)?;
        }

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

    /// Write the identity ledger for the rows about to be pruned (cross-device
    /// usage sync). Runs inside the rollup savepoint, before the DELETE.
    fn record_rollup_identities(conn: &rusqlite::Connection, cutoff: i64) -> Result<(), AppError> {
        let effective_filter = effective_usage_log_filter("l");

        // Keep the identity of every rolled-up row (what it added to which rollup
        // group), so cross-device statistics can still de-duplicate it after its
        // detail row is gone. Rows the effective filter drops are recorded as
        // aliases: they are counted through another row of this device.
        let fresh_ledger_input = fresh_input_sql("l");
        // kind 1 (published alias) only for unambiguous 1:1 pairings; other
        // heuristic duplicates are kind 2 (dropped here, never published).
        let unambiguous = crate::services::usage_stats::unambiguous_alias_sql("l");
        let ledger_sql = format!(
            "INSERT OR IGNORE INTO usage_rollup_ledger (
                request_id, data_source, kind, date, app_type, provider_id, model,
                request_model, pricing_model, success, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, total_cost_usd, rolled_up_at
             )
             SELECT request_id, data_source,
                    CASE WHEN eff THEN 0 WHEN unambiguous THEN 1 ELSE 2 END,
                    d, app_type, provider_id, model, rm, pm,
                    CASE WHEN eff AND status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END,
                    CASE WHEN eff THEN fresh_in ELSE 0 END,
                    CASE WHEN eff THEN output_tokens ELSE 0 END,
                    CASE WHEN eff THEN cache_read_tokens ELSE 0 END,
                    CASE WHEN eff THEN cache_creation_tokens ELSE 0 END,
                    CASE WHEN eff THEN cost ELSE '0' END,
                    ?2
             FROM (
                SELECT l.request_id, COALESCE(l.data_source, 'proxy') AS data_source,
                       ({effective_filter}) AS eff,
                       ({unambiguous}) AS unambiguous,
                       date(l.created_at, 'unixepoch', 'localtime') AS d,
                       l.app_type, l.provider_id, l.model,
                       COALESCE(l.request_model, '') AS rm, COALESCE(l.pricing_model, '') AS pm,
                       l.status_code, {fresh_ledger_input} AS fresh_in, l.output_tokens,
                       l.cache_read_tokens, l.cache_creation_tokens,
                       CAST(COALESCE(CAST(l.total_cost_usd AS REAL), 0) AS TEXT) AS cost
                FROM proxy_request_logs l
                WHERE l.created_at < ?1
             )"
        );
        conn.execute(
            &ledger_sql,
            rusqlite::params![cutoff, Local::now().timestamp()],
        )
        .map_err(|e| AppError::Database(format!("Rollup ledger failed: {e}")))?;
        conn.execute(
            "DELETE FROM usage_rollup_ledger WHERE date < date('now', 'localtime', ?1)",
            [format!("-{ROLLUP_LEDGER_RETENTION_DAYS} days")],
        )
        .map_err(|e| AppError::Database(format!("Rollup ledger retention failed: {e}")))?;
        Ok(())
    }
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
        assert_eq!(*input_tokens, 90, "rollup stores normalized fresh input");
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
    fn test_rollup_normalizes_total_cache_semantics_to_fresh() -> Result<(), AppError> {
        let db = Database::memory()?;
        let old_ts = chrono::Utc::now().timestamp() - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_token_semantics, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES ('total-semantics-rollup', 'p1', 'codex', 'gpt-5.5',
                          100, 5, 10, 20, 1, '0.10', 100, 200, ?1)",
                [old_ts],
            )?;
        }

        assert_eq!(db.rollup_and_prune(30)?, 1);

        let conn = crate::database::lock_conn!(db.conn);
        let row: (i64, i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, cache_creation_tokens,
                    input_token_semantics
             FROM usage_daily_rollups WHERE model = 'gpt-5.5'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(row, (70, 10, 20, 2));

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
}
