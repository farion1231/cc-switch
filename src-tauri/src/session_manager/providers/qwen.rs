//! Qwen Code sessions: `<runtime>/projects/<sanitized cwd>/chats/<sessionId>.jsonl`.
//!
//! Each line is a record with `type ∈ {user, assistant, tool_result, system}` and a
//! Gemini-style `message: { role, parts }`. Assistant parts carry text, thoughts
//! (`thought: true`) and `functionCall`s; `tool_result` records carry the matching
//! `functionResponse` plus `toolCallResult.status`. System records (telemetry,
//! snapshots) are not conversation messages.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::model::{MessageMeta, SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, thinking_block, tool_call_block, tool_result_block, ToolSource,
};
use super::utils::{
    for_each_jsonl_value, parse_timestamp_to_ms, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

pub(crate) fn session_roots() -> Vec<PathBuf> {
    vec![crate::qwen_config::runtime_dir().join("projects")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan(&crate::qwen_config::runtime_dir().join("projects"))
}

fn scan(projects: &Path) -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    let Ok(projects) = fs::read_dir(projects) else {
        return sessions;
    };
    for project in projects.flatten() {
        let Ok(chats) = fs::read_dir(project.path().join("chats")) else {
            continue;
        };
        for chat in chats.flatten() {
            let path = chat.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(meta) = parse_session(&path) {
                sessions.push(meta);
            }
        }
    }
    sessions
}

fn user_text(record: &Value) -> Option<String> {
    if record.get("type").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let text = record
        .pointer("/message/parts")
        .and_then(Value::as_array)?
        .iter()
        .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('/')).then(|| text.to_string())
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 40, 30).ok()?;
    let parse = |line: &String| serde_json::from_str::<Value>(line).ok();
    let head: Vec<Value> = head.iter().filter_map(parse).collect();
    let tail: Vec<Value> = tail.iter().filter_map(parse).collect();
    let first = head.first()?;
    let session_id = first
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })?;
    let project_dir = head
        .iter()
        .find_map(|record| record.get("cwd").and_then(Value::as_str))
        .map(str::to_string);
    let prompt = head.iter().find_map(user_text);
    // Sessions without a real prompt (e.g. aborted starts) are noise in the list.
    let prompt = prompt?;
    let created_at = first.get("timestamp").and_then(parse_timestamp_to_ms);
    let last_active_at = tail
        .iter()
        .rev()
        .find_map(|record| record.get("timestamp").and_then(parse_timestamp_to_ms))
        .or(created_at);
    Some(SessionMeta {
        provider_id: "qwen".into(),
        resume_command: resume_command(&session_id),
        session_id,
        title: Some(truncate_summary(
            prompt.lines().next().unwrap_or_default(),
            TITLE_MAX_CHARS,
        )),
        summary: Some(truncate_summary(&prompt, 200)),
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().into_owned()),
    })
}

fn resume_command(id: &str) -> Option<String> {
    let safe = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !safe {
        return None;
    }
    // A custom QWEN_HOME / QWEN_RUNTIME_DIR must reach the resumed process too.
    let mut prefix = String::new();
    for key in ["QWEN_HOME", "QWEN_RUNTIME_DIR"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                #[cfg(windows)]
                prefix.push_str(&format!("$env:{key} = '{}'; ", value.replace('\'', "''")));
                #[cfg(not(windows))]
                prefix.push_str(&format!(
                    "{key}={} ",
                    crate::session_manager::terminal::shell_escape(&value)
                ));
            }
        }
    }
    #[cfg(not(windows))]
    if !prefix.is_empty() {
        prefix.insert_str(0, "env ");
    }
    Some(format!("{prefix}qwen --resume {id}"))
}

fn usage_meta(record: &Value) -> Option<MessageMeta> {
    let usage = record.get("usageMetadata");
    let count = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
    };
    let meta = MessageMeta {
        model: record
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        input_tokens: count("promptTokenCount"),
        output_tokens: count("candidatesTokenCount"),
        cache_read_tokens: count("cachedContentTokenCount"),
        reasoning_tokens: count("thoughtsTokenCount"),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

fn tool_status(record: &Value, response: &Value) -> ToolStatus {
    match record
        .pointer("/toolCallResult/status")
        .and_then(Value::as_str)
    {
        Some("success") => ToolStatus::Success,
        Some("error") => ToolStatus::Error,
        Some("cancelled" | "canceled") => ToolStatus::Interrupted,
        _ if response.get("error").is_some() => ToolStatus::Error,
        _ if response.get("output").is_some() => ToolStatus::Success,
        _ => ToolStatus::Unknown,
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for_each_jsonl_value(path, |span, record| {
        let kind = record.get("type").and_then(Value::as_str).unwrap_or("");
        let parts = record
            .pointer("/message/parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let ts = record.get("timestamp").and_then(parse_timestamp_to_ms);
        let id = record
            .get("uuid")
            .and_then(Value::as_str)
            .map(str::to_string);
        let (role, blocks) = match kind {
            "user" => {
                let text = parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    return Ok(());
                }
                ("user", vec![SessionBlock::text(text)])
            }
            "assistant" => {
                let mut blocks = Vec::new();
                for (index, part) in parts.iter().enumerate() {
                    let base = format!("/message/parts/{index}");
                    if let Some(call) = part.get("functionCall") {
                        let Some(name) = call.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let call_id = call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("{name}-{index}"));
                        let args = call.get("args").cloned().unwrap_or(Value::Null);
                        blocks.push(tool_call_block(
                            ToolSource::Gemini,
                            call_id,
                            name,
                            &args,
                            || Some(span.content_ref(format!("{base}/functionCall/args"))),
                        ));
                    } else if let Some(text) = part.get("text").and_then(Value::as_str) {
                        if part.get("thought").and_then(Value::as_bool) == Some(true) {
                            blocks.push(thinking_block(text, None, None, || {
                                Some(span.content_ref(format!("{base}/text")))
                            }));
                        } else if !text.trim().is_empty() {
                            blocks.push(SessionBlock::text(text));
                        }
                    }
                }
                ("assistant", blocks)
            }
            "tool_result" => {
                let mut blocks = Vec::new();
                for (index, part) in parts.iter().enumerate() {
                    let Some(response) = part.get("functionResponse") else {
                        continue;
                    };
                    let call_id = response
                        .get("id")
                        .and_then(Value::as_str)
                        .or_else(|| {
                            record
                                .pointer("/toolCallResult/callId")
                                .and_then(Value::as_str)
                        })
                        .unwrap_or_default()
                        .to_string();
                    let body = response.get("response").cloned().unwrap_or(Value::Null);
                    let (text, pointer) = match ["output", "error"]
                        .iter()
                        .find_map(|key| body.get(*key).map(|value| (*key, value)))
                    {
                        Some((key, Value::String(text))) => (
                            text.clone(),
                            Some(format!(
                                "/message/parts/{index}/functionResponse/response/{key}"
                            )),
                        ),
                        Some((_, other)) => (other.to_string(), None),
                        None if body.is_null() => (String::new(), None),
                        None => (body.to_string(), None),
                    };
                    blocks.push(tool_result_block(
                        call_id,
                        tool_status(&record, &body),
                        &text,
                        || pointer.map(|pointer| span.content_ref(pointer)),
                    ));
                }
                ("tool", blocks)
            }
            _ => return Ok(()),
        };
        let mut message = SessionMessage::from_blocks(role, ts, blocks);
        if message.is_empty() {
            return Ok(());
        }
        message.id = id;
        if role == "assistant" {
            message.meta = usage_meta(&record);
        }
        messages.push(message);
        Ok(())
    })?;
    assign_turn_ids(&mut messages);
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = r#"{"uuid":"u1","parentUuid":null,"sessionId":"ae318303-3f4e","timestamp":"2026-10-22T17:31:26.869Z","type":"user","cwd":"/work/proj","message":{"role":"user","parts":[{"text":"list txt files"}]}}
{"uuid":"s1","sessionId":"ae318303-3f4e","timestamp":"2026-10-22T17:31:27.000Z","type":"system","subtype":"ui_telemetry","systemPayload":{}}
{"uuid":"a1","sessionId":"ae318303-3f4e","timestamp":"2026-10-22T17:31:27.904Z","type":"assistant","model":"qwen3-coder-plus","message":{"role":"model","parts":[{"text":"thinking...","thought":true},{"text":"Let me look."},{"functionCall":{"id":"call_1","name":"glob","args":{"pattern":"*.txt"}}}]},"usageMetadata":{"promptTokenCount":900,"candidatesTokenCount":20,"thoughtsTokenCount":5,"cachedContentTokenCount":100}}
{"uuid":"t1","sessionId":"ae318303-3f4e","timestamp":"2026-10-22T17:31:28.220Z","type":"tool_result","message":{"role":"user","parts":[{"functionResponse":{"id":"call_1","name":"glob","response":{"output":"/work/proj/a.txt"}}}]},"toolCallResult":{"callId":"call_1","status":"success"}}
{"uuid":"a2","sessionId":"ae318303-3f4e","timestamp":"2026-10-22T17:31:29.000Z","type":"assistant","model":"qwen3-coder-plus","message":{"role":"model","parts":[{"text":"Found a.txt."}]}}
"#;

    fn write_session(root: &Path) -> PathBuf {
        let chats = root.join("projects").join("-work-proj").join("chats");
        fs::create_dir_all(&chats).unwrap();
        let path = chats.join("ae318303-3f4e.jsonl");
        fs::write(&path, SESSION).unwrap();
        path
    }

    #[test]
    fn qwen_scan_reads_title_cwd_and_resume_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path());
        fs::write(dir.path().join("projects/-work-proj/chats/empty.jsonl"), "").unwrap();
        let sessions = scan(&dir.path().join("projects"));
        assert_eq!(sessions.len(), 1);
        let meta = &sessions[0];
        assert_eq!(meta.session_id, "ae318303-3f4e");
        assert_eq!(meta.title.as_deref(), Some("list txt files"));
        assert_eq!(meta.project_dir.as_deref(), Some("/work/proj"));
        assert!(meta
            .resume_command
            .as_deref()
            .unwrap()
            .ends_with("qwen --resume ae318303-3f4e"));
        assert_eq!(meta.source_path.as_deref(), path.to_str());
        assert!(meta.last_active_at > meta.created_at);
    }

    #[test]
    fn qwen_messages_pair_tool_calls_and_keep_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path());
        let messages = load_messages(&path).unwrap();
        let roles: Vec<_> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["user", "assistant", "tool", "assistant"]);
        let assistant = &messages[1];
        assert!(matches!(assistant.blocks[0], SessionBlock::Thinking { .. }));
        assert!(
            matches!(&assistant.blocks[2], SessionBlock::ToolCall { id, .. } if id == "call_1")
        );
        let meta = assistant.meta.as_ref().unwrap();
        assert_eq!(meta.input_tokens, Some(900));
        assert_eq!(meta.cache_read_tokens, Some(100));
        assert_eq!(meta.model.as_deref(), Some("qwen3-coder-plus"));
        assert!(matches!(
            &messages[2].blocks[0],
            SessionBlock::ToolResult { call_id, status: ToolStatus::Success, .. } if call_id == "call_1"
        ));
    }
}
