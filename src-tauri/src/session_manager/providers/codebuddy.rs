//! CodeBuddy Code sessions: `<config>/projects/<sanitized cwd>/<sessionId>.jsonl`.
//!
//! Every line is one event carrying `id`, `parentId`, `timestamp` (ms), `sessionId`
//! and `cwd`. Conversation events are `message` (`role` user | assistant, `content`
//! = `[{type, text}]`), `reasoning` (`rawContent`), `function_call` (`callId`, `name`,
//! JSON `arguments` string) and `function_call_result` (`callId`, `status`,
//! `output.text`). `ai-title` carries the generated title; `session-meta` and
//! `file-history-snapshot` are bookkeeping. User messages whose `providerData.isMeta`
//! is set are injected reminders, not something the user typed. The model request
//! behind a step (`providerData.stepSeq`) reports its usage on whichever record ends
//! that step: the assistant message, or the first function call when it only calls
//! tools.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::services::session_usage_codebuddy::{request_model, token_counts};
use crate::session_manager::model::{MessageMeta, SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, thinking_block, tool_call_block, tool_result_block, ToolSource,
};
use super::utils::{
    for_each_jsonl_value, parse_timestamp_to_ms, read_head_tail_lines, truncate_summary,
    TITLE_MAX_CHARS,
};

pub(crate) fn projects_dir() -> PathBuf {
    crate::codebuddy_config::data_dir().join("projects")
}

pub(crate) fn session_roots() -> Vec<PathBuf> {
    vec![projects_dir()]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan(&projects_dir())
}

/// Top-level `*.jsonl` transcripts of every project directory. Same-named
/// sub-directories only hold sidecar files (`tool-results`).
pub(crate) fn session_files(projects: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(projects) = fs::read_dir(projects) else {
        return files;
    };
    for project in projects.flatten() {
        let Ok(entries) = fs::read_dir(project.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") && path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

fn scan(projects: &Path) -> Vec<SessionMeta> {
    session_files(projects)
        .iter()
        .filter_map(|path| parse_session(path))
        .collect()
}

fn kind(record: &Value) -> &str {
    record.get("type").and_then(Value::as_str).unwrap_or("")
}

fn role(record: &Value) -> &str {
    record.get("role").and_then(Value::as_str).unwrap_or("")
}

fn is_meta(record: &Value) -> bool {
    record
        .pointer("/providerData/isMeta")
        .and_then(Value::as_bool)
        == Some(true)
}

fn content_text(record: &Value, key: &str) -> String {
    record
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text the user actually typed (injected meta messages excluded).
fn user_text(record: &Value) -> Option<String> {
    if kind(record) != "message" || role(record) != "user" || is_meta(record) {
        return None;
    }
    let text = content_text(record, "content");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn assistant_text(record: &Value) -> Option<String> {
    if kind(record) != "message" || role(record) != "assistant" {
        return None;
    }
    let text = content_text(record, "content");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn ai_title(record: &Value) -> Option<String> {
    if kind(record) != "ai-title" {
        return None;
    }
    record
        .get("aiTitle")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 60, 40).ok()?;
    let parse = |line: &String| serde_json::from_str::<Value>(line).ok();
    let head: Vec<Value> = head.iter().filter_map(parse).collect();
    let tail: Vec<Value> = tail.iter().filter_map(parse).collect();
    let session_id = head
        .iter()
        .find_map(|record| record.get("sessionId").and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })?;
    // Teammate sessions start from a channel message instead of a user prompt, so
    // fall back to the first reply; files without any conversation are skipped.
    let prompt = head
        .iter()
        .find_map(user_text)
        .or_else(|| head.iter().find_map(assistant_text))?;
    let title = tail
        .iter()
        .rev()
        .find_map(ai_title)
        .or_else(|| head.iter().rev().find_map(ai_title))
        .unwrap_or_else(|| prompt.lines().next().unwrap_or_default().to_string());
    let project_dir = head
        .iter()
        .find_map(|record| record.get("cwd").and_then(Value::as_str))
        .map(str::to_string);
    let created_at = head
        .iter()
        .find_map(|record| record.get("timestamp").and_then(parse_timestamp_to_ms));
    let last_active_at = tail
        .iter()
        .rev()
        .find_map(|record| record.get("timestamp").and_then(parse_timestamp_to_ms))
        .or(created_at);
    Some(SessionMeta {
        provider_id: "codebuddy".into(),
        resume_command: resume_command(&session_id),
        session_id,
        title: Some(truncate_summary(&title, TITLE_MAX_CHARS)),
        summary: Some(truncate_summary(&prompt, 200)),
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().into_owned()),
    })
}

/// `codebuddy --resume <id>`; a custom `CODEBUDDY_CONFIG_DIR` must reach the
/// resumed process too, otherwise it looks for the session in `~/.codebuddy`.
fn resume_command(id: &str) -> Option<String> {
    let safe = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !safe {
        return None;
    }
    let mut prefix = String::new();
    if let Ok(value) = std::env::var("CODEBUDDY_CONFIG_DIR") {
        if !value.trim().is_empty() {
            #[cfg(windows)]
            prefix.push_str(&format!(
                "$env:CODEBUDDY_CONFIG_DIR = '{}'; ",
                value.replace('\'', "''")
            ));
            #[cfg(not(windows))]
            prefix.push_str(&format!(
                "env CODEBUDDY_CONFIG_DIR={} ",
                crate::session_manager::terminal::shell_escape(&value)
            ));
        }
    }
    Some(format!("{prefix}codebuddy --resume {id}"))
}

fn usage_meta(record: &Value) -> Option<MessageMeta> {
    let provider = record.get("providerData")?;
    let counts = token_counts(provider);
    let positive = |n: u64| (n > 0).then_some(n);
    let meta = MessageMeta {
        model: request_model(provider),
        input_tokens: counts.and_then(|c| positive(c.input)),
        output_tokens: counts.and_then(|c| positive(c.output)),
        cache_read_tokens: counts.and_then(|c| positive(c.cache_read)),
        cache_write_tokens: counts.and_then(|c| positive(c.cache_write)),
        reasoning_tokens: counts.and_then(|c| positive(c.reasoning)),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

fn tool_status(record: &Value) -> ToolStatus {
    match record.get("status").and_then(Value::as_str) {
        Some("completed" | "success") => ToolStatus::Success,
        Some("failed" | "error") => ToolStatus::Error,
        Some("incomplete" | "cancelled" | "canceled" | "interrupted") => ToolStatus::Interrupted,
        Some("in_progress" | "pending") => ToolStatus::Pending,
        _ => ToolStatus::Unknown,
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for_each_jsonl_value(path, |span, record| {
        let ts = record.get("timestamp").and_then(parse_timestamp_to_ms);
        let (role, blocks, with_usage) = match kind(&record) {
            "message" if role(&record) == "user" => {
                let Some(text) = user_text(&record) else {
                    return Ok(());
                };
                ("user", vec![SessionBlock::text(text)], false)
            }
            "message" if role(&record) == "assistant" => {
                let text = content_text(&record, "content");
                let blocks = if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![SessionBlock::text(text)]
                };
                ("assistant", blocks, true)
            }
            "reasoning" => {
                let mut text = content_text(&record, "rawContent");
                if text.trim().is_empty() {
                    text = content_text(&record, "content");
                }
                if text.trim().is_empty() {
                    return Ok(());
                }
                let single = record
                    .get("rawContent")
                    .and_then(Value::as_array)
                    .is_some_and(|items| items.len() == 1);
                let block = thinking_block(&text, None, None, || {
                    single.then(|| span.content_ref("/rawContent/0/text"))
                });
                ("assistant", vec![block], false)
            }
            "function_call" => {
                let Some(name) = record.get("name").and_then(Value::as_str) else {
                    return Ok(());
                };
                let call_id = record
                    .get("callId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let raw = record.get("arguments").cloned().unwrap_or(Value::Null);
                let input = match &raw {
                    Value::String(text) => {
                        serde_json::from_str(text).unwrap_or_else(|_| raw.clone())
                    }
                    other => other.clone(),
                };
                let block = tool_call_block(ToolSource::Claude, call_id, name, &input, || {
                    Some(span.content_ref("/arguments"))
                });
                ("assistant", vec![block], true)
            }
            "function_call_result" => {
                let call_id = record
                    .get("callId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let (text, pointer) = match record.get("output") {
                    Some(Value::String(text)) => (text.clone(), Some("/output")),
                    Some(output) => match output.get("text") {
                        Some(Value::String(text)) => (text.clone(), Some("/output/text")),
                        _ => (output.to_string(), None),
                    },
                    None => (String::new(), None),
                };
                let block = tool_result_block(call_id, tool_status(&record), &text, || {
                    pointer.map(|pointer| span.content_ref(pointer))
                });
                ("tool", vec![block], false)
            }
            _ => return Ok(()),
        };
        let meta = with_usage.then(|| usage_meta(&record)).flatten();
        if blocks.is_empty() && meta.is_none() {
            return Ok(());
        }
        let mut message = SessionMessage::from_blocks(role, ts, blocks);
        if message.is_empty() && meta.is_none() {
            return Ok(());
        }
        message.id = record.get("id").and_then(Value::as_str).map(str::to_string);
        message.meta = meta;
        messages.push(message);
        Ok(())
    })?;
    assign_turn_ids(&mut messages);
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic transcript in CodeBuddy Code 2.164's event shape.
    const SESSION: &str = r#"{"type":"session-meta","id":"m0","sessionId":"0f3c-41aa","timestamp":1791900000000,"meta":{}}
{"id":"u1","parentId":null,"timestamp":1791900000100,"sessionId":"0f3c-41aa","cwd":"/work/proj","type":"message","role":"user","content":[{"type":"input_text","text":"list the txt files"}],"providerData":{"agent":"cli"}}
{"id":"r1","parentId":"u1","timestamp":1791900000200,"sessionId":"0f3c-41aa","cwd":"/work/proj","type":"reasoning","content":[],"rawContent":[{"type":"reasoning_text","text":"Use a glob."}],"providerData":{"messageId":"msg1","stepSeq":1,"model":"hy4-preview"}}
{"id":"c1","parentId":"r1","timestamp":1791900000300,"sessionId":"0f3c-41aa","cwd":"/work/proj","type":"function_call","callId":"call_1","name":"Bash","arguments":"{\"command\":\"ls *.txt\"}","providerData":{"messageId":"msg1","stepSeq":1,"model":"hy4-preview","rawUsage":{"prompt_tokens":1200,"completion_tokens":40,"prompt_cache_hit_tokens":1000,"prompt_cache_miss_tokens":200,"completion_thinking_tokens":12}}}
{"id":"o1","parentId":"c1","timestamp":1791900000400,"sessionId":"0f3c-41aa","cwd":"/work/proj","type":"function_call_result","callId":"call_1","name":"Bash","status":"completed","output":{"type":"text","text":"a.txt"}}
{"id":"u2","parentId":"o1","timestamp":1791900000500,"sessionId":"0f3c-41aa","cwd":"/work/proj","type":"message","role":"user","content":[{"type":"input_text","text":"<system-reminder>use the task tools</system-reminder>"}],"providerData":{"isMeta":true,"taskReminderKind":"task"}}
{"timestamp":1791900000550,"type":"file-history-snapshot","snapshot":{}}
{"id":"a1","parentId":"u2","timestamp":1791900000600,"sessionId":"0f3c-41aa","cwd":"/work/proj","type":"message","role":"assistant","content":[{"type":"output_text","text":"Found a.txt."}],"providerData":{"messageId":"msg1","stepSeq":2,"model":"hy4-preview","rawUsage":{"prompt_tokens":1300,"completion_tokens":8,"prompt_cache_hit_tokens":1200,"prompt_cache_miss_tokens":100}}}
{"id":"t1","timestamp":1791900000700,"sessionId":"0f3c-41aa","type":"ai-title","aiTitle":"List text files"}
"#;

    fn write_session(root: &Path) -> PathBuf {
        let project = root.join("projects").join("work-proj");
        fs::create_dir_all(project.join("0f3c-41aa").join("tool-results")).unwrap();
        let path = project.join("0f3c-41aa.jsonl");
        fs::write(&path, SESSION).unwrap();
        path
    }

    #[test]
    fn codebuddy_scan_reads_title_cwd_and_resume_command() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path());
        fs::write(dir.path().join("projects/work-proj/empty.jsonl"), "").unwrap();
        let sessions = scan(&dir.path().join("projects"));
        assert_eq!(sessions.len(), 1);
        let meta = &sessions[0];
        assert_eq!(meta.provider_id, "codebuddy");
        assert_eq!(meta.session_id, "0f3c-41aa");
        assert_eq!(meta.title.as_deref(), Some("List text files"));
        assert_eq!(meta.summary.as_deref(), Some("list the txt files"));
        assert_eq!(meta.project_dir.as_deref(), Some("/work/proj"));
        assert!(meta
            .resume_command
            .as_deref()
            .unwrap()
            .ends_with("codebuddy --resume 0f3c-41aa"));
        assert_eq!(meta.source_path.as_deref(), path.to_str());
        assert_eq!(meta.created_at, Some(1791900000000));
        assert_eq!(meta.last_active_at, Some(1791900000700));
    }

    #[test]
    fn codebuddy_teammate_session_without_prompt_uses_first_reply() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects").join("p");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("s2.jsonl"),
            r#"{"id":"c","timestamp":1,"sessionId":"s2","type":"function_call","callId":"x","name":"SpeakInChannel","arguments":"{}","providerData":{"manual":true}}
{"id":"a","timestamp":2,"sessionId":"s2","type":"message","role":"assistant","content":[{"type":"output_text","text":"Reporting in."}]}
"#,
        )
        .unwrap();
        let sessions = scan(&dir.path().join("projects"));
        assert_eq!(sessions[0].title.as_deref(), Some("Reporting in."));
    }

    #[test]
    fn codebuddy_messages_hide_meta_and_pair_tool_calls_with_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path());
        let messages = load_messages(&path).unwrap();
        let roles: Vec<_> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            ["user", "assistant", "assistant", "tool", "assistant"]
        );
        assert!(matches!(
            messages[1].blocks[0],
            SessionBlock::Thinking { .. }
        ));
        assert!(
            matches!(&messages[2].blocks[0], SessionBlock::ToolCall { id, .. } if id == "call_1")
        );
        let meta = messages[2].meta.as_ref().unwrap();
        assert_eq!(meta.model.as_deref(), Some("hy4-preview"));
        assert_eq!(meta.input_tokens, Some(200));
        assert_eq!(meta.cache_read_tokens, Some(1000));
        assert_eq!(meta.output_tokens, Some(40));
        assert_eq!(meta.reasoning_tokens, Some(12));
        assert!(matches!(
            &messages[3].blocks[0],
            SessionBlock::ToolResult { call_id, status: ToolStatus::Success, .. } if call_id == "call_1"
        ));
        assert!(messages
            .iter()
            .all(|m| !m.content.contains("system-reminder")
                && m.blocks.iter().all(|b| !matches!(b, SessionBlock::Text { text, .. } if text.contains("system-reminder")))));
        assert_eq!(messages[4].meta.as_ref().unwrap().input_tokens, Some(100));
    }
}
