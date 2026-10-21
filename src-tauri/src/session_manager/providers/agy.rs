//! Read Antigravity's exported transcript; never open its binary conversation store for writing.
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

/// Missing roots are tolerated; a session id found in several roots is reported once (first wins).
fn scan_roots(sources: &[(PathBuf, Option<PathBuf>)]) -> Vec<SessionMeta> {
    let mut seen = std::collections::HashSet::new();
    let mut sessions = Vec::new();
    for (root, gemini_dir) in sources {
        for meta in scan_root(root, gemini_dir.as_deref()) {
            if seen.insert(meta.session_id.clone()) {
                sessions.push(meta);
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
    let canonical_root = root.canonicalize().ok();
    let mut sessions = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agy_binary_only_sessions_keep_index_metadata_without_fabricated_transcripts() {
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
}
