use crate::codex_state_db::codex_state_db_paths;
use crate::config::atomic_write;
use crate::error::AppError;
use filetime::{set_file_times, FileTime};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::ops::Range;
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReconcileOutcome {
    pub changed_jsonl_files: usize,
    pub changed_state_rows: usize,
}

#[derive(Debug)]
struct BytePatch {
    range: Range<usize>,
    original: Vec<u8>,
    replacement: Vec<u8>,
}

#[derive(Debug)]
enum JsonlWrite {
    InPlace(Vec<BytePatch>),
    Replace(Vec<u8>),
}

#[derive(Debug)]
struct RouteAppend {
    target: Vec<u8>,
    rollback_route: ThreadRouteSnapshot,
}

#[derive(Debug, Clone)]
struct ThreadRouteSnapshot {
    provider: Option<Value>,
    model: Option<Value>,
}

#[derive(Debug)]
struct JsonlChange {
    path: PathBuf,
    original: Vec<u8>,
    target: HistoryTarget,
    write: Option<JsonlWrite>,
    route_append: Option<RouteAppend>,
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

pub(crate) fn reconcile_history_for_live_config() -> Result<AppliedHistoryReconcile, AppError> {
    let operation_guard = lock_history_operation();
    let codex_dir = crate::codex_config::get_codex_config_dir();
    let config_text = crate::codex_config::read_codex_config_text()?;
    let target = history_target_for_live_config(&config_text)?;
    let mut applied = reconcile_history_transaction_at(&codex_dir, &config_text, &target)?;
    applied._operation_guard = Some(operation_guard);
    Ok(applied)
}

fn history_target_for_live_config(config_text: &str) -> Result<HistoryTarget, AppError> {
    let document = config_text
        .parse::<DocumentMut>()
        .map_err(|error| AppError::Config(format!("Invalid Codex config.toml: {error}")))?;
    let provider = document
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("openai")
        .to_string();
    let model = document
        .get("model")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    Ok(HistoryTarget { provider, model })
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
        for change in &mut jsonl_changes {
            // Include the current file in rollback even when the write reports
            // an error after partially or fully reaching disk.
            applied_jsonl += 1;
            apply_jsonl_change(change)?;
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
    let original = fs::read(path).map_err(|error| AppError::io(path, error))?;
    let text = std::str::from_utf8(&original).map_err(|error| {
        AppError::Message(format!(
            "Invalid UTF-8 in Codex session file {}: {error}",
            path.display()
        ))
    })?;
    let mut patches = Vec::new();
    let mut latest_thread_settings = None;
    let mut offset = 0;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if let Some(mut patch) = session_meta_provider_patch(line, target)? {
            patch.range.start += offset;
            patch.range.end += offset;
            patches.push(patch);
        }
        if is_thread_settings_line(line) {
            latest_thread_settings = Some(line);
        }
        offset += segment.len();
    }

    let route_append = latest_thread_settings
        .map(|line| prepare_route_append(line, text.ends_with('\n'), target))
        .transpose()?
        .flatten();
    if patches.is_empty() && route_append.is_none() {
        return Ok(None);
    }

    let same_size = patches
        .iter()
        .all(|patch| patch.original.len() == patch.replacement.len());
    let write = if patches.is_empty() {
        None
    } else if same_size {
        Some(JsonlWrite::InPlace(patches))
    } else {
        Some(JsonlWrite::Replace(
            rewrite_session_meta_provider(text, target)?.into_bytes(),
        ))
    };
    Ok(Some(JsonlChange {
        path: path.to_path_buf(),
        original,
        target: target.clone(),
        write,
        route_append,
        accessed: metadata.accessed().ok(),
        modified: metadata.modified().ok(),
    }))
}

fn session_meta_provider_patch(
    line: &str,
    target: &HistoryTarget,
) -> Result<Option<BytePatch>, AppError> {
    if !line.contains("model_provider") {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }
    let field = "model_provider";
    let current = value
        .pointer("/payload/model_provider")
        .and_then(Value::as_str);
    let Some(current) = current else {
        return Ok(None);
    };
    if current == target.provider {
        return Ok(None);
    }
    let Some(range) = json_string_field_range(line, field, current) else {
        return Err(AppError::Message(format!(
            "Failed to locate {field} in a parsed Codex history line"
        )));
    };
    let replacement = serde_json::to_vec(&target.provider)
        .map_err(|source| AppError::JsonSerialize { source })?;
    Ok(Some(BytePatch {
        original: line.as_bytes()[range.clone()].to_vec(),
        range,
        replacement,
    }))
}

fn is_thread_settings_line(line: &str) -> bool {
    line.contains("thread_settings_applied")
        && serde_json::from_str::<Value>(line)
            .ok()
            .is_some_and(|value| {
                value.get("type").and_then(Value::as_str) == Some("event_msg")
                    && value.pointer("/payload/type").and_then(Value::as_str)
                        == Some("thread_settings_applied")
            })
}

fn prepare_route_append(
    line: &str,
    file_ends_with_newline: bool,
    target: &HistoryTarget,
) -> Result<Option<RouteAppend>, AppError> {
    let Some(rewritten) = rewrite_thread_settings_line(line, target)? else {
        return Ok(None);
    };
    let prefix = if file_ends_with_newline { "" } else { "\n" };
    let rollback_route = thread_route_snapshot(line).ok_or_else(|| {
        AppError::Message("Failed to snapshot Codex thread settings route".to_string())
    })?;
    Ok(Some(RouteAppend {
        target: format!("{prefix}{rewritten}\n").into_bytes(),
        rollback_route,
    }))
}

fn json_string_field_range(line: &str, field: &str, expected: &str) -> Option<Range<usize>> {
    let key = serde_json::to_string(field).ok()?;
    let expected = serde_json::to_string(expected).ok()?;
    let mut search_from = 0;
    while let Some(relative) = line[search_from..].find(&key) {
        let key_start = search_from + relative;
        let mut value_start = key_start + key.len();
        let bytes = line.as_bytes();
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        if bytes.get(value_start) != Some(&b':') {
            search_from = key_start + key.len();
            continue;
        }
        value_start += 1;
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        if line[value_start..].starts_with(&expected) {
            return Some(value_start..value_start + expected.len());
        }
        search_from = key_start + key.len();
    }
    None
}

fn rewrite_session_meta_provider(text: &str, target: &HistoryTarget) -> Result<String, AppError> {
    let mut rewritten = String::with_capacity(text.len());
    for segment in text.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        rewritten.push_str(
            &rewrite_session_meta_line(line, target)?.unwrap_or_else(|| line.to_string()),
        );
        rewritten.push_str(newline);
    }
    Ok(rewritten)
}

fn rewrite_session_meta_line(
    line: &str,
    target: &HistoryTarget,
) -> Result<Option<String>, AppError> {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }
    let changed = value
        .get_mut("payload")
        .and_then(Value::as_object_mut)
        .and_then(|payload| payload.get_mut("model_provider"))
        .is_some_and(|provider| {
            if provider.as_str() == Some(target.provider.as_str()) {
                false
            } else {
                *provider = Value::String(target.provider.clone());
                true
            }
        });
    if !changed {
        return Ok(None);
    }
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|source| AppError::JsonSerialize { source })
}

fn rewrite_thread_settings_line(
    line: &str,
    target: &HistoryTarget,
) -> Result<Option<String>, AppError> {
    rewrite_thread_settings_line_with_route(
        line,
        &ThreadRouteSnapshot {
            provider: Some(Value::String(target.provider.clone())),
            model: target.model.clone().map(Value::String),
        },
    )
}

fn thread_route_snapshot(line: &str) -> Option<ThreadRouteSnapshot> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("event_msg")
        || value.pointer("/payload/type").and_then(Value::as_str) != Some("thread_settings_applied")
    {
        return None;
    }
    let settings = value
        .pointer("/payload/thread_settings")
        .and_then(Value::as_object)?;
    Some(ThreadRouteSnapshot {
        provider: settings.get("model_provider_id").cloned(),
        model: settings.get("model").cloned(),
    })
}

fn rewrite_thread_settings_line_with_route(
    line: &str,
    route: &ThreadRouteSnapshot,
) -> Result<Option<String>, AppError> {
    let Ok(mut value) = serde_json::from_str::<Value>(line) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("event_msg")
        || value.pointer("/payload/type").and_then(Value::as_str) != Some("thread_settings_applied")
    {
        return Ok(None);
    }
    let Some(settings) = value
        .pointer_mut("/payload/thread_settings")
        .and_then(Value::as_object_mut)
    else {
        return Ok(None);
    };
    let mut changed = false;
    if settings.get("model_provider_id") != route.provider.as_ref() {
        match &route.provider {
            Some(provider) => {
                settings.insert("model_provider_id".to_string(), provider.clone());
            }
            None => {
                settings.remove("model_provider_id");
            }
        }
        changed = true;
    }
    if settings.get("model") != route.model.as_ref() {
        match &route.model {
            Some(model) => {
                settings.insert("model".to_string(), model.clone());
            }
            None => {
                settings.remove("model");
            }
        }
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|source| AppError::JsonSerialize { source })
}

fn apply_jsonl_change(change: &mut JsonlChange) -> Result<(), AppError> {
    if let Some(write) = &change.write {
        match write {
            JsonlWrite::InPlace(patches) => apply_jsonl_patches(change, patches)?,
            JsonlWrite::Replace(rewritten) => {
                ensure_variable_replace_is_safe(&change.path)?;
                ensure_file_unchanged(&change.path, change.modified, &change.original)?;
                atomic_write(&change.path, rewritten).map_err(|error| {
                    AppError::Message(format!(
                        "Cannot safely replace active Codex session {}; close Codex and retry: {error}",
                        change.path.display()
                    ))
                })?;
                restore_file_times(&change.path, change.accessed, change.modified)?;
            }
        }
    }
    let target = change.target.clone();
    if let Some(route_append) = &mut change.route_append {
        let current = fs::read(&change.path).map_err(|error| AppError::io(&change.path, error))?;
        if let Ok(text) = std::str::from_utf8(&current) {
            if let Some(line) = latest_thread_settings_line(text) {
                if !latest_thread_settings_matches(&current, &target) {
                    if let Some(route) = thread_route_snapshot(line) {
                        route_append.rollback_route = route;
                    }
                }
                route_append.target = match rewrite_thread_settings_line(line, &target)? {
                    Some(rewritten) => {
                        let prefix = if text.ends_with('\n') { "" } else { "\n" };
                        format!("{prefix}{rewritten}\n").into_bytes()
                    }
                    None => Vec::new(),
                };
            }
        }
        if !route_append.target.is_empty() {
            append_jsonl_bytes(&change.path, &route_append.target)?;
        }
        let base_len = match &change.write {
            Some(JsonlWrite::Replace(rewritten)) => rewritten.len(),
            _ => change.original.len(),
        };
        let expected_len = base_len.saturating_add(route_append.target.len()) as u64;
        if fs::metadata(&change.path)
            .map_err(|error| AppError::io(&change.path, error))?
            .len()
            == expected_len
        {
            restore_file_times(&change.path, change.accessed, change.modified)?;
        }
    }
    Ok(())
}

fn latest_thread_settings_line(text: &str) -> Option<&str> {
    text.lines().rfind(|line| is_thread_settings_line(line))
}

fn append_jsonl_bytes(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| AppError::io(path, error))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_data())
        .map_err(|error| AppError::io(path, error))
}

fn ensure_variable_replace_is_safe(_path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        if _path.starts_with(crate::codex_config::get_codex_config_dir())
            && std::env::var_os("CC_SWITCH_TEST_HOME").is_none()
            && codex_process_is_running()?
        {
            return Err(AppError::Message(format!(
                "Cannot safely replace active Codex session {}; close Codex and retry",
                _path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn codex_process_is_running() -> Result<bool, AppError> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "comm="])
        .output()
        .map_err(|error| {
            AppError::Message(format!(
                "Cannot verify whether Codex is running before session replacement: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(AppError::Message(
            "Cannot verify whether Codex is running before session replacement".to_string(),
        ));
    }
    Ok(process_list_contains_codex(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(unix)]
fn process_list_contains_codex(processes: &str) -> bool {
    processes.lines().any(|process| {
        let name = Path::new(process.trim())
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        name == "codex" || name == "codex.exe"
    })
}

fn apply_jsonl_patches(change: &JsonlChange, patches: &[BytePatch]) -> Result<(), AppError> {
    let current = fs::read(&change.path).map_err(|error| AppError::io(&change.path, error))?;
    for patch in patches {
        if current.get(patch.range.clone()) != Some(patch.original.as_slice()) {
            return Err(AppError::Message(format!(
                "Codex session changed during provider reconciliation: {}",
                change.path.display()
            )));
        }
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&change.path)
        .map_err(|error| AppError::io(&change.path, error))?;
    for patch in patches {
        file.seek(SeekFrom::Start(patch.range.start as u64))
            .and_then(|_| file.write_all(&patch.replacement))
            .map_err(|error| AppError::io(&change.path, error))?;
    }
    file.sync_data()
        .map_err(|error| AppError::io(&change.path, error))?;
    let len_after = file
        .metadata()
        .map_err(|error| AppError::io(&change.path, error))?
        .len();
    drop(file);
    if len_after == change.original.len() as u64 {
        restore_file_times(&change.path, change.accessed, change.modified)?;
    }
    Ok(())
}

fn ensure_file_unchanged(
    path: &Path,
    modified: Option<SystemTime>,
    expected: &[u8],
) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.modified().ok() != modified || metadata.len() != expected.len() as u64 {
        return Err(AppError::Message(format!(
            "Codex session file changed during provider reconciliation: {}",
            path.display()
        )));
    }
    if fs::read(path).map_err(|error| AppError::io(path, error))? != expected {
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
        let changed = if change.has_model {
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
    let (sql, include_model) = if has_model {
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
    match &change.write {
        Some(JsonlWrite::InPlace(patches)) => {
            let current =
                fs::read(&change.path).map_err(|error| AppError::io(&change.path, error))?;
            let mut rollback_patches = Vec::new();
            for patch in patches {
                match current.get(patch.range.clone()) {
                    Some(bytes) if bytes == patch.original.as_slice() => {}
                    Some(bytes) if bytes == patch.replacement.as_slice() => {
                        rollback_patches.push(BytePatch {
                            range: patch.range.clone(),
                            original: patch.replacement.clone(),
                            replacement: patch.original.clone(),
                        });
                    }
                    _ => {
                        return Err(AppError::Message(format!(
                            "Codex session changed before rollback: {}",
                            change.path.display()
                        )));
                    }
                }
            }
            if !rollback_patches.is_empty() {
                apply_jsonl_patches(change, &rollback_patches)?;
            }
            rollback_route_append_if_needed(change)
        }
        Some(JsonlWrite::Replace(rewritten)) => {
            let target_with_append = change.route_append.as_ref().map(|append| {
                let mut bytes = rewritten.clone();
                bytes.extend_from_slice(&append.target);
                bytes
            });
            match fs::read(&change.path) {
                Ok(current) if current == change.original => return Ok(()),
                Ok(current)
                    if current != *rewritten
                        && target_with_append
                            .as_ref()
                            .is_none_or(|expected| current != *expected) =>
                {
                    return Err(AppError::Message(format!(
                        "Codex session changed before rollback: {}",
                        change.path.display()
                    )));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(AppError::io(&change.path, error)),
            }
            atomic_write(&change.path, &change.original)?;
            restore_file_times(&change.path, change.accessed, change.modified)
        }
        None => rollback_route_append_if_needed(change),
    }
}

fn rollback_route_append_if_needed(change: &JsonlChange) -> Result<(), AppError> {
    let Some(route_append) = &change.route_append else {
        return Ok(());
    };
    let current = fs::read(&change.path).map_err(|error| AppError::io(&change.path, error))?;
    if !latest_thread_settings_matches(&current, &change.target) {
        return Ok(());
    }
    let text = std::str::from_utf8(&current).map_err(|error| {
        AppError::Message(format!(
            "Invalid UTF-8 in Codex session file {} during rollback: {error}",
            change.path.display()
        ))
    })?;
    let Some(line) = latest_thread_settings_line(text) else {
        return Ok(());
    };
    let Some(rewritten) =
        rewrite_thread_settings_line_with_route(line, &route_append.rollback_route)?
    else {
        return Ok(());
    };
    let prefix = if text.ends_with('\n') { "" } else { "\n" };
    append_jsonl_bytes(&change.path, format!("{prefix}{rewritten}\n").as_bytes())
}

fn latest_thread_settings_matches(content: &[u8], target: &HistoryTarget) -> bool {
    let Ok(text) = std::str::from_utf8(content) else {
        return false;
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .rfind(|value| {
            value.get("type").and_then(Value::as_str) == Some("event_msg")
                && value.pointer("/payload/type").and_then(Value::as_str)
                    == Some("thread_settings_applied")
        })
        .is_some_and(|value| {
            let settings = value.pointer("/payload/thread_settings");
            settings
                .and_then(|settings| settings.get("model_provider_id"))
                .and_then(Value::as_str)
                == Some(target.provider.as_str())
                && settings
                    .and_then(|settings| settings.get("model"))
                    .and_then(Value::as_str)
                    == target.model.as_deref()
        })
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
        let restored = if change.has_model {
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
    fn reconcile_updates_jsonl_in_place_without_losing_open_handle_appends() {
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
        let mut append_handle = fs::OpenOptions::new()
            .append(true)
            .open(&active)
            .expect("open active session for append");

        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let outcome = reconcile_history_at(temp.path(), "", &target).expect("reconcile history");

        assert_eq!(outcome.changed_jsonl_files, 2);
        assert_eq!(outcome.changed_state_rows, 0);
        append_handle
            .write_all(b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"after-switch\"}}\n")
            .expect("append after reconciliation");
        append_handle.flush().expect("flush append");
        for path in [&active, &archived] {
            let content = fs::read_to_string(path).expect("read reconciled session");
            assert!(content.contains("\"model_provider\":\"custom\""));
            assert!(content.contains("\"model_provider_id\":\"custom\""));
            assert!(content.contains("\"message\":\"hello\""));
        }
        assert!(
            fs::read_to_string(&active)
                .expect("read appended active session")
                .contains("after-switch"),
            "an already-open Codex rollout handle must keep appending to the visible file"
        );
        assert_ne!(
            fs::metadata(&active).unwrap().modified().unwrap(),
            active_mtime,
            "a post-switch append owns the active session mtime"
        );
        assert_eq!(
            fs::metadata(&archived).unwrap().modified().unwrap(),
            archived_mtime
        );

        let second = reconcile_history_at(temp.path(), "", &target).expect("reconcile again");
        assert_eq!(second, ReconcileOutcome::default());
    }

    #[cfg(windows)]
    #[test]
    fn reconcile_refuses_variable_length_replacement_while_rollout_is_open() {
        let temp = tempdir().expect("tempdir");
        let active = temp.path().join("sessions/2026/07/active.jsonl");
        write_session(&active);
        let original = fs::read(&active).expect("read original session");
        let mut append_handle = fs::OpenOptions::new()
            .append(true)
            .open(&active)
            .expect("open active session for append");

        let target = HistoryTarget::new("provider-long", Some("gpt-5.6-sol"));
        let error = reconcile_history_at(temp.path(), "", &target)
            .expect_err("an open rollout requiring replacement must abort");

        assert!(error.to_string().contains("close Codex"));
        append_handle
            .write_all(b"{\"type\":\"event_msg\",\"payload\":{\"message\":\"still-visible\"}}\n")
            .expect("append after rejected reconciliation");
        append_handle.flush().expect("flush append");
        let visible = fs::read(&active).expect("read visible session");
        assert!(visible.starts_with(&original));
        assert!(String::from_utf8_lossy(&visible).contains("still-visible"));
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
    fn reconcile_clears_nullable_state_model_without_target_override() {
        let temp = tempdir().expect("tempdir");
        let state_db = temp.path().join("state_5.sqlite");
        let connection = Connection::open(&state_db).expect("open state DB");
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    model_provider TEXT NOT NULL,
                    model TEXT
                );
                INSERT INTO threads (id, model_provider, model)
                VALUES ('thread-1', 'custom', 'deepseek-reasoner');",
            )
            .expect("seed nullable state model");
        drop(connection);

        let target = HistoryTarget::new("openai", None);
        reconcile_history_at(temp.path(), "", &target).expect("reconcile official history");

        let connection = Connection::open(&state_db).expect("reopen state DB");
        let route: (String, Option<String>) = connection
            .query_row(
                "SELECT model_provider, model FROM threads WHERE id = 'thread-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read reconciled route");
        assert_eq!(route, ("openai".to_string(), None));
    }

    #[test]
    fn reconcile_clears_jsonl_model_without_target_override() {
        let temp = tempdir().expect("tempdir");
        let session = temp.path().join("sessions/active.jsonl");
        write_session(&session);

        let target = HistoryTarget::new("custom", None);
        reconcile_history_at(temp.path(), "", &target).expect("reconcile history");

        let latest = fs::read_to_string(&session)
            .expect("read session")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .rfind(|value| {
                value.pointer("/payload/type").and_then(Value::as_str)
                    == Some("thread_settings_applied")
            })
            .expect("latest thread settings");
        assert_eq!(
            latest
                .pointer("/payload/thread_settings/model_provider_id")
                .and_then(Value::as_str),
            Some("custom")
        );
        assert!(
            latest.pointer("/payload/thread_settings/model").is_none(),
            "an absent live model must clear the stale rollout model"
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

    #[test]
    fn rollback_tolerates_a_journaled_jsonl_change_that_was_not_applied() {
        let temp = tempdir().expect("tempdir");
        let session = temp.path().join("sessions/active.jsonl");
        write_session(&session);
        let original = fs::read(&session).expect("read original session");
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let change = prepare_jsonl_change(&session, &target)
            .expect("prepare session change")
            .expect("session needs reconciliation");

        rollback_jsonl_change(&change).expect("unapplied journal rollback is a no-op");

        assert_eq!(
            fs::read(session).expect("read session after rollback"),
            original
        );
    }

    #[test]
    fn rollback_appends_original_route_after_concurrent_target_event() {
        let temp = tempdir().expect("tempdir");
        let session = temp.path().join("sessions/active.jsonl");
        write_session(&session);
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let applied = reconcile_history_transaction_at(temp.path(), "", &target)
            .expect("apply reconciliation");

        let concurrent = r#"{"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-5.6-sol","model_provider_id":"custom","service_tier":"priority"}}}"#;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&session)
            .expect("open session for concurrent append");
        writeln!(file, "{concurrent}").expect("append concurrent target event");
        file.flush().expect("flush concurrent event");

        applied.rollback().expect("rollback reconciliation");

        let latest = fs::read_to_string(&session)
            .expect("read rolled back session")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .rfind(|value| {
                value.pointer("/payload/type").and_then(Value::as_str)
                    == Some("thread_settings_applied")
            })
            .expect("latest thread settings");
        assert_eq!(
            latest
                .pointer("/payload/thread_settings/model_provider_id")
                .and_then(Value::as_str),
            Some("openai")
        );
        assert_eq!(
            latest
                .pointer("/payload/thread_settings/model")
                .and_then(Value::as_str),
            Some("gpt-5.4")
        );
        assert_eq!(
            latest
                .pointer("/payload/thread_settings/service_tier")
                .and_then(Value::as_str),
            Some("priority"),
            "rollback must preserve concurrent non-route settings"
        );
    }

    #[test]
    fn rollback_restores_the_route_seen_immediately_before_apply() {
        let temp = tempdir().expect("tempdir");
        let session = temp.path().join("sessions/active.jsonl");
        write_session(&session);
        let target = HistoryTarget::new("custom", Some("gpt-5.6-sol"));
        let mut change = prepare_jsonl_change(&session, &target)
            .expect("prepare change")
            .expect("session needs reconciliation");
        let concurrent = r#"{"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-5.4-mini","model_provider_id":"openai"}}}"#;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&session)
            .expect("open session");
        writeln!(file, "{concurrent}").expect("append newer original route");
        file.flush().expect("flush newer route");

        apply_jsonl_change(&mut change).expect("apply reconciliation");
        rollback_jsonl_change(&change).expect("rollback reconciliation");

        let latest = fs::read_to_string(&session)
            .expect("read rolled back session")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .rfind(|value| {
                value.pointer("/payload/type").and_then(Value::as_str)
                    == Some("thread_settings_applied")
            })
            .expect("latest thread settings");
        assert_eq!(
            latest
                .pointer("/payload/thread_settings/model_provider_id")
                .and_then(Value::as_str),
            Some("openai")
        );
        assert_eq!(
            latest
                .pointer("/payload/thread_settings/model")
                .and_then(Value::as_str),
            Some("gpt-5.4-mini")
        );
    }
}
