//! WorkBuddy 会话：`<data dir>/projects/<sanitized cwd>/<sessionId>.jsonl`。
//!
//! 引擎是 CodeBuddy Code，每行一个事件，带 `id` / `parentId` / `timestamp`（毫秒）/
//! `sessionId` / `cwd`：
//! - `message`：`role` 为 user / assistant，`content` 是 `{type, text}` 数组
//!   （`input_text` / `output_text`）；用户消息把真实提问包在 `<user_query>` 里，前面是
//!   引擎注入的上下文。assistant 消息的 `providerData` 带模型和用量。
//! - `reasoning`：`rawContent` 数组里是思考文本。
//! - `function_call`：`callId` / `name` / `arguments`（JSON 字符串）。
//! - `function_call_result`：`callId` / `status` / `output{type,text}`。
//! - `ai-title`：`aiTitle`；`session-meta` / `file-history-snapshot` 不是对话内容。
//!
//! WorkBuddy 没有可在终端里恢复会话的 CLI，所以不提供恢复命令。
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::model::{MessageMeta, SessionBlock, ToolStatus};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, parse_arguments, thinking_block, tool_call_block, tool_result_block,
    ToolSource,
};
use super::utils::{
    for_each_jsonl_value, parse_timestamp_to_ms, truncate_summary, TITLE_MAX_CHARS,
};

pub(crate) fn projects_dir() -> PathBuf {
    crate::workbuddy_config::data_dir().join("projects")
}

pub(crate) fn session_roots() -> Vec<PathBuf> {
    vec![projects_dir()]
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    scan(&projects_dir())
}

/// 每个项目目录下的顶层 `*.jsonl`（子目录里的子代理记录不是独立会话）。
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
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
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

/// `content` 数组里的文本（`input_text` / `output_text` / `text`）。
fn content_text(record: &Value, field: &str) -> String {
    record
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                None | Some("input_text" | "output_text" | "text" | "reasoning_text")
            )
        })
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 用户真正输入的内容：有 `<user_query>` 就取最后一段里的内容，否则整段。
fn user_query(text: &str) -> String {
    const OPEN: &str = "<user_query>";
    const CLOSE: &str = "</user_query>";
    if let Some(start) = text.rfind(OPEN) {
        let rest = &text[start + OPEN.len()..];
        let end = rest.find(CLOSE).unwrap_or(rest.len());
        return rest[..end].trim().to_string();
    }
    text.trim().to_string()
}

fn user_prompt(record: &Value) -> Option<String> {
    if kind(record) != "message" || role(record) != "user" {
        return None;
    }
    let text = user_query(&content_text(record, "content"));
    (!text.is_empty()).then_some(text)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let mut session_id = None;
    let mut project_dir = None;
    let mut prompt = None;
    let mut title = None;
    let mut created_at = None;
    let mut last_active_at = None;
    // 会话文件不大（每轮几条），整份读一遍才能拿到最后的 ai-title 和活动时间。
    for_each_jsonl_value(path, |_, record| {
        if session_id.is_none() {
            session_id = record
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if project_dir.is_none() {
            project_dir = record
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|cwd| !cwd.is_empty())
                .map(str::to_string);
        }
        if let Some(ts) = record.get("timestamp").and_then(parse_timestamp_to_ms) {
            created_at = Some(created_at.map_or(ts, |c: i64| c.min(ts)));
            last_active_at = Some(last_active_at.map_or(ts, |l: i64| l.max(ts)));
        }
        if prompt.is_none() {
            prompt = user_prompt(&record);
        }
        if kind(&record) == "ai-title" {
            if let Some(text) = record
                .get("aiTitle")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                title = Some(text.to_string());
            }
        }
        Ok(())
    })
    .ok()?;
    // 没有真实提问的会话（刚开就关的）只是噪音。
    let prompt = prompt?;
    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
    })?;
    let title = title.unwrap_or_else(|| prompt.lines().next().unwrap_or_default().to_string());
    Some(SessionMeta {
        provider_id: "workbuddy".into(),
        session_id,
        title: Some(truncate_summary(&title, TITLE_MAX_CHARS)),
        summary: Some(truncate_summary(&prompt, 200)),
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().into_owned()),
        resume_command: None,
    })
}

/// assistant 消息的用量口径，会话阅读器和用量导入共用。
///
/// `prompt_tokens` 是 OpenAI 口径（已含缓存命中：`prompt_cache_hit_tokens +
/// prompt_cache_miss_tokens == prompt_tokens`），`completion_tokens` 已含思考 token。
/// 这里拆成「新输入 / 缓存读 / 缓存写 / 输出」，新输入不含缓存。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Usage {
    pub model: Option<String>,
    pub fresh_input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl Usage {
    pub(crate) fn is_empty(&self) -> bool {
        self.fresh_input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_write == 0
    }
}

fn count(value: Option<&Value>, key: &str) -> u64 {
    value
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// 从 assistant `message` 事件里取用量；没有任何用量字段时返回 `None`。
pub(crate) fn assistant_usage(record: &Value) -> Option<Usage> {
    if kind(record) != "message" || role(record) != "assistant" {
        return None;
    }
    let data = record.get("providerData");
    let model = data
        .and_then(|data| data.get("model").or_else(|| data.get("requestModelId")))
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let raw = data.and_then(|data| data.get("rawUsage"));
    if raw.is_some_and(Value::is_object) {
        let details = raw.and_then(|raw| raw.get("prompt_tokens_details"));
        let cache_read = [
            count(raw, "prompt_cache_hit_tokens"),
            count(raw, "cache_read_input_tokens"),
            count(raw, "cached_tokens"),
            count(details, "cached_tokens"),
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        let cache_write =
            count(raw, "cache_creation_input_tokens").max(count(raw, "prompt_cache_write_tokens"));
        let reasoning = count(raw, "completion_thinking_tokens").max(count(
            raw.and_then(|raw| raw.get("completion_tokens_details")),
            "reasoning_tokens",
        ));
        return Some(Usage {
            model,
            fresh_input: count(raw, "prompt_tokens")
                .saturating_sub(cache_read)
                .saturating_sub(cache_write),
            output: count(raw, "completion_tokens"),
            cache_read,
            cache_write,
            reasoning,
        });
    }
    // 没有 rawUsage 时退回 agents SDK 的 usage（同样是含缓存的输入）。
    let usage = data.and_then(|data| data.get("usage"))?;
    let cache_read = usage
        .get("inputTokensDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|detail| count(Some(detail), "cached_tokens"))
        .sum::<u64>();
    let reasoning = usage
        .get("outputTokensDetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|detail| count(Some(detail), "reasoning_tokens"))
        .sum::<u64>();
    Some(Usage {
        model,
        fresh_input: count(Some(usage), "inputTokens").saturating_sub(cache_read),
        output: count(Some(usage), "outputTokens"),
        cache_read,
        cache_write: 0,
        reasoning,
    })
}

fn usage_meta(record: &Value) -> Option<MessageMeta> {
    let usage = assistant_usage(record)?;
    let some = |n: u64| (n > 0).then_some(n);
    let meta = MessageMeta {
        model: usage.model,
        input_tokens: some(usage.fresh_input),
        output_tokens: some(usage.output),
        cache_read_tokens: some(usage.cache_read),
        cache_write_tokens: some(usage.cache_write),
        reasoning_tokens: some(usage.reasoning),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

fn tool_status(record: &Value) -> ToolStatus {
    if record
        .pointer("/providerData/toolResult/error")
        .is_some_and(|error| !error.is_null())
        || record
            .pointer("/providerData/toolResult/rawResponse/is_error")
            .and_then(Value::as_bool)
            == Some(true)
    {
        return ToolStatus::Error;
    }
    match record.get("status").and_then(Value::as_str) {
        Some("completed" | "success") => ToolStatus::Success,
        Some("incomplete" | "failed" | "error") => ToolStatus::Error,
        Some("cancelled" | "canceled" | "interrupted") => ToolStatus::Interrupted,
        _ => ToolStatus::Unknown,
    }
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let mut messages = Vec::new();
    for_each_jsonl_value(path, |span, record| {
        let ts = record.get("timestamp").and_then(parse_timestamp_to_ms);
        let (role, blocks) = match kind(&record) {
            "message" if role(&record) == "user" => {
                let text = user_query(&content_text(&record, "content"));
                if text.is_empty() {
                    return Ok(());
                }
                ("user", vec![SessionBlock::text(text)])
            }
            "message" if role(&record) == "assistant" => {
                let text = content_text(&record, "content");
                let blocks = if text.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![SessionBlock::text(text)]
                };
                ("assistant", blocks)
            }
            "reasoning" => {
                let text = content_text(&record, "rawContent");
                if text.trim().is_empty() {
                    return Ok(());
                }
                (
                    "assistant",
                    vec![thinking_block(&text, None, None, || {
                        Some(span.content_ref("/rawContent/0/text"))
                    })],
                )
            }
            "function_call" => {
                let Some(name) = record.get("name").and_then(Value::as_str) else {
                    return Ok(());
                };
                let call_id = record
                    .get("callId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{name}-{}", span.offset));
                let args = parse_arguments(record.get("arguments").unwrap_or(&Value::Null));
                (
                    "assistant",
                    vec![tool_call_block(
                        ToolSource::Claude,
                        call_id,
                        name,
                        &args,
                        || Some(span.content_ref("/arguments")),
                    )],
                )
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
                        _ if output.is_null() => (String::new(), None),
                        _ => (output.to_string(), None),
                    },
                    None => (String::new(), None),
                };
                (
                    "tool",
                    vec![tool_result_block(
                        call_id,
                        tool_status(&record),
                        &text,
                        || pointer.map(|pointer| span.content_ref(pointer)),
                    )],
                )
            }
            _ => return Ok(()),
        };
        let meta = (role == "assistant").then(|| usage_meta(&record)).flatten();
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

    const SESSION: &str = r#"{"type":"session-meta","id":"m0","sessionId":"wb-1","timestamp":1792764358000,"meta":{"codebuddy.ai/hostKind":"unopted"}}
{"id":"u1","timestamp":1792764357000,"type":"message","role":"user","content":[{"type":"input_text","text":"<system-reminder>injected</system-reminder>\n<user_query>list txt files</user_query>"}],"sessionId":"wb-1","cwd":"/work/proj"}
{"timestamp":1792764359000,"type":"file-history-snapshot","messageId":"u1","snapshot":{},"cwd":"/work/proj"}
{"id":"t1","timestamp":1792764360000,"type":"ai-title","aiTitle":"Find text files","sessionId":"wb-1","cwd":"/work/proj"}
{"id":"r1","parentId":"u1","timestamp":1792764361000,"type":"reasoning","rawContent":[{"type":"reasoning_text","text":"I should glob."}],"content":[],"sessionId":"wb-1","cwd":"/work/proj"}
{"id":"c1","parentId":"r1","timestamp":1792764362000,"type":"function_call","callId":"call_1","name":"Glob","arguments":"{\"pattern\":\"*.txt\"}","sessionId":"wb-1","cwd":"/work/proj"}
{"id":"c2","parentId":"c1","timestamp":1792764363000,"type":"function_call_result","callId":"call_1","name":"Glob","status":"completed","output":{"type":"text","text":"/work/proj/a.txt"},"sessionId":"wb-1","cwd":"/work/proj"}
{"id":"c3","parentId":"c2","timestamp":1792764363500,"type":"function_call","callId":"call_2","name":"Read","arguments":"{\"file_path\":\"/missing\"}","sessionId":"wb-1","cwd":"/work/proj"}
{"id":"c4","parentId":"c3","timestamp":1792764363600,"type":"function_call_result","callId":"call_2","name":"Read","status":"incomplete","output":{"type":"text","text":"Error reading file"},"providerData":{"toolResult":{"error":"Error reading file"}},"sessionId":"wb-1","cwd":"/work/proj"}
{"id":"a1","parentId":"c4","timestamp":1792764364000,"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Found a.txt."}],"providerData":{"model":"glm-test","requestModelId":"fast-model","rawUsage":{"prompt_tokens":1200,"completion_tokens":34,"prompt_cache_hit_tokens":1000,"prompt_cache_miss_tokens":200,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"prompt_cache_write_tokens":0,"completion_thinking_tokens":5,"cached_tokens":0,"prompt_tokens_details":{"cached_tokens":1000}}},"sessionId":"wb-1","cwd":"/work/proj"}
"#;

    fn write_session(root: &Path) -> PathBuf {
        let project = root.join("projects").join("work-proj");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("wb-1.jsonl");
        fs::write(&path, SESSION).unwrap();
        path
    }

    #[test]
    fn workbuddy_scan_reads_ai_title_cwd_and_times() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path());
        let project = dir.path().join("projects/work-proj");
        // 没有用户提问的会话和子目录里的文件都不算。
        fs::write(project.join("empty.jsonl"), SESSION.lines().next().unwrap()).unwrap();
        fs::create_dir_all(project.join("wb-1")).unwrap();
        fs::write(project.join("wb-1/sub.jsonl"), SESSION).unwrap();
        let sessions = scan(&dir.path().join("projects"));
        assert_eq!(sessions.len(), 1);
        let meta = &sessions[0];
        assert_eq!(meta.provider_id, "workbuddy");
        assert_eq!(meta.session_id, "wb-1");
        assert_eq!(meta.title.as_deref(), Some("Find text files"));
        assert_eq!(meta.summary.as_deref(), Some("list txt files"));
        assert_eq!(meta.project_dir.as_deref(), Some("/work/proj"));
        assert_eq!(meta.created_at, Some(1792764357000));
        assert_eq!(meta.last_active_at, Some(1792764364000));
        assert_eq!(meta.resume_command, None);
        assert_eq!(meta.source_path.as_deref(), path.to_str());
    }

    #[test]
    fn workbuddy_messages_pair_tool_calls_and_keep_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path());
        let messages = load_messages(&path).unwrap();
        let roles: Vec<_> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            [
                "user",
                "assistant",
                "assistant",
                "tool",
                "assistant",
                "tool",
                "assistant"
            ]
        );
        assert!(
            matches!(&messages[0].blocks[0], SessionBlock::Text { text, .. } if text == "list txt files")
        );
        assert!(matches!(
            messages[1].blocks[0],
            SessionBlock::Thinking { .. }
        ));
        assert!(matches!(
            &messages[2].blocks[0],
            SessionBlock::ToolCall { id, raw_name, .. } if id == "call_1" && raw_name == "Glob"
        ));
        assert!(matches!(
            &messages[3].blocks[0],
            SessionBlock::ToolResult { call_id, status: ToolStatus::Success, preview, .. }
                if call_id == "call_1" && preview == "/work/proj/a.txt"
        ));
        assert!(matches!(
            &messages[5].blocks[0],
            SessionBlock::ToolResult { call_id, status: ToolStatus::Error, .. } if call_id == "call_2"
        ));
        let last = messages.last().unwrap();
        assert_eq!(last.id.as_deref(), Some("a1"));
        let meta = last.meta.as_ref().unwrap();
        assert_eq!(meta.model.as_deref(), Some("glm-test"));
        assert_eq!(meta.input_tokens, Some(200));
        assert_eq!(meta.cache_read_tokens, Some(1000));
        assert_eq!(meta.output_tokens, Some(34));
        assert_eq!(meta.reasoning_tokens, Some(5));
    }

    #[test]
    fn workbuddy_usage_falls_back_to_sdk_usage() {
        let record: Value = serde_json::json!({
            "type": "message", "role": "assistant",
            "providerData": {"model": "m", "usage": {
                "inputTokens": 900, "outputTokens": 40,
                "inputTokensDetails": [{"cached_tokens": 100}],
                "outputTokensDetails": [{"reasoning_tokens": 7}]
            }}
        });
        let usage = assistant_usage(&record).unwrap();
        assert_eq!(
            usage,
            Usage {
                model: Some("m".into()),
                fresh_input: 800,
                output: 40,
                cache_read: 100,
                cache_write: 0,
                reasoning: 7,
            }
        );
        let user = serde_json::json!({"type": "message", "role": "user"});
        assert!(assistant_usage(&user).is_none());
    }
}
