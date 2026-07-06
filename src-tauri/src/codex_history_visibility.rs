use crate::codex_config::{get_codex_config_dir, read_codex_config_text};
use crate::codex_state_db::codex_state_db_paths;
use crate::config::{atomic_write, copy_file, get_app_config_dir};
use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_CODEX_PROVIDER: &str = "openai";
const BACKUP_NAME: &str = "codex-history-visibility-repair-v1";
const SESSION_INDEX_FILENAME: &str = "session_index.jsonl";
const GLOBAL_STATE_FILENAME: &str = ".codex-global-state.json";
const GLOBAL_STATE_BACKUP_FILENAME: &str = ".codex-global-state.json.bak";

static CODEX_HISTORY_VISIBILITY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryVisibilityDiagnosis {
    pub codex_dir: String,
    pub current_provider: String,
    pub current_provider_implicit: bool,
    pub rollout_counts: BTreeMap<String, BTreeMap<String, usize>>,
    pub encrypted_content_counts: BTreeMap<String, BTreeMap<String, usize>>,
    pub rollout_files: usize,
    pub rollout_files_needing_provider_sync: usize,
    pub sqlite_counts: BTreeMap<String, BTreeMap<String, usize>>,
    pub sqlite_rows: usize,
    pub sqlite_rows_needing_provider_sync: usize,
    pub sqlite_user_event_rows_needing_repair: usize,
    pub sqlite_cwd_rows_needing_repair: usize,
    pub session_index_entries: usize,
    pub session_index_valid: bool,
    pub session_index_needs_rebuild: bool,
    pub workspace_roots_needing_repair: usize,
    pub locked_or_unreadable_rollout_files: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryVisibilityRepairResult {
    pub diagnosis_before: CodexHistoryVisibilityDiagnosis,
    pub backup_dir: String,
    pub changed_rollout_files: usize,
    pub skipped_rollout_files: usize,
    pub sqlite_provider_rows_updated: usize,
    pub sqlite_user_event_rows_updated: usize,
    pub sqlite_cwd_rows_updated: usize,
    pub workspace_roots_updated: usize,
    pub session_index_rebuilt: bool,
    pub session_index_entries_written: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct SessionChange {
    path: PathBuf,
    directory: String,
    thread_id: Option<String>,
    original_first_line: String,
    separator: String,
    updated_first_line: String,
}

#[derive(Debug, Clone, Default)]
struct SessionScan {
    changes: Vec<SessionChange>,
    provider_counts: BTreeMap<String, BTreeMap<String, usize>>,
    encrypted_content_counts: BTreeMap<String, BTreeMap<String, usize>>,
    user_event_thread_ids: HashSet<String>,
    thread_cwd_by_id: HashMap<String, String>,
    total_files: usize,
    unreadable_files: usize,
}

#[derive(Debug, Deserialize)]
struct SessionIndexEntry {
    id: Option<String>,
    thread_name: Option<String>,
    updated_at: Option<String>,
}

pub fn diagnose_codex_history_visibility(
) -> Result<CodexHistoryVisibilityDiagnosis, AppError> {
    let _guard = CODEX_HISTORY_VISIBILITY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    diagnose_inner()
}

pub fn repair_codex_history_visibility(
) -> Result<CodexHistoryVisibilityRepairResult, AppError> {
    let _guard = CODEX_HISTORY_VISIBILITY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let diagnosis_before = diagnose_inner()?;
    let codex_dir = get_codex_config_dir();
    let config_text = read_codex_config_text().unwrap_or_default();
    let (current_provider, _) = current_provider_from_config(&config_text);
    let scan = scan_sessions(&codex_dir, &current_provider);
    let backup_dir = create_backup_dir()?;

    backup_static_files(&codex_dir, &config_text, &backup_dir)?;
    write_session_manifest(&backup_dir, &scan.changes)?;

    let mut changed_rollout_files = 0;
    let mut skipped_rollout_files = 0;
    for change in &scan.changes {
        match rewrite_session_first_line(change) {
            Ok(true) => changed_rollout_files += 1,
            Ok(false) => skipped_rollout_files += 1,
            Err(err) => {
                log::warn!(
                    "Failed to rewrite Codex rollout {}: {err}",
                    change.path.display()
                );
                skipped_rollout_files += 1;
            }
        }
    }

    let sqlite_result = update_sqlite_metadata(
        &codex_dir,
        &config_text,
        &current_provider,
        &scan.user_event_thread_ids,
        &scan.thread_cwd_by_id,
    )?;
    let workspace_result = sync_workspace_roots(&codex_dir)?;
    let (session_index_rebuilt, session_index_entries_written) =
        rebuild_session_index_if_needed(&codex_dir, &config_text, &diagnosis_before)?;

    let mut warnings = diagnosis_before.warnings.clone();
    if skipped_rollout_files > 0 {
        warnings.push(format!(
            "{skipped_rollout_files} rollout file(s) were skipped because they changed or could not be rewritten"
        ));
    }

    Ok(CodexHistoryVisibilityRepairResult {
        diagnosis_before,
        backup_dir: backup_dir.to_string_lossy().to_string(),
        changed_rollout_files,
        skipped_rollout_files,
        sqlite_provider_rows_updated: sqlite_result.provider_rows,
        sqlite_user_event_rows_updated: sqlite_result.user_event_rows,
        sqlite_cwd_rows_updated: sqlite_result.cwd_rows,
        workspace_roots_updated: workspace_result.updated_roots,
        session_index_rebuilt,
        session_index_entries_written,
        warnings,
    })
}

fn diagnose_inner() -> Result<CodexHistoryVisibilityDiagnosis, AppError> {
    let codex_dir = get_codex_config_dir();
    let config_text = read_codex_config_text().unwrap_or_default();
    let (current_provider, current_provider_implicit) = current_provider_from_config(&config_text);
    let scan = scan_sessions(&codex_dir, &current_provider);
    let sqlite = read_sqlite_diagnosis(
        &codex_dir,
        &config_text,
        &current_provider,
        &scan.user_event_thread_ids,
        &scan.thread_cwd_by_id,
    )?;
    let session_index =
        inspect_session_index(&codex_dir, sqlite.unique_thread_ids_for_index)?;
    let workspace_roots_needing_repair = inspect_workspace_roots(&codex_dir)?;

    let mut warnings = Vec::new();
    for (scope, counts) in &scan.encrypted_content_counts {
        for (provider, count) in counts {
            if *count > 0 && provider != &current_provider {
                warnings.push(format!(
                    "{count} {scope} rollout file(s) contain encrypted_content from provider {provider}; visibility can be repaired, but cross-provider continuation may still fail"
                ));
            }
        }
    }
    if scan.unreadable_files > 0 {
        warnings.push(format!(
            "{} rollout file(s) could not be read during diagnosis",
            scan.unreadable_files
        ));
    }
    if session_index.needs_rebuild {
        warnings.push("session_index.jsonl is missing, invalid, or behind the SQLite thread index".to_string());
    }

    Ok(CodexHistoryVisibilityDiagnosis {
        codex_dir: codex_dir.to_string_lossy().to_string(),
        current_provider,
        current_provider_implicit,
        rollout_counts: scan.provider_counts,
        encrypted_content_counts: scan.encrypted_content_counts,
        rollout_files: scan.total_files,
        rollout_files_needing_provider_sync: scan.changes.len(),
        sqlite_counts: sqlite.counts,
        sqlite_rows: sqlite.total_rows,
        sqlite_rows_needing_provider_sync: sqlite.provider_rows_needing_repair,
        sqlite_user_event_rows_needing_repair: sqlite.user_event_rows_needing_repair,
        sqlite_cwd_rows_needing_repair: sqlite.cwd_rows_needing_repair,
        session_index_entries: session_index.entries,
        session_index_valid: session_index.valid,
        session_index_needs_rebuild: session_index.needs_rebuild,
        workspace_roots_needing_repair,
        locked_or_unreadable_rollout_files: scan.unreadable_files,
        warnings,
    })
}

fn current_provider_from_config(config_text: &str) -> (String, bool) {
    let parsed = config_text.parse::<toml_edit::DocumentMut>().ok();
    let provider = parsed
        .as_ref()
        .and_then(|doc| doc.get("model_provider"))
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    match provider {
        Some(provider) => (provider, false),
        None => (DEFAULT_CODEX_PROVIDER.to_string(), true),
    }
}

fn scan_sessions(codex_dir: &Path, target_provider: &str) -> SessionScan {
    let mut scan = SessionScan::default();
    for scope in ["sessions", "archived_sessions"] {
        let root = codex_dir.join(scope);
        let mut files = Vec::new();
        collect_rollout_files(&root, &mut files);
        for path in files {
            scan.total_files += 1;
            match inspect_rollout_file(&path, scope, target_provider) {
                Ok(Some(item)) => {
                    increment_count(&mut scan.provider_counts, scope, &item.current_provider);
                    if item.has_encrypted_content {
                        increment_count(
                            &mut scan.encrypted_content_counts,
                            scope,
                            &item.current_provider,
                        );
                    }
                    if item.has_user_event {
                        if let Some(thread_id) = item.thread_id.clone() {
                            scan.user_event_thread_ids.insert(thread_id);
                        }
                    }
                    if let (Some(thread_id), Some(cwd)) = (item.thread_id.clone(), item.cwd.clone())
                    {
                        scan.thread_cwd_by_id.insert(thread_id, cwd);
                    }
                    if let Some(change) = item.change {
                        scan.changes.push(change);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    log::warn!("Failed to inspect Codex rollout {}: {err}", path.display());
                    scan.unreadable_files += 1;
                }
            }
        }
    }
    scan
}

struct RolloutInspection {
    current_provider: String,
    thread_id: Option<String>,
    cwd: Option<String>,
    has_encrypted_content: bool,
    has_user_event: bool,
    change: Option<SessionChange>,
}

fn inspect_rollout_file(
    path: &Path,
    scope: &str,
    target_provider: &str,
) -> Result<Option<RolloutInspection>, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    let (first_line, separator, body_offset) = split_first_line(&bytes);
    let mut meta: Value = match serde_json::from_slice(first_line) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if meta.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }
    let payload = meta.get_mut("payload").and_then(Value::as_object_mut);
    let Some(payload) = payload else {
        return Ok(None);
    };
    let current_provider = payload
        .get("model_provider")
        .and_then(Value::as_str)
        .unwrap_or("(missing)")
        .to_string();
    let thread_id = payload
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(to_desktop_workspace_path)
        .filter(|value| !value.trim().is_empty());
    let has_encrypted_content = bytes.windows(b"encrypted_content".len()).any(|window| {
        window == b"encrypted_content"
    });
    let has_user_event = rollout_has_user_event(first_line, &bytes[body_offset..]);
    let change = if current_provider != target_provider {
        payload.insert(
            "model_provider".to_string(),
            Value::String(target_provider.to_string()),
        );
        Some(SessionChange {
            path: path.to_path_buf(),
            directory: scope.to_string(),
            thread_id: thread_id.clone(),
            original_first_line: String::from_utf8_lossy(first_line).to_string(),
            separator: separator.to_string(),
            updated_first_line: serde_json::to_string(&meta)
                .map_err(|e| AppError::JsonSerialize { source: e })?,
        })
    } else {
        None
    };
    Ok(Some(RolloutInspection {
        current_provider,
        thread_id,
        cwd,
        has_encrypted_content,
        has_user_event,
        change,
    }))
}

fn split_first_line(bytes: &[u8]) -> (&[u8], &str, usize) {
    if let Some(index) = bytes.iter().position(|byte| *byte == b'\n') {
        let line_end = if index > 0 && bytes[index - 1] == b'\r' {
            index - 1
        } else {
            index
        };
        let separator = if line_end == index { "\n" } else { "\r\n" };
        (&bytes[..line_end], separator, index + 1)
    } else {
        (bytes, "", bytes.len())
    }
}

fn rollout_has_user_event(first_line: &[u8], body: &[u8]) -> bool {
    if line_has_user_event(first_line) {
        return true;
    }
    for line in body.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line_has_user_event(line) {
            return true;
        }
    }
    false
}

fn line_has_user_event(line: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    if value.get("type").and_then(Value::as_str) == Some("event_msg")
        && value
            .get("payload")
            .and_then(|payload| payload.get("type"))
            .and_then(Value::as_str)
            == Some("user_message")
    {
        return true;
    }
    for key in ["payload", "item", "msg"] {
        if value
            .get(key)
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("message")
            && value
                .get(key)
                .and_then(|item| item.get("role"))
                .and_then(Value::as_str)
                == Some("user")
        {
            return true;
        }
    }
    false
}

fn rewrite_session_first_line(change: &SessionChange) -> Result<bool, AppError> {
    let bytes = fs::read(&change.path).map_err(|e| AppError::io(&change.path, e))?;
    let (first_line, separator, body_offset) = split_first_line(&bytes);
    if first_line != change.original_first_line.as_bytes() || separator != change.separator {
        return Ok(false);
    }
    let mut next = Vec::new();
    next.extend_from_slice(change.updated_first_line.as_bytes());
    next.extend_from_slice(separator.as_bytes());
    next.extend_from_slice(&bytes[body_offset..]);
    atomic_write(&change.path, &next)?;
    Ok(true)
}

#[derive(Default)]
struct SqliteDiagnosis {
    counts: BTreeMap<String, BTreeMap<String, usize>>,
    total_rows: usize,
    unique_thread_ids_for_index: usize,
    provider_rows_needing_repair: usize,
    user_event_rows_needing_repair: usize,
    cwd_rows_needing_repair: usize,
}

fn read_sqlite_diagnosis(
    codex_dir: &Path,
    config_text: &str,
    target_provider: &str,
    user_event_thread_ids: &HashSet<String>,
    thread_cwd_by_id: &HashMap<String, String>,
) -> Result<SqliteDiagnosis, AppError> {
    let mut result = SqliteDiagnosis::default();
    let mut unique_thread_ids = HashSet::new();
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if !db_path.exists() {
            continue;
        }
        let conn = open_sqlite_readonly(&db_path)?;
        let columns = table_columns(&conn, "threads")?;
        if !columns.contains("model_provider") {
            continue;
        }
        let archived_expr = if columns.contains("archived") {
            "archived"
        } else {
            "0"
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT COALESCE(NULLIF(model_provider,''), '(missing)'), {archived_expr}, COUNT(*) FROM threads GROUP BY 1, 2"
        ))?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1).unwrap_or(0),
                row.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (provider, archived, count) = row?;
            let scope = if archived != 0 {
                "archived_sessions"
            } else {
                "sessions"
            };
            *result
                .counts
                .entry(scope.to_string())
                .or_default()
                .entry(provider)
                .or_default() += count.max(0) as usize;
            result.total_rows += count.max(0) as usize;
        }
        result.provider_rows_needing_repair += conn.query_row(
            "SELECT COUNT(*) FROM threads WHERE COALESCE(model_provider, '') <> ?1",
            params![target_provider],
            |row| row.get::<_, i64>(0),
        )? as usize;

        let mut id_stmt = conn.prepare("SELECT id FROM threads WHERE id IS NOT NULL AND id <> ''")?;
        let ids = id_stmt.query_map([], |row| row.get::<_, String>(0))?;
        for id in ids {
            unique_thread_ids.insert(id?);
        }

        if columns.contains("has_user_event") {
            for thread_id in user_event_thread_ids {
                let value: Option<i64> = conn
                    .query_row(
                        "SELECT has_user_event FROM threads WHERE id = ?1",
                        params![thread_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if value.is_some_and(|value| value != 1) {
                    result.user_event_rows_needing_repair += 1;
                }
            }
        }
        if columns.contains("cwd") {
            for (thread_id, cwd) in thread_cwd_by_id {
                let value: Option<String> = conn
                    .query_row(
                        "SELECT cwd FROM threads WHERE id = ?1",
                        params![thread_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if value.is_some_and(|value| value != *cwd) {
                    result.cwd_rows_needing_repair += 1;
                }
            }
        }
    }
    result.unique_thread_ids_for_index = unique_thread_ids.len();
    Ok(result)
}

#[derive(Default)]
struct SqliteUpdateResult {
    provider_rows: usize,
    user_event_rows: usize,
    cwd_rows: usize,
}

fn update_sqlite_metadata(
    codex_dir: &Path,
    config_text: &str,
    target_provider: &str,
    user_event_thread_ids: &HashSet<String>,
    thread_cwd_by_id: &HashMap<String, String>,
) -> Result<SqliteUpdateResult, AppError> {
    let mut result = SqliteUpdateResult::default();
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if !db_path.exists() {
            continue;
        }
        let mut conn = Connection::open(&db_path).map_err(|e| {
            AppError::Database(format!("Failed to open {}: {e}", db_path.display()))
        })?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let columns = table_columns(&conn, "threads")?;
        let tx = conn.transaction()?;
        if columns.contains("model_provider") {
            result.provider_rows += tx
                .execute(
                    "UPDATE threads SET model_provider = ?1 WHERE COALESCE(model_provider, '') <> ?1",
                    params![target_provider],
                )? as usize;
        }
        if columns.contains("has_user_event") {
            let mut stmt = tx.prepare(
                "UPDATE threads SET has_user_event = 1 WHERE id = ?1 AND COALESCE(has_user_event, 0) <> 1",
            )?;
            for thread_id in user_event_thread_ids {
                result.user_event_rows += stmt.execute(params![thread_id])? as usize;
            }
            drop(stmt);
        }
        if columns.contains("cwd") {
            let mut stmt = tx.prepare(
                "UPDATE threads SET cwd = ?1 WHERE id = ?2 AND COALESCE(cwd, '') <> ?1",
            )?;
            for (thread_id, cwd) in thread_cwd_by_id {
                if !cwd.trim().is_empty() {
                    result.cwd_rows += stmt.execute(params![cwd, thread_id])? as usize;
                }
            }
            drop(stmt);
        }
        tx.commit()?;
    }
    Ok(result)
}

fn open_sqlite_readonly(path: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::Database(format!("Failed to open {}: {e}", path.display())))?;
    conn.busy_timeout(Duration::from_secs(2))?;
    Ok(conn)
}

fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>, AppError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = HashSet::new();
    for row in rows {
        columns.insert(row?);
    }
    Ok(columns)
}

#[derive(Default)]
struct SessionIndexInspection {
    entries: usize,
    valid: bool,
    needs_rebuild: bool,
}

fn inspect_session_index(
    codex_dir: &Path,
    expected_entries: usize,
) -> Result<SessionIndexInspection, AppError> {
    let path = codex_dir.join(SESSION_INDEX_FILENAME);
    if !path.exists() {
        return Ok(SessionIndexInspection {
            entries: 0,
            valid: false,
            needs_rebuild: expected_entries > 0,
        });
    }
    let file = File::open(&path).map_err(|e| AppError::io(&path, e))?;
    let reader = BufReader::new(file);
    let mut entries = 0usize;
    let mut valid = true;
    for line in reader.lines() {
        let line = line.map_err(|e| AppError::io(&path, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<SessionIndexEntry>(trimmed) {
            Ok(entry) if entry.id.is_some() && entry.thread_name.is_some() => {
                let _ = entry.updated_at;
                entries += 1;
            }
            _ => valid = false,
        }
    }
    Ok(SessionIndexInspection {
        entries,
        valid,
        needs_rebuild: !valid || entries < expected_entries,
    })
}

fn rebuild_session_index_if_needed(
    codex_dir: &Path,
    config_text: &str,
    diagnosis: &CodexHistoryVisibilityDiagnosis,
) -> Result<(bool, usize), AppError> {
    if !diagnosis.session_index_needs_rebuild {
        return Ok((false, diagnosis.session_index_entries));
    }
    let entries = load_session_index_entries_from_sqlite(codex_dir, config_text)?;
    if entries.is_empty() {
        return Ok((false, 0));
    }
    let mut output = String::new();
    for entry in &entries {
        output.push_str(
            &serde_json::to_string(entry).map_err(|e| AppError::JsonSerialize { source: e })?,
        );
        output.push('\n');
    }
    atomic_write(&codex_dir.join(SESSION_INDEX_FILENAME), output.as_bytes())?;
    Ok((true, entries.len()))
}

fn load_session_index_entries_from_sqlite(
    codex_dir: &Path,
    config_text: &str,
) -> Result<Vec<Value>, AppError> {
    let mut entries_by_id: BTreeMap<String, Value> = BTreeMap::new();
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if !db_path.exists() {
            continue;
        }
        let conn = open_sqlite_readonly(&db_path)?;
        let columns = table_columns(&conn, "threads")?;
        let title_expr = first_existing_expr(
            &columns,
            &["title", "preview", "first_user_message"],
            "'Untitled'",
        );
        let time_expr = if columns.contains("updated_at_ms") {
            "CAST(updated_at_ms / 1000 AS INTEGER)"
        } else if columns.contains("updated_at") {
            "updated_at"
        } else if columns.contains("created_at_ms") {
            "CAST(created_at_ms / 1000 AS INTEGER)"
        } else if columns.contains("created_at") {
            "created_at"
        } else {
            "0"
        };
        let sql = format!(
            "SELECT id, {title_expr} AS thread_name, {time_expr} AS ts FROM threads WHERE id IS NOT NULL AND id <> '' ORDER BY ts"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1).unwrap_or_else(|_| "Untitled".to_string());
            let ts: i64 = row.get(2).unwrap_or(0);
            Ok((id, title, ts))
        })?;
        for row in rows {
            let (id, title, ts) = row?;
            entries_by_id.insert(
                id.clone(),
                json!({
                    "id": id,
                    "thread_name": if title.trim().is_empty() { "Untitled" } else { title.trim() },
                    "updated_at": timestamp_to_rfc3339(ts),
                }),
            );
        }
    }
    Ok(entries_by_id.into_values().collect())
}

fn first_existing_expr(columns: &HashSet<String>, names: &[&str], fallback: &str) -> String {
    let mut parts: Vec<String> = names
        .iter()
        .filter(|name| columns.contains(&(*name).to_string()))
        .map(|name| format!("NULLIF({name}, '')"))
        .collect();
    parts.push(fallback.to_string());
    format!("COALESCE({})", parts.join(", "))
}

fn timestamp_to_rfc3339(ts: i64) -> String {
    chrono::DateTime::<Utc>::from_timestamp(ts.max(0), 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

#[derive(Default)]
struct WorkspaceSyncResult {
    updated_roots: usize,
}

fn inspect_workspace_roots(codex_dir: &Path) -> Result<usize, AppError> {
    let path = codex_dir.join(GLOBAL_STATE_FILENAME);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(0);
    };
    let state: Value = serde_json::from_str(&text).map_err(|e| AppError::json(&path, e))?;
    let mut count = 0;
    for key in [
        "electron-saved-workspace-roots",
        "project-order",
        "active-workspace-roots",
    ] {
        for value in path_values(state.get(key)) {
            if to_desktop_workspace_path(&value) != value {
                count += 1;
            }
        }
    }
    Ok(count)
}

fn sync_workspace_roots(codex_dir: &Path) -> Result<WorkspaceSyncResult, AppError> {
    let path = codex_dir.join(GLOBAL_STATE_FILENAME);
    if !path.exists() {
        return Ok(WorkspaceSyncResult::default());
    }
    let text = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let mut state: Value = serde_json::from_str(&text).map_err(|e| AppError::json(&path, e))?;
    let mut updated_roots = 0usize;

    for key in [
        "electron-saved-workspace-roots",
        "project-order",
        "active-workspace-roots",
    ] {
        updated_roots += normalize_path_value(state.get_mut(key));
    }
    if let Some(labels) = state
        .get_mut("electron-workspace-root-labels")
        .and_then(Value::as_object_mut)
    {
        updated_roots += normalize_object_keys(labels);
    }
    if let Some(per_path) = state
        .get_mut("open-in-target-preferences")
        .and_then(|value| value.get_mut("perPath"))
        .and_then(Value::as_object_mut)
    {
        updated_roots += normalize_object_keys(per_path);
    }

    if updated_roots > 0 {
        let bytes =
            serde_json::to_vec_pretty(&state).map_err(|e| AppError::JsonSerialize { source: e })?;
        let mut output = bytes;
        output.push(b'\n');
        atomic_write(&path, &output)?;
        atomic_write(&codex_dir.join(GLOBAL_STATE_BACKUP_FILENAME), &output)?;
    }
    Ok(WorkspaceSyncResult { updated_roots })
}

fn normalize_path_value(value: Option<&mut Value>) -> usize {
    let Some(value) = value else {
        return 0;
    };
    match value {
        Value::String(raw) => {
            let next = to_desktop_workspace_path(raw);
            if next != *raw {
                *raw = next;
                1
            } else {
                0
            }
        }
        Value::Array(items) => {
            let mut changed = 0;
            let mut seen = HashSet::new();
            let mut next_items = Vec::new();
            for item in items.iter() {
                let Some(raw) = item.as_str() else {
                    continue;
                };
                let next = to_desktop_workspace_path(raw);
                let key = comparable_path_key(&next);
                if seen.insert(key) {
                    if next != raw {
                        changed += 1;
                    }
                    next_items.push(Value::String(next));
                } else {
                    changed += 1;
                }
            }
            if changed > 0 {
                *items = next_items;
            }
            changed
        }
        _ => 0,
    }
}

fn normalize_object_keys(map: &mut Map<String, Value>) -> usize {
    let mut changed = 0usize;
    let mut next = Map::new();
    for (key, value) in std::mem::take(map) {
        let normalized = to_desktop_workspace_path(&key);
        if normalized != key {
            changed += 1;
        }
        next.entry(normalized).or_insert(value);
    }
    *map = next;
    changed
}

fn path_values(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => vec![value.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn to_desktop_workspace_path(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix("\\\\?\\UNC\\") {
        return format!("\\\\{}", rest).replace('/', "\\");
    }
    if let Some(rest) = trimmed.strip_prefix("\\\\?\\") {
        return rest.replace('/', "\\");
    }
    value.replace('/', "\\")
}

fn comparable_path_key(value: &str) -> String {
    let mut key = to_desktop_workspace_path(value);
    while key.len() > 1 && (key.ends_with('\\') || key.ends_with('/')) {
        key.pop();
    }
    #[cfg(windows)]
    {
        key.make_ascii_lowercase();
    }
    key
}

fn collect_rollout_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollout_files(&path, files);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            files.push(path);
        }
    }
}

fn increment_count(
    counts: &mut BTreeMap<String, BTreeMap<String, usize>>,
    scope: &str,
    provider: &str,
) {
    *counts
        .entry(scope.to_string())
        .or_default()
        .entry(provider.to_string())
        .or_default() += 1;
}

fn create_backup_dir() -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir()
        .join("backups")
        .join(BACKUP_NAME)
        .join(Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string());
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
    Ok(backup_dir)
}

fn backup_static_files(
    codex_dir: &Path,
    config_text: &str,
    backup_dir: &Path,
) -> Result<(), AppError> {
    let static_dir = backup_dir.join("static");
    fs::create_dir_all(&static_dir).map_err(|e| AppError::io(&static_dir, e))?;
    for file_name in [
        "config.toml",
        SESSION_INDEX_FILENAME,
        GLOBAL_STATE_FILENAME,
        GLOBAL_STATE_BACKUP_FILENAME,
    ] {
        let source = codex_dir.join(file_name);
        if source.exists() {
            copy_file(&source, &static_dir.join(file_name))?;
        }
    }
    let db_dir = backup_dir.join("state");
    fs::create_dir_all(&db_dir).map_err(|e| AppError::io(&db_dir, e))?;
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if db_path.exists() {
            let name = db_path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .map(|parent| format!("{parent}-state_5.sqlite"))
                .unwrap_or_else(|| "state_5.sqlite".to_string());
            copy_file(&db_path, &db_dir.join(name))?;
            for suffix in ["-wal", "-shm"] {
                let sidecar = PathBuf::from(format!("{}{}", db_path.display(), suffix));
                if sidecar.exists() {
                    let sidecar_name = db_dir.join(format!(
                        "{}{}",
                        db_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("state_5.sqlite"),
                        suffix
                    ));
                    copy_file(&sidecar, &sidecar_name)?;
                }
            }
        }
    }
    let meta = json!({
        "version": 1,
        "namespace": BACKUP_NAME,
        "codexDir": codex_dir.to_string_lossy(),
        "createdAt": Utc::now().to_rfc3339(),
    });
    let bytes = serde_json::to_vec_pretty(&meta).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&backup_dir.join("metadata.json"), &bytes)
}

fn write_session_manifest(backup_dir: &Path, changes: &[SessionChange]) -> Result<(), AppError> {
    let entries: Vec<Value> = changes
        .iter()
        .map(|change| {
            json!({
                "path": change.path.to_string_lossy(),
                "directory": change.directory,
                "threadId": change.thread_id,
                "originalFirstLine": change.original_first_line,
                "separator": change.separator,
            })
        })
        .collect();
    let bytes = serde_json::to_vec_pretty(&json!({
        "version": 1,
        "files": entries,
    }))
    .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&backup_dir.join("rollout-first-line-manifest.json"), &bytes)
}
