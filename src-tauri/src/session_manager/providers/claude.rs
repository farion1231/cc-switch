use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;

use crate::config::get_claude_config_dir;
use crate::session_manager::model::{
    ContentRef, DiffFile, DiffOp, DiffSummary, EventKind, ImageRef, ImageSource, MessageMeta,
    SessionBlock, ToolKind, ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    assign_turn_ids, estimate_base64_size, first_string_field, large_text_block,
    line_change_counts, normalize_tool, one_line_title, preview, preview_chars, refine_shell_kind,
    split_mcp_name, summary_event_block, title_agent, title_ask, title_mcp, title_other,
    title_path, title_read, title_search, title_shell, title_todo, title_web, NormalizedTool,
    ToolSource, INPUT_PREVIEW_CHARS, THINKING_PREVIEW_CHARS,
};
use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    FileParseCache, JsonlSpan, LineSpans, TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "claude";

/// 会话页每次打开都会全量扫描；没变过的文件直接复用上次的解析结果。
static PARSE_CACHE: LazyLock<FileParseCache> = LazyLock::new(FileParseCache::new);

pub fn scan_sessions() -> Vec<SessionMeta> {
    let root = get_claude_config_dir().join("projects");
    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files);

    PARSE_CACHE.scan(files, scan_session_file)
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path).ok_or_else(|| {
        format!(
            "Failed to parse Claude session metadata: {}",
            path.display()
        )
    })?;

    if meta.session_id != session_id {
        return Err(format!(
            "Claude session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    if let Some(stem) = path.file_stem() {
        let sibling = path.parent().unwrap_or_else(|| Path::new("")).join(stem);
        remove_path_if_exists(&sibling).map_err(|e| {
            format!(
                "Failed to delete Claude session sidecar {}: {e}",
                sibling.display()
            )
        })?;
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Claude session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    scan_session_file(path).ok().flatten()
}

/// 列表扫描用：读不了（权限、文件被占用等）返回 `Err`，不进解析缓存、下轮重试；
/// 读到了但不是会话返回 `Ok(None)`，可以缓存。
fn scan_session_file(path: &Path) -> std::io::Result<Option<SessionMeta>> {
    if is_agent_session(path) {
        return Ok(None);
    }
    let (head, tail) = read_head_tail_lines(path, 10, 30)?;
    Ok(parse_session_lines(path, head, tail))
}

fn parse_session_lines(path: &Path, head: Vec<String>, tail: Vec<String>) -> Option<SessionMeta> {
    let mut session_id: Option<String> = None;
    let mut project_dir: Option<String> = None;
    let mut created_at: Option<i64> = None;
    let mut first_user_message: Option<String> = None;

    // Extract metadata and first user message from head lines
    for line in &head {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if session_id.is_none() {
            session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        if project_dir.is_none() {
            project_dir = value
                .get("cwd")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
        }
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        // Extract first real user message as title candidate
        // Skip system-injected caveats and slash commands (e.g. /clear, /compact)
        if first_user_message.is_none() {
            let is_user = value.get("type").and_then(Value::as_str) == Some("user")
                || value
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(Value::as_str)
                    == Some("user");
            if is_user {
                if let Some(message) = value.get("message") {
                    let text = message.get("content").map(extract_text).unwrap_or_default();
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && !trimmed.contains("<local-command-caveat>")
                        && !trimmed.starts_with("<command-name>")
                    {
                        first_user_message = Some(trimmed.to_string());
                    }
                }
            }
        }
        if session_id.is_some()
            && project_dir.is_some()
            && created_at.is_some()
            && first_user_message.is_some()
        {
            break;
        }
    }

    // Extract last_active_at, summary, and custom-title from tail lines (reverse order)
    let mut last_active_at: Option<i64> = None;
    let mut summary: Option<String> = None;
    let mut custom_title: Option<String> = None;

    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if last_active_at.is_none() {
            last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        // Look for custom-title entry (take the last one, i.e. first in reverse)
        if custom_title.is_none()
            && value.get("type").and_then(Value::as_str) == Some("custom-title")
        {
            custom_title = value
                .get("customTitle")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if summary.is_none() {
            if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(message) = value.get("message") {
                let text = message.get("content").map(extract_text).unwrap_or_default();
                if !text.trim().is_empty() {
                    summary = Some(text);
                }
            }
        }
        if last_active_at.is_some() && summary.is_some() && custom_title.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| infer_session_id_from_filename(path));
    let session_id = session_id?;

    // Title priority: custom-title > first user message > directory basename
    let title = custom_title
        .map(|t| truncate_summary(&t, TITLE_MAX_CHARS))
        .or_else(|| first_user_message.map(|t| truncate_summary(&t, TITLE_MAX_CHARS)))
        .or_else(|| {
            project_dir
                .as_deref()
                .and_then(path_basename)
                .map(|v| v.to_string())
        });

    let summary = summary.map(|text| truncate_summary(&text, 160));

    Some(SessionMeta {
        provider_id: PROVIDER_ID.to_string(),
        session_id: session_id.clone(),
        title,
        summary,
        project_dir,
        created_at,
        last_active_at,
        source_path: Some(path.to_string_lossy().to_string()),
        resume_command: Some(format!("claude --resume {session_id}")),
    })
}

fn is_agent_session(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("agent-") || name == "journal.jsonl")
        .unwrap_or(false)
}

fn infer_session_id_from_filename(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_string())
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }

    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn remove_path_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

// ─── 会话正文解析（设计 §4.1）────────────────────────────────────────────
//
// 逐行读取 JSONL，用借用型的轻量结构体反序列化（只取需要的字段，图片 base64、
// 工具大输出等都只扫过不复制），再按记录类型转成结构化 blocks。大内容只放预览，
// 全文给 `ContentRef::Jsonl`（行的字节区间 + JSON Pointer），由前端按需取。

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let mut lines = LineSpans::new(BufReader::with_capacity(256 * 1024, file));
    let mut builder = TranscriptBuilder::new(path.with_extension(""));

    // 读错误（文件被截断/替换）时保留已解析部分，与旧实现跳过坏行的行为一致
    while let Ok(Some(span)) = lines.next_line() {
        if span.bytes.is_empty() {
            continue;
        }
        let record: RawRecord = match serde_json::from_slice(span.bytes) {
            Ok(record) => record,
            Err(_) => continue,
        };
        builder.push(record, span.span);
    }

    Ok(builder.finish())
}

/// 解析中的消息；全部记录处理完后再统一投影出 `content`（结果记录会回填前面的 ToolCall）。
struct Draft {
    role: String,
    ts: Option<i64>,
    id: Option<String>,
    injected: bool,
    blocks: Vec<SessionBlock>,
    meta: Option<MessageMeta>,
    /// assistant 的 API `message.id`：Claude Code 把一次回复按内容块拆成多条记录，
    /// 同 id 的相邻记录合并成一条消息
    api_id: Option<String>,
}

/// 已见过的 tool_use 在 `drafts` 中的位置，供结果记录配对与回填。
struct CallSlot {
    draft: usize,
    block: usize,
    ts: Option<i64>,
    name: String,
}

struct TranscriptBuilder {
    /// 会话附属目录 `<sessionId>/`（tool-results 等落盘文件）
    sidecar_dir: PathBuf,
    drafts: Vec<Draft>,
    calls: HashMap<String, CallSlot>,
    /// pr-link 会在每轮结束时重复写入，同一个 URL 只出一次
    pr_urls: HashSet<String>,
}

impl TranscriptBuilder {
    fn new(sidecar_dir: PathBuf) -> Self {
        Self {
            sidecar_dir,
            drafts: Vec::new(),
            calls: HashMap::new(),
            pr_urls: HashSet::new(),
        }
    }

    fn finish(self) -> Vec<SessionMessage> {
        let mut messages: Vec<SessionMessage> = self
            .drafts
            .into_iter()
            .map(|draft| {
                let mut message = SessionMessage::from_blocks(draft.role, draft.ts, draft.blocks);
                message.id = draft.id;
                message.injected = draft.injected;
                message.meta = draft.meta;
                message
            })
            .filter(|message| !message.is_empty())
            .collect();
        assign_turn_ids(&mut messages);
        messages
    }

    fn push(&mut self, record: RawRecord<'_>, line: JsonlSpan) {
        if is_true(&record.is_sidechain) || is_true(&record.is_meta) {
            return;
        }
        let ts = record.timestamp.as_ref().and_then(parse_timestamp_to_ms);

        match record.kind.as_str() {
            Some("system") => self.push_system(&record, ts, line),
            Some("pr-link") => self.push_pr_link(&record, ts),
            Some("user") | Some("assistant") | None => {
                let kind = record.kind.as_str().map(str::to_string);
                let uuid = record.uuid.as_str().map(str::to_string);
                let thinking_ms = record.thinking_duration_ms.as_ref().and_then(as_u64);
                let compact_summary = is_true(&record.is_compact_summary);
                let tool_use_result = record.tool_use_result.0;
                let Some(message) = record.message.0 else {
                    return;
                };
                let role = message
                    .role
                    .as_str()
                    .map(str::to_string)
                    .or(kind)
                    .unwrap_or_else(|| "unknown".to_string());
                if compact_summary {
                    self.push_compact_summary(message.content, ts, line);
                } else if role == "user" {
                    self.push_user(message.content, tool_use_result, uuid, ts, line);
                } else {
                    self.push_assistant(role, message, uuid, thinking_ms, ts, line);
                }
            }
            // attachment、file-history-*、queue-operation、mode、custom-title 等：跳过
            _ => {}
        }
    }

    fn push_draft(
        &mut self,
        role: &str,
        ts: Option<i64>,
        id: Option<String>,
        blocks: Vec<SessionBlock>,
    ) {
        self.drafts.push(Draft {
            role: role.to_string(),
            ts,
            id,
            injected: false,
            blocks,
            meta: None,
            api_id: None,
        });
    }

    fn push_event(
        &mut self,
        ts: Option<i64>,
        kind: EventKind,
        text: Option<String>,
        url: Option<String>,
    ) {
        self.push_draft(
            "system",
            ts,
            None,
            vec![SessionBlock::event(kind, text, url)],
        );
    }

    // ── system / pr-link ──

    fn push_system(&mut self, record: &RawRecord<'_>, ts: Option<i64>, line: JsonlSpan) {
        match record.subtype.as_str() {
            Some("stop_hook_summary") => {
                let errors: Vec<String> = match &record.hook_errors {
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|item| match item {
                            Value::String(s) => s.trim().to_string(),
                            other => other.to_string(),
                        })
                        .filter(|s| !s.is_empty())
                        .collect(),
                    _ => Vec::new(),
                };
                if !errors.is_empty() {
                    self.push_event(ts, EventKind::Hook, Some(errors.join("\n")), None);
                }
            }
            Some("compact_boundary") => {
                let text = record
                    .content
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Conversation compacted");
                self.push_event(ts, EventKind::Compaction, Some(text.to_string()), None);
            }
            // 新版 Claude Code 把本地斜杠命令记成 system.local_command
            Some("local_command") => {
                if let Some(content) = record.content.as_str() {
                    let uuid = record.uuid.as_str().map(str::to_string);
                    self.push_user_texts(
                        vec![(Cow::Borrowed(content), line.content_ref("/content"))],
                        Vec::new(),
                        Vec::new(),
                        uuid,
                        ts,
                    );
                }
            }
            // turn_duration、api_error、informational、away_summary…：不展示
            _ => {}
        }
    }

    fn push_pr_link(&mut self, record: &RawRecord<'_>, ts: Option<i64>) {
        let Some(url) = record.pr_url.as_str().filter(|u| !u.trim().is_empty()) else {
            return;
        };
        if !self.pr_urls.insert(url.to_string()) {
            return;
        }
        let text = match record.pr_number.as_ref() {
            Some(Value::Number(n)) => format!("PR #{n}"),
            Some(Value::String(s)) if !s.is_empty() => format!("PR #{s}"),
            _ => "PR".to_string(),
        };
        self.push_event(ts, EventKind::PrLink, Some(text), Some(url.to_string()));
    }

    /// `/compact` 之后的摘要记录：并入紧邻的 compact_boundary 事件，没有则单独成一条。
    /// 摘要通常有数 KB，只放预览，全文按 `/message/content` 引用取。
    fn push_compact_summary(&mut self, content: RawContent<'_>, ts: Option<i64>, line: JsonlSpan) {
        let summary = content_text(&content);
        if summary.trim().is_empty() {
            return;
        }
        let block = summary_event_block(EventKind::Compaction, &summary, || {
            Some(line.content_ref("/message/content"))
        });
        if let Some(last) = self.drafts.last_mut() {
            if let [event @ SessionBlock::Event {
                kind: EventKind::Compaction,
                ..
            }] = last.blocks.as_mut_slice()
            {
                *event = block;
                return;
            }
        }
        self.push_draft("system", ts, None, vec![block]);
    }

    // ── assistant ──

    fn push_assistant(
        &mut self,
        role: String,
        message: RawMessage<'_>,
        uuid: Option<String>,
        thinking_ms: Option<u64>,
        ts: Option<i64>,
        line: JsonlSpan,
    ) {
        let mut blocks = Vec::new();
        // (块下标, tool_use id, 工具名)
        let mut calls: Vec<(usize, String, String)> = Vec::new();
        match message.content {
            RawContent::Text(text) => {
                if !text.trim().is_empty() {
                    blocks.push(SessionBlock::text(text.into_owned()));
                }
            }
            RawContent::Items(items) => {
                for (i, item) in items.into_iter().enumerate() {
                    let Some(item) = item.0 else { continue };
                    match item.kind.as_str().unwrap_or_default() {
                        "text" => {
                            if let Some(text) = item.text.0.filter(|t| !t.trim().is_empty()) {
                                blocks.push(SessionBlock::text(text.into_owned()));
                            }
                        }
                        kind @ ("thinking" | "redacted_thinking") => {
                            if let Some(block) = thinking_block(&item, kind, i, thinking_ms, line) {
                                blocks.push(block);
                            }
                        }
                        "tool_use" | "server_tool_use" => {
                            let block = tool_call_block(item, i, line);
                            if let SessionBlock::ToolCall { id, raw_name, .. } = &block {
                                if !id.is_empty() {
                                    calls.push((blocks.len(), id.clone(), raw_name.clone()));
                                }
                            }
                            blocks.push(block);
                        }
                        _ => {}
                    }
                }
            }
            RawContent::None => {}
        }
        if blocks.is_empty() {
            return;
        }

        let meta = message_meta(&message.model, message.usage.as_ref(), &message.stop_reason);
        let api_id = message.id.as_str().map(str::to_string);
        let mergeable = self
            .drafts
            .last()
            .is_some_and(|last| last.role == role && api_id.is_some() && last.api_id == api_id);
        let (draft, base) = if mergeable {
            let last = self.drafts.last_mut().expect("checked above");
            let base = last.blocks.len();
            last.blocks.extend(blocks);
            merge_meta(&mut last.meta, meta);
            (self.drafts.len() - 1, base)
        } else {
            self.drafts.push(Draft {
                role,
                ts,
                id: uuid,
                injected: false,
                blocks,
                meta,
                api_id,
            });
            (self.drafts.len() - 1, 0)
        };
        for (index, id, name) in calls {
            self.calls.insert(
                id,
                CallSlot {
                    draft,
                    block: base + index,
                    ts,
                    name,
                },
            );
        }
    }

    // ── user（提问 / 工具结果 / 注入内容）──

    fn push_user(
        &mut self,
        content: RawContent<'_>,
        tool_use_result: Option<ToolUseResult>,
        uuid: Option<String>,
        ts: Option<i64>,
        line: JsonlSpan,
    ) {
        let mut results = Vec::new();
        let mut texts = Vec::new();
        let mut images = Vec::new();
        match content {
            RawContent::Text(text) => texts.push((text, line.content_ref("/message/content"))),
            RawContent::Items(items) => {
                let result_count = items
                    .iter()
                    .filter(|item| {
                        item.0
                            .as_ref()
                            .is_some_and(|i| i.kind.as_str() == Some("tool_result"))
                    })
                    .count();
                // 顶层 toolUseResult 只描述一个结果；多结果记录不知道属于谁，不用
                let tool_use_result = tool_use_result.filter(|_| result_count == 1);
                for (i, item) in items.into_iter().enumerate() {
                    let Some(item) = item.0 else { continue };
                    match item.kind.as_str().unwrap_or_default() {
                        "tool_result" => results.push(self.tool_result_block(
                            item,
                            i,
                            tool_use_result.as_ref(),
                            ts,
                            line,
                        )),
                        "text" => {
                            if let Some(text) = item.text.0 {
                                texts.push((
                                    text,
                                    line.content_ref(format!("/message/content/{i}/text")),
                                ));
                            }
                        }
                        "image" => {
                            if let Some(image) = image_ref(
                                item.source.0.as_ref(),
                                format!("/message/content/{i}/source/data"),
                                line,
                            ) {
                                images.push(SessionBlock::Image { image });
                            }
                        }
                        _ => {}
                    }
                }
            }
            RawContent::None => {}
        }
        self.push_user_texts(texts, results, images, uuid, ts);
    }

    /// 按 §4.1 给用户文本归类：中断 / 斜杠命令 / `!` 命令 / 注入内容 / 普通提问。
    /// `texts` 每项带上它在源记录里的引用，超长注入文本只放预览 + 引用。
    fn push_user_texts(
        &mut self,
        texts: Vec<(Cow<'_, str>, ContentRef)>,
        mut blocks: Vec<SessionBlock>,
        images: Vec<SessionBlock>,
        uuid: Option<String>,
        ts: Option<i64>,
    ) {
        let has_results = !blocks.is_empty();
        let mut injected = false;

        let injected_blocks = |texts: &[(Cow<'_, str>, ContentRef)]| -> Vec<SessionBlock> {
            texts
                .iter()
                .map(|(text, full)| large_text_block(text.to_string(), || Some(full.clone())))
                .collect()
        };
        if let Some(first) = texts.iter().map(|(t, _)| t.trim()).find(|t| !t.is_empty()) {
            if first.starts_with("[Request interrupted by user") {
                blocks.push(SessionBlock::event(
                    EventKind::Aborted,
                    Some(first.to_string()),
                    None,
                ));
            } else if let Some(command) = slash_command(first) {
                blocks.push(SessionBlock::event(
                    EventKind::SlashCommand,
                    Some(command),
                    None,
                ));
            } else if first.starts_with("<bash-input>") {
                blocks.extend(user_bash_blocks(first, uuid.as_deref()));
            } else if is_injected_text(first) {
                // 与工具结果同记录的注入文本直接丢弃；单独成条时整条标 injected、保留原文
                if !has_results {
                    injected = true;
                    blocks.extend(injected_blocks(&texts));
                }
            } else {
                let stripped: Vec<String> = texts
                    .iter()
                    .map(|(t, _)| strip_system_reminders(t))
                    .filter(|t| !t.trim().is_empty())
                    .collect();
                if !stripped.is_empty() {
                    blocks.extend(stripped.into_iter().map(SessionBlock::text));
                } else if !has_results && images.is_empty() {
                    // 全是 <system-reminder>：整条注入，保留原文
                    injected = true;
                    blocks.extend(injected_blocks(&texts));
                }
            }
        }
        blocks.extend(images);
        if blocks.is_empty() {
            return;
        }

        let all_results = blocks
            .iter()
            .all(|b| matches!(b, SessionBlock::ToolResult { .. }));
        let role = if all_results { "tool" } else { "user" };
        self.push_draft(role, ts, uuid, blocks);
        if let Some(last) = self.drafts.last_mut() {
            last.injected = injected;
        }
    }

    fn tool_result_block(
        &mut self,
        item: RawItem<'_>,
        index: usize,
        tool_use_result: Option<&ToolUseResult>,
        ts: Option<i64>,
        line: JsonlSpan,
    ) -> SessionBlock {
        let call_id = item.tool_use_id.as_str().unwrap_or_default().to_string();
        let base = format!("/message/content/{index}/content");
        let mut images = Vec::new();
        let (text, pointer): (Cow<'_, str>, Option<String>) = match item.content {
            RawContent::Text(text) => (text, Some(base)),
            RawContent::Items(parts) => {
                let mut texts: Vec<(usize, Cow<'_, str>)> = Vec::new();
                for (j, part) in parts.into_iter().enumerate() {
                    let Some(part) = part.0 else { continue };
                    match part.kind.as_str().unwrap_or_default() {
                        "text" => {
                            if let Some(text) = part.text.0 {
                                texts.push((j, text));
                            }
                        }
                        "image" => {
                            if let Some(image) = image_ref(
                                part.source.0.as_ref(),
                                format!("{base}/{j}/source/data"),
                                line,
                            ) {
                                images.push(image);
                            }
                        }
                        _ => {}
                    }
                }
                match texts.len() {
                    0 => (Cow::Borrowed(""), None),
                    // 只有一段文本时直接指向字符串，P2 不用再拼接
                    1 => {
                        let (j, text) = texts.pop().expect("len checked");
                        (text, Some(format!("{base}/{j}/text")))
                    }
                    _ => {
                        let joined = texts
                            .iter()
                            .map(|(_, t)| t.as_ref())
                            .collect::<Vec<_>>()
                            .join("\n");
                        (Cow::Owned(joined), Some(base))
                    }
                }
            }
            RawContent::None => (Cow::Borrowed(""), None),
        };
        let text = if text.contains(SYSTEM_REMINDER_OPEN) {
            Cow::Owned(strip_system_reminders(&text))
        } else {
            text
        };
        let p = preview(&text);

        let is_error = item.is_error.as_ref().and_then(Value::as_bool) == Some(true);
        let interrupted = tool_use_result
            .and_then(|r| r.interrupted.as_ref())
            .and_then(Value::as_bool)
            == Some(true);
        let status = if interrupted {
            ToolStatus::Interrupted
        } else if is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Success
        };

        let slot = self.calls.get(&call_id);
        let is_shell = slot.is_some_and(|s| s.name == "Bash");
        // 退出码与耗时只对 Bash 有意义（其余工具的「结果时间 - 调用时间」多是排队与思考时间）
        let exit_code = parse_exit_code(&text).or_else(|| {
            let background = tool_use_result.is_some_and(|r| r.background_task_id.is_some());
            (is_shell && status == ToolStatus::Success && !background).then_some(0)
        });
        let duration_ms = match (is_shell, slot.and_then(|s| s.ts), ts) {
            (true, Some(start), Some(end)) if end >= start => Some((end - start) as u64),
            _ => None,
        };

        let saved_path = tool_use_result
            .and_then(|r| r.persisted_output_path.as_ref())
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(str::to_string);
        let full = if p.truncated {
            saved_path
                .as_deref()
                .and_then(|path| self.sidecar_rel_path(path))
                .map(|rel_path| ContentRef::Sidecar { rel_path })
                .or_else(|| pointer.map(|ptr| line.content_ref(ptr)))
        } else {
            None
        };

        self.backfill_call(&call_id, status, tool_use_result);

        SessionBlock::ToolResult {
            call_id,
            status,
            preview: p.text,
            total_len: p.total_len,
            line_count: p.line_count,
            truncated: p.truncated,
            full,
            exit_code,
            duration_ms,
            images,
            saved_path,
        }
    }

    /// 结果到来时回填对应 ToolCall：Edit/Write 用 `structuredPatch` 修正 diff，
    /// 失败的改动去掉 diff；Agent 用实际模型补 detail。
    fn backfill_call(&mut self, call_id: &str, status: ToolStatus, result: Option<&ToolUseResult>) {
        let Some(slot) = self.calls.get(call_id) else {
            return;
        };
        let Some(SessionBlock::ToolCall {
            raw_name,
            detail,
            diff,
            ..
        }) = self
            .drafts
            .get_mut(slot.draft)
            .and_then(|d| d.blocks.get_mut(slot.block))
        else {
            return;
        };

        match raw_name.as_str() {
            "Edit" | "MultiEdit" | "Write" => {
                if status != ToolStatus::Success {
                    *diff = None;
                    return;
                }
                let Some(result) = result else { return };
                let op = match result.kind.as_ref().and_then(Value::as_str) {
                    Some("create") => Some(DiffOp::Add),
                    Some("update") => Some(DiffOp::Update),
                    _ => None,
                };
                let path = result
                    .file_path
                    .as_ref()
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let patch = result.structured_patch.filter(|p| p.added + p.removed > 0);
                if op.is_none() && patch.is_none() {
                    return;
                }
                let summary = diff.get_or_insert_with(|| DiffSummary {
                    files: vec![DiffFile {
                        path: String::new(),
                        op: DiffOp::Update,
                        added: 0,
                        removed: 0,
                    }],
                    added: 0,
                    removed: 0,
                    full: None,
                });
                if let Some(file) = summary.files.first_mut() {
                    if let Some(op) = op {
                        file.op = op;
                    }
                    if let Some(path) = path {
                        file.path = path;
                    }
                    if let Some(patch) = patch {
                        file.added = patch.added;
                        file.removed = patch.removed;
                    }
                    summary.added = file.added;
                    summary.removed = file.removed;
                }
            }
            "Agent" | "Task" if detail.is_none() => {
                *detail = result
                    .and_then(|r| r.resolved_model.as_ref())
                    .and_then(Value::as_str)
                    .filter(|m| !m.is_empty())
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    /// `persistedOutputPath` 在会话附属目录内时，转成相对路径（`ContentRef::Sidecar`）。
    fn sidecar_rel_path(&self, saved: &str) -> Option<String> {
        Path::new(saved)
            .strip_prefix(&self.sidecar_dir)
            .ok()
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .filter(|rel| !rel.is_empty())
    }
}

// ─── 块构造 ──────────────────────────────────────────────────────────────

const SYSTEM_REMINDER_OPEN: &str = "<system-reminder>";
const SYSTEM_REMINDER_CLOSE: &str = "</system-reminder>";

fn thinking_block(
    item: &RawItem<'_>,
    kind: &str,
    index: usize,
    duration_ms: Option<u64>,
    line: JsonlSpan,
) -> Option<SessionBlock> {
    let text = item.thinking.as_str().unwrap_or_default();
    let has_signature = item.signature.as_str().is_some_and(|s| !s.is_empty());
    let redacted = kind == "redacted_thinking" || (text.trim().is_empty() && has_signature);
    if text.trim().is_empty() && !redacted {
        return None;
    }
    let p = preview_chars(text, THINKING_PREVIEW_CHARS);
    Some(SessionBlock::Thinking {
        text: p.text,
        summary: None,
        redacted,
        duration_ms,
        full: p
            .truncated
            .then(|| line.content_ref(format!("/message/content/{index}/thinking"))),
    })
}

fn tool_call_block(item: RawItem<'_>, index: usize, line: JsonlSpan) -> SessionBlock {
    let id = item.id.as_str().unwrap_or_default().to_string();
    let raw_name = item.name.as_str().unwrap_or("unknown").to_string();
    let input = item.input.unwrap_or(Value::Null);
    let input_json = if input.is_null() {
        String::new()
    } else {
        serde_json::to_string(&input).unwrap_or_default()
    };
    let p = preview_chars(&input_json, INPUT_PREVIEW_CHARS);
    let normalized = normalize_tool(ToolSource::Claude, &raw_name, None);
    let (kind, title, detail) = claude_tool_title(&raw_name, &normalized, &input);
    let diff = input_diff(&raw_name, &input);

    SessionBlock::ToolCall {
        id,
        raw_name,
        kind,
        title,
        detail,
        server: normalized.server,
        input_preview: p.text,
        input_total_len: p.total_len,
        // 指向参数对象（不是字符串），取全文时由命令层序列化
        input_full: p
            .truncated
            .then(|| line.content_ref(format!("/message/content/{index}/input"))),
        diff,
        by_user: false,
    }
}

/// 按 §3.3 提炼 Claude 工具的 kind（Bash 细化为 search）、标题与次要信息。
fn claude_tool_title(
    raw_name: &str,
    normalized: &NormalizedTool,
    input: &Value,
) -> (ToolKind, String, Option<String>) {
    let s = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
    };
    let kind = normalized.kind;

    if kind == ToolKind::Mcp {
        let (server, tool) = split_mcp_name(raw_name)
            .unwrap_or((normalized.server.as_deref().unwrap_or_default(), raw_name));
        return (kind, title_mcp(server, tool), first_string_field(input));
    }

    match raw_name {
        "Bash" => {
            let command = s("command").unwrap_or_default();
            (
                refine_shell_kind(command),
                title_shell(command),
                s("description").map(one_line_title),
            )
        }
        "Read" => {
            let u = |key: &str| input.get(key).and_then(Value::as_u64);
            let (title, detail) =
                title_read(s("file_path").unwrap_or_default(), u("offset"), u("limit"));
            (kind, title, detail)
        }
        "Grep" => (
            kind,
            title_search(
                s("pattern").unwrap_or_default(),
                s("path").or_else(|| s("glob")),
            ),
            s("output_mode").map(str::to_string),
        ),
        "Glob" => (
            kind,
            title_search(s("pattern").unwrap_or_default(), s("path")),
            None,
        ),
        "ToolSearch" => (
            kind,
            title_search(s("query").unwrap_or_default(), None),
            None,
        ),
        "Edit" | "MultiEdit" | "Write" => {
            (kind, title_path(s("file_path").unwrap_or_default()), None)
        }
        "NotebookEdit" => (
            kind,
            title_path(s("notebook_path").unwrap_or_default()),
            None,
        ),
        "WebFetch" => (kind, title_web(&[s("url").unwrap_or_default()]), None),
        "WebSearch" => (kind, title_web(&[s("query").unwrap_or_default()]), None),
        "Agent" | "Task" => (
            kind,
            title_agent(s("description").or_else(|| s("prompt")).unwrap_or_default()),
            s("model").map(str::to_string),
        ),
        "AskUserQuestion" => {
            let question = input
                .get("questions")
                .and_then(Value::as_array)
                .and_then(|qs| qs.first())
                .and_then(|q| q.get("question"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            (kind, title_ask(question), None)
        }
        "TodoWrite" => {
            let count = input
                .get("todos")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            (kind, title_todo(count), None)
        }
        _ if kind == ToolKind::Agent => (
            kind,
            first_string_field(input).unwrap_or_else(|| title_other(raw_name)),
            None,
        ),
        _ => (kind, title_other(raw_name), first_string_field(input)),
    }
}

/// 从工具参数估算的 diff；结果记录到来后用 `structuredPatch` 修正（见 `backfill_call`）。
fn input_diff(raw_name: &str, input: &Value) -> Option<DiffSummary> {
    fn s<'v>(value: &'v Value, key: &str) -> &'v str {
        value.get(key).and_then(Value::as_str).unwrap_or_default()
    }
    let path = s(input, "file_path");
    let (op, added, removed) = match raw_name {
        "Edit" => {
            let (added, removed) =
                line_change_counts(s(input, "old_string"), s(input, "new_string"));
            (DiffOp::Update, added, removed)
        }
        "MultiEdit" => {
            let edits = input.get("edits").and_then(Value::as_array)?;
            edits
                .iter()
                .fold((DiffOp::Update, 0, 0), |(op, a, r), edit| {
                    let (added, removed) =
                        line_change_counts(s(edit, "old_string"), s(edit, "new_string"));
                    (op, a + added, r + removed)
                })
        }
        "Write" => {
            let lines = u32::try_from(s(input, "content").lines().count()).unwrap_or(u32::MAX);
            (DiffOp::Add, lines, 0)
        }
        _ => return None,
    };
    if path.is_empty() {
        return None;
    }
    Some(DiffSummary {
        files: vec![DiffFile {
            path: path.to_string(),
            op,
            added,
            removed,
        }],
        added,
        removed,
        full: None,
    })
}

fn image_ref(source: Option<&RawSource<'_>>, pointer: String, line: JsonlSpan) -> Option<ImageRef> {
    let source = source?;
    if source.kind.as_str() != Some("base64") {
        return None;
    }
    let data = source.data.as_str()?;
    Some(ImageRef {
        source: ImageSource::Inline {
            content: line.content_ref(pointer),
        },
        media_type: source
            .media_type
            .as_str()
            .unwrap_or("application/octet-stream")
            .to_string(),
        size: estimate_base64_size(data.len()),
        alt: None,
    })
}

/// 用户 `!cmd`：`<bash-input>cmd</bash-input><bash-stdout>…</bash-stdout><bash-stderr>…</bash-stderr>`
fn user_bash_blocks(text: &str, uuid: Option<&str>) -> Vec<SessionBlock> {
    let command = tag_content(text, "bash-input").unwrap_or_default();
    let id = format!("user-bash-{}", uuid.unwrap_or_default());
    let input = preview_chars(command, INPUT_PREVIEW_CHARS);
    let mut blocks = vec![SessionBlock::ToolCall {
        id: id.clone(),
        raw_name: "Bash".to_string(),
        kind: refine_shell_kind(command),
        title: title_shell(command),
        detail: None,
        server: None,
        input_preview: input.text,
        input_total_len: input.total_len,
        input_full: None,
        diff: None,
        by_user: true,
    }];

    let stdout = tag_content(text, "bash-stdout");
    let stderr = tag_content(text, "bash-stderr");
    if stdout.is_some() || stderr.is_some() {
        let stdout = stdout.unwrap_or_default().trim_end();
        let stderr = stderr.unwrap_or_default().trim_end();
        let output = match (stdout.is_empty(), stderr.is_empty()) {
            (_, true) => stdout.to_string(),
            (true, false) => stderr.to_string(),
            (false, false) => format!("{stdout}\n{stderr}"),
        };
        let p = preview(&output);
        blocks.push(SessionBlock::ToolResult {
            call_id: id,
            // 只有 stderr 不代表失败，状态未知
            status: if stderr.is_empty() {
                ToolStatus::Success
            } else {
                ToolStatus::Unknown
            },
            preview: p.text,
            total_len: p.total_len,
            line_count: p.line_count,
            truncated: p.truncated,
            full: None,
            exit_code: None,
            duration_ms: None,
            images: Vec::new(),
            saved_path: None,
        });
    }
    blocks
}

fn message_meta(
    model: &LStr<'_>,
    usage: Option<&Value>,
    stop_reason: &LStr<'_>,
) -> Option<MessageMeta> {
    // 0 视为缺省，省掉序列化体积
    let tokens = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
    };
    let meta = MessageMeta {
        model: model
            .as_str()
            .filter(|m| !m.is_empty() && *m != "<synthetic>")
            .map(str::to_string),
        input_tokens: tokens("input_tokens"),
        output_tokens: tokens("output_tokens"),
        cache_read_tokens: tokens("cache_read_input_tokens"),
        cache_write_tokens: tokens("cache_creation_input_tokens"),
        stop_reason: stop_reason.as_str().map(str::to_string),
        ..MessageMeta::default()
    };
    (meta != MessageMeta::default()).then_some(meta)
}

/// 同一回复拆成的多条记录：后到的非空字段覆盖先前的（usage 以最后一块为准）。
fn merge_meta(target: &mut Option<MessageMeta>, next: Option<MessageMeta>) {
    let Some(next) = next else { return };
    let Some(current) = target.as_mut() else {
        *target = Some(next);
        return;
    };
    macro_rules! take {
        ($($field:ident),*) => {$(
            if next.$field.is_some() {
                current.$field = next.$field;
            }
        )*};
    }
    take!(
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        stop_reason
    );
}

// ─── 文本规则 ────────────────────────────────────────────────────────────

/// 去掉所有 `<system-reminder>…</system-reminder>` 片段；没有闭合标签时去到末尾。
fn strip_system_reminders(text: &str) -> String {
    if !text.contains(SYSTEM_REMINDER_OPEN) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(SYSTEM_REMINDER_OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + SYSTEM_REMINDER_OPEN.len()..];
        rest = match after.find(SYSTEM_REMINDER_CLOSE) {
            Some(end) => &after[end + SYSTEM_REMINDER_CLOSE.len()..],
            None => "",
        };
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// 系统/工具注入、不是用户亲手输入的文本。
fn is_injected_text(text: &str) -> bool {
    text.contains("<local-command-caveat>")
        || text.starts_with("<local-command-stdout>")
        || text.starts_with("<local-command-stderr>")
        || text.starts_with("<task-notification>")
}

/// `<command-name>/compact</command-name><command-args>…</command-args>` → `/compact …`
fn slash_command(text: &str) -> Option<String> {
    if !text.starts_with("<command-") {
        return None;
    }
    let name = tag_content(text, "command-name")?.trim();
    if name.is_empty() {
        return None;
    }
    let args = tag_content(text, "command-args").unwrap_or_default().trim();
    Some(if args.is_empty() {
        name.to_string()
    } else {
        format!("{name} {args}")
    })
}

/// 取 `<tag>…</tag>` 之间的内容（第一处）。
fn tag_content<'t>(text: &'t str, tag: &str) -> Option<&'t str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close).map_or(text.len(), |i| start + i);
    Some(&text[start..end])
}

/// `Exit code N` 开头的工具输出（Claude Bash 失败时的格式）
fn parse_exit_code(text: &str) -> Option<i32> {
    let first = text.trim_start().lines().next()?;
    let rest = first.strip_prefix("Error: ").unwrap_or(first);
    rest.strip_prefix("Exit code ")?.trim().parse().ok()
}

fn content_text(content: &RawContent<'_>) -> String {
    match content {
        RawContent::Text(text) => text.to_string(),
        RawContent::Items(items) => items
            .iter()
            .filter_map(|item| item.0.as_ref())
            .filter(|item| item.kind.as_str() == Some("text"))
            .filter_map(|item| item.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        RawContent::None => String::new(),
    }
}

fn is_true(value: &Option<Value>) -> bool {
    value.as_ref().and_then(Value::as_bool) == Some(true)
}

fn as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64))
}

// ─── 原始记录（只声明用到的字段，其余由 serde 跳过）──────────────────────
//
// 所有字段都是「宽松」类型：类型不符时当作缺省而不是让整行解析失败；
// 字符串尽量借用行缓冲区，图片 base64 等大字段不复制。

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecord<'a> {
    #[serde(rename = "type", default, borrow)]
    kind: LStr<'a>,
    #[serde(default, borrow)]
    subtype: LStr<'a>,
    #[serde(default)]
    is_meta: Option<Value>,
    #[serde(default)]
    is_sidechain: Option<Value>,
    #[serde(default)]
    is_compact_summary: Option<Value>,
    #[serde(default, borrow)]
    uuid: LStr<'a>,
    #[serde(default)]
    timestamp: Option<Value>,
    #[serde(default, borrow)]
    message: Lenient<RawMessage<'a>>,
    #[serde(default)]
    tool_use_result: Lenient<ToolUseResult>,
    #[serde(default)]
    thinking_duration_ms: Option<Value>,
    #[serde(default)]
    hook_errors: Option<Value>,
    /// system 记录的正文（compact_boundary / local_command）
    #[serde(default, borrow)]
    content: LStr<'a>,
    #[serde(default, borrow)]
    pr_url: LStr<'a>,
    #[serde(default)]
    pr_number: Option<Value>,
}

#[derive(Deserialize)]
struct RawMessage<'a> {
    #[serde(default, borrow)]
    role: LStr<'a>,
    #[serde(default, borrow)]
    id: LStr<'a>,
    #[serde(default, borrow)]
    model: LStr<'a>,
    #[serde(default, borrow)]
    stop_reason: LStr<'a>,
    #[serde(default)]
    usage: Option<Value>,
    #[serde(default, borrow)]
    content: RawContent<'a>,
}

/// `message.content[]` 与 `tool_result.content[]` 的元素（字段取并集）
#[derive(Deserialize)]
struct RawItem<'a> {
    #[serde(rename = "type", default, borrow)]
    kind: LStr<'a>,
    #[serde(default, borrow)]
    text: LStr<'a>,
    #[serde(default, borrow)]
    thinking: LStr<'a>,
    #[serde(default, borrow)]
    signature: LStr<'a>,
    #[serde(default, borrow)]
    id: LStr<'a>,
    #[serde(default, borrow)]
    name: LStr<'a>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default, borrow)]
    tool_use_id: LStr<'a>,
    #[serde(default)]
    is_error: Option<Value>,
    #[serde(default, borrow)]
    content: RawContent<'a>,
    #[serde(default, borrow)]
    source: Lenient<RawSource<'a>>,
}

#[derive(Deserialize)]
struct RawSource<'a> {
    #[serde(rename = "type", default, borrow)]
    kind: LStr<'a>,
    #[serde(default, borrow)]
    media_type: LStr<'a>,
    #[serde(default, borrow)]
    data: LStr<'a>,
}

/// 顶层 `toolUseResult`：只取回填需要的字段（stdout、file、originalFile 等大字段跳过）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolUseResult {
    #[serde(default)]
    interrupted: Option<Value>,
    #[serde(default)]
    persisted_output_path: Option<Value>,
    #[serde(rename = "type", default)]
    kind: Option<Value>,
    #[serde(default)]
    file_path: Option<Value>,
    #[serde(default)]
    resolved_model: Option<Value>,
    #[serde(default)]
    background_task_id: Option<Value>,
    #[serde(default)]
    structured_patch: Option<PatchCounts>,
}

/// 宽松的借用字符串：非字符串值当作 `None`。
#[derive(Default)]
struct LStr<'a>(Option<Cow<'a, str>>);

impl LStr<'_> {
    fn as_str(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

/// 只接受对象的宽松包装：其他类型当作 `None`。
struct Lenient<T>(Option<T>);

impl<T> Default for Lenient<T> {
    fn default() -> Self {
        Self(None)
    }
}

/// 字符串或块数组（`message.content` / `tool_result.content`）
#[derive(Default)]
enum RawContent<'a> {
    #[default]
    None,
    Text(Cow<'a, str>),
    Items(Vec<Lenient<RawItem<'a>>>),
}

/// `structuredPatch` 里以 `+` / `-` 开头的行数
#[derive(Clone, Copy, Default)]
struct PatchCounts {
    added: u32,
    removed: u32,
}

/// 非预期类型的值整段跳过，返回 `$value`。
macro_rules! skip_other_values {
    ($value:expr) => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok($value)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok($value)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok($value)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok($value)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($value)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($value)
        }
    };
}

/// 字符串值整段跳过，返回 `$value`。
macro_rules! skip_str_values {
    ($value:expr) => {
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok($value)
        }
    };
}

/// 数组 / 对象整段跳过，返回 `$value`。
macro_rules! skip_seq_values {
    ($value:expr) => {
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok($value)
        }
    };
}

macro_rules! skip_map_values {
    ($value:expr) => {
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok($value)
        }
    };
}

impl<'de: 'a, 'a> Deserialize<'de> for LStr<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = LStr<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(LStr(Some(Cow::Borrowed(v))))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(LStr(Some(Cow::Owned(v.to_string()))))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(LStr(Some(Cow::Owned(v))))
            }
            skip_other_values!(LStr(None));
            skip_seq_values!(LStr(None));
            skip_map_values!(LStr(None));
        }
        deserializer.deserialize_any(V(PhantomData))
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = Lenient<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(de::value::MapAccessDeserializer::new(map)).map(|v| Lenient(Some(v)))
            }
            skip_other_values!(Lenient(None));
            skip_str_values!(Lenient(None));
            skip_seq_values!(Lenient(None));
        }
        deserializer.deserialize_any(V(PhantomData))
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for RawContent<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = RawContent<'a>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("string or array")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(RawContent::Text(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(RawContent::Text(Cow::Owned(v.to_string())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(RawContent::Text(Cow::Owned(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(item) = seq.next_element::<Lenient<RawItem<'a>>>()? {
                    items.push(item);
                }
                Ok(RawContent::Items(items))
            }
            skip_other_values!(RawContent::None);
            skip_map_values!(RawContent::None);
        }
        deserializer.deserialize_any(V(PhantomData))
    }
}

impl<'de> Deserialize<'de> for PatchCounts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// hunk 对象：只看 `lines`
        struct Hunk;
        impl<'de> Visitor<'de> for Hunk {
            type Value = PatchCounts;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("hunk")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut counts = PatchCounts::default();
                while let Some(key) = map.next_key::<LStr<'_>>()? {
                    if key.as_str() == Some("lines") {
                        counts = map.next_value_seed(Lines)?;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(counts)
            }
            skip_other_values!(PatchCounts::default());
            skip_str_values!(PatchCounts::default());
            skip_seq_values!(PatchCounts::default());
        }

        /// `lines` 数组：按首字符计数
        struct Lines;
        impl<'de> de::DeserializeSeed<'de> for Lines {
            type Value = PatchCounts;
            fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                d.deserialize_any(self)
            }
        }
        impl<'de> Visitor<'de> for Lines {
            type Value = PatchCounts;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("lines")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut counts = PatchCounts::default();
                while let Some(line) = seq.next_element::<LStr<'_>>()? {
                    match line.as_str().and_then(|l| l.chars().next()) {
                        Some('+') => counts.added += 1,
                        Some('-') => counts.removed += 1,
                        _ => {}
                    }
                }
                Ok(counts)
            }
            skip_other_values!(PatchCounts::default());
            skip_str_values!(PatchCounts::default());
            skip_map_values!(PatchCounts::default());
        }

        /// 外层 hunk 数组：累加
        struct Hunks;
        impl<'de> Visitor<'de> for Hunks {
            type Value = PatchCounts;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("structuredPatch")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut total = PatchCounts::default();
                while let Some(hunk) = seq.next_element_seed(HunkSeed)? {
                    total.added += hunk.added;
                    total.removed += hunk.removed;
                }
                Ok(total)
            }
            skip_other_values!(PatchCounts::default());
            skip_str_values!(PatchCounts::default());
            skip_map_values!(PatchCounts::default());
        }
        struct HunkSeed;
        impl<'de> de::DeserializeSeed<'de> for HunkSeed {
            type Value = PatchCounts;
            fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                d.deserialize_any(Hunk)
            }
        }

        deserializer.deserialize_any(Hunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn delete_session_removes_main_file_and_sidecar_directory() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("abc123-session.jsonl");
        let sidecar = temp.path().join("abc123-session");
        let subagents = sidecar.join("subagents");
        let tool_results = sidecar.join("tool-results");

        std::fs::create_dir_all(&subagents).expect("create subagents");
        std::fs::create_dir_all(&tool_results).expect("create tool-results");
        std::fs::write(subagents.join("agent-1.jsonl"), "{}").expect("write subagent");
        std::fs::write(tool_results.join("tool-1.txt"), "result").expect("write tool result");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-123\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n"
            ),
        )
        .expect("write session");

        delete_session(temp.path(), &path, "session-123").expect("delete session");

        assert!(!path.exists());
        assert!(!sidecar.exists());
    }

    #[test]
    fn load_messages_tool_use_shows_as_assistant() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Write\",\"input\":{\"file_path\":\"a.txt\"}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"File written\"}]},\"timestamp\":\"2026-03-06T10:00:01Z\"}\n",
            ),
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("[Tool: Write]"));
        assert_eq!(msgs[1].role, "tool");
        assert_eq!(msgs[1].content, "File written");
    }

    #[test]
    fn load_messages_mixed_text_and_tool_use() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Let me help.\"},{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Read\",\"input\":{}}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "assistant");
        assert!(msgs[0].content.contains("Let me help."));
        assert!(msgs[0].content.contains("[Tool: Read]"));
    }

    #[test]
    fn load_messages_mixed_user_tool_result_and_text_stays_user() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            "{\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"result\"},{\"type\":\"text\",\"text\":\"Please continue\"}]},\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert!(msgs[0].content.contains("Please continue"));
    }

    /// 审查 #7825 的复现：会话文件暂时读不了（权限），恢复后文件没变也要重新出现在列表里
    #[cfg(unix)]
    #[test]
    fn scan_recovers_session_after_read_permission_returns() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-perm.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-perm\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"},\"sessionId\":\"session-perm\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            ),
        )
        .expect("write");
        let cache = FileParseCache::new();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        if std::fs::read(&path).is_ok() {
            // 以 root 运行时权限不起作用，这个场景复现不了
            return;
        }
        assert!(cache.scan(vec![path.clone()], scan_session_file).is_empty());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let sessions = cache.scan(vec![path], scan_session_file);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-perm");
    }

    #[test]
    fn parse_session_uses_first_user_message_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-abc.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-abc\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"How do I deploy?\"},\"sessionId\":\"session-abc\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Here is how...\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("How do I deploy?"));
    }

    #[test]
    fn parse_session_custom_title_overrides_first_message() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-def.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-def\",\"cwd\":\"/tmp/project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"fix something\"},\"sessionId\":\"session-def\",\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Done.\"},\"timestamp\":\"2026-03-06T10:02:00Z\"}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\"fix-login-bug\",\"sessionId\":\"session-def\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("fix-login-bug"));
    }

    #[test]
    fn parse_session_falls_back_to_dir_basename() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-ghi.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"session-ghi\",\"cwd\":\"/tmp/my-project\",\"timestamp\":\"2026-03-06T10:00:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"timestamp\":\"2026-03-06T10:01:00Z\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        // No user message and no custom-title → falls back to dir basename
        assert_eq!(meta.title.as_deref(), Some("my-project"));
    }

    #[test]
    fn parse_session_truncates_long_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-trunc.jsonl");
        let long_msg = "a".repeat(200);
        std::fs::write(
            &path,
            format!(
                "{{\"sessionId\":\"session-trunc\",\"cwd\":\"/tmp/p\",\"timestamp\":\"2026-03-06T10:00:00Z\"}}\n\
                 {{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{long_msg}\"}},\"sessionId\":\"session-trunc\",\"timestamp\":\"2026-03-06T10:01:00Z\"}}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        let title = meta.title.unwrap();
        assert!(title.len() <= TITLE_MAX_CHARS + 3); // +3 for "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn parse_session_new_format_with_snapshot() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-new.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"file-history-snapshot\",\"messageId\":\"msg-1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"请帮我重构这个函数\"},\"sessionId\":\"session-new\",\"timestamp\":\"2026-03-06T10:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"OK\"},\"timestamp\":\"2026-03-06T10:01:00Z\",\"cwd\":\"/tmp/project\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("请帮我重构这个函数"));
    }

    #[test]
    fn parse_session_skips_command_caveat_and_slash_commands() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session-clear.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"file-history-snapshot\",\"messageId\":\"msg-1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands.</local-command-caveat>\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:00:00Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-name>/clear</command-name>\\n<command-message>clear</command-message>\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:00:01Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"Done.\"},\"timestamp\":\"2026-03-06T10:00:02Z\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"帮我看看工作区的改动\"},\"sessionId\":\"session-clear\",\"timestamp\":\"2026-03-06T10:01:00Z\",\"cwd\":\"/tmp/project\"}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("帮我看看工作区的改动"));
    }

    #[test]
    fn workflow_journal_log_is_excluded_from_sessions() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("journal.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"started\",\"key\":\"k\",\"agentId\":\"a\"}\n",
        )
        .expect("write");

        assert!(is_agent_session(&path));
    }

    #[test]
    fn agent_session_files_are_still_excluded() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("agent-abc123.jsonl");
        std::fs::write(&path, "{\"type\":\"user\",\"isSidechain\":true}\n").expect("write");

        assert!(is_agent_session(&path));
    }

    #[test]
    fn real_session_file_is_not_excluded() {
        let temp = tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("8f8e7c8e-0000-0000-0000-000000000000.jsonl");
        std::fs::write(
            &path,
            "{\"sessionId\":\"8f8e7c8e-0000-0000-0000-000000000000\"}\n",
        )
        .expect("write");

        assert!(!is_agent_session(&path));
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// 把记录写成 JSONL 并解析；返回消息与每行的字节偏移（用来校验 ContentRef）。
    fn parse_lines(lines: &[Value]) -> (tempfile::TempDir, PathBuf, Vec<SessionMessage>, Vec<u64>) {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        let mut body = String::new();
        let mut offsets = Vec::new();
        for line in lines {
            offsets.push(body.len() as u64);
            body.push_str(&serde_json::to_string(line).unwrap());
            body.push('\n');
        }
        std::fs::write(&path, body).expect("write");
        let messages = load_messages(&path).expect("load");
        (temp, path, messages, offsets)
    }

    fn user(uuid: &str, content: Value) -> Value {
        json!({
            "type": "user", "uuid": uuid, "timestamp": "2026-10-03T10:00:00Z",
            "message": { "role": "user", "content": content }
        })
    }

    fn assistant(uuid: &str, msg_id: &str, content: Value) -> Value {
        json!({
            "type": "assistant", "uuid": uuid, "timestamp": "2026-10-03T10:00:01Z",
            "message": { "id": msg_id, "role": "assistant", "model": "claude-opus-5-5", "content": content }
        })
    }

    fn block_types(message: &SessionMessage) -> Vec<String> {
        serde_json::to_value(&message.blocks)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn thinking_redacted_and_split_records_merge() {
        let (_t, _p, msgs, _) = parse_lines(&[
            user("u1", json!("hi")),
            {
                let mut v = assistant(
                    "a1",
                    "msg_1",
                    json!([{ "type": "thinking", "thinking": "", "signature": "EuYBCkQ" }]),
                );
                v["thinkingDurationMs"] = json!(3200);
                v
            },
            assistant("a2", "msg_1", json!([{ "type": "text", "text": "answer" }])),
            assistant(
                "a3",
                "msg_2",
                json!([{ "type": "thinking", "thinking": "x".repeat(500), "signature": "s" }]),
            ),
            // 既无正文又无签名的 thinking 直接丢弃
            assistant(
                "a4",
                "msg_3",
                json!([{ "type": "thinking", "thinking": "" }]),
            ),
        ]);
        assert_eq!(msgs.len(), 3, "同 message.id 的拆分记录合并为一条");
        assert_eq!(msgs[1].id.as_deref(), Some("a1"));
        assert_eq!(block_types(&msgs[1]), ["thinking", "text"]);
        assert_eq!(msgs[1].content, "answer", "thinking 不进入 content");
        match &msgs[1].blocks[0] {
            SessionBlock::Thinking {
                text,
                redacted,
                duration_ms,
                full,
                ..
            } => {
                assert!(text.is_empty());
                assert!(*redacted);
                assert_eq!(*duration_ms, Some(3200));
                assert!(full.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
        match &msgs[2].blocks[0] {
            SessionBlock::Thinking {
                text,
                redacted,
                full,
                ..
            } => {
                assert_eq!(text.chars().count(), THINKING_PREVIEW_CHARS);
                assert!(!*redacted);
                assert!(matches!(
                    full,
                    Some(ContentRef::Jsonl { pointer, .. }) if pointer == "/message/content/0/thinking"
                ));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tool_result_images_become_refs() {
        let data = "QUJD".repeat(1000);
        let (_t, path, msgs, offsets) = parse_lines(&[
            user("u1", json!("截个图")),
            assistant(
                "a1",
                "msg_1",
                json!([{ "type": "tool_use", "id": "toolu_1", "name": "mcp__claude-in-chrome__computer", "input": { "action": "screenshot", "tabId": 1 } }]),
            ),
            user(
                "u2",
                json!([{ "type": "tool_result", "tool_use_id": "toolu_1", "content": [
                    { "type": "text", "text": "Successfully captured screenshot" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": data } }
                ]}]),
            ),
            user(
                "u3",
                json!([
                    { "type": "text", "text": "看这张图" },
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": data } }
                ]),
            ),
        ]);
        let raw = std::fs::read(&path).unwrap();

        assert_eq!(
            msgs[1].content,
            "[Tool: mcp__claude-in-chrome__computer] claude-in-chrome.computer"
        );
        match &msgs[1].blocks[0] {
            SessionBlock::ToolCall {
                kind,
                server,
                detail,
                ..
            } => {
                assert_eq!(*kind, ToolKind::Mcp);
                assert_eq!(server.as_deref(), Some("claude-in-chrome"));
                assert_eq!(detail.as_deref(), Some("screenshot"));
            }
            other => panic!("unexpected {other:?}"),
        }

        assert_eq!(msgs[2].role, "tool");
        let SessionBlock::ToolResult {
            images, preview, ..
        } = &msgs[2].blocks[0]
        else {
            panic!("expected tool_result");
        };
        assert_eq!(preview, "Successfully captured screenshot");
        assert_eq!(images.len(), 1);
        let image = &images[0];
        assert_eq!(image.media_type, "image/jpeg");
        assert_eq!(image.size, 3000);
        let ImageSource::Inline {
            content:
                ContentRef::Jsonl {
                    offset,
                    len,
                    pointer,
                },
        } = &image.source
        else {
            panic!("expected inline jsonl ref");
        };
        assert_eq!(*offset, offsets[2]);
        assert_eq!(pointer, "/message/content/0/content/1/source/data");
        // 引用能回取：行区间可解析，pointer 指向 base64 原文
        let line: Value =
            serde_json::from_slice(&raw[*offset as usize..*offset as usize + *len as usize])
                .unwrap();
        assert_eq!(
            line.pointer(pointer).and_then(Value::as_str),
            Some(data.as_str())
        );
        assert!(
            !serde_json::to_string(&msgs).unwrap().contains(&data),
            "不内联 base64"
        );

        // 用户贴图
        assert_eq!(msgs[3].role, "user");
        assert_eq!(block_types(&msgs[3]), ["text", "image"]);
        assert_eq!(msgs[3].content, "看这张图\n\n[Image: image/png 3000]");
        assert_eq!(msgs[3].turn_id.as_deref(), Some("t2"));
    }

    #[test]
    fn edit_diff_is_backfilled_from_structured_patch() {
        let mut result = user(
            "u2",
            json!([{ "type": "tool_result", "tool_use_id": "toolu_e", "content": "The file a.rs has been updated successfully." }]),
        );
        result["toolUseResult"] = json!({
            "filePath": "/p/a.rs",
            "oldString": "x",
            "newString": "y",
            "structuredPatch": [
                { "oldStart": 1, "oldLines": 3, "newStart": 1, "newLines": 4, "lines": [" a", "-b", "+c", "+d", " e"] },
                { "oldStart": 9, "oldLines": 1, "newStart": 10, "newLines": 1, "lines": ["-f", "+g"] }
            ]
        });
        let mut create = user(
            "u4",
            json!([{ "type": "tool_result", "tool_use_id": "toolu_w", "content": "File created successfully at: /p/new.md" }]),
        );
        create["toolUseResult"] = json!({ "type": "create", "filePath": "/p/new.md", "content": "1\n2\n3\n", "structuredPatch": [] });
        let failed = user(
            "u6",
            json!([{ "type": "tool_result", "tool_use_id": "toolu_f", "is_error": true, "content": "String to replace not found" }]),
        );

        let (_t, _p, msgs, _) = parse_lines(&[
            user("u1", json!("改一下")),
            assistant(
                "a1",
                "m1",
                json!([{ "type": "tool_use", "id": "toolu_e", "name": "Edit", "input": { "file_path": "/p/a.rs", "old_string": "b", "new_string": "c\nd" } }]),
            ),
            result,
            assistant(
                "a2",
                "m2",
                json!([{ "type": "tool_use", "id": "toolu_w", "name": "Write", "input": { "file_path": "/p/new.md", "content": "1\n2\n3\n" } }]),
            ),
            create,
            assistant(
                "a3",
                "m3",
                json!([{ "type": "tool_use", "id": "toolu_f", "name": "Edit", "input": { "file_path": "/p/b.rs", "old_string": "q", "new_string": "r" } }]),
            ),
            failed,
        ]);

        let diff_of = |m: &SessionMessage| match &m.blocks[0] {
            SessionBlock::ToolCall { diff, .. } => diff.clone(),
            other => panic!("unexpected {other:?}"),
        };
        let edit = diff_of(&msgs[1]).expect("edit diff");
        assert_eq!(
            (edit.added, edit.removed),
            (3, 2),
            "以 structuredPatch 为准"
        );
        assert_eq!(edit.files[0].path, "/p/a.rs");
        assert_eq!(edit.files[0].op, DiffOp::Update);

        let write = diff_of(&msgs[3]).expect("write diff");
        assert_eq!(write.files[0].op, DiffOp::Add);
        assert_eq!((write.added, write.removed), (3, 0), "新建文件按内容行数");

        assert!(diff_of(&msgs[5]).is_none(), "失败的改动不显示 diff");
        let SessionBlock::ToolResult {
            status, exit_code, ..
        } = &msgs[6].blocks[0]
        else {
            panic!("expected tool_result");
        };
        assert_eq!(*status, ToolStatus::Error);
        assert_eq!(*exit_code, None);
    }

    #[test]
    fn system_reminders_are_stripped_or_marked_injected() {
        let (_t, _p, msgs, _) = parse_lines(&[
            user(
                "u1",
                json!("<system-reminder>\nThe user opened a.rs\n</system-reminder>\n\n修一下这个 bug"),
            ),
            user("u2", json!("<system-reminder>\nonly a reminder\n</system-reminder>")),
            user(
                "u3",
                json!("<task-notification>\n<task-id>t1</task-id>\n<status>completed</status>\n</task-notification>"),
            ),
            user(
                "u4",
                json!([
                    { "type": "tool_result", "tool_use_id": "toolu_x", "content": "ok\n<system-reminder>noise</system-reminder>" },
                    { "type": "text", "text": "<system-reminder>also noise</system-reminder>" }
                ]),
            ),
        ]);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].content, "修一下这个 bug");
        assert!(!msgs[0].injected);
        assert_eq!(msgs[0].turn_id.as_deref(), Some("t1"));

        assert!(msgs[1].injected);
        assert!(
            msgs[1].content.starts_with("<system-reminder>"),
            "整条注入时保留原文"
        );
        assert_eq!(msgs[1].turn_id.as_deref(), Some("t1"), "注入内容不开新轮");

        assert!(msgs[2].injected);
        assert_eq!(msgs[2].turn_id.as_deref(), Some("t1"));

        assert_eq!(msgs[3].role, "tool");
        assert_eq!(msgs[3].content, "ok");
        assert!(!msgs[3].injected);
    }

    #[test]
    fn slash_commands_interrupts_and_system_events() {
        let mut boundary = json!({ "type": "system", "subtype": "compact_boundary", "content": "Conversation compacted", "uuid": "s1", "timestamp": "2026-10-03T10:01:00Z" });
        boundary["compactMetadata"] = json!({ "trigger": "manual" });
        let (_t, _p, msgs, _) = parse_lines(&[
            user("u1", json!("第一问")),
            json!({ "type": "pr-link", "prNumber": 128, "prUrl": "https://github.com/o/r/pull/128", "timestamp": "2026-10-03T10:00:02Z" }),
            json!({ "type": "pr-link", "prNumber": 128, "prUrl": "https://github.com/o/r/pull/128", "timestamp": "2026-10-03T10:00:03Z" }),
            json!({ "type": "attachment", "attachment": { "type": "deferred_tools_delta" } }),
            json!({ "type": "user", "isMeta": true, "message": { "role": "user", "content": "<local-command-caveat>Caveat</local-command-caveat>" } }),
            user(
                "u2",
                json!("<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"),
            ),
            boundary,
            json!({ "type": "user", "isCompactSummary": true, "uuid": "u3", "message": { "role": "user", "content": "This session is being continued…" } }),
            user("u4", json!("<local-command-stdout>Compacted </local-command-stdout>")),
            user("u5", json!([{ "type": "text", "text": "[Request interrupted by user for tool use]" }])),
            json!({ "type": "system", "subtype": "stop_hook_summary", "hookErrors": [], "timestamp": "2026-10-03T10:02:00Z" }),
            json!({ "type": "system", "subtype": "stop_hook_summary", "hookErrors": ["notify.sh exited with code 127"], "timestamp": "2026-10-03T10:02:01Z" }),
            json!({ "type": "system", "subtype": "local_command", "uuid": "s2", "content": "<command-name>/status</command-name>\n<command-args>verbose</command-args>" }),
            user("u6", json!("<bash-input>git status</bash-input><bash-stdout>clean</bash-stdout><bash-stderr></bash-stderr>")),
        ]);

        let summary: Vec<(String, String, String, bool, String)> = msgs
            .iter()
            .map(|m| {
                (
                    m.role.clone(),
                    m.turn_id.clone().unwrap(),
                    block_types(m).join(","),
                    m.injected,
                    m.content.clone(),
                )
            })
            .collect();
        let row = |role: &str, turn: &str, types: &str, injected: bool, content: &str| {
            (
                role.to_string(),
                turn.to_string(),
                types.to_string(),
                injected,
                content.to_string(),
            )
        };
        assert_eq!(
            summary,
            vec![
                row("user", "t1", "text", false, "第一问"),
                row("system", "t1", "event", false, "PR #128"),
                row("user", "t2", "event", false, "/compact"),
                row(
                    "system",
                    "t2",
                    "event",
                    false,
                    "This session is being continued…"
                ),
                row(
                    "user",
                    "t2",
                    "text",
                    true,
                    "<local-command-stdout>Compacted </local-command-stdout>"
                ),
                row(
                    "user",
                    "t2",
                    "event",
                    false,
                    "[Request interrupted by user for tool use]"
                ),
                row(
                    "system",
                    "t2",
                    "event",
                    false,
                    "notify.sh exited with code 127"
                ),
                row("user", "t3", "event", false, "/status verbose"),
                row(
                    "user",
                    "t3",
                    "tool_call,tool_result",
                    false,
                    "[Tool: Bash] git status\n\nclean"
                ),
            ]
        );
        let kinds: Vec<EventKind> = msgs
            .iter()
            .filter_map(|m| match m.blocks.first() {
                Some(SessionBlock::Event { kind, .. }) => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                EventKind::PrLink,
                EventKind::SlashCommand,
                EventKind::Compaction,
                EventKind::Aborted,
                EventKind::Hook,
                EventKind::SlashCommand
            ]
        );
        assert!(matches!(
            &msgs[1].blocks[0],
            SessionBlock::Event { url: Some(url), .. } if url == "https://github.com/o/r/pull/128"
        ));
        assert!(matches!(
            &msgs[8].blocks[0],
            SessionBlock::ToolCall { by_user: true, .. }
        ));
    }

    #[test]
    fn tool_titles_project_into_content() {
        let (_t, _p, msgs, _) = parse_lines(&[assistant(
            "a1",
            "m1",
            json!([
                { "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "rg -n foo src\necho done", "description": "Search foo" } },
                { "type": "tool_use", "id": "t2", "name": "Read", "input": { "file_path": "/p/a.rs", "offset": 10, "limit": 31 } },
                { "type": "tool_use", "id": "t3", "name": "Grep", "input": { "pattern": "foo", "path": "src", "output_mode": "content" } },
                { "type": "tool_use", "id": "t4", "name": "WebSearch", "input": { "query": "tokio util" } },
                { "type": "tool_use", "id": "t5", "name": "TodoWrite", "input": { "todos": [{}, {}] } },
                { "type": "tool_use", "id": "t6", "name": "AskUserQuestion", "input": { "questions": [{ "question": "用哪个版本？" }] } },
                { "type": "tool_use", "id": "t7", "name": "Skill", "input": { "skill": "commit" } },
                { "type": "tool_use", "id": "t8", "name": "Agent", "input": { "description": "Review README", "model": "fable", "prompt": "x" } }
            ]),
        )]);
        assert_eq!(
            msgs[0].content,
            [
                "[Tool: Bash] rg -n foo src…",
                "[Tool: Read] /p/a.rs",
                "[Tool: Grep] foo in src",
                "[Tool: WebSearch] tokio util",
                "[Tool: TodoWrite] 2 项",
                "[Tool: AskUserQuestion] 用哪个版本？",
                "[Tool: Skill] Skill",
                "[Tool: Agent] Review README",
            ]
            .join("\n\n")
        );
        let details: Vec<(ToolKind, Option<String>)> = msgs[0]
            .blocks
            .iter()
            .map(|b| match b {
                SessionBlock::ToolCall { kind, detail, .. } => (*kind, detail.clone()),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            details,
            vec![
                (ToolKind::Search, Some("Search foo".to_string())),
                (ToolKind::Read, Some(":10-40".to_string())),
                (ToolKind::Search, Some("content".to_string())),
                (ToolKind::Web, None),
                (ToolKind::Todo, None),
                (ToolKind::Ask, None),
                (ToolKind::Other, Some("commit".to_string())),
                (ToolKind::Agent, Some("fable".to_string())),
            ]
        );
    }

    #[test]
    fn large_tool_results_only_carry_preview_and_refs() {
        let long: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        let mut interrupted = user(
            "u3",
            json!([{ "type": "tool_result", "tool_use_id": "toolu_b", "content": long }]),
        );
        let temp_marker = "__SIDECAR__";
        interrupted["toolUseResult"] = json!({
            "stdout": long, "stderr": "", "interrupted": true,
            "persistedOutputPath": format!("{temp_marker}/tool-results/toolu_b.txt")
        });
        let (temp, _p, msgs, offsets) = {
            // persistedOutputPath 需要真实的附属目录前缀，先算出路径再替换
            let temp = tempdir().expect("tempdir");
            let path = temp.path().join("session.jsonl");
            let sidecar = path.with_extension("");
            // 替换发生在序列化之后，路径要按 JSON 字符串转义（Windows 的反斜杠）
            let sidecar_json = serde_json::to_string(&sidecar.to_string_lossy()).unwrap();
            let sidecar_json = &sidecar_json[1..sidecar_json.len() - 1];
            let lines = [
                user("u1", json!("跑测试")),
                assistant(
                    "a1",
                    "m1",
                    json!([
                        { "type": "tool_use", "id": "toolu_a", "name": "Bash", "input": { "command": "cargo build" } },
                        { "type": "tool_use", "id": "toolu_b", "name": "Bash", "input": { "command": "cargo test", "description": "x".repeat(2000) } }
                    ]),
                ),
                {
                    let mut v = user(
                        "u2",
                        json!([{ "type": "tool_result", "tool_use_id": "toolu_a", "is_error": true, "content": format!("Exit code 101\n{long}") }]),
                    );
                    v["timestamp"] = json!("2026-10-03T10:00:13Z");
                    v
                },
                interrupted,
            ];
            let mut body = String::new();
            let mut offsets = Vec::new();
            for line in &lines {
                offsets.push(body.len() as u64);
                body.push_str(
                    &serde_json::to_string(line)
                        .unwrap()
                        .replace(temp_marker, sidecar_json),
                );
                body.push('\n');
            }
            std::fs::write(&path, body).unwrap();
            let msgs = load_messages(&path).unwrap();
            (temp, path, msgs, offsets)
        };
        let _ = temp;

        let SessionBlock::ToolCall {
            input_full,
            input_total_len,
            input_preview,
            ..
        } = &msgs[1].blocks[1]
        else {
            panic!("expected tool_call");
        };
        assert_eq!(input_preview.chars().count(), INPUT_PREVIEW_CHARS);
        assert!(*input_total_len as usize > INPUT_PREVIEW_CHARS);
        assert!(matches!(
            input_full,
            Some(ContentRef::Jsonl { offset, pointer, .. }) if *offset == offsets[1] && pointer == "/message/content/1/input"
        ));

        let SessionBlock::ToolResult {
            status,
            preview,
            truncated,
            line_count,
            full,
            exit_code,
            duration_ms,
            ..
        } = &msgs[2].blocks[0]
        else {
            panic!("expected tool_result");
        };
        assert_eq!(*status, ToolStatus::Error);
        assert_eq!(*exit_code, Some(101));
        assert_eq!(*duration_ms, Some(12_000));
        assert!(*truncated);
        assert_eq!(*line_count, 201);
        assert_eq!(preview.lines().count(), 12);
        assert!(matches!(
            full,
            Some(ContentRef::Jsonl { offset, pointer, .. }) if *offset == offsets[2] && pointer == "/message/content/0/content"
        ));

        let SessionBlock::ToolResult {
            status,
            full,
            saved_path,
            exit_code,
            ..
        } = &msgs[3].blocks[0]
        else {
            panic!("expected tool_result");
        };
        assert_eq!(*status, ToolStatus::Interrupted);
        assert_eq!(*exit_code, None);
        assert!(saved_path
            .as_deref()
            .unwrap()
            .ends_with("tool-results/toolu_b.txt"));
        assert_eq!(
            full,
            &Some(ContentRef::Sidecar {
                rel_path: "tool-results/toolu_b.txt".to_string()
            })
        );
    }

    /// 与契约 fixture `tests/fixtures/sessions/claude.messages.json` 同构的会话：
    /// 解析结果的「形状」（角色、轮次、注入标记、块类型/kind/status 与字段集合）必须一致。
    #[test]
    fn output_shape_matches_shared_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/sessions/claude.messages.json"
        ))
        .unwrap();

        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        let sidecar = path.with_extension("");
        let img = "iVBORw0KGgo".repeat(400);
        let long_err: String = std::iter::once("Exit code 101".to_string())
            .chain((1..=40).map(|i| format!("error line {i}")))
            .collect::<Vec<_>>()
            .join("\n");
        let long_read: String = (1..=40).map(|i| format!("{i:>6}\tline {i}\n")).collect();
        let long_test: String = (1..=231).map(|i| format!("test {i} ... ok\n")).collect();

        let mut ts = 0;
        let mut at = |mut v: Value| {
            ts += 1;
            v["timestamp"] = json!(format!("2026-10-03T10:{:02}:{:02}Z", ts / 60, ts % 60));
            v
        };
        let usage = json!({ "input_tokens": 3, "output_tokens": 214, "cache_read_input_tokens": 18432, "cache_creation_input_tokens": 2048 });
        let ai = |uuid: &str, id: &str, content: Value| {
            let mut v = assistant(uuid, id, content);
            v["message"]["usage"] = usage.clone();
            v["message"]["stop_reason"] = json!("tool_use");
            v
        };
        let result = |uuid: &str, call: &str, content: Value| {
            user(
                uuid,
                json!([{ "type": "tool_result", "tool_use_id": call, "content": content }]),
            )
        };
        let tool = |id: &str, name: &str, input: Value| json!({ "type": "tool_use", "id": id, "name": name, "input": input });

        let mut lines = vec![
            at(user("u01", json!([
                { "type": "text", "text": "cargo build 报错，帮我看看。" },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": img } }
            ]))),
            at(user("u02", json!("<system-reminder>\nThe user opened src/net/client.rs\n</system-reminder>"))),
            at({
                let mut v = ai("u03", "msg_a", json!([{ "type": "thinking", "thinking": "", "signature": "sig" }]));
                v["thinkingDurationMs"] = json!(3200);
                v
            }),
            at(ai("u03b", "msg_a", json!([{ "type": "text", "text": "我先跑一遍构建。" }]))),
            at(ai("u03c", "msg_a", json!([tool("toolu_01Bash", "Bash", json!({ "command": "cargo build 2>&1 | tail -50", "description": "Build the project" }))]))),
            at({
                let mut v = user("u04", json!([{ "type": "tool_result", "tool_use_id": "toolu_01Bash", "is_error": true, "content": long_err }]));
                v["toolUseResult"] = json!("Error: Exit code 101");
                v
            }),
            at({
                let mut v = ai("u05", "msg_b", json!([{ "type": "thinking", "thinking": "缺少 tokio-util 依赖。", "signature": "sig" }]));
                v["thinkingDurationMs"] = json!(1800);
                v
            }),
            at(ai("u05b", "msg_b", json!([
                tool("toolu_02Read", "Read", json!({ "file_path": "/p/Cargo.toml", "offset": 1, "limit": 40 })),
                tool("toolu_03Grep", "Grep", json!({ "pattern": "tokio_util", "path": "src", "output_mode": "content" }))
            ]))),
            at(result("u06", "toolu_02Read", json!(long_read))),
            at(result("u07", "toolu_03Grep", json!("src/net/client.rs:7:use tokio_util::codec::LinesCodec;"))),
            at(ai("u08", "msg_c", json!([
                { "type": "text", "text": "补上依赖。" },
                tool("toolu_04Edit", "Edit", json!({ "file_path": "/p/Cargo.toml", "old_string": "a", "new_string": "a\nb" })),
                tool("toolu_05Edit", "Edit", json!({ "file_path": "/p/src/net/client.rs", "old_string": "x", "new_string": "y\nz" }))
            ]))),
            at({
                let mut v = result("u09", "toolu_04Edit", json!("The file /p/Cargo.toml has been updated successfully."));
                v["toolUseResult"] = json!({ "filePath": "/p/Cargo.toml", "structuredPatch": [{ "lines": [" a", "+b"] }] });
                v
            }),
            at(result("u0a", "toolu_05Edit", json!("The file /p/src/net/client.rs has been updated successfully."))),
            at(ai("u0b", "msg_d", json!([tool("toolu_06Bash", "Bash", json!({ "command": "cargo build", "description": "Rebuild" }))]))),
            at({
                let mut v = result("u0c", "toolu_06Bash", json!("Finished `dev` profile"));
                v["toolUseResult"] = json!({ "stdout": "Finished `dev` profile", "stderr": "", "interrupted": false });
                v
            }),
            at(ai("u0d", "msg_e", json!([tool("toolu_07Mcp", "mcp__claude-in-chrome__computer", json!({ "action": "screenshot", "tabId": 1820356711 }))]))),
            at(result("u0e", "toolu_07Mcp", json!([
                { "type": "text", "text": "Successfully captured screenshot" },
                { "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": img } }
            ]))),
            at(ai("u0f", "msg_f", json!([{ "type": "text", "text": "构建已经通过。" }]))),
            at(json!({ "type": "pr-link", "prNumber": 128, "prUrl": "https://github.com/o/r/pull/128" })),
            at(user("u10", json!("<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"))),
            at(json!({ "type": "system", "subtype": "compact_boundary", "content": "Conversation compacted", "uuid": "s1" })),
            at(json!({ "type": "user", "isCompactSummary": true, "uuid": "u10b", "message": { "role": "user", "content": "Conversation compacted: fixed tokio-util." } })),
            at(user("u11", json!("再把全部测试跑一遍。"))),
            at(ai("u12", "msg_g", json!([
                tool("toolu_08Todo", "TodoWrite", json!({ "todos": [{ "content": "a" }, { "content": "b" }] })),
                tool("toolu_09Agent", "Agent", json!({ "description": "Review README install steps", "subagent_type": "Explore", "model": "fable", "prompt": "Check README." })),
                tool("toolu_10Bash", "Bash", json!({ "command": "cargo test --workspace", "description": "Run all tests", "timeout": 600000 }))
            ]))),
            at(result("u13", "toolu_08Todo", json!("Todos have been modified successfully."))),
            at({
                let mut v = result("u14", "toolu_10Bash", json!(long_test));
                v["toolUseResult"] = json!({
                    "interrupted": true,
                    "persistedOutputPath": format!("{}/tool-results/toolu_10Bash.txt", sidecar.display())
                });
                v
            }),
            at(user("u15", json!([{ "type": "text", "text": "[Request interrupted by user for tool use]" }]))),
            at(json!({ "type": "system", "subtype": "stop_hook_summary", "hookErrors": ["Stop hook failed: notify.sh exited with code 127"] })),
            at(user("u16", json!("继续，把 CHANGELOG 也补一条。"))),
            at(ai("u17", "msg_h", json!([tool("toolu_11Ask", "AskUserQuestion", json!({ "questions": [{ "question": "CHANGELOG 用哪个版本号？" }] }))]))),
            at(result("u18", "toolu_11Ask", json!("User has answered your questions."))),
            at(ai("u19", "msg_i", json!([
                tool("toolu_12Web", "WebFetch", json!({ "url": "https://keepachangelog.com/en/1.1.0/", "prompt": "Summarize" })),
                tool("toolu_13Write", "Write", json!({ "file_path": "/p/CHANGELOG.md", "content": "# Changelog\n\n## [0.3.2]\n" }))
            ]))),
            at(result("u1a", "toolu_12Web", json!("Added, Changed, Fixed."))),
            at({
                let mut v = result("u1b", "toolu_13Write", json!("File created successfully at: /p/CHANGELOG.md"));
                v["toolUseResult"] = json!({ "type": "create", "filePath": "/p/CHANGELOG.md", "structuredPatch": [] });
                v
            }),
            at(ai("u1c", "msg_j", json!([{ "type": "text", "text": "已新增 0.3.2 条目。" }]))),
        ];
        // 噪声记录：应被跳过
        lines.insert(
            1,
            json!({ "type": "attachment", "attachment": { "type": "deferred_tools_delta" } }),
        );
        lines.push(json!({ "type": "custom-title", "customTitle": "x" }));
        lines.push(json!({ "type": "last-prompt", "lastPrompt": "x" }));

        let body: String = lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap() + "\n")
            .collect();
        std::fs::write(&path, body).unwrap();
        let parsed = serde_json::to_value(load_messages(&path).unwrap()).unwrap();

        /// 形状：角色、轮次、注入标记、有无 id/meta，以及每个块的类型、判别值和字段集合
        fn shape(messages: &Value) -> Vec<Value> {
            messages
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    let blocks: Vec<Value> = m["blocks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|b| {
                            let mut keys: Vec<&str> =
                                b.as_object().unwrap().keys().map(String::as_str).collect();
                            keys.sort_unstable();
                            json!({
                                "type": b["type"], "kind": b["kind"], "status": b["status"],
                                "rawName": b["rawName"], "redacted": b["redacted"], "keys": keys,
                                "full": b["full"]["kind"], "imageKinds": b["images"].as_array().map(|i| i.len()),
                                "diffOp": b["diff"]["files"][0]["op"],
                            })
                        })
                        .collect();
                    json!({
                        "role": m["role"], "turnId": m["turnId"], "injected": m.get("injected"),
                        "hasId": m.get("id").is_some(), "hasMeta": m.get("meta").is_some(),
                        "blocks": blocks,
                    })
                })
                .collect()
        }

        let expected = shape(&fixture);
        let actual = shape(&parsed);
        assert_eq!(actual.len(), expected.len(), "消息条数");
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "第 {i} 条消息形状不一致");
        }

        // 回填与配对
        let messages: Vec<SessionMessage> = serde_json::from_value(parsed).unwrap();
        let call_ids: HashSet<&str> = messages
            .iter()
            .flat_map(|m| &m.blocks)
            .filter_map(|b| match b {
                SessionBlock::ToolCall { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        for block in messages.iter().flat_map(|m| &m.blocks) {
            if let SessionBlock::ToolResult { call_id, .. } = block {
                assert!(call_ids.contains(call_id.as_str()), "{call_id} 未配对");
            }
        }
        // 有 blocks 的消息不下发 content（前端从 blocks 推导）
        assert!(messages.iter().all(|m| m.content.is_empty()));
    }
}
