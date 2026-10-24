//! GitHub Copilot CLI 的会话：`~/.copilot/session-state/<id>/events.jsonl`，旁边的
//! `workspace.yaml` 存会话名、工作目录和时间（旧版本可能没有，退回 `session.start`）。
//!
//! 事件流里 `user.message` / `assistant.message` 是正文，`assistant.message.toolRequests`
//! 是工具调用，`tool.execution_complete` 是结果；带 `parentToolCallId` 的是子代理内部的
//! 消息，不进主会话。`session.usage_record`（新版本才有）在对应的回复之前写出。
use super::{blocks, utils};
use crate::session_manager::model::{ContentRef, EventKind, MessageMeta, SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const PROVIDER_ID: &str = "copilot";

static PARSE_CACHE: LazyLock<utils::FileParseCache> = LazyLock::new(utils::FileParseCache::new);

pub(crate) fn session_roots() -> Vec<PathBuf> {
    vec![crate::copilot_config::session_state_dir()]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let files = event_files(&crate::copilot_config::session_state_dir());
    PARSE_CACHE.scan(files, |path| Ok(parse_session(path)))
}

fn event_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path().join("events.jsonl"))
        .filter(|path| path.is_file())
        .collect()
}

#[derive(Default)]
struct Workspace {
    id: Option<String>,
    cwd: Option<String>,
    name: Option<String>,
    summary: Option<String>,
    created_at: Option<i64>,
    updated_at: Option<i64>,
}

fn read_workspace(dir: &Path) -> Workspace {
    let Some(doc) = std::fs::read_to_string(dir.join("workspace.yaml"))
        .ok()
        .and_then(|text| serde_yaml::from_str::<serde_yaml::Value>(&text).ok())
    else {
        return Workspace::default();
    };
    let text = |key: &str| {
        doc.get(key)
            .and_then(serde_yaml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let time =
        |key: &str| text(key).and_then(|value| utils::parse_timestamp_to_ms(&Value::String(value)));
    Workspace {
        id: text("id"),
        cwd: text("cwd"),
        name: text("name"),
        summary: text("summary"),
        created_at: time("created_at"),
        updated_at: time("updated_at"),
    }
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let dir = path.parent()?;
    let workspace = read_workspace(dir);
    let (head, tail) = utils::read_head_tail_lines(path, 64, 8).ok()?;
    let mut session_id = workspace.id.clone();
    let mut cwd = workspace.cwd.clone();
    let mut created_at = workspace.created_at;
    let mut first_prompt = None;
    for line in &head {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["type"].as_str() {
            Some("session.start") => {
                let data = &event["data"];
                session_id = session_id.or_else(|| data["sessionId"].as_str().map(str::to_string));
                cwd = cwd.or_else(|| data.pointer("/context/cwd")?.as_str().map(str::to_string));
                created_at =
                    created_at.or_else(|| utils::parse_timestamp_to_ms(&data["startTime"]));
            }
            Some("user.message")
                if first_prompt.is_none() && event.pointer("/data/parentToolCallId").is_none() =>
            {
                first_prompt = event["data"]["content"]
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string);
            }
            _ => {}
        }
    }
    let session_id = session_id.or_else(|| {
        dir.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })?;
    let last_event_at = tail.iter().rev().find_map(|line| {
        serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|event| utils::parse_timestamp_to_ms(&event["timestamp"]))
    });
    let title = workspace
        .name
        .clone()
        .or_else(|| first_prompt.clone())
        .map(|text| utils::truncate_summary(&text, utils::TITLE_MAX_CHARS));
    let summary = workspace
        .summary
        .or(first_prompt)
        .map(|text| utils::truncate_summary(&text, 200));
    Some(SessionMeta {
        provider_id: PROVIDER_ID.into(),
        resume_command: valid_session_id(&session_id)
            .then(|| crate::copilot_config::resume_command(&session_id)),
        session_id,
        title,
        summary,
        project_dir: cwd,
        created_at,
        last_active_at: last_event_at.max(workspace.updated_at).or(created_at),
        source_path: Some(path.to_string_lossy().into_owned()),
    })
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// 回复前最近一次 `session.usage_record` 的用量。
#[derive(Default, Clone)]
struct PendingUsage {
    model: Option<String>,
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
    duration_ms: Option<u64>,
}

impl PendingUsage {
    fn of(usage: &Value) -> Self {
        let cache_read = usage["cacheReadTokens"].as_u64();
        Self {
            model: usage["model"].as_str().map(str::to_string),
            // Copilot 的 inputTokens 含缓存命中，这里展示新输入。
            input: usage["inputTokens"]
                .as_u64()
                .map(|input| input.saturating_sub(cache_read.unwrap_or(0))),
            output: usage["outputTokens"].as_u64(),
            cache_read,
            cache_write: usage["cacheWriteTokens"].as_u64(),
            duration_ms: usage["duration"].as_u64(),
        }
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    let mut pending: Option<PendingUsage> = None;
    utils::for_each_jsonl_value(path, |span, event| {
        let data = &event["data"];
        // 子代理内部的消息和工具调用不进主会话。
        if data.get("parentToolCallId").is_some_and(|v| !v.is_null()) {
            return Ok(());
        }
        let ts = utils::parse_timestamp_to_ms(&event["timestamp"]);
        let reference = |pointer: String| -> Option<ContentRef> { Some(span.content_ref(pointer)) };
        match event["type"].as_str() {
            Some("user.message") => {
                let text = data["content"].as_str().unwrap_or_default();
                if text.trim().is_empty() {
                    return Ok(());
                }
                let mut message =
                    SessionMessage::from_blocks("user", ts, vec![SessionBlock::text(text)]);
                message.id = data["messageId"].as_str().map(str::to_string);
                messages.push(message);
            }
            Some("session.usage_record") => {
                pending = Some(PendingUsage::of(&data["usage"]));
            }
            Some("assistant.message") => {
                let mut message_blocks = Vec::new();
                if let Some(reasoning) = data["reasoningText"].as_str() {
                    if !reasoning.trim().is_empty() {
                        message_blocks.push(blocks::thinking_block(reasoning, None, None, || {
                            reference("/data/reasoningText".into())
                        }));
                    }
                }
                if let Some(text) = data["content"].as_str() {
                    if !text.trim().is_empty() {
                        message_blocks.push(SessionBlock::text(text));
                    }
                }
                for (i, call) in data["toolRequests"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let name = call["name"].as_str().unwrap_or_default();
                    // report_intent 只是给界面看的意图标签，不是实际操作。
                    if name == "report_intent" {
                        continue;
                    }
                    let input = blocks::parse_arguments(&call["arguments"]);
                    message_blocks.push(blocks::tool_call_block(
                        blocks::ToolSource::Generic,
                        call["toolCallId"].as_str().unwrap_or_default(),
                        name,
                        &input,
                        || reference(format!("/data/toolRequests/{i}/arguments")),
                    ));
                }
                if message_blocks.is_empty() {
                    return Ok(());
                }
                let mut message = SessionMessage::from_blocks("assistant", ts, message_blocks);
                message.id = data["messageId"].as_str().map(str::to_string);
                let usage = pending.take().unwrap_or_default();
                let model = data["model"]
                    .as_str()
                    .map(str::to_string)
                    .or(usage.model.clone());
                let output = usage.output.or_else(|| data["outputTokens"].as_u64());
                if model.is_some() || output.is_some() || usage.input.is_some() {
                    message.meta = Some(MessageMeta {
                        model,
                        input_tokens: usage.input,
                        output_tokens: output,
                        cache_read_tokens: usage.cache_read,
                        cache_write_tokens: usage.cache_write,
                        duration_ms: usage.duration_ms,
                        ..Default::default()
                    });
                }
                messages.push(message);
            }
            Some("tool.execution_complete") => {
                let call_id = data["toolCallId"].as_str().unwrap_or_default();
                if data
                    .pointer("/result/detailedContent")
                    .and_then(Value::as_str)
                    .is_some()
                    && data["result"]["content"] == "Intent logged"
                {
                    return Ok(());
                }
                let success = data["success"].as_bool().unwrap_or(true);
                let (text, pointer) = if success {
                    (
                        data.pointer("/result/content")
                            .map(utils::extract_text)
                            .unwrap_or_default(),
                        "/data/result/content",
                    )
                } else {
                    (
                        data.pointer("/error/message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        "/data/error/message",
                    )
                };
                let status = if success {
                    ToolStatus::Success
                } else {
                    ToolStatus::Error
                };
                let block =
                    blocks::tool_result_block(call_id, status, &text, || reference(pointer.into()));
                messages.push(SessionMessage::from_blocks("tool", ts, vec![block]));
            }
            Some("session.compaction_complete") => {
                if let Some(summary) = data["summaryContent"].as_str() {
                    let block = blocks::summary_event_block(EventKind::Compaction, summary, || {
                        reference("/data/summaryContent".into())
                    });
                    let mut message = SessionMessage::from_blocks("system", ts, vec![block]);
                    message.injected = true;
                    messages.push(message);
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    blocks::assign_turn_ids(&mut messages);
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_events(path: &Path, events: &[Value]) {
        std::fs::write(
            path,
            events
                .iter()
                .map(|event| format!("{event}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }

    fn fixture(dir: &Path) -> PathBuf {
        let session = dir.join("0cb916db-0000-4000-8000-000000000001");
        std::fs::create_dir_all(&session).unwrap();
        let events = session.join("events.jsonl");
        write_events(
            &events,
            &[
                json!({"type":"session.start","timestamp":"2026-10-10T09:07:56.748Z","data":{"sessionId":"0cb916db-0000-4000-8000-000000000001","startTime":"2026-10-10T09:07:56.720Z","context":{"cwd":"/work/project"}}}),
                json!({"type":"system.message","timestamp":"2026-10-10T09:07:57.000Z","data":{"role":"system","content":"You are GitHub Copilot"}}),
                json!({"type":"user.message","timestamp":"2026-10-10T09:08:00.043Z","data":{"content":"Fix the build","transformedContent":"<current_datetime/>Fix the build","messageId":"u1"}}),
                json!({"type":"session.usage_record","timestamp":"2026-10-10T09:08:01.000Z","data":{"usage":{"model":"gpt-5-mini","inputTokens":100,"outputTokens":12,"cacheReadTokens":30,"cacheWriteTokens":0,"duration":2988}}}),
                json!({"type":"assistant.message","timestamp":"2026-10-10T09:08:02.000Z","data":{"messageId":"a1","model":"gpt-5-mini","content":"Running tests","reasoningText":"Think first","toolRequests":[
                    {"toolCallId":"intent","name":"report_intent","arguments":{"intent":"Fixing"}},
                    {"toolCallId":"t1","name":"bash","arguments":{"command":"cargo test"}}
                ]}}),
                json!({"type":"tool.execution_complete","timestamp":"2026-10-10T09:08:03.000Z","data":{"toolCallId":"intent","success":true,"result":{"content":"Intent logged","detailedContent":"Fixing"}}}),
                json!({"type":"tool.execution_complete","timestamp":"2026-10-10T09:08:04.000Z","data":{"toolCallId":"t1","success":false,"error":{"message":"Command blocked"}}}),
                json!({"type":"assistant.message","timestamp":"2026-10-10T09:08:05.000Z","data":{"messageId":"sub","content":"inner","parentToolCallId":"t1"}}),
                json!({"type":"assistant.message","timestamp":"2026-10-10T09:08:06.000Z","data":{"messageId":"a2","content":"Done","outputTokens":5}}),
            ],
        );
        events
    }

    #[test]
    fn copilot_sessions_fall_back_to_session_start_without_workspace_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let events = fixture(dir.path());
        let meta = parse_session(&events).unwrap();
        assert_eq!(meta.session_id, "0cb916db-0000-4000-8000-000000000001");
        assert_eq!(meta.project_dir.as_deref(), Some("/work/project"));
        assert_eq!(meta.title.as_deref(), Some("Fix the build"));
        assert_eq!(
            meta.resume_command.as_deref(),
            Some("copilot --resume=0cb916db-0000-4000-8000-000000000001")
        );
        assert!(meta.last_active_at > meta.created_at);
    }

    #[test]
    fn copilot_workspace_yaml_supplies_names_and_directories() {
        let dir = tempfile::tempdir().unwrap();
        let events = fixture(dir.path());
        std::fs::write(
            events.parent().unwrap().join("workspace.yaml"),
            "id: 0cb916db-0000-4000-8000-000000000001\ncwd: /other\nname: Build fix\nsummary: Fixed CI\ncreated_at: 2026-10-10T09:07:56.720Z\nupdated_at: 2026-10-11T09:07:58.221Z\n",
        )
        .unwrap();
        let meta = parse_session(&events).unwrap();
        assert_eq!(meta.project_dir.as_deref(), Some("/other"));
        assert_eq!(meta.title.as_deref(), Some("Build fix"));
        assert_eq!(meta.summary.as_deref(), Some("Fixed CI"));
        assert_eq!(
            meta.last_active_at,
            utils::parse_timestamp_to_ms(&json!("2026-10-11T09:07:58.221Z"))
        );
    }

    #[test]
    fn copilot_transcript_keeps_main_agent_messages_tools_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        let events = fixture(dir.path());
        let messages = load_messages(&events).unwrap();
        let roles: Vec<_> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "tool", "assistant"]);
        assert_eq!(messages[0].content, "Fix the build");
        let meta = messages[1].meta.as_ref().unwrap();
        assert_eq!(meta.model.as_deref(), Some("gpt-5-mini"));
        assert_eq!(meta.input_tokens, Some(70));
        assert_eq!(meta.cache_read_tokens, Some(30));
        assert_eq!(meta.output_tokens, Some(12));
        let calls = messages[1]
            .blocks
            .iter()
            .filter(|block| matches!(block, SessionBlock::ToolCall { .. }))
            .count();
        assert_eq!(calls, 1, "report_intent is not shown as a tool call");
        assert!(matches!(
            messages[2].blocks[0],
            SessionBlock::ToolResult {
                status: ToolStatus::Error,
                ..
            }
        ));
        assert_eq!(messages[3].meta.as_ref().unwrap().output_tokens, Some(5));
    }
}
