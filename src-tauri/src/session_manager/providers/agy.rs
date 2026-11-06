//! Read Antigravity's exported transcript; never open its binary conversation store for writing.
use super::{
    blocks::{self, assign_turn_ids, ToolSource},
    utils::{for_each_jsonl_value, parse_timestamp_to_ms, truncate_summary, JsonlSpan},
};
use crate::session_manager::model::{
    DiffOp, EventKind, ImageRef, ImageSource, MessageMeta, SessionBlock, ToolKind, ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    time::Duration,
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
    let mut index = read_summaries(
        root,
        base,
        canonical_root.as_deref(),
        canonical_base.as_deref(),
        gemini_dir,
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
                || db_file(&conv_dir, canonical_base.as_deref(), &id)
                    .as_deref()
                    .is_some_and(is_non_main_trajectory))
        {
            continue;
        }
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
        // Avoid parsing every transcript on list refresh when agy's own index is complete.
        if meta.title.is_none() || meta.last_active_at.is_none() {
            if let Ok(rows) = records(&path) {
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
            continue;
        }
        meta.source_path = Some(path.to_string_lossy().into_owned());
        sessions.push(meta);
    }
    // A binary-only conversation can still be found and resumed, but has no readable transcript.
    sessions.extend(
        index
            .sessions
            .into_values()
            .filter(|meta| meta.title.is_some()),
    );
    sessions
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

fn open_db(path: &Path) -> Option<rusqlite::Connection> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(Duration::ZERO).ok()?;
    Some(conn)
}

fn db_file(conv_dir: &Path, canonical_base: Option<&Path>, id: &str) -> Option<PathBuf> {
    let path = conv_dir.join(format!("{id}.db"));
    (path.is_file()
        && canonical_base.is_none_or(|base| {
            path.canonicalize()
                .ok()
                .is_some_and(|p| p.starts_with(base))
        }))
    .then_some(path)
}

/// Normal top-level conversations have `trajectory_meta.trajectory_type = 4` (`CASCADE`).
/// Side questions (`/btw`) use `trajectory_type = 22` (`SIDE_QUESTION`) and are excluded.
fn is_non_main_trajectory(db_path: &Path) -> bool {
    open_db(db_path).is_some_and(|conn| {
        conn.query_row(
            "SELECT trajectory_type FROM trajectory_meta LIMIT 1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .is_ok_and(|t| t != 4)
    })
}

fn read_summaries(
    root: &Path,
    base: &Path,
    canonical_root: Option<&Path>,
    canonical_base: Option<&Path>,
    gemini_dir: Option<&Path>,
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
    index.has_db = true;
    for (id, title, uris, preview, modified, step_count, nesting_depth, agent_name) in
        rows.flatten()
    {
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
        if conv_db.as_deref().is_some_and(is_non_main_trajectory) {
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
    index
}

/// agy stores each prompt wrapped in `<USER_REQUEST>`, followed by context it adds itself
/// (`<ADDITIONAL_METADATA>`, `<USER_SETTINGS_CHANGE>`, ...). Only the user's text is shown.
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
    let mut pending_calls: VecDeque<(String, Option<String>)> = VecDeque::new();
    let mut subagent_labels: HashMap<String, String> = HashMap::new();

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
                let content = user_request(value["content"].as_str().unwrap_or_default()).trim();
                if !content.is_empty() {
                    msg_blocks.push(SessionBlock::text(content));
                }
                for image in media_images(&value, span) {
                    msg_blocks.push(SessionBlock::Image { image });
                }
            }
            _ if step_type == Some("PLANNER_RESPONSE")
                || (value["source"] == "MODEL" && step_type != Some("GENERIC")) =>
            {
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
                        pending_calls.push_back((call_id.clone(), subagent_type));
                        msg_blocks.push(agy_tool_call(&call_id, raw_name, args, span, idx));
                    }
                }
            }
            Some("GENERIC") => {
                role = "tool";
                let (call_id, pending_subagent_type) = pending_calls
                    .pop_front()
                    .unwrap_or_else(|| (format!("step-{step_index}-result"), None));
                let content = value["content"].as_str().unwrap_or_default();
                let error = value["error"].as_str().unwrap_or_default();
                if let Some(subagent_type) = pending_subagent_type {
                    if let Some(conv_id) = extract_subagent_conversation_id(content) {
                        let label = if !subagent_type.is_empty() {
                            subagent_type
                        } else {
                            default_subagent_label(&conv_id)
                        };
                        subagent_labels.insert(conv_id, label);
                    }
                }
                let status_str = value["status"].as_str().unwrap_or_default();
                let status = if status_str == "ERROR" || !error.is_empty() {
                    ToolStatus::Error
                } else if status_str == "RUNNING" {
                    ToolStatus::Pending
                } else {
                    ToolStatus::Success
                };
                let (output, pointer) = if !content.is_empty() {
                    (content, "/content")
                } else if !error.is_empty() {
                    (error, "/error")
                } else {
                    ("", "/content")
                };
                let mut block = blocks::tool_result_block(call_id, status, output, || {
                    Some(span.content_ref(pointer))
                });
                if let SessionBlock::ToolResult { images, .. } = &mut block {
                    *images = media_images(&value, span);
                }
                msg_blocks.push(block);
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
        message.meta = message_meta(&value);
        messages.push(message);
    }
    assign_turn_ids(&mut messages);
    Ok(messages)
}

fn message_meta(value: &Value) -> Option<MessageMeta> {
    let meta = MessageMeta {
        model: value["model"]
            .as_str()
            .filter(|s| !s.is_empty())
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
        "run_command" => {
            let cmd = str_in(&["CommandLine", "command", "cmd"]).unwrap_or_default();
            Some((
                blocks::refine_shell_kind(cmd),
                blocks::title_shell(cmd),
                line_in(&["toolSummary", "Cwd", "cwd", "description"]),
            ))
        }
        "command_status" | "manage_task" | "send_command_input" => Some((
            ToolKind::Shell,
            title_in(&["CommandId", "TaskId", "Action", "action"]),
            line_in(&["Input", "Status"]),
        )),
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
                Some(chunks) => chunks
                    .iter()
                    .filter_map(pair_counts)
                    .reduce(|(a, r), (x, y)| (a.saturating_add(x), r.saturating_add(y))),
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
        "search_web" | "read_url_content" => Some((
            ToolKind::Web,
            title_in(&["query", "Query", "Url", "url"]),
            line_in(&["domain", "Domain"]),
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
        "call_mcp_tool" | "list_resources" | "read_resource" => {
            server = str_in(&["ServerName", "server_name", "server"]).map(str::to_string);
            let detail = if raw_name == "call_mcp_tool" {
                line_in(&["ServerName"])
            } else {
                line_in(&["Uri"])
            };
            Some((ToolKind::Mcp, title_in(&["ToolName", "ServerName"]), detail))
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
                        {"name": "call_mcp_tool", "args": {"ServerName": "github", "ToolName": "get_issue"}},
                        {"name": "read_resource", "args": {"ServerName": "docs", "Uri": "doc://1"}},
                        {"name": "send_message", "args": {"Recipient": "parent-1", "Message": "Done\nDetails"}},
                        {"name": "manage_subagents", "args": {"Action": "list", "Target": "all"}},
                        {"name": "manage_task", "args": {"Action": "status", "TaskId": "t-1", "Status": "running"}}
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
                    "get_issue",
                    Some("github"),
                    Some("github"),
                    None
                ),
                (ToolKind::Mcp, "docs", Some("doc://1"), Some("docs"), None),
                (ToolKind::Agent, "parent-1", Some("Done…"), None, None),
                (ToolKind::Agent, "list", Some("all"), None, None),
                (ToolKind::Shell, "t-1", Some("running"), None, None),
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
}
