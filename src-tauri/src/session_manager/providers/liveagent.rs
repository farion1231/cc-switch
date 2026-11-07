//! Read-only LiveAgent (K-brain backend) session browser.
//!
//! Layout under `<data dir>/sessions`: global sessions live at
//! `<session>/session.jsonl`, project sessions at `<project>/<session>/session.jsonl`.
//! Each line is `{"type": ..., "payload": {...}}`. The backend rewrites the whole
//! file atomically and owns related runtime state, so deletion stays in LiveAgent.
use super::blocks::{
    assign_turn_ids, large_text_block, openai_tool_calls, summary_event_block, thinking_block,
    tool_result_block,
};
use super::codex_items::image_from_url;
use super::utils::{
    for_each_jsonl_value, parse_timestamp_to_ms, truncate_summary, FileParseCache, JsonlSpan,
    ScanStatus, TITLE_MAX_CHARS,
};
use crate::session_manager::{
    model::{EventKind, MessageMeta, SessionBlock, ToolStatus},
    SessionMessage, SessionMeta,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

const PROVIDER_ID: &str = "liveagent";
const SESSION_FILE: &str = "session.jsonl";
/// Backend run logs share the sessions directory but are not conversations.
const IGNORED_DIRS: &[&str] = &["backend-events"];
const MAX_ENTRIES: usize = 100_000;

static PARSE_CACHE: LazyLock<FileParseCache> = LazyLock::new(FileParseCache::new);

pub fn session_roots() -> Vec<PathBuf> {
    crate::liveagent_config::data_dir()
        .map(|dir| vec![dir.join("sessions")])
        .unwrap_or_default()
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan_sessions_with_status(&mut ScanStatus::default())
}

pub(crate) fn scan_sessions_with_status(status: &mut ScanStatus) -> Vec<SessionMeta> {
    match crate::liveagent_config::data_dir() {
        Ok(dir) => scan_root(&PARSE_CACHE, &dir.join("sessions"), status),
        Err(error) => {
            status.incomplete = true;
            log::warn!("LiveAgent session discovery unavailable: {error}");
            Vec::new()
        }
    }
}

fn scan_root(cache: &FileParseCache, root: &Path, status: &mut ScanStatus) -> Vec<SessionMeta> {
    let files = session_files(root, status);
    cache.scan_with_status(files, parse_session_meta, status)
}

fn is_session_dir_name(name: &str) -> bool {
    !name.starts_with('.') && !IGNORED_DIRS.contains(&name)
}

/// `<root>/<session>/session.jsonl` and `<root>/<project>/<session>/session.jsonl`.
/// Directory links are not followed.
fn session_files(root: &Path, status: &mut ScanStatus) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for entry in status.entries(root).into_iter().take(MAX_ENTRIES) {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if !entry.file_name().to_str().is_some_and(is_session_dir_name) {
            continue;
        }
        let dir = entry.path();
        let direct = dir.join(SESSION_FILE);
        if is_regular_file(&direct) {
            result.push(direct);
            continue;
        }
        for session in status.entries(&dir).into_iter().take(MAX_ENTRIES) {
            if !session.file_type().is_ok_and(|kind| kind.is_dir())
                || !session
                    .file_name()
                    .to_str()
                    .is_some_and(is_session_dir_name)
            {
                continue;
            }
            let file = session.path().join(SESSION_FILE);
            if is_regular_file(&file) {
                result.push(file);
            }
        }
    }
    result
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}

/// Only the record type; the payload is skipped without building a tree.
#[derive(Deserialize)]
struct RecordKind {
    #[serde(rename = "type", default)]
    kind: String,
}

fn parse_session_meta(path: &Path) -> io::Result<Option<SessionMeta>> {
    let data = std::fs::read(path)?;
    let mut meta: Option<Value> = None;
    let mut first_prompt: Option<String> = None;
    for line in data.split(|byte| *byte == b'\n') {
        let line = line.trim_ascii();
        if line.is_empty() {
            continue;
        }
        let Ok(RecordKind { kind }) = serde_json::from_slice::<RecordKind>(line) else {
            continue;
        };
        match kind.as_str() {
            // Later metadata records update the session; the last one wins.
            "session_meta" => {
                if let Ok(mut record) = serde_json::from_slice::<Value>(line) {
                    meta = Some(record["payload"].take());
                }
            }
            "message" if first_prompt.is_none() => {
                let Ok(record) = serde_json::from_slice::<Value>(line) else {
                    continue;
                };
                let message = &record["payload"]["message"];
                if message["role"] == "user" && message["origin"] != "background" {
                    let text = content_text(&message["content"]);
                    if !text.trim().is_empty() {
                        first_prompt = Some(text);
                    }
                }
            }
            _ => {}
        }
    }
    let Some(meta) = meta.filter(Value::is_object) else {
        return Ok(None);
    };
    let session_id = meta["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let title = meta["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .map(|title| truncate_summary(title, TITLE_MAX_CHARS))
        .or_else(|| {
            first_prompt
                .as_deref()
                .map(|text| truncate_summary(text, TITLE_MAX_CHARS))
        });
    let created_at = parse_timestamp_to_ms(&meta["created_at"]);
    Ok(Some(SessionMeta {
        provider_id: PROVIDER_ID.into(),
        session_id,
        title,
        summary: None,
        project_dir: meta["cwd"]
            .as_str()
            .filter(|cwd| !cwd.is_empty())
            .map(str::to_string),
        created_at,
        last_active_at: parse_timestamp_to_ms(&meta["updated_at"]).or(created_at),
        source_path: Some(path.to_string_lossy().into_owned()),
        resume_command: None,
    }))
}

/// Plain text of a message `content`: a string, or `ContentPart[]` text parts.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `content` → Text / Image blocks. Parts follow K-brain's `ai.ContentPart`
/// (`text`, `image_url{url}`, `file_url{url, filename}`).
fn content_blocks(content: &Value, span: JsonlSpan, pointer: &str) -> Vec<SessionBlock> {
    match content {
        Value::String(text) if !text.trim().is_empty() => vec![SessionBlock::text(text.clone())],
        Value::Array(parts) => parts
            .iter()
            .enumerate()
            .filter_map(|(index, part)| match part["type"].as_str() {
                Some("text") => part["text"]
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(SessionBlock::text),
                Some("image_url") => part["image_url"]["url"].as_str().and_then(|url| {
                    image_from_url(url, span, format!("{pointer}/{index}/image_url/url"))
                        .map(|image| SessionBlock::Image { image })
                }),
                Some("file_url") => {
                    let file = &part["file_url"];
                    let name = file["filename"]
                        .as_str()
                        .filter(|name| !name.is_empty())
                        .or_else(|| file["url"].as_str())?;
                    Some(SessionBlock::text(format!("[File: {name}]")))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

struct Entry {
    seq: i64,
    span: JsonlSpan,
    record: Value,
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut compactions: Vec<Entry> = Vec::new();
    for_each_jsonl_value(path, |span, record| {
        let target = match record["type"].as_str() {
            Some("message") => &mut entries,
            Some("compaction") => &mut compactions,
            _ => return Ok(()),
        };
        let Some(seq) = record["payload"]["seq"].as_i64() else {
            return Ok(());
        };
        target.push(Entry { seq, span, record });
        Ok(())
    })?;

    // Messages are keyed by seq; a later record with the same seq replaces the earlier one.
    entries.sort_by_key(|entry| entry.seq);
    entries.dedup_by(|later, earlier| {
        if later.seq == earlier.seq {
            std::mem::swap(later, earlier);
            true
        } else {
            false
        }
    });
    compactions.sort_by_key(|entry| entry.seq);

    // A compaction's `cutoff` counts the raw messages it summarises; show it there.
    let mut notices: Vec<(usize, SessionMessage)> = compactions
        .iter()
        .filter_map(|entry| {
            let payload = &entry.record["payload"];
            let summary = payload["summary"].as_str().unwrap_or_default();
            let block = if summary.trim().is_empty() {
                SessionBlock::event(EventKind::Compaction, None, None)
            } else {
                summary_event_block(EventKind::Compaction, summary, || {
                    Some(entry.span.content_ref("/payload/summary"))
                })
            };
            let cutoff = usize::try_from(payload["cutoff"].as_i64()?.max(0)).ok()?;
            let mut message = SessionMessage::from_blocks("system", None, vec![block]);
            message.meta = Some(MessageMeta {
                model: payload["model"]
                    .as_str()
                    .filter(|m| !m.is_empty())
                    .map(str::to_string),
                ..usage_meta(&payload["usage"])
            });
            Some((cutoff.min(entries.len()), message))
        })
        .collect();
    notices.sort_by_key(|(cutoff, _)| *cutoff);
    let mut notices = notices.into_iter().peekable();

    let mut messages = Vec::with_capacity(entries.len() + compactions.len());
    for (index, entry) in entries.iter().enumerate() {
        while let Some((_, notice)) = notices.next_if(|(cutoff, _)| *cutoff <= index) {
            messages.push(notice);
        }
        if let Some(message) = convert_message(entry) {
            messages.push(message);
        }
    }
    messages.extend(notices.map(|(_, notice)| notice));

    assign_turn_ids(&mut messages);
    Ok(messages)
}

fn usage_meta(usage: &Value) -> MessageMeta {
    let positive = |key: &str| usage[key].as_u64().filter(|n| *n > 0);
    MessageMeta {
        input_tokens: positive("prompt_tokens"),
        output_tokens: positive("completion_tokens"),
        cache_read_tokens: positive("prompt_cache_hit_tokens")
            .or_else(|| usage["prompt_tokens_details"]["cached_tokens"].as_u64()),
        cache_write_tokens: positive("prompt_cache_write_tokens"),
        ..Default::default()
    }
}

fn convert_message(entry: &Entry) -> Option<SessionMessage> {
    const BASE: &str = "/payload/message";
    let span = entry.span;
    let raw = &entry.record["payload"]["message"];
    let role = raw["role"].as_str().filter(|role| !role.is_empty())?;
    let content = &raw["content"];
    let content_pointer = format!("{BASE}/content");
    let content_ref = || {
        content
            .is_string()
            .then(|| span.content_ref(format!("{BASE}/content")))
    };

    let mut injected = false;
    let blocks = match role {
        "tool" => vec![tool_result_block(
            raw["tool_call_id"].as_str().unwrap_or_default(),
            // K-brain does not record whether a tool call failed.
            ToolStatus::Unknown,
            &content_text(content),
            content_ref,
        )],
        "assistant" => {
            let mut blocks = Vec::new();
            if let Some(reasoning) = raw["reasoning"].as_str().filter(|r| !r.trim().is_empty()) {
                blocks.push(thinking_block(reasoning, None, None, || {
                    Some(span.content_ref(format!("{BASE}/reasoning")))
                }));
            }
            blocks.extend(content_blocks(content, span, &content_pointer));
            blocks.extend(openai_tool_calls(raw.get("tool_calls"), |pointer| {
                Some(span.content_ref(format!("{BASE}/tool_calls{pointer}")))
            }));
            blocks
        }
        "user" => {
            // Runtime notices (wait results, subagent reports, …) are user-role but not typed.
            injected = raw["origin"] == "background";
            if injected {
                let text = content_text(content);
                if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![large_text_block(text, content_ref)]
                }
            } else {
                content_blocks(content, span, &content_pointer)
            }
        }
        _ => {
            injected = true;
            let text = content_text(content);
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![large_text_block(text, content_ref)]
            }
        }
    };
    if blocks.is_empty() {
        return None;
    }
    let ts = parse_timestamp_to_ms(&raw["sent_at"]);
    let mut message = SessionMessage::from_blocks(role, ts, blocks);
    message.injected = injected;
    message.id = raw["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    if role == "assistant" {
        message.meta = Some(MessageMeta {
            model: raw["model"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            stop_reason: raw["stop_reason"]
                .as_str()
                .filter(|r| !r.is_empty())
                .map(str::to_string),
            ..usage_meta(&raw["usage"])
        });
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::model::ContentRef;
    use serde_json::json;
    use std::fs;

    fn write_session(path: &Path, records: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = String::new();
        for record in records {
            text.push_str(&record.to_string());
            text.push('\n');
        }
        fs::write(path, text).unwrap();
    }

    fn meta(id: &str, title: &str, cwd: &str, updated: &str) -> Value {
        json!({"type":"session_meta","payload":{
            "version":1,"id":id,"title":title,"cwd":cwd,"model":"m","provider":"p",
            "created_at":"2026-01-01T00:00:00Z","updated_at":updated,
            "archived":false,"pinned":false,"tags":[],"usage_in":1,"usage_out":2,"usage_cached":0}})
    }

    fn message(seq: i64, message: Value) -> Value {
        json!({"type":"message","payload":{"seq":seq,"message":message}})
    }

    fn scan(root: &Path) -> Vec<SessionMeta> {
        scan_root(&FileParseCache::new(), root, &mut ScanStatus::default())
    }

    #[test]
    fn scans_global_and_project_layouts_and_ignores_backend_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write_session(
            &root.join("global-1/session.jsonl"),
            &[meta("global-1", "Global", "/g", "2026-01-02T00:00:00Z")],
        );
        write_session(
            &root.join("proj-abc123/proj-1/session.jsonl"),
            &[
                meta("proj-1", "", "/work/proj", "2026-01-03T00:00:00Z"),
                message(0, json!({"role":"user","content":"First prompt"})),
            ],
        );
        // Not sessions.
        write_session(
            &root.join("backend-events/abc/session.jsonl"),
            &[meta("x", "Ignored", "/", "2026-01-01T00:00:00Z")],
        );
        fs::write(root.join("backend-events/abc.jsonl"), "{}\n").unwrap();
        fs::write(root.join(".lock"), "").unwrap();
        fs::write(root.join("planning.json"), "{}").unwrap();
        fs::write(root.join("proj-abc123/proj-1/other.jsonl"), "{}\n").unwrap();
        // A directory without session_meta is not a session.
        write_session(
            &root.join("no-meta/session.jsonl"),
            &[message(0, json!({"role":"user","content":"x"}))],
        );

        let mut sessions = scan(root);
        sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "global-1");
        assert_eq!(sessions[0].title.as_deref(), Some("Global"));
        assert_eq!(sessions[0].project_dir.as_deref(), Some("/g"));
        assert_eq!(sessions[1].session_id, "proj-1");
        assert_eq!(sessions[1].provider_id, "liveagent");
        // Empty title falls back to the first user prompt.
        assert_eq!(sessions[1].title.as_deref(), Some("First prompt"));
        assert_eq!(sessions[1].project_dir.as_deref(), Some("/work/proj"));
        assert_eq!(
            sessions[1].last_active_at,
            parse_timestamp_to_ms(&json!("2026-01-03T00:00:00Z"))
        );
        assert!(sessions[1]
            .source_path
            .as_deref()
            .unwrap()
            .ends_with("proj-1/session.jsonl"));
    }

    #[test]
    fn last_session_meta_wins_and_malformed_lines_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("s1/session.jsonl");
        write_session(
            &path,
            &[
                meta("s1", "Old title", "/old", "2026-01-02T00:00:00Z"),
                message(0, json!({"role":"user","content":"Hello"})),
            ],
        );
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        text.push_str(&meta("s1", "New title", "/new", "2026-02-01T00:00:00Z").to_string());
        text.push('\n');
        text.push_str(&message(1, json!({"role":"assistant","content":"Hi"})).to_string());
        text.push_str("\n{\"type\":\"message\",\"payload\":");
        fs::write(&path, text).unwrap();

        let sessions = scan(temp.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title.as_deref(), Some("New title"));
        assert_eq!(sessions[0].project_dir.as_deref(), Some("/new"));
        assert_eq!(
            sessions[0].last_active_at,
            parse_timestamp_to_ms(&json!("2026-02-01T00:00:00Z"))
        );

        let messages = load_messages(&path).unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|m| (m.role.as_str(), m.content.as_str()))
                .collect::<Vec<_>>(),
            vec![("user", "Hello"), ("assistant", "Hi")]
        );
    }

    #[test]
    fn converts_reasoning_tool_calls_results_usage_and_compaction() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("p/s/session.jsonl");
        write_session(
            &path,
            &[
                meta("s", "T", "/w", "2026-01-02T00:00:00Z"),
                message(
                    0,
                    json!({"role":"user","content":"Read it","sent_at":"2026-01-01T00:00:01Z","authored":true}),
                ),
                message(
                    1,
                    json!({
                        "id":"a1","role":"assistant","content":"Reading","reasoning":"Think first",
                        "model":"deepseek-chat","stop_reason":"tool_calls","sent_at":"2026-01-01T00:00:02Z",
                        "tool_calls":[{"id":"call-1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.rs\"}"}}],
                        "usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_cache_hit_tokens":80}
                    }),
                ),
                message(
                    2,
                    json!({"role":"tool","tool_call_id":"call-1","name":"read_file","content":"file body"}),
                ),
                json!({"type":"compaction","payload":{"seq":1,"cutoff":3,"summary":"Earlier work","model":"m","usage":{"prompt_tokens":5,"completion_tokens":6},"drop_prior":false,"fresh":false}}),
                json!({"type":"snapshot","payload":{"seq":2,"ref":"abc"}}),
                message(
                    3,
                    json!({"role":"user","origin":"background","content":"Process exited"}),
                ),
                message(
                    4,
                    json!({"role":"user","content":[{"type":"text","text":"See"},{"type":"image_url","image_url":{"url":"data:image/png;base64,AAAA"}},{"type":"file_url","file_url":{"url":"file:///x.pdf","filename":"x.pdf"}}]}),
                ),
            ],
        );

        let messages = load_messages(&path).unwrap();
        let roles: Vec<_> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            vec!["user", "assistant", "tool", "system", "user", "user"]
        );

        let assistant = &messages[1];
        assert_eq!(assistant.id.as_deref(), Some("a1"));
        assert!(
            matches!(assistant.blocks[0], SessionBlock::Thinking { ref text, .. } if text == "Think first")
        );
        assert!(
            matches!(assistant.blocks[1], SessionBlock::Text { ref text, .. } if text == "Reading")
        );
        assert!(matches!(
            assistant.blocks[2],
            SessionBlock::ToolCall { ref id, ref raw_name, .. } if id == "call-1" && raw_name == "read_file"
        ));
        let meta = assistant.meta.as_ref().unwrap();
        assert_eq!(meta.model.as_deref(), Some("deepseek-chat"));
        assert_eq!(meta.input_tokens, Some(100));
        assert_eq!(meta.output_tokens, Some(20));
        assert_eq!(meta.cache_read_tokens, Some(80));
        assert_eq!(meta.stop_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            assistant.ts,
            parse_timestamp_to_ms(&json!("2026-01-01T00:00:02Z"))
        );

        assert!(matches!(
            messages[2].blocks[0],
            SessionBlock::ToolResult { ref call_id, ref preview, .. } if call_id == "call-1" && preview == "file body"
        ));

        assert!(matches!(
            messages[3].blocks[0],
            SessionBlock::Event { kind: EventKind::Compaction, ref text, .. } if text.as_deref() == Some("Earlier work")
        ));

        assert!(messages[4].injected);
        assert!(!messages[5].injected);
        assert!(matches!(messages[5].blocks[1], SessionBlock::Image { .. }));
        assert!(
            matches!(messages[5].blocks[2], SessionBlock::Text { ref text, .. } if text == "[File: x.pdf]")
        );

        // Background notices do not open a new turn.
        assert_eq!(messages[0].turn_id, messages[4].turn_id);
        assert_ne!(messages[0].turn_id, messages[5].turn_id);
    }

    #[test]
    fn long_compaction_summary_references_its_line() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("s/session.jsonl");
        let summary = "summary line\n".repeat(400);
        write_session(
            &path,
            &[
                meta("s", "T", "/w", "2026-01-02T00:00:00Z"),
                json!({"type":"compaction","payload":{"seq":1,"cutoff":99,"summary":summary,"model":"m","usage":{}}}),
            ],
        );
        let messages = load_messages(&path).unwrap();
        assert_eq!(messages.len(), 1);
        let SessionBlock::Event {
            full: Some(ContentRef::Jsonl { pointer, .. }),
            ..
        } = &messages[0].blocks[0]
        else {
            panic!("expected a compaction event with a full-text reference");
        };
        assert_eq!(pointer, "/payload/summary");
    }
}
