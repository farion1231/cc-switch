//! TRAE CLI 2.0 writes Codex-derived rollouts to `$TRAE_HOME/cli/sessions/YYYY/MM/DD/`.
//! Conversation items are committed through `history_mutation` records whose
//! `items` use the OpenAI Responses item shapes (message / function_call /
//! function_call_output / reasoning).
use super::blocks::{self, ToolSource};
use super::utils;
use crate::session_manager::model::{SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

/// Session metadata and the first prompt sit near the start of the file.
const HEAD_LINES: usize = 64;

pub(crate) fn session_roots() -> Vec<PathBuf> {
    vec![crate::trae_config::data_dir().join("cli").join("sessions")]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = Vec::new();
    for root in session_roots() {
        let mut files = Vec::new();
        collect_rollouts(&root, 0, &mut files);
        sessions.extend(files.iter().filter_map(|path| parse_meta(path)));
    }
    sessions
}

fn collect_rollouts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    // YYYY/MM/DD: three levels below the root.
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollouts(&path, depth + 1, out);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            out.push(path);
        }
    }
}

/// Real user input is tagged `user.text`; everything else (AGENTS.md,
/// environment context, system reminders) is context traecli injected.
fn is_user_text(item: &Value) -> bool {
    item.pointer("/internal_chat_message_metadata_passthrough/content_item_kinds")
        .and_then(Value::as_array)
        .is_some_and(|kinds| kinds.iter().any(|kind| kind == "user.text"))
}

fn item_text(item: &Value) -> String {
    item.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn appended_items(record: &Value) -> impl Iterator<Item = &Value> {
    let append = record.get("type").and_then(Value::as_str) == Some("history_mutation")
        && record.pointer("/payload/operation").and_then(Value::as_str) == Some("append");
    record
        .pointer("/payload/items")
        .and_then(Value::as_array)
        .filter(|_| append)
        .into_iter()
        .flatten()
}

fn parse_meta(path: &Path) -> Option<SessionMeta> {
    let file = fs::File::open(path).ok()?;
    let mut meta = None;
    let mut first_prompt = None;
    for line in BufReader::new(file).lines().take(HEAD_LINES) {
        let Ok(record) = serde_json::from_str::<Value>(&line.ok()?) else {
            continue;
        };
        if record["type"] == "session_meta" {
            // Sub-agent threads are listed under their parent session.
            if record
                .pointer("/payload/source")
                .is_some_and(Value::is_object)
            {
                return None;
            }
            meta = Some(record["payload"].clone());
        }
        if first_prompt.is_none() {
            first_prompt = appended_items(&record)
                .find(|item| item["role"] == "user" && is_user_text(item))
                .map(item_text)
                .filter(|text| !text.trim().is_empty());
        }
        if meta.is_some() && first_prompt.is_some() {
            break;
        }
    }
    let meta = meta?;
    let id = meta
        .get("id")
        .or_else(|| meta.get("session_id"))
        .and_then(Value::as_str)?
        .to_string();
    let last_active_at = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64);
    Some(SessionMeta {
        provider_id: "trae".into(),
        title: first_prompt
            .as_deref()
            .map(|text| utils::truncate_summary(text, utils::TITLE_MAX_CHARS)),
        summary: first_prompt.map(|text| utils::truncate_summary(&text, 200)),
        project_dir: meta.get("cwd").and_then(Value::as_str).map(str::to_owned),
        created_at: meta.get("timestamp").and_then(utils::parse_timestamp_to_ms),
        last_active_at,
        source_path: Some(path.to_string_lossy().into_owned()),
        resume_command: resume_command(&id),
        session_id: id,
    })
}

fn resume_command(id: &str) -> Option<String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return None;
    }
    let Some(home) = std::env::var_os("TRAE_HOME").filter(|value| !value.is_empty()) else {
        return Some(format!("traecli resume {id}"));
    };
    let home = home.to_string_lossy();
    #[cfg(windows)]
    let command = format!(
        "$env:TRAE_HOME = '{}'; traecli resume {id}",
        home.replace('\'', "''")
    );
    #[cfg(not(windows))]
    let command = format!(
        "env TRAE_HOME={} traecli resume {id}",
        crate::session_manager::terminal::shell_escape(&home)
    );
    Some(command)
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut messages: Vec<SessionMessage> = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| error.to_string())?;
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let ts = record
            .get("timestamp")
            .and_then(utils::parse_timestamp_to_ms);
        let turn = record
            .pointer("/payload/turn_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        for item in appended_items(&record) {
            let Some(mut message) = project_item(item, ts) else {
                continue;
            };
            message.turn_id = turn.clone();
            message.id = item.get("id").and_then(Value::as_str).map(str::to_owned);
            messages.push(message);
        }
    }
    Ok(messages)
}

fn project_item(item: &Value, ts: Option<i64>) -> Option<SessionMessage> {
    match item.get("type").and_then(Value::as_str)? {
        "message" => {
            let role = item.get("role").and_then(Value::as_str)?;
            let text = item_text(item);
            if text.trim().is_empty() {
                return None;
            }
            let (role, injected) = match role {
                "assistant" => ("assistant", false),
                "user" => ("user", !is_user_text(item)),
                _ => ("system", true),
            };
            let mut message = SessionMessage::from_blocks(
                role,
                ts,
                vec![blocks::large_text_block(&text, || None)],
            );
            message.injected = injected;
            Some(message)
        }
        "function_call" | "custom_tool_call" => {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let input = blocks::parse_arguments(
                item.get("arguments")
                    .or_else(|| item.get("input"))
                    .unwrap_or(&Value::Null),
            );
            let block = blocks::tool_call_block(ToolSource::Claude, call_id, name, &input, || None);
            Some(SessionMessage::from_blocks("assistant", ts, vec![block]))
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let output = match item.get("output") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => utils::extract_text(other),
                None => String::new(),
            };
            let block = blocks::tool_result_block(call_id, ToolStatus::Success, &output, || None);
            Some(SessionMessage::from_blocks("tool", ts, vec![block]))
        }
        "reasoning" => {
            let summary = item
                .get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            let text = item_text(item);
            let visible = if text.is_empty() { &summary } else { &text };
            if visible.trim().is_empty() {
                return None;
            }
            let block: SessionBlock = blocks::thinking_block(visible, None, None, || None);
            Some(SessionMessage::from_blocks("assistant", ts, vec![block]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn append(turn: &str, items: Value) -> String {
        json!({"timestamp":"2026-10-23T06:36:43.000Z","type":"history_mutation","payload":{"version":1,"turn_id":turn,"operation":"append","items":items}}).to_string()
    }

    fn fixture(dir: &Path) -> PathBuf {
        let path = dir.join("cli/sessions/2026/10/23/rollout-2026-10-23T14-36-43-abc-123.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let lines = [
            json!({"timestamp":"2026-10-23T06:36:43.000Z","type":"session_meta","payload":{"id":"abc-123","timestamp":"2026-10-23T06:36:43.000Z","cwd":"/work","source":"cli","model_provider":"custom"}}).to_string(),
            append("t1", json!([
                {"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions\n\nbe nice"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["agents_md.instructions"]}},
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"# Environment"}]}
            ])),
            append("t1", json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"run echo"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text"]}}])),
            append("t1", json!([{"type":"function_call","id":"fc_1","name":"Bash","arguments":"{\"command\":\"echo hi\"}","call_id":"call_1"}])),
            append("t1", json!([{"type":"function_call_output","call_id":"call_1","output":"hi"}])),
            append("t1", json!([{"type":"message","id":"msg_2","role":"assistant","content":[{"type":"output_text","text":"Done."}]}])),
            "{truncated".to_string(),
        ];
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    #[test]
    fn trae_rollout_lists_metadata_and_projects_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture(dir.path());
        let meta = parse_meta(&path).unwrap();
        assert_eq!(meta.session_id, "abc-123");
        assert_eq!(meta.project_dir.as_deref(), Some("/work"));
        assert_eq!(meta.title.as_deref(), Some("run echo"));
        assert!(meta
            .resume_command
            .unwrap()
            .ends_with("traecli resume abc-123"));

        let messages = load_messages(&path).unwrap();
        let roles: Vec<_> = messages
            .iter()
            .map(|m| (m.role.as_str(), m.injected))
            .collect();
        assert_eq!(
            roles,
            [
                ("user", true),
                ("system", true),
                ("user", false),
                ("assistant", false),
                ("tool", false),
                ("assistant", false)
            ]
        );
        assert!(
            matches!(&messages[3].blocks[0], SessionBlock::ToolCall { raw_name, .. } if raw_name == "Bash")
        );
        assert!(
            matches!(&messages[4].blocks[0], SessionBlock::ToolResult { call_id, .. } if call_id == "call_1")
        );
        assert_eq!(messages[5].content, "Done.");
        assert_eq!(messages[5].turn_id.as_deref(), Some("t1"));
    }

    #[test]
    fn trae_subagent_rollouts_are_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        fs::write(
            &path,
            json!({"type":"session_meta","payload":{"id":"child","source":{"subagent":"Explore"}}})
                .to_string(),
        )
        .unwrap();
        assert!(parse_meta(&path).is_none());
    }
}
