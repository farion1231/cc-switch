use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::session_manager::model::{
    ContentRef, DiffOp, EventKind, MessageMeta, SessionBlock, ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, count_diff_lines, single_file_diff, thinking_block, tool_call_block,
    tool_result_block, ToolSource,
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

fn is_antigravity_transcript(path: &Path) -> bool {
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
            let path_str = url.path();
            if path_str.len() >= 3
                && path_str.as_bytes()[0] == b'/'
                && path_str.as_bytes()[1].is_ascii_alphabetic()
                && path_str.as_bytes()[2] == b':'
            {
                url_decode_simple(&path_str[1..])
            } else if let Some(host) = url.host_str() {
                if !host.is_empty() {
                    let decoded_path = url_decode_simple(url.path());
                    format!(r"\\{host}{decoded_path}")
                } else {
                    return None;
                }
            } else {
                return None;
            }
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

        return Some(path);
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
        Some(raw.to_string())
    } else {
        None
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
    let mut val: u64 = 0;
    let mut shift: u32 = 0;
    while *offset < buf.len() {
        let b = buf[*offset];
        *offset += 1;
        val = val.checked_add(((b & 0x7f) as u64).checked_shl(shift)?)?;
        if (b & 0x80) == 0 {
            return Some(val);
        }
        shift = shift.checked_add(7)?;
        if shift > 64 {
            return None;
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
        resume_command: Some(format!("agy --conversation {session_id}")),
    })
}

fn load_antigravity_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read Antigravity transcript: {e}"))?;
    let mut result = Vec::new();

    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let source = value.get("source").and_then(Value::as_str);
        let message_type = value.get("type").and_then(Value::as_str);
        let role = match (source, message_type) {
            (Some("USER_EXPLICIT"), Some("USER_INPUT")) => "user",
            (Some("MODEL"), Some("PLANNER_RESPONSE") | Some("GENERIC")) => "assistant",
            _ => continue,
        };

        let content = clean_antigravity_content(extract_antigravity_content(&value));
        if content.trim().is_empty() {
            continue;
        }

        let ts = parse_antigravity_timestamp(&value);
        let mut message = SessionMessage::from_blocks(role, ts, vec![SessionBlock::text(content)]);
        message.id = value
            .get("step_index")
            .and_then(|v| v.as_i64())
            .map(|n| n.to_string());
        result.push(message);
    }

    assign_turn_ids(&mut result);
    Ok(result)
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

    let conversation_base = root.join("conversations");
    for suffix in ["db", "db-shm", "db-wal", "db-journal", "pb", "meta"] {
        let file = conversation_base.join(format!("{session_id}.{suffix}"));
        remove_file_if_exists(&file).map_err(|e| {
            format!(
                "Failed to delete Antigravity conversation file {}: {e}",
                file.display()
            )
        })?;
    }

    let brain_dir = root.join("brain").join(session_id);
    remove_dir_all_if_exists(&brain_dir).map_err(|e| {
        format!(
            "Failed to delete Antigravity brain directory {}: {e}",
            brain_dir.display()
        )
    })?;

    Ok(true)
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

    #[test]
    fn test_normalize_workspace_path_handles_uris_and_local_paths() {
        assert_eq!(
            normalize_workspace_path("file:///home/example/Documents/Lab%20report/"),
            Some("/home/example/Documents/Lab report/".to_string())
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
}
