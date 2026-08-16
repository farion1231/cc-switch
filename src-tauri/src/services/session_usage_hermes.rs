//! Read-only Hermes `session_model_usage` importer.
//!
//! Hermes owns the source SQLite database and stores cumulative counters.  This
//! adapter never writes that database (including WAL checkpoints).  It probes
//! the schema before selecting rows, keeps a durable snapshot through the
//! T01 snapshot DAO, and exposes only non-negative sync-window deltas.  The
//! `task` column is retained as a source dimension; it is deliberately not
//! interpreted as a parent/child relationship. It also never writes
//! `proxy_request_logs` or pretends cumulative snapshots are request events.

use crate::database::{AgentSessionUsageSnapshot, AgentSessionUsageSnapshotKey, Database};
use crate::error::AppError;
use crate::hermes_config::get_hermes_dir;
use crate::services::agent_session_usage::{
    write_agent_session_usage_hermes_delta_on_conn, NormalizedUsageRollupFact,
    NormalizedUsageSnapshot, RequestCountSemantics, SessionNodeMetadata, SessionRelationClaim,
    TimeSemantics, UsagePrecision,
};
use crate::services::session_usage::SessionSyncResult;
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use chrono::{DateTime, Utc};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, Row};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub(crate) const HERMES_APP_TYPE: &str = "hermes";
pub(crate) const HERMES_DATA_SOURCE: &str = "hermes_session_model_usage";
pub(crate) const HERMES_SOURCE_VERSION: &str = "session_model_usage:v1";
pub(crate) const HERMES_PRECISION: &str = "sync_window_delta";
pub(crate) const HERMES_TIME_SEMANTICS: &str = "sync_window_end";
pub(crate) const HERMES_REQUEST_COUNT_SEMANTICS: &str = "unavailable";
/// Hermes' canonical usage normalizer stores `input_tokens` after removing
/// cache-read and cache-write components. This is source evidence, not an
/// app-type inference, and is persisted separately from the token values.
pub(crate) const HERMES_INPUT_TOKEN_SEMANTICS: i64 = INPUT_TOKEN_SEMANTICS_FRESH;

/// One source row's non-negative sync-window delta.  This is intentionally
/// richer than the generic rollup contract: Hermes reasoning tokens and its
/// billing/task dimensions must not be silently discarded or mapped to HTTP
/// request fields. The same full dimensions are durably persisted by the v20
/// fact bridge; T12 can query this result without treating it as request data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HermesUsageDelta {
    pub source_identity: String,
    pub profile_id: String,
    pub database_identity: String,
    pub session_id: String,
    /// Canonical node/rollup identity. The raw Hermes session ID is retained
    /// above for source diagnostics, while this value prevents identical IDs
    /// in different profiles/database incarnations from colliding in T03's
    /// `(app_type, session_id)` node key.
    pub canonical_session_id: String,
    pub model: String,
    pub provider_id: String,
    pub base_url_digest: String,
    pub billing_mode: String,
    pub task: String,
    pub data_source: String,
    pub source_version: String,
    pub precision: &'static str,
    pub time_semantics: &'static str,
    pub request_count_semantics: &'static str,
    pub input_token_semantics: i64,
    pub sync_window_start: i64,
    pub sync_window_end: i64,
    pub api_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost_usd: Option<Decimal>,
    pub cost_kind: CostKind,
    pub cost_delta_kind: CostDeltaKind,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    /// Hermes source timestamps are normalized to milliseconds for node/UI
    /// metadata. They are converted to Unix seconds only when facts are
    /// persisted to `first_event_at`/`last_event_at`.
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostKind {
    Actual,
    Estimated,
    ExplicitZero,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostDeltaKind {
    Increase,
    Reconciliation,
    None,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HermesSyncResult {
    pub observed_at: i64,
    pub profiles_scanned: u32,
    pub rows_scanned: u32,
    pub baselined: u32,
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
    pub deltas: Vec<HermesUsageDelta>,
    /// All discovered session identities, including first-sync baselines that
    /// intentionally emit no usage delta. T12 can persist these as standalone
    /// Hermes nodes without treating task rows as child relationships.
    pub sessions: Vec<HermesSessionIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HermesSessionIdentity {
    pub profile_id: String,
    pub database_identity: String,
    pub session_id: String,
    pub canonical_session_id: String,
    pub model: String,
    pub provider_id: String,
    pub base_url_digest: String,
    pub billing_mode: String,
    pub task: String,
    pub title: Option<String>,
    pub project_dir: Option<String>,
    /// Source/UI timestamps remain milliseconds; facts use explicit seconds.
    pub first_seen_ms: Option<i64>,
    pub last_seen_ms: Option<i64>,
}

impl HermesSyncResult {
    pub(crate) fn as_session_sync_result(&self) -> SessionSyncResult {
        SessionSyncResult {
            imported: self.imported,
            skipped: self.skipped.saturating_add(self.baselined),
            files_scanned: self.profiles_scanned,
            errors: self.errors.clone(),
            ..SessionSyncResult::default()
        }
    }
}

#[derive(Debug, Clone)]
struct HermesDatabaseSource {
    profile_id: String,
    path: PathBuf,
    sessions_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct HermesNativeMetadata {
    title: Option<String>,
    project_dir: Option<String>,
}

#[derive(Debug, Clone)]
struct HermesSourceRow {
    session_id: String,
    model: String,
    provider_id: String,
    base_url_digest: String,
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
    cost_status: Option<String>,
    cost_source: Option<String>,
    first_seen_ms: Option<i64>,
    last_seen_ms: Option<i64>,
    selected_cost_usd: Option<Decimal>,
    selected_cost_kind: CostKind,
}

#[derive(Debug, Clone)]
struct SnapshotCounters {
    api_call_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    selected_cost_usd: Option<Decimal>,
    cost_baseline_usd: Option<Decimal>,
    emitted_cost_balance_usd: Decimal,
    last_synced_at: i64,
}

#[derive(Debug, Clone, Default)]
struct CostState {
    baseline: Option<Decimal>,
    emitted: Decimal,
}

#[derive(Debug, Clone)]
struct HermesSnapshotDelta {
    api_call_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    cost_usd: Option<Decimal>,
    cost_baseline_usd: Option<Decimal>,
    emitted_cost_balance_usd: Decimal,
    cost_delta_kind: CostDeltaKind,
}

/// Production entry point.  Tests use [`sync_hermes_usage_at_root`] with an
/// anonymous temporary directory and never touch the user's `~/.hermes` DB.
#[cfg(test)]
pub(crate) fn sync_hermes_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_hermes_usage_detailed(db).map(|result| result.as_session_sync_result())
}

pub(crate) fn sync_hermes_usage_detailed(db: &Database) -> Result<HermesSyncResult, AppError> {
    sync_hermes_usage_at_root(db, &get_hermes_dir(), Utc::now().timestamp())
}

pub(crate) fn sync_hermes_usage_at_root(
    db: &Database,
    hermes_root: &Path,
    observed_at: i64,
) -> Result<HermesSyncResult, AppError> {
    let sources = discover_hermes_databases(hermes_root)?;
    let mut result = HermesSyncResult {
        profiles_scanned: sources.len() as u32,
        observed_at,
        ..HermesSyncResult::default()
    };
    for source in sources {
        match import_hermes_database(db, &source, observed_at) {
            Ok((rows, baselined, imported, skipped, deltas, sessions)) => {
                result.rows_scanned = result.rows_scanned.saturating_add(rows);
                result.baselined = result.baselined.saturating_add(baselined);
                result.imported = result.imported.saturating_add(imported);
                result.skipped = result.skipped.saturating_add(skipped);
                result.deltas.extend(deltas);
                result.sessions.extend(sessions);
            }
            Err(error) => result.errors.push(format!(
                "Hermes profile '{}' 同步失败: {error}",
                source.profile_id
            )),
        }
    }
    Ok(result)
}

/// Convert discovered Hermes rows into standalone normalized nodes. Hermes has
/// no proven parent field in `session_model_usage`; task/model rows therefore
/// never become child claims. Duplicate rows from different billing tasks are
/// collapsed by the canonical profile/database/session identity.
pub(crate) fn hermes_standalone_session_claims(
    result: &HermesSyncResult,
) -> Vec<SessionRelationClaim> {
    let mut seen = HashSet::new();
    result
        .sessions
        .iter()
        .filter_map(|session| {
            if !seen.insert(session.canonical_session_id.clone()) {
                return None;
            }
            Some(SessionRelationClaim {
                app_type: HERMES_APP_TYPE.to_string(),
                session_id: session.canonical_session_id.clone(),
                relation: crate::services::agent_session_usage::RelationClaim::Standalone,
                metadata: SessionNodeMetadata {
                    title: session.title.clone(),
                    project_dir: session.project_dir.clone(),
                    source_path: Some(format!(
                        "sqlite:{}#{}",
                        session.database_identity, session.session_id
                    )),
                    created_at: session.first_seen_ms,
                    last_active_at: session.last_seen_ms,
                    last_synced_at: result.observed_at,
                },
            })
        })
        .collect()
}

/// Discover exactly the default state database and immediate
/// `profiles/<id>/state.db` databases.  Invalid/absent profiles are isolated
/// by the caller; no recursive glob or JSONL source is mixed into this path.
fn discover_hermes_databases(root: &Path) -> Result<Vec<HermesDatabaseSource>, AppError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut sources = Vec::new();
    let default_db = root.join("state.db");
    if default_db.is_file() {
        sources.push(HermesDatabaseSource {
            profile_id: "default".to_string(),
            path: default_db,
            sessions_dir: root.join("sessions"),
        });
    }
    let profiles_dir = root.join("profiles");
    if profiles_dir.is_dir() {
        for entry in
            fs::read_dir(&profiles_dir).map_err(|error| AppError::io(&profiles_dir, error))?
        {
            let entry = entry.map_err(|error| AppError::io(&profiles_dir, error))?;
            let profile_dir = entry.path();
            if !profile_dir.is_dir() {
                continue;
            }
            let profile_id = entry.file_name().to_string_lossy().trim().to_string();
            if profile_id.is_empty() {
                continue;
            }
            let state_db = profile_dir.join("state.db");
            if state_db.is_file() {
                sources.push(HermesDatabaseSource {
                    profile_id,
                    path: state_db,
                    sessions_dir: root.join("sessions"),
                });
            }
        }
    }
    sources.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    Ok(sources)
}

/// Load only Hermes' native session title and directory fields. The usage
/// table is keyed by session ID, while Hermes stores user-visible metadata in
/// its `sessions` table or session JSONL files. Message content is never
/// inspected and cannot become a fabricated title.
fn load_hermes_native_metadata(
    source: &HermesDatabaseSource,
) -> HashMap<String, HermesNativeMetadata> {
    let mut metadata = load_hermes_sqlite_session_metadata(&source.path);
    if let Ok(entries) = fs::read_dir(&source.sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let extension = path.extension().and_then(|value| value.to_str());
            if extension != Some("jsonl") && extension != Some("json") {
                continue;
            }
            let Some((session_id, native)) = read_hermes_session_file_metadata(&path) else {
                continue;
            };
            let existing = metadata.entry(session_id).or_default();
            if existing.title.is_none() {
                existing.title = native.title;
            }
            if existing.project_dir.is_none() {
                existing.project_dir = native.project_dir;
            }
        }
    }
    metadata
}

fn load_hermes_sqlite_session_metadata(path: &Path) -> HashMap<String, HermesNativeMetadata> {
    let mut metadata = HashMap::new();
    let Ok(conn) = open_hermes_database(path) else {
        return metadata;
    };
    let has_sessions: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !has_sessions {
        return metadata;
    }
    let columns = table_columns(&conn, "sessions").unwrap_or_default();
    let Some(id_column) = find_native_column(&columns, &["session_id", "sessionId", "id"]) else {
        return metadata;
    };
    let title_column = find_native_column(&columns, &["title"]);
    let project_column = find_native_column(&columns, &["cwd", "directory", "project_dir"]);
    let title_expr = title_column
        .map(quote_sql_identifier)
        .unwrap_or_else(|| "NULL".to_string());
    let project_expr = project_column
        .map(quote_sql_identifier)
        .unwrap_or_else(|| "NULL".to_string());
    let sql = format!(
        "SELECT {}, {}, {} FROM sessions",
        quote_sql_identifier(id_column),
        title_expr,
        project_expr
    );
    let Ok(mut statement) = conn.prepare(&sql) else {
        return metadata;
    };
    let Ok(mut rows) = statement.query([]) else {
        return metadata;
    };
    while let Ok(Some(row)) = rows.next() {
        let Ok(Some(session_id)) = optional_text(row, 0) else {
            continue;
        };
        let session_id = session_id.trim();
        if session_id.is_empty() {
            continue;
        }
        let title = optional_text(row, 1)
            .ok()
            .flatten()
            .and_then(normalize_native_text);
        let project_dir = optional_text(row, 2)
            .ok()
            .flatten()
            .and_then(normalize_native_text);
        if title.is_some() || project_dir.is_some() {
            metadata.insert(
                session_id.to_string(),
                HermesNativeMetadata { title, project_dir },
            );
        }
    }
    metadata
}

fn read_hermes_session_file_metadata(path: &Path) -> Option<(String, HermesNativeMetadata)> {
    let file = fs::File::open(path).ok()?;
    let mut session_id = None;
    let mut native = HermesNativeMetadata::default();
    for line in BufReader::new(file).lines().take(64).flatten() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let line_type = value.get("type").and_then(serde_json::Value::as_str);
        if !matches!(line_type, Some("session") | Some("init")) {
            continue;
        }
        if session_id.is_none() {
            session_id = value
                .get("id")
                .or_else(|| value.get("sessionId"))
                .and_then(serde_json::Value::as_str)
                .and_then(|value| normalize_native_text(value.to_string()));
        }
        if native.title.is_none() {
            native.title = value
                .get("title")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| normalize_native_text(value.to_string()));
        }
        if native.project_dir.is_none() {
            native.project_dir = value
                .get("cwd")
                .or_else(|| value.get("directory"))
                .and_then(serde_json::Value::as_str)
                .and_then(|value| normalize_native_text(value.to_string()));
        }
        if session_id.is_some() && native.title.is_some() && native.project_dir.is_some() {
            break;
        }
    }
    session_id.map(|id| (id, native))
}

fn find_native_column<'a>(columns: &'a [String], candidates: &[&str]) -> Option<&'a str> {
    candidates.iter().find_map(|candidate| {
        columns
            .iter()
            .find(|column| column.eq_ignore_ascii_case(candidate))
            .map(String::as_str)
    })
}

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn normalize_native_text(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

type HermesImportResult = (
    u32,
    u32,
    u32,
    u32,
    Vec<HermesUsageDelta>,
    Vec<HermesSessionIdentity>,
);

fn import_hermes_database(
    db: &Database,
    source: &HermesDatabaseSource,
    observed_at: i64,
) -> Result<HermesImportResult, AppError> {
    let database_identity = source_file_identity(&source.path)?;
    // Keep a readable namespace prefix while retaining the replacement-safe
    // database incarnation. The full key also stores each component
    // separately, so this is not a bare path, filename, or session ID.
    let source_identity = format!(
        "{}:{}:{}:{}",
        HERMES_APP_TYPE, HERMES_SOURCE_VERSION, source.profile_id, database_identity
    );

    // The source connection is read-only and dropped before the CC Switch
    // transaction starts. This also makes source WAL checkpoints impossible.
    let rows = read_hermes_database(&source.path)?;
    let native_metadata = load_hermes_native_metadata(source);
    let conn = crate::database::lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("启动 Hermes snapshot 事务失败: {error}")))?;
    let mut baselined = 0u32;
    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut deltas = Vec::new();
    let sessions = rows
        .iter()
        .map(|row| HermesSessionIdentity {
            profile_id: source.profile_id.clone(),
            database_identity: database_identity.clone(),
            session_id: row.session_id.clone(),
            canonical_session_id: canonical_session_id(
                &source.profile_id,
                &database_identity,
                &row.session_id,
            ),
            model: row.model.clone(),
            provider_id: row.provider_id.clone(),
            base_url_digest: row.base_url_digest.clone(),
            billing_mode: row.billing_mode.clone(),
            task: row.task.clone(),
            title: native_metadata
                .get(&row.session_id)
                .and_then(|metadata| metadata.title.clone()),
            project_dir: native_metadata
                .get(&row.session_id)
                .and_then(|metadata| metadata.project_dir.clone()),
            first_seen_ms: row.first_seen_ms,
            last_seen_ms: row.last_seen_ms,
        })
        .collect::<Vec<_>>();

    for row in &rows {
        let key = snapshot_key(
            &source_identity,
            &source.profile_id,
            &database_identity,
            row,
        );
        let previous = Database::get_agent_session_usage_snapshot_on_conn(&tx, &key)?
            .map(snapshot_to_counters)
            .transpose()?;
        let current = SnapshotCounters {
            api_call_count: row.api_call_count,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_write_tokens: row.cache_write_tokens,
            reasoning_tokens: row.reasoning_tokens,
            selected_cost_usd: row.selected_cost_usd,
            cost_baseline_usd: None,
            emitted_cost_balance_usd: Decimal::ZERO,
            last_synced_at: observed_at,
        };

        let (next, delta) = match previous {
            None => {
                baselined = baselined.saturating_add(1);
                (
                    SnapshotCounters {
                        cost_baseline_usd: current.selected_cost_usd,
                        ..current.clone()
                    },
                    None,
                )
            }
            Some(previous) if counters_regressed(&previous, &current) => {
                // Source reset/corruption: discard the previous sequence and
                // start a fresh baseline. Never emit negative counters.
                baselined = baselined.saturating_add(1);
                (
                    SnapshotCounters {
                        cost_baseline_usd: current.selected_cost_usd,
                        ..current.clone()
                    },
                    None,
                )
            }
            Some(previous) => {
                let delta = HermesSnapshotDelta::between(&previous, &current)?;
                let next = SnapshotCounters {
                    cost_baseline_usd: delta.cost_baseline_usd,
                    emitted_cost_balance_usd: delta.emitted_cost_balance_usd,
                    ..current.clone()
                };
                (next, Some((previous, delta)))
            }
        };

        if let Some((previous, delta)) = delta {
            if delta.has_usage() {
                // A non-baseline Hermes delta is one logical write: the
                // full-fidelity usage fact and its replacement cumulative
                // snapshot must commit or roll back together. In particular,
                // do not advance the snapshot before validating/persisting
                // the fact, otherwise a failed fact would lose the window.
                let fact = build_fact(
                    &source_identity,
                    &source.profile_id,
                    &database_identity,
                    row,
                    &delta,
                    previous.last_synced_at,
                    observed_at,
                );
                let snapshot = normalized_snapshot(&key, row, &next, observed_at);
                write_agent_session_usage_hermes_delta_on_conn(&tx, &fact, &snapshot)?;
                imported = imported.saturating_add(1);
                deltas.push(build_delta(
                    &source_identity,
                    &source.profile_id,
                    &database_identity,
                    row,
                    &delta,
                    previous.last_synced_at,
                    observed_at,
                ));
            } else {
                // No durable usage fact is needed for a metadata-only/no-op
                // window, but the latest cumulative source state still must
                // replace the baseline so a later increment is measured from
                // this observation.
                upsert_snapshot(&tx, &key, row, &next, observed_at)?;
                skipped = skipped.saturating_add(1);
            }
        } else {
            // First observation, counter reset, or database replacement:
            // baseline only. Historical cumulative totals never become a
            // user-facing usage fact.
            upsert_snapshot(&tx, &key, row, &next, observed_at)?;
        }
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 Hermes snapshot 事务失败: {error}")))?;
    Ok((
        rows.len() as u32,
        baselined,
        imported,
        skipped,
        deltas,
        sessions,
    ))
}

fn snapshot_key(
    source_identity: &str,
    profile_id: &str,
    database_identity: &str,
    row: &HermesSourceRow,
) -> AgentSessionUsageSnapshotKey {
    AgentSessionUsageSnapshotKey {
        app_type: HERMES_APP_TYPE.to_string(),
        source_identity: source_identity.to_string(),
        profile_id: profile_id.to_string(),
        database_identity: database_identity.to_string(),
        session_id: row.session_id.clone(),
        model: row.model.clone(),
        provider_id: row.provider_id.clone(),
        base_url_digest: row.base_url_digest.clone(),
        billing_mode: row.billing_mode.clone(),
        task: row.task.clone(),
        data_source: HERMES_DATA_SOURCE.to_string(),
        source_version: HERMES_SOURCE_VERSION.to_string(),
    }
}

fn snapshot_to_counters(snapshot: AgentSessionUsageSnapshot) -> Result<SnapshotCounters, AppError> {
    Ok(SnapshotCounters {
        api_call_count: non_negative(snapshot.api_call_count, "api_call_count")?,
        input_tokens: non_negative(snapshot.input_tokens, "input_tokens")?,
        output_tokens: non_negative(snapshot.output_tokens, "output_tokens")?,
        cache_read_tokens: non_negative(snapshot.cache_read_tokens, "cache_read_tokens")?,
        cache_write_tokens: non_negative(snapshot.cache_write_tokens, "cache_write_tokens")?,
        reasoning_tokens: non_negative(snapshot.reasoning_tokens, "reasoning_tokens")?,
        selected_cost_usd: select_snapshot_cost(&snapshot)?,
        cost_baseline_usd: parse_cost_state(snapshot.correction_state.as_deref())?.baseline,
        emitted_cost_balance_usd: parse_cost_state(snapshot.correction_state.as_deref())?.emitted,
        last_synced_at: snapshot.last_synced_at,
    })
}

fn select_snapshot_cost(snapshot: &AgentSessionUsageSnapshot) -> Result<Option<Decimal>, AppError> {
    let estimated = snapshot
        .estimated_cost_usd
        .as_deref()
        .map(|value| parse_non_negative_decimal(value, "estimated_cost_usd"))
        .transpose()?;
    let actual = snapshot
        .actual_cost_usd
        .as_deref()
        .map(|value| parse_non_negative_decimal(value, "actual_cost_usd"))
        .transpose()?;
    let status = snapshot.cost_status.as_deref();
    Ok(select_cost(estimated, actual, status).0)
}

fn upsert_snapshot(
    conn: &Connection,
    key: &AgentSessionUsageSnapshotKey,
    row: &HermesSourceRow,
    counters: &SnapshotCounters,
    observed_at: i64,
) -> Result<(), AppError> {
    let cost_state = format_cost_state(
        counters.cost_baseline_usd,
        counters.emitted_cost_balance_usd,
    );
    let snapshot = AgentSessionUsageSnapshot {
        app_type: key.app_type.clone(),
        source_identity: key.source_identity.clone(),
        profile_id: key.profile_id.clone(),
        database_identity: key.database_identity.clone(),
        session_id: key.session_id.clone(),
        model: key.model.clone(),
        provider_id: key.provider_id.clone(),
        base_url_digest: key.base_url_digest.clone(),
        billing_mode: key.billing_mode.clone(),
        task: key.task.clone(),
        data_source: key.data_source.clone(),
        source_version: key.source_version.clone(),
        api_call_count: counters.api_call_count,
        input_tokens: counters.input_tokens,
        output_tokens: counters.output_tokens,
        cache_read_tokens: counters.cache_read_tokens,
        cache_write_tokens: counters.cache_write_tokens,
        reasoning_tokens: counters.reasoning_tokens,
        first_seen: row.first_seen_ms,
        last_seen: row.last_seen_ms,
        last_synced_at: observed_at,
        estimated_cost_usd: row
            .estimated_cost_usd
            .as_ref()
            .map(|value| value.to_string()),
        actual_cost_usd: row.actual_cost_usd.as_ref().map(|value| value.to_string()),
        cost_status: row.cost_status.clone(),
        cost_source: row.cost_source.clone(),
        correction_state: Some(cost_state),
    };
    Database::upsert_agent_session_usage_snapshot_on_conn(conn, &snapshot)
}

fn normalized_snapshot(
    key: &AgentSessionUsageSnapshotKey,
    row: &HermesSourceRow,
    counters: &SnapshotCounters,
    observed_at: i64,
) -> NormalizedUsageSnapshot {
    NormalizedUsageSnapshot {
        app_type: key.app_type.clone(),
        source_identity: key.source_identity.clone(),
        profile_id: key.profile_id.clone(),
        database_identity: key.database_identity.clone(),
        session_id: key.session_id.clone(),
        model: key.model.clone(),
        provider_id: key.provider_id.clone(),
        base_url_digest: key.base_url_digest.clone(),
        billing_mode: key.billing_mode.clone(),
        task: key.task.clone(),
        data_source: key.data_source.clone(),
        source_version: key.source_version.clone(),
        api_call_count: counters.api_call_count,
        input_tokens: counters.input_tokens,
        output_tokens: counters.output_tokens,
        cache_read_tokens: counters.cache_read_tokens,
        cache_write_tokens: counters.cache_write_tokens,
        reasoning_tokens: counters.reasoning_tokens,
        first_seen: row.first_seen_ms,
        last_seen: row.last_seen_ms,
        last_synced_at: observed_at,
        estimated_cost_usd: row
            .estimated_cost_usd
            .as_ref()
            .map(|value| value.to_string()),
        actual_cost_usd: row.actual_cost_usd.as_ref().map(|value| value.to_string()),
        cost_status: row.cost_status.clone(),
        cost_source: row.cost_source.clone(),
        correction_state: Some(format_cost_state(
            counters.cost_baseline_usd,
            counters.emitted_cost_balance_usd,
        )),
    }
}

fn build_fact(
    source_identity: &str,
    profile_id: &str,
    database_identity: &str,
    row: &HermesSourceRow,
    delta: &HermesSnapshotDelta,
    sync_window_start: i64,
    sync_window_end: i64,
) -> NormalizedUsageRollupFact {
    let canonical_session_id = canonical_session_id(profile_id, database_identity, &row.session_id);
    NormalizedUsageRollupFact {
        date: sync_window_date(sync_window_end),
        app_type: HERMES_APP_TYPE.to_string(),
        session_id: canonical_session_id,
        provider_id: row.provider_id.clone(),
        model: row.model.clone(),
        request_model: row.model.clone(),
        pricing_model: row.model.clone(),
        data_source: HERMES_DATA_SOURCE.to_string(),
        precision: UsagePrecision::SyncWindowDelta,
        time_semantics: TimeSemantics::SyncWindowEnd,
        request_count_semantics: RequestCountSemantics::Unavailable,
        input_token_semantics: HERMES_INPUT_TOKEN_SEMANTICS,
        source_identity: source_identity.to_string(),
        profile_id: profile_id.to_string(),
        database_identity: database_identity.to_string(),
        base_url_digest: row.base_url_digest.clone(),
        billing_mode: row.billing_mode.clone(),
        task: row.task.clone(),
        source_version: HERMES_SOURCE_VERSION.to_string(),
        sync_window_start,
        sync_window_end,
        request_count: None,
        api_call_count: Some(delta.api_call_count),
        input_tokens: Some(delta.input_tokens),
        output_tokens: Some(delta.output_tokens),
        cache_read_tokens: Some(delta.cache_read_tokens),
        // Hermes exposes cache writes as a separate component. It does not
        // prove a generic cache-creation bucket, so this stays NULL.
        cache_creation_tokens: None,
        cache_write_tokens: Some(delta.cache_write_tokens),
        reasoning_tokens: Some(delta.reasoning_tokens),
        total_cost_usd: delta.cost_usd.as_ref().map(|value| value.to_string()),
        cost_status: row.cost_status.clone(),
        cost_source: row.cost_source.clone(),
        cost_delta_kind: cost_delta_kind_text(delta.cost_delta_kind),
        correction_state: delta
            .cost_usd
            .as_ref()
            .map(|_| format_cost_state(delta.cost_baseline_usd, delta.emitted_cost_balance_usd)),
        first_event_at: unix_seconds_from_timestamp_ms(row.first_seen_ms),
        last_event_at: unix_seconds_from_timestamp_ms(row.last_seen_ms),
    }
}

fn cost_delta_kind_text(kind: CostDeltaKind) -> Option<String> {
    match kind {
        CostDeltaKind::Increase => Some("increase".to_string()),
        CostDeltaKind::Reconciliation => Some("reconciliation".to_string()),
        CostDeltaKind::None => None,
    }
}

fn sync_window_date(timestamp: i64) -> String {
    let seconds = if timestamp.unsigned_abs() >= 100_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .map(|value| value.date_naive().to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn build_delta(
    source_identity: &str,
    profile_id: &str,
    database_identity: &str,
    row: &HermesSourceRow,
    delta: &HermesSnapshotDelta,
    sync_window_start: i64,
    sync_window_end: i64,
) -> HermesUsageDelta {
    let canonical_session_id = canonical_session_id(profile_id, database_identity, &row.session_id);
    HermesUsageDelta {
        source_identity: source_identity.to_string(),
        profile_id: profile_id.to_string(),
        database_identity: database_identity.to_string(),
        session_id: row.session_id.clone(),
        canonical_session_id,
        model: row.model.clone(),
        provider_id: row.provider_id.clone(),
        base_url_digest: row.base_url_digest.clone(),
        billing_mode: row.billing_mode.clone(),
        task: row.task.clone(),
        data_source: HERMES_DATA_SOURCE.to_string(),
        source_version: HERMES_SOURCE_VERSION.to_string(),
        precision: HERMES_PRECISION,
        time_semantics: HERMES_TIME_SEMANTICS,
        request_count_semantics: HERMES_REQUEST_COUNT_SEMANTICS,
        input_token_semantics: HERMES_INPUT_TOKEN_SEMANTICS,
        sync_window_start,
        sync_window_end,
        api_call_count: delta.api_call_count,
        input_tokens: delta.input_tokens,
        output_tokens: delta.output_tokens,
        cache_read_tokens: delta.cache_read_tokens,
        cache_write_tokens: delta.cache_write_tokens,
        reasoning_tokens: delta.reasoning_tokens,
        cost_usd: delta.cost_usd,
        cost_kind: row.selected_cost_kind,
        cost_delta_kind: delta.cost_delta_kind,
        cost_status: row.cost_status.clone(),
        cost_source: row.cost_source.clone(),
        first_seen_ms: row.first_seen_ms,
        last_seen_ms: row.last_seen_ms,
    }
}

/// Build the canonical Hermes usage/node ID shared by ingestion and Session
/// Manager scanning. `session_id` remains the raw Hermes identity; only its
/// digest is placed in the canonical key so path/profile identities cannot
/// collide or leak into a user-facing ID.
pub(crate) fn canonical_session_id(
    profile_id: &str,
    database_identity: &str,
    session_id: &str,
) -> String {
    format!(
        "hermes:{}:{}:{}",
        profile_id,
        database_identity,
        digest_parts(&[session_id])
    )
}

fn counters_regressed(previous: &SnapshotCounters, current: &SnapshotCounters) -> bool {
    current.api_call_count < previous.api_call_count
        || current.input_tokens < previous.input_tokens
        || current.output_tokens < previous.output_tokens
        || current.cache_read_tokens < previous.cache_read_tokens
        || current.cache_write_tokens < previous.cache_write_tokens
        || current.reasoning_tokens < previous.reasoning_tokens
}

impl HermesSnapshotDelta {
    fn between(previous: &SnapshotCounters, current: &SnapshotCounters) -> Result<Self, AppError> {
        let next_cost_baseline = match (
            previous.cost_baseline_usd.as_ref(),
            current.selected_cost_usd.as_ref(),
        ) {
            (None, Some(cost)) => Some(*cost),
            (Some(baseline), _) => Some(*baseline),
            (None, None) => None,
        };
        let target_balance = match (
            previous.cost_baseline_usd.as_ref(),
            current.selected_cost_usd.as_ref(),
        ) {
            // If the first observation had no cost, the next known cumulative
            // value becomes a fresh baseline.  This prevents importing an
            // unknowable historical balance in a later sync.
            (None, Some(_)) => None,
            (Some(baseline), Some(cost)) => {
                let amount = *cost - *baseline;
                Some(if amount > Decimal::ZERO {
                    amount
                } else {
                    Decimal::ZERO
                })
            }
            (_, None) => None,
        };
        let (cost_usd, emitted_cost_balance_usd, cost_delta_kind) = match target_balance {
            Some(target) => {
                let delta = target - previous.emitted_cost_balance_usd;
                let kind = if delta < Decimal::ZERO {
                    CostDeltaKind::Reconciliation
                } else if delta > Decimal::ZERO {
                    CostDeltaKind::Increase
                } else {
                    CostDeltaKind::None
                };
                (Some(delta), target, kind)
            }
            None => (None, previous.emitted_cost_balance_usd, CostDeltaKind::None),
        };
        Ok(Self {
            api_call_count: checked_delta(
                current.api_call_count,
                previous.api_call_count,
                "api_call_count",
            )?,
            input_tokens: checked_delta(
                current.input_tokens,
                previous.input_tokens,
                "input_tokens",
            )?,
            output_tokens: checked_delta(
                current.output_tokens,
                previous.output_tokens,
                "output_tokens",
            )?,
            cache_read_tokens: checked_delta(
                current.cache_read_tokens,
                previous.cache_read_tokens,
                "cache_read_tokens",
            )?,
            cache_write_tokens: checked_delta(
                current.cache_write_tokens,
                previous.cache_write_tokens,
                "cache_write_tokens",
            )?,
            reasoning_tokens: checked_delta(
                current.reasoning_tokens,
                previous.reasoning_tokens,
                "reasoning_tokens",
            )?,
            cost_usd,
            cost_baseline_usd: next_cost_baseline,
            emitted_cost_balance_usd,
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
            || self.cost_usd.is_some_and(|cost| cost != Decimal::ZERO)
    }
}

/// Open a Hermes database in read-only mode and assert query-only at runtime.
/// No immutable mode is used so committed WAL frames remain visible.
fn open_hermes_database(path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| AppError::Database(format!("打开 Hermes SQLite 失败: {error}")))?;
    conn.execute_batch("PRAGMA query_only = ON;")
        .map_err(|error| AppError::Database(format!("设置 Hermes query_only 失败: {error}")))?;
    let query_only: i64 = conn
        .query_row("PRAGMA query_only", [], |row| row.get(0))
        .map_err(|error| AppError::Database(format!("验证 Hermes query_only 失败: {error}")))?;
    if query_only != 1 {
        return Err(AppError::Database(
            "Hermes SQLite 未启用 query_only=ON".into(),
        ));
    }
    Ok(conn)
}

fn read_hermes_database(path: &Path) -> Result<Vec<HermesSourceRow>, AppError> {
    let conn = open_hermes_database(path)?;
    let columns = table_columns(&conn, "session_model_usage")?;
    const REQUIRED: &[&str] = &[
        "session_id",
        "model",
        "billing_provider",
        "billing_base_url",
        "billing_mode",
        "task",
        "api_call_count",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
        "estimated_cost_usd",
        "actual_cost_usd",
        "cost_status",
        "cost_source",
        "first_seen",
        "last_seen",
    ];
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|name| !columns.iter().any(|column| column == name))
        .collect();
    if !missing.is_empty() {
        return Err(AppError::Database(format!(
            "Hermes session_model_usage 缺少列: {}",
            missing.join(", ")
        )));
    }
    let mut statement = conn.prepare(
        "SELECT session_id, model, billing_provider, billing_base_url, billing_mode, task,
                api_call_count, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, reasoning_tokens, estimated_cost_usd, actual_cost_usd,
                cost_status, cost_source, first_seen, last_seen
         FROM session_model_usage
         ORDER BY session_id, model, billing_provider, billing_base_url, billing_mode, task",
    )?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(parse_source_row(row)?);
    }
    Ok(result)
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(AppError::Database(format!("Hermes 缺少 {table} 表")));
    }
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns)
}

fn parse_source_row(row: &Row<'_>) -> Result<HermesSourceRow, AppError> {
    let session_id = required_text(row, 0, "session_id")?;
    let model = dimension_text(row, 1)?;
    let provider_id = dimension_text(row, 2)?;
    let base_url = optional_text(row, 3)?;
    let billing_mode = dimension_text(row, 4)?;
    let task = dimension_text(row, 5)?;
    let estimated_cost_usd = optional_decimal(row, 12, "estimated_cost_usd")?;
    let actual_cost_usd = optional_decimal(row, 13, "actual_cost_usd")?;
    let cost_status = optional_text(row, 14)?;
    let cost_source = optional_text(row, 15)?;
    let (selected_cost_usd, selected_cost_kind) =
        select_cost(estimated_cost_usd, actual_cost_usd, cost_status.as_deref());
    // Keep NULL (unknown) distinct from an explicitly empty endpoint. Both
    // are stable digests but must not collapse two billing identities.
    let base_url_digest = digest_parts(&[base_url.as_deref().unwrap_or("<null>")]);
    Ok(HermesSourceRow {
        session_id,
        model,
        provider_id,
        base_url_digest,
        billing_mode,
        task,
        api_call_count: counter(row, 6, "api_call_count")?,
        input_tokens: counter(row, 7, "input_tokens")?,
        output_tokens: counter(row, 8, "output_tokens")?,
        cache_read_tokens: counter(row, 9, "cache_read_tokens")?,
        cache_write_tokens: counter(row, 10, "cache_write_tokens")?,
        reasoning_tokens: counter(row, 11, "reasoning_tokens")?,
        estimated_cost_usd,
        actual_cost_usd,
        cost_status,
        cost_source,
        first_seen_ms: optional_timestamp_ms(row, 16)?,
        last_seen_ms: optional_timestamp_ms(row, 17)?,
        selected_cost_usd,
        selected_cost_kind,
    })
}

fn required_text(row: &Row<'_>, index: usize, name: &str) -> Result<String, AppError> {
    optional_text(row, index)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Database(format!("Hermes {name} 为空或 NULL")))
}

fn dimension_text(row: &Row<'_>, index: usize) -> Result<String, AppError> {
    Ok(optional_text(row, index)?.unwrap_or_default())
}

fn optional_text(row: &Row<'_>, index: usize) -> Result<Option<String>, AppError> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        ValueRef::Text(value) => String::from_utf8(value.to_vec())
            .map(Some)
            .map_err(|error| AppError::Database(format!("Hermes 文本不是 UTF-8: {error}"))),
        ValueRef::Integer(value) => Ok(Some(value.to_string())),
        ValueRef::Real(value) => Ok(Some(value.to_string())),
        ValueRef::Blob(value) => Ok(Some(digest_bytes(value))),
    }
}

fn counter(row: &Row<'_>, index: usize, name: &str) -> Result<i64, AppError> {
    let value = match row.get_ref(index)? {
        ValueRef::Integer(value) => value,
        ValueRef::Real(value) if value.is_finite() && value.fract() == 0.0 => {
            if value < 0.0 || value > i64::MAX as f64 {
                return Err(AppError::Database(format!("Hermes {name} 超出整数范围")));
            }
            value as i64
        }
        ValueRef::Text(value) => std::str::from_utf8(value)
            .map_err(|error| AppError::Database(format!("Hermes {name} 不是 UTF-8: {error}")))?
            .trim()
            .parse::<i64>()
            .map_err(|error| AppError::Database(format!("Hermes {name} 不是整数: {error}")))?,
        _ => return Err(AppError::Database(format!("Hermes {name} 必须是非负整数"))),
    };
    non_negative(value, name)
}

fn optional_decimal(row: &Row<'_>, index: usize, name: &str) -> Result<Option<Decimal>, AppError> {
    optional_text(row, index)?
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_non_negative_decimal(&value, name))
        .transpose()
}

/// Parse a Hermes source timestamp into milliseconds. The source accepts
/// either Unix seconds/milliseconds or RFC3339 text; callers must keep the
/// unit suffix explicit when routing it to node metadata versus facts.
fn optional_timestamp_ms(row: &Row<'_>, index: usize) -> Result<Option<i64>, AppError> {
    let value = optional_text(row, index)?;
    Ok(value.and_then(|value| parse_timestamp_ms(&value)))
}

/// Canonical usage queries compare event boundaries as inclusive Unix
/// seconds. Hermes node display metadata intentionally remains milliseconds,
/// so this conversion is only used by `NormalizedUsageRollupFact` writes.
fn unix_seconds_from_timestamp_ms(timestamp_ms: Option<i64>) -> Option<i64> {
    timestamp_ms.map(|value| value.div_euclid(1_000))
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    if let Ok(number) = value.trim().parse::<i64>() {
        return Some(if number.unsigned_abs() < 10_000_000_000 {
            number.saturating_mul(1000)
        } else {
            number
        });
    }
    if let Ok(number) = value.trim().parse::<f64>() {
        if number.is_finite() {
            let millis = if number.abs() < 10_000_000_000.0 {
                number * 1000.0
            } else {
                number
            };
            if millis >= i64::MIN as f64 && millis <= i64::MAX as f64 {
                return Some(millis.round() as i64);
            }
        }
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp_millis())
}

fn select_cost(
    estimated: Option<Decimal>,
    actual: Option<Decimal>,
    status: Option<&str>,
) -> (Option<Decimal>, CostKind) {
    let normalized_status = status
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let authoritative = normalized_status
        .as_deref()
        .is_some_and(|value| matches!(value, "actual" | "final" | "settled" | "complete"));
    let explicitly_unknown = normalized_status
        .as_deref()
        .is_some_and(|value| matches!(value, "unknown" | "unavailable" | "none" | "n/a"));
    let mut selected = if authoritative {
        actual
    } else if explicitly_unknown {
        None
    } else {
        estimated
    };
    // Hermes historically defaulted estimated_cost_usd to numeric zero. A
    // row without a cost status cannot prove that this default was an
    // explicit zero; preserve it as unknown instead. Known statuses such as
    // `estimated` or `included` retain an explicit zero.
    if !authoritative
        && normalized_status.is_none()
        && selected
            .as_ref()
            .is_some_and(|value| value == &Decimal::ZERO)
    {
        selected = None;
    }
    let kind = match (selected.as_ref(), authoritative) {
        (Some(cost), true) if cost == &Decimal::ZERO => CostKind::ExplicitZero,
        (Some(_), true) => CostKind::Actual,
        (Some(cost), false) if cost == &Decimal::ZERO => CostKind::ExplicitZero,
        (Some(_), false) => CostKind::Estimated,
        (None, _) => CostKind::Unknown,
    };
    (selected, kind)
}

fn parse_non_negative_decimal(value: &str, name: &str) -> Result<Decimal, AppError> {
    let parsed = Decimal::from_str(value.trim())
        .map_err(|error| AppError::Database(format!("Hermes {name} 不是有效成本: {error}")))?;
    if parsed < Decimal::ZERO {
        return Err(AppError::Database(format!("Hermes {name} 不能为负数")));
    }
    Ok(parsed)
}

fn non_negative(value: i64, name: &str) -> Result<i64, AppError> {
    if value < 0 {
        Err(AppError::Database(format!("Hermes {name} 不能为负数")))
    } else {
        Ok(value)
    }
}

fn checked_delta(current: i64, previous: i64, name: &str) -> Result<i64, AppError> {
    current
        .checked_sub(previous)
        .filter(|value| *value >= 0)
        .ok_or_else(|| AppError::Database(format!("Hermes {name} 计数回退")))
}

fn parse_cost_state(state: Option<&str>) -> Result<CostState, AppError> {
    let Some(state) = state else {
        return Ok(CostState::default());
    };
    let value: serde_json::Value = serde_json::from_str(state)
        .map_err(|error| AppError::Database(format!("Hermes cost state 无效: {error}")))?;
    let baseline = match value.get("baseline") {
        Some(serde_json::Value::String(value)) => {
            Some(parse_non_negative_decimal(value, "cost baseline")?)
        }
        Some(serde_json::Value::Null) | None => None,
        _ => return Err(AppError::Database("Hermes cost baseline 无效".into())),
    };
    let emitted = value
        .get("emitted")
        .and_then(serde_json::Value::as_str)
        .map(|value| Decimal::from_str(value.trim()))
        .transpose()
        .map_err(|error| AppError::Database(format!("Hermes emitted cost 无效: {error}")))?
        .unwrap_or(Decimal::ZERO);
    if emitted < Decimal::ZERO {
        return Err(AppError::Database("Hermes emitted cost 不能为负数".into()));
    }
    Ok(CostState { baseline, emitted })
}

fn format_cost_state(baseline: Option<Decimal>, emitted: Decimal) -> String {
    serde_json::json!({
        "baseline": baseline.map(|value| value.to_string()),
        "emitted": emitted.to_string(),
    })
    .to_string()
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

/// Return the replacement-safe identity used to namespace a Hermes database.
/// Session Manager must call this helper instead of reimplementing the
/// platform-specific inode/file-index digest logic.
pub(crate) fn source_file_identity(path: &Path) -> Result<String, AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    let identity = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            format!("unix:{}:{}", metadata.dev(), metadata.ino())
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
            };
            let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
            if ok == 0 {
                format!(
                    "windows-fallback:{}:{}",
                    metadata.len(),
                    metadata.creation_time()
                )
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use tempfile::tempdir;

    const SOURCE_SCHEMA: &str = r#"
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
            last_seen TEXT
        );"#;

    fn source_db(path: &Path, task: &str) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(SOURCE_SCHEMA).unwrap();
        insert_source_row(
            &conn,
            task,
            1,
            100,
            50,
            10,
            20,
            7,
            Some("1.0"),
            None,
            Some("estimated"),
        );
        conn
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_source_row(
        conn: &Connection,
        task: &str,
        calls: i64,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
        reasoning: i64,
        estimated: Option<&str>,
        actual: Option<&str>,
        status: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO session_model_usage
             (session_id,model,billing_provider,billing_base_url,billing_mode,task,
              api_call_count,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,
              reasoning_tokens,estimated_cost_usd,actual_cost_usd,cost_status,cost_source,first_seen,last_seen)
             VALUES ('session-1','model-a','provider-a','https://example.test/v1','chat',?1,
                     ?2,?3,?4,?5,?6,?7,?8,?9,?10,'hermes','2026-08-03T00:00:00Z','2026-08-03T00:01:00Z')",
            params![task,calls,input,output,cache_read,cache_write,reasoning,estimated,actual,status],
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
        estimated: Option<&str>,
        actual: Option<&str>,
        status: Option<&str>,
    ) {
        conn.execute(
            "UPDATE session_model_usage SET api_call_count=?1,input_tokens=?2,output_tokens=?3,
             cache_read_tokens=?4,cache_write_tokens=?5,reasoning_tokens=?6,estimated_cost_usd=?7,
             actual_cost_usd=?8,cost_status=?9",
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

    #[test]
    fn discovers_default_and_profiles_with_distinct_identity() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("profiles/work")).unwrap();
        let default = source_db(&root.path().join("state.db"), "default-task");
        let work = source_db(&root.path().join("profiles/work/state.db"), "work-task");
        let sources = discover_hermes_databases(root.path()).unwrap();
        assert_eq!(
            sources
                .iter()
                .map(|source| source.profile_id.as_str())
                .collect::<Vec<_>>(),
            vec!["default", "work"]
        );
        assert_ne!(
            source_file_identity(&root.path().join("state.db")).unwrap(),
            source_file_identity(&root.path().join("profiles/work/state.db")).unwrap()
        );
        drop((default, work));
    }

    #[test]
    fn native_session_title_and_directory_are_reused_without_message_fallback() {
        let root = tempdir().unwrap();
        let _source = source_db(&root.path().join("state.db"), "native-task");
        fs::create_dir_all(root.path().join("sessions")).unwrap();
        fs::write(
            root.path().join("sessions/session-1.jsonl"),
            concat!(
                "{\"type\":\"session\",\"id\":\"session-1\",\"title\":\"Native Hermes task\",\"cwd\":\"/workspace/hermes\"}\n",
                "{\"type\":\"message\",\"role\":\"user\",\"content\":\"Native Hermes task\"}\n",
            ),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        let result = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        let claims = hermes_standalone_session_claims(&result);
        let claim = claims.first().expect("Hermes session claim");
        assert_eq!(claim.metadata.title.as_deref(), Some("Native Hermes task"));
        assert_eq!(
            claim.metadata.project_dir.as_deref(),
            Some("/workspace/hermes")
        );
    }

    #[test]
    fn read_only_query_only_rejects_writes_and_missing_schema_is_error() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let _writer = source_db(&path, "task");
        let readonly = open_hermes_database(&path).unwrap();
        assert_eq!(
            readonly
                .query_row("PRAGMA query_only", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(readonly
            .execute("CREATE TABLE forbidden (id INTEGER)", [])
            .is_err());
        drop(readonly);
        let missing_path = root.path().join("missing.db");
        Connection::open(&missing_path)
            .unwrap()
            .execute_batch("CREATE TABLE other (x INTEGER)")
            .unwrap();
        assert!(read_hermes_database(&missing_path).is_err());
    }

    #[test]
    fn cost_selection_distinguishes_actual_estimated_zero_and_unknown() {
        let estimated = Decimal::from_str("2").unwrap();
        assert_eq!(
            select_cost(
                Some(estimated.clone()),
                Some(Decimal::from_str("3").unwrap()),
                Some("actual")
            ),
            (Some(Decimal::from_str("3").unwrap()), CostKind::Actual)
        );
        assert_eq!(
            select_cost(
                Some(estimated),
                Some(Decimal::from_str("0").unwrap()),
                Some("complete")
            ),
            (Some(Decimal::ZERO), CostKind::ExplicitZero)
        );
        assert_eq!(
            select_cost(
                Some(estimated),
                Some(Decimal::from_str("3").unwrap()),
                Some("estimated")
            ),
            (Some(estimated), CostKind::Estimated)
        );
        assert_eq!(
            select_cost(None, None, Some("actual")),
            (None, CostKind::Unknown)
        );
        assert_eq!(
            select_cost(Some(Decimal::ZERO), None, Some("unknown")),
            (None, CostKind::Unknown)
        );
        assert_eq!(
            select_cost(Some(Decimal::ZERO), None, None),
            (None, CostKind::Unknown)
        );
        assert_eq!(
            select_cost(Some(Decimal::ZERO), None, Some("estimated")),
            (Some(Decimal::ZERO), CostKind::ExplicitZero)
        );
        assert_eq!(parse_timestamp_ms("1700000000.25"), Some(1_700_000_000_250));
    }

    #[test]
    fn source_milliseconds_are_kept_for_nodes_but_facts_use_unix_seconds() {
        assert_eq!(parse_timestamp_ms("1700000000"), Some(1_700_000_000_000));
        assert_eq!(
            parse_timestamp_ms("2023-11-14T22:13:20.250Z"),
            Some(1_700_000_000_250)
        );
        let first_seen_ms = 1_700_000_000_250;
        let last_seen_ms = 1_700_000_060_999;
        let row = HermesSourceRow {
            session_id: "s".into(),
            model: "m".into(),
            provider_id: "p".into(),
            base_url_digest: "b".into(),
            billing_mode: "chat".into(),
            task: "task".into(),
            api_call_count: 1,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            actual_cost_usd: None,
            cost_status: None,
            cost_source: None,
            first_seen_ms: Some(first_seen_ms),
            last_seen_ms: Some(last_seen_ms),
            selected_cost_usd: None,
            selected_cost_kind: CostKind::Unknown,
        };
        let delta = HermesSnapshotDelta {
            api_call_count: 1,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            cost_usd: None,
            cost_baseline_usd: None,
            emitted_cost_balance_usd: Decimal::ZERO,
            cost_delta_kind: CostDeltaKind::None,
        };

        let fact = build_fact("source", "default", "database", &row, &delta, 1, 2);
        assert_eq!(fact.first_event_at, Some(1_700_000_000));
        assert_eq!(fact.last_event_at, Some(1_700_000_060));

        let result = HermesSyncResult {
            observed_at: 2,
            sessions: vec![HermesSessionIdentity {
                profile_id: "default".into(),
                database_identity: "database".into(),
                session_id: row.session_id,
                canonical_session_id: canonical_session_id("default", "database", "s"),
                model: row.model,
                provider_id: row.provider_id,
                base_url_digest: row.base_url_digest,
                billing_mode: row.billing_mode,
                task: row.task,
                title: Some("Hermes".into()),
                project_dir: Some("/workspace/hermes".into()),
                first_seen_ms: Some(first_seen_ms),
                last_seen_ms: Some(last_seen_ms),
            }],
            ..HermesSyncResult::default()
        };
        let claim = hermes_standalone_session_claims(&result)
            .pop()
            .expect("Hermes node claim");
        assert_eq!(claim.metadata.created_at, Some(first_seen_ms));
        assert_eq!(claim.metadata.last_active_at, Some(last_seen_ms));
    }

    #[test]
    fn delta_is_non_negative_and_cost_correction_is_bounded() {
        let previous = SnapshotCounters {
            api_call_count: 1,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 2,
            cache_write_tokens: 3,
            reasoning_tokens: 1,
            selected_cost_usd: Some(Decimal::from_str("3").unwrap()),
            cost_baseline_usd: Some(Decimal::from_str("1").unwrap()),
            emitted_cost_balance_usd: Decimal::from_str("2").unwrap(),
            last_synced_at: 100,
        };
        let current = SnapshotCounters {
            api_call_count: 2,
            input_tokens: 20,
            output_tokens: 9,
            cache_read_tokens: 4,
            cache_write_tokens: 5,
            reasoning_tokens: 2,
            selected_cost_usd: Some(Decimal::from_str("2").unwrap()),
            ..previous.clone()
        };
        let delta = HermesSnapshotDelta::between(&previous, &current).unwrap();
        assert_eq!(
            (
                delta.api_call_count,
                delta.input_tokens,
                delta.reasoning_tokens
            ),
            (1, 10, 1)
        );
        assert_eq!(delta.cost_usd, Some(Decimal::from_str("-1").unwrap()));
        assert_eq!(delta.cost_delta_kind, CostDeltaKind::Reconciliation);
        let regressed = SnapshotCounters {
            api_call_count: 0,
            ..current
        };
        assert!(counters_regressed(&previous, &regressed));
    }

    #[test]
    fn task_is_only_a_dimension_not_a_parent_relation() {
        let first = HermesSourceRow {
            session_id: "s".into(),
            model: "m".into(),
            provider_id: "p".into(),
            base_url_digest: "b".into(),
            billing_mode: "chat".into(),
            task: "task-a".into(),
            api_call_count: 1,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            actual_cost_usd: None,
            cost_status: None,
            cost_source: None,
            first_seen_ms: None,
            last_seen_ms: None,
            selected_cost_usd: None,
            selected_cost_kind: CostKind::Unknown,
        };
        let mut second = first.clone();
        second.task = "task-b".into();
        assert_ne!(first.task, second.task);
        assert_ne!(
            digest_parts(&[
                &first.session_id,
                &first.model,
                &first.provider_id,
                &first.base_url_digest,
                &first.billing_mode,
                &first.task
            ]),
            digest_parts(&[
                &second.session_id,
                &second.model,
                &second.provider_id,
                &second.base_url_digest,
                &second.billing_mode,
                &second.task
            ])
        );
    }

    #[test]
    fn negative_source_counter_is_rejected_without_delta() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task");
        update_source_row(
            &writer,
            -1,
            100,
            50,
            10,
            20,
            7,
            Some("1"),
            None,
            Some("estimated"),
        );
        assert!(read_hermes_database(&path).is_err());
    }

    #[test]
    fn first_sync_is_baseline_then_only_non_negative_window_delta_is_emitted() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task");
        let db = Database::memory().unwrap();

        let first = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        assert_eq!(
            (first.baselined, first.imported, first.deltas.len()),
            (1, 0, 0)
        );
        assert_eq!(first.sessions.len(), 1);
        assert!(first.sessions[0]
            .canonical_session_id
            .starts_with("hermes:default:"));
        let claims = hermes_standalone_session_claims(&first);
        assert_eq!(claims.len(), 1);
        assert!(matches!(
            claims[0].relation,
            crate::services::agent_session_usage::RelationClaim::Standalone
        ));

        update_source_row(
            &writer,
            4,
            400,
            80,
            15,
            25,
            12,
            Some("2.0"),
            None,
            Some("estimated"),
        );
        let second = sync_hermes_usage_at_root(&db, root.path(), 200).unwrap();
        assert_eq!(
            (second.baselined, second.imported, second.deltas.len()),
            (0, 1, 1)
        );
        let delta = &second.deltas[0];
        assert_eq!(
            (
                delta.api_call_count,
                delta.input_tokens,
                delta.output_tokens,
                delta.cache_read_tokens,
                delta.cache_write_tokens,
                delta.reasoning_tokens
            ),
            (3, 300, 30, 5, 5, 5)
        );
        assert_eq!(delta.cost_usd, Some(Decimal::from_str("1").unwrap()));
        assert_eq!((delta.sync_window_start, delta.sync_window_end), (100, 200));

        // The non-baseline write is one full-fidelity fact plus one
        // replacement snapshot.  Cache writes and reasoning remain separate
        // components; cache creation is explicitly unknown.
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let fact_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((fact_count, snapshot_count), (1, 1));
        let fact: (
            String,
            String,
            i64,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            String,
            String,
            String,
            i64,
            i64,
            String,
            String,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT session_id, task, input_token_semantics, request_count,
                        api_call_count, input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, cache_write_tokens, precision,
                        time_semantics, request_count_semantics, sync_window_start,
                        sync_window_end, source_identity, database_identity,
                        total_cost_usd
                 FROM agent_session_usage_rollups",
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
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                        row.get(15)?,
                        row.get(16)?,
                        row.get(17)?,
                    ))
                },
            )
            .unwrap();
        assert!(fact.0.starts_with("hermes:default:"));
        assert_eq!(fact.1, "task");
        assert_eq!(fact.2, HERMES_INPUT_TOKEN_SEMANTICS);
        assert_eq!(
            (fact.3, fact.4, fact.5, fact.6, fact.7, fact.8, fact.9),
            (None, Some(3), Some(300), Some(30), Some(5), None, Some(5))
        );
        let reasoning_tokens: Option<i64> = conn
            .query_row(
                "SELECT reasoning_tokens FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reasoning_tokens, Some(5));
        assert_eq!(
            (
                fact.10.as_str(),
                fact.11.as_str(),
                fact.12.as_str(),
                fact.13,
                fact.14,
            ),
            (
                HERMES_PRECISION,
                HERMES_TIME_SEMANTICS,
                HERMES_REQUEST_COUNT_SEMANTICS,
                100,
                200
            )
        );
        assert_eq!(
            fact.17
                .as_deref()
                .map(|value| Decimal::from_str(value).unwrap()),
            Some(Decimal::ONE)
        );
        drop(conn);

        let repeated = sync_hermes_usage_at_root(&db, root.path(), 300).unwrap();
        assert_eq!((repeated.imported, repeated.deltas.len()), (0, 0));
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let fact_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let snapshot_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            (fact_count, snapshot_count),
            (1, 1),
            "idempotent sync must replace, not append"
        );
    }

    #[test]
    fn default_and_profile_rows_keep_separate_identity_and_deltas() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("profiles/work")).unwrap();
        let default_writer = source_db(&root.path().join("state.db"), "same-task");
        let profile_writer = source_db(&root.path().join("profiles/work/state.db"), "same-task");
        let db = Database::memory().unwrap();
        let first = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        assert_eq!(first.baselined, 2);

        update_source_row(
            &profile_writer,
            3,
            200,
            70,
            15,
            20,
            10,
            Some("1.5"),
            None,
            Some("estimated"),
        );
        let second = sync_hermes_usage_at_root(&db, root.path(), 200).unwrap();
        assert_eq!(second.imported, 1);
        assert_eq!(second.deltas[0].profile_id, "work");
        assert_eq!(second.deltas[0].api_call_count, 2);
        assert!(second.deltas[0]
            .canonical_session_id
            .starts_with("hermes:work:"));
        drop((default_writer, profile_writer));
    }

    #[test]
    fn task_and_base_url_are_snapshot_dimensions_not_parent_relations() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task-a");
        writer
            .execute(
                "INSERT INTO session_model_usage
                 (session_id,model,billing_provider,billing_base_url,billing_mode,task,
                  api_call_count,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,
                  reasoning_tokens,estimated_cost_usd,actual_cost_usd,cost_status,cost_source,
                  first_seen,last_seen)
                 VALUES ('session-1','model-a','provider-a','https://example.test/v2','chat','task-b',
                         2,200,80,20,30,9,'2.0',NULL,'estimated','hermes',
                         '2026-08-03T00:00:00Z','2026-08-03T00:01:00Z')",
                [],
            )
            .unwrap();
        let db = Database::memory().unwrap();
        let first = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        assert_eq!((first.baselined, first.imported), (2, 0));
        assert_eq!(hermes_standalone_session_claims(&first).len(), 1);
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let snapshot_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let task_count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT task) FROM agent_session_usage_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let base_digest_count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT base_url_digest) FROM agent_session_usage_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((snapshot_count, task_count, base_digest_count), (2, 2, 2));
        drop(conn);
        drop(writer);
    }

    #[test]
    fn broken_profile_schema_isolated_from_valid_profile() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("profiles/broken")).unwrap();
        let valid_writer = source_db(&root.path().join("state.db"), "valid-task");
        let broken_path = root.path().join("profiles/broken/state.db");
        Connection::open(&broken_path)
            .unwrap()
            .execute_batch("CREATE TABLE session_model_usage (session_id TEXT)")
            .unwrap();
        let db = Database::memory().unwrap();
        let result = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        assert_eq!(result.baselined, 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("broken"));
        drop(valid_writer);
    }

    #[test]
    fn counter_reset_and_database_replacement_start_fresh_baselines() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task");
        let db = Database::memory().unwrap();
        sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();

        update_source_row(
            &writer,
            0,
            20,
            10,
            1,
            2,
            1,
            Some("0.1"),
            None,
            Some("estimated"),
        );
        let reset = sync_hermes_usage_at_root(&db, root.path(), 200).unwrap();
        assert_eq!(
            (reset.baselined, reset.imported, reset.deltas.len()),
            (1, 0, 0)
        );
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let facts_after_reset: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(facts_after_reset, 0, "counter reset is baseline-only");
        drop(conn);
        update_source_row(
            &writer,
            2,
            30,
            20,
            2,
            3,
            2,
            Some("0.2"),
            None,
            Some("estimated"),
        );
        let after_reset = sync_hermes_usage_at_root(&db, root.path(), 300).unwrap();
        assert_eq!((after_reset.imported, after_reset.deltas.len()), (1, 1));
        assert_eq!(after_reset.deltas[0].api_call_count, 2);

        drop(writer);
        fs::remove_file(&path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let replacement = source_db(&path, "task");
        let replaced = sync_hermes_usage_at_root(&db, root.path(), 400).unwrap();
        assert_eq!(
            (replaced.baselined, replaced.imported, replaced.deltas.len()),
            (1, 0, 0)
        );
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let facts_after_replacement: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(facts_after_replacement, 1, "replacement is baseline-only");
        drop(conn);
        drop(replacement);
    }

    #[test]
    fn actual_estimated_zero_and_missing_cost_never_add_two_costs() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task");
        let db = Database::memory().unwrap();
        sync_hermes_usage_at_root(&db, root.path(), 100).unwrap(); // $1 estimated baseline

        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            Some("3.0"),
            None,
            Some("estimated"),
        );
        let estimate = sync_hermes_usage_at_root(&db, root.path(), 200).unwrap();
        assert_eq!(
            estimate.deltas[0].cost_usd,
            Some(Decimal::from_str("2").unwrap())
        );

        // Actual $2 replaces the cumulative estimate. Only the previously
        // imported $2 is eligible for a -$2 correction; baseline $1 remains
        // excluded and no estimate+actual double count is possible.
        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            Some("3.0"),
            Some("0.0"),
            Some("actual"),
        );
        let actual_zero = sync_hermes_usage_at_root(&db, root.path(), 300).unwrap();
        assert_eq!(
            actual_zero.deltas[0].cost_usd,
            Some(Decimal::from_str("-2").unwrap())
        );
        assert_eq!(
            actual_zero.deltas[0].cost_delta_kind,
            CostDeltaKind::Reconciliation
        );

        // Missing cost is not explicit zero and must not generate a cost row.
        update_source_row(&writer, 3, 120, 60, 12, 22, 9, None, None, Some("actual"));
        let missing = sync_hermes_usage_at_root(&db, root.path(), 400).unwrap();
        assert_eq!(missing.deltas[0].cost_usd, None);

        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let mut statement = conn
            .prepare(
                "SELECT sync_window_end, total_cost_usd, cost_status,
                        cost_delta_kind, correction_state
                 FROM agent_session_usage_rollups
                 ORDER BY sync_window_end",
            )
            .unwrap();
        let facts = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(facts.len(), 3);
        assert_eq!(
            facts[0]
                .1
                .as_deref()
                .map(|value| Decimal::from_str(value).unwrap()),
            Some(Decimal::from_str("2").unwrap())
        );
        assert_eq!(facts[0].2, Some("estimated".to_string()));
        assert_eq!(
            facts[1]
                .1
                .as_deref()
                .map(|value| Decimal::from_str(value).unwrap()),
            Some(Decimal::from_str("-2").unwrap())
        );
        assert_eq!(facts[1].2, Some("actual".to_string()));
        assert_eq!(facts[1].3, Some("reconciliation".to_string()));
        assert!(facts[1].4.is_some());
        assert_eq!(facts[2].1, None);
        assert_eq!(facts[2].2, Some("actual".to_string()));
        drop(statement);
        drop(conn);
    }

    #[test]
    fn explicit_zero_cost_is_retained_on_a_known_token_delta() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task");
        update_source_row(
            &writer,
            1,
            100,
            50,
            10,
            20,
            7,
            Some("1.0"),
            Some("0"),
            Some("actual"),
        );
        let db = Database::memory().unwrap();
        let baseline = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        assert_eq!((baseline.baselined, baseline.imported), (1, 0));

        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            Some("2.0"),
            Some("0"),
            Some("actual"),
        );
        let delta = sync_hermes_usage_at_root(&db, root.path(), 200).unwrap();
        assert_eq!(delta.imported, 1);
        assert_eq!(delta.deltas[0].cost_usd, Some(Decimal::ZERO));
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let cost: Option<String> = conn
            .query_row(
                "SELECT total_cost_usd FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            cost.as_deref()
                .map(|value| Decimal::from_str(value).unwrap()),
            Some(Decimal::ZERO)
        );
        drop(conn);
        drop(writer);
    }

    #[test]
    fn unknown_cost_baseline_then_known_cost_does_not_backfill_history() {
        let root = tempdir().unwrap();
        let path = root.path().join("state.db");
        let writer = source_db(&path, "task");
        update_source_row(&writer, 1, 100, 50, 10, 20, 7, None, None, Some("unknown"));
        let db = Database::memory().unwrap();
        let baseline = sync_hermes_usage_at_root(&db, root.path(), 100).unwrap();
        assert_eq!((baseline.baselined, baseline.imported), (1, 0));

        update_source_row(
            &writer,
            2,
            110,
            55,
            11,
            21,
            8,
            Some("4.0"),
            None,
            Some("estimated"),
        );
        let delta = sync_hermes_usage_at_root(&db, root.path(), 200).unwrap();
        assert_eq!(delta.imported, 1);
        assert_eq!(delta.deltas[0].cost_usd, None);
        let conn = db.conn.lock().expect("lock anonymous Hermes database");
        let cost: Option<String> = conn
            .query_row(
                "SELECT total_cost_usd FROM agent_session_usage_rollups",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cost, None);
        drop(conn);
        drop(writer);
    }
}
