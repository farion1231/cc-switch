//! Hermes Agent session-model usage importer.
//!
//! Hermes owns `session_model_usage` and stores cumulative counters. This
//! module reads those databases as short-lived SQLite read-only connections,
//! keeps a CC Switch snapshot for each source row, and writes non-negative
//! counter deltas plus bounded signed cost adjustments to CC Switch-owned
//! tables.

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::hermes_config::get_hermes_dir;
use crate::services::session_usage::SessionSyncResult;
use chrono::Utc;
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub(crate) const HERMES_APP_TYPE: &str = "hermes";
pub(crate) const HERMES_DATA_SOURCE: &str = "hermes_session";
pub(crate) const HERMES_PRECISION: &str = "aggregate_delta";
const HERMES_UNATTRIBUTED_MAIN_TASK: &str = "unattributed_main";

/// Filters used by dashboard aggregate queries. A filter is applied only to
/// Hermes delta rows; non-Hermes queries keep their existing semantics.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HermesUsageFilters<'a> {
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub profile_name: Option<&'a str>,
    pub task: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct HermesDeltaRow {
    pub provider: String,
    pub model: String,
    pub sync_window_end: i64,
    pub api_call_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: Decimal,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HermesAggregate {
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_usd: Decimal,
}

impl HermesAggregate {
    pub(crate) fn add_row(&mut self, row: &HermesDeltaRow) -> Result<(), AppError> {
        self.request_count = checked_add(self.request_count, row.api_call_count, "请求数")?;
        self.input_tokens = checked_add(self.input_tokens, row.input_tokens, "输入 token")?;
        self.output_tokens = checked_add(self.output_tokens, row.output_tokens, "输出 token")?;
        self.cache_read_tokens = checked_add(
            self.cache_read_tokens,
            row.cache_read_tokens,
            "缓存读取 token",
        )?;
        self.cache_write_tokens = checked_add(
            self.cache_write_tokens,
            row.cache_write_tokens,
            "缓存写入 token",
        )?;
        self.reasoning_tokens =
            checked_add(self.reasoning_tokens, row.reasoning_tokens, "推理 token")?;
        self.cost_usd += row.cost_usd;
        Ok(())
    }
}

/// Hermes has no request-level status or latency facts. The dashboard can use
/// this explanation instead of rendering a fabricated request log.
pub(crate) const HERMES_AGGREGATE_EXPLANATION: &str =
    "Hermes usage is imported from cumulative session/model counters at aggregate sync-window precision; individual request status and latency are not available.";

#[derive(Debug, Clone)]
struct HermesDatabaseSource {
    profile_name: String,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct HermesSourceRow {
    session_id: String,
    model: String,
    billing_provider: String,
    billing_base_url_digest: String,
    billing_mode: String,
    task: String,
    api_call_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    estimated_cost_usd: Option<Decimal>,
    actual_cost_usd: Option<Decimal>,
    selected_cost_usd: Decimal,
    selected_cost_kind: String,
    cost_status: Option<String>,
    cost_source: Option<String>,
    first_seen: Option<String>,
    last_seen: Option<String>,
    row_key: String,
}

#[derive(Debug, Clone)]
struct HermesSnapshotState {
    api_call_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    selected_cost_usd: Decimal,
    cost_baseline_usd: Decimal,
    emitted_cost_balance_usd: Decimal,
    observed_at: i64,
}

/// Production entry point. Path resolution intentionally remains delegated to
/// the existing CC Switch Hermes authority.
pub fn sync_hermes_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_hermes_usage_at_root(db, &get_hermes_dir(), Utc::now().timestamp())
}

/// Testable sync entry point. Tests pass a temporary Hermes root and never use
/// the user's live Hermes home.
pub(crate) fn sync_hermes_usage_at_root(
    db: &Database,
    hermes_root: &Path,
    observed_at: i64,
) -> Result<SessionSyncResult, AppError> {
    let sources = discover_hermes_databases(hermes_root)?;
    let mut result = SessionSyncResult {
        files_scanned: sources.len() as u32,
        ..SessionSyncResult::default()
    };

    for source in sources {
        match import_hermes_database(db, &source, observed_at) {
            Ok((imported, skipped)) => {
                result.imported = result.imported.saturating_add(imported);
                result.skipped = result.skipped.saturating_add(skipped);
            }
            Err(error) => {
                // Keep the valid Profiles usable when one source is corrupt or
                // has not yet reached the Hermes schema.
                result.errors.push(format!(
                    "Hermes Profile '{}' 同步失败: {error}",
                    source.profile_name
                ));
            }
        }
    }

    Ok(result)
}

fn discover_hermes_databases(root: &Path) -> Result<Vec<HermesDatabaseSource>, AppError> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut sources = Vec::new();
    let default_db = root.join("state.db");
    if default_db.is_file() {
        sources.push(HermesDatabaseSource {
            profile_name: "default".to_string(),
            path: default_db,
        });
    }

    let profiles_dir = root.join("profiles");
    if profiles_dir.is_dir() {
        let entries = fs::read_dir(&profiles_dir)
            .map_err(|error| AppError::Config(format!("读取 Hermes Profiles 失败: {error}")))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| AppError::Config(format!("读取 Hermes Profile 失败: {error}")))?;
            let profile_dir = entry.path();
            if !profile_dir.is_dir() {
                continue;
            }
            let profile_name = entry.file_name().to_string_lossy().to_string();
            let state_db = profile_dir.join("state.db");
            if state_db.is_file() {
                sources.push(HermesDatabaseSource {
                    profile_name,
                    path: state_db,
                });
            }
        }
    }

    sources.sort_by(|left, right| left.profile_name.cmp(&right.profile_name));
    Ok(sources)
}

fn import_hermes_database(
    db: &Database,
    source: &HermesDatabaseSource,
    observed_at: i64,
) -> Result<(u32, u32), AppError> {
    let source_incarnation = source_file_incarnation(&source.path)?;
    let source_path_digest = digest_parts(&[&source.path.to_string_lossy()]);
    let source_id = digest_parts(&[
        HERMES_APP_TYPE,
        &source.profile_name,
        &source_path_digest,
        &source_incarnation,
    ]);

    // This connection is intentionally scoped to the read phase. It is
    // read-only, query-only, and does not use SQLite's immutable mode, so WAL
    // frames remain visible. It is dropped before the CC Switch transaction.
    let rows = read_hermes_database(&source.path)?;

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("启动 Hermes snapshot 事务失败: {error}")))?;
    let mut imported = 0u32;
    let mut skipped = 0u32;

    for row in &rows {
        let previous = load_snapshot(&tx, &source_id, &row.row_key)?;
        let current = HermesSnapshotState {
            api_call_count: row.api_call_count,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_write_tokens: row.cache_write_tokens,
            reasoning_tokens: row.reasoning_tokens,
            selected_cost_usd: row.selected_cost_usd,
            cost_baseline_usd: Decimal::ZERO,
            emitted_cost_balance_usd: Decimal::ZERO,
            observed_at,
        };

        match previous {
            None => {
                write_snapshot(
                    &tx,
                    &source_id,
                    &source_incarnation,
                    &source.profile_name,
                    row,
                    row.selected_cost_usd,
                    Decimal::ZERO,
                    observed_at,
                )?;
                skipped = skipped.saturating_add(1);
            }
            Some(previous) if counters_regressed(&previous, &current) => {
                // A counter regression is a source reset/corruption boundary.
                // Replace the baseline and never emit negative counters or a
                // mixed-incarnation cost adjustment.
                write_snapshot(
                    &tx,
                    &source_id,
                    &source_incarnation,
                    &source.profile_name,
                    row,
                    row.selected_cost_usd,
                    Decimal::ZERO,
                    observed_at,
                )?;
                skipped = skipped.saturating_add(1);
            }
            Some(previous) => {
                let delta = HermesSnapshotDelta::between(&previous, &current)?;
                write_snapshot(
                    &tx,
                    &source_id,
                    &source_incarnation,
                    &source.profile_name,
                    row,
                    previous.cost_baseline_usd,
                    delta.emitted_cost_balance_usd,
                    observed_at,
                )?;
                if delta.has_usage() {
                    insert_delta(
                        &tx,
                        &source_id,
                        &source_incarnation,
                        &source.profile_name,
                        row,
                        &delta,
                        previous.observed_at,
                        observed_at,
                    )?;
                    imported = imported.saturating_add(1);
                } else {
                    skipped = skipped.saturating_add(1);
                }
            }
        }
    }

    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Hermes snapshot 事务失败: {error}")))?;
    Ok((imported, skipped))
}

fn open_hermes_database(path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| AppError::Database(format!("打开 Hermes SQLite 失败: {error}")))?;
    conn.execute_batch("PRAGMA query_only = ON;")
        .map_err(|error| AppError::Database(format!("设置 Hermes query_only 失败: {error}")))?;
    let query_only: i64 = conn
        .query_row("PRAGMA query_only;", [], |row| row.get(0))
        .map_err(|error| AppError::Database(format!("验证 Hermes query_only 失败: {error}")))?;
    if query_only != 1 {
        return Err(AppError::Database(
            "Hermes SQLite 未启用 PRAGMA query_only=ON".to_string(),
        ));
    }
    Ok(conn)
}

fn read_hermes_database(path: &Path) -> Result<Vec<HermesSourceRow>, AppError> {
    let conn = open_hermes_database(path)?;
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("启动 Hermes 读取事务失败: {error}")))?;
    let table_exists: bool = tx
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'session_model_usage'
            )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| AppError::Database(format!("检查 Hermes usage 表失败: {error}")))?;
    if !table_exists {
        return Err(AppError::Database(
            "Hermes SQLite 缺少 session_model_usage 表".to_string(),
        ));
    }

    let mut result = {
        let mut statement = tx.prepare(
            "SELECT
                session_id, model, billing_provider, billing_base_url, billing_mode, task,
                api_call_count, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, reasoning_tokens, estimated_cost_usd, actual_cost_usd,
                cost_status, cost_source, first_seen, last_seen
             FROM session_model_usage
             ORDER BY session_id, model, billing_provider, billing_mode, task",
        )?;
        let mut rows = statement.query([])?;
        let mut detail_rows = Vec::new();
        while let Some(row) = rows.next()? {
            detail_rows.push(parse_hermes_source_row(row)?);
        }
        detail_rows
    };
    if has_session_call_count_contract(&tx)? {
        result.extend(read_unattributed_main_rows(&tx)?);
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Hermes 读取事务失败: {error}")))?;
    Ok(result)
}

fn has_session_call_count_contract(conn: &Connection) -> Result<bool, AppError> {
    let mut statement = conn.prepare("PRAGMA table_info(sessions)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_id = false;
    let mut has_api_call_count = false;
    for column in columns {
        match column?.as_str() {
            "id" => has_id = true,
            "api_call_count" => has_api_call_count = true,
            _ => {}
        }
    }
    Ok(has_id && has_api_call_count)
}

fn read_unattributed_main_rows(conn: &Connection) -> Result<Vec<HermesSourceRow>, AppError> {
    let mut statement = conn.prepare(
        "SELECT s.id, s.api_call_count,
                COALESCE(SUM(CASE WHEN usage.task = '' THEN usage.api_call_count ELSE 0 END), 0)
         FROM sessions s
         LEFT JOIN session_model_usage usage ON usage.session_id = s.id
         GROUP BY s.id, s.api_call_count
         ORDER BY s.id",
    )?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let session_id = read_required_text(row, 0, "sessions.id")?;
        let session_total = read_counter(row, 1, "sessions.api_call_count")?;
        let attributed_main = read_counter(row, 2, "main api_call_count")?;
        let residual = session_total.saturating_sub(attributed_main);
        if residual == 0 {
            continue;
        }
        result.push(HermesSourceRow {
            row_key: digest_parts(&[
                "cc_switch_reconciliation",
                &session_id,
                HERMES_UNATTRIBUTED_MAIN_TASK,
            ]),
            session_id,
            model: "unknown".to_string(),
            billing_provider: String::new(),
            billing_base_url_digest: digest_parts(&[""]),
            billing_mode: String::new(),
            task: HERMES_UNATTRIBUTED_MAIN_TASK.to_string(),
            api_call_count: residual,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            actual_cost_usd: None,
            selected_cost_usd: Decimal::ZERO,
            selected_cost_kind: "none".to_string(),
            cost_status: None,
            cost_source: None,
            first_seen: None,
            last_seen: None,
        });
    }
    Ok(result)
}

fn parse_hermes_source_row(row: &Row<'_>) -> Result<HermesSourceRow, AppError> {
    let session_id = read_required_text(row, 0, "session_id")?;
    let model = read_required_text(row, 1, "model")?;
    let billing_provider = read_required_text(row, 2, "billing_provider")?;
    let billing_base_url = read_optional_text(row, 3)?;
    let billing_mode = read_required_text(row, 4, "billing_mode")?;
    let task = read_required_text(row, 5, "task")?;
    let estimated_cost_usd = read_optional_decimal(row, 12, "estimated_cost_usd")?;
    let actual_cost_usd = read_optional_decimal(row, 13, "actual_cost_usd")?;
    let cost_status = read_optional_text(row, 14)?;
    let cost_source = read_optional_text(row, 15)?;

    let (selected_cost_usd, selected_cost_kind) =
        select_source_cost(estimated_cost_usd, actual_cost_usd, cost_status.as_deref());
    let billing_base_url_digest = digest_parts(&[billing_base_url.as_deref().unwrap_or("")]);
    let row_key = digest_parts(&[
        &session_id,
        &model,
        &billing_provider,
        &billing_base_url_digest,
        &billing_mode,
        &task,
    ]);

    Ok(HermesSourceRow {
        session_id,
        model,
        billing_provider,
        billing_base_url_digest,
        billing_mode,
        task,
        api_call_count: read_counter(row, 6, "api_call_count")?,
        input_tokens: read_counter(row, 7, "input_tokens")?,
        output_tokens: read_counter(row, 8, "output_tokens")?,
        cache_read_tokens: read_counter(row, 9, "cache_read_tokens")?,
        cache_write_tokens: read_counter(row, 10, "cache_write_tokens")?,
        reasoning_tokens: read_counter(row, 11, "reasoning_tokens")?,
        estimated_cost_usd,
        actual_cost_usd,
        selected_cost_usd,
        selected_cost_kind: selected_cost_kind.to_string(),
        cost_status,
        cost_source,
        first_seen: read_optional_text(row, 16)?,
        last_seen: read_optional_text(row, 17)?,
        row_key,
    })
}

/// Cost rule: Hermes' cumulative actual cost is authoritative only for the
/// normalized statuses below. Otherwise the cumulative estimated cost is used.
/// A transition from estimated to actual therefore compares the selected
/// cumulative totals; it never adds both fields and cannot double-count history.
fn select_source_cost(
    estimated_cost_usd: Option<Decimal>,
    actual_cost_usd: Option<Decimal>,
    cost_status: Option<&str>,
) -> (Decimal, &'static str) {
    let status = cost_status
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let actual_is_authoritative =
        matches!(status.as_str(), "actual" | "final" | "settled" | "complete");
    if actual_is_authoritative {
        if let Some(actual) = actual_cost_usd {
            return (actual, "actual");
        }
    }
    if let Some(estimated) = estimated_cost_usd {
        return (estimated, "estimated");
    }
    (Decimal::ZERO, "none")
}

fn counters_regressed(previous: &HermesSnapshotState, current: &HermesSnapshotState) -> bool {
    current.api_call_count < previous.api_call_count
        || current.input_tokens < previous.input_tokens
        || current.output_tokens < previous.output_tokens
        || current.cache_read_tokens < previous.cache_read_tokens
        || current.cache_write_tokens < previous.cache_write_tokens
        || current.reasoning_tokens < previous.reasoning_tokens
}

#[derive(Debug, Clone)]
struct HermesSnapshotDelta {
    api_call_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    cost_usd: Decimal,
    emitted_cost_balance_usd: Decimal,
    cost_delta_kind: &'static str,
}

impl HermesSnapshotDelta {
    fn between(
        previous: &HermesSnapshotState,
        current: &HermesSnapshotState,
    ) -> Result<Self, AppError> {
        let amount_above_baseline = current.selected_cost_usd - previous.cost_baseline_usd;
        let target_emitted_cost_balance_usd = if amount_above_baseline > Decimal::ZERO {
            amount_above_baseline
        } else {
            Decimal::ZERO
        };
        let cost_usd = target_emitted_cost_balance_usd - previous.emitted_cost_balance_usd;
        let cost_delta_kind = if cost_usd < Decimal::ZERO {
            "reconciliation"
        } else if cost_usd > Decimal::ZERO {
            "increase"
        } else {
            "none"
        };

        Ok(Self {
            api_call_count: checked_delta(
                current.api_call_count,
                previous.api_call_count,
                "请求数",
            )?,
            input_tokens: checked_delta(current.input_tokens, previous.input_tokens, "输入 token")?,
            output_tokens: checked_delta(
                current.output_tokens,
                previous.output_tokens,
                "输出 token",
            )?,
            cache_read_tokens: checked_delta(
                current.cache_read_tokens,
                previous.cache_read_tokens,
                "缓存读取 token",
            )?,
            cache_write_tokens: checked_delta(
                current.cache_write_tokens,
                previous.cache_write_tokens,
                "缓存写入 token",
            )?,
            reasoning_tokens: checked_delta(
                current.reasoning_tokens,
                previous.reasoning_tokens,
                "推理 token",
            )?,
            cost_usd,
            emitted_cost_balance_usd: target_emitted_cost_balance_usd,
            cost_delta_kind,
        })
    }

    fn has_usage(&self) -> bool {
        self.api_call_count > 0
            || self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_write_tokens > 0
            || self.reasoning_tokens > 0
            || self.cost_usd != Decimal::ZERO
    }
}

fn load_snapshot(
    conn: &Connection,
    source_id: &str,
    row_key: &str,
) -> Result<Option<HermesSnapshotState>, AppError> {
    let raw = conn
        .query_row(
            "SELECT api_call_count, input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, reasoning_tokens, selected_cost_usd,
                    cost_baseline_usd, emitted_cost_balance_usd, observed_at
             FROM hermes_usage_snapshots
             WHERE source_id = ?1 AND row_key = ?2",
            params![source_id, row_key],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()?;

    raw.map(
        |(
            api_call_count,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens,
            selected_cost_usd,
            cost_baseline_usd,
            emitted_cost_balance_usd,
            observed_at,
        )| {
            Ok(HermesSnapshotState {
                api_call_count,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                reasoning_tokens,
                selected_cost_usd: parse_decimal(&selected_cost_usd, "selected_cost_usd")?,
                cost_baseline_usd: parse_decimal(&cost_baseline_usd, "cost_baseline_usd")?,
                emitted_cost_balance_usd: parse_decimal(
                    &emitted_cost_balance_usd,
                    "emitted_cost_balance_usd",
                )?,
                observed_at,
            })
        },
    )
    .transpose()
}

#[allow(clippy::too_many_arguments)]
fn write_snapshot(
    conn: &Connection,
    source_id: &str,
    source_incarnation: &str,
    profile_name: &str,
    row: &HermesSourceRow,
    cost_baseline_usd: Decimal,
    emitted_cost_balance_usd: Decimal,
    observed_at: i64,
) -> Result<(), AppError> {
    let estimated_cost = row.estimated_cost_usd.map(|value| value.to_string());
    let actual_cost = row.actual_cost_usd.map(|value| value.to_string());
    conn.execute(
        "INSERT OR REPLACE INTO hermes_usage_snapshots (
            source_id, source_incarnation, profile_name, row_key,
            session_id, model, billing_provider, billing_base_url_digest,
            billing_mode, task, api_call_count, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens,
            estimated_cost_usd, actual_cost_usd, selected_cost_usd,
            cost_baseline_usd, emitted_cost_balance_usd, selected_cost_kind, cost_status,
            cost_source, first_seen, last_seen, observed_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25,
            ?26, ?27
        )",
        params![
            source_id,
            source_incarnation,
            profile_name,
            row.row_key,
            row.session_id,
            row.model,
            row.billing_provider,
            row.billing_base_url_digest,
            row.billing_mode,
            row.task,
            row.api_call_count,
            row.input_tokens,
            row.output_tokens,
            row.cache_read_tokens,
            row.cache_write_tokens,
            row.reasoning_tokens,
            estimated_cost.as_deref().unwrap_or("0"),
            actual_cost.as_deref(),
            row.selected_cost_usd.to_string(),
            format_cost(&cost_baseline_usd),
            format_cost(&emitted_cost_balance_usd),
            row.selected_cost_kind,
            row.cost_status,
            row.cost_source,
            row.first_seen,
            row.last_seen,
            observed_at,
        ],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_delta(
    conn: &Connection,
    source_id: &str,
    source_incarnation: &str,
    profile_name: &str,
    row: &HermesSourceRow,
    delta: &HermesSnapshotDelta,
    sync_window_start: i64,
    sync_window_end: i64,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO hermes_usage_deltas (
            source_id, source_incarnation, profile_name, row_key,
            session_id, provider, model, billing_base_url_digest, billing_mode, task,
            sync_window_start, sync_window_end,
            api_call_count, input_tokens, output_tokens, cache_read_tokens,
            cache_write_tokens, reasoning_tokens, cost_usd, cost_kind,
            cost_delta_kind, cost_status, cost_source, data_source, precision
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
        )",
        params![
            source_id,
            source_incarnation,
            profile_name,
            row.row_key,
            row.session_id,
            row.billing_provider,
            row.model,
            row.billing_base_url_digest,
            row.billing_mode,
            row.task,
            sync_window_start,
            sync_window_end,
            delta.api_call_count,
            delta.input_tokens,
            delta.output_tokens,
            delta.cache_read_tokens,
            delta.cache_write_tokens,
            delta.reasoning_tokens,
            delta.cost_usd.to_string(),
            row.selected_cost_kind,
            delta.cost_delta_kind,
            row.cost_status,
            row.cost_source,
            HERMES_DATA_SOURCE,
            HERMES_PRECISION,
        ],
    )?;
    Ok(())
}

/// Query the dedicated delta table. The date predicate deliberately treats a
/// sync window as an interval, so a range includes deltas whose window overlaps
/// it rather than claiming a request-level date that Hermes never supplied.
pub(crate) fn query_hermes_deltas(
    conn: &Connection,
    filters: HermesUsageFilters<'_>,
) -> Result<Vec<HermesDeltaRow>, AppError> {
    let mut conditions = vec![
        "data_source = 'hermes_session'".to_string(),
        "precision = 'aggregate_delta'".to_string(),
    ];
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(start) = filters.start_date {
        conditions.push("sync_window_end >= ?".to_string());
        values.push(Box::new(start));
    }
    if let Some(end) = filters.end_date {
        conditions.push("sync_window_start <= ?".to_string());
        values.push(Box::new(end));
    }
    if let Some(provider) = filters.provider {
        conditions.push("provider = ?".to_string());
        values.push(Box::new(provider.to_string()));
    }
    if let Some(model) = filters.model {
        conditions.push("LOWER(model) = LOWER(?)".to_string());
        values.push(Box::new(model.to_string()));
    }
    if let Some(profile_name) = filters.profile_name {
        conditions.push("profile_name = ?".to_string());
        values.push(Box::new(profile_name.to_string()));
    }
    if let Some(task) = filters.task {
        conditions.push("task = ?".to_string());
        values.push(Box::new(task.to_string()));
    }
    let sql = format!(
        "SELECT provider, model, sync_window_end,
                api_call_count, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, reasoning_tokens, cost_usd
         FROM hermes_usage_deltas
         WHERE {} ORDER BY sync_window_end, delta_id",
        conditions.join(" AND ")
    );
    let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|value| value.as_ref()).collect();
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(refs.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (
            provider,
            model,
            sync_window_end,
            api_call_count,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens,
            cost_usd,
        ) = row?;
        result.push(HermesDeltaRow {
            provider,
            model,
            sync_window_end,
            api_call_count: non_negative_i64(api_call_count, "api_call_count")? as u64,
            input_tokens: non_negative_i64(input_tokens, "input_tokens")? as u64,
            output_tokens: non_negative_i64(output_tokens, "output_tokens")? as u64,
            cache_read_tokens: non_negative_i64(cache_read_tokens, "cache_read_tokens")? as u64,
            cache_write_tokens: non_negative_i64(cache_write_tokens, "cache_write_tokens")? as u64,
            reasoning_tokens: non_negative_i64(reasoning_tokens, "reasoning_tokens")? as u64,
            cost_usd: parse_signed_decimal(&cost_usd, "cost_usd")?,
        });
    }
    Ok(result)
}

pub(crate) fn query_hermes_aggregate(
    conn: &Connection,
    filters: HermesUsageFilters<'_>,
) -> Result<HermesAggregate, AppError> {
    let mut aggregate = HermesAggregate::default();
    for row in query_hermes_deltas(conn, filters)? {
        aggregate.add_row(&row)?;
    }
    Ok(aggregate)
}

fn read_required_text(row: &Row<'_>, index: usize, name: &str) -> Result<String, AppError> {
    Ok(read_optional_text(row, index)?.unwrap_or_else(|| {
        log::debug!("Hermes usage row has NULL {name}; treating it as empty dimension");
        String::new()
    }))
}

fn read_optional_text(row: &Row<'_>, index: usize) -> Result<Option<String>, AppError> {
    let value = row
        .get_ref(index)
        .map_err(|error| AppError::Database(format!("读取 Hermes 字段失败: {error}")))?;
    match value {
        ValueRef::Null => Ok(None),
        ValueRef::Text(value) => String::from_utf8(value.to_vec())
            .map(Some)
            .map_err(|error| AppError::Database(format!("Hermes 文本字段不是 UTF-8: {error}"))),
        ValueRef::Integer(value) => Ok(Some(value.to_string())),
        ValueRef::Real(value) => Ok(Some(value.to_string())),
        // This path is only for malformed/unusual source schemas. Keep the
        // value private and stable rather than retaining binary data verbatim.
        ValueRef::Blob(value) => Ok(Some(digest_bytes(value))),
    }
}

fn read_counter(row: &Row<'_>, index: usize, name: &str) -> Result<i64, AppError> {
    let value = row
        .get_ref(index)
        .map_err(|error| AppError::Database(format!("读取 Hermes {name} 失败: {error}")))?;
    match value {
        ValueRef::Null => Ok(0),
        ValueRef::Integer(value) => non_negative_i64(value, name),
        ValueRef::Real(value) if value.is_finite() && value.fract() == 0.0 => {
            if value < 0.0 || value > i64::MAX as f64 {
                Err(AppError::Database(format!(
                    "Hermes {name} 超出非负整数范围"
                )))
            } else {
                Ok(value as i64)
            }
        }
        ValueRef::Text(value) => {
            let text = std::str::from_utf8(value).map_err(|error| {
                AppError::Database(format!("Hermes {name} 不是有效文本: {error}"))
            })?;
            let parsed = text.trim().parse::<u64>().map_err(|error| {
                AppError::Database(format!("Hermes {name} 不是非负整数: {error}"))
            })?;
            i64::try_from(parsed)
                .map_err(|_| AppError::Database(format!("Hermes {name} 超出 SQLite INTEGER 范围")))
        }
        _ => Err(AppError::Database(format!("Hermes {name} 不是非负整数"))),
    }
}

fn read_optional_decimal(
    row: &Row<'_>,
    index: usize,
    name: &str,
) -> Result<Option<Decimal>, AppError> {
    read_optional_text(row, index)?
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_decimal(&value, name))
        .transpose()
}

fn parse_decimal(value: &str, name: &str) -> Result<Decimal, AppError> {
    let parsed = parse_signed_decimal(value, name)?;
    if parsed < Decimal::ZERO {
        return Err(AppError::Database(format!("Hermes {name} 不能为负数")));
    }
    Ok(parsed)
}

fn parse_signed_decimal(value: &str, name: &str) -> Result<Decimal, AppError> {
    Decimal::from_str(value.trim())
        .map_err(|error| AppError::Database(format!("Hermes {name} 不是有效成本: {error}")))
}

fn non_negative_i64(value: i64, name: &str) -> Result<i64, AppError> {
    if value < 0 {
        Err(AppError::Database(format!("Hermes {name} 不能为负数")))
    } else {
        Ok(value)
    }
}

fn checked_delta(current: i64, previous: i64, name: &str) -> Result<i64, AppError> {
    current
        .checked_sub(previous)
        .ok_or_else(|| AppError::Database(format!("计算 Hermes {name} delta 溢出")))
}

fn checked_add(current: u64, addition: u64, name: &str) -> Result<u64, AppError> {
    current
        .checked_add(addition)
        .ok_or_else(|| AppError::Database(format!("汇总 Hermes {name} 溢出")))
}

fn digest_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    digest_bytes(&hasher.finalize())
}

fn digest_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn linux_file_birth_time(path: &Path) -> Option<(i64, u32)> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = MaybeUninit::<libc::statx>::zeroed();
    // SAFETY: `path` is NUL-terminated and `stat` points to writable storage for
    // one `libc::statx` value. The value is only read after a successful call.
    let result = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            path.as_ptr(),
            libc::AT_STATX_SYNC_AS_STAT,
            libc::STATX_BTIME,
            stat.as_mut_ptr(),
        )
    };
    if result != 0 {
        return None;
    }

    // SAFETY: statx returned success and initialized the output structure.
    let stat = unsafe { stat.assume_init() };
    (stat.stx_mask & libc::STATX_BTIME != 0)
        .then_some((stat.stx_btime.tv_sec, stat.stx_btime.tv_nsec))
}

fn source_file_incarnation(path: &Path) -> Result<String, AppError> {
    let metadata = fs::metadata(path)
        .map_err(|error| AppError::Database(format!("读取 Hermes SQLite 元数据失败: {error}")))?;
    let identity = {
        #[cfg(all(unix, not(all(target_os = "linux", target_env = "gnu"))))]
        {
            use std::os::unix::fs::MetadataExt;
            format!("unix:{}:{}", metadata.dev(), metadata.ino())
        }
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            use std::os::unix::fs::MetadataExt;

            if let Some((seconds, nanoseconds)) = linux_file_birth_time(path) {
                format!(
                    "unix:{}:{}:{seconds}:{nanoseconds}",
                    metadata.dev(),
                    metadata.ino()
                )
            } else {
                format!("unix:{}:{}", metadata.dev(), metadata.ino())
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
            };

            // Windows 上单独依赖 creation_time 不可靠：NTFS 在快速“删除后重建”
            // 同一路径文件时可能复用同一 100ns 创建时刻（MFT 记录被立即重用），
            // 导致被替换的数据库被误判为同一文件而无法开启新 baseline。
            // 改用文件系统文件 ID（卷序列号 + 文件索引）作为主信号，
            // 叠加 creation_time 作为双保险；两者同时碰撞的概率可忽略。
            let file = std::fs::File::open(path).map_err(|error| {
                AppError::Database(format!("读取 Hermes SQLite 元数据失败: {error}"))
            })?;
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
            if ok == 0 {
                format!("windows-fallback:{}", metadata.creation_time())
            } else {
                format!(
                    "windows:{}:{:016x}{:016x}:{}",
                    info.dwVolumeSerialNumber,
                    info.nFileIndexHigh,
                    info.nFileIndexLow,
                    metadata.creation_time()
                )
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|value| value.as_nanos().to_string())
                .unwrap_or_default();
            format!("fallback:{}:{}", metadata.len(), modified)
        }
    };
    Ok(digest_parts(&[&identity]))
}

fn format_cost(value: &Decimal) -> String {
    format!("{value:.6}")
}

pub(crate) fn aggregate_cost_string(value: &Decimal) -> String {
    format_cost(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use rusqlite::Connection;
    use tempfile::tempdir;

    const SOURCE_SCHEMA: &str = "
        CREATE TABLE session_model_usage (
            session_id TEXT NOT NULL,
            model TEXT NOT NULL,
            billing_provider TEXT NOT NULL,
            billing_base_url TEXT,
            billing_mode TEXT NOT NULL,
            task TEXT NOT NULL,
            api_call_count INTEGER NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            cache_write_tokens INTEGER NOT NULL,
            reasoning_tokens INTEGER NOT NULL,
            estimated_cost_usd TEXT,
            actual_cost_usd TEXT,
            cost_status TEXT,
            cost_source TEXT,
            first_seen TEXT,
            last_seen TEXT,
            PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
        );";

    fn source_db(path: &Path, profile_row: &str) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(SOURCE_SCHEMA).unwrap();
        insert_source_row(
            &conn,
            profile_row,
            1,
            100,
            50,
            10,
            20,
            7,
            "1.000000",
            None,
            "estimated",
        );
        conn
    }

    fn insert_source_row(
        conn: &Connection,
        task: &str,
        calls: i64,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
        reasoning: i64,
        estimated: &str,
        actual: Option<&str>,
        status: &str,
    ) {
        conn.execute(
            "INSERT INTO session_model_usage (
                session_id, model, billing_provider, billing_base_url, billing_mode, task,
                api_call_count, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, reasoning_tokens, estimated_cost_usd, actual_cost_usd,
                cost_status, cost_source, first_seen, last_seen
            ) VALUES ('session-1', 'model-a', 'provider-a', 'https://private.example/v1',
                      'chat', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                      'hermes', '2026-08-03T00:00:00Z', '2026-08-03T00:01:00Z')",
            params![
                task,
                calls,
                input,
                output,
                cache_read,
                cache_write,
                reasoning,
                estimated,
                actual,
                status
            ],
        )
        .unwrap();
    }

    fn update_source_row(
        conn: &Connection,
        calls: i64,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
        reasoning: i64,
        estimated: &str,
        actual: Option<&str>,
        status: &str,
    ) {
        conn.execute(
            "UPDATE session_model_usage SET api_call_count=?1, input_tokens=?2,
             output_tokens=?3, cache_read_tokens=?4, cache_write_tokens=?5,
             reasoning_tokens=?6, estimated_cost_usd=?7, actual_cost_usd=?8,
             cost_status=?9 WHERE session_id='session-1'",
            params![
                calls,
                input,
                output,
                cache_read,
                cache_write,
                reasoning,
                estimated,
                actual,
                status
            ],
        )
        .unwrap();
    }

    fn add_session_call_total(conn: &Connection, session_id: &str, calls: i64) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                api_call_count INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sessions (id, api_call_count) VALUES (?1, ?2)",
            params![session_id, calls],
        )
        .unwrap();
    }

    fn run_sync(db: &Database, root: &Path, at: i64) -> SessionSyncResult {
        sync_hermes_usage_at_root(db, root, at).unwrap()
    }

    #[test]
    fn discovers_default_and_immediate_named_profiles_only() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("profiles/one")).unwrap();
        fs::create_dir_all(root.path().join("profiles/two")).unwrap();
        fs::create_dir_all(root.path().join("profiles/deep/nested")).unwrap();
        let _default = source_db(&root.path().join("state.db"), "default-task");
        let _one = source_db(&root.path().join("profiles/one/state.db"), "one-task");
        let _two = source_db(&root.path().join("profiles/two/state.db"), "two-task");
        let _deep = source_db(
            &root.path().join("profiles/deep/nested/state.db"),
            "ignored-task",
        );
        fs::write(
            root.path().join("profiles/unrelated.sqlite"),
            b"not a database",
        )
        .unwrap();

        let sources = discover_hermes_databases(root.path()).unwrap();
        let labels: Vec<_> = sources
            .iter()
            .map(|source| source.profile_name.as_str())
            .collect();
        assert_eq!(labels, vec!["default", "one", "two"]);
    }

    #[test]
    fn opens_read_only_and_sees_committed_wal_data() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        writer.execute_batch(SOURCE_SCHEMA).unwrap();
        insert_source_row(
            &writer,
            "wal-task",
            4,
            400,
            50,
            10,
            20,
            7,
            "2.000000",
            None,
            "estimated",
        );

        let read_only = open_hermes_database(&path).unwrap();
        let query_only: i64 = read_only
            .query_row("PRAGMA query_only;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(query_only, 1);
        assert!(read_only
            .execute("CREATE TABLE should_not_be_created (id INTEGER)", [])
            .is_err());
        drop(read_only);

        let rows = read_hermes_database(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].api_call_count, 4);
        assert_eq!(rows[0].task, "wal-task");
        drop(writer);
    }

    #[test]
    fn session_total_adds_only_unattributed_main_residual() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch(SOURCE_SCHEMA).unwrap();
        insert_source_row(
            &writer,
            "",
            6,
            600,
            60,
            0,
            0,
            0,
            "3.000000",
            None,
            "estimated",
        );
        insert_source_row(
            &writer,
            "compression",
            3,
            300,
            30,
            0,
            0,
            0,
            "1.500000",
            None,
            "estimated",
        );
        add_session_call_total(&writer, "session-1", 10);

        let rows = read_hermes_database(&path).unwrap();
        assert_eq!(rows.len(), 3);
        let residual = rows
            .iter()
            .find(|row| row.task == "unattributed_main")
            .unwrap();
        assert_eq!(residual.api_call_count, 4);
        assert_eq!(residual.model, "unknown");
        assert_eq!(residual.billing_provider, "");
        assert_eq!(residual.billing_base_url_digest, digest_parts(&[""]));
        assert_eq!(residual.billing_mode, "");
        assert_eq!(residual.input_tokens, 0);
        assert_eq!(residual.output_tokens, 0);
        assert_eq!(residual.cache_read_tokens, 0);
        assert_eq!(residual.cache_write_tokens, 0);
        assert_eq!(residual.reasoning_tokens, 0);
        assert_eq!(residual.selected_cost_usd, Decimal::ZERO);
    }

    #[test]
    fn covered_total_or_missing_session_contract_adds_no_residual() {
        let root = tempdir().unwrap();
        let covered_path = root.path().join("covered.db");
        let covered = source_db(&covered_path, "");
        add_session_call_total(&covered, "session-1", 1);
        let covered_rows = read_hermes_database(&covered_path).unwrap();
        assert_eq!(covered_rows.len(), 1);
        assert!(covered_rows
            .iter()
            .all(|row| row.task != "unattributed_main"));

        let legacy_path = root.path().join("legacy.db");
        let _legacy = source_db(&legacy_path, "");
        let legacy_rows = read_hermes_database(&legacy_path).unwrap();
        assert_eq!(legacy_rows.len(), 1);
        assert!(legacy_rows
            .iter()
            .all(|row| row.task != "unattributed_main"));
    }

    #[test]
    fn session_residual_increase_is_emitted_once() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch(SOURCE_SCHEMA).unwrap();
        insert_source_row(
            &writer,
            "",
            6,
            600,
            60,
            0,
            0,
            0,
            "3.000000",
            None,
            "estimated",
        );
        insert_source_row(
            &writer,
            "compression",
            3,
            300,
            30,
            0,
            0,
            0,
            "1.500000",
            None,
            "estimated",
        );
        add_session_call_total(&writer, "session-1", 10);
        let db = Database::memory().unwrap();

        let first = run_sync(&db, root.path(), 100);
        assert_eq!(first.imported, 0);
        writer
            .execute(
                "UPDATE sessions SET api_call_count = 12 WHERE id = 'session-1'",
                [],
            )
            .unwrap();

        let second = run_sync(&db, root.path(), 200);
        assert_eq!(second.imported, 1);
        let conn = db.conn.lock().unwrap();
        let delta: (String, String, String, i64, i64, String) = conn
            .query_row(
                "SELECT task, provider, model, api_call_count, input_tokens, cost_usd
                 FROM hermes_usage_deltas",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            delta,
            (
                "unattributed_main".to_string(),
                "".to_string(),
                "unknown".to_string(),
                2,
                0,
                "0.000000".to_string(),
            )
        );
        drop(conn);

        let third = run_sync(&db, root.path(), 300);
        assert_eq!(third.imported, 0);
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM hermes_usage_deltas", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn first_sync_baselines_then_imports_exact_delta_and_is_idempotent() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        let db = Database::memory().unwrap();

        let first = run_sync(&db, root.path(), 100);
        assert_eq!(first.imported, 0);
        assert_eq!(first.errors.len(), 0);

        update_source_row(
            &writer,
            4,
            400,
            80,
            15,
            25,
            12,
            "2.000000",
            None,
            "estimated",
        );
        let second = run_sync(&db, root.path(), 200);
        assert_eq!(second.imported, 1);

        let conn = db.conn.lock().unwrap();
        let delta: (i64, i64, i64, i64, i64, i64, String) = conn
            .query_row(
                "SELECT api_call_count, input_tokens, output_tokens, cache_read_tokens,
                        cache_write_tokens, reasoning_tokens, cost_usd
                 FROM hermes_usage_deltas",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(delta, (3, 300, 30, 5, 5, 5, "1.000000".to_string()));
        drop(conn);

        let repeated = run_sync(&db, root.path(), 300);
        assert_eq!(repeated.imported, 0);
        let conn = db.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM hermes_usage_deltas", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn identical_rows_in_profiles_do_not_collide() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("profiles/one")).unwrap();
        let default_path = root.path().join("state.db");
        let named_path = root.path().join("profiles/one/state.db");
        let default_writer = source_db(&default_path, "same-task");
        let named_writer = source_db(&named_path, "same-task");
        let db = Database::memory().unwrap();

        run_sync(&db, root.path(), 100);
        update_source_row(
            &named_writer,
            3,
            200,
            70,
            15,
            20,
            10,
            "1.500000",
            None,
            "estimated",
        );
        let result = run_sync(&db, root.path(), 200);
        assert_eq!(result.imported, 1);
        drop(default_writer);

        let conn = db.conn.lock().unwrap();
        let profiles: Vec<String> = conn
            .prepare("SELECT profile_name FROM hermes_usage_snapshots ORDER BY profile_name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(profiles, vec!["default", "one"]);
    }

    #[test]
    fn replacing_database_at_same_path_starts_new_baseline() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        {
            let writer = source_db(&path, "task-a");
            let db = Database::memory().unwrap();
            run_sync(&db, root.path(), 100);
            drop(writer);

            fs::remove_file(&path).unwrap();
            // 模拟真实替换间隔，避免 Windows 上“删除后立即重建”撞上
            // 文件系统元数据复用窗口（见 source_file_incarnation 的说明）。
            std::thread::sleep(std::time::Duration::from_millis(20));
            let replacement = source_db(&path, "task-a");
            let result = run_sync(&db, root.path(), 200);
            assert_eq!(result.imported, 0);
            let conn = db.conn.lock().unwrap();
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM hermes_usage_deltas", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(DISTINCT source_incarnation) FROM hermes_usage_snapshots",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                2
            );
            drop(conn);
            drop(replacement);
        }
    }

    #[cfg(windows)]
    #[test]
    fn source_file_incarnation_is_stable_for_updates_and_changes_for_replacement() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        fs::write(&path, b"initial").unwrap();

        let initial = source_file_incarnation(&path).unwrap();
        assert_eq!(initial, source_file_incarnation(&path).unwrap());

        fs::write(&path, b"updated content").unwrap();
        assert_eq!(initial, source_file_incarnation(&path).unwrap());

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert_ne!(initial, source_file_incarnation(&path).unwrap());
    }

    #[test]
    fn regression_resets_baseline_without_negative_delta() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        let db = Database::memory().unwrap();
        run_sync(&db, root.path(), 100);
        update_source_row(&writer, 0, 20, 10, 1, 2, 1, "0.100000", None, "estimated");
        let regression = run_sync(&db, root.path(), 200);
        assert_eq!(regression.imported, 0);
        update_source_row(&writer, 2, 30, 20, 2, 3, 2, "0.200000", None, "estimated");
        let after_reset = run_sync(&db, root.path(), 300);
        assert_eq!(after_reset.imported, 1);
        let conn = db.conn.lock().unwrap();
        let delta: (i64, String) = conn
            .query_row(
                "SELECT api_call_count, cost_usd FROM hermes_usage_deltas",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(delta, (2, "0.100000".to_string()));
    }

    #[test]
    fn cost_transition_uses_selected_cumulative_cost_once() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        let db = Database::memory().unwrap();
        run_sync(&db, root.path(), 100);
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "1.000000",
            Some("1.200000"),
            "actual",
        );
        run_sync(&db, root.path(), 200);
        update_source_row(
            &writer,
            3,
            120,
            60,
            12,
            22,
            9,
            "1.500000",
            Some("1.200000"),
            "actual",
        );
        run_sync(&db, root.path(), 300);
        let conn = db.conn.lock().unwrap();
        let (count, cost): (i64, String) = conn.query_row("SELECT COUNT(*), COALESCE(SUM(CAST(cost_usd AS REAL)), 0) FROM hermes_usage_deltas", [], |row| Ok((row.get(0)?, row.get::<_, f64>(1)?.to_string()))).unwrap();
        assert_eq!(count, 2);
        assert_eq!(cost.parse::<f64>().unwrap(), 0.2);
    }

    #[test]
    fn actual_cost_reconciliation_rolls_back_previously_imported_estimate() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        let db = Database::memory().unwrap();

        // The initial $1 estimate is a baseline and is not imported.
        run_sync(&db, root.path(), 100);
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "3.000000",
            None,
            "estimated",
        );
        assert_eq!(run_sync(&db, root.path(), 200).imported, 1);

        // Hermes finalizes the cumulative cost at $2. Only the previously
        // imported $2 estimate is eligible for rollback, so emit -$1 without
        // inventing negative calls or tokens.
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "3.000000",
            Some("2.000000"),
            "actual",
        );
        assert_eq!(run_sync(&db, root.path(), 300).imported, 1);

        let conn = db.conn.lock().unwrap();
        let deltas: Vec<(i64, i64, String)> = conn
            .prepare(
                "SELECT api_call_count, input_tokens, cost_usd
                 FROM hermes_usage_deltas ORDER BY delta_id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            deltas,
            vec![
                (1, 10, "2.000000".to_string()),
                (0, 0, "-1.000000".to_string()),
            ]
        );
        let net_cost: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(CAST(cost_usd AS REAL)), 0)
                 FROM hermes_usage_deltas",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(net_cost, 1.0);

        let adjustment_window = query_hermes_aggregate(
            &conn,
            HermesUsageFilters {
                start_date: Some(201),
                end_date: Some(300),
                provider: None,
                model: None,
                profile_name: None,
                task: None,
            },
        )
        .unwrap();
        assert_eq!(adjustment_window.request_count, 0);
        assert_eq!(adjustment_window.cost_usd, Decimal::NEGATIVE_ONE);
    }

    #[test]
    fn actual_cost_reconciliation_never_rolls_back_unimported_baseline() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        let db = Database::memory().unwrap();

        // Baseline $1 is deliberately excluded from CC Switch totals.
        run_sync(&db, root.path(), 100);
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "3.000000",
            None,
            "estimated",
        );
        assert_eq!(run_sync(&db, root.path(), 200).imported, 1);

        // A raw $3 downward correction may only reverse the $2 emitted after
        // baseline. The unimported $1 baseline must not make the dashboard
        // total negative.
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "3.000000",
            Some("0.000000"),
            "actual",
        );
        assert_eq!(run_sync(&db, root.path(), 300).imported, 1);

        let conn = db.conn.lock().unwrap();
        let deltas: Vec<String> = conn
            .prepare("SELECT cost_usd FROM hermes_usage_deltas ORDER BY delta_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            deltas,
            vec!["2.000000".to_string(), "-2.000000".to_string()]
        );
        let (net_cost, balance, kind): (f64, String, String) = conn
            .query_row(
                "SELECT
                    (SELECT COALESCE(SUM(CAST(cost_usd AS REAL)), 0)
                     FROM hermes_usage_deltas),
                    emitted_cost_balance_usd,
                    (SELECT cost_delta_kind FROM hermes_usage_deltas
                     ORDER BY delta_id DESC LIMIT 1)
                 FROM hermes_usage_snapshots",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(net_cost, 0.0);
        assert_eq!(balance, "0.000000");
        assert_eq!(kind, "reconciliation");
        drop(conn);

        // Returning to the original $1 baseline must not import historical
        // baseline cost a second time.
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "3.000000",
            Some("1.000000"),
            "actual",
        );
        assert_eq!(run_sync(&db, root.path(), 400).imported, 0);

        // Only the amount above the original baseline is newly importable.
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "3.000000",
            Some("1.500000"),
            "actual",
        );
        assert_eq!(run_sync(&db, root.path(), 500).imported, 1);
        let conn = db.conn.lock().unwrap();
        let deltas: Vec<String> = conn
            .prepare("SELECT cost_usd FROM hermes_usage_deltas ORDER BY delta_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            deltas,
            vec![
                "2.000000".to_string(),
                "-2.000000".to_string(),
                "0.500000".to_string(),
            ]
        );
    }

    #[test]
    fn bad_profile_and_missing_table_do_not_block_valid_profile() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("profiles/bad")).unwrap();
        fs::create_dir_all(root.path().join("profiles/missing")).unwrap();
        let valid_path = root.path().join("state.db");
        let valid_writer = source_db(&valid_path, "valid-task");
        fs::write(root.path().join("profiles/bad/state.db"), b"not sqlite").unwrap();
        Connection::open(root.path().join("profiles/missing/state.db"))
            .unwrap()
            .execute_batch("CREATE TABLE other (id INTEGER);")
            .unwrap();
        let db = Database::memory().unwrap();

        let first = run_sync(&db, root.path(), 100);
        assert_eq!(first.errors.len(), 2);
        update_source_row(
            &valid_writer,
            2,
            110,
            55,
            11,
            21,
            8,
            "1.100000",
            None,
            "estimated",
        );
        let second = run_sync(&db, root.path(), 200);
        assert_eq!(second.imported, 1);
        assert_eq!(second.errors.len(), 2);
    }

    #[test]
    fn billing_base_url_is_stored_only_as_digest() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        let db = Database::memory().unwrap();
        run_sync(&db, root.path(), 100);
        drop(writer);

        let read_only = open_hermes_database(&path).unwrap();
        let query_only: i64 = read_only
            .query_row("PRAGMA query_only;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(query_only, 1);
        assert!(read_only
            .execute("CREATE TABLE should_not_be_created (id INTEGER)", [])
            .is_err());
        drop(read_only);

        let conn = db.conn.lock().unwrap();
        let digest: String = conn
            .query_row(
                "SELECT billing_base_url_digest FROM hermes_usage_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(digest, "https://private.example/v1");
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains("private"));
    }

    #[test]
    fn source_cost_rule_is_deterministic() {
        assert_eq!(
            select_source_cost(
                Some(Decimal::new(100, 2)),
                Some(Decimal::new(120, 2)),
                Some("actual")
            ),
            (Decimal::new(120, 2), "actual")
        );
        assert_eq!(
            select_source_cost(
                Some(Decimal::new(100, 2)),
                Some(Decimal::new(120, 2)),
                Some("estimated")
            ),
            (Decimal::new(100, 2), "estimated")
        );
        assert_eq!(
            select_source_cost(None, Some(Decimal::new(120, 2)), Some("unknown")),
            (Decimal::ZERO, "none")
        );
    }
}
