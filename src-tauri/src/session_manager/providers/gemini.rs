use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::cache::Transcript;
use crate::session_manager::model::{
    ContentRef, DiffOp, EventKind, MessageMeta, SessionBlock, ToolKind, ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, count_diff_lines, normalize_tool, parse_antigravity_args, single_file_diff,
    thinking_block, tool_call_block, tool_result_block, ToolSource,
};
use super::utils::{
    parse_timestamp_to_ms, remove_dir_all_if_exists, remove_file_if_exists, truncate_summary,
};

const PROVIDER_ID: &str = "gemini";

pub fn session_roots() -> Vec<PathBuf> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let mut roots = vec![gemini_dir.join("tmp")];
    roots.extend(
        crate::gemini_config::ANTIGRAVITY_ROOTS
            .iter()
            .map(|root| gemini_dir.join(root)),
    );
    roots
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let mut sessions = scan_gemini_sessions();
    sessions.extend(scan_antigravity_sessions());
    sessions
}

fn scan_gemini_sessions() -> Vec<SessionMeta> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.exists() {
        return Vec::new();
    }

    let mut sessions = Vec::new();

    // Iterate over project directories: tmp/<project_name>/chats/session-*.json(l)
    let project_dirs = match std::fs::read_dir(&tmp_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in project_dirs.flatten() {
        let chats_dir = entry.path().join("chats");
        if !chats_dir.is_dir() {
            continue;
        }

        let chat_files = match std::fs::read_dir(&chats_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let project_root_file = entry.path().join(".project_root");
        let project_dir = std::fs::read_to_string(project_root_file).ok();

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            if !is_session_file(&path) {
                continue;
            }
            if let Some(meta) = parse_session(&path) {
                sessions.push(SessionMeta {
                    project_dir: project_dir.clone(),
                    ..meta
                });
            }
        }
    }

    sessions
}

/// 会话文件：旧版 `.json`，或新版 Gemini CLI 写的 `.jsonl`。旧文件被 resume 迁移后
/// 会留下同名 `.json`，此时只认 `.jsonl`，避免同一会话列两次。
pub(crate) fn is_session_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jsonl") => true,
        Some("json") => !path.with_extension("jsonl").exists(),
        _ => false,
    }
}

/// 把会话文件还原成旧版单个 JSON 对象的形状 `{sessionId, ..., messages: [...]}`。
///
/// 旧版 `.json` 整个文件就是这个对象，原样返回。新版 `.jsonl` 按 Gemini CLI 的
/// `loadConversationRecord` 回放：带 `sessionId` 的行是元数据；带 `id` 的行是消息，
/// 同 id 后写覆盖先写（位置不变）；`{"$set": {...}}` 合并元数据，带 `messages` 时整体
/// 替换消息；`{"$rewindTo": id}` 删掉该条及之后的消息，找不到 id 时清空。
pub(crate) fn parse_session_document(data: &str) -> Option<Value> {
    // 只有元数据一行的 `.jsonl` 也能整体解析成对象，补上空的 messages
    if let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(data) {
        map.entry("messages")
            .or_insert_with(|| Value::Array(Vec::new()));
        return Some(Value::Object(map));
    }

    let mut metadata = serde_json::Map::new();
    let mut log = MessageLog::default();
    let mut seen_record = false;

    for line in data.lines() {
        let Ok(Value::Object(mut record)) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        seen_record = true;
        if let Some(id) = record.get("$rewindTo").and_then(Value::as_str) {
            log.rewind_to(id);
        } else if record.get("id").is_some_and(Value::is_string) {
            log.upsert(Value::Object(record));
        } else if let Some(Value::Object(mut set)) = record.remove("$set") {
            if let Some(Value::Array(list)) = set.remove("messages") {
                log.reset(list);
            }
            metadata.extend(set);
        } else if record.get("sessionId").is_some_and(Value::is_string) {
            if let Some(Value::Array(list)) = record.remove("messages") {
                for msg in list {
                    log.upsert(msg);
                }
            }
            metadata.extend(record);
        }
    }

    if !seen_record {
        return None;
    }
    metadata.insert("messages".to_string(), Value::Array(log.messages));
    Some(Value::Object(metadata))
}

/// JSONL 回放中的消息表：同 id 原位覆盖、保持首次出现的顺序（同上游的 JS `Map`），
/// 按 id 建索引，整体回放是线性的。
#[derive(Default)]
struct MessageLog {
    messages: Vec<Value>,
    index: HashMap<String, usize>,
}

impl MessageLog {
    /// 没有字符串 `id` 的不是消息，忽略
    fn upsert(&mut self, msg: Value) {
        let Some(id) = msg.get("id").and_then(Value::as_str).map(str::to_string) else {
            return;
        };
        match self.index.get(&id) {
            Some(&i) => self.messages[i] = msg,
            None => {
                self.index.insert(id, self.messages.len());
                self.messages.push(msg);
            }
        }
    }

    /// 删掉该条及之后的消息；找不到 id 时清空
    fn rewind_to(&mut self, id: &str) {
        let len = self.index.get(id).copied().unwrap_or(0);
        self.messages.truncate(len);
        self.index.retain(|_, i| *i < len);
    }

    /// `$set.messages` 检查点：整体替换
    fn reset(&mut self, list: Vec<Value>) {
        self.messages.clear();
        self.index.clear();
        for msg in list {
            self.upsert(msg);
        }
    }
}

/// Gemini CLI 注入或非提问的用户文本（同上游 `isIgnoredUserContent`）
fn is_ignored_user_text(text: &str) -> bool {
    let t = text.trim();
    t.is_empty() || t.starts_with('/') || t.starts_with('?') || is_injected_user_text(t)
}

/// CLI 注入的上下文（环境信息、hook 输出），不是用户的提问
fn is_injected_user_text(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<session_context>") || t.starts_with("<hook_context>")
}

fn read_session_document(path: &Path) -> Result<Value, String> {
    let data = std::fs::read_to_string(path).map_err(|e| format!("Failed to read session: {e}"))?;
    parse_session_document(&data).ok_or_else(|| "Failed to parse session JSON".to_string())
}

pub(crate) fn load_transcript(path: &Path) -> Result<Transcript, String> {
    if is_antigravity_transcript(path) {
        return load_antigravity_transcript(path);
    }
    load_messages(path).map(Transcript::new)
}

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    if is_antigravity_transcript(path) {
        return load_antigravity_messages(path);
    }

    let value = read_session_document(path)?;

    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "No messages array found".to_string())?;

    // 全文引用指向会话文件本身：rel_path 相对 provider 根（`~/.gemini/tmp`），即 `<hash>/chats/<file>`
    let rel_path = relative_to_tmp_root(path);
    let file_ref = |pointer: String| -> Option<ContentRef> {
        Some(ContentRef::File {
            rel_path: rel_path.clone(),
            pointer,
        })
    };

    let mut result = Vec::new();
    for (index, msg) in messages.iter().enumerate() {
        let ts = msg.get("timestamp").and_then(parse_timestamp_to_ms);
        let base = format!("/messages/{index}");
        let mut message = match msg.get("type").and_then(Value::as_str) {
            Some("user") => {
                let text = content_text(msg.get("content"));
                if text.trim().is_empty() {
                    continue;
                }
                let injected = is_injected_user_text(&text);
                let mut message =
                    SessionMessage::from_blocks("user", ts, vec![SessionBlock::text(text)]);
                message.injected = injected;
                message
            }
            Some("gemini") => {
                let blocks = gemini_blocks(msg, &base, &file_ref);
                let mut message = SessionMessage::from_blocks("assistant", ts, blocks);
                message.meta = gemini_meta(msg);
                message
            }
            Some(kind @ ("info" | "error")) => {
                let text = content_text(msg.get("content"));
                if text.trim().is_empty() {
                    continue;
                }
                let event_kind = if kind == "info" {
                    EventKind::Info
                } else {
                    EventKind::Error
                };
                let mut message = SessionMessage::from_blocks(
                    "system",
                    ts,
                    vec![SessionBlock::event(event_kind, Some(text), None)],
                );
                // info（登录、刷新等提示）默认折叠
                message.injected = kind == "info";
                message
            }
            Some(_) | None => continue,
        };
        if message.is_empty() {
            continue;
        }
        message.id = msg.get("id").and_then(Value::as_str).map(str::to_string);
        result.push(message);
    }

    assign_turn_ids(&mut result);
    Ok(result)
}

/// 多条 `thoughts[{subject, description}]` 合并后的思考：`summary` 为各 subject 以 ` · ` 连接，
/// `text` 为 `**subject**\n\ndescription` 以空行连接。
pub(crate) struct MergedThoughts {
    pub summary: String,
    pub text: String,
}

/// 合并 Gemini 的 `thoughts` 数组；不是对象数组、或没有任何非空 subject/description 时返回 `None`。
/// 解析器生成预览与按引用取全文共用这一份格式。
pub(crate) fn format_thoughts(value: &Value) -> Option<MergedThoughts> {
    let items = value.as_array()?;
    let mut thoughts = Vec::new();
    for item in items {
        let object = item.as_object()?;
        let field = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or("").trim();
        let (subject, description) = (field("subject"), field("description"));
        if !subject.is_empty() || !description.is_empty() {
            thoughts.push((subject, description));
        }
    }
    if thoughts.is_empty() {
        return None;
    }
    let summary = thoughts
        .iter()
        .map(|(subject, _)| *subject)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let text = thoughts
        .iter()
        .map(
            |(subject, description)| match (subject.is_empty(), description.is_empty()) {
                (false, false) => format!("**{subject}**\n\n{description}"),
                (false, true) => format!("**{subject}**"),
                _ => description.to_string(),
            },
        )
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(MergedThoughts { summary, text })
}

/// 会话文件相对 `tmp/` 的路径：取末尾三段 `<hash>/chats/<file>`（不足三段时取已有部分）。
fn relative_to_tmp_root(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .rev()
        .take(3)
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

/// Gemini content 可能是字符串，也可能是 `[{text}]` 数组。
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `type=gemini` 消息：思考（多条合并为一块）→ 正文 → 工具调用与结果（按生成顺序）。
fn gemini_blocks(
    msg: &Value,
    base: &str,
    file_ref: &dyn Fn(String) -> Option<ContentRef>,
) -> Vec<SessionBlock> {
    let mut blocks = Vec::new();

    if let Some(thoughts) = msg.get("thoughts").and_then(format_thoughts) {
        // 合并后的正文与取回的全文同一口径：引用指向整个数组，由 `content::resolve_content_ref`
        // 按 `format_thoughts` 格式化
        blocks.push(thinking_block(
            &thoughts.text,
            (!thoughts.summary.is_empty()).then_some(thoughts.summary),
            None,
            || file_ref(format!("{base}/thoughts")),
        ));
    }

    let text = content_text(msg.get("content"));
    if !text.trim().is_empty() {
        blocks.push(SessionBlock::text(text));
    }

    for (j, call) in msg
        .get("toolCalls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(name) = call.get("name").and_then(Value::as_str) else {
            continue;
        };
        let call_base = format!("{base}/toolCalls/{j}");
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{name}-{j}"));
        let args = call.get("args").cloned().unwrap_or(Value::Null);
        let mut call_block = tool_call_block(ToolSource::Gemini, id.clone(), name, &args, || {
            file_ref(format!("{call_base}/args"))
        });

        let (output, pointer) = tool_output(call, &call_base);
        // resultDisplay 是 FileDiff 时用真实 diff 覆盖参数估算
        if let Some(diff_text) = call
            .pointer("/resultDisplay/fileDiff")
            .and_then(Value::as_str)
        {
            if let SessionBlock::ToolCall { diff, title, .. } = &mut call_block {
                let (added, removed) = count_diff_lines(diff_text);
                let path = call
                    .pointer("/resultDisplay/filePath")
                    .or_else(|| call.pointer("/resultDisplay/fileName"))
                    .and_then(Value::as_str)
                    .unwrap_or(title.as_str())
                    .to_string();
                let op = diff
                    .as_ref()
                    .and_then(|d| d.files.first())
                    .map_or(DiffOp::Update, |f| f.op);
                let mut summary = single_file_diff(&path, op, added, removed);
                summary.full = file_ref(format!("{call_base}/resultDisplay/fileDiff"));
                *diff = Some(summary);
            }
        }
        blocks.push(call_block);

        let status = match call.get("status").and_then(Value::as_str) {
            Some("success" | "completed") => ToolStatus::Success,
            Some("error") => ToolStatus::Error,
            Some("cancelled" | "canceled") => ToolStatus::Interrupted,
            Some("executing" | "scheduled" | "validating" | "awaiting_approval") => {
                ToolStatus::Pending
            }
            _ => ToolStatus::Unknown,
        };
        blocks.push(tool_result_block(id, status, &output, || {
            pointer.and_then(file_ref)
        }));
    }

    blocks
}

/// 工具输出文本与其 JSON Pointer（能精确指到字符串时才给）。
/// 优先 `resultDisplay`（字符串或 FileDiff），否则 `result[].functionResponse.response.output|error`。
fn tool_output(call: &Value, call_base: &str) -> (String, Option<String>) {
    match call.get("resultDisplay") {
        Some(Value::String(text)) if !text.is_empty() => {
            return (text.clone(), Some(format!("{call_base}/resultDisplay")));
        }
        Some(display @ Value::Object(_)) => {
            if let Some(diff) = display.get("fileDiff").and_then(Value::as_str) {
                return (
                    diff.to_string(),
                    Some(format!("{call_base}/resultDisplay/fileDiff")),
                );
            }
            return (display.to_string(), None);
        }
        _ => {}
    }
    for (k, item) in call
        .get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        for key in ["output", "error"] {
            if let Some(text) = item
                .pointer(&format!("/functionResponse/response/{key}"))
                .and_then(Value::as_str)
            {
                return (
                    text.to_string(),
                    Some(format!(
                        "{call_base}/result/{k}/functionResponse/response/{key}"
                    )),
                );
            }
        }
    }
    match call.get("result") {
        Some(Value::String(text)) => (text.clone(), Some(format!("{call_base}/result"))),
        Some(Value::Null) | None => (String::new(), None),
        Some(other) => (other.to_string(), None),
    }
}

/// `tokens{input, output, cached, thoughts, tool, total}` + `model` → meta（0 视为缺省）。
fn gemini_meta(msg: &Value) -> Option<MessageMeta> {
    let tokens = msg.get("tokens");
    let count = |key: &str| {
        tokens
            .and_then(|t| t.get(key))
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
    };
    let meta = MessageMeta {
        model: msg
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        input_tokens: count("input"),
        output_tokens: count("output"),
        cache_read_tokens: count("cached"),
        reasoning_tokens: count("thoughts"),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

pub fn delete_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if is_antigravity_transcript(path) {
        return delete_antigravity_session(root, path, session_id);
    }

    let meta = parse_session(path).ok_or_else(|| {
        format!(
            "Failed to parse Gemini session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Gemini session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    // resume 迁移后残留的旧版 `.json` 先删：失败就整体失败，否则 `.jsonl` 没了它会重新出现在列表里
    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
        let legacy = path.with_extension("json");
        if legacy.is_file() {
            std::fs::remove_file(&legacy).map_err(|e| {
                format!(
                    "Failed to delete legacy Gemini session file {}: {e}",
                    legacy.display()
                )
            })?;
        }
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Gemini session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    let value = read_session_document(path).ok()?;

    let session_id = value.get("sessionId").and_then(Value::as_str)?.to_string();

    let created_at = value.get("startTime").and_then(parse_timestamp_to_ms);
    let last_active_at = value.get("lastUpdated").and_then(parse_timestamp_to_ms);

    // 标题取第一条真正的提问，跳过 CLI 注入的上下文和斜杠命令
    let title = value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|msgs| {
            msgs.iter()
                .filter(|m| m.get("type").and_then(Value::as_str) == Some("user"))
                .map(|m| content_text(m.get("content")))
                .find(|s| !is_ignored_user_text(s))
                .map(|s| truncate_summary(&s, 160))
        });

    let source_path = path.to_string_lossy().to_string();

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title: title.clone(),
        summary: title,
        project_dir: None, // (optionally) populated later
        created_at,
        last_active_at: last_active_at.or(created_at),
        source_path: Some(source_path),
        resume_command: Some(format!("gemini --resume {session_id}")),
    })
}

fn scan_antigravity_sessions() -> Vec<SessionMeta> {
    let mut by_id: HashMap<String, SessionMeta> = HashMap::new();
    let mut project_cache: HashMap<String, Option<String>> = HashMap::new();

    for root in antigravity_roots() {
        let brain_dir = root.join("brain");
        let entries = match std::fs::read_dir(&brain_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let summaries = load_root_conversation_summaries(&root);

        for entry in entries.flatten() {
            let session_path = entry.path();
            let transcript = session_path
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            if !transcript.is_file() {
                continue;
            }
            let session_id = match entry.file_name().to_str() {
                Some(id) if is_safe_id_component(id) => id.to_string(),
                _ => continue,
            };

            let project_dir = resolve_antigravity_workspace_dir(
                &root,
                &session_id,
                summaries.get(&session_id),
                &mut project_cache,
            );

            let Some(meta) =
                parse_antigravity_session_with_project_dir(&transcript, &session_id, project_dir)
            else {
                continue;
            };

            let incoming_ts = meta.last_active_at.or(meta.created_at).unwrap_or(0);
            match by_id.get(&meta.session_id) {
                Some(existing)
                    if existing.last_active_at.or(existing.created_at).unwrap_or(0)
                        >= incoming_ts => {}
                _ => {
                    by_id.insert(meta.session_id.clone(), meta);
                }
            }
        }
    }

    by_id.into_values().collect()
}

fn antigravity_roots() -> Vec<PathBuf> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    crate::gemini_config::ANTIGRAVITY_ROOTS
        .iter()
        .map(|root| gemini_dir.join(root))
        .collect()
}

pub(crate) fn is_antigravity_transcript(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("transcript.jsonl")
        && path
            .components()
            .any(|component| component.as_os_str() == ".system_generated")
}

fn is_safe_id_component(id: &str) -> bool {
    if id.is_empty() || id == "." || id == ".." {
        return false;
    }
    if id.contains('/') || id.contains('\\') || id.contains('\0') {
        return false;
    }
    let mut comps = Path::new(id).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

fn url_decode_simple(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    result.push(b);
                    i += 3;
                    continue;
                }
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

fn normalize_workspace_path(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if raw.starts_with("file://") {
        let url = url::Url::parse(raw).ok()?;
        if url.scheme() != "file" {
            return None;
        }
        let raw_path = if let Ok(file_path) = url.to_file_path() {
            file_path.to_string_lossy().to_string()
        } else {
            portable_file_workspace_path(&url)?
        };

        // If the path looks like "/C:/...", strip the leading slash for Windows drive compatibility
        let bytes = raw_path.as_bytes();
        let path = if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            raw_path[1..].to_string()
        } else {
            raw_path
        };

        return Some(normalize_workspace_separators(&path));
    }

    if raw.contains("://") {
        return None;
    }

    let is_abs = Path::new(raw).is_absolute()
        || raw.starts_with('/')
        || raw.starts_with(r"\\")
        || (raw.len() >= 3
            && raw.as_bytes()[0].is_ascii_alphabetic()
            && raw.as_bytes()[1] == b':'
            && (raw.as_bytes()[2] == b'/' || raw.as_bytes()[2] == b'\\'));

    if is_abs {
        Some(normalize_workspace_separators(raw))
    } else {
        None
    }
}

// Mirrors file URI semantics when the host OS cannot represent this path.
fn portable_file_workspace_path(url: &url::Url) -> Option<String> {
    let path = url_decode_simple(url.path());
    if let Some(host) = url.host_str().filter(|host| !host.is_empty()) {
        Some(format!(r"\\{host}{path}"))
    } else if path.len() >= 3
        && path.as_bytes()[0] == b'/'
        && path.as_bytes()[1].is_ascii_alphabetic()
        && path.as_bytes()[2] == b':'
    {
        Some(path[1..].to_string())
    } else if path.starts_with('/') {
        Some(path)
    } else {
        None
    }
}

// Normalize lexically: metadata may refer to projects that no longer exist.
fn normalize_workspace_separators(path: &str) -> String {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let unc = path.starts_with(r"\\") || path.starts_with("//");
    let normalized = if drive {
        path.replace('\\', "/")
    } else if unc {
        path.replace('/', r"\")
    } else {
        path.to_string()
    };
    let trimmed = if drive || unc {
        normalized.trim_end_matches(['/', '\\'])
    } else {
        normalized.trim_end_matches('/')
    };
    if trimmed.is_empty() {
        "/".to_string()
    } else if drive && trimmed.len() == 2 {
        format!("{trimmed}/")
    } else {
        trimmed.to_string()
    }
}

fn extract_path_from_workspace_uris_json(json_str: &str) -> Option<String> {
    let uris: Vec<String> = serde_json::from_str(json_str).ok()?;
    for uri in uris {
        if let Some(path) = normalize_workspace_path(&uri) {
            return Some(path);
        }
    }
    None
}

#[derive(Debug, Clone, Default)]
struct AntigravitySummary {
    workspace_uris: Option<String>,
    project_id: Option<String>,
}

fn load_root_conversation_summaries(root: &Path) -> HashMap<String, AntigravitySummary> {
    let db_path = root.join("conversation_summaries.db");
    if !db_path.is_file() {
        return HashMap::new();
    }
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));

    let mut stmt = match conn
        .prepare("SELECT conversation_id, workspace_uris, project_id FROM conversation_summaries")
    {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    let rows = match stmt.query_map([], |row| {
        let conv_id: Option<String> = row.get(0)?;
        let uris: Option<String> = row.get(1)?;
        let project_id: Option<String> = row.get(2)?;
        Ok((conv_id, uris, project_id))
    }) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };

    let mut summaries = HashMap::new();
    for row in rows.flatten() {
        if let (Some(id), uris, project_id) = row {
            summaries.insert(
                id,
                AntigravitySummary {
                    workspace_uris: uris.filter(|s| !s.trim().is_empty()),
                    project_id: project_id.filter(|s| !s.trim().is_empty()),
                },
            );
        }
    }
    summaries
}

#[allow(dead_code)]
fn load_single_conversation_summary(root: &Path, session_id: &str) -> Option<AntigravitySummary> {
    let db_path = root.join("conversation_summaries.db");
    if !db_path.is_file() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));
    let mut stmt = conn
        .prepare(
            "SELECT workspace_uris, project_id FROM conversation_summaries WHERE conversation_id = ?1 LIMIT 1",
        )
        .ok()?;
    stmt.query_row([session_id], |row| {
        let uris: Option<String> = row.get(0)?;
        let project_id: Option<String> = row.get(1)?;
        Ok(AntigravitySummary {
            workspace_uris: uris.filter(|s| !s.trim().is_empty()),
            project_id: project_id.filter(|s| !s.trim().is_empty()),
        })
    })
    .ok()
}

fn resolve_project_dir_from_config(
    project_id: &str,
    project_cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if !is_safe_id_component(project_id)
        || project_id == "outside-of-project"
        || project_id == "default-cli-project"
    {
        return None;
    }

    if let Some(cached) = project_cache.get(project_id) {
        return cached.clone();
    }

    let resolved = read_project_config_dir(project_id);
    project_cache.insert(project_id.to_string(), resolved.clone());
    resolved
}

fn read_project_config_dir(project_id: &str) -> Option<String> {
    let gemini_dir = crate::gemini_config::get_gemini_dir();
    let config_path = gemini_dir
        .join("config")
        .join("projects")
        .join(format!("{project_id}.json"));
    if !config_path.is_file() {
        return None;
    }

    let content = std::fs::read_to_string(&config_path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let resources = value
        .get("projectResources")?
        .get("resources")?
        .as_array()?;

    for item in resources {
        if let Some(uri) = item.get("folderUri").and_then(Value::as_str) {
            if let Some(path) = normalize_workspace_path(uri) {
                return Some(path);
            }
        }
        if let Some(uri) = item
            .get("gitFolder")
            .and_then(|g| g.get("folderUri"))
            .and_then(Value::as_str)
        {
            if let Some(path) = normalize_workspace_path(uri) {
                return Some(path);
            }
        }
    }
    None
}

fn read_acp_meta_cwd(root: &Path, session_id: &str) -> Option<String> {
    if !is_safe_id_component(session_id) {
        return None;
    }
    let meta_path = root
        .join("conversations")
        .join(format!("{session_id}.meta"));
    if !meta_path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&meta_path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let cwd = value.get("cwd").and_then(Value::as_str)?;
    normalize_workspace_path(cwd)
}

fn read_proto_varint(buf: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for index in 0..10 {
        let byte = *buf.get(*offset)?;
        *offset += 1;
        // A u64 has only one remaining bit in its tenth protobuf byte.
        if index == 9 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn read_proto_tag(buf: &[u8], offset: &mut usize) -> Option<(u32, u32)> {
    let key = read_proto_varint(buf, offset)?;
    let field_num = u32::try_from(key >> 3).ok()?;
    let wire_type = (key & 0x7) as u32;
    Some((field_num, wire_type))
}

fn read_proto_length_delimited<'a>(buf: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let len = read_proto_varint(buf, offset)?;
    let len = usize::try_from(len).ok()?;
    let end = offset.checked_add(len)?;
    if end > buf.len() {
        return None;
    }
    let slice = &buf[*offset..end];
    *offset = end;
    Some(slice)
}

fn parse_workspace_from_trajectory_metadata_blob(data: &[u8]) -> Option<String> {
    let mut offset = 0;
    let mut candidate_f7 = None;

    while offset < data.len() {
        let (field_num, wire_type) = read_proto_tag(data, &mut offset)?;
        match wire_type {
            0 => {
                read_proto_varint(data, &mut offset)?;
            }
            1 => {
                offset = offset.checked_add(8)?;
                if offset > data.len() {
                    return None;
                }
            }
            5 => {
                offset = offset.checked_add(4)?;
                if offset > data.len() {
                    return None;
                }
            }
            2 => {
                let bytes = read_proto_length_delimited(data, &mut offset)?;
                if field_num == 1 {
                    let mut sub_offset = 0;
                    while sub_offset < bytes.len() {
                        let (sub_fn, sub_wt) = match read_proto_tag(bytes, &mut sub_offset) {
                            Some(tag) => tag,
                            None => break,
                        };
                        match sub_wt {
                            0 => {
                                if read_proto_varint(bytes, &mut sub_offset).is_none() {
                                    break;
                                }
                            }
                            1 => {
                                sub_offset = match sub_offset.checked_add(8) {
                                    Some(o) if o <= bytes.len() => o,
                                    _ => break,
                                };
                            }
                            5 => {
                                sub_offset = match sub_offset.checked_add(4) {
                                    Some(o) if o <= bytes.len() => o,
                                    _ => break,
                                };
                            }
                            2 => {
                                let sub_bytes =
                                    match read_proto_length_delimited(bytes, &mut sub_offset) {
                                        Some(b) => b,
                                        None => break,
                                    };
                                if sub_fn == 1 {
                                    if let Ok(s) = std::str::from_utf8(sub_bytes) {
                                        if let Some(path) = normalize_workspace_path(s) {
                                            return Some(path);
                                        }
                                    }
                                }
                            }
                            _ => break,
                        }
                    }
                } else if field_num == 7 {
                    if let Ok(s) = std::str::from_utf8(bytes) {
                        if let Some(path) = normalize_workspace_path(s) {
                            candidate_f7 = Some(path);
                        }
                    }
                }
            }
            _ => return None,
        }
    }

    candidate_f7
}

fn read_trajectory_metadata_workspace(root: &Path, session_id: &str) -> Option<String> {
    if !is_safe_id_component(session_id) {
        return None;
    }
    let db_path = root.join("conversations").join(format!("{session_id}.db"));
    if !db_path.is_file() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let _ = conn.busy_timeout(std::time::Duration::from_millis(500));

    let mut stmt = conn
        .prepare("SELECT data FROM trajectory_metadata_blob WHERE id = 'main' LIMIT 1")
        .ok()?;
    let data: Vec<u8> = stmt.query_row([], |row| row.get(0)).ok()?;
    parse_workspace_from_trajectory_metadata_blob(&data)
}

fn resolve_antigravity_workspace_dir(
    root: &Path,
    session_id: &str,
    summary: Option<&AntigravitySummary>,
    project_cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    // 1. conversation_summaries.db workspace_uris
    if let Some(summary) = summary {
        if let Some(uris_str) = &summary.workspace_uris {
            if let Some(path) = extract_path_from_workspace_uris_json(uris_str) {
                return Some(path);
            }
        }
        // 2. project_id -> config/projects/<id>.json
        if let Some(project_id) = &summary.project_id {
            if let Some(path) = resolve_project_dir_from_config(project_id, project_cache) {
                return Some(path);
            }
        }
    }

    // 3. ACP conversations/<session_id>.meta -> cwd
    if let Some(path) = read_acp_meta_cwd(root, session_id) {
        return Some(path);
    }

    // 4. conversations/<session_id>.db -> trajectory_metadata_blob
    if let Some(path) = read_trajectory_metadata_workspace(root, session_id) {
        return Some(path);
    }

    // 5. None
    None
}

fn find_antigravity_root_and_id_for_transcript(
    transcript_path: &Path,
) -> Option<(PathBuf, String)> {
    let mut current = transcript_path.parent();
    while let Some(dir) = current {
        if let Some(parent) = dir.parent() {
            if parent.file_name().and_then(|n| n.to_str()) == Some("brain") {
                let session_id = dir.file_name()?.to_str()?.to_string();
                let root = parent.parent()?.to_path_buf();
                return Some((root, session_id));
            }
        }
        current = dir.parent();
    }
    None
}

fn antigravity_session_id_from_transcript(path: &Path) -> Option<String> {
    find_antigravity_root_and_id_for_transcript(path)
        .map(|(_, id)| id)
        .or_else(|| {
            path.parent()?
                .parent()?
                .parent()?
                .file_name()?
                .to_str()
                .map(|value| value.to_string())
        })
}

fn parse_antigravity_timestamp(value: &Value) -> Option<i64> {
    value
        .get("ts")
        .and_then(parse_timestamp_to_ms)
        .or_else(|| value.get("created_at").and_then(parse_timestamp_to_ms))
}

fn antigravity_resume_command(session_id: &str) -> Option<String> {
    // Resume commands are executed by a shell or copied into a terminal. Keep
    // identifiers safe for both POSIX shells and Windows terminals, while still
    // allowing sessions with unusual directory names to be read and deleted.
    if !is_safe_id_component(session_id)
        || session_id.starts_with('-')
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    Some(format!("agy --conversation {session_id}"))
}

#[allow(dead_code)]
pub(crate) fn parse_antigravity_session(path: &Path) -> Option<SessionMeta> {
    let (root, session_id) = find_antigravity_root_and_id_for_transcript(path).or_else(|| {
        let id = antigravity_session_id_from_transcript(path)?;
        let root = path.parent()?.parent()?.parent()?.parent()?.to_path_buf();
        Some((root, id))
    })?;
    let mut project_cache = HashMap::new();
    let summary = load_single_conversation_summary(&root, &session_id);
    let project_dir =
        resolve_antigravity_workspace_dir(&root, &session_id, summary.as_ref(), &mut project_cache);
    parse_antigravity_session_with_project_dir(path, &session_id, project_dir)
}

fn parse_antigravity_session_with_project_dir(
    path: &Path,
    session_id: &str,
    project_dir: Option<String>,
) -> Option<SessionMeta> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    let mut created_at = None;
    let mut last_active_at = None;
    let mut title = None;

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if let Some(ts) = parse_antigravity_timestamp(&value) {
            created_at.get_or_insert(ts);
            last_active_at = Some(ts);
        }
        if title.is_none()
            && value.get("source").and_then(Value::as_str) == Some("USER_EXPLICIT")
            && value.get("type").and_then(Value::as_str) == Some("USER_INPUT")
        {
            let content = clean_antigravity_content(extract_antigravity_content(&value));
            if !content.trim().is_empty() {
                title = Some(truncate_summary(&content, 160));
            }
        }
    }
    let last_active_at = last_active_at.or(created_at);

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.to_string(),
        title: title.clone(),
        summary: title,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: antigravity_resume_command(session_id),
    })
}

/// Reader and cache must resolve the same DB, within this transcript's root.
/// Session IDs can occur in several Antigravity roots (for example after copying
/// a session); another root's database must not supply this transcript's usage.
pub(crate) fn find_antigravity_db_path(transcript_path: &Path) -> Option<PathBuf> {
    if !is_antigravity_transcript(transcript_path) {
        return None;
    }
    let (root, session_id) = find_antigravity_root_and_id_for_transcript(transcript_path)?;
    if !is_safe_id_component(&session_id) {
        return None;
    }
    let db = root.join("conversations").join(format!("{session_id}.db"));
    db.is_file().then_some(db)
}

fn load_antigravity_step_token_map(
    db_path: &Path,
) -> Result<HashMap<i64, crate::services::session_usage_gemini::AntigravityTokenData>, String> {
    use crate::services::session_usage_gemini::{parse_gen_metadata_blob, parse_step_gen_idx};

    // Like the usage importer, join generation usage and step references from
    // one read snapshot. The caller must leave degraded results uncached so it
    // can retry optional metadata without making transcript text unavailable.
    let read = || -> rusqlite::Result<_> {
        let mut conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(500))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
        let mut gen_meta_map = HashMap::new();
        {
            let mut stmt = tx.prepare("SELECT idx, data FROM gen_metadata")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
            for row in rows {
                let (idx, data) = row?;
                if let Some(token_data) = parse_gen_metadata_blob(&data) {
                    gen_meta_map.insert(idx, token_data);
                }
            }
        }

        let mut step_token_map = HashMap::new();
        {
            let mut stmt = tx.prepare("SELECT idx, metadata FROM steps")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
            })?;
            for row in rows {
                let (step_idx, metadata) = row?;
                if let Some(gen_idx) = metadata.as_deref().and_then(parse_step_gen_idx) {
                    if let Some(token_data) = gen_meta_map.get(&gen_idx) {
                        step_token_map.insert(step_idx, token_data.clone());
                    }
                }
            }
        }
        tx.commit()?;
        Ok(step_token_map)
    };
    read().map_err(|e| {
        format!(
            "Failed to read Antigravity metadata {}: {e}",
            db_path.display()
        )
    })
}

fn load_antigravity_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    load_antigravity_transcript(path).map(|transcript| transcript.messages)
}

fn load_antigravity_transcript(path: &Path) -> Result<Transcript, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read Antigravity transcript: {e}"))?;

    let (step_token_map, skip_cache) = match find_antigravity_db_path(path)
        .map(|db| load_antigravity_step_token_map(&db))
        .transpose()
    {
        Ok(map) => (map.unwrap_or_default(), false),
        Err(error) => {
            log::warn!("{error}; loading Antigravity transcript without database metadata");
            (HashMap::new(), true)
        }
    };
    // Keep physical byte ranges so previews can retrieve the original JSONL text.
    let mut offset = 0u64;
    let steps: Vec<_> = data
        .split_inclusive('\n')
        .filter_map(|line| {
            let start = offset;
            offset += line.len() as u64;
            let value = serde_json::from_str::<Value>(line).ok()?;
            Some((value, start, u32::try_from(line.len()).ok()))
        })
        .collect();
    let mut planner_counts = HashMap::new();
    for (step, ..) in &steps {
        if is_antigravity_planner(step) {
            if let Some(index) = step.get("step_index").and_then(Value::as_i64) {
                *planner_counts.entry(index).or_insert(0usize) += 1;
            }
        }
    }
    // Rewound/replayed logs can reuse step indices. Only those planners need a
    // physical-row suffix; ordinary logs retain their existing deterministic IDs.
    let step_ids: Vec<_> = steps
        .iter()
        .enumerate()
        .map(
            |(row, (step, ..))| match step.get("step_index").and_then(Value::as_i64) {
                Some(index) if planner_counts.get(&index).copied().unwrap_or(0) > 1 => {
                    format!("{index}:row-{row}")
                }
                Some(index) => index.to_string(),
                None => format!("row-{row}"),
            },
        )
        .collect();
    // Resolve forwards within one planner's execution segment. A new request,
    // planner, or index rewind ends the segment, so later records cannot replace
    // an earlier result or finish a call from another turn.
    let mut pending_calls = HashMap::new();
    let mut result_calls = HashMap::new();
    let mut previous_index = None;
    for (row, (step, ..)) in steps.iter().enumerate() {
        let index = step.get("step_index").and_then(Value::as_i64);
        let planner = is_antigravity_planner(step);
        let user_input = step.get("source").and_then(Value::as_str) == Some("USER_EXPLICIT")
            && step.get("type").and_then(Value::as_str) == Some("USER_INPUT");
        let rewound = index
            .zip(previous_index)
            .is_some_and(|(current, previous)| current <= previous);
        if planner || user_input || rewound {
            pending_calls.clear();
        }
        if let Some(index) = index {
            previous_index = Some(index);
            if planner {
                for (tool_index, call) in antigravity_calls(step).enumerate() {
                    let Some(name) = call
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    else {
                        continue;
                    };
                    let Some(result_index) = index
                        .checked_add(1)
                        .and_then(|n| n.checked_add(i64::try_from(tool_index).ok()?))
                    else {
                        continue;
                    };
                    pending_calls.insert(
                        result_index,
                        (format!("agy:{}:{tool_index}", step_ids[row]), name),
                    );
                }
            } else if let Some((call_id, name)) = pending_calls.remove(&index) {
                if is_antigravity_tool_result(step, name) {
                    result_calls.insert(row, (call_id, name));
                }
            }
        }
    }
    let mut result = Vec::new();
    for (row, (value, offset, len)) in steps.iter().enumerate() {
        let ts = parse_antigravity_timestamp(value);
        let step_index = value.get("step_index").and_then(Value::as_i64);
        let full = |pointer: String| {
            len.map(|len| ContentRef::Jsonl {
                offset: *offset,
                len,
                pointer,
            })
        };
        let (role, blocks) = if let Some((call_id, name)) = result_calls.get(&row) {
            (
                "tool",
                vec![antigravity_tool_result(value, call_id, name, &full)],
            )
        } else if value.get("source").and_then(Value::as_str) == Some("USER_EXPLICIT")
            && value.get("type").and_then(Value::as_str) == Some("USER_INPUT")
        {
            let content = clean_antigravity_content(extract_antigravity_content(value));
            if content.is_empty() {
                continue;
            }
            ("user", vec![SessionBlock::text(content)])
        } else if is_antigravity_planner(value) {
            (
                "assistant",
                antigravity_model_blocks(value, &step_ids[row], &full),
            )
        } else {
            continue;
        };
        if blocks.is_empty() {
            continue;
        }
        let mut message = SessionMessage::from_blocks(role, ts, blocks);
        message.id = step_index.map(|n| n.to_string());

        let mut input_tokens = value.get("input_tokens").and_then(Value::as_u64);
        let mut output_tokens = value.get("output_tokens").and_then(Value::as_u64);
        let mut cache_read = value.get("cache_read_tokens").and_then(Value::as_u64);
        let mut duration_ms = value
            .get("duration_ms")
            .and_then(Value::as_u64)
            .filter(|d| *d > 0 && *d <= 3_600_000);
        let mut model = value
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if role == "assistant" {
            if let Some(step_idx) = step_index {
                if let Some(token_data) = step_token_map.get(&step_idx) {
                    if input_tokens.is_none() && token_data.input_tokens > 0 {
                        input_tokens = Some(token_data.input_tokens as u64);
                    }
                    if output_tokens.is_none() && token_data.output_tokens > 0 {
                        output_tokens = Some(token_data.output_tokens as u64);
                    }
                    if cache_read.is_none() && token_data.cached_tokens > 0 {
                        cache_read = Some(token_data.cached_tokens as u64);
                    }
                    if duration_ms.is_none() {
                        if let Some(d) = token_data.duration_ms {
                            if d > 0 && d <= 3_600_000 {
                                duration_ms = Some(d as u64);
                            }
                        }
                    }
                    if model.is_none()
                        && !token_data.model.is_empty()
                        && token_data.model != "unknown"
                    {
                        model = Some(token_data.model.clone());
                    }
                }
            }
        }

        if input_tokens.is_some()
            || output_tokens.is_some()
            || cache_read.is_some()
            || duration_ms.is_some()
            || model.is_some()
        {
            message.meta = Some(MessageMeta {
                model,
                input_tokens,
                output_tokens,
                cache_read_tokens: cache_read,
                duration_ms,
                ..Default::default()
            });
        }

        result.push(message);
    }

    assign_turn_ids(&mut result);
    let mut transcript = Transcript::new(result);
    transcript.skip_cache = skip_cache;
    Ok(transcript)
}

fn is_antigravity_planner(step: &Value) -> bool {
    step.get("source").and_then(Value::as_str) == Some("MODEL")
        && step.get("type").and_then(Value::as_str) == Some("PLANNER_RESPONSE")
}

fn antigravity_calls(step: &Value) -> impl Iterator<Item = &Value> {
    step.get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn is_antigravity_tool_result(step: &Value, name: &str) -> bool {
    let source = step.get("source").and_then(Value::as_str);
    let kind = step.get("type").and_then(Value::as_str);
    if source == Some("SYSTEM") && kind == Some("ERROR_MESSAGE") {
        return true;
    }
    if source != Some("MODEL") {
        return false;
    }
    let expected = match name {
        "replace_file_content" | "multi_replace_file_content" | "write_to_file" => {
            "CODE_ACTION".to_string()
        }
        "list_dir" => "LIST_DIRECTORY".to_string(),
        _ => name.to_ascii_uppercase(),
    };
    kind == Some("GENERIC") || kind == Some(expected.as_str())
}

// Derived content keeps the existing reader contract and physical JSONL source validation.
pub(crate) const ANTIGRAVITY_READER_REF_PREFIX: &str = "/_cc_switch/antigravity/tool_calls/";

fn antigravity_reader_args(raw: &Value) -> Value {
    fn rename_fields(value: &mut Value) {
        let Some(map) = value.as_object_mut() else {
            return;
        };
        for (alias, key) in [
            ("CommandLine", "command"),
            ("AbsolutePath", "path"),
            ("DirectoryPath", "path"),
            ("TargetFile", "path"),
            ("SearchPath", "path"),
            ("SearchDirectory", "path"),
            ("TargetContent", "old_string"),
            ("ReplacementContent", "new_string"),
            ("ReplacementChunks", "edits"),
            ("CodeContent", "content"),
            ("Url", "url"),
            ("Pattern", "pattern"),
            ("StartLine", "start_line"),
            ("EndLine", "end_line"),
        ] {
            if !map.contains_key(key) {
                if let Some(value) = map.remove(alias) {
                    map.insert(key.into(), value);
                }
            }
        }
        if let Some(edits) = map.get_mut("edits").and_then(Value::as_array_mut) {
            for edit in edits {
                rename_fields(edit);
            }
        }
    }
    // Decode exactly once: textual values such as "true" must not become booleans on replay.
    let mut args = parse_antigravity_args(raw);
    rename_fields(&mut args);
    args
}

fn antigravity_diff_pairs(args: &Value, kind: ToolKind) -> Option<Vec<(&str, &str)>> {
    match kind {
        ToolKind::Write => Some(vec![("", args.get("content")?.as_str()?)]),
        ToolKind::Edit => {
            fn pair(item: &Value) -> Option<(&str, &str)> {
                Some((
                    item.get("old_string")?.as_str()?,
                    item.get("new_string")?.as_str()?,
                ))
            }
            match args.get("edits") {
                Some(edits) => {
                    let edits = edits.as_array()?;
                    if edits.is_empty() {
                        return None;
                    }
                    // An incomplete chunk list cannot represent a complete diff.
                    edits.iter().map(pair).collect()
                }
                None => Some(vec![pair(args)?]),
            }
        }
        _ => None,
    }
}

pub(crate) fn resolve_antigravity_reader_content(
    step: &Value,
    pointer: &str,
) -> Result<String, String> {
    let stale = || "会话已更新，请重新读取".to_string();
    if step.get("source").and_then(Value::as_str) != Some("MODEL")
        || step.get("type").and_then(Value::as_str) != Some("PLANNER_RESPONSE")
    {
        return Err(stale());
    }
    let (index, field) = pointer
        .strip_prefix(ANTIGRAVITY_READER_REF_PREFIX)
        .and_then(|suffix| suffix.split_once('/'))
        .ok_or_else(stale)?;
    let index = index.parse::<usize>().map_err(|_| stale())?;
    let call = antigravity_calls(step).nth(index).ok_or_else(stale)?;
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(stale)?;
    let args = antigravity_reader_args(call.get("args").unwrap_or(&Value::Null));
    match field {
        "input" => serde_json::to_string_pretty(&args).map_err(|_| stale()),
        "diff" => {
            let kind = normalize_tool(ToolSource::Gemini, name, None).kind;
            let pairs = antigravity_diff_pairs(&args, kind).ok_or_else(stale)?;
            let mut diff = String::new();
            for (old, new) in pairs {
                diff.push_str("@@\n");
                for (prefix, text) in [('-', old), ('+', new)] {
                    let normalized = text.replace("\r\n", "\n");
                    let text = normalized.strip_suffix('\n').unwrap_or(&normalized);
                    if !normalized.is_empty() {
                        // Keep content resembling paired ---/+++ file headers visible in the reader.
                        if prefix == '+'
                            && text.starts_with("++ ")
                            && diff
                                .lines()
                                .last()
                                .is_some_and(|line| line.starts_with("--- "))
                        {
                            diff.push_str("@@\n");
                        }
                        for line in text.split('\n') {
                            diff.push(prefix);
                            diff.push_str(line);
                            diff.push('\n');
                        }
                    }
                }
            }
            Ok(diff)
        }
        _ => Err(stale()),
    }
}

fn antigravity_model_blocks(
    step: &Value,
    step_id: &str,
    full: &dyn Fn(String) -> Option<ContentRef>,
) -> Vec<SessionBlock> {
    let mut blocks = Vec::new();
    if let Some(thinking) = step.get("thinking").and_then(Value::as_str) {
        if !thinking.trim().is_empty() {
            blocks.push(thinking_block(thinking.trim(), None, None, || {
                full("/thinking".into())
            }));
        }
    }
    let content = clean_antigravity_content(extract_antigravity_content(step));
    if !content.is_empty() {
        blocks.push(SessionBlock::text(content));
    }
    for (tool_index, call) in antigravity_calls(step).enumerate() {
        let Some(name) = call
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let args = antigravity_reader_args(call.get("args").unwrap_or(&Value::Null));
        let mut block = tool_call_block(
            ToolSource::Gemini,
            format!("agy:{step_id}:{tool_index}"),
            name,
            &args,
            || full(format!("{ANTIGRAVITY_READER_REF_PREFIX}{tool_index}/input")),
        );
        if let SessionBlock::ToolCall {
            input_total_len, ..
        } = &mut block
        {
            // The existing reader compares JavaScript string.length (UTF-16 code units).
            let input = match &args {
                Value::Null => String::new(),
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            *input_total_len = input.encode_utf16().count().min(u32::MAX as usize) as u32;
        }
        if let SessionBlock::ToolCall {
            kind,
            diff: Some(diff),
            ..
        } = &mut block
        {
            if antigravity_diff_pairs(&args, *kind).is_some() {
                diff.full = full(format!("{ANTIGRAVITY_READER_REF_PREFIX}{tool_index}/diff"));
            }
        }
        blocks.push(block);
    }
    blocks
}

fn antigravity_tool_result(
    step: &Value,
    call_id: &str,
    tool_name: &str,
    full: &dyn Fn(String) -> Option<ContentRef>,
) -> SessionBlock {
    // Tool output is literal text: user-request/metadata markers are not wrappers here.
    let content = extract_antigravity_content(step);
    let (output, pointer) = if content.trim().is_empty() {
        (
            step.get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            "/error",
        )
    } else {
        let pointer = if step.get("content").is_some_and(Value::is_object) {
            "/content/text"
        } else {
            "/content"
        };
        (content, pointer)
    };
    let exit_code = if matches!(tool_name, "run_command" | "run_shell_command") {
        antigravity_command_exit_code(step, &output)
    } else {
        None
    };
    let is_system_error = step.get("type").and_then(Value::as_str) == Some("ERROR_MESSAGE");
    let status = if is_system_error {
        ToolStatus::Error
    } else {
        match step.get("status").and_then(Value::as_str) {
            Some("DONE") if exit_code.is_some_and(|code| code != 0) => ToolStatus::Error,
            Some("DONE") => ToolStatus::Success,
            Some("ERROR") => ToolStatus::Error,
            Some("RUNNING") => ToolStatus::Pending,
            Some("CANCELLED" | "CANCELED" | "INTERRUPTED") => ToolStatus::Interrupted,
            _ => ToolStatus::Unknown,
        }
    };
    let mut block = tool_result_block(call_id, status, &output, || full(pointer.into()));
    if let SessionBlock::ToolResult {
        exit_code: code, ..
    } = &mut block
    {
        *code = exit_code;
    }
    block
}

/// DONE describes the tool step, not the process's exit status. Only inspect
/// shell metadata before Output:, never arbitrary stdout or file contents.
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
        if line == "Output:" || line == "Command Output:" {
            break;
        }
        if let Some(code) = line.strip_prefix("The command failed with exit code:") {
            return code.trim().parse().ok();
        }
        if line == "The command completed successfully." {
            return Some(0);
        }
    }
    None
}

fn extract_antigravity_content(value: &Value) -> String {
    match value.get("content") {
        Some(Value::String(text)) => text.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(map)) => map
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn clean_antigravity_content(content: String) -> String {
    let mut cleaned = content;
    if let Some(after) = cleaned.split("<USER_REQUEST>").nth(1) {
        cleaned = after.to_string();
        if let Some((before, _)) = cleaned.split_once("</USER_REQUEST>") {
            cleaned = before.to_string();
        }
    }
    if let Some((before, _)) = cleaned.split_once("<ADDITIONAL_METADATA>") {
        cleaned = before.to_string();
    }
    cleaned.trim().to_string()
}

fn delete_antigravity_session(root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    if !is_safe_id_component(session_id) {
        return Err("Invalid Antigravity session ID".to_string());
    }
    let parsed_id = antigravity_session_id_from_transcript(path).ok_or_else(|| {
        format!(
            "Failed to parse Antigravity session ID from {}",
            path.display()
        )
    })?;
    if parsed_id != session_id {
        return Err(format!(
            "Antigravity session ID mismatch: expected {session_id}, found {parsed_id}"
        ));
    }

    let root = root
        .canonicalize()
        .map_err(|e| format!("Failed to resolve Antigravity session root: {e}"))?;
    let brain_dir = root.join("brain").join(session_id);
    let expected_source = brain_dir.join(".system_generated/logs/transcript.jsonl");
    let source = path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve Antigravity transcript: {e}"))?;
    let expected_source = expected_source
        .canonicalize()
        .map_err(|e| format!("Failed to resolve expected Antigravity transcript: {e}"))?;
    if source != expected_source || !source.starts_with(&root) {
        return Err("Antigravity transcript does not belong to the selected session root".into());
    }

    let conversation_base = root.join("conversations");
    let files: Vec<_> = ["db", "db-shm", "db-wal", "db-journal", "pb", "meta"]
        .iter()
        .map(|suffix| conversation_base.join(format!("{session_id}.{suffix}")))
        .collect();

    // Check every target before deleting anything, including absent sidecars:
    // their existing parent directories may be symlinks outside this root.
    for target in files.iter().chain(std::iter::once(&brain_dir)) {
        validate_antigravity_delete_target(&root, target)?;
    }

    for file in files {
        remove_file_if_exists(&file).map_err(|e| {
            format!(
                "Failed to delete Antigravity conversation file {}: {e}",
                file.display()
            )
        })?;
    }

    remove_dir_all_if_exists(&brain_dir).map_err(|e| {
        format!(
            "Failed to delete Antigravity brain directory {}: {e}",
            brain_dir.display()
        )
    })?;

    Ok(true)
}

fn validate_antigravity_delete_target(root: &Path, target: &Path) -> Result<(), String> {
    let mut existing = target;
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => {
                let resolved = existing.canonicalize().map_err(|e| {
                    format!(
                        "Failed to resolve Antigravity deletion target {}: {e}",
                        target.display()
                    )
                })?;
                if !resolved.starts_with(root) {
                    return Err(format!(
                        "Antigravity deletion target is outside the session root: {}",
                        target.display()
                    ));
                }
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                existing = existing.parent().ok_or_else(|| {
                    format!(
                        "Cannot resolve Antigravity deletion target: {}",
                        target.display()
                    )
                })?;
            }
            Err(e) => {
                return Err(format!(
                    "Failed to inspect Antigravity deletion target {}: {e}",
                    target.display()
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_manager::model::ToolKind;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_json_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-03-06T10-17-test.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "gemini-session-123",
              "startTime": "2026-03-06T10:17:58.000Z",
              "lastUpdated": "2026-03-06T10:20:00.000Z",
              "messages": [
                {
                  "id": "msg-1",
                  "timestamp": "2026-03-06T10:17:58.000Z",
                  "type": "user",
                  "content": "hello"
                }
              ]
            }"#,
        )
        .expect("write session");

        delete_session(temp.path(), &path, "gemini-session-123").expect("delete session");

        assert!(!path.exists());
    }

    fn write_jsonl(path: &Path, lines: &[Value]) {
        let data: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(path, data).expect("write jsonl");
    }

    /// #7861：新版 Gemini CLI 写 `.jsonl`，按记录回放成消息列表
    #[test]
    fn jsonl_session_replays_updates_set_and_rewind() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-10-04T10-00-abcd1234.jsonl");
        let user = |id: &str, text: &str| {
            serde_json::json!({"id": id, "timestamp": "2026-10-04T10:00:00Z", "type": "user",
                               "content": [{"text": text}]})
        };
        let gemini = |id: &str, text: &str| {
            serde_json::json!({"id": id, "timestamp": "2026-10-04T10:00:01Z", "type": "gemini",
                               "content": text})
        };
        write_jsonl(
            &path,
            &[
                serde_json::json!({"sessionId": "sess-1", "projectHash": "h",
                                   "startTime": "2026-10-04T10:00:00Z",
                                   "lastUpdated": "2026-10-04T10:00:00Z"}),
                user("u1", "first question"),
                gemini("g1", "draft"),
                // 同 id 再写一次：原位替换
                gemini("g1", "answer one"),
                user("u2", "dropped"),
                gemini("g2", "dropped too"),
                serde_json::json!({"$rewindTo": "u2"}),
                user("u3", "second question"),
                serde_json::json!({"$set": {"lastUpdated": "2026-10-04T11:00:00Z"}}),
            ],
        );

        let texts: Vec<_> = load_messages(&path)
            .expect("load")
            .into_iter()
            .map(|m| (m.role, m.content))
            .collect();
        assert_eq!(
            texts,
            [
                ("user".to_string(), "first question".to_string()),
                ("assistant".to_string(), "answer one".to_string()),
                ("user".to_string(), "second question".to_string()),
            ]
        );

        let meta = parse_session(&path).expect("meta");
        assert_eq!(meta.session_id, "sess-1");
        assert_eq!(meta.title.as_deref(), Some("first question"));
        assert_eq!(
            meta.last_active_at,
            parse_timestamp_to_ms(&Value::from("2026-10-04T11:00:00Z"))
        );

        // `$set.messages` 是检查点：整体替换消息
        let doc = parse_session_document(
            &[
                serde_json::json!({"sessionId": "s", "projectHash": "h"}),
                user("a", "old"),
                serde_json::json!({"$set": {"messages": [user("b", "new")]}}),
            ]
            .iter()
            .map(|l| format!("{l}\n"))
            .collect::<String>(),
        )
        .expect("doc");
        assert_eq!(doc["messages"].as_array().map(Vec::len), Some(1));
        assert_eq!(doc["messages"][0]["id"], "b");
    }

    /// 新会话的首个 `$set.messages` 检查点以 CLI 注入的 `<session_context>` 开头：
    /// 标题跳过它和斜杠命令，详情里标成注入内容
    #[test]
    fn jsonl_session_context_is_skipped_for_title_and_marked_injected() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-2026-10-04T10-00-abcd1234.jsonl");
        let user = |id: &str, text: &str| serde_json::json!({"id": id, "type": "user", "content": [{"text": text}]});
        write_jsonl(
            &path,
            &[
                serde_json::json!({"sessionId": "sess-1", "projectHash": "h"}),
                serde_json::json!({"$set": {"messages": [
                    user("ctx", "<session_context>\nThis is the Gemini CLI. We are setting up the context"),
                ]}}),
                user("cmd", "/model"),
                user("q", "fix the build"),
            ],
        );

        let meta = parse_session(&path).expect("meta");
        assert_eq!(meta.title.as_deref(), Some("fix the build"));

        let msgs = load_messages(&path).expect("load");
        let injected: Vec<_> = msgs.iter().map(|m| m.injected).collect();
        assert_eq!(injected, [true, false, false]);
    }

    #[test]
    fn jsonl_with_only_metadata_line_loads_empty() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-x.jsonl");
        write_jsonl(
            &path,
            &[serde_json::json!({"sessionId": "x", "projectHash": "h"})],
        );

        assert!(load_messages(&path).expect("load").is_empty());
        assert_eq!(parse_session(&path).expect("meta").session_id, "x");
    }

    #[test]
    fn migrated_legacy_json_is_hidden_and_deleted_with_jsonl() {
        let temp = tempdir().expect("tempdir");
        let legacy = temp.path().join("session-x.json");
        let migrated = temp.path().join("session-x.jsonl");
        let only_legacy = temp.path().join("session-y.json");
        std::fs::write(&legacy, r#"{"sessionId":"x","messages":[]}"#).expect("write");
        std::fs::write(&only_legacy, r#"{"sessionId":"y","messages":[]}"#).expect("write");
        write_jsonl(
            &migrated,
            &[serde_json::json!({"sessionId": "x", "projectHash": "h"})],
        );

        assert!(!is_session_file(&legacy));
        assert!(is_session_file(&migrated));
        assert!(is_session_file(&only_legacy));

        delete_session(temp.path(), &migrated, "x").expect("delete");
        assert!(!migrated.exists());
        assert!(!legacy.exists());
        assert!(only_legacy.exists());
    }

    #[test]
    fn load_messages_handles_array_content() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-06T10:00:00Z","type":"user","content":[{"text":"hello"}]},
                {"id":"2","timestamp":"2026-03-06T10:00:01Z","type":"gemini","content":"world"},
                {"id":"3","timestamp":"2026-03-06T10:00:02Z","type":"info","content":"system info"},
                {"id":"4","timestamp":"2026-03-06T10:00:03Z","type":"error","content":"MCP ERROR"}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        // info / error 现在作为 system 事件保留（info 标记为注入内容）
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "world");
        assert_eq!((msgs[2].role.as_str(), msgs[2].injected), ("system", true));
        assert!(matches!(
            &msgs[2].blocks[0],
            SessionBlock::Event { kind: EventKind::Info, text: Some(t), .. } if t == "system info"
        ));
        assert!(!msgs[3].injected);
        assert!(matches!(
            &msgs[3].blocks[0],
            SessionBlock::Event { kind: EventKind::Error, text: Some(t), .. } if t == "MCP ERROR"
        ));
    }

    #[test]
    fn load_messages_includes_tool_calls() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{
              "sessionId": "test",
              "messages": [
                {"id":"1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"","toolCalls":[{"id":"call_1","name":"web_search","args":{"query":"test"}}]},
                {"id":"2","timestamp":"2026-03-10T08:25:00Z","type":"gemini","content":"Here are the results.","toolCalls":[{"id":"call_2","name":"web_fetch","args":{"url":"http://example.com"}}]}
              ]
            }"#,
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: web_search]"));
        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("Here are the results."));
        assert!(msgs[1].content.contains("[Tool: web_fetch]"));
    }

    #[test]
    fn load_messages_maps_thoughts_tool_calls_and_meta() {
        let temp = tempdir().expect("tempdir");
        let chats = temp.path().join("tmp").join("hash1").join("chats");
        std::fs::create_dir_all(&chats).expect("chats dir");
        let path = chats.join("session-x.json");
        let long_output: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let session = serde_json::json!({
            "sessionId": "test",
            "messages": [
                {"id":"i0","timestamp":"2026-03-10T08:24:40Z","type":"info","content":"Authenticated"},
                {"id":"u1","timestamp":"2026-03-10T08:24:45Z","type":"user","content":"build it"},
                {"id":"g1","timestamp":"2026-03-10T08:24:50Z","type":"gemini","content":"Done.",
                 "model":"gemini-2.5-pro",
                 "tokens":{"input":10342,"output":418,"cached":0,"thoughts":236,"tool":0,"total":10996},
                 "thoughts":[
                    {"subject":"Locating the entry point","description":"Read src/main.ts first."},
                    {"subject":"Planning the build check","description":"Then run npm run build."}],
                 "toolCalls":[
                    {"id":"run-1","name":"run_shell_command","args":{"command":"npm run build","description":"Build the project"},
                     "status":"error","resultDisplay":long_output},
                    {"id":"rep-1","name":"replace","status":"success",
                     "args":{"file_path":"/p/config.ts","old_string":"a","new_string":"b"},
                     "resultDisplay":{"fileName":"config.ts","fileDiff":"--- a\n+++ b\n-a\n+b\n+c\n"}},
                    {"id":"cancel-1","name":"run_shell_command","args":{"command":"sleep 9"},"status":"cancelled",
                     "result":[{"functionResponse":{"id":"cancel-1","name":"run_shell_command",
                       "response":{"error":"Command was cancelled by the user."}}}]}
                 ]}
            ]
        });
        std::fs::write(&path, session.to_string()).expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 3);
        let turns: Vec<_> = msgs.iter().map(|m| m.turn_id.as_deref().unwrap()).collect();
        assert_eq!(turns, ["t0", "t1", "t1"]);
        assert_eq!(msgs[1].id.as_deref(), Some("u1"));

        let a = &msgs[2];
        match &a.blocks[0] {
            SessionBlock::Thinking {
                text,
                summary,
                full,
                ..
            } => {
                assert_eq!(
                    summary.as_deref(),
                    Some("Locating the entry point · Planning the build check")
                );
                assert!(text.starts_with("**Locating the entry point**\n\nRead src/main.ts first."));
                assert!(full.is_none());
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&a.blocks[1], SessionBlock::Text { text, .. } if text == "Done."));
        match &a.blocks[2] {
            SessionBlock::ToolCall {
                kind,
                title,
                detail,
                ..
            } => {
                assert_eq!(*kind, ToolKind::Shell);
                assert_eq!(title, "npm run build");
                assert_eq!(detail.as_deref(), Some("Build the project"));
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[3] {
            SessionBlock::ToolResult {
                status,
                truncated,
                full,
                ..
            } => {
                assert_eq!(*status, ToolStatus::Error);
                assert!(*truncated);
                assert_eq!(
                    full,
                    &Some(ContentRef::File {
                        rel_path: "hash1/chats/session-x.json".into(),
                        pointer: "/messages/2/toolCalls/0/resultDisplay".into(),
                    })
                );
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[4] {
            SessionBlock::ToolCall { kind, diff, .. } => {
                assert_eq!(*kind, ToolKind::Edit);
                let diff = diff.as_ref().expect("diff");
                assert_eq!((diff.added, diff.removed), (2, 1));
                assert_eq!(diff.files[0].path, "config.ts");
            }
            other => panic!("{other:?}"),
        }
        match &a.blocks[7] {
            SessionBlock::ToolResult {
                status, preview, ..
            } => {
                assert_eq!(*status, ToolStatus::Interrupted);
                assert_eq!(preview, "Command was cancelled by the user.");
            }
            other => panic!("{other:?}"),
        }
        let meta = a.meta.as_ref().expect("meta");
        assert_eq!(meta.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(meta.input_tokens, Some(10342));
        assert_eq!(meta.cache_read_tokens, None);
        assert_eq!(meta.reasoning_tokens, Some(236));
        // 思考不进入 content
        assert!(a
            .content
            .starts_with("Done.\n\n[Tool: run_shell_command] npm run build"));
    }

    #[test]
    fn parse_antigravity_timestamp_prefers_ts_over_created_at() {
        let value = serde_json::json!({
            "ts": 1771061953123i64,
            "created_at": "2026-07-10T13:22:44Z"
        });
        assert_eq!(parse_antigravity_timestamp(&value), Some(1_771_061_953_123));
    }

    #[test]
    fn parse_antigravity_timestamp_normalizes_seconds_and_iso_to_millis() {
        let val_sec = serde_json::json!({ "ts": 1771061953i64 });
        assert_eq!(
            parse_antigravity_timestamp(&val_sec),
            Some(1_771_061_953_000)
        );

        let val_iso = serde_json::json!({ "created_at": "2026-07-10T13:22:44Z" });
        assert_eq!(
            parse_antigravity_timestamp(&val_iso),
            Some(1_783_689_764_000)
        );
    }

    #[test]
    fn delete_antigravity_session_removes_auxiliary_files_and_brain() {
        let temp = tempdir().expect("tempdir");
        let session_id = "agy-session-123";
        let transcript = temp
            .path()
            .join("brain")
            .join(session_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        std::fs::create_dir_all(transcript.parent().expect("transcript parent"))
            .expect("create brain");
        std::fs::write(&transcript, "{}\n").expect("write transcript");

        let conv_dir = temp.path().join("conversations");
        std::fs::create_dir_all(&conv_dir).expect("create conv_dir");
        let db_file = conv_dir.join(format!("{session_id}.db"));
        std::fs::write(&db_file, "db").expect("write db");
        let meta_file = conv_dir.join(format!("{session_id}.meta"));
        std::fs::write(&meta_file, "meta").expect("write meta");

        let deleted = delete_antigravity_session(temp.path(), &transcript, session_id)
            .expect("delete antigravity session");
        assert!(deleted);
        assert!(!db_file.exists());
        assert!(!meta_file.exists());
        assert!(!temp.path().join("brain").join(session_id).exists());
    }

    fn test_proto_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }

    fn test_proto_varint_field(field: u32, value: u64) -> Vec<u8> {
        let mut out = test_proto_varint((field as u64) << 3);
        out.extend(test_proto_varint(value));
        out
    }

    fn test_proto_len_field(field: u32, payload: Vec<u8>) -> Vec<u8> {
        let mut out = test_proto_varint(((field as u64) << 3) | 2);
        out.extend(test_proto_varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    fn test_step_metadata(gen_idx: i64) -> Vec<u8> {
        // Proto3 omits the scalar field for the first generation (index zero).
        let gen_ref = if gen_idx == 0 {
            Vec::new()
        } else {
            test_proto_varint_field(3, gen_idx as u64)
        };
        test_proto_len_field(20, gen_ref)
    }

    fn test_gen_metadata_blob(
        input: u64,
        output: u64,
        cache_read: u64,
        duration_ms: u64,
        model: &str,
    ) -> Vec<u8> {
        let mut usage = test_proto_varint_field(1, 1016);
        usage.extend(test_proto_varint_field(2, input));
        usage.extend(test_proto_varint_field(3, output));
        usage.extend(test_proto_varint_field(5, cache_read));

        let seconds = duration_ms / 1000;
        let nanos = (duration_ms % 1000) * 1_000_000;
        let mut dur = test_proto_varint_field(1, seconds);
        dur.extend(test_proto_varint_field(2, nanos));

        let mut metadata = test_proto_len_field(4, usage);
        metadata.extend(test_proto_len_field(11, dur));
        metadata.extend(test_proto_len_field(19, model.as_bytes().to_vec()));
        test_proto_len_field(1, metadata)
    }

    fn antigravity_message_fixture() -> (tempfile::TempDir, PathBuf, rusqlite::Connection) {
        let temp = tempdir().expect("tempdir");
        let session_id = "agy-session-meta";
        let transcript = temp
            .path()
            .join("brain")
            .join(session_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        std::fs::create_dir_all(transcript.parent().expect("transcript parent"))
            .expect("create brain");

        let conv_dir = temp.path().join("conversations");
        std::fs::create_dir_all(&conv_dir).expect("create conv_dir");
        let db_file = conv_dir.join(format!("{session_id}.db"));

        let conn = rusqlite::Connection::open(&db_file).expect("open db");
        conn.execute_batch(
            "CREATE TABLE steps (idx INTEGER PRIMARY KEY, metadata BLOB);
             CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB);",
        )
        .expect("create tables");
        conn.execute(
            "INSERT INTO steps (idx, metadata) VALUES (1, ?1)",
            [test_step_metadata(0)],
        )
        .expect("insert step");
        conn.execute(
            "INSERT INTO gen_metadata (idx, data) VALUES (0, ?1)",
            [test_gen_metadata_blob(
                1500,
                42,
                100,
                2500,
                "gemini-3-flash-a",
            )],
        )
        .expect("insert gen_metadata");

        // 真实 transcript 记录不包含 input_tokens / output_tokens / cache_read_tokens / duration_ms
        let lines = [
            serde_json::json!({
                "step_index": 0,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "content": "<USER_REQUEST>hello</USER_REQUEST>",
                "created_at": "2026-10-10T10:00:00Z"
            })
            .to_string(),
            serde_json::json!({
                "step_index": 1,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "content": "Hi there!",
                "created_at": "2026-10-10T10:00:00Z"
            })
            .to_string(),
        ];
        std::fs::write(&transcript, lines.join("\n")).expect("write transcript");

        (temp, transcript, conn)
    }

    #[test]
    fn antigravity_structured_calls_preserve_order_results_and_usage() {
        use crate::session_manager::model::ToolKind;
        let (_temp, path, _conn) = antigravity_message_fixture();
        let rows = [
            serde_json::json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT",
                "content":"<USER_REQUEST>检查文件</USER_REQUEST><ADDITIONAL_METADATA>hidden"}),
            serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "thinking":"先检查状态", "content":"正在检查", "tool_calls":[
                    {"name":"run_command", "args":{"CommandLine":"\"git status\"",
                        "toolSummary":"\"Check git status\"", "toolAction":"\"Running command\""}},
                    {"name":"view_file", "args":{"AbsolutePath":"\"/tmp/package.json\"",
                        "StartLine":"1", "EndLine":"95"}}]}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"RUN_COMMAND",
                "status":"DONE", "created_at":"2026-10-10T10:00:01Z",
                "content":"<USER_REQUEST>literal tool output</USER_REQUEST>"}),
            serde_json::json!({"step_index":3,"source":"MODEL","type":"GENERIC",
                "status":"ERROR","content":"permission denied"}),
            serde_json::json!({"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE",
                "content":"检查完成"}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let messages = load_antigravity_messages(&path).unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].content, "检查文件");
        assert!(messages.iter().all(|m| m.turn_id.as_deref() == Some("t1")));
        assert_eq!(messages[1].id.as_deref(), Some("1"));
        assert_eq!(messages[1].meta.as_ref().unwrap().input_tokens, Some(1500));
        let blocks = &messages[1].blocks;
        assert_eq!(blocks.len(), 4);
        assert!(matches!(&blocks[0], SessionBlock::Thinking { text, .. } if text == "先检查状态"));
        assert!(matches!(&blocks[1], SessionBlock::Text { text, .. } if text == "正在检查"));
        assert!(
            matches!(&blocks[2], SessionBlock::ToolCall { id, kind: ToolKind::Shell, title, detail, .. }
            if id == "agy:1:0" && title == "Check git status" && detail.as_deref() == Some("Running command"))
        );
        assert!(
            matches!(&blocks[3], SessionBlock::ToolCall { id, kind: ToolKind::Read, title, detail, .. }
            if id == "agy:1:1" && title == "/tmp/package.json" && detail.as_deref() == Some(":1-95"))
        );
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].ts, Some(1_791_626_401_000));
        assert!(
            matches!(&messages[2].blocks[0], SessionBlock::ToolResult { call_id, status: ToolStatus::Success, preview, .. }
            if call_id == "agy:1:0" && preview == "<USER_REQUEST>literal tool output</USER_REQUEST>")
        );
        assert!(
            matches!(&messages[3].blocks[0], SessionBlock::ToolResult { call_id, status: ToolStatus::Error, .. }
            if call_id == "agy:1:1")
        );
    }

    #[test]
    fn antigravity_tool_only_and_thinking_only_steps_survive_without_false_pairing() {
        let (_temp, path, _conn) = antigravity_message_fixture();
        let rows = [
            serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{"CommandLine":"pwd"}}]}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE",
                "thinking":"工具尚未完成"}),
            serde_json::json!({"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"停止"}),
            serde_json::json!({"step_index":4,"source":"MODEL","type":"GENERIC","status":"DONE","content":"unpaired"}),
            serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"view_file","args":{}}]}),
            serde_json::json!({"step_index":i64::MAX,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{}}]}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let messages = load_antigravity_messages(&path).unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].meta.as_ref().unwrap().output_tokens, Some(42));
        assert!(
            matches!(&messages[0].blocks[0], SessionBlock::ToolCall { id, .. } if id == "agy:1:0")
        );
        assert!(matches!(
            &messages[1].blocks[0],
            SessionBlock::Thinking { .. }
        ));
        assert!(messages
            .iter()
            .flat_map(|m| &m.blocks)
            .all(|b| !matches!(b, SessionBlock::ToolResult { .. })));
        assert_eq!(messages[2].turn_id.as_deref(), Some("t1"));
        assert!(
            matches!(&messages[3].blocks[0], SessionBlock::ToolCall { id, .. } if id == "agy:row-4:0")
        );
    }

    #[test]
    fn antigravity_reused_indices_keep_each_execution_and_unique_call_ids() {
        for new_turn in [true, false] {
            let (_temp, path, _conn) = antigravity_message_fixture();
            let planner = serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{"CommandLine":"pwd"}}]});
            let boundary = if new_turn {
                serde_json::json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"再试一次"})
            } else {
                serde_json::json!({"step_index":0,"source":"SYSTEM","type":"CHECKPOINT"})
            };
            let rows = [
                planner.clone(),
                serde_json::json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"original output"}),
                boundary,
                planner,
                serde_json::json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"ERROR","content":"replayed output"}),
            ];
            std::fs::write(
                &path,
                rows.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            let messages = load_antigravity_messages(&path).unwrap();
            let calls: Vec<_> = messages
                .iter()
                .flat_map(|m| &m.blocks)
                .filter_map(|b| {
                    if let SessionBlock::ToolCall { id, .. } = b {
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            let results: Vec<_> = messages
                .iter()
                .filter_map(|m| {
                    if let SessionBlock::ToolResult {
                        call_id,
                        preview,
                        status,
                        ..
                    } = &m.blocks[0]
                    {
                        Some((call_id, preview.as_str(), *status, m.turn_id.as_deref()))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(calls.len(), 2);
            assert_ne!(calls[0], calls[1]);
            assert_eq!(results.len(), 2);
            assert_eq!(
                results[0],
                (calls[0], "original output", ToolStatus::Success, Some("t0"))
            );
            assert_eq!(
                results[1],
                (
                    calls[1],
                    "replayed output",
                    ToolStatus::Error,
                    Some(if new_turn { "t1" } else { "t0" })
                )
            );
        }
    }

    #[test]
    fn antigravity_pending_calls_do_not_cross_execution_boundaries() {
        let boundaries = [
            serde_json::json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"new request"}),
            serde_json::json!({"step_index":0,"source":"SYSTEM","type":"CHECKPOINT"}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","content":"another planner"}),
        ];
        for boundary in boundaries {
            let (_temp, path, _conn) = antigravity_message_fixture();
            let rows = [
                serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                    "tool_calls":[{"name":"run_command","args":{"CommandLine":"pwd"}}]}),
                boundary,
                serde_json::json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"unrelated result"}),
            ];
            std::fs::write(
                &path,
                rows.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            let messages = load_antigravity_messages(&path).unwrap();
            assert!(matches!(
                &messages[0].blocks[0],
                SessionBlock::ToolCall { .. }
            ));
            assert!(messages
                .iter()
                .flat_map(|m| &m.blocks)
                .all(|b| !matches!(b, SessionBlock::ToolResult { .. })));
        }
    }

    #[test]
    fn antigravity_done_shell_failure_and_file_output_keep_distinct_statuses() {
        let (_temp, path, _conn) = antigravity_message_fixture();
        let failure = "Created At: 2026-10-11T00:00:00Z\nCompleted At: 2026-10-11T00:00:01Z\n    The command failed with exit code: 1\n    Output:\nerror details";
        let rows = [
            serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
                "tool_calls":[{"name":"run_command","args":{"CommandLine":"false"}},
                    {"name":"view_file","args":{"AbsolutePath":"/tmp/log"}}]}),
            serde_json::json!({"step_index":2,"source":"MODEL","type":"RUN_COMMAND","status":"DONE","content":failure}),
            serde_json::json!({"step_index":3,"source":"MODEL","type":"VIEW_FILE","status":"DONE","content":failure}),
        ];
        std::fs::write(
            &path,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let messages = load_antigravity_messages(&path).unwrap();
        assert!(matches!(&messages[1].blocks[0], SessionBlock::ToolResult {
            status: ToolStatus::Error, exit_code: Some(1), call_id, .. } if call_id == "agy:1:0"));
        assert!(matches!(&messages[2].blocks[0], SessionBlock::ToolResult {
            status: ToolStatus::Success, exit_code: None, call_id, .. } if call_id == "agy:1:1"));
    }

    #[test]
    fn antigravity_shell_exit_codes_respect_metadata_and_lifecycle() {
        let failure = "Created At: now\nThe command failed with exit code: -1\nOutput:\ndetails";
        let cases = [
            ("DONE", "Created At: now\nThe command completed successfully.\nOutput:\nThe command failed with exit code: 2", ToolStatus::Success, Some(0)),
            ("DONE", "Created At: now\nOutput:\nThe command failed with exit code: 2", ToolStatus::Success, None),
            ("DONE", "The command failed with exit code: 2", ToolStatus::Success, None),
            ("DONE", failure, ToolStatus::Error, Some(-1)),
            ("RUNNING", failure, ToolStatus::Pending, Some(-1)),
            ("CANCELLED", failure, ToolStatus::Interrupted, Some(-1)),
        ];
        for (state, content, expected, code) in cases {
            let step = serde_json::json!({"type":"GENERIC","status":state,"content":content});
            assert!(
                matches!(antigravity_tool_result(&step, "c1", "run_command", &|_| None),
                SessionBlock::ToolResult { status, exit_code, .. } if status == expected && exit_code == code)
            );
        }
        let explicit = serde_json::json!({"type":"GENERIC","status":"DONE","exit_code":124,
            "content":"Created At: now\nThe command completed successfully.\nOutput:\n"});
        assert!(matches!(
            antigravity_tool_result(&explicit, "c2", "run_command", &|_| None),
            SessionBlock::ToolResult {
                status: ToolStatus::Error,
                exit_code: Some(124),
                ..
            }
        ));
        let error = serde_json::json!({"type":"GENERIC","status":"ERROR","exit_code":0});
        assert!(matches!(
            antigravity_tool_result(&error, "c3", "run_shell_command", &|_| None),
            SessionBlock::ToolResult {
                status: ToolStatus::Error,
                exit_code: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn antigravity_result_statuses_and_type_guards() {
        for (status, expected) in [
            ("DONE", ToolStatus::Success),
            ("ERROR", ToolStatus::Error),
            ("RUNNING", ToolStatus::Pending),
            ("CANCELLED", ToolStatus::Interrupted),
            ("INVALID", ToolStatus::Unknown),
        ] {
            let step = serde_json::json!({"source":"MODEL","type":"GENERIC","status":status,"content":"output"});
            assert!(is_antigravity_tool_result(&step, "run_command"));
            assert!(
                matches!(antigravity_tool_result(&step, "c1", "run_command", &|_| None),
                SessionBlock::ToolResult { status, .. } if status == expected)
            );
        }
        let error = serde_json::json!({"source":"SYSTEM","type":"ERROR_MESSAGE","status":"DONE","error":"invalid args"});
        assert!(is_antigravity_tool_result(&error, "view_file"));
        assert!(
            matches!(antigravity_tool_result(&error, "c2", "view_file", &|_| None),
            SessionBlock::ToolResult { status: ToolStatus::Error, preview, .. } if preview == "invalid args")
        );
        for (name, kind) in [
            ("write_to_file", "CODE_ACTION"),
            ("replace_file_content", "CODE_ACTION"),
            ("multi_replace_file_content", "CODE_ACTION"),
            ("list_dir", "LIST_DIRECTORY"),
        ] {
            assert!(is_antigravity_tool_result(
                &serde_json::json!({"source":"MODEL","type":kind}),
                name
            ));
        }
        for step in [
            serde_json::json!({"source":"USER_EXPLICIT","type":"USER_INPUT"}),
            serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE"}),
            serde_json::json!({"source":"MODEL","type":"SEARCH_WEB"}),
            serde_json::json!({"source":"SYSTEM","type":"EPHEMERAL_MESSAGE"}),
        ] {
            assert!(!is_antigravity_tool_result(&step, "run_command"));
        }
    }

    #[test]
    fn antigravity_reader_maps_args_without_decoding_text_twice() {
        let args = serde_json::json!({
            "CommandLine": "\"true\"", "toolSummary": "Run check",
            "ReplacementChunks": "[{\"TargetContent\":\"true\",\"ReplacementContent\":\"null\"}]",
            "Url": "\"https://example.com\"", "Overwrite": "true"
        });
        let normalized = antigravity_reader_args(&args);
        assert_eq!(normalized["command"], "true");
        assert_eq!(normalized["edits"][0]["old_string"], "true");
        assert_eq!(normalized["edits"][0]["new_string"], "null");
        assert_eq!(normalized["url"], "https://example.com");
        assert_eq!(normalized["Overwrite"], true);
        assert!(normalized.get("CommandLine").is_none());
        let conflict =
            antigravity_reader_args(&serde_json::json!({"command":"pwd", "CommandLine":"\"ls\""}));
        assert_eq!(conflict["command"], "pwd");
        assert_eq!(conflict["CommandLine"], "ls");
    }

    #[test]
    fn antigravity_reader_refs_serve_normalized_inputs_and_complete_diffs() {
        use crate::session_manager::content::{
            resolve_content_ref, SourceLocation, ValidatedSource,
        };
        let (temp, path, _conn) = antigravity_message_fixture();
        let long_old = "old line\n".repeat(100);
        let step = serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
        "tool_calls":[
            {"name":"run_command", "args":{"CommandLine":"\"true\"", "toolSummary":"Run check"}},
            {"name":"multi_replace_file_content", "args":{"TargetFile":"\"/tmp/test\"", "ReplacementChunks":serde_json::to_string(&serde_json::json!([
                {"TargetContent":long_old,"ReplacementContent":"new\r\n"},
                {"TargetContent":"","ReplacementContent":"\n"}
            ])).unwrap()}},
            {"name":"write_to_file", "args":{"TargetFile":"\"/tmp/new\"", "CodeContent":"\"hello\\n\""}}
        ]});
        let original = format!("{step}\n");
        std::fs::write(&path, &original).unwrap();
        let source = ValidatedSource {
            provider_id: "gemini".into(),
            raw: path.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: path.clone(),
                root: temp.path().to_path_buf(),
            },
        };
        let messages = load_antigravity_messages(&path).unwrap();
        let SessionBlock::ToolCall {
            input_preview,
            input_full,
            title,
            ..
        } = &messages[0].blocks[0]
        else {
            panic!("call")
        };
        assert_eq!(title, "Run check");
        assert!(input_full.is_none());
        assert_eq!(
            serde_json::from_str::<Value>(input_preview).unwrap()["command"],
            "true"
        );
        let SessionBlock::ToolCall {
            input_full: Some(input),
            diff: Some(diff),
            ..
        } = &messages[0].blocks[1]
        else {
            panic!("long edit")
        };
        let normalized: Value =
            serde_json::from_str(&resolve_content_ref(&source, input).unwrap()).unwrap();
        assert_eq!(normalized["edits"][0]["old_string"], long_old);
        let body = resolve_content_ref(&source, diff.full.as_ref().unwrap()).unwrap();
        assert_eq!(
            body,
            format!("@@\n{}+new\n@@\n+\n", "-old line\n".repeat(100))
        );
        let SessionBlock::ToolCall {
            diff: Some(write), ..
        } = &messages[0].blocks[2]
        else {
            panic!("write")
        };
        assert_eq!(
            resolve_content_ref(&source, write.full.as_ref().unwrap()).unwrap(),
            "@@\n+hello\n"
        );
        for suffix in ["999/input", "0/diff", "1/input/extra", "../input"] {
            let bad = ContentRef::Jsonl {
                offset: 0,
                len: original.len() as u32,
                pointer: format!("{ANTIGRAVITY_READER_REF_PREFIX}{suffix}"),
            };
            assert!(resolve_content_ref(&source, &bad).is_err());
        }
        let other = ValidatedSource {
            provider_id: "codex".into(),
            ..source
        };
        assert!(resolve_content_ref(&other, input).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn antigravity_diff_preserves_content_that_looks_like_file_headers() {
        let step = serde_json::json!({"source":"MODEL","type":"PLANNER_RESPONSE", "tool_calls":[
            {"name":"replace_file_content", "args":{"TargetFile":"/tmp/file", "TargetContent":"\"-- a\"", "ReplacementContent":"\"++ b\""}}
        ]});
        let body = resolve_antigravity_reader_content(
            &step,
            &format!("{ANTIGRAVITY_READER_REF_PREFIX}0/diff"),
        )
        .unwrap();
        assert_eq!(body, "@@\n--- a\n@@\n+++ b\n");
    }

    #[test]
    fn antigravity_emoji_inputs_keep_full_params_accessible_to_existing_reader() {
        let command = "echo 🦀".to_string() + &"🦀".repeat(420);
        let step = serde_json::json!({"tool_calls":[{"name":"run_command", "args":{"CommandLine":serde_json::to_string(&command).unwrap()}}]});
        let blocks = antigravity_model_blocks(&step, "1", &|pointer| {
            Some(ContentRef::Jsonl {
                offset: 0,
                len: 1,
                pointer,
            })
        });
        let SessionBlock::ToolCall {
            input_preview,
            input_total_len,
            input_full,
            ..
        } = &blocks[0]
        else {
            panic!("call")
        };
        assert!(input_full.is_some());
        assert!(input_preview.encode_utf16().count() < *input_total_len as usize);
        assert_eq!(
            *input_total_len as usize,
            antigravity_reader_args(&step["tool_calls"][0]["args"])
                .to_string()
                .encode_utf16()
                .count()
        );
    }

    #[test]
    fn antigravity_reader_rejects_incomplete_diff_chunks_and_non_planner_refs() {
        let incomplete = serde_json::json!({"edits":[{"old_string":"a","new_string":"b"},{"old_string":"missing"}]});
        assert!(antigravity_diff_pairs(&incomplete, ToolKind::Edit).is_none());
        let truncated =
            antigravity_reader_args(&serde_json::json!({"ReplacementChunks":"[{<truncated>"}));
        assert!(antigravity_diff_pairs(&truncated, ToolKind::Edit).is_none());
        let result = serde_json::json!({"source":"MODEL","type":"GENERIC","tool_calls":[{"name":"run_command","args":{}}]});
        assert!(resolve_antigravity_reader_content(
            &result,
            &format!("{ANTIGRAVITY_READER_REF_PREFIX}0/input")
        )
        .is_err());
    }

    #[test]
    fn antigravity_long_content_refs_resolve_physical_jsonl_lines() {
        use crate::session_manager::content::{
            resolve_content_ref, SourceLocation, ValidatedSource,
        };
        let (temp, path, _conn) = antigravity_message_fixture();
        let thought = "思考🦀".repeat(200);
        let output = (0..30)
            .map(|i| format!("第{i}行输出"))
            .collect::<Vec<_>>()
            .join("\n");
        let args = serde_json::json!({"CommandLine": format!("\"{}\"", "echo hello ".repeat(60))});
        let planner = serde_json::json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE",
            "thinking":thought,"tool_calls":[{"name":"run_command","args":args}]});
        let execution = serde_json::json!({"step_index":2,"source":"MODEL","type":"RUN_COMMAND",
            "status":"DONE","content":[{"text":output}]});
        // Invalid/blank lines, CRLF, indentation and no trailing newline all affect byte ranges.
        std::fs::write(&path, format!("invalid\r\n\r\n  {planner}\r\n{execution}")).unwrap();
        let messages = load_antigravity_messages(&path).unwrap();
        let source = ValidatedSource {
            provider_id: "gemini".into(),
            raw: path.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: path.clone(),
                root: temp.path().to_path_buf(),
            },
        };
        let SessionBlock::Thinking {
            full: Some(thinking_ref),
            ..
        } = &messages[0].blocks[0]
        else {
            panic!("missing thinking ref")
        };
        assert_eq!(resolve_content_ref(&source, thinking_ref).unwrap(), thought);
        let SessionBlock::ToolCall {
            input_full: Some(args_ref),
            ..
        } = &messages[0].blocks[1]
        else {
            panic!("missing args ref")
        };
        assert_eq!(
            serde_json::from_str::<Value>(&resolve_content_ref(&source, args_ref).unwrap())
                .unwrap(),
            antigravity_reader_args(&args)
        );
        let SessionBlock::ToolResult {
            full: Some(result_ref),
            truncated: true,
            total_len,
            ..
        } = &messages[1].blocks[0]
        else {
            panic!("missing result ref")
        };
        assert_eq!(*total_len, output.chars().count() as u32);
        assert_eq!(resolve_content_ref(&source, result_ref).unwrap(), output);
    }

    #[test]
    fn load_antigravity_messages_populates_meta_from_conversations_db() {
        let (_temp, transcript, _conn) = antigravity_message_fixture();
        let messages = load_antigravity_messages(&transcript).expect("load messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, "assistant");
        let meta = messages[1].meta.as_ref().expect("meta should be present");
        assert_eq!(meta.input_tokens, Some(1500));
        assert_eq!(meta.output_tokens, Some(42));
        assert_eq!(meta.cache_read_tokens, Some(100));
        assert_eq!(meta.duration_ms, Some(2500));
        assert_eq!(meta.model.as_deref(), Some("gemini-3-flash-a"));
    }

    #[test]
    fn antigravity_cached_messages_follow_database_and_wal_updates() {
        use crate::session_manager::cache::{source_fingerprint, TranscriptCache};
        use crate::session_manager::content::{SourceLocation, ValidatedSource};

        let (temp, transcript, conn) = antigravity_message_fixture();
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;")
            .unwrap();
        let db_path = find_antigravity_db_path(&transcript).unwrap();
        let source = ValidatedSource {
            provider_id: "gemini".into(),
            raw: transcript.to_string_lossy().into_owned(),
            location: SourceLocation::Path {
                path: transcript.clone(),
                root: temp.path().to_path_buf(),
            },
        };
        let cache = TranscriptCache::new(8, usize::MAX);
        let key = (source.provider_id.clone(), source.raw.clone());
        let load = || load_transcript(&transcript);
        let before = source_fingerprint(&source).unwrap();
        let (first, cached) = cache.get_or_load(key.clone(), before, load).unwrap();
        assert!(!cached);
        assert_eq!(
            first.messages[1].meta.as_ref().unwrap().duration_ms,
            Some(2500)
        );
        assert!(cache.get_or_load(key.clone(), before, load).unwrap().1);

        let main_before = std::fs::metadata(&db_path).unwrap();
        conn.execute(
            "UPDATE gen_metadata SET data = ?1 WHERE idx = 0",
            [test_gen_metadata_blob(
                1500,
                42,
                100,
                5000,
                "gemini-3-flash-a",
            )],
        )
        .unwrap();
        let main_after = std::fs::metadata(&db_path).unwrap();
        assert_eq!(
            main_before.modified().unwrap(),
            main_after.modified().unwrap()
        );
        assert_eq!(main_before.len(), main_after.len());
        let wal_changed = source_fingerprint(&source).unwrap();
        assert_ne!(
            before, wal_changed,
            "WAL-only changes must invalidate messages"
        );
        let (updated, cached) = cache.get_or_load(key.clone(), wal_changed, load).unwrap();
        assert!(!cached);
        assert_eq!(
            updated.messages[1].meta.as_ref().unwrap().duration_ms,
            Some(5000)
        );
        assert_eq!(
            updated.messages[1].meta.as_ref().unwrap().output_tokens,
            Some(42)
        );

        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;")
            .unwrap();
        let checkpointed = source_fingerprint(&source).unwrap();
        conn.execute(
            "UPDATE gen_metadata SET data = ?1 WHERE idx = 0",
            [test_gen_metadata_blob(
                1500,
                42,
                100,
                8000,
                "gemini-3-flash-a",
            )],
        )
        .unwrap();
        // Make an in-place DB write observable even on coarse-mtime filesystems.
        std::fs::File::options()
            .write(true)
            .open(&db_path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        let db_changed = source_fingerprint(&source).unwrap();
        assert_ne!(checkpointed, db_changed);
        let (updated, cached) = cache.get_or_load(key, db_changed, load).unwrap();
        assert!(!cached);
        assert_eq!(
            updated.messages[1].meta.as_ref().unwrap().duration_ms,
            Some(8000)
        );
    }

    #[test]
    fn antigravity_metadata_read_errors_are_retried_without_caching_partial_messages() {
        use crate::session_manager::cache::{source_fingerprint, TranscriptCache};
        use crate::session_manager::content::{SourceLocation, ValidatedSource};

        for (failure, recovery) in [
            ("BEGIN EXCLUSIVE;", "COMMIT;"),
            (
                "ALTER TABLE gen_metadata RENAME TO unavailable_metadata;",
                "ALTER TABLE unavailable_metadata RENAME TO gen_metadata;",
            ),
            ("UPDATE steps SET metadata = 'invalid blob';", ""),
        ] {
            let (temp, transcript, conn) = antigravity_message_fixture();
            // Inline metadata must survive even when optional DB enrichment fails.
            let text = std::fs::read_to_string(&transcript).unwrap();
            let text = text.replace("\"Hi there!\"", "\"Hi there!\", \"input_tokens\": 7");
            std::fs::write(&transcript, text).unwrap();
            let source = ValidatedSource {
                provider_id: "gemini".into(),
                raw: transcript.to_string_lossy().into_owned(),
                location: SourceLocation::Path {
                    path: transcript.clone(),
                    root: temp.path().to_path_buf(),
                },
            };
            let cache = TranscriptCache::new(8, usize::MAX);
            let key = (source.provider_id.clone(), source.raw.clone());
            conn.execute_batch(failure).unwrap();
            let fingerprint = source_fingerprint(&source).unwrap();
            let load = || load_transcript(&transcript);
            let (degraded, cached) = cache.get_or_load(key.clone(), fingerprint, load).unwrap();
            assert!(!cached);
            assert_eq!(degraded.messages.len(), 2);
            assert_eq!(degraded.messages[0].content, "hello");
            assert_eq!(degraded.messages[1].content, "Hi there!");
            assert_eq!(
                degraded.messages[1].meta.as_ref().unwrap().input_tokens,
                Some(7)
            );
            assert_eq!(
                degraded.messages[1].meta.as_ref().unwrap().duration_ms,
                None
            );
            assert!(cache.get(&key, fingerprint).is_none());
            // The non-cached loader must also remain usable while the DB is broken.
            assert_eq!(load_messages(&transcript).unwrap(), degraded.messages);

            conn.execute_batch(recovery).unwrap();
            conn.execute("UPDATE steps SET metadata = ?1", [test_step_metadata(0)])
                .unwrap();
            // Deliberately reuse the fingerprint: unlocking need not modify the DB.
            let (loaded, cached) = cache.get_or_load(key.clone(), fingerprint, load).unwrap();
            assert!(!cached);
            let meta = loaded.messages[1].meta.as_ref().unwrap();
            assert_eq!(meta.input_tokens, Some(7));
            assert_eq!(meta.output_tokens, Some(42));
            assert_eq!(meta.duration_ms, Some(2500));
            assert!(cache.get_or_load(key, fingerprint, load).unwrap().1);
        }
    }

    #[test]
    fn antigravity_database_lookup_stays_with_the_transcript_root() {
        let (temp, transcript, conn) = antigravity_message_fixture();
        let own_db = find_antigravity_db_path(&transcript).unwrap();
        drop(conn);
        std::fs::remove_file(&own_db).unwrap();
        // The old ancestor search would accidentally use this unrelated DB.
        // Keep all fixtures inside the temp root by making it the ancestor root.
        let nested_transcript = temp
            .path()
            .join("nested/brain/agy-session-meta/.system_generated/logs/transcript.jsonl");
        std::fs::create_dir_all(nested_transcript.parent().unwrap()).unwrap();
        std::fs::write(&nested_transcript, "{}\n").unwrap();
        std::fs::write(&own_db, b"unrelated DB").unwrap();
        assert_eq!(find_antigravity_db_path(&nested_transcript), None);
        assert_eq!(find_antigravity_db_path(&transcript), Some(own_db));
        assert_eq!(
            find_antigravity_db_path(&temp.path().join("transcript.jsonl")),
            None
        );
    }

    #[test]
    fn load_antigravity_messages_does_not_use_next_record_gap_for_duration() {
        let temp = tempdir().expect("tempdir");
        let transcript = temp.path().join("transcript.jsonl");

        // 助手回复后，用户等待十分钟（600,000ms）再提问：
        // 绝不应将此 10 分钟等待算作助手的生成耗时
        let lines = [
            serde_json::json!({
                "step_index": 0,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "content": "<USER_REQUEST>hello</USER_REQUEST>",
                "created_at": "2026-10-10T10:00:00Z"
            })
            .to_string(),
            serde_json::json!({
                "step_index": 1,
                "source": "MODEL",
                "type": "PLANNER_RESPONSE",
                "content": "Hi there!",
                "created_at": "2026-10-10T10:00:00Z"
            })
            .to_string(),
            serde_json::json!({
                "step_index": 2,
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "content": "<USER_REQUEST>next question after 10m</USER_REQUEST>",
                "created_at": "2026-10-10T10:10:00Z"
            })
            .to_string(),
        ];
        std::fs::write(&transcript, lines.join("\n")).expect("write transcript");

        let messages = load_antigravity_messages(&transcript).expect("load messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].role, "assistant");
        let duration = messages[1].meta.as_ref().and_then(|m| m.duration_ms);
        assert_eq!(
            duration, None,
            "无法确定真实生成耗时时应保持缺失，绝不使用下一记录间隔"
        );
    }

    #[test]
    fn delete_antigravity_session_keeps_brain_when_conversation_cleanup_fails() {
        let temp = tempdir().expect("tempdir");
        let session_id = "agy-session-123";
        let transcript = temp
            .path()
            .join("brain")
            .join(session_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        std::fs::create_dir_all(transcript.parent().expect("transcript parent"))
            .expect("create brain");
        std::fs::write(&transcript, "{}\n").expect("write transcript");

        let blocking_db = temp
            .path()
            .join("conversations")
            .join(format!("{session_id}.db"));
        std::fs::create_dir_all(&blocking_db).expect("create blocking db directory");

        delete_antigravity_session(temp.path(), &transcript, session_id)
            .expect_err("conversation cleanup should fail");

        assert!(
            transcript.is_file(),
            "brain transcript must remain retryable"
        );
    }

    fn write_antigravity_delete_fixture(root: &Path, session_id: &str) -> PathBuf {
        let transcript = root
            .join("brain")
            .join(session_id)
            .join(".system_generated/logs/transcript.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(&transcript, "{}\n").unwrap();
        transcript
    }

    #[cfg(any(unix, windows))]
    fn create_test_directory_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();

        #[cfg(windows)]
        {
            // Junctions cover Windows directory reparse points without
            // requiring Developer Mode or symbolic-link privileges on CI.
            let target = if target.is_absolute() {
                target.to_path_buf()
            } else {
                link.parent().unwrap().join(target)
            };
            let result = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .expect("create Windows directory junction");
            assert!(
                result.status.success(),
                "failed to create junction: {} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn delete_antigravity_session_rejects_external_and_dangling_conversation_directories() {
        for target_kind in ["existing", "empty", "dangling"] {
            let root = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let session_id = "agy-session-123";
            let transcript = write_antigravity_delete_fixture(root.path(), session_id);
            let external_db = outside.path().join(format!("{session_id}.db"));
            if target_kind == "existing" {
                std::fs::write(&external_db, "outside database").unwrap();
            }
            let target = if target_kind == "dangling" {
                outside.path().join("missing")
            } else {
                outside.path().to_path_buf()
            };
            create_test_directory_link(&target, &root.path().join("conversations"));

            assert!(delete_antigravity_session(root.path(), &transcript, session_id).is_err());
            assert!(
                transcript.exists(),
                "validation must preserve the transcript"
            );
            if target_kind == "existing" {
                assert_eq!(
                    std::fs::read_to_string(external_db).unwrap(),
                    "outside database"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn delete_antigravity_session_validates_late_targets_before_deleting_any_files() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let session_id = "agy-session-123";
        let transcript = write_antigravity_delete_fixture(root.path(), session_id);
        let conversations = root.path().join("conversations");
        std::fs::create_dir(&conversations).unwrap();
        let db = conversations.join(format!("{session_id}.db"));
        std::fs::write(&db, "session database").unwrap();
        let external_meta = outside.path().join("metadata");
        std::fs::write(&external_meta, "external metadata").unwrap();
        std::os::unix::fs::symlink(
            &external_meta,
            conversations.join(format!("{session_id}.meta")),
        )
        .unwrap();

        let error = delete_antigravity_session(root.path(), &transcript, session_id).unwrap_err();
        assert!(error.contains("outside the session root"));
        assert!(transcript.exists());
        assert_eq!(std::fs::read_to_string(db).unwrap(), "session database");
        assert_eq!(
            std::fs::read_to_string(external_meta).unwrap(),
            "external metadata"
        );
    }

    #[cfg(windows)]
    #[test]
    fn delete_antigravity_session_validates_late_junctions_before_deleting_any_files() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let session_id = "agy-session-123";
        let transcript = write_antigravity_delete_fixture(root.path(), session_id);
        let conversations = root.path().join("conversations");
        std::fs::create_dir(&conversations).unwrap();
        let db = conversations.join(format!("{session_id}.db"));
        std::fs::write(&db, "session database").unwrap();
        let external_meta = outside.path().join("metadata");
        std::fs::write(&external_meta, "external metadata").unwrap();
        create_test_directory_link(
            outside.path(),
            &conversations.join(format!("{session_id}.meta")),
        );

        let error = delete_antigravity_session(root.path(), &transcript, session_id).unwrap_err();
        assert!(error.contains("outside the session root"));
        assert!(transcript.exists());
        assert_eq!(std::fs::read_to_string(db).unwrap(), "session database");
        assert_eq!(
            std::fs::read_to_string(external_meta).unwrap(),
            "external metadata"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn delete_antigravity_session_accepts_conversation_symlinks_inside_the_root() {
        let root = tempdir().unwrap();
        let session_id = "agy-session-123";
        let transcript = write_antigravity_delete_fixture(root.path(), session_id);
        let conversations = root.path().join("stored-conversations");
        std::fs::create_dir(&conversations).unwrap();
        let db = conversations.join(format!("{session_id}.db"));
        std::fs::write(&db, "session database").unwrap();
        create_test_directory_link(
            Path::new("stored-conversations"),
            &root.path().join("conversations"),
        );

        assert!(delete_antigravity_session(root.path(), &transcript, session_id).unwrap());
        assert!(!transcript.exists());
        assert!(!db.exists());
        assert!(conversations.is_dir());
    }

    #[test]
    fn delete_antigravity_session_rejects_a_transcript_from_another_root() {
        let root = tempdir().unwrap();
        let other_root = tempdir().unwrap();
        let session_id = "agy-session-123";
        let transcript = write_antigravity_delete_fixture(root.path(), session_id);
        let other_transcript = write_antigravity_delete_fixture(other_root.path(), session_id);

        assert!(delete_antigravity_session(root.path(), &other_transcript, session_id).is_err());
        assert!(transcript.exists());
        assert!(other_transcript.exists());
    }

    #[test]
    fn delete_antigravity_session_handles_unicode_paths_without_conversation_files() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("Antigravity sessions 中文");
        let session_id = "会话 session-123";
        let transcript = write_antigravity_delete_fixture(&root, session_id);
        let other = write_antigravity_delete_fixture(&root, "other-session");

        assert!(delete_antigravity_session(&root, &transcript, session_id).unwrap());
        assert!(!transcript.exists());
        assert!(other.exists());
    }

    #[cfg(windows)]
    #[test]
    fn delete_antigravity_session_preserves_brain_when_database_is_locked() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempdir().unwrap();
        let session_id = "agy-session-123";
        let transcript = write_antigravity_delete_fixture(root.path(), session_id);
        let conversations = root.path().join("conversations");
        std::fs::create_dir(&conversations).unwrap();
        let db = conversations.join(format!("{session_id}.db"));
        std::fs::write(&db, "locked database").unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&db)
            .unwrap();

        assert!(delete_antigravity_session(root.path(), &transcript, session_id).is_err());
        assert!(transcript.exists());
        drop(locked);
        assert_eq!(std::fs::read_to_string(db).unwrap(), "locked database");
    }

    #[test]
    fn test_normalize_workspace_path_handles_uris_and_local_paths() {
        assert_eq!(
            normalize_workspace_path("file:///home/example/Documents/Lab%20report/"),
            Some("/home/example/Documents/Lab report".to_string())
        );
        assert_eq!(normalize_workspace_path("file:///"), Some("/".to_string()));
        assert_eq!(
            normalize_workspace_path("file:///home/example/my-project"),
            Some("/home/example/my-project".to_string())
        );
        assert_eq!(
            normalize_workspace_path("file:///C:/Users/example/Project%20A"),
            Some("C:/Users/example/Project A".to_string())
        );
        assert_eq!(
            normalize_workspace_path("/var/log/app"),
            Some("/var/log/app".to_string())
        );
        assert_eq!(
            normalize_workspace_path(r"\\server\share\folder"),
            Some(r"\\server\share\folder".to_string())
        );
        assert_eq!(
            normalize_workspace_path("https://github.com/org/repo"),
            None
        );
        assert_eq!(normalize_workspace_path("ssh://git@github.com/repo"), None);
        assert_eq!(normalize_workspace_path("relative/path/to/folder"), None);
        assert_eq!(normalize_workspace_path(""), None);
        assert_eq!(normalize_workspace_path("   "), None);
    }

    #[test]
    fn file_uri_fallback_is_portable_across_windows_and_unix() {
        for (uri, expected) in [
            ("file:///", "/"),
            (
                "file:///Users/example/Project%20A/",
                "/Users/example/Project A",
            ),
            (
                "file:///home/example/%E4%B8%AD%E6%96%87/",
                "/home/example/中文",
            ),
            ("file:///C:/", "C:/"),
            ("file:///C%3A/Work/", "C:/Work"),
            ("file://server/share/", r"\\server\share"),
            ("file://server/C:/Work/", "C:/Work"),
        ] {
            let url = url::Url::parse(uri).unwrap();
            let fallback = portable_file_workspace_path(&url).unwrap();
            assert_eq!(normalize_workspace_separators(&fallback), expected);
            assert_eq!(normalize_workspace_path(uri).as_deref(), Some(expected));
        }
    }

    #[test]
    fn workspace_metadata_sources_use_the_same_directory_key() {
        for (uri, local, expected) in [
            ("file:///work/p/", "/work/p", "/work/p"),
            ("file:///C:/Work/p/", r"C:\Work\p\", "C:/Work/p"),
            (
                "file://server/share/p/",
                r"\\server\share\p\",
                r"\\server\share\p",
            ),
            ("file:///", "/", "/"),
            ("file:///C:/", r"C:\", "C:/"),
            ("file://server/share/", r"\\server\share", r"\\server\share"),
        ] {
            assert_eq!(normalize_workspace_path(uri).as_deref(), Some(expected));
            assert_eq!(normalize_workspace_path(local).as_deref(), Some(expected));
        }
        // Backslashes can be literal characters in POSIX directory names.
        assert_eq!(
            normalize_workspace_path("/work/p\\"),
            Some("/work/p\\".into())
        );
    }

    #[test]
    fn workspace_varints_reject_overflow_and_truncation() {
        for value in [0, 127, 128, u64::MAX] {
            let bytes = encode_proto_varint_test(value);
            let mut offset = 0;
            assert_eq!(read_proto_varint(&bytes, &mut offset), Some(value));
            assert_eq!(offset, bytes.len());
        }
        for bytes in [
            vec![0xff; 9],
            [vec![0xff; 9], vec![2]].concat(),
            [vec![0x80; 10], vec![0]].concat(),
        ] {
            assert_eq!(read_proto_varint(&bytes, &mut 0), None);
            let mut blob = vec![0x0a];
            blob.extend(bytes);
            assert_eq!(parse_workspace_from_trajectory_metadata_blob(&blob), None);
        }
    }

    #[test]
    fn test_is_safe_id_component_rejects_traversals() {
        assert!(!is_safe_id_component(""));
        assert!(!is_safe_id_component("."));
        assert!(!is_safe_id_component(".."));
        assert!(!is_safe_id_component("foo/bar"));
        assert!(!is_safe_id_component(r"foo\bar"));
        assert!(!is_safe_id_component("../escape"));
        assert!(!is_safe_id_component("foo\0bar"));
        assert!(is_safe_id_component("valid-id-123"));
        assert!(is_safe_id_component("0b5bc2e5-c309-4e7a-9468-caf34402bf01"));
    }

    fn encode_proto_varint_test(val: u64) -> Vec<u8> {
        let mut v = Vec::new();
        let mut n = val;
        while n >= 0x80 {
            v.push(((n & 0x7f) | 0x80) as u8);
            n >>= 7;
        }
        v.push(n as u8);
        v
    }

    fn encode_proto_tag_test(field_num: u32, wire_type: u32) -> Vec<u8> {
        encode_proto_varint_test(((field_num as u64) << 3) | (wire_type as u64))
    }

    fn encode_proto_len_delimited_test(field_num: u32, data: &[u8]) -> Vec<u8> {
        let mut v = encode_proto_tag_test(field_num, 2);
        v.extend(encode_proto_varint_test(data.len() as u64));
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn test_protobuf_workspace_parsing_order_and_corruption_resilience() {
        // Construct Protobuf: field 3 (dummy string) followed by field 1 (subfield 1 = uri)
        let sub1 = encode_proto_len_delimited_test(1, b"file:///home/example/Workspace/Project");
        let f1 = encode_proto_len_delimited_test(1, &sub1);
        let f3 = encode_proto_len_delimited_test(3, b"dummy-metadata");

        let mut blob = Vec::new();
        blob.extend(f3);
        blob.extend(f1);

        let parsed = parse_workspace_from_trajectory_metadata_blob(&blob);
        assert_eq!(parsed, Some("/home/example/Workspace/Project".to_string()));

        // Field 7 fallback
        let f7 = encode_proto_len_delimited_test(7, b"file:///home/example/FallbackF7");
        let parsed_f7 = parse_workspace_from_trajectory_metadata_blob(&f7);
        assert_eq!(parsed_f7, Some("/home/example/FallbackF7".to_string()));

        // Corrupted slice / invalid wire type / truncated varint
        assert_eq!(
            parse_workspace_from_trajectory_metadata_blob(&[0xFF, 0xFF]),
            None
        );
        assert_eq!(
            parse_workspace_from_trajectory_metadata_blob(&[0x0A, 0x50, 0x01]),
            None
        );
        assert_eq!(parse_workspace_from_trajectory_metadata_blob(&[]), None);
    }

    #[test]
    fn test_resolve_antigravity_workspace_dir_full_priority_chain() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        let session_id = "test-session-priority";
        let mut cache = HashMap::new();

        // 1. Summaries workspace_uris with multiple entries (first valid wins)
        let summary1 = AntigravitySummary {
            workspace_uris: Some(
                "[\"not-a-valid-path\", \"file:///home/example/FirstValidWS\", \"file:///home/example/SecondWS\"]"
                    .to_string(),
            ),
            project_id: Some("ignored-project-id".to_string()),
        };
        let res1 = resolve_antigravity_workspace_dir(root, session_id, Some(&summary1), &mut cache);
        assert_eq!(res1, Some("/home/example/FirstValidWS".to_string()));

        // 2. Fallback to ACP .meta
        let conv_dir = root.join("conversations");
        std::fs::create_dir_all(&conv_dir).expect("create conv_dir");
        let meta_file = conv_dir.join(format!("{session_id}.meta"));
        std::fs::write(&meta_file, r#"{"cwd": "/home/example/ACPCwd"}"#).expect("write meta");

        let res2 = resolve_antigravity_workspace_dir(root, session_id, None, &mut cache);
        assert_eq!(res2, Some("/home/example/ACPCwd".to_string()));

        // 3. Fallback to trajectory_metadata_blob in .db
        std::fs::remove_file(&meta_file).expect("remove meta");
        let db_file = conv_dir.join(format!("{session_id}.db"));
        let conn = rusqlite::Connection::open(&db_file).expect("open db");
        conn.execute(
            "CREATE TABLE trajectory_metadata_blob (id text DEFAULT 'main', data blob, PRIMARY KEY(id))",
            [],
        )
        .expect("create table");

        let sub1 = encode_proto_len_delimited_test(1, b"file:///home/example/TrajectoryBlobWS");
        let f1 = encode_proto_len_delimited_test(1, &sub1);
        conn.execute(
            "INSERT INTO trajectory_metadata_blob (id, data) VALUES ('main', ?1)",
            rusqlite::params![f1],
        )
        .expect("insert blob");

        let res3 = resolve_antigravity_workspace_dir(root, session_id, None, &mut cache);
        assert_eq!(res3, Some("/home/example/TrajectoryBlobWS".to_string()));

        // 4. Traversal session ID is rejected
        let res_traversal = resolve_antigravity_workspace_dir(root, "../sneaky", None, &mut cache);
        assert_eq!(res_traversal, None);

        // 5. None when all sources exhausted
        conn.execute("DELETE FROM trajectory_metadata_blob", [])
            .expect("delete blob");
        let res_none = resolve_antigravity_workspace_dir(root, session_id, None, &mut cache);
        assert_eq!(res_none, None);
    }

    #[test]
    fn test_parse_antigravity_session_directly() {
        let temp = tempdir().expect("tempdir");
        let session_id = "direct-parse-session";
        let transcript = temp
            .path()
            .join("brain")
            .join(session_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        std::fs::create_dir_all(transcript.parent().expect("parent")).expect("create brain");
        std::fs::write(
            &transcript,
            r#"{"ts": 1783689764000, "source": "USER_EXPLICIT", "type": "USER_INPUT", "content": "Direct test message"}"#,
        )
        .expect("write transcript");

        let meta = parse_antigravity_session(&transcript).expect("parse session");
        assert_eq!(meta.session_id, session_id);
        assert_eq!(meta.title.as_deref(), Some("Direct test message"));
    }

    #[test]
    fn antigravity_metadata_disables_resume_for_unsafe_session_ids() {
        let temp = tempdir().expect("tempdir");
        let transcript = temp.path().join("transcript.jsonl");
        std::fs::write(
            &transcript,
            r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"Example"}"#,
        )
        .expect("write transcript");

        for session_id in [
            "session; printf unexpected",
            "session$(printf unexpected)",
            "session`printf unexpected`",
            "session\nprintf unexpected",
            "session with spaces",
            "session'quote",
            "session\"quote",
            "session&command",
            "session|command",
            "session>file",
            "session%VARIABLE%",
            "session!VARIABLE!",
            "session^command",
            "-option",
            "",
            ".",
            "..",
        ] {
            let meta = parse_antigravity_session_with_project_dir(&transcript, session_id, None)
                .expect("session remains readable");
            assert_eq!(meta.session_id, session_id);
            assert_eq!(meta.title.as_deref(), Some("Example"));
            assert!(meta.resume_command.is_none(), "{session_id:?}");
        }
    }

    #[test]
    fn antigravity_resume_command_preserves_safe_session_ids() {
        for session_id in [
            "12345678-1234-1234-1234-123456789abc",
            "agy-session-123",
            "session_with_underscores.v2",
        ] {
            assert_eq!(
                antigravity_resume_command(session_id),
                Some(format!("agy --conversation {session_id}"))
            );
        }
    }
    // Opt-in validation of copied client metadata; never runs against user data in CI.
    #[test]
    #[ignore = "requires copied AGY metadata under CC_SWITCH_TEST_HOME"]
    fn validate_platform_workspace_metadata_copy() {
        assert!(std::env::var_os("CC_SWITCH_TEST_HOME").is_some());
        let base = crate::gemini_config::get_gemini_dir();
        let mut cache = HashMap::new();
        let mut total = 0;
        let mut resolved = 0;
        let mut blobs = 0;
        for name in crate::gemini_config::ANTIGRAVITY_ROOTS {
            let root = base.join(name);
            let summaries = load_root_conversation_summaries(&root);
            for (id, summary) in &summaries {
                total += 1;
                if let Some(path) =
                    resolve_antigravity_workspace_dir(&root, id, Some(summary), &mut cache)
                {
                    assert_eq!(
                        normalize_workspace_path(&path).as_deref(),
                        Some(path.as_str())
                    );
                    resolved += 1;
                }
                if read_trajectory_metadata_workspace(&root, id).is_some() {
                    blobs += 1;
                }
            }
        }
        assert!(total > 0, "fixture must contain summaries");
        assert!(resolved > 0, "fixture must contain a resolvable workspace");
        println!(
            "platform metadata: summaries={total}, resolved={resolved}, blob_workspaces={blobs}"
        );
    }
}
