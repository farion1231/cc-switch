//! Usage rollup DAO
//!
//! Aggregates proxy_request_logs into daily rollups and prunes old detail rows.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::sql_helpers::{fresh_input_sql, INPUT_TOKEN_SEMANTICS_FRESH};
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

        let result = Self::do_rollup_and_prune_scoped(&conn, cutoff, None, true);

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

    /// Rebuild-only compaction for Codex's native compatibility partition.
    ///
    /// This deliberately skips session canonical rollups: the Codex source
    /// parser has already produced the authoritative canonical generation in
    /// the staging database.  Only the legacy raw/daily ownership boundary is
    /// compacted before the staged generation is published.
    pub(crate) fn rollup_and_prune_codex_staging(&self, retain_days: i64) -> Result<u64, AppError> {
        let cutoff = compute_local_midnight_cutoff(Local::now(), retain_days)?;
        let conn = lock_conn!(self.conn);
        conn.execute("SAVEPOINT codex_rollup_prune;", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        let result = Self::do_rollup_and_prune_scoped(
            &conn,
            cutoff,
            Some(("codex", "_codex_session", "codex_session")),
            false,
        );
        match result {
            Ok(deleted) => {
                conn.execute("RELEASE codex_rollup_prune;", [])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(deleted)
            }
            Err(error) => {
                conn.execute("ROLLBACK TO codex_rollup_prune;", []).ok();
                conn.execute("RELEASE codex_rollup_prune;", []).ok();
                Err(error)
            }
        }
    }

    fn do_rollup_and_prune_scoped(
        conn: &rusqlite::Connection,
        cutoff: i64,
        scope: Option<(&str, &str, &str)>,
        include_session_rollups: bool,
    ) -> Result<u64, AppError> {
        // Session buckets are persisted before deleting detail rows.  This is
        // intentionally part of the same savepoint as the existing global
        // rollup and prune so a failure cannot leave a partial retention state.
        if include_session_rollups {
            Self::rollup_session_usage(conn, cutoff)?;
        }

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
                   AND (?2 IS NULL OR l.app_type = ?2)
                   AND (?3 IS NULL OR l.provider_id = ?3)
                   AND (?4 IS NULL OR l.data_source = ?4)
                 GROUP BY d, a, p, m, rm, pm
            ) agg
            LEFT JOIN usage_daily_rollups old
                ON old.date = agg.d AND old.app_type = agg.a
                AND old.provider_id = agg.p AND old.model = agg.m
                AND old.request_model = agg.rm AND old.pricing_model = agg.pm"
        );

        let (scope_app, scope_provider, scope_source) = scope
            .map(|(app, provider, source)| (Some(app), Some(provider), Some(source)))
            .unwrap_or((None, None, None));
        conn.execute(
            &aggregation_sql,
            rusqlite::params![cutoff, scope_app, scope_provider, scope_source],
        )
        .map_err(|e| AppError::Database(format!("Rollup aggregation failed: {e}")))?;

        // INSERT uses the effective-log filter to exclude duplicate session rows.
        // DELETE intentionally prunes all old details so those duplicates are discarded.
        let deleted = conn
            .execute(
                "DELETE FROM proxy_request_logs
                 WHERE created_at < ?1
                   AND (?2 IS NULL OR app_type = ?2)
                   AND (?3 IS NULL OR provider_id = ?3)
                   AND (?4 IS NULL OR data_source = ?4)",
                rusqlite::params![cutoff, scope_app, scope_provider, scope_source],
            )
            .map_err(|e| AppError::Database(format!("Pruning old logs failed: {e}")))?;

        Ok(deleted as u64)
    }

    /// Aggregate effective, session-tagged raw rows into durable per-session
    /// daily buckets.  Rows without a session id remain available only to the
    /// existing global daily rollup and are never assigned to a task.
    fn rollup_session_usage(conn: &rusqlite::Connection, cutoff: i64) -> Result<(), AppError> {
        // Keep the existing proxy/session deduplication boundary for raw
        // compatibility rows.  The additional durable coverage marker is
        // narrower: it excludes a raw request only when its exact request ID
        // was atomically represented by a direct canonical bucket.  Thus an
        // unmarked raw-only request still survives, including partial facts.
        let effective_filter = effective_usage_log_filter("l");
        let fresh_detail_input = fresh_input_sql("l");
        // Direct session sources do not carry per-component presence flags in
        // `proxy_request_logs`.  For Claude/Codex/Gemini, a parser fallback
        // zero is therefore not evidence that the source reported zero.  A
        // component is only retained when every row in the bucket is
        // non-zero (otherwise the whole component remains partial/NULL).
        // Grok and OpenCode have source-proven component semantics, so their
        // explicit zeros remain valid values.  Proxy rows retain the existing
        // fresh-input/global behavior.
        let data_source_expr = "COALESCE(l.data_source, 'proxy')";
        let unsafe_direct_sources = "'session_log', 'codex_session', 'gemini_session'";
        let direct_session_sources =
            "'session_log', 'codex_session', 'gemini_session', 'grok_session', 'opencode_session'";
        let session_rollup_sql = format!(
            "INSERT OR REPLACE INTO agent_session_usage_rollups (
                date, app_type, session_id, provider_id, model,
                request_model, pricing_model, data_source, precision,
                time_semantics, request_count_semantics, input_token_semantics,
                source_identity, profile_id, database_identity, base_url_digest,
                billing_mode, task, source_version, sync_window_start,
                sync_window_end, request_count, api_call_count, input_tokens,
                output_tokens, cache_read_tokens, cache_creation_tokens,
                cache_write_tokens, reasoning_tokens, total_cost_usd, cost_status, cost_source,
                cost_delta_kind,
                correction_state, first_event_at, last_event_at
             )
             SELECT
                agg.d, agg.a, agg.s, agg.p, agg.m, agg.rm, agg.pm, agg.ds,
                agg.precision, agg.time_semantics, agg.request_count_semantics,
                agg.input_token_semantics, '', '', '', '', '', '', '', 0, 0,
                CASE WHEN old.date IS NULL THEN agg.new_req
                     WHEN old.request_count IS NULL THEN NULL
                     ELSE old.request_count + agg.new_req END,
                old.api_call_count,
                CASE WHEN old.date IS NULL THEN agg.new_in
                     WHEN old.input_tokens IS NULL OR agg.new_in IS NULL THEN NULL
                     ELSE old.input_tokens + agg.new_in END,
                CASE WHEN old.date IS NULL THEN agg.new_out
                     WHEN old.output_tokens IS NULL OR agg.new_out IS NULL THEN NULL
                     ELSE old.output_tokens + agg.new_out END,
                CASE WHEN old.date IS NULL THEN agg.new_cr
                     WHEN old.cache_read_tokens IS NULL OR agg.new_cr IS NULL THEN NULL
                     ELSE old.cache_read_tokens + agg.new_cr END,
                CASE WHEN old.date IS NULL THEN agg.new_cc
                     WHEN old.cache_creation_tokens IS NULL OR agg.new_cc IS NULL THEN NULL
                     ELSE old.cache_creation_tokens + agg.new_cc END,
                NULL, NULL,
                CASE
                    WHEN old.date IS NULL THEN agg.new_cost_usd
                    WHEN old.total_cost_usd IS NULL OR agg.new_cost_usd IS NULL THEN NULL
                    ELSE CAST(CAST(old.total_cost_usd AS REAL) + agg.new_cost_usd AS TEXT)
                END,
                NULL, NULL, NULL, NULL,
                CASE WHEN old.first_event_at IS NULL THEN agg.first_event_at
                     WHEN agg.first_event_at IS NULL THEN old.first_event_at
                     ELSE MIN(old.first_event_at, agg.first_event_at) END,
                CASE WHEN old.last_event_at IS NULL THEN agg.last_event_at
                     WHEN agg.last_event_at IS NULL THEN old.last_event_at
                     ELSE MAX(old.last_event_at, agg.last_event_at) END
             FROM (
                SELECT
                    date(l.created_at, 'unixepoch', 'localtime') AS d,
                    l.app_type AS a,
                    l.session_id AS s,
                    l.provider_id AS p,
                    l.model AS m,
                    COALESCE(l.request_model, '') AS rm,
                    COALESCE(l.pricing_model, '') AS pm,
                    COALESCE(l.data_source, 'proxy') AS ds,
                    'request_exact' AS precision,
                    'event_time' AS time_semantics,
                    CASE WHEN COALESCE(l.data_source, 'proxy') = 'proxy'
                         THEN 'http_request'
                          WHEN COALESCE(l.data_source, 'proxy') IN ('grok_session', 'codex_session')
                          THEN 'agent_call'
                          WHEN COALESCE(l.data_source, 'proxy') = 'pi_session'
                          THEN 'usage_event'
                          ELSE 'assistant_message' END AS request_count_semantics,
                     l.input_token_semantics AS input_token_semantics,
                     COUNT(*) AS new_req,
                     CASE
                         WHEN {data_source_expr} IN ({unsafe_direct_sources})
                         THEN CASE
                             WHEN COUNT(CASE WHEN l.input_tokens = 0 THEN NULL ELSE 1 END)
                                      = COUNT(*)
                             THEN SUM(l.input_tokens)
                         END
                         WHEN {data_source_expr} IN ('grok_session', 'opencode_session')
                         THEN SUM(l.input_tokens)
                         ELSE SUM({fresh_detail_input})
                     END AS new_in,
                     CASE
                         WHEN {data_source_expr} IN ({unsafe_direct_sources})
                         THEN CASE
                             WHEN COUNT(CASE WHEN l.output_tokens = 0 THEN NULL ELSE 1 END)
                                      = COUNT(*)
                             THEN SUM(l.output_tokens)
                         END
                         ELSE SUM(l.output_tokens)
                     END AS new_out,
                     CASE
                         WHEN {data_source_expr} IN ({unsafe_direct_sources})
                         THEN CASE
                             WHEN COUNT(CASE WHEN l.cache_read_tokens = 0 THEN NULL ELSE 1 END)
                                      = COUNT(*)
                             THEN SUM(l.cache_read_tokens)
                         END
                         ELSE SUM(l.cache_read_tokens)
                     END AS new_cr,
                     CASE
                         WHEN {data_source_expr} IN ('codex_session', 'gemini_session', 'grok_session')
                         THEN NULL
                         WHEN {data_source_expr} = 'session_log'
                         THEN CASE
                             WHEN COUNT(CASE WHEN l.cache_creation_tokens = 0 THEN NULL ELSE 1 END)
                                      = COUNT(*)
                             THEN SUM(l.cache_creation_tokens)
                         END
                         ELSE SUM(l.cache_creation_tokens)
                     END AS new_cc,
                     CASE
                         WHEN {data_source_expr} IN ({direct_session_sources})
                         THEN NULL
                         WHEN SUM(CASE WHEN l.total_cost_usd IS NULL
                                            OR TRIM(l.total_cost_usd) = ''
                                       THEN 1 ELSE 0 END) > 0
                         THEN NULL
                         ELSE COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0)
                     END AS new_cost_usd,
                    MIN(l.created_at) AS first_event_at,
                    MAX(l.created_at) AS last_event_at
                FROM proxy_request_logs l
                WHERE l.created_at < ?1
                  AND l.session_id IS NOT NULL
                  AND TRIM(l.session_id) <> ''
                  AND NOT EXISTS (
                      SELECT 1
                      FROM agent_session_canonical_coverage coverage
                      WHERE coverage.app_type = l.app_type
                        AND coverage.data_source = COALESCE(l.data_source, 'proxy')
                        AND coverage.request_id = l.request_id
                  )
                  AND {effective_filter}
                GROUP BY d, a, s, p, m, rm, pm, ds,
                         precision, time_semantics, request_count_semantics,
                         input_token_semantics
             ) agg
               LEFT JOIN agent_session_usage_rollups old
               ON old.date = agg.d
              AND old.app_type = agg.a
              AND old.session_id = agg.s
              AND old.provider_id = agg.p
              AND old.model = agg.m
              AND old.request_model = agg.rm
              AND old.pricing_model = agg.pm
              AND old.data_source = agg.ds
              AND old.precision = agg.precision
              AND old.time_semantics = agg.time_semantics
              AND old.request_count_semantics = agg.request_count_semantics
              AND old.input_token_semantics = agg.input_token_semantics
              AND old.source_identity = ''
              AND old.profile_id = ''
              AND old.database_identity = ''
              AND old.base_url_digest = ''
              AND old.billing_mode = ''
              AND old.task = ''
              AND old.source_version = ''
              AND old.sync_window_start = 0
              AND old.sync_window_end = 0",
        );

        conn.execute(&session_rollup_sql, [cutoff])
            .map_err(|e| AppError::Database(format!("Session rollup aggregation failed: {e}")))?;
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
    fn codex_staging_rollup_only_moves_codex_legacy_rows() -> Result<(), AppError> {
        let db = Database::memory()?;
        let now = chrono::Utc::now().timestamp();
        let old_ts = now - 40 * 86400;
        let recent_ts = now - 5 * 86400;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('codex-old', '_codex_session', 'codex', 'gpt-5.6-sol',
                           10, 2, '0.1', 1, 200, ?1, 'codex_session')",
                [old_ts],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('codex-recent', '_codex_session', 'codex', 'gpt-5.6-sol',
                           3, 1, '0.02', 1, 200, ?1, 'codex_session')",
                [recent_ts],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('claude-old', 'claude-provider', 'claude', 'claude-3',
                           10, 2, '0.1', 1, 200, ?1, 'session_log')",
                [old_ts],
            )?;
            conn.execute(
                "INSERT INTO agent_session_usage_rollups (
                    date, app_type, session_id, provider_id, model, data_source,
                    request_count, input_tokens
                 ) VALUES ('2026-08-01', 'codex', 'codex-root', '_codex_session',
                           'gpt-5.6-sol', 'codex_session', 99, 999)",
                [],
            )?;
        }

        assert_eq!(db.rollup_and_prune_codex_staging(30)?, 1);
        let conn = crate::database::lock_conn!(db.conn);
        let counts: (i64, i64, i64, i64) = conn.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'codex-old'),
                 (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'codex-recent'),
                 (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'claude-old'),
                 (SELECT request_count FROM usage_daily_rollups
                  WHERE provider_id = '_codex_session' AND model = 'gpt-5.6-sol')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(counts, (0, 1, 1, 1));
        let canonical_count: i64 = conn.query_row(
            "SELECT request_count FROM agent_session_usage_rollups
             WHERE app_type = 'codex' AND session_id = 'codex-root'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(canonical_count, 99);
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
            conn.execute(
                "INSERT INTO agent_session_usage_snapshots (
                    app_type, source_identity, profile_id, database_identity,
                    session_id, model, provider_id, base_url_digest, billing_mode,
                    task, data_source, source_version, api_call_count,
                    input_tokens, output_tokens, last_synced_at
                 ) VALUES (
                    'hermes', 'hermes:fixture:v1', 'profile-a', 'db-a',
                    'snapshot-retained', 'fixture-model', 'fixture-provider',
                    'sha256:fixture-base', 'actual', 'task-a', 'fixture', '1',
                    4, 10, 5, ?1
                 )",
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
        // Total-inclusive input counts cache reads but keeps cache creation as
        // its own component, so fresh input is 100 - 10 rather than 100 - 10 - 20.
        assert_eq!(row, (90, 10, 20, 2));

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

    #[test]
    fn test_rollup_persists_session_buckets_before_prune_and_is_idempotent() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let old_ts = chrono::Utc::now().timestamp() - 40 * 86400;

        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    pricing_model, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, session_id, data_source
                 ) VALUES
                    ('session-root-1', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     100, 25, 5, 0, '0.10', 100, 200, ?1, 'root-session', 'proxy'),
                    ('session-root-2', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     50, 10, 0, 0, '0.05', 100, 200, ?1, 'root-session', 'proxy'),
                    ('session-child-1', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     20, 5, 0, 0, '0.02', 100, 200, ?1, 'child-session', 'proxy'),
                    ('session-none', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     999, 999, 0, 0, '9.99', 100, 200, ?1, NULL, 'proxy')",
                [old_ts],
            )?;
            conn.execute(
                "INSERT INTO agent_session_usage_snapshots (
                    app_type, source_identity, profile_id, database_identity,
                    session_id, model, provider_id, base_url_digest, billing_mode,
                    task, data_source, source_version, api_call_count,
                    input_tokens, output_tokens, last_synced_at
                 ) VALUES (
                    'hermes', 'hermes:fixture:v1', 'profile-a', 'db-a',
                    'snapshot-retained', 'fixture-model', 'fixture-provider',
                    'sha256:fixture-base', 'actual', 'task-a', 'fixture', '1',
                    4, 10, 5, ?1
                 )",
                [old_ts],
            )?;
        }

        assert_eq!(db.rollup_and_prune(30)?, 4);

        let conn = crate::database::lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT session_id, request_count, input_tokens, output_tokens, total_cost_usd
             FROM agent_session_usage_rollups ORDER BY session_id",
        )?;
        let rows: Vec<(String, i64, i64, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                ("child-session".to_string(), 1, 20, 5, "0.02".to_string()),
                ("root-session".to_string(), 2, 150, 35, "0.15".to_string()),
            ]
        );
        drop(stmt);
        let remaining_raw: i64 =
            conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
                row.get(0)
            })?;
        assert_eq!(remaining_raw, 0);
        let snapshot_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_snapshots
             WHERE session_id = 'snapshot-retained'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            snapshot_count, 1,
            "pruning raw logs must preserve source baselines"
        );
        drop(conn);

        // The second pass sees no old detail rows and therefore cannot add the
        // same buckets again.
        assert_eq!(db.rollup_and_prune(30)?, 0);
        let conn = crate::database::lock_conn!(db.conn);
        let root: (i64, i64, String) = conn.query_row(
            "SELECT request_count, input_tokens, total_cost_usd
             FROM agent_session_usage_rollups WHERE session_id = 'root-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(root, (2, 150, "0.15".to_string()));
        let snapshot_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_snapshots
             WHERE session_id = 'snapshot-retained'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(snapshot_count, 1);
        Ok(())
    }

    #[test]
    fn test_rollup_preserves_pi_usage_event_semantics() -> Result<(), AppError> {
        let db = Database::memory()?;
        let old_ts = chrono::Utc::now().timestamp() - 40 * 86400;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, latency_ms, status_code,
                    created_at, session_id, data_source
                 ) VALUES (
                    'pi:legacy-session:usage:1', 'pi', 'pi', 'pi-model',
                    10, 5, 2, 1, 0, 200, ?1, 'legacy-session', 'pi_session'
                 )",
                [old_ts],
            )?;
        }

        assert_eq!(db.rollup_and_prune(30)?, 1);

        let conn = crate::database::lock_conn!(db.conn);
        let (request_count, semantics): (i64, String) = conn.query_row(
            "SELECT request_count, request_count_semantics
             FROM agent_session_usage_rollups
             WHERE app_type = 'pi' AND session_id = 'legacy-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(request_count, 1);
        assert_eq!(semantics, "usage_event");
        Ok(())
    }

    #[test]
    fn test_rollup_uses_per_request_coverage_and_keeps_unmarked_mismatch() -> Result<(), AppError> {
        let db = Database::memory()?;
        let old_ts = chrono::Utc::now().timestamp() - 40 * 86400;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO agent_session_usage_rollups (
                    date, app_type, session_id, provider_id, model,
                    request_model, pricing_model, data_source, precision,
                    time_semantics, request_count_semantics, request_count,
                    input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd
                 ) VALUES (date(?1, 'unixepoch', 'localtime'), 'claude',
                    'coverage-session', 'p1', 'claude-3', 'claude-3', 'claude-3',
                    'session_log', 'request_exact', 'event_time', 'assistant_message',
                    1, 100, 10, 0, 0, '0.10')",
                [old_ts],
            )?;
            conn.execute(
                "INSERT INTO agent_session_canonical_coverage (
                    app_type, data_source, request_id, canonical_session_id, marked_at
                 ) VALUES ('claude', 'session_log', 'covered-request',
                           'coverage-session', ?1)",
                [old_ts],
            )?;
            conn.execute_batch(&format!(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    pricing_model, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, session_id, data_source
                 ) VALUES
                    ('covered-request', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     100, 10, 0, 0, '0.10', 100, 200, {old_ts}, 'coverage-session', 'session_log'),
                    ('unmarked-request', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     30, 3, 0, 0, '0.03', 100, 200, {old_ts}, 'coverage-session', 'session_log'),
                    ('mismatched-request', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     999, 1, 9, 0, '0.01', 100, 200, {old_ts}, 'coverage-session', 'session_log'),
                    ('deduped-request', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     40, 4, 0, 0, '0.04', 100, 200, {old_ts}, 'coverage-session', 'session_log'),
                    ('proxy-dedup', 'p1', 'claude', 'claude-3', 'claude-3', 'claude-3',
                     40, 4, 0, 0, '0.04', 100, 200, {old_ts}, NULL, 'proxy');"
            ))?;
        }

        assert_eq!(db.rollup_and_prune(30)?, 5);
        let conn = crate::database::lock_conn!(db.conn);
        let bucket: (i64, i64, i64, Option<String>) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens, total_cost_usd
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'coverage-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(bucket, (3, 1129, 14, None));
        // The matching proxy/session pair is excluded by the existing
        // effective filter; only marker coverage and the two genuinely
        // unmarked session rows contribute here.
        drop(conn);
        assert_eq!(db.rollup_and_prune(30)?, 0);
        let conn = crate::database::lock_conn!(db.conn);
        let repeated: (i64, i64, i64) = conn.query_row(
            "SELECT request_count, input_tokens, output_tokens
             FROM agent_session_usage_rollups
             WHERE app_type = 'claude' AND session_id = 'coverage-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(repeated, (3, 1129, 14));
        Ok(())
    }

    #[test]
    fn test_rollup_persists_partial_cache_creation_for_unsupported_sources() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let old_ts = chrono::Utc::now().timestamp() - 40 * 86400;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                &format!(
                    "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    pricing_model, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, session_id, data_source
                 ) VALUES ('codex-unproven', '_codex_session', 'codex', 'gpt-5', 'gpt-5',
                    'gpt-5', 100, 20, 5, 0, '0.10', 0, 200, ?1,
                    'codex-session', 'codex_session'),
                 ('gemini-unproven', '_gemini_session', 'gemini', 'gemini-2', 'gemini-2',
                    'gemini-2', 70, 12, 3, 0, '0.07', 0, 200, {old_ts},
                    'gemini-session', 'gemini_session'),
                 ('grok-unproven', '_grok_session', 'grokbuild', 'grok-2', 'grok-2',
                    'grok-2', 50, 8, 2, 0, '0.05', 0, 200, {old_ts},
                    'grok-session', 'grok_session');"
                )
                .replace("?1", &old_ts.to_string()),
            )?;
        }
        assert_eq!(db.rollup_and_prune(30)?, 3);
        let conn = crate::database::lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT data_source, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens
             FROM agent_session_usage_rollups
             WHERE data_source IN ('codex_session', 'gemini_session', 'grok_session')
             ORDER BY data_source",
        )?;
        let rows: Vec<(String, i64, i64, i64, Option<i64>)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                ("codex_session".into(), 100, 20, 5, None),
                ("gemini_session".into(), 70, 12, 3, None),
                ("grok_session".into(), 50, 8, 2, None),
            ]
        );
        drop(stmt);
        drop(conn);
        assert_eq!(db.rollup_and_prune(30)?, 0);
        let conn = crate::database::lock_conn!(db.conn);
        let repeated: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_usage_rollups
             WHERE data_source IN ('codex_session', 'gemini_session', 'grok_session')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(repeated, 3);
        let global_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM usage_daily_rollups
             WHERE app_type = 'codex' AND provider_id = '_codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(global_count, 1);
        Ok(())
    }

    #[test]
    fn test_rollup_preserves_raw_presence_semantics_by_source() -> Result<(), AppError> {
        let db = Database::memory()?;
        let old_ts = chrono::Utc::now().timestamp() - 40 * 86400;
        {
            let conn = crate::database::lock_conn!(db.conn);
            let insert = |request_id: &str,
                          app_type: &str,
                          provider_id: &str,
                          data_source: &str,
                          input_token_semantics: i64,
                          input_tokens: i64,
                          output_tokens: i64,
                          cache_read_tokens: i64,
                          cache_creation_tokens: i64|
             -> rusqlite::Result<()> {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, input_token_semantics,
                        total_cost_usd, latency_ms, status_code, created_at,
                        session_id, data_source
                     ) VALUES (?1, ?2, ?3, 'presence-model', ?4, ?5, ?6, ?7,
                               ?8, '0.25', 0, 200, ?9, 'presence-session', ?10)",
                    rusqlite::params![
                        request_id,
                        provider_id,
                        app_type,
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cache_creation_tokens,
                        input_token_semantics,
                        old_ts,
                        data_source,
                    ],
                )?;
                Ok(())
            };

            // A zero in any standard component is not source-presence proof for
            // these direct parsers, so each affected aggregate must remain NULL.
            insert(
                "presence-claude-a",
                "claude",
                "_claude_session",
                "session_log",
                0,
                10,
                0,
                2,
                0,
            )?;
            insert(
                "presence-claude-b",
                "claude",
                "_claude_session",
                "session_log",
                0,
                0,
                4,
                0,
                5,
            )?;
            insert(
                "presence-codex-a",
                "codex",
                "_codex_session",
                "codex_session",
                0,
                10,
                0,
                2,
                0,
            )?;
            insert(
                "presence-codex-b",
                "codex",
                "_codex_session",
                "codex_session",
                0,
                0,
                4,
                0,
                5,
            )?;
            insert(
                "presence-gemini-a",
                "gemini",
                "_gemini_session",
                "gemini_session",
                0,
                10,
                0,
                2,
                0,
            )?;
            insert(
                "presence-gemini-b",
                "gemini",
                "_gemini_session",
                "gemini_session",
                0,
                0,
                4,
                0,
                5,
            )?;

            // Grok's input/output/cache-read zeroes are source-proven.  Its
            // cache creation field and raw cost remain unavailable.
            insert(
                "presence-grok-a",
                "grokbuild",
                "_grok_session",
                "grok_session",
                1,
                10,
                0,
                2,
                0,
            )?;
            insert(
                "presence-grok-b",
                "grokbuild",
                "_grok_session",
                "grok_session",
                1,
                0,
                4,
                0,
                9,
            )?;

            // OpenCode proves all four generic token components, including a
            // real cache-creation zero.
            insert(
                "presence-opencode-a",
                "opencode",
                "_opencode_session",
                "opencode_session",
                0,
                10,
                0,
                2,
                0,
            )?;
            insert(
                "presence-opencode-b",
                "opencode",
                "_opencode_session",
                "opencode_session",
                0,
                0,
                4,
                0,
                9,
            )?;
        }

        assert_eq!(db.rollup_and_prune(30)?, 10);
        let conn = crate::database::lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT data_source, request_count, request_count_semantics,
                    input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, total_cost_usd
             FROM agent_session_usage_rollups
             WHERE session_id = 'presence-session'
             ORDER BY data_source",
        )?;
        let rows: Vec<(
            String,
            i64,
            String,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                (
                    "codex_session".into(),
                    2,
                    "agent_call".into(),
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                (
                    "gemini_session".into(),
                    2,
                    "assistant_message".into(),
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                (
                    "grok_session".into(),
                    2,
                    "agent_call".into(),
                    Some(10),
                    Some(4),
                    Some(2),
                    None,
                    None,
                ),
                (
                    "opencode_session".into(),
                    2,
                    "assistant_message".into(),
                    Some(10),
                    Some(4),
                    Some(2),
                    Some(9),
                    None,
                ),
                (
                    "session_log".into(),
                    2,
                    "assistant_message".into(),
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
            ]
        );
        let global_request_count: i64 = conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0)
             FROM usage_daily_rollups WHERE model = 'presence-model'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(global_request_count, 10);
        drop(stmt);
        drop(conn);

        // Raw rows have been pruned, so a repeat cannot add a second copy of
        // any durable bucket.
        assert_eq!(db.rollup_and_prune(30)?, 0);
        let conn = crate::database::lock_conn!(db.conn);
        let mut repeated_stmt = conn.prepare(
            "SELECT data_source, request_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens
             FROM agent_session_usage_rollups
             WHERE session_id = 'presence-session'
             ORDER BY data_source",
        )?;
        let repeated: Vec<(
            String,
            i64,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
        )> = repeated_stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(repeated.len(), 5);
        assert_eq!(repeated[0].1, 2);
        assert_eq!(repeated[2].2, Some(10));
        assert_eq!(repeated[3].5, Some(9));
        assert!(repeated.iter().all(|row| row.1 == 2));
        Ok(())
    }
}
