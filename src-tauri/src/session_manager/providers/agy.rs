//! Read Antigravity's exported transcript. The conversation store is written only by
//! [`delete_session`], which removes a CLI conversation the way agy's own picker does.
use super::{
    blocks::assign_turn_ids,
    utils::{parse_timestamp_to_ms, truncate_summary},
};
use crate::session_manager::model::{MessageMeta, SessionBlock};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
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

pub(crate) fn records(path: &Path) -> Result<Vec<Value>, String> {
    let file =
        fs::File::open(path).map_err(|e| format!("Cannot read Antigravity transcript: {e}"))?;
    // The export can rewrite steps as they finish. The latest record for a step wins.
    let mut steps = BTreeMap::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("Cannot read Antigravity transcript: {e}"))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(index) = value["step_index"].as_u64() {
            steps.insert(index, value);
        }
    }
    Ok(steps.into_values().collect())
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan_roots(&session_sources())
}

/// Enumerate `(session_id, transcript_path)` across all Antigravity roots without opening
/// `conversation_summaries.db` or parsing transcript contents for session-list metadata.
pub(crate) fn transcript_files() -> Vec<(String, PathBuf)> {
    transcript_files_in_roots(&session_sources())
}

pub(crate) fn transcript_files_in_roots(
    sources: &[(PathBuf, Option<PathBuf>)],
) -> Vec<(String, PathBuf)> {
    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::new();
    for (root, _) in sources {
        for (id, path) in transcripts_in_root(root) {
            if seen.insert(id.clone()) {
                files.push((id, path));
            }
        }
    }
    files
}

fn transcripts_in_root(root: &Path) -> Vec<(String, PathBuf)> {
    let canonical_root = root.canonicalize().ok();
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Some(id) = entry
                .file_name()
                .to_str()
                .map(str::to_string)
                .filter(|id| valid_session_id(id))
            else {
                continue;
            };
            let path = entry
                .path()
                .join(".system_generated/logs/transcript_full.jsonl");
            if !path.is_file()
                || !path
                    .canonicalize()
                    .ok()
                    .zip(canonical_root.as_ref())
                    .is_some_and(|(path, root)| path.starts_with(root))
            {
                continue;
            }
            files.push((id, path));
        }
    }
    files
}

/// Missing roots are tolerated; a session id found in several roots is reported once. The first
/// root wins, except that an index-only entry yields to a later root holding the transcript: the
/// IDE can continue a conversation the CLI index also lists, and only the transcript has usage.
fn scan_roots(sources: &[(PathBuf, Option<PathBuf>)]) -> Vec<SessionMeta> {
    let mut seen = std::collections::HashMap::new();
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
    let mut summaries = read_summaries(
        &root
            .parent()
            .unwrap_or(root)
            .join("conversation_summaries.db"),
        gemini_dir,
    );
    let mut sessions = Vec::new();
    for (id, path) in transcripts_in_root(root) {
        let summary = summaries.remove(&id);
        // Avoid parsing every transcript on list refresh when agy's own index is available.
        let mut meta = if let Some(meta) = summary {
            meta
        } else {
            let Ok(rows) = records(&path) else { continue };
            let first = rows
                .iter()
                .find(|v| v["type"] == "USER_INPUT")
                .and_then(|v| v["content"].as_str())
                .map(user_request);
            SessionMeta {
                provider_id: "agy".into(),
                session_id: id.clone(),
                title: first.map(|s| truncate_summary(s, 80)),
                summary: first.map(|s| truncate_summary(s, 160)),
                project_dir: None,
                created_at: rows.iter().find_map(timestamp),
                last_active_at: rows.iter().filter_map(timestamp).max(),
                source_path: None,
                resume_command: Some(crate::agy_config::resume_command(gemini_dir, &id)),
            }
        };
        meta.source_path = Some(path.to_string_lossy().into_owned());
        sessions.push(meta);
    }
    // A binary-only conversation can still be found and resumed, but has no readable transcript.
    sessions.extend(summaries.into_values());
    sessions
}

fn read_summaries(path: &Path, gemini_dir: Option<&Path>) -> BTreeMap<String, SessionMeta> {
    let mut result = BTreeMap::new();
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return result;
    };
    let Ok(mut query) = conn.prepare("SELECT conversation_id, title, workspace_uris, preview, last_modified_time FROM conversation_summaries") else { return result };
    let Ok(rows) = query.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    }) else {
        return result;
    };
    for (id, title, uris, preview, modified) in rows.flatten() {
        if !valid_session_id(&id) {
            continue;
        }
        let project_dir = serde_json::from_str::<Vec<String>>(&uris)
            .ok()
            .and_then(|v| v.into_iter().next())
            .and_then(|uri| url::Url::parse(&uri).ok()?.to_file_path().ok())
            .map(|p| p.to_string_lossy().into_owned());
        let last_active_at =
            parse_timestamp_to_ms(&Value::String(modified.clone())).or_else(|| {
                chrono::NaiveDateTime::parse_from_str(&modified, "%Y-%m-%d %H:%M:%S%.f")
                    .ok()
                    .map(|t| t.and_utc().timestamp_millis())
            });
        result.insert(
            id.clone(),
            SessionMeta {
                provider_id: "agy".into(),
                session_id: id.clone(),
                title: (!title.is_empty()).then_some(title),
                summary: (!preview.is_empty()).then(|| truncate_summary(&preview, 160)),
                project_dir,
                created_at: None,
                last_active_at,
                source_path: None,
                resume_command: Some(crate::agy_config::resume_command(gemini_dir, &id)),
            },
        );
    }
    result
}

/// agy stores each prompt wrapped in `<USER_REQUEST>`, followed by context it adds itself
/// (`<ADDITIONAL_METADATA>`, `<USER_SETTINGS_CHANGE>`, ...). Only the part the user typed is shown.
fn user_request(content: &str) -> &str {
    content
        .split_once("<USER_REQUEST>")
        .and_then(|(_, rest)| rest.split_once("</USER_REQUEST>"))
        .map(|(inner, _)| inner.trim())
        .filter(|inner| !inner.is_empty())
        .unwrap_or(content)
}

pub(crate) fn timestamp(value: &Value) -> Option<i64> {
    parse_timestamp_to_ms(&value["created_at"])
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for value in records(path)? {
        let role = match value["type"].as_str() {
            Some("USER_INPUT") => "user",
            Some("PLANNER_RESPONSE") => "assistant",
            _ if value["source"] == "MODEL" => "assistant",
            _ => "system",
        };
        let content = value["content"].as_str().unwrap_or("");
        let content = if role == "user" {
            user_request(content)
        } else {
            content
        };
        // Keep unrecognized steps visible instead of silently dropping future tool/event types.
        let content = if content.is_empty() {
            serde_json::to_string_pretty(&value).unwrap_or_default()
        } else {
            content.into()
        };
        let mut message = SessionMessage::from_blocks(
            role,
            timestamp(&value),
            vec![SessionBlock::Text {
                text: content,
                full: None,
            }],
        );
        message.id = value["step_index"].as_u64().map(|n| n.to_string());
        message.meta = Some(MessageMeta {
            model: value["model"].as_str().map(String::from),
            input_tokens: value["input_tokens"].as_u64(),
            output_tokens: value["output_tokens"].as_u64(),
            cache_read_tokens: value["cache_read_tokens"].as_u64(),
            cache_write_tokens: value["cache_write_tokens"].as_u64(),
            reasoning_tokens: value["reasoning_tokens"].as_u64(),
            ..Default::default()
        });
        messages.push(message);
    }
    assign_turn_ids(&mut messages);
    Ok(messages)
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
    for file in [
        conversation.clone(),
        conversation.with_extension("db-wal"),
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
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let session = root.path().join("abc-123");
        fs::create_dir_all(&session).unwrap();
        let logs = outside.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("transcript_full.jsonl"),
            r#"{"step_index":0,"type":"USER_INPUT","content":"outside"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), session.join(".system_generated")).unwrap();
        assert!(scan_root(root.path(), None).is_empty());
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
    #[allow(clippy::incompatible_msrv)]
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
}
