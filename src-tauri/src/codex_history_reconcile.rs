use crate::error::AppError;
use crate::provider::Provider;
use crate::{codex_state_db::codex_state_db_paths, config::atomic_write};
use filetime::{set_file_times, FileTime};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use toml_edit::DocumentMut;

static CODEX_HISTORY_OP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn lock_history_operation() -> std::sync::MutexGuard<'static, ()> {
    CODEX_HISTORY_OP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryTarget {
    pub provider: String,
    pub model: Option<String>,
}

#[cfg(test)]
impl HistoryTarget {
    pub(crate) fn new(provider: &str, model: Option<&str>) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.map(str::to_string),
        }
    }
}

fn rewrite_history_line(line: &str, target: &HistoryTarget) -> Result<Option<String>, AppError> {
    if !line.contains("model_provider") {
        return Ok(None);
    }
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return Ok(None);
    };

    let changed = match value.get("type").and_then(Value::as_str) {
        Some("session_meta") => rewrite_session_meta(&mut value, target),
        Some("event_msg") => rewrite_thread_settings(&mut value, target),
        _ => false,
    };
    if !changed {
        return Ok(None);
    }
    let line =
        serde_json::to_string(&value).map_err(|source| AppError::JsonSerialize { source })?;
    Ok(Some(line))
}

fn rewrite_session_meta(value: &mut Value, target: &HistoryTarget) -> bool {
    let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(provider) = payload.get("model_provider").and_then(Value::as_str) else {
        return false;
    };
    if provider == target.provider {
        return false;
    }
    payload.insert(
        "model_provider".to_string(),
        Value::String(target.provider.clone()),
    );
    true
}

fn rewrite_thread_settings(value: &mut Value, target: &HistoryTarget) -> bool {
    let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
        return false;
    };
    if payload.get("type").and_then(Value::as_str) != Some("thread_settings_applied") {
        return false;
    }
    let Some(settings) = payload
        .get_mut("thread_settings")
        .and_then(Value::as_object_mut)
    else {
        return false;
    };
    let Some(provider) = settings.get("model_provider_id").and_then(Value::as_str) else {
        return false;
    };
    let provider_changed = provider != target.provider;
    let model_changed = target.model.as_ref().is_some_and(|target_model| {
        settings.get("model").and_then(Value::as_str) != Some(target_model.as_str())
    });
    if !provider_changed && !model_changed {
        return false;
    }

    settings.insert(
        "model_provider_id".to_string(),
        Value::String(target.provider.clone()),
    );
    if let Some(target_model) = &target.model {
        settings.insert("model".to_string(), Value::String(target_model.clone()));
    }
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReconcileOutcome {
    pub changed_jsonl_files: usize,
    pub changed_state_rows: usize,
}

#[derive(Debug)]
struct JsonlChange {
    path: PathBuf,
    original: String,
    rewritten: String,
    accessed: Option<SystemTime>,
    modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
struct StateRoute {
    id: String,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug)]
struct StateDbChange {
    path: PathBuf,
    has_model: bool,
    routes: Vec<StateRoute>,
}

#[derive(Debug)]
pub(crate) struct AppliedHistoryReconcile {
    outcome: ReconcileOutcome,
    target: HistoryTarget,
    jsonl_changes: Vec<JsonlChange>,
    state_db_changes: Vec<StateDbChange>,
    _operation_guard: Option<std::sync::MutexGuard<'static, ()>>,
}

impl AppliedHistoryReconcile {
    pub(crate) fn outcome(&self) -> &ReconcileOutcome {
        &self.outcome
    }

    pub(crate) fn rollback(self) -> Result<(), AppError> {
        rollback_changes(&self.jsonl_changes, &self.state_db_changes, &self.target)
    }
}

pub(crate) fn reconcile_history_for_provider(
    provider: &Provider,
) -> Result<AppliedHistoryReconcile, AppError> {
    let operation_guard = lock_history_operation();
    let target = history_target_for_provider(provider)?;
    let codex_dir = crate::codex_config::get_codex_config_dir();
    let config_text = crate::codex_config::read_codex_config_text()?;
    let mut applied = reconcile_history_transaction_at(&codex_dir, &config_text, &target)?;
    applied._operation_guard = Some(operation_guard);
    Ok(applied)
}

pub(crate) fn reconcile_history_for_current_provider(
    database: &crate::database::Database,
) -> Result<Option<ReconcileOutcome>, AppError> {
    let Some(current_id) = crate::settings::get_effective_current_provider(
        database,
        &crate::app_config::AppType::Codex,
    )?
    else {
        return Ok(None);
    };
    let providers = database.get_all_providers(crate::app_config::AppType::Codex.as_str())?;
    let provider = providers.get(&current_id).ok_or_else(|| {
        AppError::Config(format!(
            "Current Codex provider '{current_id}' does not exist"
        ))
    })?;
    let applied = reconcile_history_for_provider(provider)?;
    Ok(Some(applied.outcome().clone()))
}

fn history_target_for_provider(provider: &Provider) -> Result<HistoryTarget, AppError> {
    let config_text = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or("");
    let document = config_text.parse::<DocumentMut>().map_err(|error| {
        AppError::Config(format!("Invalid Codex provider config.toml: {error}"))
    })?;
    let model = document
        .get("model")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let provider_id = if provider.category.as_deref() == Some("official") {
        "openai".to_string()
    } else {
        document
            .get("model_provider")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                AppError::Config(
                    "Third-party Codex provider config must define model_provider".to_string(),
                )
            })?
    };
    Ok(HistoryTarget {
        provider: provider_id,
        model,
    })
}

#[cfg(test)]
fn reconcile_history_at(
    codex_dir: &Path,
    config_text: &str,
    target: &HistoryTarget,
) -> Result<ReconcileOutcome, AppError> {
    Ok(reconcile_history_transaction_at(codex_dir, config_text, target)?.outcome)
}

fn reconcile_history_transaction_at(
    codex_dir: &Path,
    config_text: &str,
    target: &HistoryTarget,
) -> Result<AppliedHistoryReconcile, AppError> {
    let mut jsonl_files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut jsonl_files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut jsonl_files, 0, 4);
    jsonl_files.sort();

    // Prepare and validate every artifact before the first write. This prevents
    // a malformed later database from leaving earlier session files migrated.
    let mut jsonl_changes = Vec::new();
    for path in jsonl_files {
        if let Some(change) = prepare_jsonl_change(&path, target)? {
            jsonl_changes.push(change);
        }
    }
    let mut state_db_changes = Vec::new();
    for path in codex_state_db_paths(codex_dir, config_text) {
        if let Some(change) = prepare_state_db_change(&path, target)? {
            state_db_changes.push(change);
        }
    }

    let mut applied_jsonl = 0;
    let mut applied_state_dbs = 0;
    let apply_result = (|| {
        for change in &jsonl_changes {
            apply_jsonl_change(change)?;
            applied_jsonl += 1;
            restore_file_times(&change.path, change.accessed, change.modified)?;
        }
        for change in &state_db_changes {
            apply_state_db_change(change, target)?;
            applied_state_dbs += 1;
        }
        Ok(())
    })();

    if let Err(error) = apply_result {
        let rollback = rollback_changes(
            &jsonl_changes[..applied_jsonl],
            &state_db_changes[..applied_state_dbs],
            target,
        );
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(AppError::Message(format!(
                "{error}; failed to roll back Codex history reconciliation: {rollback_error}"
            ))),
        };
    }

    Ok(AppliedHistoryReconcile {
        outcome: ReconcileOutcome {
            changed_jsonl_files: jsonl_changes.len(),
            changed_state_rows: state_db_changes
                .iter()
                .map(|change| change.routes.len())
                .sum(),
        },
        target: target.clone(),
        jsonl_changes,
        state_db_changes,
        _operation_guard: None,
    })
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>, depth: usize, max_depth: usize) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth);
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
        {
            files.push(path);
        }
    }
}

fn prepare_jsonl_change(
    path: &Path,
    target: &HistoryTarget,
) -> Result<Option<JsonlChange>, AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    let modified = metadata.modified().ok();
    let accessed = metadata.accessed().ok();
    let original = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    let mut rewritten = String::with_capacity(original.len());
    let mut changed = false;

    for segment in original.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(line_rewrite) = rewrite_history_line(line, target)? {
            rewritten.push_str(&line_rewrite);
            changed = true;
        } else {
            rewritten.push_str(line);
        }
        rewritten.push_str(newline);
    }
    if !changed {
        return Ok(None);
    }

    Ok(Some(JsonlChange {
        path: path.to_path_buf(),
        original,
        rewritten,
        accessed,
        modified,
    }))
}

fn apply_jsonl_change(change: &JsonlChange) -> Result<(), AppError> {
    ensure_file_unchanged(&change.path, change.modified, change.original.as_bytes())?;
    atomic_write(&change.path, change.rewritten.as_bytes())
}

fn ensure_file_unchanged(
    path: &Path,
    modified: Option<SystemTime>,
    expected: &[u8],
) -> Result<(), AppError> {
    let current = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    if current.modified().ok() != modified || current.len() != expected.len() as u64 {
        return Err(AppError::Message(format!(
            "Codex session file changed during provider reconciliation: {}",
            path.display()
        )));
    }
    let current = fs::read(path).map_err(|error| AppError::io(path, error))?;
    if current != expected {
        return Err(AppError::Message(format!(
            "Codex session file changed during provider reconciliation: {}",
            path.display()
        )));
    }
    Ok(())
}

fn restore_file_times(
    path: &Path,
    accessed: Option<SystemTime>,
    modified: Option<SystemTime>,
) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    let accessed = accessed
        .map(FileTime::from_system_time)
        .unwrap_or_else(|| FileTime::from_last_access_time(&metadata));
    let modified = modified
        .map(FileTime::from_system_time)
        .unwrap_or_else(|| FileTime::from_last_modification_time(&metadata));
    set_file_times(path, accessed, modified).map_err(|error| AppError::io(path, error))
}

fn prepare_state_db_change(
    path: &Path,
    target: &HistoryTarget,
) -> Result<Option<StateDbChange>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open(path).map_err(|error| {
        AppError::Database(format!(
            "Failed to open Codex state DB {}: {error}",
            path.display()
        ))
    })?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| {
            AppError::Database(format!("Failed to configure Codex state DB: {error}"))
        })?;
    let columns = state_thread_columns(&connection)?;
    if !columns.contains("id") || !columns.contains("model_provider") {
        return Ok(None);
    }
    let has_model = columns.contains("model");
    let routes = load_changed_state_routes(&connection, has_model, target)?;
    if routes.is_empty() {
        return Ok(None);
    }
    Ok(Some(StateDbChange {
        path: path.to_path_buf(),
        has_model,
        routes,
    }))
}

fn apply_state_db_change(change: &StateDbChange, target: &HistoryTarget) -> Result<(), AppError> {
    let mut connection = open_state_db(&change.path)?;
    let transaction = connection.transaction().map_err(|error| {
        AppError::Database(format!(
            "Failed to begin Codex state DB transaction: {error}"
        ))
    })?;
    for route in &change.routes {
        let changed = if change.has_model && target.model.is_some() {
            transaction.execute(
                "UPDATE threads SET model_provider = ?1, model = ?2
                 WHERE id = ?3 AND model_provider IS ?4 AND model IS ?5",
                params![
                    target.provider,
                    target.model,
                    route.id,
                    route.provider,
                    route.model
                ],
            )
        } else {
            transaction.execute(
                "UPDATE threads SET model_provider = ?1
                 WHERE id = ?2 AND model_provider IS ?3",
                params![target.provider, route.id, route.provider],
            )
        }
        .map_err(|error| {
            AppError::Database(format!("Failed to reconcile Codex state DB: {error}"))
        })?;
        if changed != 1 {
            return Err(AppError::Message(format!(
                "Codex state changed during provider reconciliation: {}",
                change.path.display()
            )));
        }
    }
    transaction.commit().map_err(|error| {
        AppError::Database(format!(
            "Failed to commit Codex state DB transaction: {error}"
        ))
    })?;
    Ok(())
}

fn open_state_db(path: &Path) -> Result<Connection, AppError> {
    let connection = Connection::open(path).map_err(|error| {
        AppError::Database(format!(
            "Failed to open Codex state DB {}: {error}",
            path.display()
        ))
    })?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| {
            AppError::Database(format!("Failed to configure Codex state DB: {error}"))
        })?;
    Ok(connection)
}

fn load_changed_state_routes(
    connection: &Connection,
    has_model: bool,
    target: &HistoryTarget,
) -> Result<Vec<StateRoute>, AppError> {
    let (sql, include_model) = if has_model && target.model.is_some() {
        (
            "SELECT id, model_provider, model FROM threads
             WHERE model_provider IS NOT ?1 OR model IS NOT ?2",
            true,
        )
    } else {
        (
            "SELECT id, model_provider, NULL FROM threads WHERE model_provider IS NOT ?1",
            false,
        )
    };
    let mut statement = connection.prepare(sql).map_err(|error| {
        AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
    })?;
    if include_model {
        let rows = statement
            .query_map(params![target.provider, target.model], |row| {
                Ok(StateRoute {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    model: row.get(2)?,
                })
            })
            .map_err(|error| {
                AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
            })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| {
            AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
        })
    } else {
        let rows = statement
            .query_map(params![target.provider], |row| {
                Ok(StateRoute {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    model: None,
                })
            })
            .map_err(|error| {
                AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
            })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| {
            AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
        })
    }
}

fn rollback_changes(
    jsonl_changes: &[JsonlChange],
    state_db_changes: &[StateDbChange],
    target: &HistoryTarget,
) -> Result<(), AppError> {
    let mut errors = Vec::new();
    for change in state_db_changes.iter().rev() {
        if let Err(error) = rollback_state_db_change(change, target) {
            errors.push(error.to_string());
        }
    }
    for change in jsonl_changes.iter().rev() {
        if let Err(error) = rollback_jsonl_change(change) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(errors.join("; ")))
    }
}

fn rollback_jsonl_change(change: &JsonlChange) -> Result<(), AppError> {
    let current = fs::read(&change.path).map_err(|error| AppError::io(&change.path, error))?;
    if current != change.rewritten.as_bytes() {
        return Err(AppError::Message(format!(
            "Codex session changed before rollback: {}",
            change.path.display()
        )));
    }
    atomic_write(&change.path, change.original.as_bytes())?;
    restore_file_times(&change.path, change.accessed, change.modified)
}

fn rollback_state_db_change(
    change: &StateDbChange,
    target: &HistoryTarget,
) -> Result<(), AppError> {
    let mut connection = open_state_db(&change.path)?;
    let transaction = connection.transaction().map_err(|error| {
        AppError::Database(format!("Failed to begin Codex state DB rollback: {error}"))
    })?;
    for route in &change.routes {
        let restored = if change.has_model && target.model.is_some() {
            transaction.execute(
                "UPDATE threads SET model_provider = ?1, model = ?2
                 WHERE id = ?3 AND model_provider IS ?4 AND model IS ?5",
                params![
                    route.provider,
                    route.model,
                    route.id,
                    target.provider,
                    target.model
                ],
            )
        } else {
            transaction.execute(
                "UPDATE threads SET model_provider = ?1
                 WHERE id = ?2 AND model_provider IS ?3",
                params![route.provider, route.id, target.provider],
            )
        }
        .map_err(|error| {
            AppError::Database(format!("Failed to roll back Codex state DB: {error}"))
        })?;
        if restored != 1 {
            return Err(AppError::Message(format!(
                "Codex state changed before rollback: {}",
                change.path.display()
            )));
        }
    }
    transaction.commit().map_err(|error| {
        AppError::Database(format!("Failed to commit Codex state DB rollback: {error}"))
    })
}

fn state_thread_columns(connection: &Connection) -> Result<BTreeSet<String>, AppError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(threads)")
        .map_err(|error| {
            AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
        })?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| AppError::Database(format!("Failed to inspect Codex state DB: {error}")))?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| {
            AppError::Database(format!("Failed to inspect Codex state DB: {error}"))
        })?;
    Ok(columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::Path;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    const SESSION_META: &str = r#"{"timestamp":"2026-07-27T01:00:00Z","type":"session_meta","payload":{"id":"thread-1","model_provider":"openai","cwd":"D:\\work"}}"#;
    const THREAD_SETTINGS: &str = r#"{"timestamp":"2026-07-27T01:01:00Z","type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-5.4","model_provider_id":"openai","service_tier":"default"}}}"#;

    #[test]
    fn rewrite_session_meta_changes_only_provider() {
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));

        let rewrite = rewrite_history_line(SESSION_META, &target)
            .expect("valid JSONL")
            .expect("route changed");
        let value: serde_json::Value =
            serde_json::from_str(&rewrite).expect("parse rewritten line");

        assert_eq!(value["payload"]["model_provider"], "custom");
        assert_eq!(value["payload"]["cwd"], "D:\\work");
    }

    #[test]
    fn rewrite_thread_settings_changes_provider_and_target_model() {
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));

        let rewrite = rewrite_history_line(THREAD_SETTINGS, &target)
            .expect("valid JSONL")
            .expect("route changed");
        let value: serde_json::Value =
            serde_json::from_str(&rewrite).expect("parse rewritten line");

        assert_eq!(
            value["payload"]["thread_settings"]["model_provider_id"],
            "custom"
        );
        assert_eq!(value["payload"]["thread_settings"]["model"], "gpt-5.6-sol");
    }

    #[test]
    fn rewrite_thread_settings_keeps_model_without_target_override() {
        let target = HistoryTarget::new("custom", None);

        let rewrite = rewrite_history_line(THREAD_SETTINGS, &target)
            .expect("valid JSONL")
            .expect("provider changed");
        let value: serde_json::Value =
            serde_json::from_str(&rewrite).expect("parse rewritten line");

        assert_eq!(value["payload"]["thread_settings"]["model"], "gpt-5.4");
    }

    #[test]
    fn rewrite_history_line_ignores_unknown_and_malformed_lines() {
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let unknown = r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"}}"#;

        assert!(rewrite_history_line(unknown, &target)
            .expect("valid unknown event")
            .is_none());
        assert!(rewrite_history_line("not-json", &target)
            .expect("malformed history is preserved")
            .is_none());
    }

    #[test]
    fn rewrite_history_line_is_idempotent() {
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let first = rewrite_history_line(THREAD_SETTINGS, &target)
            .expect("valid JSONL")
            .expect("first rewrite");

        assert!(rewrite_history_line(&first, &target)
            .expect("rewritten JSONL")
            .is_none());
    }

    fn write_session(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create session parent");
        }
        let unknown = r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"}}"#;
        fs::write(
            path,
            format!("{SESSION_META}\n{unknown}\n{THREAD_SETTINGS}\n"),
        )
        .expect("write session");
    }

    fn create_state_db(path: &Path, thread_id: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create state DB parent");
        }
        let connection = Connection::open(path).expect("open state DB");
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    model_provider TEXT NOT NULL,
                    model TEXT NOT NULL
                );",
            )
            .expect("create threads table");
        connection
            .execute(
                "INSERT INTO threads (id, model_provider, model) VALUES (?1, 'openai', 'gpt-5.4')",
                params![thread_id],
            )
            .expect("insert thread");
    }

    fn state_route(path: &Path, thread_id: &str) -> (String, String) {
        Connection::open(path)
            .expect("open state DB")
            .query_row(
                "SELECT model_provider, model FROM threads WHERE id = ?1",
                params![thread_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read thread route")
    }

    #[test]
    fn reconcile_updates_active_and_archived_jsonl_without_changing_mtime() {
        let temp = tempdir().expect("tempdir");
        let active = temp.path().join("sessions/2026/07/active.jsonl");
        let archived = temp.path().join("archived_sessions/archived.jsonl");
        write_session(&active);
        write_session(&archived);
        let active_mtime = fs::metadata(&active)
            .expect("active metadata")
            .modified()
            .expect("active mtime");
        let archived_mtime = fs::metadata(&archived)
            .expect("archived metadata")
            .modified()
            .expect("archived mtime");
        thread::sleep(Duration::from_millis(25));

        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let outcome = reconcile_history_at(temp.path(), "", &target).expect("reconcile history");

        assert_eq!(outcome.changed_jsonl_files, 2);
        assert_eq!(outcome.changed_state_rows, 0);
        for path in [&active, &archived] {
            let content = fs::read_to_string(path).expect("read rewritten session");
            assert!(content.contains("\"model_provider\":\"custom\""));
            assert!(content.contains("\"model_provider_id\":\"custom\""));
            assert!(content.contains("\"model\":\"gpt-5.6-sol\""));
            assert!(content.contains("\"message\":\"hello\""));
        }
        assert_eq!(
            fs::metadata(&active).unwrap().modified().unwrap(),
            active_mtime
        );
        assert_eq!(
            fs::metadata(&archived).unwrap().modified().unwrap(),
            archived_mtime
        );

        let second = reconcile_history_at(temp.path(), "", &target).expect("reconcile again");
        assert_eq!(second, ReconcileOutcome::default());
    }

    #[test]
    fn reconcile_updates_every_discovered_state_database() {
        let temp = tempdir().expect("tempdir");
        let root_db = temp.path().join("state_4.sqlite");
        let nested_db = temp.path().join("sqlite/state_6.sqlite");
        create_state_db(&root_db, "root-thread");
        create_state_db(&nested_db, "nested-thread");

        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let outcome = reconcile_history_at(temp.path(), "", &target).expect("reconcile history");

        assert_eq!(outcome.changed_jsonl_files, 0);
        assert_eq!(outcome.changed_state_rows, 2);
        assert_eq!(
            state_route(&root_db, "root-thread"),
            ("custom".to_string(), "gpt-5.6-sol".to_string())
        );
        assert_eq!(
            state_route(&nested_db, "nested-thread"),
            ("custom".to_string(), "gpt-5.6-sol".to_string())
        );
    }

    #[test]
    fn reconcile_rolls_back_all_artifacts_when_a_state_database_is_invalid() {
        let temp = tempdir().expect("tempdir");
        let session = temp.path().join("sessions/active.jsonl");
        let valid_db = temp.path().join("state_4.sqlite");
        let invalid_db = temp.path().join("state_5.sqlite");
        write_session(&session);
        create_state_db(&valid_db, "thread-1");
        fs::write(&invalid_db, b"not a sqlite database").expect("write invalid DB");
        let original_session = fs::read(&session).expect("read original session");

        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let error = reconcile_history_at(temp.path(), "", &target)
            .expect_err("invalid state DB must fail reconciliation");

        assert!(error.to_string().contains("Codex state DB"));
        assert_eq!(
            fs::read(&session).expect("read rolled back session"),
            original_session
        );
        assert_eq!(
            state_route(&valid_db, "thread-1"),
            ("openai".to_string(), "gpt-5.4".to_string())
        );
    }
}
