//! Explicit provider-scoped historical session-usage rebuilds.
//!
//! A rebuild is deliberately separate from the normal startup/sync path.  The
//! selected provider is first rebuilt in an isolated SQLite generation cloned
//! from the current live database.  Only a complete scan is published, and the
//! publish transaction replaces rows owned by that provider while leaving
//! proxy rows and every other provider untouched.

use crate::config::get_claude_config_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::opencode_config::get_opencode_db_path;
use crate::services::session_usage::SessionSyncResult;
use crate::services::session_usage_pipeline::adapter_for;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::NamedTempFile;

/// These tables form one provider-owned canonical generation. Staging is an
/// exact SQLite clone of live v18, so `SELECT *` preserves every current
/// dimension and automatically keeps a newly added canonical column atomic
/// with its owning provider rather than silently dropping it during publish.
const APP_SCOPED_CANONICAL_TABLES: &[&str] = &[
    "agent_session_nodes",
    "agent_session_usage_rollups",
    "agent_session_usage_snapshots",
    "agent_session_canonical_coverage",
];

/// The provider set intentionally matches the dashboard's supported rebuild
/// filters.  Serde names are part of the Tauri command contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentUsageRebuildApp {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "grokbuild")]
    GrokBuild,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "hermes")]
    Hermes,
    #[serde(rename = "pi")]
    Pi,
}

impl AgentUsageRebuildApp {
    fn app_type(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::GrokBuild => "grokbuild",
            Self::OpenCode => "opencode",
            Self::Hermes => "hermes",
            Self::Pi => "pi",
        }
    }

    fn direct_source(self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("session_log"),
            Self::Codex => Some("codex_session"),
            Self::GrokBuild => Some("grok_session"),
            Self::OpenCode => Some("opencode_session"),
            Self::Hermes => None,
            Self::Pi => Some("pi_session"),
        }
    }

    fn durable_dedup_source(self) -> Option<&'static str> {
        match self {
            Self::Pi => Some("pi_session"),
            Self::Claude | Self::Codex | Self::GrokBuild | Self::OpenCode | Self::Hermes => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildAgentSessionUsageRequest {
    pub app_types: Vec<AgentUsageRebuildApp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderUsageRebuildStatus {
    Published,
    KeptPrevious,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageRebuildResult {
    pub app_type: AgentUsageRebuildApp,
    pub status: ProviderUsageRebuildStatus,
    pub sync_result: SessionSyncResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildAgentSessionUsageResult {
    pub providers: Vec<ProviderUsageRebuildResult>,
}

#[derive(Debug, Clone)]
struct CursorRow {
    file_path: String,
    last_modified: i64,
    last_line_offset: i64,
    last_synced_at: i64,
}

/// Normalize, validate, and de-duplicate in request order.  Serde rejects an
/// unknown provider before entering this function; an empty selection is an
/// explicit command error and therefore cannot mutate the database.
fn normalized_apps(
    request: &RebuildAgentSessionUsageRequest,
) -> Result<Vec<AgentUsageRebuildApp>, AppError> {
    if request.app_types.is_empty() {
        return Err(AppError::InvalidInput(
            "至少选择一个 Agent 用量 provider".to_string(),
        ));
    }
    let mut seen = HashSet::new();
    let mut apps = Vec::with_capacity(request.app_types.len());
    for app in &request.app_types {
        if seen.insert(*app) {
            apps.push(*app);
        }
    }
    Ok(apps)
}

fn sync_error(message: impl Into<String>) -> SessionSyncResult {
    SessionSyncResult {
        errors: vec![message.into()],
        ..SessionSyncResult::default()
    }
}

fn kept_previous(
    app: AgentUsageRebuildApp,
    result: SessionSyncResult,
) -> ProviderUsageRebuildResult {
    ProviderUsageRebuildResult {
        app_type: app,
        status: ProviderUsageRebuildStatus::KeptPrevious,
        sync_result: result,
    }
}

fn published(app: AgentUsageRebuildApp, result: SessionSyncResult) -> ProviderUsageRebuildResult {
    ProviderUsageRebuildResult {
        app_type: app,
        status: ProviderUsageRebuildStatus::Published,
        sync_result: result,
    }
}

/// Read-only source preflight.  Missing/invalid sources are represented as a
/// provider-level `keptPrevious` result so one unavailable integration does not
/// block the other selected providers.
fn source_preflight(app: AgentUsageRebuildApp) -> Result<(), AppError> {
    let adapter = adapter_for(app.app_type())
        .ok_or_else(|| AppError::InvalidInput(format!("未注册 {} 的用量适配器", app.app_type())))?;
    adapter.preflight().unwrap_or_else(|| {
        Err(AppError::InvalidInput(format!(
            "{} 不支持 provider-scoped 用量重建",
            adapter.display_name()
        )))
    })
}

fn normalized_path(value: &str) -> String {
    value.replace('\\', "/").to_ascii_lowercase()
}

/// `session_log_sync` has no app_type column.  Capture the source roots once
/// at the start of a provider operation, then reuse the immutable matcher for
/// both stage reset and publish.  Besides preventing cross-provider cursor
/// deletion, this avoids resolving process-global test overrides midway
/// through an operation.
#[derive(Debug, Clone)]
enum CursorScope {
    PathPrefixes(Vec<String>),
    SourceKeys(Vec<String>),
    None,
}

impl CursorScope {
    fn capture(app: AgentUsageRebuildApp) -> Self {
        match app {
            AgentUsageRebuildApp::Claude => Self::PathPrefixes(vec![normalized_path(
                &get_claude_config_dir().join("projects").to_string_lossy(),
            )]),
            AgentUsageRebuildApp::Codex => {
                let codex_dir = crate::codex_config::get_codex_config_dir();
                Self::PathPrefixes(vec![
                    normalized_path(&codex_dir.join("sessions").to_string_lossy()),
                    normalized_path(&codex_dir.join("archived_sessions").to_string_lossy()),
                ])
            }
            AgentUsageRebuildApp::GrokBuild => Self::PathPrefixes(
                crate::session_manager::providers::grokbuild::session_roots()
                    .iter()
                    .map(|root| normalized_path(&root.to_string_lossy()))
                    .collect(),
            ),
            AgentUsageRebuildApp::OpenCode => {
                let db_path = get_opencode_db_path();
                let mut keys = vec![normalized_path(&db_path.to_string_lossy())];
                if let Some(parent) = db_path.parent() {
                    keys.push(normalized_path(&parent.join("storage").to_string_lossy()));
                }
                Self::SourceKeys(keys)
            }
            AgentUsageRebuildApp::Hermes => Self::None,
            AgentUsageRebuildApp::Pi => Self::PathPrefixes(
                crate::session_manager::providers::pi::session_roots()
                    .iter()
                    .map(|root| normalized_path(&root.to_string_lossy()))
                    .collect(),
            ),
        }
    }

    fn matches(&self, file_path: &str) -> bool {
        let path = normalized_path(file_path);
        match self {
            Self::PathPrefixes(roots) => roots
                .iter()
                .any(|root| path == *root || path.starts_with(&(root.clone() + "/"))),
            Self::SourceKeys(keys) => keys
                .iter()
                .any(|key| path == *key || path.starts_with(&(key.clone() + ":"))),
            Self::None => false,
        }
    }
}

fn read_cursor_rows(conn: &Connection, schema: &str) -> Result<Vec<CursorRow>, AppError> {
    let sql = format!(
        "SELECT file_path, last_modified, last_line_offset, last_synced_at FROM {schema}.session_log_sync"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| AppError::Database(format!("读取 provider cursor 失败: {error}")))?;
    let rows = statement
        .query_map([], |row| {
            Ok(CursorRow {
                file_path: row.get(0)?,
                last_modified: row.get(1)?,
                last_line_offset: row.get(2)?,
                last_synced_at: row.get(3)?,
            })
        })
        .map_err(|error| AppError::Database(format!("查询 provider cursor 失败: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(format!("解析 provider cursor 失败: {error}")))?;
    Ok(rows)
}

fn copy_live_to_staging(db: &Database) -> Result<tempfile::TempPath, AppError> {
    let temp_path = NamedTempFile::new()
        .map_err(|error| AppError::IoContext {
            context: "创建 provider 重建暂存文件失败".to_string(),
            source: error,
        })?
        .into_temp_path();
    let mut staging_conn = Connection::open(&temp_path)
        .map_err(|error| AppError::Database(format!("打开 provider 重建暂存库失败: {error}")))?;
    let live_conn = lock_conn!(db.conn);
    let backup = Backup::new(&live_conn, &mut staging_conn)
        .map_err(|error| AppError::Database(format!("复制 live 数据库到暂存库失败: {error}")))?;
    let step = backup
        .step(-1)
        .map_err(|error| AppError::Database(format!("复制 live 数据库到暂存库失败: {error}")))?;
    if !matches!(step, StepResult::Done) {
        return Err(AppError::Database(format!(
            "复制 live 数据库到暂存库未完成: {step:?}"
        )));
    }
    drop(backup);
    staging_conn
        .execute("PRAGMA foreign_keys = ON", [])
        .map_err(|error| AppError::Database(format!("启用暂存库外键失败: {error}")))?;
    drop(staging_conn);
    Ok(temp_path)
}

fn delete_app_scoped_canonical_rows(
    tx: &rusqlite::Transaction<'_>,
    app_type: &str,
) -> Result<(), AppError> {
    for table in APP_SCOPED_CANONICAL_TABLES {
        tx.execute(
            &format!("DELETE FROM {table} WHERE app_type = ?1"),
            [app_type],
        )?;
    }
    Ok(())
}

fn replace_app_scoped_canonical_rows_from_stage(
    tx: &rusqlite::Transaction<'_>,
    app_type: &str,
) -> Result<(), AppError> {
    delete_app_scoped_canonical_rows(tx, app_type)?;
    for table in APP_SCOPED_CANONICAL_TABLES {
        tx.execute(
            &format!("INSERT INTO {table} SELECT * FROM rebuild_stage.{table} WHERE app_type = ?1"),
            [app_type],
        )?;
    }
    Ok(())
}

fn reset_provider_stage(
    stage_db: &Database,
    app: AgentUsageRebuildApp,
    cursor_scope: &CursorScope,
) -> Result<(), AppError> {
    let conn = lock_conn!(stage_db.conn);
    let existing_cursors = {
        let mut statement = conn
            .prepare("SELECT file_path FROM session_log_sync")
            .map_err(|error| AppError::Database(format!("读取暂存 cursor 失败: {error}")))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| AppError::Database(format!("查询暂存 cursor 失败: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::Database(format!("解析暂存 cursor 失败: {error}")))?;
        rows
    };
    let tx = conn
        .unchecked_transaction()
        .map_err(|error| AppError::Database(format!("开启 provider 暂存清理事务失败: {error}")))?;
    delete_app_scoped_canonical_rows(&tx, app.app_type())?;
    if let Some(source) = app.direct_source() {
        tx.execute(
            "DELETE FROM proxy_request_logs
             WHERE app_type = ?1 AND data_source = ?2",
            rusqlite::params![app.app_type(), source],
        )?;
    }
    if app == AgentUsageRebuildApp::Codex {
        tx.execute(
            "DELETE FROM usage_daily_rollups
             WHERE provider_id = '_codex_session'",
            [],
        )?;
    }
    if let Some(source) = app.durable_dedup_source() {
        tx.execute(
            "DELETE FROM session_usage_dedup WHERE data_source = ?1",
            [source],
        )?;
    }
    for file_path in existing_cursors
        .into_iter()
        .filter(|path| cursor_scope.matches(path))
    {
        tx.execute(
            "DELETE FROM session_log_sync WHERE file_path = ?1",
            [file_path],
        )?;
    }
    tx.commit()
        .map_err(|error| AppError::Database(format!("提交 provider 暂存清理事务失败: {error}")))
}

fn sync_provider_in_stage(
    stage_db: &Database,
    app: AgentUsageRebuildApp,
) -> Result<SessionSyncResult, AppError> {
    let adapter = adapter_for(app.app_type())
        .ok_or_else(|| AppError::InvalidInput(format!("未注册 {} 的用量适配器", app.app_type())))?;
    adapter.sync_for_rebuild(stage_db).unwrap_or_else(|| {
        Err(AppError::InvalidInput(format!(
            "{} 不支持 provider-scoped 用量重建",
            adapter.display_name()
        )))
    })
}

fn provider_node_count(conn: &Connection, app: AgentUsageRebuildApp) -> Result<i64, AppError> {
    conn.query_row(
        "SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = ?1 AND TRIM(session_id) <> ''",
        [app.app_type()],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("读取 provider 暂存身份失败: {error}")))
}

fn copy_provider_rows_from_stage(
    tx: &rusqlite::Transaction<'_>,
    app: AgentUsageRebuildApp,
) -> Result<(), AppError> {
    let app_type = app.app_type();
    replace_app_scoped_canonical_rows_from_stage(tx, app_type)?;
    if app == AgentUsageRebuildApp::Codex {
        tx.execute(
            "DELETE FROM usage_daily_rollups
             WHERE provider_id = '_codex_session'",
            [],
        )?;
        tx.execute(
            "INSERT INTO usage_daily_rollups
             SELECT * FROM rebuild_stage.usage_daily_rollups
             WHERE provider_id = '_codex_session'",
            [],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value)
             VALUES ('codex_usage_canonical_replay_v1', 'complete')",
            [],
        )?;
    }
    if let Some(source) = app.direct_source() {
        tx.execute(
            "DELETE FROM proxy_request_logs
             WHERE app_type = ?1 AND data_source = ?2",
            rusqlite::params![app_type, source],
        )?;
        // Never replace a proxy row on request_id collision.  Direct source
        // rows are disposable rebuild material; proxy rows are canonical raw
        // evidence and must survive every provider rebuild.
        tx.execute(
            // Staging is a byte-for-byte SQLite clone, so SELECT * keeps any
            // future raw dimension and preserves the same ignore-on-collision
            // rule for ordinary proxy rows without a second column mirror.
            "INSERT OR IGNORE INTO proxy_request_logs
             SELECT * FROM rebuild_stage.proxy_request_logs
             WHERE app_type = ?1 AND data_source = ?2",
            rusqlite::params![app_type, source],
        )?;
    }
    if let Some(source) = app.durable_dedup_source() {
        tx.execute(
            "DELETE FROM session_usage_dedup WHERE data_source = ?1",
            [source],
        )?;
        tx.execute(
            "INSERT INTO session_usage_dedup (
                data_source, request_id, semantic_id, has_entry_id
             ) SELECT data_source, request_id, semantic_id, has_entry_id
             FROM rebuild_stage.session_usage_dedup
             WHERE data_source = ?1",
            [source],
        )?;
    }
    Ok(())
}

fn publish_provider_from_stage(
    db: &Database,
    stage_path: &Path,
    app: AgentUsageRebuildApp,
    cursor_scope: &CursorScope,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    conn.execute(
        "ATTACH DATABASE ?1 AS rebuild_stage",
        [stage_path.to_string_lossy().as_ref()],
    )
    .map_err(|error| AppError::Database(format!("附加 provider 暂存库失败: {error}")))?;

    let result = (|| {
        let live_cursors = read_cursor_rows(&conn, "main")?;
        let staged_cursors = read_cursor_rows(&conn, "rebuild_stage")?;
        let mut affected_paths = HashSet::new();
        for row in live_cursors.iter().chain(staged_cursors.iter()) {
            if cursor_scope.matches(&row.file_path) {
                affected_paths.insert(row.file_path.clone());
            }
        }

        let tx = conn
            .unchecked_transaction()
            .map_err(|error| AppError::Database(format!("开启 provider 发布事务失败: {error}")))?;
        copy_provider_rows_from_stage(&tx, app)?;

        for file_path in &affected_paths {
            tx.execute(
                "DELETE FROM session_log_sync WHERE file_path = ?1",
                [file_path],
            )?;
        }
        for row in staged_cursors
            .into_iter()
            .filter(|row| affected_paths.contains(&row.file_path))
        {
            tx.execute(
                "INSERT OR REPLACE INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    row.file_path,
                    row.last_modified,
                    row.last_line_offset,
                    row.last_synced_at
                ],
            )?;
        }
        tx.commit()
            .map_err(|error| AppError::Database(format!("提交 provider 发布事务失败: {error}")))
    })();

    let detach = conn.execute("DETACH DATABASE rebuild_stage", []);
    match (result, detach) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(AppError::Database(format!(
            "卸载 provider 暂存库失败: {error}"
        ))),
    }
}

fn rebuild_provider_in_stage(
    db: &Database,
    app: AgentUsageRebuildApp,
    cursor_scope: &CursorScope,
) -> Result<ProviderUsageRebuildResult, AppError> {
    let stage_path = copy_live_to_staging(db)?;
    let stage_conn = Connection::open(&stage_path)
        .map_err(|error| AppError::Database(format!("打开 provider 暂存库失败: {error}")))?;
    let stage_db = Database {
        conn: Mutex::new(stage_conn),
    };

    reset_provider_stage(&stage_db, app, cursor_scope)?;
    let sync_result = sync_provider_in_stage(&stage_db, app)?;
    if app == AgentUsageRebuildApp::Codex {
        // The staging database is disposable.  Running the normal retention
        // pass here lets Codex history be split into its raw and daily-owned
        // partitions before the Codex-owned rows are atomically published.
        stage_db.rollup_and_prune_codex_staging(30)?;
    }
    let identity_count = {
        let conn = lock_conn!(stage_db.conn);
        provider_node_count(&conn, app)?
    };
    // Close staging before ATTACH on Windows.  The temporary path remains
    // alive until this function returns.
    drop(stage_db);
    finalize_staged_provider(
        db,
        &stage_path,
        app,
        cursor_scope,
        sync_result,
        identity_count,
    )
}

fn finalize_staged_provider(
    db: &Database,
    stage_path: &Path,
    app: AgentUsageRebuildApp,
    cursor_scope: &CursorScope,
    sync_result: SessionSyncResult,
    identity_count: i64,
) -> Result<ProviderUsageRebuildResult, AppError> {
    let eligible =
        sync_result.errors.is_empty() && sync_result.deferred_files == 0 && identity_count > 0;
    if !eligible {
        let mut result = sync_result;
        if result.errors.is_empty() {
            result
                .errors
                .push("provider 扫描未产生有效 session identity，保留上一代数据".to_string());
        }
        return Ok(kept_previous(app, result));
    }

    publish_provider_from_stage(db, stage_path, app, cursor_scope)?;
    Ok(published(app, sync_result))
}

/// Rebuild selected providers in stable request order.  The caller (Tauri
/// command) owns `session_sync_mutex`; this synchronous core intentionally
/// never attempts to lock it again.
pub fn rebuild_agent_session_usage(
    db: &Database,
    request: &RebuildAgentSessionUsageRequest,
) -> Result<RebuildAgentSessionUsageResult, AppError> {
    rebuild_agent_session_usage_inner(db, request, true)
}

/// Compatibility callers can reuse the exact generic provider pipeline while
/// owning their historical notification semantics (the old Codex wrapper
/// emits after any reset attempt, including an incomplete replay).
pub(crate) fn rebuild_agent_session_usage_without_event(
    db: &Database,
    request: &RebuildAgentSessionUsageRequest,
) -> Result<RebuildAgentSessionUsageResult, AppError> {
    rebuild_agent_session_usage_inner(db, request, false)
}

fn rebuild_agent_session_usage_inner(
    db: &Database,
    request: &RebuildAgentSessionUsageRequest,
    emit_event: bool,
) -> Result<RebuildAgentSessionUsageResult, AppError> {
    let apps = normalized_apps(request)?;
    let mut preflight = HashMap::new();
    for app in &apps {
        preflight.insert(*app, source_preflight(*app));
    }

    // One safety backup covers the complete serialized operation.  Individual
    // provider failures below never replace the live generation.
    db.backup_database_file()?;

    let mut providers = Vec::with_capacity(apps.len());
    let mut any_published = false;
    for app in apps {
        if let Some(Err(error)) = preflight.remove(&app) {
            providers.push(kept_previous(
                app,
                sync_error(format!(
                    "{} source preflight failed: {error}",
                    app.app_type()
                )),
            ));
            continue;
        }

        let cursor_scope = CursorScope::capture(app);
        let provider_result = match rebuild_provider_in_stage(db, app, &cursor_scope) {
            Ok(result) => {
                if result.status == ProviderUsageRebuildStatus::Published {
                    any_published = true;
                }
                result
            }
            Err(error) => kept_previous(
                app,
                sync_error(format!("{} staged rebuild failed: {error}", app.app_type())),
            ),
        };
        providers.push(provider_result);
    }

    if any_published && emit_event {
        crate::usage_events::notify_log_recorded();
    }
    Ok(RebuildAgentSessionUsageResult { providers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_staged_rebuild_is_provider_scoped_and_keeps_live_data_on_failure() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let pi_root = PathBuf::from(r"C:\\cc-switch-pi-fixtures");
        let cursor_scope =
            CursorScope::PathPrefixes(vec![normalized_path(&pi_root.to_string_lossy())]);
        let old_cursor = pi_root.join("old.jsonl").to_string_lossy().to_string();
        let new_cursor = pi_root.join("new.jsonl").to_string_lossy().to_string();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('pi-direct-old', 'p', 'pi', 'm', 1, 1, '0', 0, 200, 1, 'pi_session')",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('other-direct', 'p', 'claude', 'm', 1, 1, '0', 0, 200, 1, 'session_log')",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('pi', 'pi-old', 'pi-old', 'standalone', 'high', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('claude', 'other-session', 'other-session', 'standalone', 'high', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_canonical_coverage (
                    app_type, data_source, request_id, canonical_session_id, marked_at
                 ) VALUES ('pi', 'pi_session', 'pi-direct-old', 'pi-old', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_canonical_coverage (
                    app_type, data_source, request_id, canonical_session_id, marked_at
                 ) VALUES ('claude', 'session_log', 'other-direct', 'other-session', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_usage_dedup
                 (data_source, request_id, semantic_id, has_entry_id)
                 VALUES ('pi_session', 'pi-direct-old', 'pi-old', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_usage_dedup
                 (data_source, request_id, semantic_id, has_entry_id)
                 VALUES ('other-source', 'other-direct', 'other', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync
                 (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES (?1, 1, 1, 1)",
                [&old_cursor],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync
                 (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES ('C:\\other-provider\\session.jsonl', 1, 1, 1)",
                [],
            )?;
        }

        let stage_path = copy_live_to_staging(&db)?;
        let stage_conn = Connection::open(&stage_path)?;
        let stage_db = Database {
            conn: Mutex::new(stage_conn),
        };
        reset_provider_stage(&stage_db, AgentUsageRebuildApp::Pi, &cursor_scope)?;
        {
            let conn = lock_conn!(stage_db.conn);
            let residuals: (i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'pi-direct-old'),
                    (SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'pi_session'),
                    (SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1),
                    (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'other-direct')",
                [&old_cursor],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            assert_eq!(residuals, (0, 0, 0, 1));
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('pi-direct-new', 'p', 'pi', 'm', 2, 2, '0', 0, 200, 2, 'pi_session')",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('pi', 'pi-new', 'pi-new', 'standalone', 'high', 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_canonical_coverage (
                    app_type, data_source, request_id, canonical_session_id, marked_at
                 ) VALUES ('pi', 'pi_session', 'pi-direct-new', 'pi-new', 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_usage_dedup
                 (data_source, request_id, semantic_id, has_entry_id)
                 VALUES ('pi_session', 'pi-direct-new', 'pi-new', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync
                 (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES (?1, 2, 2, 2)",
                [&new_cursor],
            )?;
        }
        drop(stage_db);

        let kept = finalize_staged_provider(
            &db,
            &stage_path,
            AgentUsageRebuildApp::Pi,
            &cursor_scope,
            SessionSyncResult {
                errors: vec!["synthetic Pi source failure".to_string()],
                deferred_files: 1,
                ..SessionSyncResult::default()
            },
            1,
        )?;
        assert_eq!(kept.status, ProviderUsageRebuildStatus::KeptPrevious);
        let old_is_still_live: i64 = lock_conn!(db.conn).query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'pi-direct-old'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(old_is_still_live, 1);

        let published = finalize_staged_provider(
            &db,
            &stage_path,
            AgentUsageRebuildApp::Pi,
            &cursor_scope,
            SessionSyncResult::default(),
            1,
        )?;
        assert_eq!(published.status, ProviderUsageRebuildStatus::Published);
        let conn = lock_conn!(db.conn);
        let final_counts: (i64, i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'pi-direct-old'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'pi-direct-new'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'other-direct'),
                (SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = 'pi' AND session_id = 'pi-new'),
                (SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = 'claude' AND session_id = 'other-session'),
                (SELECT COUNT(*) FROM session_usage_dedup WHERE data_source = 'pi_session' AND request_id = 'pi-direct-new'),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = 'C:\\other-provider\\session.jsonl')",
            [&new_cursor],
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
                ))
            },
        )?;
        assert_eq!(final_counts, (0, 1, 1, 1, 1, 1, 1, 1));
        Ok(())
    }

    #[test]
    fn staged_codex_publish_replaces_native_partitions_and_preserves_other_usage(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let codex_root = crate::codex_config::get_codex_config_dir();
        let cursor_scope = CursorScope::PathPrefixes(vec![
            normalized_path(&codex_root.join("sessions").to_string_lossy()),
            normalized_path(&codex_root.join("archived_sessions").to_string_lossy()),
        ]);
        let old_cursor = codex_root
            .join("sessions/2026/08/old.jsonl")
            .to_string_lossy()
            .to_string();
        let new_cursor = codex_root
            .join("sessions/2026/08/new.jsonl")
            .to_string_lossy()
            .to_string();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('proxy-keep', 'p', 'codex', 'gpt-5.6-sol', 1, 1,
                           '0', 0, 200, 1, 'proxy')",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('codex-old', '_codex_session', 'codex', 'gpt-5.6-sol',
                           2, 1, '0.1', 0, 200, 1, 'codex_session')",
                [],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, input_tokens
                 ) VALUES ('2026-08-01', 'codex', '_codex_session', 'gpt-5.6-sol', 9, 90)",
                [],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, input_tokens
                 ) VALUES ('2026-08-01', 'claude', 'claude-provider', 'claude-3', 4, 40)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('codex', 'codex-old', 'codex-old', 'standalone', 'high', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_usage_rollups (
                    date, app_type, session_id, provider_id, model, data_source,
                    request_count, input_tokens
                 ) VALUES ('2026-08-01', 'codex', 'codex-old', '_codex_session',
                           'gpt-5.6-sol', 'codex_session', 9, 90)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES (?1, 1, 1, 1)",
                [&old_cursor],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES ('other-provider-key', 1, 1, 1)",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value)
                 VALUES ('codex_usage_canonical_replay_v1', 'pending')",
                [],
            )?;
        }

        let stage_path = copy_live_to_staging(&db)?;
        let stage_conn = Connection::open(&stage_path)?;
        let stage_db = Database {
            conn: Mutex::new(stage_conn),
        };
        reset_provider_stage(&stage_db, AgentUsageRebuildApp::Codex, &cursor_scope)?;
        {
            let conn = lock_conn!(stage_db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('codex-new', '_codex_session', 'codex', 'gpt-5.6-sol',
                           3, 2, '0.2', 0, 200, 2, 'codex_session')",
                [],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model, request_count, input_tokens
                 ) VALUES ('2026-08-02', 'codex', '_codex_session', 'gpt-5.6-sol', 12, 120)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('codex', 'codex-new', 'codex-new', 'standalone', 'high', 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_usage_rollups (
                    date, app_type, session_id, provider_id, model, data_source,
                    request_count, input_tokens
                 ) VALUES ('2026-08-02', 'codex', 'codex-new', '_codex_session',
                           'gpt-5.6-sol', 'codex_session', 12, 120)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES (?1, 2, 2, 2)",
                [&new_cursor],
            )?;
        }
        drop(stage_db);

        publish_provider_from_stage(&db, &stage_path, AgentUsageRebuildApp::Codex, &cursor_scope)?;

        let conn = lock_conn!(db.conn);
        let counts: (i64, i64, i64, i64, i64, i64, i64, i64, i64) = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'proxy-keep'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'codex-old'),
                (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'codex-new'),
                (SELECT request_count FROM usage_daily_rollups
                 WHERE provider_id = '_codex_session' AND date = '2026-08-02'),
                (SELECT COUNT(*) FROM usage_daily_rollups
                 WHERE provider_id = '_codex_session' AND date = '2026-08-01'),
                (SELECT COUNT(*) FROM usage_daily_rollups
                 WHERE provider_id = 'claude-provider'),
                (SELECT COUNT(*) FROM agent_session_nodes
                 WHERE app_type = 'codex' AND session_id = 'codex-new'),
                (SELECT COUNT(*) FROM agent_session_nodes
                 WHERE app_type = 'codex' AND session_id = 'codex-old'),
                (SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1)
             ",
            [&new_cursor],
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
                ))
            },
        )?;
        assert_eq!(counts, (1, 0, 1, 12, 0, 1, 1, 0, 1));
        let old_cursor_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1",
            [&old_cursor],
            |row| row.get(0),
        )?;
        let other_cursor_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path = 'other-provider-key'",
            [],
            |row| row.get(0),
        )?;
        let replay_state: String = conn.query_row(
            "SELECT value FROM settings
             WHERE key = 'codex_usage_canonical_replay_v1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(old_cursor_count, 0);
        assert_eq!(other_cursor_count, 1);
        assert_eq!(replay_state, "complete");
        Ok(())
    }
}
