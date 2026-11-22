//! Read Antigravity's exported transcript. The conversation store is written only by
//! [`delete_session`], which removes a CLI conversation the way agy's own picker does.
use super::{
    blocks::{self, assign_turn_ids, ToolSource},
    utils::{for_each_jsonl_value, parse_timestamp_to_ms, truncate_summary, JsonlSpan},
};
use crate::session_manager::model::{
    DiffOp, EventKind, ImageRef, ImageSource, MessageMeta, SessionBlock, ToolKind, ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};
use crate::sync_utils::MutexExt;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, SystemTime},
};

/// Antigravity CLI keeps sessions in `~/.gemini/antigravity-cli/brain`, while the IDE writes the
/// same layout to `~/.gemini/antigravity/brain`. API-key providers run agy with a private
/// `--gemini_dir` (`~/.cc-switch/agy/<provider>/`), so their sessions live under that directory's
/// `antigravity-cli/brain`. All of them are scanned.
pub fn session_roots() -> Vec<PathBuf> {
    session_sources()
        .into_iter()
        .map(|(root, _)| root)
        .collect()
}

/// Each root, with the private `--gemini_dir` its sessions must be resumed with.
fn session_sources() -> Vec<(PathBuf, Option<PathBuf>)> {
    let cli = crate::agy_config::agy_dir();
    let ide = cli.with_file_name("antigravity");
    let mut sources = vec![(cli.join("brain"), None), (ide.join("brain"), None)];
    let mut profiles: Vec<PathBuf> = fs::read_dir(crate::agy_config::key_profiles_root())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .map(|entry| entry.path())
        .collect();
    profiles.sort();
    for dir in profiles {
        sources.push((dir.join("antigravity-cli").join("brain"), Some(dir)));
    }
    sources
}

fn spanned_records(path: &Path) -> Result<Vec<(u64, JsonlSpan, Value)>, String> {
    // The export can rewrite steps as they finish. The latest record for a step wins,
    // along with the byte span of that winning JSONL line for ContentRef::Jsonl resolution.
    let mut steps = BTreeMap::new();
    for_each_jsonl_value(path, |span, value| {
        if let Some(index) = value["step_index"].as_u64() {
            steps.insert(index, (span, value));
        }
        Ok(())
    })
    .map_err(|e| format!("Cannot read Antigravity transcript: {e}"))?;
    Ok(steps
        .into_iter()
        .map(|(index, (span, value))| (index, span, value))
        .collect())
}

pub(crate) fn records(path: &Path) -> Result<Vec<Value>, String> {
    Ok(spanned_records(path)?
        .into_iter()
        .map(|(_, _, value)| value)
        .collect())
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan_roots(&session_sources())
}

/// Every readable `transcript_full.jsonl` across CLI, IDE, and API-key roots, including `/btw`
/// side questions and `/boost` subagent runs that [`scan_sessions`] hides from the session list.
pub(crate) fn transcript_files() -> Vec<(String, PathBuf)> {
    transcript_files_in_roots(&session_sources())
}

pub(crate) fn transcript_files_in_roots(
    sources: &[(PathBuf, Option<PathBuf>)],
) -> Vec<(String, PathBuf)> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for (root, _) in sources {
        let canonical_root = root.canonicalize().ok();
        for (id, path) in transcript_files_in_root(root, canonical_root.as_deref()) {
            if seen.insert(id.clone()) {
                files.push((id, path));
            }
        }
    }
    files
}

fn transcript_files_in_root(root: &Path, canonical_root: Option<&Path>) -> Vec<(String, PathBuf)> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|entry| {
            let id = entry
                .file_name()
                .into_string()
                .ok()
                .filter(|id| valid_session_id(id))?;
            Some((id.clone(), valid_transcript(root, canonical_root, &id)?))
        })
        .collect()
}

fn valid_transcript(root: &Path, canonical_root: Option<&Path>, id: &str) -> Option<PathBuf> {
    let path = root
        .join(id)
        .join(".system_generated/logs/transcript_full.jsonl");
    (path.is_file()
        && path
            .canonicalize()
            .ok()
            .zip(canonical_root)
            .is_some_and(|(path, root)| path.starts_with(root)))
    .then_some(path)
}

/// Missing roots are tolerated; a session id found in several roots is reported once. The first
/// root wins, except that an index-only entry yields to a later root holding the transcript: the
/// IDE can continue a conversation the CLI index also lists, and only the transcript has usage.
fn scan_roots(sources: &[(PathBuf, Option<PathBuf>)]) -> Vec<SessionMeta> {
    let mut seen = HashMap::new();
    let mut sessions: Vec<SessionMeta> = Vec::new();
    for (root, gemini_dir) in sources {
        for meta in scan_root(root, gemini_dir.as_deref()) {
            match seen.get(&meta.session_id) {
                None => {
                    seen.insert(meta.session_id.clone(), sessions.len());
                    sessions.push(meta);
                }
                // Replace the whole entry so its resume command matches the transcript's root.
                Some(&i) if sessions[i].source_path.is_none() && meta.source_path.is_some() => {
                    sessions[i] = meta;
                }
                Some(_) => {}
            }
        }
    }
    sessions
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn scan_root(root: &Path, gemini_dir: Option<&Path>) -> Vec<SessionMeta> {
    let base = root.parent().unwrap_or(root);
    let conv_dir = base.join("conversations");
    let canonical_root = root.canonicalize().ok();
    let canonical_base = base.canonicalize().ok();
    let mut seen_dbs = HashSet::new();
    let mut index = read_summaries(
        root,
        base,
        canonical_root.as_deref(),
        canonical_base.as_deref(),
        gemini_dir,
        &mut seen_dbs,
    );
    let mut sessions = Vec::new();
    for (id, path) in transcript_files_in_root(root, canonical_root.as_deref()) {
        if index.subagents.contains(&id) {
            continue;
        }
        let summary = index.sessions.remove(&id);
        // Side questions (/btw) and unindexed internal runs are never written to
        // conversation_summaries.db when the index database is present.
        if summary.is_none()
            && (index.has_db
                || db_file(&conv_dir, canonical_base.as_deref(), &id).is_some_and(|db_path| {
                    seen_dbs.insert(db_path.clone());
                    is_non_main_trajectory(&db_path)
                }))
        {
            continue;
        }
        let has_summary = summary.is_some();
        let mut meta = summary.unwrap_or_else(|| SessionMeta {
            provider_id: "agy".into(),
            session_id: id.clone(),
            title: None,
            summary: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: None,
            resume_command: Some(crate::agy_config::resume_command(gemini_dir, &id)),
        });
        let mut has_user_input = false;
        // Avoid parsing every transcript on list refresh when agy's own index is complete.
        if meta.title.is_none() || meta.last_active_at.is_none() {
            if let Ok(rows) = records(&path) {
                has_user_input = rows.iter().any(|v| v["type"] == "USER_INPUT");
                let first = first_prompt(&rows);
                if meta.title.is_none() {
                    meta.title = first.map(|s| truncate_summary(s, 80));
                }
                if meta.summary.is_none() {
                    meta.summary = first.map(|s| truncate_summary(s, 160));
                }
                if meta.created_at.is_none() {
                    meta.created_at = rows.iter().find_map(timestamp);
                }
                if meta.last_active_at.is_none() {
                    meta.last_active_at = rows.iter().filter_map(timestamp).max();
                }
            }
        }
        if meta.title.is_none() {
            if !has_summary && !has_user_input {
                continue;
            }
            meta.title = Some(fallback_session_title(meta.project_dir.as_deref(), &id));
        }
        meta.source_path = Some(path.to_string_lossy().into_owned());
        sessions.push(meta);
    }
    // A binary-only conversation can still be found and resumed, but has no readable transcript.
    for mut meta in index.sessions.into_values() {
        if meta.title.is_none() {
            meta.title = Some(fallback_session_title(
                meta.project_dir.as_deref(),
                &meta.session_id,
            ));
        }
        sessions.push(meta);
    }
    if let Some(conv_prefix) = canonical_conv_dir(&conv_dir, canonical_base.as_deref()) {
        let keep_existing_files = !index.has_db && base.join("conversation_summaries.db").exists();
        prune_trajectory_cache(&conv_prefix, &seen_dbs, keep_existing_files);
    }
    sessions
}

fn fallback_session_title(project_dir: Option<&str>, session_id: &str) -> String {
    project_dir
        .and_then(|p| Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(session_id)
        .to_string()
}

fn first_prompt(rows: &[Value]) -> Option<&str> {
    rows.iter()
        .filter(|v| v["type"] == "USER_INPUT")
        .filter_map(|v| v["content"].as_str())
        .map(user_request)
        .map(str::trim)
        .find(|s| !s.is_empty())
}

#[derive(Default)]
struct SummaryIndex {
    has_db: bool,
    sessions: BTreeMap<String, SessionMeta>,
    subagents: HashSet<String>,
}

fn wal_sibling_path(path: &Path) -> PathBuf {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    PathBuf::from(wal)
}

fn has_nonempty_wal(path: &Path) -> bool {
    let wal_nonempty = |db: &Path| match fs::metadata(wal_sibling_path(db)) {
        Ok(meta) => meta.len() > 0,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    };
    if wal_nonempty(path) {
        return true;
    }
    path.canonicalize()
        .ok()
        .filter(|canonical| canonical != path)
        .is_some_and(|canonical| wal_nonempty(&canonical))
}

fn open_db(path: &Path) -> Option<rusqlite::Connection> {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    match rusqlite::Connection::open_with_flags(path, flags) {
        Ok(conn) => {
            let _ = conn.busy_timeout(Duration::ZERO);
            let probe = conn.prepare("SELECT 1 FROM sqlite_master LIMIT 1").err();
            match probe {
                None => return Some(conn),
                Some(rusqlite::Error::SqliteFailure(err, _))
                    if err.code == rusqlite::ErrorCode::CannotOpen => {}
                Some(_) => return None,
            }
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::CannotOpen && path.is_file() => {}
        Err(_) => return None,
    }
    open_db_immutable(path, flags)
}

/// Fallback for checkpointed WAL databases without a `-shm` sibling that fail in
/// `SQLITE_OPEN_READ_ONLY` with `SQLITE_CANTOPEN` (14). Refuses to open when a non-empty `-wal`
/// file exists, because `immutable=1` would silently ignore uncheckpointed rows in `-wal`.
fn open_db_immutable(path: &Path, flags: rusqlite::OpenFlags) -> Option<rusqlite::Connection> {
    if has_nonempty_wal(path) {
        return None;
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut uri = url::Url::from_file_path(&abs).ok()?;
    uri.set_query(Some("immutable=1"));
    let conn = rusqlite::Connection::open_with_flags(
        uri.as_str(),
        flags | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    conn.busy_timeout(Duration::ZERO).ok()?;
    conn.prepare("SELECT 1 FROM sqlite_master LIMIT 1").ok()?;
    Some(conn)
}

fn canonical_conv_dir(conv_dir: &Path, canonical_base: Option<&Path>) -> Option<PathBuf> {
    conv_dir
        .canonicalize()
        .ok()
        .or_else(|| canonical_base.map(|base| base.join("conversations")))
}

fn trajectory_cache_key(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let mut missing = Vec::new();
    let mut cur = path;
    while let (Some(parent), Some(name)) = (cur.parent(), cur.file_name()) {
        missing.push(name.to_os_string());
        if let Ok(mut canonical) = parent.canonicalize() {
            for component in missing.into_iter().rev() {
                canonical.push(component);
            }
            return canonical;
        }
        cur = parent;
    }
    path.to_path_buf()
}

fn prune_trajectory_cache(
    conv_prefix: &Path,
    seen_dbs: &HashSet<PathBuf>,
    keep_existing_files: bool,
) {
    if !keep_existing_files {
        TRAJECTORY_CACHE
            .lock_recover()
            .retain(|path, _| !path.starts_with(conv_prefix) || seen_dbs.contains(path));
        return;
    }
    let candidates: Vec<PathBuf> = TRAJECTORY_CACHE
        .lock_recover()
        .keys()
        .filter(|path| path.starts_with(conv_prefix) && !seen_dbs.contains(*path))
        .cloned()
        .collect();
    let missing: HashSet<PathBuf> = candidates
        .into_iter()
        .filter(|path| !path.is_file())
        .collect();
    if !missing.is_empty() {
        TRAJECTORY_CACHE
            .lock_recover()
            .retain(|path, _| !missing.contains(path));
    }
}

/// Returns the canonicalized path of `conversations/<id>.db` when it exists inside `canonical_base`.
/// Returning the canonical path keeps `TRAJECTORY_CACHE` keys aligned with `delete_session`.
fn db_file(conv_dir: &Path, canonical_base: Option<&Path>, id: &str) -> Option<PathBuf> {
    let path = conv_dir.join(format!("{id}.db"));
    if !path.is_file() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    canonical_base
        .is_none_or(|base| canonical.starts_with(base))
        .then_some(canonical)
}

const SIDE_QUESTION_TRAJECTORY_TYPE: i64 = 22;

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DbStamp {
    db: FileStamp,
    wal: Option<FileStamp>,
}

fn db_stamp(db_path: &Path) -> Option<DbStamp> {
    let db_meta = fs::metadata(db_path).ok()?;
    let db = FileStamp {
        modified: db_meta.modified().ok()?,
        len: db_meta.len(),
    };
    let wal = match fs::metadata(wal_sibling_path(db_path)) {
        Ok(meta) => Some(FileStamp {
            modified: meta.modified().ok()?,
            len: meta.len(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return None,
    };
    Some(DbStamp { db, wal })
}

static TRAJECTORY_CACHE: LazyLock<Mutex<HashMap<PathBuf, (DbStamp, bool)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Side questions (`/btw`) use `trajectory_meta.trajectory_type = 22` (`SIDE_QUESTION`) and are
/// excluded. Results are cached by canonical path + `.db`/`-wal` stamp so list refreshes do not
/// reopen every database while still noticing uncheckpointed WAL updates.
fn is_non_main_trajectory(canonical_db_path: &Path) -> bool {
    let stamp = db_stamp(canonical_db_path);
    if let Some(stamp) = stamp {
        if let Some(&(cached_stamp, non_main)) =
            TRAJECTORY_CACHE.lock_recover().get(canonical_db_path)
        {
            if cached_stamp == stamp {
                return non_main;
            }
        }
    }
    let Some(conn) = open_db(canonical_db_path) else {
        return false;
    };
    let result = conn.query_row(
        "SELECT trajectory_type FROM trajectory_meta LIMIT 1",
        [],
        |r| r.get::<_, Option<i64>>(0),
    );
    let non_main = match result {
        Ok(t) => t == Some(SIDE_QUESTION_TRAJECTORY_TYPE),
        Err(rusqlite::Error::QueryReturnedNoRows) => false,
        Err(rusqlite::Error::SqliteFailure(err, Some(ref msg)))
            if err.code == rusqlite::ErrorCode::Unknown && msg.contains("no such") =>
        {
            false
        }
        Err(_) => return false,
    };
    if let Some(stamp) = stamp {
        TRAJECTORY_CACHE
            .lock_recover()
            .insert(canonical_db_path.to_path_buf(), (stamp, non_main));
    }
    non_main
}

fn read_summaries(
    root: &Path,
    base: &Path,
    canonical_root: Option<&Path>,
    canonical_base: Option<&Path>,
    gemini_dir: Option<&Path>,
    seen_dbs: &mut HashSet<PathBuf>,
) -> SummaryIndex {
    let mut index = SummaryIndex::default();
    let conv_dir = base.join("conversations");
    let has_conv_dir = conv_dir.is_dir();
    let Some(conn) = open_db(&base.join("conversation_summaries.db")) else {
        return index;
    };
    let sql = "SELECT conversation_id, title, workspace_uris, preview, last_modified_time, step_count, nesting_depth, agent_name FROM conversation_summaries";
    let fallback = "SELECT conversation_id, title, workspace_uris, preview, last_modified_time, 1, 0, '' FROM conversation_summaries";
    let Ok(mut query) = conn.prepare(sql).or_else(|_| conn.prepare(fallback)) else {
        return index;
    };
    let Ok(rows) = query.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            r.get::<_, Option<i64>>(5)?.unwrap_or(1),
            r.get::<_, Option<i64>>(6)?.unwrap_or(0),
            r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        ))
    }) else {
        return index;
    };
    let mut had_row_err = false;
    for row in rows {
        let Ok((id, title, uris, preview, modified, step_count, nesting_depth, agent_name)) = row
        else {
            had_row_err = true;
            continue;
        };
        if !valid_session_id(&id) {
            continue;
        }
        if nesting_depth > 0 || !agent_name.is_empty() {
            index.subagents.insert(id);
            continue;
        }
        if step_count <= 0 {
            continue;
        }
        let conv_db = db_file(&conv_dir, canonical_base, &id);
        if has_conv_dir
            && conv_db.is_none()
            && valid_transcript(root, canonical_root, &id).is_none()
        {
            continue;
        }
        if conv_db.as_ref().is_some_and(|db_path| {
            seen_dbs.insert(db_path.clone());
            is_non_main_trajectory(db_path)
        }) {
            continue;
        }
        let project_dir = serde_json::from_str::<Vec<String>>(&uris)
            .ok()
            .and_then(|v| v.into_iter().next())
            .and_then(|uri| url::Url::parse(&uri).ok()?.to_file_path().ok())
            .map(|p| p.to_string_lossy().into_owned());
        let last_active_at = parse_timestamp_to_ms(&Value::String(modified.clone()))
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&modified, "%Y-%m-%d %H:%M:%S%.f")
                    .ok()
                    .map(|t| t.and_utc().timestamp_millis())
            })
            .filter(|&ms| ms > 0);
        let title = title.trim();
        let preview_text = user_request(preview.trim()).trim();
        let title = (!title.is_empty())
            .then(|| title.to_string())
            .or_else(|| (!preview_text.is_empty()).then(|| truncate_summary(preview_text, 80)));
        index.sessions.insert(
            id.clone(),
            SessionMeta {
                provider_id: "agy".into(),
                session_id: id.clone(),
                title,
                summary: (!preview_text.is_empty()).then(|| truncate_summary(preview_text, 160)),
                project_dir,
                created_at: None,
                last_active_at,
                source_path: None,
                resume_command: Some(crate::agy_config::resume_command(gemini_dir, &id)),
            },
        );
    }
    if !had_row_err {
        index.has_db = true;
    }
    index
}

/// agy stores each prompt wrapped in `<USER_REQUEST>`, followed by context it adds itself
/// (`<ADDITIONAL_METADATA>`, `<USER_SETTINGS_CHANGE>`, ...). Only the user's text is shown.
fn user_request(content: &str) -> &str {
    content
        .split_once("<USER_REQUEST>")
        .and_then(|(_, rest)| rest.split_once("</USER_REQUEST>"))
        .map(|(inner, _)| inner.trim())
        .unwrap_or(content)
}

/// agy 把模型切换写进会话正文：`changed setting \`Model Selection\` from X to Y`。
/// 逐条响应本身通常没有模型字段，按 step 顺序继承最近一次切换即可还原当时的模型。
pub(crate) fn model_from_settings_change(content: &str) -> Option<String> {
    let start = content.find("Model Selection")?;
    let rest = &content[start..];
    let to = rest.find(" to ")? + 4;
    let tail = &rest[to..];
    // 模型名里本身有点（Gemini 3.8），只把「句号 + 空格」当成句子结束。
    let end = tail
        .find(". ")
        .or_else(|| tail.find('\n'))
        .or_else(|| tail.find('`'))
        .unwrap_or(tail.len());
    let model = tail[..end].trim().trim_matches('.').trim();
    if model.is_empty() || model.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(model.to_string())
    }
}

pub(crate) fn timestamp(value: &Value) -> Option<i64> {
    parse_timestamp_to_ms(&value["created_at"])
}

struct PendingCall {
    id: String,
    raw_name: String,
    subagent_type: Option<String>,
}

fn is_typed_tool_result(step: &Value, raw_name: &str) -> bool {
    let Some(kind) = step["type"].as_str() else {
        return false;
    };
    if kind == "GENERIC" {
        return true;
    }
    if kind == "ERROR_MESSAGE"
        && step["source"]
            .as_str()
            .is_none_or(|source| source == "SYSTEM")
    {
        return true;
    }
    if step["source"].as_str() != Some("MODEL") || kind == "PLANNER_RESPONSE" {
        return false;
    }
    match raw_name {
        "replace_file_content"
        | "edit_file"
        | "multi_replace_file_content"
        | "write_to_file"
        | "write_file" => kind == "CODE_ACTION",
        "list_dir" => kind == "LIST_DIRECTORY",
        "run_command" | "run_shell_command" => matches!(kind, "RUN_COMMAND" | "RUN_SHELL_COMMAND"),
        _ => kind.eq_ignore_ascii_case(raw_name),
    }
}

/// `DONE` describes the tool step, not the process's exit status. Only inspect shell
/// metadata before `Output:` / `Command output:`, never arbitrary stdout or file contents.
fn antigravity_command_exit_code(step: &Value, output: &str) -> Option<i32> {
    if let Some(code) = step
        .get("exit_code")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok())
    {
        return Some(code);
    }
    if !output.trim_start().starts_with("Created At:") {
        return None;
    }
    for line in output.lines().take(12).map(str::trim) {
        if matches!(line, "Output:" | "Command Output:" | "Command output:") {
            break;
        }
        if let Some(code) = line.strip_prefix("The command failed with exit code:") {
            return code.trim().parse().ok();
        }
        if let Some(code) = line
            .strip_prefix("Command exited with code ")
            .map(|s| s.trim_end_matches('.'))
        {
            return code.trim().parse().ok();
        }
        if line == "The command completed successfully." {
            return Some(0);
        }
    }
    None
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    let mut pending_calls: VecDeque<PendingCall> = VecDeque::new();
    let mut subagent_labels: HashMap<String, String> = HashMap::new();
    let mut current_model: Option<String> = None;

    for (step_index, span, value) in spanned_records(path)? {
        let ts = timestamp(&value);
        let step_type = value["type"].as_str();
        let mut role = "system";
        let mut injected = false;
        let mut msg_blocks = Vec::new();

        match step_type {
            Some("USER_INPUT") => {
                role = "user";
                pending_calls.clear();
                let raw = value["content"].as_str().unwrap_or_default();
                if let Some(model) = model_from_settings_change(raw) {
                    current_model = Some(model);
                }
                let content = user_request(raw).trim();
                if !content.is_empty() {
                    msg_blocks.push(SessionBlock::text(content));
                }
                for image in media_images(&value, span) {
                    msg_blocks.push(SessionBlock::Image { image });
                }
            }
            _ if step_type == Some("GENERIC")
                || pending_calls
                    .front()
                    .is_some_and(|call| is_typed_tool_result(&value, &call.raw_name)) =>
            {
                role = "tool";
                let (call_id, raw_name, pending_subagent_type) = match pending_calls.pop_front() {
                    Some(call) => (call.id, Some(call.raw_name), call.subagent_type),
                    None => (format!("step-{step_index}-result"), None, None),
                };
                let content_value = &value["content"];
                let content = match content_value {
                    Value::String(text) => text.clone(),
                    Value::Object(map) => map
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    Value::Array(items) => items
                        .iter()
                        .filter_map(|item| {
                            item.as_str()
                                .or_else(|| item.get("text").and_then(Value::as_str))
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                let error = value["error"].as_str().unwrap_or_default();
                if let Some(subagent_type) = pending_subagent_type {
                    if let Some(conv_id) = extract_subagent_conversation_id(&content) {
                        let label = if !subagent_type.is_empty() {
                            subagent_type
                        } else {
                            default_subagent_label(&conv_id)
                        };
                        subagent_labels.insert(conv_id, label);
                    }
                }
                let (output, pointer) = if !content.trim().is_empty() {
                    (
                        content.as_str(),
                        if content_value.is_object() {
                            "/content/text"
                        } else {
                            "/content"
                        },
                    )
                } else if !error.trim().is_empty() {
                    (error, "/error")
                } else {
                    (content.as_str(), "/content")
                };
                let exit_code = raw_name
                    .as_deref()
                    .filter(|name| matches!(*name, "run_command" | "run_shell_command"))
                    .and_then(|_| antigravity_command_exit_code(&value, output));
                let status_str = value["status"].as_str().unwrap_or_default();
                let status = if step_type == Some("ERROR_MESSAGE")
                    || matches!(status_str, "ERROR" | "INVALID")
                    || !error.is_empty()
                    || (status_str == "DONE" && exit_code.is_some_and(|code| code != 0))
                {
                    ToolStatus::Error
                } else if matches!(status_str, "CANCELLED" | "CANCELED" | "INTERRUPTED") {
                    ToolStatus::Interrupted
                } else if status_str == "RUNNING" {
                    ToolStatus::Pending
                } else if status_str == "DONE" {
                    ToolStatus::Success
                } else {
                    ToolStatus::Unknown
                };
                let mut block = blocks::tool_result_block(call_id, status, output, || {
                    Some(span.content_ref(pointer))
                });
                if let SessionBlock::ToolResult {
                    exit_code: code,
                    images,
                    ..
                } = &mut block
                {
                    *code = exit_code;
                    *images = media_images(&value, span);
                }
                msg_blocks.push(block);
            }
            _ if step_type == Some("PLANNER_RESPONSE") || value["source"] == "MODEL" => {
                role = "assistant";
                pending_calls.clear();
                if let Some(thinking) = value["thinking"].as_str().filter(|s| !s.trim().is_empty())
                {
                    msg_blocks.push(blocks::thinking_block(thinking, None, None, || {
                        Some(span.content_ref("/thinking"))
                    }));
                }
                if let Some(content) = value["content"].as_str().filter(|s| !s.trim().is_empty()) {
                    msg_blocks.push(SessionBlock::text(content));
                }
                if let Some(calls) = value["tool_calls"].as_array() {
                    for (idx, call) in calls.iter().enumerate() {
                        let call_id = call["id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("step-{step_index}-call-{idx}"));
                        let raw_name = call["name"]
                            .as_str()
                            .or_else(|| call.pointer("/function/name").and_then(Value::as_str))
                            .unwrap_or("unknown");
                        let args = call.get("args").unwrap_or(&Value::Null);
                        let subagent_type = (raw_name == "invoke_subagent").then(|| {
                            let label = blocks::str_field(args, &["TypeName", "type_name"])
                                .map(sanitize_subagent_label)
                                .unwrap_or_default();
                            if let Some(conv_id) =
                                blocks::str_field(args, &["ConversationId", "conversation_id"])
                            {
                                if !label.is_empty() {
                                    subagent_labels.insert(conv_id.to_string(), label.clone());
                                }
                            }
                            label
                        });
                        pending_calls.push_back(PendingCall {
                            id: call_id.clone(),
                            raw_name: raw_name.to_string(),
                            subagent_type,
                        });
                        msg_blocks.push(agy_tool_call(&call_id, raw_name, args, span, idx));
                    }
                }
            }
            Some("SYSTEM_MESSAGE") => {
                let inner = strip_system_message_wrapper(value["content"].as_str().unwrap_or(""));
                if value["source"] == "SYSTEM_SDK" {
                    injected = true;
                    if !inner.is_empty() {
                        msg_blocks.push(SessionBlock::text(inner));
                    }
                } else if let Some((sender, body)) = parse_message_envelope(inner) {
                    let (first_line, rest) = body.split_once('\n').unwrap_or((body, ""));
                    if !sender.is_empty() && sender != "system" && !sender.contains("/task-") {
                        let label = subagent_labels
                            .get(sender)
                            .cloned()
                            .unwrap_or_else(|| default_subagent_label(sender));
                        msg_blocks.push(SessionBlock::event(
                            EventKind::SubAgent,
                            Some(format!("{label}: {}", first_line.trim())),
                            None,
                        ));
                    } else {
                        msg_blocks.push(SessionBlock::event(
                            EventKind::Info,
                            Some(first_line.trim().to_string()),
                            None,
                        ));
                    }
                    if !rest.trim().is_empty() {
                        msg_blocks.push(SessionBlock::text(rest.trim().to_string()));
                    }
                } else if !inner.is_empty() {
                    msg_blocks.push(SessionBlock::text(inner));
                }
            }
            Some("CHECKPOINT") => {
                let content = value["content"].as_str().unwrap_or_default();
                if !content.trim().is_empty() {
                    msg_blocks.push(blocks::summary_event_block(
                        EventKind::Compaction,
                        content,
                        || Some(span.content_ref("/content")),
                    ));
                }
            }
            Some("ERROR_MESSAGE") => {
                let err = value["error"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| value["content"].as_str().filter(|s| !s.trim().is_empty()))
                    .unwrap_or("Error");
                msg_blocks.push(SessionBlock::event(
                    EventKind::Error,
                    Some(err.trim().to_string()),
                    None,
                ));
            }
            _ => {
                let content = value["content"].as_str().unwrap_or_default();
                let text = if content.is_empty() {
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                } else {
                    content.to_string()
                };
                if !text.trim().is_empty() {
                    msg_blocks.push(SessionBlock::text(text));
                }
            }
        }

        if msg_blocks.is_empty() {
            continue;
        }
        let mut message = SessionMessage::from_blocks(role, ts, msg_blocks);
        message.id = Some(step_index.to_string());
        message.injected = injected;
        message.meta = message_meta(
            &value,
            (role == "assistant")
                .then_some(())
                .and(current_model.as_deref()),
        );
        messages.push(message);
    }
    assign_turn_ids(&mut messages);
    Ok(messages)
}

fn message_meta(value: &Value, inherited_model: Option<&str>) -> Option<MessageMeta> {
    let meta = MessageMeta {
        model: value["model"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("unknown"))
            .or(inherited_model)
            .map(String::from),
        input_tokens: value["input_tokens"].as_u64(),
        output_tokens: value["output_tokens"].as_u64(),
        cache_read_tokens: value["cache_read_tokens"].as_u64(),
        cache_write_tokens: value["cache_write_tokens"].as_u64(),
        reasoning_tokens: value["reasoning_tokens"].as_u64(),
        ..Default::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

fn media_images(value: &Value, span: JsonlSpan) -> Vec<ImageRef> {
    value["media"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(idx, item)| {
            let media_type =
                blocks::str_field(item, &["mime_type", "mimeType", "media_type", "mediaType"])
                    .unwrap_or("image/png");
            if !media_type.starts_with("image/") {
                return None;
            }
            let (field, val) = ["data", "inline_data", "uri", "path"]
                .into_iter()
                .find_map(|k| item[k].as_str().filter(|s| !s.is_empty()).map(|s| (k, s)))?;
            let (source, size) =
                if matches!(field, "data" | "inline_data") || val.starts_with("data:") {
                    (
                        ImageSource::Inline {
                            content: span.content_ref(format!("/media/{idx}/{field}")),
                        },
                        blocks::estimate_base64_size(val.len()),
                    )
                } else {
                    (
                        ImageSource::LocalFile {
                            path: val.to_string(),
                        },
                        0,
                    )
                };
            Some(ImageRef {
                source,
                media_type: media_type.to_string(),
                size,
                alt: blocks::str_field(item, &["alt", "name", "filename"]).map(str::to_string),
            })
        })
        .collect()
}

fn sanitize_subagent_label(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == ':' {
                '-'
            } else {
                c
            }
        })
        .take(40)
        .collect()
}

fn default_subagent_label(sender: &str) -> String {
    let prefix: String = sender
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .take(8)
        .collect();
    if prefix.is_empty() {
        "subagent".to_string()
    } else {
        format!("subagent-{prefix}")
    }
}

fn extract_subagent_conversation_id(content: &str) -> Option<String> {
    if !content.contains("Created the following subagent:") {
        return None;
    }
    let (_, after_key) = content.split_once("\"conversationId\"")?;
    let (_, after_colon) = after_key.split_once(':')?;
    let rest = after_colon.trim_start().strip_prefix('"')?;
    let (id, _) = rest.split_once('"')?;
    (!id.is_empty()).then(|| id.to_string())
}

fn strip_system_message_wrapper(raw: &str) -> &str {
    let mut s = raw.trim();
    if s.starts_with("The following is a <SYSTEM_MESSAGE>") {
        if let Some((_, rest)) = s.split_once('\n') {
            s = rest.trim();
        }
    }
    if let Some(after_open) = s.strip_prefix("<SYSTEM_MESSAGE") {
        if let Some((_, body)) = after_open.split_once('>') {
            s = body
                .strip_suffix("</SYSTEM_MESSAGE>")
                .unwrap_or(body)
                .trim();
        }
    }
    s
}

fn parse_message_envelope(inner: &str) -> Option<(&str, &str)> {
    let rest = inner.strip_prefix("[Message]")?.trim_start();
    let (_, after_sender) = rest.split_once("sender=")?;
    let end = after_sender
        .find(char::is_whitespace)
        .unwrap_or(after_sender.len());
    let sender = &after_sender[..end];
    let (_, content) = after_sender[end..].split_once("content=")?;
    Some((sender.trim(), content.trim()))
}

fn raw_str<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| v[*k].as_str())
}

fn agy_tool_call(
    call_id: &str,
    raw_name: &str,
    args: &Value,
    span: JsonlSpan,
    idx: usize,
) -> SessionBlock {
    let args_pointer = format!("/tool_calls/{idx}/args");
    let str_in = |keys: &[&str]| blocks::str_field(args, keys);
    let line_in = |keys: &[&str]| str_in(keys).map(blocks::one_line_title);
    let title_in = |keys: &[&str]| line_in(keys).unwrap_or_default();
    let u64_in = |keys: &[&str]| {
        keys.iter().find_map(|k| {
            let v = args.get(*k)?;
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
    };
    let path = str_in(&[
        "AbsolutePath",
        "TargetFile",
        "File",
        "Path",
        "file_path",
        "filePath",
        "path",
    ])
    .unwrap_or("");

    let mut server = None;
    let mut diff = None;
    let mapped = match raw_name {
        "invoke_subagent" | "send_message" | "manage_subagents" | "define_subagent" => Some((
            ToolKind::Agent,
            title_in(&[
                "Role",
                "role",
                "Recipient",
                "recipient",
                "Action",
                "action",
                "Name",
                "name",
                "Prompt",
                "prompt",
            ]),
            line_in(&[
                "TypeName",
                "type_name",
                "Message",
                "message",
                "ConversationId",
                "conversation_id",
                "Target",
                "Description",
            ]),
        )),
        "read_board" | "swarm" | "swarm_wait" | "post_message" => {
            let title = line_in(&["action", "Action"]).unwrap_or_else(|| raw_name.to_string());
            let detail = line_in(&["text", "message", "Message", "name", "target"])
                .or_else(|| u64_in(&["since"]).map(|n| format!("since {n}")))
                .filter(|d| d != &title);
            Some((ToolKind::Agent, title, detail))
        }
        "run_command" | "run_shell_command" => {
            let cmd = str_in(&["CommandLine", "command", "cmd"]).unwrap_or_default();
            Some((
                blocks::refine_shell_kind(cmd),
                blocks::title_shell(cmd),
                line_in(&["toolSummary", "Cwd", "cwd", "description"]),
            ))
        }
        "command_status" | "manage_task" | "send_command_input" => {
            let title = title_in(&["CommandId", "TaskId", "Action", "action"]);
            let detail = line_in(&["Input", "Status", "Action", "action"]).filter(|d| d != &title);
            Some((ToolKind::Shell, title, detail))
        }
        "view_file" | "read_file" | "view_file_outline" | "view_code_item" => {
            let detail = match (
                u64_in(&["StartLine", "start_line", "offset"]),
                u64_in(&["EndLine", "end_line"]),
            ) {
                (Some(s), Some(e)) => Some(format!(":{s}-{e}")),
                (Some(s), None) => Some(format!(":{s}")),
                (None, Some(e)) => Some(format!(":1-{e}")),
                (None, None) => line_in(&["NodePath"]).or_else(|| {
                    let items: Vec<&str> = args["NodePaths"]
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_str)
                        .collect();
                    (!items.is_empty()).then(|| blocks::title_web(&items))
                }),
            };
            Some((ToolKind::Read, blocks::title_path(path), detail))
        }
        "replace_file_content" | "edit_file" | "multi_replace_file_content" => {
            let pair_counts = |item: &Value| {
                Some(blocks::line_change_counts(
                    raw_str(item, &["TargetContent", "Target", "old_string"])?,
                    raw_str(item, &["ReplacementContent", "Replacement", "new_string"])?,
                ))
            };
            let counts = match args["ReplacementChunks"].as_array() {
                Some(chunks) if !chunks.is_empty() => {
                    chunks
                        .iter()
                        .map(pair_counts)
                        .try_fold((0u32, 0u32), |(a, r), counts| {
                            let (x, y) = counts?;
                            Some((a.saturating_add(x), r.saturating_add(y)))
                        })
                }
                Some(_) => None,
                None => pair_counts(args),
            };
            diff = counts.filter(|_| !path.is_empty()).map(|(added, removed)| {
                blocks::single_file_diff(path, DiffOp::Update, added, removed)
            });
            Some((
                ToolKind::Edit,
                blocks::title_path(path),
                line_in(&["Instruction", "Description"]),
            ))
        }
        "write_to_file" | "write_file" => {
            diff = raw_str(args, &["CodeContent", "Content", "content"])
                .filter(|_| !path.is_empty())
                .map(|code| {
                    let added = u32::try_from(code.lines().count()).unwrap_or(u32::MAX);
                    let op = if args["Append"].as_bool() == Some(true) {
                        DiffOp::Update
                    } else {
                        DiffOp::Add
                    };
                    blocks::single_file_diff(path, op, added, 0)
                });
            Some((
                ToolKind::Write,
                blocks::title_path(path),
                line_in(&["Description"]),
            ))
        }
        "grep_search" | "find_by_name" | "list_dir" => {
            let pattern = line_in(&["Query", "Pattern", "query", "pattern"]);
            let dir = line_in(&[
                "SearchPath",
                "SearchDirectory",
                "DirectoryPath",
                "path",
                "dir_path",
            ]);
            let (title, detail) = match (pattern, dir) {
                (Some(p), d) => (p, d),
                (None, Some(d)) => (d, None),
                (None, None) => (String::new(), None),
            };
            Some((ToolKind::Search, title, detail))
        }
        "search_marketplace" => {
            let queries: Vec<&str> = args["queries"]
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let title = if queries.is_empty() {
                title_in(&["query", "Query"])
            } else {
                blocks::title_web(&queries)
            };
            Some((ToolKind::Search, title, line_in(&["kind", "Kind"])))
        }
        "search_web" | "read_url_content" => Some((
            ToolKind::Web,
            title_in(&["query", "Query", "Url", "url"]),
            line_in(&["domain", "Domain"]),
        )),
        "webview_browser" | "browser_subagent" => Some((
            ToolKind::Web,
            title_in(&[
                "intention",
                "Intention",
                "Task",
                "task",
                "url",
                "Url",
                "script",
            ]),
            line_in(&["observation", "Observation", "RecordingName"]),
        )),
        "ask_question" | "ask_permission" => {
            let question = args
                .pointer("/questions/0/question")
                .and_then(Value::as_str)
                .or_else(|| str_in(&["Action", "question", "prompt"]));
            Some((
                ToolKind::Ask,
                question.map(blocks::title_ask).unwrap_or_default(),
                line_in(&["Target", "Reason"]),
            ))
        }
        "generate_image" => Some((
            ToolKind::Other,
            raw_name.to_string(),
            line_in(&["ImageName", "toolSummary", "Prompt"]),
        )),
        "call_mcp_tool" | "list_resources" | "read_resource" => {
            let srv = str_in(&["ServerName", "server_name", "server"]);
            server = srv.map(str::to_string);
            let tool = str_in(&["ToolName", "tool_name", "tool"]).unwrap_or(raw_name);
            let title = match srv {
                Some(srv) => blocks::title_mcp(srv, tool),
                None => blocks::one_line_title(tool),
            };
            let detail = if raw_name == "call_mcp_tool" {
                args.get("Arguments")
                    .and_then(blocks::first_string_field)
                    .or_else(|| line_in(&["Arguments"]))
            } else {
                line_in(&["Uri", "uri"])
            };
            Some((ToolKind::Mcp, title, detail))
        }
        name if name.starts_with("mcp_") && !name.starts_with("mcp__") => {
            let rest = &name["mcp_".len()..];
            let (srv, tool) = rest.split_once('_').unwrap_or((rest, rest));
            server = Some(srv.to_string());
            let detail = line_in(&["toolSummary", "query", "chunk_id", "uri"])
                .or_else(|| blocks::first_string_field(args));
            Some((ToolKind::Mcp, blocks::title_mcp(srv, tool), detail))
        }
        _ => None,
    };

    if let Some((kind, title, detail)) = mapped {
        let preview = blocks::input_preview(args);
        SessionBlock::ToolCall {
            id: call_id.to_string(),
            raw_name: raw_name.to_string(),
            kind,
            title,
            detail,
            server,
            input_preview: preview.text,
            input_total_len: preview.total_len,
            input_full: preview.truncated.then(|| span.content_ref(args_pointer)),
            diff,
            by_user: false,
        }
    } else {
        blocks::tool_call_block(ToolSource::Generic, call_id, raw_name, args, || {
            Some(span.content_ref(args_pointer))
        })
    }
}

/// Delete a CLI conversation the way agy's picker (`/resume`, F4) does: its row in
/// `conversation_summaries.db` (list + full-text search), its entry in
/// `jetbox_summaries_proto.pb`, then `conversations/<id>.db`, `brain/<id>/` and the annotation.
///
/// The jetbox file is the source agy rebuilds the summaries table from, so dropping only the
/// table rows would bring the conversation back on the next start. It is changed first in
/// memory and checked to be the known map layout; anything else is refused before writing.
/// IDE conversations (`~/.gemini/antigravity`) keep their index elsewhere and stay read-only.
pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if !valid_session_id(session_id) {
        return Err("Invalid Antigravity session ID".into());
    }
    let expected = root
        .join(session_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript_full.jsonl");
    if path != expected {
        return Err(format!(
            "Unexpected Antigravity session source: {}",
            path.display()
        ));
    }
    let app_dir = root
        .parent()
        .filter(|_| root.file_name().is_some_and(|name| name == "brain"))
        .filter(|dir| {
            dir.file_name()
                .is_some_and(|name| name == "antigravity-cli")
        })
        .ok_or("Delete this session in Antigravity to keep its index and summaries consistent")?;

    // agy holds this lock while the conversation is open; keep it until the files are gone.
    let presence = app_dir.join("presence").join(format!("{session_id}.lock"));
    let presence_lock = lock_presence(&presence)?;

    let jetbox_path = app_dir.join("jetbox_summaries_proto.pb");
    let jetbox = match fs::read(&jetbox_path) {
        Ok(bytes) => Some(jetbox_without(&bytes, session_id)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Cannot read Antigravity conversation index: {e}")),
    };

    delete_summary_rows(&app_dir.join("conversation_summaries.db"), session_id)?;
    if let Some(Some(bytes)) = jetbox {
        crate::config::atomic_write_private(&jetbox_path, &bytes)
            .map_err(|e| format!("Cannot update Antigravity conversation index: {e}"))?;
    }

    let conversation = app_dir
        .join("conversations")
        .join(format!("{session_id}.db"));
    let cache_key = trajectory_cache_key(&conversation);
    ignore_missing(&conversation, fs::remove_file(&conversation))?;
    TRAJECTORY_CACHE.lock_recover().remove(&cache_key);
    for file in [
        wal_sibling_path(&conversation),
        conversation.with_extension("db-shm"),
        app_dir
            .join("annotations")
            .join(format!("{session_id}.pbtxt")),
    ] {
        ignore_missing(&file, fs::remove_file(&file))?;
    }
    let brain_dir = root.join(session_id);
    ignore_missing(&brain_dir, fs::remove_dir_all(&brain_dir))?;
    // Windows cannot delete a file this process still has open.
    drop(presence_lock);
    ignore_missing(&presence, fs::remove_file(&presence))?;
    Ok(true)
}

// `rust-version` in Cargo.toml is stale (dependencies already need 1.88); the pinned
// toolchain (rust-toolchain.toml) has `File::try_lock`.
#[allow(clippy::incompatible_msrv)]
fn lock_presence(path: &Path) -> Result<Option<fs::File>, String> {
    let file = match fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "Cannot check whether agy is using this session: {e}"
            ))
        }
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => {
            Err("This conversation is open in agy. Close it there before deleting".into())
        }
        Err(fs::TryLockError::Error(e)) => Err(format!(
            "Cannot check whether agy is using this session: {e}"
        )),
    }
}

fn delete_summary_rows(db: &Path, session_id: &str) -> Result<(), String> {
    if !db.exists() {
        return Ok(());
    }
    let err = |e: rusqlite::Error| format!("Cannot update Antigravity conversation list: {e}");
    let mut conn = rusqlite::Connection::open(db).map_err(err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(err)?;
    let tx = conn.transaction().map_err(err)?;
    for table in [
        "conversation_summaries",
        "conversations_fts",
        "conversation_parents",
    ] {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(err)?;
        if exists {
            tx.execute(
                &format!("DELETE FROM {table} WHERE conversation_id = ?1"),
                [session_id],
            )
            .map_err(err)?;
        }
    }
    tx.commit().map_err(err)
}

fn ignore_missing(path: &Path, result: std::io::Result<()>) -> Result<(), String> {
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to delete {}: {e}", path.display())),
    }
}

/// `jetbox_summaries_proto.pb` is a `map<string, Summary>`: repeated top-level field 1, each a
/// message whose field 1 is the conversation id. Returns the file without that id's entry, or
/// `None` when it has none. Any other layout is refused rather than rewritten.
fn jetbox_without(bytes: &[u8], session_id: &str) -> Result<Option<Vec<u8>>, String> {
    let unknown =
        || "Unrecognized Antigravity conversation index; delete this session in agy".to_string();
    let mut kept = Vec::with_capacity(bytes.len());
    let mut dropped = false;
    let mut rest = bytes;
    while !rest.is_empty() {
        let (field, wire, body, len) = proto_field(rest).ok_or_else(unknown)?;
        if field != 1 || wire != 2 {
            return Err(unknown());
        }
        let (key_field, key_wire, key, _) = proto_field(body).ok_or_else(unknown)?;
        if key_field != 1 || key_wire != 2 {
            return Err(unknown());
        }
        if key == session_id.as_bytes() {
            dropped = true;
        } else {
            kept.extend_from_slice(&rest[..len]);
        }
        rest = &rest[len..];
    }
    Ok(dropped.then_some(kept))
}

/// One length-delimited field: `(field number, wire type, payload, bytes consumed)`. Other wire
/// types are reported with an empty payload so the caller can reject them.
fn proto_field(bytes: &[u8]) -> Option<(u64, u8, &[u8], usize)> {
    fn varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *bytes.get(*at)?;
            *at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Some(value);
            }
        }
        None
    }
    let mut at = 0;
    let key = varint(bytes, &mut at)?;
    let wire = (key & 7) as u8;
    if wire != 2 {
        return Some((key >> 3, wire, &[], at));
    }
    let len = usize::try_from(varint(bytes, &mut at)?).ok()?;
    let end = at.checked_add(len).filter(|end| *end <= bytes.len())?;
    Some((key >> 3, wire, &bytes[at..end], end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::content::{resolve_content_ref, SourceLocation, ValidatedSource};
    use crate::session_manager::model::ContentRef;
    use serde_json::json;

    fn write_jsonl(path: &Path, lines: &[Value]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let raw: String = lines.iter().map(|v| format!("{v}\n")).collect();
        fs::write(path, raw).unwrap();
    }

    fn validated_source(file: &Path, root: &Path) -> ValidatedSource {
        ValidatedSource {
            provider_id: "agy".into(),
            raw: file.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: file.canonicalize().unwrap(),
                root: root.canonicalize().unwrap(),
            },
        }
    }

    type CallTuple<'a> = (
        &'a str,
        ToolKind,
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        Option<(u32, u32)>,
    );

    fn call_tuple(block: &SessionBlock) -> CallTuple<'_> {
        match block {
            SessionBlock::ToolCall {
                id,
                kind,
                title,
                detail,
                server,
                diff,
                ..
            } => (
                id.as_str(),
                *kind,
                title.as_str(),
                detail.as_deref(),
                server.as_deref(),
                diff.as_ref().map(|d| (d.added, d.removed)),
            ),
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    fn result_tuple(
        msg: &SessionMessage,
    ) -> (&str, ToolStatus, bool, &str, Option<&ContentRef>, usize) {
        match &msg.blocks[0] {
            SessionBlock::ToolResult {
                call_id,
                status,
                truncated,
                preview,
                full,
                images,
                ..
            } => (
                call_id.as_str(),
                *status,
                *truncated,
                preview.as_str(),
                full.as_ref(),
                images.len(),
            ),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn agy_binary_only_sessions_keep_index_metadata_without_fabricated_transcripts() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project space");
        let workspace_uris =
            serde_json::to_string(&[url::Url::from_file_path(&project).unwrap().to_string()])
                .unwrap();
        let db = rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
        db.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT,title TEXT,workspace_uris TEXT,preview TEXT,last_modified_time TEXT);").unwrap();
        db.execute(
            "INSERT INTO conversation_summaries VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![
                "abc-123",
                "Test title",
                workspace_uris,
                "Preview",
                "2026-10-13T17:26:12Z"
            ],
        )
        .unwrap();
        let sessions = scan_root(&dir.path().join("brain"), None);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title.as_deref(), Some("Test title"));
        assert_eq!(
            sessions[0].project_dir.as_deref(),
            Some(project.to_string_lossy().as_ref())
        );
        assert!(sessions[0].source_path.is_none());
        assert!(sessions[0].last_active_at.is_some());
    }

    #[test]
    fn agy_filters_empty_deleted_side_question_and_subagent_rows_while_keeping_transcript_files_for_usage(
    ) {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("rea");
        let workspace_uris =
            serde_json::to_string(&[url::Url::from_file_path(&project).unwrap().to_string()])
                .unwrap();
        let conv_dir = dir.path().join("conversations");
        fs::create_dir_all(&conv_dir).unwrap();
        for id in [
            "empty-0",
            "kept-preview",
            "kept-transcript",
            "subagent-1",
            "subagent-depth",
        ] {
            fs::write(conv_dir.join(format!("{id}.db")), b"").unwrap();
        }
        rusqlite::Connection::open(conv_dir.join("side-1.db"))
            .unwrap()
            .execute_batch(
                "CREATE TABLE trajectory_meta(trajectory_id TEXT, trajectory_type INTEGER);
                 INSERT INTO trajectory_meta VALUES ('side-1', 22);",
            )
            .unwrap();

        let root = dir.path().join("brain");
        for (id, line) in [
            (
                "kept-transcript",
                json!({"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>\nfrom transcript\n</USER_REQUEST>"}),
            ),
            (
                "side-1",
                json!({"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>\nThis is a side question from the user. Here is the question: pnpm start?\n</USER_REQUEST>"}),
            ),
            (
                "subagent-1",
                json!({"step_index":0,"type":"SYSTEM_MESSAGE","content":"task"}),
            ),
            (
                "subagent-depth",
                json!({"step_index":0,"type":"SYSTEM_MESSAGE","content":"task"}),
            ),
            (
                "subagent-unindexed",
                json!({"step_index":0,"type":"SYSTEM_MESSAGE","content":"task"}),
            ),
        ] {
            write_jsonl(
                &root
                    .join(id)
                    .join(".system_generated/logs/transcript_full.jsonl"),
                &[line],
            );
        }

        let db = rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE conversation_summaries(conversation_id TEXT,title TEXT,workspace_uris TEXT,preview TEXT,last_modified_time TEXT,step_count INTEGER,parent_conversation_id TEXT,nesting_depth INTEGER,agent_name TEXT);
             INSERT INTO conversation_summaries VALUES
             ('empty-0','','[\"file:///tmp\"]','','0001-01-01 00:00:00+00:00',0,'',0,''),
             ('deleted-parent','Ghost','[]','hi','2026-10-13T17:26:12Z',2,'',0,''),
             ('kept-preview','   ','[]','<USER_REQUEST>\nfrom preview\n</USER_REQUEST>','0001-01-01 00:00:00+00:00',2,'',0,''),
             ('subagent-1','','','','2026-11-05 15:06:03.386663+00:00',9,'',1,'DeepCoder'),
             ('subagent-depth','Internal','','','2026-11-05 15:06:03.386663+00:00',5,'subagent-1',2,'');",
        )
        .unwrap();
        db.execute(
            "INSERT INTO conversation_summaries VALUES ('kept-transcript','',?1,'','2026-11-05 15:06:03.386663+00:00',4,'',0,'')",
            [&workspace_uris],
        )
        .unwrap();
        drop(db);

        let mut sessions = scan_root(&root, None);
        sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            (
                sessions[0].session_id.as_str(),
                sessions[0].title.as_deref(),
                sessions[0].summary.as_deref(),
                sessions[0].last_active_at
            ),
            (
                "kept-preview",
                Some("from preview"),
                Some("from preview"),
                None
            )
        );
        assert_eq!(
            (
                sessions[1].session_id.as_str(),
                sessions[1].title.as_deref(),
                sessions[1].project_dir.as_deref()
            ),
            (
                "kept-transcript",
                Some("from transcript"),
                Some(project.to_string_lossy().as_ref())
            )
        );
        assert!(sessions[1].last_active_at.is_some());

        fs::remove_file(dir.path().join("conversation_summaries.db")).unwrap();
        let without_index: Vec<_> = scan_root(&root, None)
            .into_iter()
            .map(|s| s.session_id)
            .collect();
        assert_eq!(without_index, ["kept-transcript"]);

        let mut files: Vec<_> = transcript_files_in_roots(&[(root, None)])
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                "kept-transcript",
                "side-1",
                "subagent-1",
                "subagent-depth",
                "subagent-unindexed"
            ]
        );
    }

    #[test]
    fn agy_scans_cli_and_ide_roots_deduplicating_and_tolerating_missing() {
        let home = tempfile::tempdir().unwrap();
        let write = |root: &str, id: &str| {
            let logs = home
                .path()
                .join(root)
                .join("brain")
                .join(id)
                .join(".system_generated/logs");
            fs::create_dir_all(&logs).unwrap();
            fs::write(
                logs.join("transcript_full.jsonl"),
                r#"{"step_index":0,"type":"USER_INPUT","content":"hi"}"#,
            )
            .unwrap();
        };
        write("antigravity-cli", "cli-1");
        write("antigravity", "ide-1");
        write("antigravity", "cli-1");
        let roots = vec![
            (home.path().join("antigravity-cli/brain"), None),
            (home.path().join("antigravity/brain"), None),
            (home.path().join("missing/brain"), None),
        ];
        let mut ids: Vec<_> = scan_roots(&roots)
            .into_iter()
            .map(|s| s.session_id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["cli-1", "ide-1"]);
    }

    #[test]
    fn agy_index_only_entry_yields_to_a_later_root_with_the_transcript() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let cli = home.path().join("antigravity-cli");
        fs::create_dir_all(&cli).unwrap();
        let db = rusqlite::Connection::open(cli.join("conversation_summaries.db")).unwrap();
        db.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT,title TEXT,workspace_uris TEXT,preview TEXT,last_modified_time TEXT);").unwrap();
        db.execute(
            "INSERT INTO conversation_summaries VALUES ('shared-1','CLI title','[]','','2026-10-13T17:26:12Z')",
            [],
        )
        .unwrap();
        let logs = home
            .path()
            .join("antigravity/brain/shared-1/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"type":"USER_INPUT","content":"hi"}"#,
        )
        .unwrap();
        let roots = vec![
            (cli.join("brain"), None),
            (home.path().join("antigravity/brain"), None),
        ];
        let sessions = scan_roots(&roots);
        assert_eq!(sessions.len(), 1);
        // Compare as paths: on Windows the joined segments mix `/` and `\`.
        assert_eq!(
            sessions[0].source_path.as_deref().map(Path::new),
            Some(logs.join("transcript_full.jsonl").as_path())
        );
    }

    #[test]
    #[cfg(unix)]
    fn agy_discovery_does_not_follow_transcripts_outside_its_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("brain");
        let conv_dir = dir.path().join("conversations");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&conv_dir).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let session = root.join("abc-123");
        fs::create_dir_all(&session).unwrap();
        let logs = outside.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"type":"USER_INPUT","content":"outside"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), session.join(".system_generated")).unwrap();
        let outside_db = outside.path().join("outside.db");
        fs::write(&outside_db, b"").unwrap();
        std::os::unix::fs::symlink(&outside_db, conv_dir.join("abc-123.db")).unwrap();
        if !crate::config::sqlite_unsupported_in_temp_dir() {
            let db =
                rusqlite::Connection::open(dir.path().join("conversation_summaries.db")).unwrap();
            db.execute_batch("CREATE TABLE conversation_summaries(conversation_id TEXT,title TEXT,workspace_uris TEXT,preview TEXT,last_modified_time TEXT);").unwrap();
            db.execute(
                "INSERT INTO conversation_summaries VALUES ('abc-123','Outside','[]','hi','2026-10-13T17:26:12Z')",
                [],
            )
            .unwrap();
        }
        assert!(scan_root(&root, None).is_empty());
    }

    #[test]
    fn agy_shows_only_the_typed_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("abc-123/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let file = logs.join("transcript_full.jsonl");
        fs::write(&file, concat!(
            "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"<USER_REQUEST>\\nfix the build\\n</USER_REQUEST>\\n<ADDITIONAL_METADATA>\\nThe current local time is: 2026-10-21T08:40:35+08:00.\\n</ADDITIONAL_METADATA>\"}\n",
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"<USER_REQUEST> stays in replies\"}\n",
            "{\"step_index\":2,\"type\":\"USER_INPUT\",\"content\":\"plain prompt\"}\n",
        )).unwrap();
        assert_eq!(
            scan_root(dir.path(), None)[0].title.as_deref(),
            Some("fix the build")
        );
        let messages = load_messages(&file).unwrap();
        assert_eq!(messages[0].content, "fix the build");
        assert_eq!(messages[1].content, "<USER_REQUEST> stays in replies");
        assert_eq!(messages[2].content, "plain prompt");
    }

    #[test]
    fn agy_reads_tokens_rewritten_steps_and_partial_tail() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("abc-123/.system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let file = logs.join("transcript_full.jsonl");
        fs::write(&file, concat!(
            "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"hello\",\"created_at\":\"2026-10-13T17:26:12Z\"}\n",
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"draft\"}\n",
            "{\"step_index\":1,\"type\":\"PLANNER_RESPONSE\",\"content\":\"done\",\"input_tokens\":100,\"output_tokens\":12,\"cache_read_tokens\":20}\n{"
        )).unwrap();
        let sessions = scan_root(dir.path(), None);
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].resume_command.as_deref(),
            Some("agy --conversation=abc-123")
        );
        let private = Path::new("/home/me/.cc-switch/agy/relay");
        assert_eq!(
            scan_root(dir.path(), Some(private))[0]
                .resume_command
                .as_deref(),
            Some("agy --gemini_dir=/home/me/.cc-switch/agy/relay --conversation=abc-123")
        );
        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].content, "done");
        assert_eq!(messages[1].meta.as_ref().unwrap().input_tokens, Some(100));
        assert_eq!(
            messages[1].meta.as_ref().unwrap().cache_read_tokens,
            Some(20)
        );
    }

    #[test]
    fn agy_reconstructs_rich_blocks_subagent_events_and_content_refs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("abc-123/.system_generated/logs/transcript_full.jsonl");
        let long_thinking = "T".repeat(blocks::THINKING_PREVIEW_CHARS + 50);
        let long_output: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let long_output = long_output.trim_end();
        let long_checkpoint = "C".repeat(blocks::EVENT_PREVIEW_CHARS + 40);

        write_jsonl(
            &file,
            &[
                json!({
                    "step_index": 0, "type": "USER_INPUT",
                    "content": "<USER_REQUEST>\ninvestigate and fix\n</USER_REQUEST>",
                    "media": [
                        {"mime_type": "image/png", "data": "aGVsbG8="},
                        {"mime_type": "image/jpeg", "uri": "/tmp/shot.jpg"}
                    ]
                }),
                json!({"step_index": 1, "type": "PLANNER_RESPONSE", "thinking": "old draft"}),
                json!({
                    "step_index": 1, "type": "PLANNER_RESPONSE",
                    "thinking": long_thinking,
                    "content": "Delegating to DeepCoder and checking files.",
                    "tool_calls": [
                        {"id": "call-sub", "name": "invoke_subagent", "args": {"TypeName": "DeepCoder", "Role": "Fix parser", "Prompt": "Update agy.rs"}},
                        {"id": "call-cmd", "name": "run_command", "args": {"CommandLine": "rg -n SessionBlock src-tauri", "toolSummary": "Search SessionBlock"}},
                        {"id": "call-edit", "name": "replace_file_content", "args": {"TargetFile": "/repo/src/agy.rs", "Instruction": "Use SessionBlock", "TargetContent": "old_a\nold_b", "ReplacementContent": "new_a\nnew_b\nnew_c"}}
                    ],
                    "input_tokens": 500, "output_tokens": 120, "reasoning_tokens": 40
                }),
                json!({"step_index": 2, "type": "GENERIC", "status": "DONE", "content": "Created the following subagent:\n```json\n{\n  \"conversationId\": \"sub-conv-999\"\n}\n```"}),
                json!({"step_index": 3, "type": "GENERIC", "status": "DONE", "content": long_output}),
                json!({"step_index": 4, "type": "GENERIC", "status": "ERROR", "error": "TargetContent not found"}),
                json!({
                    "step_index": 5, "type": "SYSTEM_MESSAGE", "source": "SYSTEM_SDK",
                    "content": "The following is a <SYSTEM_MESSAGE> not actually sent by the user.\n\n<SYSTEM_MESSAGE label=\"periodic_reminder\">\nSubagent sub-conv-999 is still running.\n</SYSTEM_MESSAGE>"
                }),
                json!({
                    "step_index": 6, "type": "SYSTEM_MESSAGE", "source": "SYSTEM",
                    "content": "The following is a <SYSTEM_MESSAGE> not actually sent by the user.\n\n<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-06T03:00:00Z sender=sub-conv-999 priority=MESSAGE_PRIORITY_HIGH content=Completed parser upgrade.\n\n## 1. What I changed\n- Updated agy.rs\n</SYSTEM_MESSAGE>"
                }),
                json!({"step_index": 7, "type": "CHECKPOINT", "content": long_checkpoint, "input_tokens": 1000}),
                json!({"step_index": 8, "type": "ERROR_MESSAGE", "error": "Model quota exceeded"}),
            ],
        );

        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 9);
        let validated = validated_source(&file, dir.path());

        assert_eq!(messages[0].role, "user");
        assert_eq!(
            messages[0].blocks[0],
            SessionBlock::text("investigate and fix")
        );
        match &messages[0].blocks[1] {
            SessionBlock::Image {
                image:
                    ImageRef {
                        source: ImageSource::Inline { content },
                        media_type,
                        ..
                    },
            } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(
                    resolve_content_ref(&validated, content).unwrap(),
                    "aGVsbG8="
                );
            }
            other => panic!("unexpected block: {other:?}"),
        }

        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].blocks.len(), 5);
        match &messages[1].blocks[0] {
            SessionBlock::Thinking { full: Some(r), .. } => {
                assert_eq!(resolve_content_ref(&validated, r).unwrap(), long_thinking);
            }
            other => panic!("expected truncated thinking, got {other:?}"),
        }
        assert_eq!(
            messages[1].blocks[1],
            SessionBlock::text("Delegating to DeepCoder and checking files.")
        );
        let calls: Vec<_> = messages[1].blocks[2..].iter().map(call_tuple).collect();
        assert_eq!(
            calls,
            [
                (
                    "call-sub",
                    ToolKind::Agent,
                    "Fix parser",
                    Some("DeepCoder"),
                    None,
                    None
                ),
                (
                    "call-cmd",
                    ToolKind::Search,
                    "rg -n SessionBlock src-tauri",
                    Some("Search SessionBlock"),
                    None,
                    None
                ),
                (
                    "call-edit",
                    ToolKind::Edit,
                    "/repo/src/agy.rs",
                    Some("Use SessionBlock"),
                    None,
                    Some((3, 2))
                ),
            ]
        );

        let r0 = result_tuple(&messages[2]);
        let r1 = result_tuple(&messages[3]);
        let r2 = result_tuple(&messages[4]);
        assert_eq!((r0.0, r0.1), ("call-sub", ToolStatus::Success));
        assert_eq!((r1.0, r1.1, r1.2), ("call-cmd", ToolStatus::Success, true));
        assert_eq!(
            resolve_content_ref(&validated, r1.4.unwrap()).unwrap(),
            long_output
        );
        assert_eq!(
            (r2.0, r2.1, r2.3),
            ("call-edit", ToolStatus::Error, "TargetContent not found")
        );

        assert!(messages[5].injected);
        assert_eq!(
            messages[5].blocks,
            vec![SessionBlock::text(
                "Subagent sub-conv-999 is still running."
            )]
        );
        assert!(!messages[6].injected);
        assert_eq!(
            messages[6].blocks,
            vec![
                SessionBlock::event(
                    EventKind::SubAgent,
                    Some("DeepCoder: Completed parser upgrade.".into()),
                    None,
                ),
                SessionBlock::text("## 1. What I changed\n- Updated agy.rs"),
            ]
        );
        match &messages[7].blocks[0] {
            SessionBlock::Event {
                kind: EventKind::Compaction,
                full: Some(r @ ContentRef::Jsonl { pointer, .. }),
                ..
            } => {
                assert_eq!(pointer, "/content");
                assert_eq!(resolve_content_ref(&validated, r).unwrap(), long_checkpoint);
            }
            other => panic!("expected Compaction event, got {other:?}"),
        }
        assert_eq!(
            messages[8].blocks,
            vec![SessionBlock::event(
                EventKind::Error,
                Some("Model quota exceeded".into()),
                None,
            )]
        );
    }

    #[test]
    fn agy_maps_all_tool_kinds_and_system_message_variants() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("abc-456/.system_generated/logs/transcript_full.jsonl");
        let large_code = "fn main() {}\n".repeat(40);

        write_jsonl(
            &file,
            &[
                json!({
                    "step_index": 0, "type": "PLANNER_RESPONSE",
                    "tool_calls": [
                        {"name": "view_file", "args": {"AbsolutePath": "/a/b.rs", "StartLine": 10, "EndLine": 25}},
                        {"name": "view_code_item", "args": {"File": "/a/b.rs", "NodePaths": ["Foo.bar", "Foo.baz"]}},
                        {"name": "multi_replace_file_content", "args": {
                            "TargetFile": "/a/b.rs", "Instruction": "Two edits",
                            "ReplacementChunks": [
                                {"TargetContent": "a", "ReplacementContent": "b\nc"},
                                {"TargetContent": "x\ny", "ReplacementContent": "z"}
                            ]
                        }},
                        {"name": "write_to_file", "args": {"TargetFile": "/a/new.rs", "Description": "Create file", "CodeContent": large_code}},
                        {"name": "grep_search", "args": {"Query": "SessionBlock", "SearchPath": "/a"}},
                        {"name": "list_dir", "args": {"DirectoryPath": "/a/src"}},
                        {"name": "search_web", "args": {"query": "rust serde", "domain": "docs.rs"}},
                        {"name": "ask_question", "args": {"questions": [{"question": "Proceed?"}], "Reason": "Confirm"}},
                        {"name": "call_mcp_tool", "args": {"ServerName": "github", "ToolName": "get_issue", "Arguments": {"repo": "cc-switch"}}},
                        {"name": "read_resource", "args": {"ServerName": "docs", "Uri": "doc://1"}},
                        {"name": "send_message", "args": {"Recipient": "parent-1", "Message": "Done\nDetails"}},
                        {"name": "manage_subagents", "args": {"Action": "list", "Target": "all"}},
                        {"name": "manage_task", "args": {"Action": "kill", "TaskId": "t-1"}},
                        {"name": "webview_browser", "args": {"intention": "Open PR page", "observation": "Check checks", "script": "await page.goto('https://example.com')"}},
                        {"name": "read_board", "args": {"since": 12}},
                        {"name": "swarm", "args": {"action": "status"}},
                        {"name": "post_message", "args": {"text": "@coord done"}},
                        {"name": "search_marketplace", "args": {"kind": "plugin", "queries": ["gdrive", "cloud"]}},
                        {"name": "generate_image", "args": {"AspectRatio": "16:9", "ImageName": "hero", "Prompt": "A diagram"}}
                    ]
                }),
                json!({
                    "step_index": 1, "type": "GENERIC", "status": "RUNNING",
                    "content": "still running",
                    "media": [{"mime_type": "image/png", "inline_data": "aGVsbG8="}]
                }),
                json!({
                    "step_index": 2, "type": "SYSTEM_MESSAGE", "source": "SYSTEM",
                    "content": "<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-06T03:00:00Z sender=conv-1/task-65 priority=HIGH content=Task finished\nExit code 0\n</SYSTEM_MESSAGE>"
                }),
                json!({
                    "step_index": 3, "type": "SYSTEM_MESSAGE", "source": "SYSTEM",
                    "content": "<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-06T03:00:00Z sender=abcdef123456 priority=HIGH content=Fallback subagent label\n</SYSTEM_MESSAGE>"
                }),
            ],
        );

        let validated = validated_source(&file, dir.path());
        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 4);
        let calls = &messages[0].blocks;
        let summary: Vec<_> = calls
            .iter()
            .map(call_tuple)
            .map(|(_, k, t, d, s, diff)| (k, t, d, s, diff))
            .collect();
        assert_eq!(
            summary,
            [
                (ToolKind::Read, "/a/b.rs", Some(":10-25"), None, None),
                (
                    ToolKind::Read,
                    "/a/b.rs",
                    Some("Foo.bar, Foo.baz"),
                    None,
                    None
                ),
                (
                    ToolKind::Edit,
                    "/a/b.rs",
                    Some("Two edits"),
                    None,
                    Some((3, 3))
                ),
                (
                    ToolKind::Write,
                    "/a/new.rs",
                    Some("Create file"),
                    None,
                    Some((40, 0))
                ),
                (ToolKind::Search, "SessionBlock", Some("/a"), None, None),
                (ToolKind::Search, "/a/src", None, None, None),
                (ToolKind::Web, "rust serde", Some("docs.rs"), None, None),
                (ToolKind::Ask, "Proceed?", Some("Confirm"), None, None),
                (
                    ToolKind::Mcp,
                    "github.get_issue",
                    Some("cc-switch"),
                    Some("github"),
                    None
                ),
                (
                    ToolKind::Mcp,
                    "docs.read_resource",
                    Some("doc://1"),
                    Some("docs"),
                    None
                ),
                (ToolKind::Agent, "parent-1", Some("Done…"), None, None),
                (ToolKind::Agent, "list", Some("all"), None, None),
                (ToolKind::Shell, "t-1", Some("kill"), None, None),
                (
                    ToolKind::Web,
                    "Open PR page",
                    Some("Check checks"),
                    None,
                    None
                ),
                (ToolKind::Agent, "read_board", Some("since 12"), None, None),
                (ToolKind::Agent, "status", None, None, None),
                (
                    ToolKind::Agent,
                    "post_message",
                    Some("@coord done"),
                    None,
                    None
                ),
                (
                    ToolKind::Search,
                    "gdrive, cloud",
                    Some("plugin"),
                    None,
                    None
                ),
                (ToolKind::Other, "generate_image", Some("hero"), None, None),
            ]
        );
        match &calls[3] {
            SessionBlock::ToolCall {
                input_full: Some(r),
                ..
            } => assert!(resolve_content_ref(&validated, r)
                .unwrap()
                .contains("fn main() {}")),
            other => panic!("unexpected: {other:?}"),
        }
        let r = result_tuple(&messages[1]);
        assert_eq!((r.0, r.1, r.5), ("step-0-call-0", ToolStatus::Pending, 1));
        assert_eq!(
            messages[2].blocks,
            vec![
                SessionBlock::event(EventKind::Info, Some("Task finished".into()), None),
                SessionBlock::text("Exit code 0"),
            ]
        );
        assert_eq!(
            messages[3].blocks,
            vec![SessionBlock::event(
                EventKind::SubAgent,
                Some("subagent-abcdef12: Fallback subagent label".into()),
                None,
            )]
        );
    }

    #[test]
    fn agy_handles_interrupted_tool_batches_long_replies_and_single_underscore_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("abc-789/.system_generated/logs/transcript_full.jsonl");
        let long_reply = "R".repeat(blocks::INLINE_TEXT_MAX_CHARS + 500);

        write_jsonl(
            &file,
            &[
                json!({
                    "step_index": 0, "type": "PLANNER_RESPONSE",
                    "tool_calls": [
                        {"id": "call-sub-1", "name": "invoke_subagent", "args": {"TypeName": "DeepCoder", "Role": "Worker"}},
                        {"id": "call-cancelled-1", "name": "run_command", "args": {"CommandLine": "echo cancelled"}}
                    ]
                }),
                json!({"step_index": 1, "type": "GENERIC", "status": "DONE", "content": "Created the following subagent:\n{\n  \"conversationId\": \"sub-conv-1\"\n}"}),
                json!({"step_index": 3, "type": "USER_INPUT", "content": "continue"}),
                json!({
                    "step_index": 4, "type": "PLANNER_RESPONSE",
                    "tool_calls": [{"id": "call-cancelled-2", "name": "write_file", "args": {"Path": "/tmp/a", "Content": "x"}}]
                }),
                json!({"step_index": 5, "type": "ERROR_MESSAGE", "error": "The stream was interrupted."}),
                json!({
                    "step_index": 6, "type": "PLANNER_RESPONSE",
                    "content": long_reply,
                    "tool_calls": [{
                        "id": "call-mcp-1",
                        "name": "mcp_gemini-api_gemini-api-docs_gemini_search_docs",
                        "args": {"query": "computer use", "toolSummary": "Search Gemini docs"}
                    }]
                }),
                json!({
                    "step_index": 7, "type": "GENERIC", "status": "DONE",
                    "content": "Command output:\nCreated the following subagent:\n{\n  \"conversationId\": \"sub-conv-1\"\n}"
                }),
                json!({
                    "step_index": 8, "type": "SYSTEM_MESSAGE", "source": "SYSTEM",
                    "content": "<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-06T03:00:00Z sender=sub-conv-1 priority=HIGH content=Finished work.\n</SYSTEM_MESSAGE>"
                }),
            ],
        );

        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 8);
        assert_eq!(
            result_tuple(&messages[4]),
            (
                "call-cancelled-2",
                ToolStatus::Error,
                false,
                "The stream was interrupted.",
                None,
                0,
            )
        );
        assert_eq!(messages[5].blocks[0], SessionBlock::text(long_reply));
        assert_eq!(
            call_tuple(&messages[5].blocks[1]),
            (
                "call-mcp-1",
                ToolKind::Mcp,
                "gemini-api.gemini-api-docs_gemini_search_docs",
                Some("Search Gemini docs"),
                Some("gemini-api"),
                None,
            )
        );
        assert_eq!(result_tuple(&messages[6]).0, "call-mcp-1");
        assert_eq!(
            messages[7].blocks,
            vec![SessionBlock::event(
                EventKind::SubAgent,
                Some("DeepCoder: Finished work.".into()),
                None,
            )]
        );
    }

    #[test]
    fn agy_reads_wal_databases_without_shm_and_handles_busy_summaries_and_image_only_titles() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("antigravity");
        let summaries_db = root.join("conversation_summaries.db");
        fs::create_dir_all(root.join("conversations")).unwrap();

        let main_id = "11111111-1111-1111-1111-111111111111";
        let btw_id = "22222222-2222-2222-2222-222222222222";
        let img_id = "33333333-3333-3333-3333-333333333333";
        let img_project = dir.path().join("my-image-app");
        let img_workspace_uris =
            serde_json::to_string(&[url::Url::from_file_path(&img_project).unwrap().to_string()])
                .unwrap();

        {
            let conn = rusqlite::Connection::open(&summaries_db).unwrap();
            let _: String = conn
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .unwrap();
            conn.execute_batch(
                "CREATE TABLE conversation_summaries(
                    conversation_id TEXT,
                    title TEXT,
                    workspace_uris TEXT,
                    preview TEXT,
                    last_modified_time TEXT
                );",
            )
            .unwrap();
            for (id, title, ws, preview) in [
                (main_id, "WAL Main Session", "[]", "Hello WAL"),
                (btw_id, "WAL Side Question", "[]", "/btw quick check"),
                (img_id, "", img_workspace_uris.as_str(), ""),
            ] {
                conn.execute(
                    "INSERT INTO conversation_summaries VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id, title, ws, preview, "2026-11-08T01:00:00Z"],
                )
                .unwrap();
            }
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        let _ = fs::remove_file(root.join("conversation_summaries.db-shm"));
        let _ = fs::remove_file(root.join("conversation_summaries.db-wal"));

        let btw_db = root.join("conversations").join(format!("{btw_id}.db"));
        {
            let conn = rusqlite::Connection::open(&btw_db).unwrap();
            let _: String = conn
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .unwrap();
            conn.execute_batch(
                "CREATE TABLE trajectory_meta(trajectory_id TEXT, trajectory_type INTEGER);
                 INSERT INTO trajectory_meta VALUES ('22222222-2222-2222-2222-222222222222', 22);",
            )
            .unwrap();
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        let _ = fs::remove_file(root.join("conversations").join(format!("{btw_id}.db-shm")));
        let _ = fs::remove_file(root.join("conversations").join(format!("{btw_id}.db-wal")));

        for id in [main_id, img_id] {
            fs::write(root.join("conversations").join(format!("{id}.db")), b"x").unwrap();
        }

        write_jsonl(
            &root
                .join("brain")
                .join(main_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            &[json!({
                "step_index": 0, "type": "USER_INPUT",
                "created_at": "2026-11-08T01:00:00Z",
                "content": "<USER_REQUEST>\nHello WAL\n</USER_REQUEST>"
            })],
        );
        write_jsonl(
            &root
                .join("brain")
                .join(btw_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            &[json!({
                "step_index": 0, "type": "USER_INPUT",
                "created_at": "2026-11-08T01:01:00Z",
                "content": "<USER_REQUEST>\n/btw quick check\n</USER_REQUEST>"
            })],
        );
        write_jsonl(
            &root
                .join("brain")
                .join(img_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            &[json!({
                "step_index": 0, "type": "USER_INPUT",
                "created_at": "2026-11-08T01:02:00Z",
                "content": "<USER_REQUEST>\n</USER_REQUEST>",
                "media": [{"mime_type": "image/png", "uri": "file:///tmp/screenshot.png"}]
            })],
        );

        let sessions = scan_root(&root.join("brain"), None);
        let by_id: std::collections::HashMap<&str, &SessionMeta> = sessions
            .iter()
            .map(|s| (s.session_id.as_str(), s))
            .collect();
        assert_eq!(by_id.len(), 2);
        assert_eq!(
            by_id.get(main_id).and_then(|s| s.title.as_deref()),
            Some("WAL Main Session")
        );
        assert_eq!(
            by_id.get(img_id).and_then(|s| s.title.as_deref()),
            Some("my-image-app")
        );
        assert!(!by_id.contains_key(btw_id));

        // Simulate SQLITE_BUSY on conversation_summaries.db: transcripts must still be discovered
        // instead of being treated as subagents because index.has_db remains false.
        let busy_root = dir.path().join("busy_antigravity");
        fs::create_dir_all(&busy_root).unwrap();
        let busy_db = busy_root.join("conversation_summaries.db");
        let writer = rusqlite::Connection::open(&busy_db).unwrap();
        writer
            .execute_batch(
                "CREATE TABLE conversation_summaries(
                    conversation_id TEXT,
                    title TEXT,
                    workspace_uris TEXT,
                    preview TEXT,
                    last_modified_time TEXT
                );
                BEGIN EXCLUSIVE;
                INSERT INTO conversation_summaries VALUES ('x', 'y', '[]', 'p', '2026-11-08T02:00:00Z');",
            )
            .unwrap();

        write_jsonl(
            &busy_root
                .join("brain")
                .join(main_id)
                .join(".system_generated/logs/transcript_full.jsonl"),
            &[json!({
                "step_index": 0, "type": "USER_INPUT",
                "created_at": "2026-11-08T02:00:00Z",
                "content": "<USER_REQUEST>\nStill visible when DB is busy\n</USER_REQUEST>"
            })],
        );

        let busy_sessions = scan_root(&busy_root.join("brain"), None);
        assert_eq!(busy_sessions.len(), 1);
        assert_eq!(busy_sessions[0].session_id, main_id);
        assert_eq!(
            busy_sessions[0].title.as_deref(),
            Some("Still visible when DB is busy")
        );
    }

    #[test]
    fn agy_pairs_typed_tool_results_error_steps_cancelled_status_and_command_exit_codes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("abc-999/.system_generated/logs/transcript_full.jsonl");

        write_jsonl(
            &file,
            &[
                json!({
                    "step_index": 0, "type": "PLANNER_RESPONSE",
                    "tool_calls": [
                        {"id": "call-err-1", "name": "read_file", "args": {"Path": "/missing"}},
                        {"id": "call-ok-2", "name": "run_command", "args": {"CommandLine": "false"}},
                        {"id": "call-cancel-3", "name": "run_shell_command", "args": {"command": "sleep 10"}},
                        {"id": "call-typed-4", "name": "run_command", "args": {"CommandLine": "echo hi"}}
                    ]
                }),
                json!({"step_index": 1, "type": "ERROR_MESSAGE", "error": "file not found"}),
                json!({
                    "step_index": 2, "type": "GENERIC", "status": "DONE",
                    "content": "Created At: 2026-11-08T01:00:00Z\nCommand exited with code 2.\nOutput:\nfailed"
                }),
                json!({
                    "step_index": 3, "type": "GENERIC", "status": "CANCELLED",
                    "content": "Command cancelled by user"
                }),
                json!({
                    "step_index": 4, "type": "RUN_COMMAND", "source": "MODEL", "status": "DONE",
                    "content": "Created At: 2026-11-08T01:00:01Z\nThe command completed successfully.\nOutput:\nhi"
                }),
            ],
        );

        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(
            result_tuple(&messages[1]),
            (
                "call-err-1",
                ToolStatus::Error,
                false,
                "file not found",
                None,
                0,
            )
        );
        assert_eq!(
            result_tuple(&messages[2]),
            (
                "call-ok-2",
                ToolStatus::Error,
                false,
                "Created At: 2026-11-08T01:00:00Z\nCommand exited with code 2.\nOutput:\nfailed",
                None,
                0,
            )
        );
        match &messages[2].blocks[0] {
            SessionBlock::ToolResult { exit_code, .. } => assert_eq!(*exit_code, Some(2)),
            other => panic!("expected ToolResult, got {other:?}"),
        }
        assert_eq!(
            result_tuple(&messages[3]),
            (
                "call-cancel-3",
                ToolStatus::Interrupted,
                false,
                "Command cancelled by user",
                None,
                0,
            )
        );
        assert_eq!(
            result_tuple(&messages[4]),
            (
                "call-typed-4",
                ToolStatus::Success,
                false,
                "Created At: 2026-11-08T01:00:01Z\nThe command completed successfully.\nOutput:\nhi",
                None,
                0,
            )
        );
        match &messages[4].blocks[0] {
            SessionBlock::ToolResult { exit_code, .. } => assert_eq!(*exit_code, Some(0)),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn agy_preserves_structured_results_invalid_status_and_inherited_model() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("transcript_full.jsonl");
        let long = "output line\n".repeat(300);
        write_jsonl(
            &file,
            &[
                json!({
                    "step_index": 0, "type": "USER_INPUT",
                    "content": "<USER_REQUEST>\ncheck\n</USER_REQUEST>\n<USER_SETTINGS_CHANGE>\nThe user changed setting `Model Selection` from None to Gemini 4 Argon (High). No need to comment.\n</USER_SETTINGS_CHANGE>"
                }),
                json!({"step_index": 1, "type": "PLANNER_RESPONSE", "tool_calls": [
                    {"id": "a", "name": "read_file", "args": {}},
                    {"id": "b", "name": "read_file", "args": {}},
                    {"id": "c", "name": "run_command", "args": {}}
                ]}),
                json!({"step_index": 2, "type": "GENERIC", "status": "DONE", "content": {"text": long}}),
                json!({"step_index": 3, "type": "GENERIC", "status": "NEW_STATE", "content": ["first", {"text": "second"}]}),
                json!({"step_index": 4, "type": "GENERIC", "status": "INVALID", "content": "Error invalid tool call: missing WaitMsBeforeAsync"}),
            ],
        );
        let messages = load_messages(&file).unwrap();
        assert_eq!(
            messages[1].meta.as_ref().and_then(|m| m.model.as_deref()),
            Some("Gemini 4 Argon (High)")
        );
        let result = result_tuple(&messages[2]);
        assert_eq!(result.1, ToolStatus::Success);
        assert!(result.2);
        let source = validated_source(&file, dir.path());
        assert_eq!(
            resolve_content_ref(&source, result.4.unwrap()).unwrap(),
            long
        );
        let result = result_tuple(&messages[3]);
        assert_eq!((result.1, result.3), (ToolStatus::Unknown, "first\nsecond"));
        let invalid = result_tuple(&messages[4]);
        assert_eq!((invalid.0, invalid.1), ("c", ToolStatus::Error));
    }

    #[test]
    fn agy_omits_incomplete_multi_edit_counts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("transcript_full.jsonl");
        write_jsonl(
            &file,
            &[
                json!({"step_index": 0, "type": "PLANNER_RESPONSE", "tool_calls": [
                    {"id": "edit", "name": "multi_replace_file_content", "args": {
                        "TargetFile": "/tmp/example", "ReplacementChunks": [
                            {"TargetContent": "old", "ReplacementContent": "new"},
                            {"TargetContent": "missing replacement"}
                        ]
                    }}
                ]}),
            ],
        );
        let messages = load_messages(&file).unwrap();
        assert_eq!(call_tuple(&messages[0].blocks[0]).5, None);
    }

    fn len_field(field: u8, payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 128);
        let mut out = vec![(field << 3) | 2, payload.len() as u8];
        out.extend_from_slice(payload);
        out
    }

    fn jetbox_entry(id: &str, summary: &[u8]) -> Vec<u8> {
        len_field(
            1,
            &[len_field(1, id.as_bytes()), len_field(2, summary)].concat(),
        )
    }

    #[test]
    fn agy_jetbox_drops_only_the_entry_keyed_by_the_session() {
        let parent = jetbox_entry("aaa-1", b"title");
        // A child summary mentions its parent's id; only the key decides which entry goes.
        let child = jetbox_entry("bbb-2", b"parent aaa-1");
        let file = [parent.clone(), child.clone()].concat();

        assert_eq!(jetbox_without(&file, "aaa-1").unwrap(), Some(child.clone()));
        assert_eq!(jetbox_without(&file, "bbb-2").unwrap(), Some(parent));
        assert_eq!(jetbox_without(&file, "ccc-3").unwrap(), None);
        assert_eq!(jetbox_without(&[], "aaa-1").unwrap(), None);
    }

    #[test]
    fn agy_jetbox_refuses_layouts_it_does_not_know() {
        let entry = jetbox_entry("aaa-1", b"title");
        for file in [
            [entry.clone(), len_field(2, b"x")].concat(), // another top-level field
            [entry.clone(), vec![0x08, 0x01]].concat(),   // a varint at top level
            len_field(1, &len_field(2, b"no key first")), // entry without a key
            entry[..entry.len() - 1].to_vec(),            // truncated
        ] {
            assert!(jetbox_without(&file, "aaa-1").is_err());
        }
    }

    struct AgyFixture {
        _home: tempfile::TempDir,
        app: PathBuf,
    }

    impl AgyFixture {
        fn new(dir_name: &str, ids: &[&str]) -> Self {
            let home = tempfile::tempdir().unwrap();
            let app = home.path().join(dir_name);
            for sub in ["conversations", "annotations", "presence"] {
                fs::create_dir_all(app.join(sub)).unwrap();
            }
            let db = rusqlite::Connection::open(app.join("conversation_summaries.db")).unwrap();
            db.execute_batch(
                "CREATE TABLE conversation_summaries(conversation_id TEXT PRIMARY KEY, title TEXT);
                 CREATE VIRTUAL TABLE conversations_fts USING fts5(conversation_id UNINDEXED, title);
                 CREATE TABLE conversation_parents(conversation_id TEXT PRIMARY KEY, parent_id TEXT NOT NULL) WITHOUT ROWID;",
            )
            .unwrap();
            let mut jetbox = Vec::new();
            for id in ids {
                let logs = app.join("brain").join(id).join(".system_generated/logs");
                fs::create_dir_all(&logs).unwrap();
                fs::write(logs.join("transcript_full.jsonl"), "{}\n").unwrap();
                fs::write(app.join("conversations").join(format!("{id}.db")), "db").unwrap();
                fs::write(app.join("annotations").join(format!("{id}.pbtxt")), "title").unwrap();
                fs::write(app.join("presence").join(format!("{id}.lock")), "").unwrap();
                db.execute("INSERT INTO conversation_summaries VALUES (?1, 't')", [id])
                    .unwrap();
                db.execute("INSERT INTO conversations_fts VALUES (?1, 't')", [id])
                    .unwrap();
                jetbox.extend(jetbox_entry(id, b"summary"));
            }
            fs::write(app.join("jetbox_summaries_proto.pb"), jetbox).unwrap();
            Self { _home: home, app }
        }

        fn delete(&self, id: &str) -> Result<bool, String> {
            let root = self.app.join("brain").canonicalize().unwrap();
            let path = root
                .join(id)
                .join(".system_generated/logs/transcript_full.jsonl");
            delete_session(&root, &path.canonicalize().unwrap(), id)
        }

        fn rows(&self, table: &str, id: &str) -> i64 {
            rusqlite::Connection::open(self.app.join("conversation_summaries.db"))
                .unwrap()
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE conversation_id = ?1"),
                    [id],
                    |row| row.get(0),
                )
                .unwrap()
        }

        fn traces(&self, id: &str) -> Vec<PathBuf> {
            [
                self.app.join("brain").join(id),
                self.app.join("conversations").join(format!("{id}.db")),
                self.app.join("annotations").join(format!("{id}.pbtxt")),
                self.app.join("presence").join(format!("{id}.lock")),
            ]
            .into_iter()
            .filter(|path| path.exists())
            .collect()
        }

        fn jetbox(&self) -> Vec<u8> {
            fs::read(self.app.join("jetbox_summaries_proto.pb")).unwrap()
        }
    }

    #[test]
    fn agy_delete_removes_index_entries_and_files_of_one_cli_conversation() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let fixture = AgyFixture::new("antigravity-cli", &["aaa-1", "bbb-2"]);

        assert_eq!(fixture.delete("aaa-1"), Ok(true));

        assert!(fixture.traces("aaa-1").is_empty());
        assert_eq!(fixture.rows("conversation_summaries", "aaa-1"), 0);
        assert_eq!(fixture.rows("conversations_fts", "aaa-1"), 0);
        assert_eq!(fixture.jetbox(), jetbox_entry("bbb-2", b"summary"));
        assert_eq!(fixture.traces("bbb-2").len(), 4);
        assert_eq!(fixture.rows("conversation_summaries", "bbb-2"), 1);
        assert_eq!(fixture.rows("conversations_fts", "bbb-2"), 1);
    }

    #[test]
    fn agy_delete_refuses_open_ide_and_unrecognized_conversations_without_changes() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let fixture = AgyFixture::new("antigravity-cli", &["aaa-1"]);
        let held = fs::File::open(fixture.app.join("presence/aaa-1.lock")).unwrap();
        held.lock().unwrap();
        assert!(fixture.delete("aaa-1").unwrap_err().contains("open in agy"));
        drop(held);
        assert_eq!(fixture.traces("aaa-1").len(), 4);

        let original = fixture.jetbox();
        fs::write(
            fixture.app.join("jetbox_summaries_proto.pb"),
            [original.clone(), vec![0x08, 0x01]].concat(),
        )
        .unwrap();
        assert!(fixture.delete("aaa-1").is_err());
        assert_eq!(fixture.traces("aaa-1").len(), 4);
        assert_eq!(fixture.rows("conversation_summaries", "aaa-1"), 1);

        let ide = AgyFixture::new("antigravity", &["aaa-1"]);
        assert!(ide.delete("aaa-1").is_err());
        assert_eq!(ide.traces("aaa-1").len(), 4);
    }

    #[test]
    fn agy_trajectory_cache_normalizes_keys_and_evicts_deleted_sessions() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let fixture = AgyFixture::new("antigravity-cli", &["aaa-1", "bbb-2", "ccc-3"]);
        {
            let conn =
                rusqlite::Connection::open(fixture.app.join("conversation_summaries.db")).unwrap();
            conn.execute_batch(
                "DROP TABLE conversation_summaries;
                 CREATE TABLE conversation_summaries(
                     conversation_id TEXT PRIMARY KEY,
                     title TEXT,
                     workspace_uris TEXT,
                     preview TEXT,
                     last_modified_time TEXT,
                     step_count INTEGER,
                     nesting_depth INTEGER,
                     agent_name TEXT
                 );
                 INSERT INTO conversation_summaries VALUES
                     ('aaa-1', 'Session A', '[]', 'hi', '2026-11-08T01:00:00Z', 2, 0, ''),
                     ('bbb-2', 'Session B', '[]', 'hi', '2026-11-08T01:01:00Z', 2, 0, ''),
                     ('ccc-3', 'Side C', '[]', '/btw', '2026-11-08T01:02:00Z', 2, 0, '');",
            )
            .unwrap();
        }
        for (id, trajectory_type) in [("aaa-1", 4), ("bbb-2", 4), ("ccc-3", 22)] {
            let db_path = fixture.app.join("conversations").join(format!("{id}.db"));
            fs::remove_file(&db_path).unwrap();
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(&format!(
                "CREATE TABLE trajectory_meta(trajectory_id TEXT, trajectory_type INTEGER);
                 INSERT INTO trajectory_meta VALUES ('{id}', {trajectory_type});"
            ))
            .unwrap();
        }
        let key_a = fixture
            .app
            .join("conversations/aaa-1.db")
            .canonicalize()
            .unwrap();
        let key_b = fixture
            .app
            .join("conversations/bbb-2.db")
            .canonicalize()
            .unwrap();
        let key_c = fixture
            .app
            .join("conversations/ccc-3.db")
            .canonicalize()
            .unwrap();

        #[cfg(unix)]
        let (scan_brain, raw_symlink_key) = {
            let link_dir = tempfile::tempdir().unwrap();
            let link_app = link_dir.path().join("antigravity-cli");
            std::os::unix::fs::symlink(&fixture.app, &link_app).unwrap();
            let raw_key = link_app.join("conversations/aaa-1.db");
            (link_app.join("brain"), Some((link_dir, raw_key)))
        };
        #[cfg(not(unix))]
        let (scan_brain, raw_symlink_key): (PathBuf, Option<(tempfile::TempDir, PathBuf)>) =
            (fixture.app.join("brain"), None);

        let mut sessions = scan_root(&scan_brain, None);
        sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, ["aaa-1", "bbb-2"]);
        {
            let cache = TRAJECTORY_CACHE.lock_recover();
            assert_eq!(cache.get(&key_a).map(|&(_, v)| v), Some(false));
            assert_eq!(cache.get(&key_b).map(|&(_, v)| v), Some(false));
            assert_eq!(cache.get(&key_c).map(|&(_, v)| v), Some(true));
            if let Some((_, ref raw_key)) = raw_symlink_key {
                assert!(!cache.contains_key(raw_key));
            }
        }

        // Scanning an unrelated root must not evict this root's cached entries.
        let other = tempfile::tempdir().unwrap();
        fs::create_dir_all(other.path().join("brain")).unwrap();
        assert!(scan_root(&other.path().join("brain"), None).is_empty());
        assert!(TRAJECTORY_CACHE.lock_recover().contains_key(&key_a));

        // delete_session receives canonicalized paths (mod.rs resolve_under_roots) and must
        // evict the entry even when scan_root originally walked an uncanonicalized/symlinked root.
        assert_eq!(fixture.delete("aaa-1"), Ok(true));
        {
            let cache = TRAJECTORY_CACHE.lock_recover();
            assert!(!cache.contains_key(&key_a));
            assert!(cache.contains_key(&key_b));
            assert!(cache.contains_key(&key_c));
        }

        // External deletion in agy: next scan_root prunes the stale entry for bbb-2.
        {
            let conn =
                rusqlite::Connection::open(fixture.app.join("conversation_summaries.db")).unwrap();
            conn.execute(
                "DELETE FROM conversation_summaries WHERE conversation_id = 'bbb-2'",
                [],
            )
            .unwrap();
        }
        fs::remove_file(fixture.app.join("conversations/bbb-2.db")).unwrap();
        fs::remove_dir_all(fixture.app.join("brain/bbb-2")).unwrap();
        assert!(scan_root(&scan_brain, None).is_empty());
        {
            let cache = TRAJECTORY_CACHE.lock_recover();
            assert!(!cache.contains_key(&key_b));
            assert!(cache.contains_key(&key_c));
        }

        // Even if conversations/<id>.db was already deleted externally before delete_session,
        // trajectory_cache_key still canonicalizes the parent directory to hit the cached key.
        fs::remove_file(fixture.app.join("conversations/ccc-3.db")).unwrap();
        assert_eq!(fixture.delete("ccc-3"), Ok(true));
        assert!(!TRAJECTORY_CACHE.lock_recover().contains_key(&key_c));

        // Re-populate without conversation_summaries.db and verify filesystem-only scan prunes too.
        let fs_db = fixture.app.join("conversations/ddd-4.db");
        {
            let conn = rusqlite::Connection::open(&fs_db).unwrap();
            conn.execute_batch(
                "CREATE TABLE trajectory_meta(trajectory_id TEXT, trajectory_type INTEGER);
                 INSERT INTO trajectory_meta VALUES ('ddd-4', 22);",
            )
            .unwrap();
        }
        write_jsonl(
            &fixture
                .app
                .join("brain/ddd-4/.system_generated/logs/transcript_full.jsonl"),
            &[json!({"step_index": 0, "type": "USER_INPUT", "content": "/btw side"})],
        );
        fs::remove_file(fixture.app.join("conversation_summaries.db")).unwrap();
        let key_d = fs_db.canonicalize().unwrap();
        assert!(scan_root(&scan_brain, None).is_empty());
        assert_eq!(
            TRAJECTORY_CACHE.lock_recover().get(&key_d).map(|&(_, v)| v),
            Some(true)
        );
        fs::remove_file(&fs_db).unwrap();
        assert_eq!(scan_root(&scan_brain, None).len(), 1);
        assert!(!TRAJECTORY_CACHE.lock_recover().contains_key(&key_d));

        // WAL-only writes update `-wal` without touching `.db`; cache stamp must notice.
        let wal_db = fixture.app.join("conversations/wal-upd.db");
        {
            let conn = rusqlite::Connection::open(&wal_db).unwrap();
            let _: String = conn
                .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
                .unwrap();
            conn.execute_batch(
                "PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE trajectory_meta(trajectory_id TEXT, trajectory_type INTEGER);
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
            let wal_key = wal_db.canonicalize().unwrap();
            assert!(!is_non_main_trajectory(&wal_key));
            assert_eq!(
                TRAJECTORY_CACHE
                    .lock_recover()
                    .get(&wal_key)
                    .map(|&(_, v)| v),
                Some(false)
            );
            conn.execute("INSERT INTO trajectory_meta VALUES ('wal-upd', 22)", [])
                .unwrap();
            assert!(is_non_main_trajectory(&wal_key));
            assert_eq!(
                TRAJECTORY_CACHE
                    .lock_recover()
                    .get(&wal_key)
                    .map(|&(_, v)| v),
                Some(true)
            );
        }

        // Corrupt non-SQLite DB returns None from open_db and must not poison TRAJECTORY_CACHE,
        // while a valid SQLite DB missing `trajectory_meta` caches `false`.
        let corrupt_db = fixture.app.join("conversations/corrupt-1.db");
        fs::write(&corrupt_db, b"not a sqlite database").unwrap();
        let corrupt_key = corrupt_db.canonicalize().unwrap();
        assert!(open_db(&corrupt_key).is_none());
        assert!(!is_non_main_trajectory(&corrupt_key));
        assert!(!TRAJECTORY_CACHE.lock_recover().contains_key(&corrupt_key));

        let empty_schema_db = fixture.app.join("conversations/empty-schema.db");
        {
            let conn = rusqlite::Connection::open(&empty_schema_db).unwrap();
            conn.execute_batch("CREATE TABLE unrelated(id INTEGER);")
                .unwrap();
        }
        let empty_schema_key = empty_schema_db.canonicalize().unwrap();
        assert!(!is_non_main_trajectory(&empty_schema_key));
        assert_eq!(
            TRAJECTORY_CACHE
                .lock_recover()
                .get(&empty_schema_key)
                .map(|&(_, v)| v),
            Some(false)
        );

        // Degraded scan (unreadable conversation_summaries.db -> !index.has_db):
        // deleted files are still pruned while existing binary-only files stay cached.
        fs::write(fixture.app.join("conversation_summaries.db"), b"corrupt").unwrap();
        let wal_key = wal_db.canonicalize().unwrap();
        fs::remove_file(&wal_db).unwrap();
        let _ = fs::remove_file(wal_sibling_path(&wal_db));
        let _ = fs::remove_file(wal_db.with_extension("db-shm"));
        let _ = scan_root(&scan_brain, None);
        {
            let cache = TRAJECTORY_CACHE.lock_recover();
            assert!(!cache.contains_key(&wal_key));
            assert!(cache.contains_key(&empty_schema_key));
        }

        // Even if the entire conversations/ directory was deleted, trajectory_cache_key walks up
        // to the nearest existing ancestor (app_dir) so symlinked paths still normalize cleanly.
        fs::remove_dir_all(fixture.app.join("conversations")).unwrap();
        let expected_after_dir_removal = fixture
            .app
            .canonicalize()
            .unwrap()
            .join("conversations")
            .join("empty-schema.db");
        assert_eq!(
            trajectory_cache_key(
                &scan_brain
                    .parent()
                    .unwrap()
                    .join("conversations")
                    .join("empty-schema.db")
            ),
            expected_after_dir_removal
        );
        let _ = scan_root(&scan_brain, None);
        assert!(!TRAJECTORY_CACHE
            .lock_recover()
            .contains_key(&empty_schema_key));

        drop(raw_symlink_key);
    }

    #[test]
    fn agy_skips_immutable_fallback_when_nonempty_wal_exists_without_shm() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let live_dir = dir.path().join("live");
        let crash_dir = dir.path().join("crash");
        fs::create_dir_all(&live_dir).unwrap();
        fs::create_dir_all(crash_dir.join("conversations")).unwrap();

        let live_db = live_dir.join("conversation_summaries.db");
        {
            let conn = rusqlite::Connection::open(&live_db).unwrap();
            let _: String = conn
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .unwrap();
            conn.execute_batch(
                "PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE conversation_summaries(
                     conversation_id TEXT,
                     title TEXT,
                     workspace_uris TEXT,
                     preview TEXT,
                     last_modified_time TEXT,
                     step_count INTEGER,
                     nesting_depth INTEGER,
                     agent_name TEXT
                 );
                 INSERT INTO conversation_summaries VALUES
                     ('persisted-1', 'Persisted Title', '[]', 'hi', '2026-11-08T01:00:00Z', 2, 0, '');
                 PRAGMA wal_checkpoint(TRUNCATE);
                 INSERT INTO conversation_summaries VALUES
                     ('wal-only-2', 'WAL Only Title', '[]', 'wal', '2026-11-08T01:05:00Z', 3, 0, '');",
            )
            .unwrap();
            let live_wal = wal_sibling_path(&live_db);
            assert!(
                fs::metadata(&live_wal).unwrap().len() > 0,
                "expected uncheckpointed rows in -wal"
            );
            // Copy .db + non-empty -wal without -shm to simulate an abnormal crash.
            fs::copy(&live_db, crash_dir.join("conversation_summaries.db")).unwrap();
            fs::copy(&live_wal, crash_dir.join("conversation_summaries.db-wal")).unwrap();
        }

        let crash_summaries = crash_dir.join("conversation_summaries.db");
        let ro_flags =
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;

        // Cross-platform check: immutable fallback refuses non-empty -wal.
        assert!(has_nonempty_wal(&crash_summaries));
        assert!(open_db_immutable(&crash_summaries, ro_flags).is_none());

        for (id, prompt) in [
            ("persisted-1", "persisted prompt"),
            ("wal-only-2", "wal only prompt"),
        ] {
            write_jsonl(
                &crash_dir
                    .join("brain")
                    .join(id)
                    .join(".system_generated/logs/transcript_full.jsonl"),
                &[json!({
                    "step_index": 0,
                    "type": "USER_INPUT",
                    "created_at": "2026-11-08T01:00:00Z",
                    "content": format!("<USER_REQUEST>\n{prompt}\n</USER_REQUEST>")
                })],
            );
        }

        // Non-empty -wal without -shm on conversations/<id>.db as well.
        let live_conv = live_dir.join("side-wal.db");
        let crash_conv = crash_dir.join("conversations/side-wal.db");
        {
            let conn = rusqlite::Connection::open(&live_conv).unwrap();
            let _: String = conn
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .unwrap();
            conn.execute_batch(
                "PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE trajectory_meta(trajectory_id TEXT, trajectory_type INTEGER);
                 PRAGMA wal_checkpoint(TRUNCATE);
                 INSERT INTO trajectory_meta VALUES ('side-wal', 22);",
            )
            .unwrap();
            let live_wal = wal_sibling_path(&live_conv);
            assert!(fs::metadata(&live_wal).unwrap().len() > 0);
            fs::copy(&live_conv, &crash_conv).unwrap();
            fs::copy(&live_wal, wal_sibling_path(&crash_conv)).unwrap();
        }
        assert!(open_db_immutable(&crash_conv, ro_flags).is_none());

        // End-to-end check on Unix (non-root): a read-only directory prevents SQLite from
        // creating `-shm` in `SQLITE_OPEN_READ_ONLY`, forcing `SQLITE_CANTOPEN` -> immutable
        // fallback. With a non-empty `-wal`, `open_db` must return `None` so `scan_root` falls
        // back to transcripts and does not hide `wal-only-2`.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            struct RestorePerms(Vec<PathBuf>);
            impl Drop for RestorePerms {
                fn drop(&mut self) {
                    for p in &self.0 {
                        let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
                    }
                }
            }

            let conv_dir = crash_dir.join("conversations");
            let _guard = RestorePerms(vec![crash_dir.clone(), conv_dir.clone()]);
            fs::set_permissions(&crash_dir, fs::Permissions::from_mode(0o555)).unwrap();
            fs::set_permissions(&conv_dir, fs::Permissions::from_mode(0o555)).unwrap();

            // Skip only if running as root where mode 0555 still allows file creation.
            let dir_is_enforced_ro = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(crash_dir.join(".perm_probe"))
                .is_err();
            if dir_is_enforced_ro {
                // Confirm the initial read-only probe actually hits SQLITE_CANTOPEN so
                // `open_db` must rely on `open_db_immutable`'s `-wal` guard.
                let probe_err = rusqlite::Connection::open_with_flags(&crash_summaries, ro_flags)
                    .and_then(|c| c.prepare("SELECT 1 FROM sqlite_master LIMIT 1").map(|_| ()))
                    .unwrap_err();
                assert!(
                    matches!(
                        probe_err,
                        rusqlite::Error::SqliteFailure(err, _)
                            if err.code == rusqlite::ErrorCode::CannotOpen
                    ),
                    "expected SQLITE_CANTOPEN on WAL DB without -shm in read-only dir, got {probe_err:?}"
                );
                assert!(open_db(&crash_summaries).is_none());
                let mut sessions = scan_root(&crash_dir.join("brain"), None);
                sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
                let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
                assert_eq!(ids, ["persisted-1", "wal-only-2"]);

                let canon_conv = crash_conv.canonicalize().unwrap();
                assert!(open_db(&canon_conv).is_none());
                assert!(!is_non_main_trajectory(&canon_conv));
                assert!(!TRAJECTORY_CACHE.lock_recover().contains_key(&canon_conv));

                // Truncate -wal to 0 bytes (still no -shm, still read-only dir):
                // immutable=1 fallback must now succeed and read the checkpointed DB.
                fs::set_permissions(&crash_dir, fs::Permissions::from_mode(0o755)).unwrap();
                fs::write(crash_dir.join("conversation_summaries.db-wal"), b"").unwrap();
                fs::set_permissions(&crash_dir, fs::Permissions::from_mode(0o555)).unwrap();

                assert!(open_db(&crash_summaries).is_some());
                let sessions = scan_root(&crash_dir.join("brain"), None);
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].session_id, "persisted-1");
                assert_eq!(sessions[0].title.as_deref(), Some("Persisted Title"));
            }
        }

        // Cross-platform check: 0-byte -wal allows immutable=1 fallback and reads persisted row.
        fs::write(crash_dir.join("conversation_summaries.db-wal"), b"").unwrap();
        assert!(!has_nonempty_wal(&crash_summaries));
        let conn = open_db_immutable(&crash_summaries, ro_flags).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM conversation_summaries", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        drop(conn);

        // Corrupt file must also fail open_db_immutable's sqlite_master probe.
        let bad_db = crash_dir.join("corrupt.db");
        fs::write(&bad_db, b"not sqlite").unwrap();
        assert!(open_db_immutable(&bad_db, ro_flags).is_none());
    }

    #[test]
    fn agy_maps_run_shell_command_like_run_command() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir
            .path()
            .join("shell-1/.system_generated/logs/transcript_full.jsonl");
        write_jsonl(
            &file,
            &[
                json!({
                    "step_index": 0, "type": "PLANNER_RESPONSE",
                    "tool_calls": [
                        {
                            "id": "sh-search",
                            "name": "run_shell_command",
                            "args": {"CommandLine": "rg -n TRAJECTORY_CACHE src-tauri", "toolSummary": "Search cache"}
                        },
                        {
                            "id": "sh-cmd",
                            "name": "run_shell_command",
                            "args": {"CommandLine": "cargo test --lib agy", "Cwd": "/repo/src-tauri"}
                        },
                        {
                            "id": "sh-grep",
                            "name": "run_shell_command",
                            "args": {"command": "grep -n open_db agy.rs", "description": "Find open_db"}
                        },
                        {
                            "id": "sh-after",
                            "name": "read_file",
                            "args": {"Path": "/repo/README.md"}
                        }
                    ]
                }),
                json!({
                    "step_index": 1, "type": "GENERIC", "status": "DONE",
                    "content": "Created At: 2026-11-20T01:00:00Z\nCommand exited with code 1.\nOutput:\nno matches"
                }),
                json!({
                    "step_index": 2, "type": "RUN_COMMAND", "source": "MODEL", "status": "DONE",
                    "exit_code": 2, "content": "cargo failed"
                }),
                json!({
                    "step_index": 3, "type": "RUN_SHELL_COMMAND", "source": "MODEL", "status": "DONE",
                    "content": "Created At: 2026-11-20T01:00:02Z\nThe command completed successfully.\nOutput:\nmatch"
                }),
                json!({
                    "step_index": 4, "type": "GENERIC", "status": "DONE",
                    "content": "# README"
                }),
            ],
        );

        let messages = load_messages(&file).unwrap();
        assert_eq!(messages.len(), 5);
        let calls: Vec<_> = messages[0].blocks.iter().map(call_tuple).collect();
        assert_eq!(
            calls,
            [
                (
                    "sh-search",
                    ToolKind::Search,
                    "rg -n TRAJECTORY_CACHE src-tauri",
                    Some("Search cache"),
                    None,
                    None
                ),
                (
                    "sh-cmd",
                    ToolKind::Shell,
                    "cargo test --lib agy",
                    Some("/repo/src-tauri"),
                    None,
                    None
                ),
                (
                    "sh-grep",
                    ToolKind::Search,
                    "grep -n open_db agy.rs",
                    Some("Find open_db"),
                    None,
                    None
                ),
                (
                    "sh-after",
                    ToolKind::Read,
                    "/repo/README.md",
                    None,
                    None,
                    None
                ),
            ]
        );
        assert_eq!(messages[1].role, "tool");
        assert_eq!(result_tuple(&messages[1]).0, "sh-search");
        assert_eq!(result_tuple(&messages[1]).1, ToolStatus::Error);
        assert!(matches!(
            messages[1].blocks[0],
            SessionBlock::ToolResult {
                exit_code: Some(1),
                ..
            }
        ));
        assert_eq!(messages[2].role, "tool");
        assert_eq!(result_tuple(&messages[2]).0, "sh-cmd");
        assert_eq!(result_tuple(&messages[2]).1, ToolStatus::Error);
        assert!(matches!(
            messages[2].blocks[0],
            SessionBlock::ToolResult {
                exit_code: Some(2),
                ..
            }
        ));
        assert_eq!(messages[3].role, "tool");
        assert_eq!(result_tuple(&messages[3]).0, "sh-grep");
        assert_eq!(result_tuple(&messages[3]).1, ToolStatus::Success);
        assert!(matches!(
            messages[3].blocks[0],
            SessionBlock::ToolResult {
                exit_code: Some(0),
                ..
            }
        ));
        assert_eq!(messages[4].role, "tool");
        assert_eq!(result_tuple(&messages[4]).0, "sh-after");
        assert_eq!(result_tuple(&messages[4]).1, ToolStatus::Success);
    }
}
