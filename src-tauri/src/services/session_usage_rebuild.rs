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
use crate::hermes_config::get_hermes_dir;
use crate::opencode_config::get_opencode_db_path;
use crate::services::agent_session_usage::{normalize_session_relations, write_agent_session_node};
use crate::services::session_usage::SessionSyncResult;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;

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
    match app {
        AgentUsageRebuildApp::Claude => {
            let root = get_claude_config_dir().join("projects");
            ensure_readable_directory(&root, "Claude projects")
        }
        AgentUsageRebuildApp::Codex => {
            crate::services::session_usage_codex::preflight_codex_usage()
        }
        AgentUsageRebuildApp::GrokBuild => {
            let roots = crate::session_manager::providers::grokbuild::session_roots();
            if roots.iter().any(|root| root.is_dir()) {
                Ok(())
            } else {
                Err(AppError::Config(
                    "没有找到可用于 Grok Build 用量重建的会话目录".to_string(),
                ))
            }
        }
        AgentUsageRebuildApp::OpenCode => {
            let db_path = get_opencode_db_path();
            let storage = db_path
                .parent()
                .map(|path| path.join("storage"))
                .unwrap_or_else(|| PathBuf::from("storage"));
            if db_path.is_file() || storage.is_dir() {
                Ok(())
            } else {
                Err(AppError::Config(
                    "没有找到可用于 OpenCode 用量重建的来源".to_string(),
                ))
            }
        }
        AgentUsageRebuildApp::Hermes => {
            let root = get_hermes_dir();
            if root.join("state.db").is_file()
                || root.join("profiles").is_dir()
                    && fs::read_dir(root.join("profiles"))
                        .ok()
                        .into_iter()
                        .flatten()
                        .any(|entry| {
                            entry
                                .ok()
                                .is_some_and(|entry| entry.path().join("state.db").is_file())
                        })
            {
                Ok(())
            } else {
                Err(AppError::Config(
                    "没有找到可用于 Hermes 用量重建的 state.db".to_string(),
                ))
            }
        }
        AgentUsageRebuildApp::Pi => {
            let files = crate::session_manager::providers::pi::session_files()
                .map_err(|error| AppError::Config(format!("无法发现 Pi 会话来源: {error}")))?;
            if files.is_empty() {
                Err(AppError::Config(
                    "没有找到可用于 Pi 用量重建的会话文件".to_string(),
                ))
            } else {
                Ok(())
            }
        }
    }
}

fn ensure_readable_directory(path: &Path, label: &str) -> Result<(), AppError> {
    if !path.is_dir() {
        return Err(AppError::Config(format!(
            "{label} 目录不存在: {}",
            path.display()
        )));
    }
    fs::read_dir(path).map(|_| ()).map_err(|error| {
        AppError::Config(format!("无法读取 {label} 目录 {}: {error}", path.display()))
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
    tx.execute(
        "DELETE FROM agent_session_nodes WHERE app_type = ?1",
        [app.app_type()],
    )?;
    tx.execute(
        "DELETE FROM agent_session_usage_rollups WHERE app_type = ?1",
        [app.app_type()],
    )?;
    tx.execute(
        "DELETE FROM agent_session_usage_snapshots WHERE app_type = ?1",
        [app.app_type()],
    )?;
    tx.execute(
        "DELETE FROM agent_session_canonical_coverage WHERE app_type = ?1",
        [app.app_type()],
    )?;
    if let Some(source) = app.direct_source() {
        tx.execute(
            "DELETE FROM proxy_request_logs
             WHERE app_type = ?1 AND data_source = ?2",
            rusqlite::params![app.app_type(), source],
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
    match app {
        AgentUsageRebuildApp::Claude => {
            crate::services::session_usage::sync_claude_session_logs(stage_db)
        }
        AgentUsageRebuildApp::GrokBuild => {
            crate::services::session_usage_grokbuild::sync_grokbuild_usage(stage_db)
        }
        AgentUsageRebuildApp::OpenCode => {
            crate::services::session_usage_opencode::sync_opencode_usage(stage_db)
        }
        AgentUsageRebuildApp::Hermes => {
            let detailed =
                crate::services::session_usage_hermes::sync_hermes_usage_detailed(stage_db)?;
            let claims =
                crate::services::session_usage_hermes::hermes_standalone_session_claims(&detailed);
            let normalized = normalize_session_relations(&claims)?;
            for node in &normalized {
                write_agent_session_node(stage_db, node)?;
            }
            Ok(detailed.as_session_sync_result())
        }
        AgentUsageRebuildApp::Pi => crate::services::session_usage_pi::sync_pi_usage(stage_db),
        AgentUsageRebuildApp::Codex => Err(AppError::InvalidInput(
            "Codex 必须通过 replay shadow generation 重建".to_string(),
        )),
    }
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
    tx.execute(
        "DELETE FROM agent_session_nodes WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "INSERT INTO agent_session_nodes (
            app_type, session_id, parent_session_id, root_session_id,
            node_kind, relation_confidence, title, project_dir, source_path,
            created_at, last_active_at, last_synced_at
         ) SELECT app_type, session_id, parent_session_id, root_session_id,
            node_kind, relation_confidence, title, project_dir, source_path,
            created_at, last_active_at, last_synced_at
         FROM rebuild_stage.agent_session_nodes WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "DELETE FROM agent_session_usage_rollups WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "INSERT INTO agent_session_usage_rollups (
            date, app_type, session_id, provider_id, model, request_model,
            pricing_model, data_source, precision, time_semantics,
            request_count_semantics, input_token_semantics, source_identity,
            profile_id, database_identity, base_url_digest, billing_mode, task,
            source_version, sync_window_start, sync_window_end, request_count,
            api_call_count, input_tokens, output_tokens, cache_read_tokens,
            cache_creation_tokens, cache_write_tokens, reasoning_tokens,
            total_cost_usd, cost_status, cost_source, cost_delta_kind,
            correction_state, first_event_at, last_event_at
         ) SELECT date, app_type, session_id, provider_id, model, request_model,
            pricing_model, data_source, precision, time_semantics,
            request_count_semantics, input_token_semantics, source_identity,
            profile_id, database_identity, base_url_digest, billing_mode, task,
            source_version, sync_window_start, sync_window_end, request_count,
            api_call_count, input_tokens, output_tokens, cache_read_tokens,
            cache_creation_tokens, cache_write_tokens, reasoning_tokens,
            total_cost_usd, cost_status, cost_source, cost_delta_kind,
            correction_state, first_event_at, last_event_at
         FROM rebuild_stage.agent_session_usage_rollups WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "DELETE FROM agent_session_usage_snapshots WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "INSERT INTO agent_session_usage_snapshots (
            app_type, source_identity, profile_id, database_identity, session_id,
            model, provider_id, base_url_digest, billing_mode, task, data_source,
            source_version, api_call_count, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens, first_seen,
            last_seen, last_synced_at, estimated_cost_usd, actual_cost_usd,
            cost_status, cost_source, correction_state
         ) SELECT app_type, source_identity, profile_id, database_identity, session_id,
            model, provider_id, base_url_digest, billing_mode, task, data_source,
            source_version, api_call_count, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens, first_seen,
            last_seen, last_synced_at, estimated_cost_usd, actual_cost_usd,
            cost_status, cost_source, correction_state
         FROM rebuild_stage.agent_session_usage_snapshots WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "DELETE FROM agent_session_canonical_coverage WHERE app_type = ?1",
        [app_type],
    )?;
    tx.execute(
        "INSERT INTO agent_session_canonical_coverage (
            app_type, data_source, request_id, canonical_session_id, marked_at
         ) SELECT app_type, data_source, request_id, canonical_session_id, marked_at
         FROM rebuild_stage.agent_session_canonical_coverage WHERE app_type = ?1",
        [app_type],
    )?;
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
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics, input_cost_usd, output_cost_usd,
                cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, duration_ms, status_code, error_message,
                session_id, provider_type, is_streaming, cost_multiplier, created_at,
                data_source
             ) SELECT request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics, input_cost_usd, output_cost_usd,
                cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, duration_ms, status_code, error_message,
                session_id, provider_type, is_streaming, cost_multiplier, created_at,
                data_source
             FROM rebuild_stage.proxy_request_logs
             WHERE app_type = ?1 AND data_source = ?2
               AND NOT EXISTS (
                   SELECT 1 FROM proxy_request_logs live
                   WHERE live.request_id = rebuild_stage.proxy_request_logs.request_id
                     AND COALESCE(live.data_source, 'proxy') = 'proxy'
               )",
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

fn rebuild_non_codex_provider(
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

        let provider_result = if app == AgentUsageRebuildApp::Codex {
            match crate::services::session_usage_codex::rebuild_codex_usage_with_backup(db, false) {
                Ok(sync_result) => {
                    let published_state = sync_result.errors.is_empty()
                        && sync_result.deferred_files == 0
                        && crate::services::session_usage_codex::codex_rebuild_is_published(db)?;
                    if published_state {
                        any_published = true;
                        published(app, sync_result)
                    } else {
                        kept_previous(app, sync_result)
                    }
                }
                Err(error) => kept_previous(app, sync_error(format!("Codex 重建失败: {error}"))),
            }
        } else {
            let cursor_scope = CursorScope::capture(app);
            match rebuild_non_codex_provider(db, app, &cursor_scope) {
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
            }
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
    fn request_dedup_preserves_stable_order() {
        let request = RebuildAgentSessionUsageRequest {
            app_types: vec![
                AgentUsageRebuildApp::OpenCode,
                AgentUsageRebuildApp::Claude,
                AgentUsageRebuildApp::OpenCode,
                AgentUsageRebuildApp::Hermes,
                AgentUsageRebuildApp::Pi,
            ],
        };
        assert_eq!(
            normalized_apps(&request).unwrap(),
            vec![
                AgentUsageRebuildApp::OpenCode,
                AgentUsageRebuildApp::Claude,
                AgentUsageRebuildApp::Hermes,
                AgentUsageRebuildApp::Pi,
            ]
        );
    }

    #[test]
    fn empty_request_is_rejected_before_database_work() {
        let request = RebuildAgentSessionUsageRequest { app_types: vec![] };
        assert!(normalized_apps(&request).is_err());
    }

    #[test]
    fn result_contract_uses_camel_case_and_provider_names() {
        let value = ProviderUsageRebuildResult {
            app_type: AgentUsageRebuildApp::GrokBuild,
            status: ProviderUsageRebuildStatus::KeptPrevious,
            sync_result: SessionSyncResult::default(),
        };
        let json = serde_json::to_value(value).unwrap();
        assert_eq!(json["appType"], "grokbuild");
        assert_eq!(json["status"], "keptPrevious");
        assert!(json.get("syncResult").is_some());
        assert_eq!(
            serde_json::from_str::<AgentUsageRebuildApp>("\"pi\"").unwrap(),
            AgentUsageRebuildApp::Pi
        );
    }

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
    fn staged_publish_replaces_only_selected_provider_and_proxy_survives() -> Result<(), AppError> {
        let db = Database::memory()?;
        let claude_root = get_claude_config_dir().join("projects");
        let cursor_scope =
            CursorScope::PathPrefixes(vec![normalized_path(&claude_root.to_string_lossy())]);
        let old_claude_cursor = claude_root.join("old.jsonl").to_string_lossy().to_string();
        let new_claude_cursor = claude_root.join("new.jsonl").to_string_lossy().to_string();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('proxy-keep', 'p', 'claude', 'm', 1, 1, '0', 0, 200, 1, 'proxy')",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('direct-old', 'p', 'claude', 'm', 1, 1, '0', 0, 200, 1, 'session_log')",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('opencode', 'other-session', 'other-session', 'standalone', 'high', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES (?1, 1, 1, 1)",
                [&old_claude_cursor],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES ('other-provider-key', 2, 2, 2)",
                [],
            )?;
        }

        let stage_path = copy_live_to_staging(&db)?;
        let stage_conn = Connection::open(&stage_path)?;
        let stage_db = Database {
            conn: Mutex::new(stage_conn),
        };
        reset_provider_stage(&stage_db, AgentUsageRebuildApp::Claude, &cursor_scope)?;
        {
            let conn = lock_conn!(stage_db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('direct-new', 'p', 'claude', 'm', 2, 2, '0', 0, 200, 2, 'session_log')",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('claude', 'new-session', 'new-session', 'standalone', 'high', 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES (?1, 2, 2, 2)",
                [&new_claude_cursor],
            )?;
        }
        drop(stage_db);
        publish_provider_from_stage(
            &db,
            &stage_path,
            AgentUsageRebuildApp::Claude,
            &cursor_scope,
        )?;

        let conn = lock_conn!(db.conn);
        let proxy_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'proxy-keep'",
            [],
            |row| row.get(0),
        )?;
        let old_direct_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'direct-old'",
            [],
            |row| row.get(0),
        )?;
        let new_direct_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'direct-new'",
            [],
            |row| row.get(0),
        )?;
        let other_node_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_session_nodes WHERE app_type = 'opencode'",
            [],
            |row| row.get(0),
        )?;
        let claude_node: String = conn.query_row(
            "SELECT session_id FROM agent_session_nodes WHERE app_type = 'claude'",
            [],
            |row| row.get(0),
        )?;
        let old_cursor_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1",
            [&old_claude_cursor],
            |row| row.get(0),
        )?;
        let new_cursor_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1",
            [&new_claude_cursor],
            |row| row.get(0),
        )?;
        let other_cursor_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_log_sync WHERE file_path = 'other-provider-key'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(proxy_count, 1);
        assert_eq!(old_direct_count, 0);
        assert_eq!(new_direct_count, 1);
        assert_eq!(other_node_count, 1);
        assert_eq!(claude_node, "new-session");
        assert_eq!(old_cursor_count, 0);
        assert_eq!(new_cursor_count, 1);
        assert_eq!(other_cursor_count, 1);
        Ok(())
    }

    #[test]
    fn failed_or_deferred_stage_keeps_live_provider_rows_and_cursor() -> Result<(), AppError> {
        let db = Database::memory()?;
        let claude_root = get_claude_config_dir().join("projects");
        let cursor_scope =
            CursorScope::PathPrefixes(vec![normalized_path(&claude_root.to_string_lossy())]);
        let cursor = claude_root
            .join("unchanged.jsonl")
            .to_string_lossy()
            .to_string();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('live-direct', 'p', 'claude', 'm', 1, 1, '0', 0, 200, 1, 'session_log')",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_log_sync (
                    file_path, last_modified, last_line_offset, last_synced_at
                 ) VALUES (?1, 1, 1, 1)",
                [&cursor],
            )?;
        }

        let stage_path = copy_live_to_staging(&db)?;
        let stage_conn = Connection::open(&stage_path)?;
        let stage_db = Database {
            conn: Mutex::new(stage_conn),
        };
        reset_provider_stage(&stage_db, AgentUsageRebuildApp::Claude, &cursor_scope)?;
        {
            let conn = lock_conn!(stage_db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, input_tokens,
                    output_tokens, total_cost_usd, latency_ms, status_code,
                    created_at, data_source
                 ) VALUES ('staged-only', 'p', 'claude', 'm', 9, 9, '0', 0, 200, 9, 'session_log')",
                [],
            )?;
            conn.execute(
                "INSERT INTO agent_session_nodes (
                    app_type, session_id, root_session_id, node_kind,
                    relation_confidence, last_synced_at
                 ) VALUES ('claude', 'staged-session', 'staged-session', 'standalone', 'high', 9)",
                [],
            )?;
        }
        drop(stage_db);

        for sync_result in [
            SessionSyncResult {
                errors: vec!["synthetic stage parse error".to_string()],
                ..SessionSyncResult::default()
            },
            SessionSyncResult {
                deferred_files: 1,
                ..SessionSyncResult::default()
            },
        ] {
            let result = finalize_staged_provider(
                &db,
                &stage_path,
                AgentUsageRebuildApp::Claude,
                &cursor_scope,
                sync_result,
                1,
            )?;
            assert_eq!(result.status, ProviderUsageRebuildStatus::KeptPrevious);
            let conn = lock_conn!(db.conn);
            let live_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'live-direct'",
                [],
                |row| row.get(0),
            )?;
            let staged_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'staged-only'",
                [],
                |row| row.get(0),
            )?;
            let cursor_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM session_log_sync WHERE file_path = ?1",
                [&cursor],
                |row| row.get(0),
            )?;
            assert_eq!((live_count, staged_count, cursor_count), (1, 0, 1));
        }
        Ok(())
    }
}
