use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::codex_config::{get_codex_config_dir, read_codex_config_text};
use crate::codex_state_db::codex_state_db_paths;
use crate::session_manager::model::{
    project_content, ContentRef, EventKind, ImageRef, MessageMeta, SessionBlock, ToolKind,
    ToolStatus,
};
use crate::session_manager::{SessionMessage, SessionMeta};

use super::blocks::{
    first_string_field, large_text_block, normalize_tool, preview, preview_chars,
    refine_shell_kind, summary_event_block, title_agent, title_ask, title_mcp, title_other,
    title_path, title_read, title_shell, title_todo, title_web, ToolSource, THINKING_PREVIEW_CHARS,
};
use super::codex_items::{
    command_from_value, diff_kind, diff_summary, diff_title, exec_title, image_from_url,
    local_image, opt_str, parse_apply_patch, parse_exec_script, parse_legacy_shell_output,
    same_command, status_from_str, strip_output_header, web_action_title, CallSpec, CommandInfo,
    Contents, FileChangeInfo, ItemRecord, RawItem, ResultSpec, Str,
};

use super::utils::{
    extract_text, parse_timestamp_to_ms, path_basename, read_head_tail_lines, truncate_summary,
    FileParseCache, JsonlSpan, LineSpans, TITLE_MAX_CHARS,
};

const PROVIDER_ID: &str = "codex";
const CODEX_SESSION_INDEX_FILENAME: &str = "session_index.jsonl";
const VSCODE_CONTEXT_PREFIX: &str = "# Context from my IDE setup:";
const CODEX_REQUEST_MARKER: &str = "my request for codex";

static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .unwrap()
});

#[derive(Deserialize)]
struct SessionIndexEntry {
    id: String,
    thread_name: String,
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let roots = session_roots();
    scan_sessions_in_roots(&roots)
}

pub fn session_roots() -> Vec<PathBuf> {
    let config_dir = get_codex_config_dir();
    vec![
        config_dir.join("sessions"),
        config_dir.join("archived_sessions"),
    ]
}

fn scan_sessions_in_roots(roots: &[PathBuf]) -> Vec<SessionMeta> {
    let thread_titles = load_thread_titles();
    scan_sessions_in_roots_with_titles(roots, &thread_titles)
}

fn scan_sessions_in_roots_with_titles(
    roots: &[PathBuf],
    thread_titles: &HashMap<String, String>,
) -> Vec<SessionMeta> {
    let mut files = Vec::new();
    for root in roots {
        collect_jsonl_files(root, &mut files);
    }

    // 缓存里只放文件本身解析出的结果；线程标题来自外部索引 / 数据库，
    // 每轮重新读取后再覆盖上去，和逐个调用 parse_session_with_titles 等价。
    let mut sessions = PARSE_CACHE.scan(files, scan_session_file);
    for meta in &mut sessions {
        if let Some(title) = thread_titles.get(&meta.session_id) {
            meta.title = Some(truncate_summary(title, TITLE_MAX_CHARS));
        }
    }
    sessions
}

/// 会话页每次打开都会全量扫描；没变过的文件直接复用上次的解析结果。
static PARSE_CACHE: LazyLock<FileParseCache> = LazyLock::new(FileParseCache::new);

fn load_thread_titles() -> HashMap<String, String> {
    let config_dir = get_codex_config_dir();
    let config_text = read_codex_config_text().unwrap_or_default();
    let db_paths = codex_state_db_paths(&config_dir, &config_text);
    load_thread_titles_from_paths(&config_dir.join(CODEX_SESSION_INDEX_FILENAME), &db_paths)
}

fn load_thread_titles_from_paths(
    session_index_path: &Path,
    db_paths: &[PathBuf],
) -> HashMap<String, String> {
    let mut titles = load_thread_titles_from_session_index(session_index_path);
    for db_path in db_paths {
        titles.extend(load_thread_titles_from_db(db_path));
    }
    titles
}

fn load_thread_titles_from_session_index(index_path: &Path) -> HashMap<String, String> {
    if !index_path.exists() {
        return HashMap::new();
    }

    let file = match File::open(index_path) {
        Ok(file) => file,
        Err(err) => {
            log::warn!(
                "Failed to open Codex session index {}: {err}",
                index_path.display()
            );
            return HashMap::new();
        }
    };

    let reader = BufReader::new(file);
    let mut titles = HashMap::new();
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };
        let Ok(entry) = serde_json::from_str::<SessionIndexEntry>(line.trim()) else {
            continue;
        };
        let id = entry.id.trim();
        let title = entry.thread_name.trim();
        if !id.is_empty() && !title.is_empty() {
            titles.insert(id.to_string(), title.to_string());
        }
    }

    titles
}

fn load_thread_titles_from_db(db_path: &Path) -> HashMap<String, String> {
    if !db_path.exists() {
        return HashMap::new();
    }

    let conn = match Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(err) => {
            log::warn!(
                "Failed to open Codex state database {}: {err}",
                db_path.display()
            );
            return HashMap::new();
        }
    };
    // Codex keeps this DB open and write-locked while running; without a busy
    // timeout a read during a write fails immediately and titles silently drop.
    if let Err(err) = conn.busy_timeout(Duration::from_secs(2)) {
        log::warn!(
            "Failed to set Codex state database busy timeout for {}: {err}",
            db_path.display()
        );
        return HashMap::new();
    }

    // Mirror Codex's own `distinct_thread_metadata_title`: keep a title only
    // when it differs from the first user message. Push the comparison into SQL
    // (NULL-safe) so we never SELECT the unbounded `first_user_message` blob —
    // it can grow large enough to OOM (openai/codex#29007).
    let mut stmt = match conn.prepare(
        "SELECT id, title FROM threads \
         WHERE title <> '' \
         AND (first_user_message IS NULL OR TRIM(title) <> TRIM(first_user_message))",
    ) {
        Ok(stmt) => stmt,
        Err(err) => {
            log::warn!(
                "Failed to prepare Codex thread title query for {}: {err}",
                db_path.display()
            );
            return HashMap::new();
        }
    };

    let rows = match stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        Ok((id, title))
    }) {
        Ok(rows) => rows,
        Err(err) => {
            log::warn!(
                "Failed to query Codex thread titles from {}: {err}",
                db_path.display()
            );
            return HashMap::new();
        }
    };

    rows.flatten()
        .filter_map(|(id, title)| {
            let id = id.trim();
            let title = title.trim();
            if id.is_empty() || title.is_empty() {
                None
            } else {
                Some((id.to_string(), title.to_string()))
            }
        })
        .collect()
}

// ─── 会话内容解析（设计 §4.2）────────────────────────────────────────────
//
// 一遍顺序读取 rollout，按「调用 → item_completed → 输出」维护状态机：
// 工具调用先落成 ToolCall 块并记下位置，之后到达的结构化项（CommandExecution、
// FileChange、McpToolCall…）挂在该调用上，输出到达时回填标题 / kind / diff，
// 再生成 ToolResult。大内容只给预览 + ContentRef（行字节区间 + JSON Pointer）。

/// 嗅探行首类型时最多看的字节数
const SNIFF_BYTES: usize = 512;
/// 注入型用户文本的前缀（AGENTS.md、环境上下文等，不是用户的提问）
const INJECTED_USER_PREFIXES: [&str; 4] = [
    "# AGENTS.md instructions",
    "<environment_context>",
    "<user_instructions>",
    "<turn_aborted>",
];

pub fn load_messages(path: &Path) -> Result<Vec<SessionMessage>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open session file: {e}"))?;
    let mut lines = LineSpans::new(BufReader::with_capacity(1 << 20, file));
    let mut parser = RolloutParser::default();
    while let Some(line) = lines
        .next_line()
        .map_err(|e| format!("Failed to read session file: {e}"))?
    {
        let mut bytes = line.bytes;
        while let [rest @ .., b'\n' | b'\r'] = bytes {
            bytes = rest;
        }
        if !bytes.is_empty() {
            parser.process_line(bytes, line.span);
        }
    }
    Ok(parser.finish())
}

// ─── 行结构（只声明用得到的字段，其余跳过不分配）──────────────────────────

#[derive(Deserialize)]
struct Line<'a, P> {
    #[serde(default, borrow)]
    timestamp: Option<Str<'a>>,
    payload: P,
}

#[derive(Deserialize)]
struct Head<'a> {
    #[serde(rename = "type", default, borrow)]
    ty: Option<Str<'a>>,
    #[serde(default, borrow)]
    payload: Option<PayloadHead<'a>>,
}

#[derive(Deserialize)]
struct PayloadHead<'a> {
    #[serde(rename = "type", default, borrow)]
    ty: Option<Str<'a>>,
}

#[derive(Deserialize)]
struct MessagePayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    role: Option<Str<'a>>,
    #[serde(default, borrow)]
    content: Option<Contents<'a>>,
    #[serde(default, borrow)]
    phase: Option<Str<'a>>,
}

#[derive(Deserialize)]
struct ReasoningPayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    summary: Option<Contents<'a>>,
    #[serde(default, borrow)]
    content: Option<Contents<'a>>,
}

#[derive(Deserialize)]
struct FunctionCallPayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    name: Option<Str<'a>>,
    #[serde(default, borrow)]
    namespace: Option<Str<'a>>,
    #[serde(default, borrow)]
    arguments: Option<Str<'a>>,
    #[serde(default, borrow)]
    call_id: Option<Str<'a>>,
}

#[derive(Deserialize)]
struct CustomCallPayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    name: Option<Str<'a>>,
    #[serde(default, borrow)]
    input: Option<Str<'a>>,
    #[serde(default, borrow)]
    call_id: Option<Str<'a>>,
}

#[derive(Deserialize)]
struct OutputPayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    call_id: Option<Str<'a>>,
    #[serde(default, borrow)]
    output: Option<Contents<'a>>,
}

#[derive(Deserialize)]
struct ActionPayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    call_id: Option<Str<'a>>,
    #[serde(default, borrow)]
    status: Option<Str<'a>>,
    #[serde(default)]
    action: Option<Value>,
}

#[derive(Deserialize)]
struct AgentMessagePayload<'a> {
    #[serde(default, borrow)]
    id: Option<Str<'a>>,
    #[serde(default, borrow)]
    author: Option<Str<'a>>,
    #[serde(default, borrow)]
    content: Option<Contents<'a>>,
}

#[derive(Deserialize)]
struct TurnContextPayload<'a> {
    #[serde(default, borrow)]
    turn_id: Option<Str<'a>>,
    #[serde(default, borrow)]
    model: Option<Str<'a>>,
}

#[derive(Deserialize, Default, Clone, Copy)]
struct TokenUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    reasoning_output_tokens: u64,
}

impl TokenUsage {
    fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_write_input_tokens += other.cache_write_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_output_tokens += other.reasoning_output_tokens;
    }
}

#[derive(Deserialize)]
struct UsageRecordPayload<'a> {
    #[serde(default, borrow)]
    turn_id: Option<Str<'a>>,
    #[serde(default)]
    turn_token_usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct CompactedPayload<'a> {
    #[serde(default, borrow)]
    message: Option<Str<'a>>,
}

#[derive(Deserialize)]
struct ThreadSettings<'a> {
    #[serde(default, borrow)]
    model: Option<Str<'a>>,
}

#[derive(Deserialize)]
struct TokenInfo {
    #[serde(default)]
    last_token_usage: Option<TokenUsage>,
}

/// item_completed 以外的 event_msg（task_started / task_complete / turn_aborted /
/// thread_settings_applied / token_count）
#[derive(Deserialize)]
struct EventPayload<'a> {
    #[serde(default, borrow)]
    turn_id: Option<Str<'a>>,
    #[serde(default)]
    reason: Option<Value>,
    #[serde(default, borrow)]
    thread_settings: Option<ThreadSettings<'a>>,
    #[serde(default)]
    info: Option<TokenInfo>,
}

#[derive(Deserialize)]
struct ItemCompletedPayload<'a> {
    #[serde(borrow)]
    item: RawItem<'a>,
}

fn owned(value: &Option<Str<'_>>) -> Option<String> {
    opt_str(value).map(str::to_string)
}

fn parse_ts(raw: Option<&Str<'_>>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(raw?.as_str())
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// 解析整行，只取 `timestamp` 与指定形状的 `payload`
fn parse_line<'a, P: Deserialize<'a>>(line: &'a [u8]) -> Option<(Option<i64>, P)> {
    let parsed: Line<'a, P> = serde_json::from_slice(line).ok()?;
    Some((parse_ts(parsed.timestamp.as_ref()), parsed.payload))
}

/// 从行首嗅探 `(顶层 type, payload.type)`，避免为分派把大行完整解析两遍。
/// Codex 写出的行形如 `{"timestamp":…,"ordinal":N,"type":"response_item","payload":{"type":"…",…`；
/// 键序不符时返回 `None`，由调用方退回完整解析。
fn sniff_types(line: &[u8]) -> Option<(&str, Option<&str>)> {
    let head = &line[..line.len().min(SNIFF_BYTES)];
    let head = match std::str::from_utf8(head) {
        Ok(head) => head,
        Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).ok()?,
    };
    let payload_at = head.find("\"payload\":")?;
    let top = quoted_value_after(&head[..payload_at], "\"type\":\"")?;
    let sub = head[payload_at + "\"payload\":".len()..]
        .trim_start()
        .strip_prefix('{')
        .map(str::trim_start)
        .and_then(|p| p.strip_prefix("\"type\":\""))
        .and_then(|p| p.split_once('"'))
        .map(|(ty, _)| ty);
    Some((top, sub))
}

fn quoted_value_after<'s>(haystack: &'s str, key: &str) -> Option<&'s str> {
    let start = haystack.find(key)? + key.len();
    haystack[start..].split_once('"').map(|(value, _)| value)
}

// ─── 状态机 ──────────────────────────────────────────────────────────────

/// 工具调用的来源形态，决定哪些结构化项能挂上来
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallFlavor {
    /// custom_tool_call(exec)：一段 JS，可能调用多条命令 / MCP / apply_patch
    Exec,
    /// function_call(exec_command/shell)、local_shell_call
    Shell,
    ApplyPatch,
    Mcp,
    Other,
}

struct PendingCall {
    msg_idx: usize,
    block_idx: usize,
    flavor: CallFlavor,
    /// 调用参数里能看到的命令，用来认领 CommandExecution
    cmds: Vec<String>,
    items: Vec<ItemRecord>,
}

/// 工具输出（去掉输出头后的正文 + 头里的结构化信息）
struct OutputInfo {
    text: String,
    full: Option<ContentRef>,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    failed: bool,
    images: Vec<ImageRef>,
}

impl OutputInfo {
    fn from_contents(contents: Option<&Contents<'_>>, span: JsonlSpan) -> Self {
        let mut info = OutputInfo {
            text: String::new(),
            full: None,
            exit_code: None,
            duration_ms: None,
            failed: false,
            images: Vec::new(),
        };
        match contents {
            None => {}
            Some(Contents::Text(text)) => {
                let text = text.as_str();
                if let Some((body, exit_code, duration_ms)) = parse_legacy_shell_output(text) {
                    info.text = body;
                    info.exit_code = exit_code;
                    info.duration_ms = duration_ms;
                } else {
                    let header = strip_output_header(text);
                    info.text = header.body.to_string();
                    info.exit_code = header.exit_code;
                    info.duration_ms = header.duration_ms;
                    info.failed = header.failed;
                }
                info.full = Some(span.content_ref("/payload/output"));
            }
            Some(Contents::Items(items)) => {
                let mut pieces: Vec<(usize, &str)> = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    match item.kind() {
                        "input_image" | "image" => {
                            if let Some(url) = opt_str(&item.image_url) {
                                let pointer = format!("/payload/output/{i}/image_url");
                                info.images.extend(image_from_url(url, span, pointer));
                            }
                        }
                        _ => {
                            let Some(text) = opt_str(&item.text) else {
                                continue;
                            };
                            let header = strip_output_header(text);
                            info.exit_code = info.exit_code.or(header.exit_code);
                            info.duration_ms = info.duration_ms.or(header.duration_ms);
                            info.failed |= header.failed;
                            if !header.body.is_empty() {
                                pieces.push((i, header.body));
                            }
                        }
                    }
                }
                info.full = Some(span.content_ref(match pieces.as_slice() {
                    [(i, _)] => format!("/payload/output/{i}/text"),
                    _ => "/payload/output".to_string(),
                }));
                info.text = pieces.iter().map(|(_, t)| *t).collect();
            }
        }
        info
    }
}

#[derive(Default)]
struct RolloutParser {
    messages: Vec<SessionMessage>,
    turn_id: Option<String>,
    /// 最近一次看到的模型（turn_context / thread_settings_applied）
    last_model: Option<String>,
    /// 模型变了，等下一轮开始时输出 ModelChange 事件
    pending_model_event: Option<(String, Option<i64>)>,
    turn_models: HashMap<String, String>,
    turn_usage: HashMap<String, TokenUsage>,
    usage_from_records: bool,
    /// AgentMessage.phase（commentary / final_answer），按消息 id
    phases: HashMap<String, String>,
    calls: HashMap<String, PendingCall>,
    /// 已经输出结果的调用 id。Codex 0.119–0.128 的写入顺序是「调用 → 输出 →
    /// item_completed」，迟到的 item_completed 按 id 命中这里时只回填退出码 / 耗时，
    /// 不再单独成一个步骤（否则同一条命令会显示两次）
    finished: HashSet<String>,
    /// 还没等到输出的调用（按出现顺序）
    open: Vec<String>,
    /// 并行 exec 时归属不明的 CommandExecution，等输出到达时按内容认领
    pool: Vec<CommandInfo>,
    /// 紧跟的 web_search_call 没有 id 时借用 WebSearch 项的 id
    last_web_search_id: Option<String>,
    /// 刚输出的 compaction 事件（compacted / compaction / ContextCompaction 去重）
    last_compaction: Option<usize>,
}

impl RolloutParser {
    fn process_line(&mut self, line: &[u8], span: JsonlSpan) {
        let (top, sub): (Cow<str>, Option<Cow<str>>) = match sniff_types(line) {
            Some((top, Some(sub))) => (Cow::Borrowed(top), Some(Cow::Borrowed(sub))),
            Some((top, None)) if !matches!(top, "response_item" | "event_msg") => {
                (Cow::Borrowed(top), None)
            }
            _ => {
                let Ok(head) = serde_json::from_slice::<Head>(line) else {
                    return;
                };
                let Some(top) = head.ty else {
                    return;
                };
                (top.0, head.payload.and_then(|p| p.ty).map(|t| t.0))
            }
        };
        let sub = sub.as_deref().unwrap_or_default();
        match top.as_ref() {
            "response_item" => self.response_item(sub, line, span),
            "event_msg" => self.event_msg(sub, line, span),
            "turn_context" => {
                if let Some((_, p)) = parse_line::<TurnContextPayload>(line) {
                    if let Some(turn) = owned(&p.turn_id) {
                        self.start_turn(turn);
                    }
                    if let Some(model) = owned(&p.model) {
                        if let Some(turn) = &self.turn_id {
                            self.turn_models.insert(turn.clone(), model.clone());
                        }
                        self.last_model = Some(model);
                    }
                }
            }
            "token_usage_record" => {
                if let Some((_, p)) = parse_line::<UsageRecordPayload>(line) {
                    self.usage_from_records = true;
                    let turn = owned(&p.turn_id).or_else(|| self.turn_id.clone());
                    if let (Some(turn), Some(usage)) = (turn, p.turn_token_usage) {
                        self.turn_usage.insert(turn, usage);
                    }
                }
            }
            "compacted" => {
                if let Some((ts, p)) = parse_line::<CompactedPayload>(line) {
                    let summary = opt_str(&p.message)
                        .filter(|m| !m.trim().is_empty())
                        .map(|m| {
                            summary_event_block(EventKind::Compaction, m, || {
                                Some(span.content_ref("/payload/message"))
                            })
                        });
                    self.compaction_event(ts, summary);
                }
            }
            // session_meta / world_state / inter_agent_communication_metadata …：不产生消息
            _ => {}
        }
    }

    fn response_item(&mut self, sub: &str, line: &[u8], span: JsonlSpan) {
        match sub {
            "message" => {
                if let Some((ts, p)) = parse_line::<MessagePayload>(line) {
                    self.on_message(p, ts, span);
                }
            }
            "reasoning" => {
                if let Some((ts, p)) = parse_line::<ReasoningPayload>(line) {
                    self.on_reasoning(p, ts, span);
                }
            }
            "function_call" => {
                if let Some((ts, p)) = parse_line::<FunctionCallPayload>(line) {
                    self.on_function_call(p, ts, span);
                }
            }
            "custom_tool_call" => {
                if let Some((ts, p)) = parse_line::<CustomCallPayload>(line) {
                    self.on_custom_call(p, ts, span);
                }
            }
            "function_call_output" | "custom_tool_call_output" => {
                if let Some((ts, p)) = parse_line::<OutputPayload>(line) {
                    self.on_output(p, ts, span);
                }
            }
            "local_shell_call" => {
                if let Some((ts, p)) = parse_line::<ActionPayload>(line) {
                    self.on_local_shell(p, ts, span);
                }
            }
            "web_search_call" => {
                if let Some((ts, p)) = parse_line::<ActionPayload>(line) {
                    self.on_web_search(p, ts);
                }
            }
            "compaction" => {
                if let Some((ts, _)) = parse_line::<serde::de::IgnoredAny>(line) {
                    self.compaction_event(ts, None);
                }
            }
            "agent_message" => {
                if let Some((ts, p)) = parse_line::<AgentMessagePayload>(line) {
                    self.on_agent_message(p, ts);
                }
            }
            _ => {}
        }
    }

    fn event_msg(&mut self, sub: &str, line: &[u8], span: JsonlSpan) {
        if sub == "item_completed" {
            if let Some((ts, p)) = parse_line::<ItemCompletedPayload>(line) {
                self.on_item(p.item, ts, span);
            }
            return;
        }
        if !matches!(
            sub,
            "task_started"
                | "task_complete"
                | "turn_aborted"
                | "thread_settings_applied"
                | "token_count"
        ) {
            return;
        }
        let Some((ts, p)) = parse_line::<EventPayload>(line) else {
            return;
        };
        match sub {
            "task_started" => {
                if let Some(turn) = owned(&p.turn_id) {
                    self.start_turn(turn);
                }
            }
            "task_complete" => self.flush_pool(ts),
            "turn_aborted" => {
                let reason = match p.reason {
                    Some(Value::String(reason)) => reason,
                    Some(other) if !other.is_null() => other.to_string(),
                    _ => "interrupted".to_string(),
                };
                self.abort_turn(ts, reason);
            }
            "thread_settings_applied" => {
                if let Some(model) = p.thread_settings.as_ref().and_then(|s| owned(&s.model)) {
                    self.note_model(model, ts);
                }
            }
            "token_count" => {
                if self.usage_from_records {
                    return;
                }
                let usage = p.info.and_then(|i| i.last_token_usage);
                if let (Some(turn), Some(usage)) = (self.turn_id.clone(), usage) {
                    self.turn_usage.entry(turn).or_default().add(&usage);
                }
            }
            _ => {}
        }
    }

    // ─── 轮次 ────────────────────────────────────────────────────────

    fn start_turn(&mut self, turn: String) {
        if self.turn_id.as_deref() != Some(turn.as_str()) {
            self.flush_pool(None);
            if let Some(model) = &self.last_model {
                self.turn_models
                    .entry(turn.clone())
                    .or_insert_with(|| model.clone());
            }
            self.turn_id = Some(turn);
        }
        self.flush_model_event();
    }

    /// 模型与上一个不同才记一次 ModelChange（第一次只作为基准）
    fn note_model(&mut self, model: String, ts: Option<i64>) {
        if self.last_model.as_ref().is_some_and(|last| *last != model) {
            self.pending_model_event = Some((model.clone(), ts));
        }
        self.last_model = Some(model);
    }

    fn flush_model_event(&mut self) {
        if let Some((model, ts)) = self.pending_model_event.take() {
            self.push(
                "system",
                ts,
                None,
                vec![SessionBlock::event(
                    EventKind::ModelChange,
                    Some(model),
                    None,
                )],
                false,
            );
        }
    }

    fn abort_turn(&mut self, ts: Option<i64>, reason: String) {
        for call_id in std::mem::take(&mut self.open) {
            if let Some(call) = self.calls.remove(&call_id) {
                self.finish_call(call_id, call, None, ts, None, true);
            }
        }
        self.flush_pool(ts);
        self.push(
            "system",
            ts,
            None,
            vec![SessionBlock::event(EventKind::Aborted, Some(reason), None)],
            false,
        );
    }

    /// `summary` 为带摘要的 Compaction 事件块（`compacted` 记录）；其余来源只标记发生过压缩。
    fn compaction_event(&mut self, ts: Option<i64>, summary: Option<SessionBlock>) {
        if let Some(idx) = self.last_compaction {
            // 同一次压缩的多种记录：补上缺的摘要即可
            if let (Some(first), Some(summary)) = (self.messages[idx].blocks.first_mut(), summary) {
                if matches!(first, SessionBlock::Event { text: None, .. }) {
                    *first = summary;
                }
            }
            return;
        }
        let block =
            summary.unwrap_or_else(|| SessionBlock::event(EventKind::Compaction, None, None));
        let idx = self.push("system", ts, None, vec![block], false);
        self.last_compaction = Some(idx);
    }

    // ─── 消息输出 ────────────────────────────────────────────────────

    fn push(
        &mut self,
        role: &str,
        ts: Option<i64>,
        id: Option<String>,
        blocks: Vec<SessionBlock>,
        injected: bool,
    ) -> usize {
        self.flush_model_event();
        self.last_compaction = None;
        self.messages.push(SessionMessage {
            role: role.to_string(),
            ts,
            id,
            turn_id: self.turn_id.clone(),
            injected,
            blocks,
            ..SessionMessage::default()
        });
        self.messages.len() - 1
    }

    /// 助手侧块：前一条是同一轮「只有思考」的助手消息时并入，否则新起一条。
    /// 返回 `(消息下标, 首个新块下标)`。
    fn push_assistant(
        &mut self,
        ts: Option<i64>,
        id: Option<String>,
        blocks: Vec<SessionBlock>,
    ) -> (usize, usize) {
        self.flush_model_event();
        let turn_id = self.turn_id.clone();
        if let Some(last) = self.messages.last_mut() {
            let thinking_only = !last.blocks.is_empty()
                && last
                    .blocks
                    .iter()
                    .all(|b| matches!(b, SessionBlock::Thinking { .. }));
            if last.role == "assistant" && last.turn_id == turn_id && thinking_only {
                let start = last.blocks.len();
                last.blocks.extend(blocks);
                last.ts = ts.or(last.ts);
                if id.is_some() {
                    last.id = id;
                }
                self.last_compaction = None;
                return (self.messages.len() - 1, start);
            }
        }
        (self.push("assistant", ts, id, blocks, false), 0)
    }

    fn on_message(&mut self, p: MessagePayload<'_>, ts: Option<i64>, span: JsonlSpan) {
        let id = owned(&p.id);
        let Some(contents) = p.content.as_ref() else {
            return;
        };
        match opt_str(&p.role).unwrap_or_default() {
            "user" => self.on_user_message(contents, id, ts, span),
            "assistant" => {
                let text = contents.joined_text("\n");
                if text.trim().is_empty() {
                    return;
                }
                let (idx, _) = self.push_assistant(ts, id, vec![SessionBlock::text(text)]);
                if let Some(phase) = owned(&p.phase) {
                    self.messages[idx]
                        .meta
                        .get_or_insert_with(MessageMeta::default)
                        .stop_reason = Some(phase);
                }
            }
            // developer / system：系统注入，默认隐藏；超长时只放预览 + 引用
            _ => {
                let text = contents.joined_text("\n");
                if !text.trim().is_empty() {
                    let block = large_text_block(text, || {
                        Some(span.content_ref(contents.text_pointer("/payload/content")))
                    });
                    self.push("system", ts, id, vec![block], true);
                }
            }
        }
    }

    fn on_user_message(
        &mut self,
        contents: &Contents<'_>,
        id: Option<String>,
        ts: Option<i64>,
        span: JsonlSpan,
    ) {
        let mut blocks = Vec::new();
        // (注入文本, 在行内的 JSON Pointer)
        let mut injected: Vec<(String, String)> = Vec::new();
        let mut pending_text: Vec<String> = Vec::new();
        let flush_text = |pending: &mut Vec<String>, blocks: &mut Vec<SessionBlock>| {
            if !pending.is_empty() {
                blocks.push(SessionBlock::text(pending.join("\n")));
                pending.clear();
            }
        };
        let mut handle_text = |text: &str, pointer: String, pending: &mut Vec<String>| {
            let trimmed = text.trim();
            if trimmed.is_empty() || is_image_wrapper(trimmed) {
                return;
            }
            if INJECTED_USER_PREFIXES
                .iter()
                .any(|p| trimmed.starts_with(p))
                || is_wrapped_in_tag(trimmed)
            {
                injected.push((text.to_string(), pointer));
            } else if trimmed.starts_with(VSCODE_CONTEXT_PREFIX) {
                match extract_codex_prompt_from_ide_context(trimmed) {
                    Some(prompt) => pending.push(prompt),
                    None => injected.push((text.to_string(), pointer)),
                }
            } else {
                pending.push(text.to_string());
            }
        };
        match contents {
            Contents::Text(text) => handle_text(
                text.as_str(),
                "/payload/content".to_string(),
                &mut pending_text,
            ),
            Contents::Items(items) => {
                for (i, item) in items.iter().enumerate() {
                    if matches!(item.kind(), "input_image" | "image") {
                        let pointer = format!("/payload/content/{i}/image_url");
                        if let Some(image) = opt_str(&item.image_url)
                            .and_then(|url| image_from_url(url, span, pointer))
                        {
                            flush_text(&mut pending_text, &mut blocks);
                            blocks.push(SessionBlock::Image { image });
                        }
                    } else if let Some(text) = opt_str(&item.text) {
                        handle_text(
                            text,
                            format!("/payload/content/{i}/text"),
                            &mut pending_text,
                        );
                    }
                }
            }
        }
        flush_text(&mut pending_text, &mut blocks);

        if !injected.is_empty() {
            // 每段注入文本单独成块，超长的（AGENTS.md 之类）只放预览 + 引用
            let injected_blocks = injected
                .into_iter()
                .map(|(text, pointer)| large_text_block(text, || Some(span.content_ref(pointer))))
                .collect();
            self.push("user", ts, id.clone(), injected_blocks, true);
        }
        if !blocks.is_empty() {
            self.push("user", ts, id, blocks, false);
        }
    }

    fn on_reasoning(&mut self, p: ReasoningPayload<'_>, ts: Option<i64>, span: JsonlSpan) {
        let summary = p
            .summary
            .as_ref()
            .map(|s| s.joined_text("\n\n"))
            .unwrap_or_default();
        let content = p
            .content
            .as_ref()
            .map(|c| c.joined_text("\n\n"))
            .unwrap_or_default();
        let redacted = summary.trim().is_empty() && content.trim().is_empty();
        if redacted && self.last_block_is_redacted_thinking() {
            return;
        }
        let text = preview_chars(&content, THINKING_PREVIEW_CHARS);
        let full = match p.content.as_ref() {
            Some(c) if text.truncated => Some(span.content_ref(c.text_pointer("/payload/content"))),
            _ => None,
        };
        let block = SessionBlock::Thinking {
            text: text.text,
            summary: (!summary.trim().is_empty()).then_some(summary),
            redacted,
            duration_ms: None,
            full,
        };
        self.push_assistant(ts, owned(&p.id), vec![block]);
    }

    fn last_block_is_redacted_thinking(&self) -> bool {
        self.messages.last().is_some_and(|m| {
            m.role == "assistant"
                && m.turn_id == self.turn_id
                && matches!(
                    m.blocks.last(),
                    Some(SessionBlock::Thinking { redacted: true, .. })
                )
        })
    }

    fn on_agent_message(&mut self, p: AgentMessagePayload<'_>, ts: Option<i64>) {
        let text = p
            .content
            .as_ref()
            .map(|c| c.joined_text("\n"))
            .unwrap_or_default();
        let text = text.trim();
        let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
        let author = opt_str(&p.author).unwrap_or_default();
        let event = if author.is_empty() {
            first.trim().to_string()
        } else {
            format!("{author}: {}", first.trim())
        };
        let mut blocks = vec![SessionBlock::event(EventKind::SubAgent, Some(event), None)];
        if !rest.trim().is_empty() {
            blocks.push(SessionBlock::text(rest.trim().to_string()));
        }
        self.push("system", ts, owned(&p.id), blocks, false);
    }

    fn on_web_search(&mut self, p: ActionPayload<'_>, ts: Option<i64>) {
        let id = owned(&p.id)
            .or_else(|| self.last_web_search_id.take())
            .unwrap_or_else(|| format!("ws_{}", self.messages.len()));
        self.last_web_search_id = None;
        let action = p.action.unwrap_or(Value::Null);
        let input = if action.is_null() {
            String::new()
        } else {
            action.to_string()
        };
        let status = match opt_str(&p.status).map(status_from_str) {
            Some(ToolStatus::Error) => ToolStatus::Error,
            Some(ToolStatus::Interrupted) => ToolStatus::Interrupted,
            _ => ToolStatus::Success,
        };
        let call = CallSpec::new(
            id.clone(),
            "web_search",
            ToolKind::Web,
            web_action_title(&action),
            &input,
        );
        let result = ResultSpec::empty(id.clone(), status);
        self.push_assistant(ts, Some(id), vec![call.into_block(), result.into_block()]);
    }

    // ─── 工具调用 ────────────────────────────────────────────────────

    fn register_call(
        &mut self,
        ts: Option<i64>,
        id: Option<String>,
        call_id: String,
        block: SessionBlock,
        flavor: CallFlavor,
        cmds: Vec<String>,
    ) {
        let (msg_idx, block_idx) = self.push_assistant(ts, id, vec![block]);
        self.open.retain(|c| *c != call_id);
        self.open.push(call_id.clone());
        self.calls.insert(
            call_id,
            PendingCall {
                msg_idx,
                block_idx,
                flavor,
                cmds,
                items: Vec::new(),
            },
        );
    }

    fn on_function_call(&mut self, p: FunctionCallPayload<'_>, ts: Option<i64>, span: JsonlSpan) {
        let name = opt_str(&p.name).unwrap_or("unknown");
        let namespace = opt_str(&p.namespace);
        let arguments = opt_str(&p.arguments).unwrap_or_default();
        let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
        let call_id = owned(&p.call_id)
            .or_else(|| owned(&p.id))
            .unwrap_or_default();

        let normalized = normalize_tool(ToolSource::Codex, name, namespace);
        let str_arg = |key: &str| {
            args.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        };
        let mut detail = None;
        let mut cmds = Vec::new();
        let mut flavor = CallFlavor::Other;
        let mut kind = normalized.kind;
        let title = match kind {
            ToolKind::Mcp => {
                flavor = CallFlavor::Mcp;
                detail = first_string_field(&args);
                title_mcp(normalized.server.as_deref().unwrap_or("mcp"), name)
            }
            ToolKind::Shell if name == "write_stdin" => match str_arg("chars") {
                Some(chars) => title_shell(chars),
                None => match args.get("session_id") {
                    Some(id) if !id.is_null() => format!("session {id}"),
                    _ => title_other(name),
                },
            },
            ToolKind::Shell => {
                flavor = CallFlavor::Shell;
                let cmd = args
                    .get("cmd")
                    .or_else(|| args.get("command"))
                    .map(command_from_value)
                    .unwrap_or_default();
                detail = str_arg("workdir").map(str::to_string);
                kind = refine_shell_kind(&cmd);
                let title = title_shell(&cmd);
                if !cmd.is_empty() {
                    cmds.push(cmd);
                }
                title
            }
            ToolKind::Ask => {
                let question = args
                    .get("questions")
                    .and_then(Value::as_array)
                    .and_then(|q| q.first())
                    .and_then(|q| {
                        ["title", "question", "header"]
                            .iter()
                            .find_map(|k| q.get(*k).and_then(Value::as_str))
                    })
                    .or_else(|| str_arg("question"));
                question.map(title_ask).unwrap_or_else(|| title_other(name))
            }
            ToolKind::Agent => {
                detail = str_arg("model").map(str::to_string);
                ["task_name", "description", "agent_type", "target", "name"]
                    .iter()
                    .find_map(|k| str_arg(k))
                    .map(title_agent)
                    .unwrap_or_else(|| title_other(name))
            }
            ToolKind::Todo => {
                let count = args
                    .get("plan")
                    .or_else(|| args.get("todos"))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                title_todo(count)
            }
            ToolKind::Read | ToolKind::Write | ToolKind::Edit => {
                match str_arg("path").or_else(|| str_arg("file_path")) {
                    Some(path) if kind == ToolKind::Read => {
                        let num = |k: &str| args.get(k).and_then(Value::as_u64);
                        let (title, range) = title_read(path, num("offset"), num("limit"));
                        detail = range;
                        title
                    }
                    Some(path) => title_path(path),
                    None => title_other(name),
                }
            }
            ToolKind::Web => ["query", "url", "q"]
                .iter()
                .find_map(|k| str_arg(k))
                .map(|q| title_web(&[q]))
                .unwrap_or_else(|| title_other(name)),
            _ => {
                detail = first_string_field(&args);
                title_other(name)
            }
        };

        let block = CallSpec {
            detail,
            server: normalized.server,
            input_ref: Some(span.content_ref("/payload/arguments")),
            ..CallSpec::new(call_id.clone(), name, kind, title, arguments)
        }
        .into_block();
        self.register_call(ts, owned(&p.id), call_id, block, flavor, cmds);
    }

    fn on_custom_call(&mut self, p: CustomCallPayload<'_>, ts: Option<i64>, span: JsonlSpan) {
        let name = opt_str(&p.name).unwrap_or("unknown");
        let input = opt_str(&p.input).unwrap_or_default();
        let call_id = owned(&p.call_id)
            .or_else(|| owned(&p.id))
            .unwrap_or_default();

        let mut spec = CallSpec {
            input_ref: Some(span.content_ref("/payload/input")),
            ..CallSpec::new(
                call_id.clone(),
                name,
                ToolKind::Other,
                title_other(name),
                input,
            )
        };
        let (flavor, cmds) = match name {
            "exec" => {
                let script = parse_exec_script(input);
                let (kind, title) = exec_title(&script, input);
                spec.kind = kind;
                spec.title = title;
                spec.detail = script.workdir.clone();
                spec.server = script.mcp.as_ref().map(|(server, _)| server.clone());
                (CallFlavor::Exec, script.cmds)
            }
            "apply_patch" => {
                // 先按补丁文本统计；匹配到 FileChange 时再用其 unified_diff 替换
                let diff = diff_summary(
                    parse_apply_patch(input),
                    Some(span.content_ref("/payload/input")),
                );
                spec.kind = diff_kind(&diff);
                spec.title = diff_title(&diff);
                spec.diff = Some(diff);
                (CallFlavor::ApplyPatch, Vec::new())
            }
            _ => {
                let normalized = normalize_tool(ToolSource::Codex, name, None);
                spec.kind = normalized.kind;
                spec.server = normalized.server;
                (CallFlavor::Other, Vec::new())
            }
        };
        let block = spec.into_block();
        self.register_call(ts, owned(&p.id), call_id, block, flavor, cmds);
    }

    fn on_local_shell(&mut self, p: ActionPayload<'_>, ts: Option<i64>, span: JsonlSpan) {
        let action = p.action.unwrap_or(Value::Null);
        let cmd = action
            .get("command")
            .map(command_from_value)
            .unwrap_or_default();
        let input = action.to_string();
        let call_id = owned(&p.call_id)
            .or_else(|| owned(&p.id))
            .unwrap_or_default();
        let block = CallSpec {
            detail: action
                .get("working_directory")
                .and_then(Value::as_str)
                .map(str::to_string),
            input_ref: Some(span.content_ref("/payload/action")),
            ..CallSpec::new(
                call_id.clone(),
                "local_shell",
                refine_shell_kind(&cmd),
                title_shell(&cmd),
                &input,
            )
        }
        .into_block();
        let cmds = if cmd.is_empty() {
            Vec::new()
        } else {
            vec![cmd]
        };
        self.register_call(ts, owned(&p.id), call_id, block, CallFlavor::Shell, cmds);
    }

    // ─── 结构化项 ────────────────────────────────────────────────────

    fn on_item(&mut self, item: RawItem<'_>, ts: Option<i64>, span: JsonlSpan) {
        match item.ty.as_str() {
            "AgentMessage" => {
                if let (Some(id), Some(phase)) = (owned(&item.id), owned(&item.phase)) {
                    self.phases.insert(id, phase);
                }
                return;
            }
            "ContextCompaction" => {
                self.compaction_event(ts, None);
                return;
            }
            "WebSearch" => {
                // 紧跟的 web_search_call 已带 action，这里只借 id
                self.last_web_search_id = owned(&item.id);
                return;
            }
            _ => {}
        }
        let item_id = owned(&item.id).unwrap_or_default();
        let Some(record) = item.into_record(span) else {
            return;
        };

        // 1. id 直接命中调用：exec_command 的 CommandExecution、McpToolCall、sleep 的 Extension
        if let Some(call) = self.calls.get_mut(&item_id) {
            call.items.push(record);
            return;
        }
        // 1b. 调用已经随输出结束（输出先于 item_completed 到达）：只回填，不再成步骤
        if !item_id.is_empty() && self.finished.contains(&item_id) {
            if let ItemRecord::Command(info) = &record {
                self.backfill_finished_result(&item_id, info);
            }
            return;
        }
        // 2. 按类型挂到最近的未完成调用
        let claim = match &record {
            ItemRecord::Command(info) => self.claim_command(info),
            ItemRecord::FileChange(_) => Claim::from(
                self.latest_open(&[CallFlavor::ApplyPatch])
                    .or_else(|| self.latest_open(&[CallFlavor::Exec])),
            ),
            ItemRecord::Mcp(_) | ItemRecord::Web(_) => {
                Claim::from(self.latest_open(&[CallFlavor::Exec]))
            }
            ItemRecord::ImageView { .. } => Claim::from(self.open.last().cloned()),
        };
        match (claim, record) {
            (Claim::Pool, ItemRecord::Command(info)) => self.pool.push(info),
            (Claim::Call(call_id), record) => match self.calls.get_mut(&call_id) {
                Some(call) => call.items.push(record),
                None => self.emit_standalone(record, ts),
            },
            (_, record) => self.emit_standalone(record, ts),
        }
    }

    fn latest_open(&self, flavors: &[CallFlavor]) -> Option<String> {
        self.open
            .iter()
            .rev()
            .find(|id| {
                self.calls
                    .get(*id)
                    .is_some_and(|c| flavors.contains(&c.flavor))
            })
            .cloned()
    }

    /// CommandExecution 归属：命令文本匹配的调用 → 唯一一个看不出命令的调用 →
    /// 多个看不出命令的调用（先放池子）→ 都不是（后台进程结束等，单独成块）
    fn claim_command(&self, info: &CommandInfo) -> Claim {
        let candidates: Vec<(&String, &PendingCall)> = self
            .open
            .iter()
            .rev()
            .filter_map(|id| self.calls.get(id).map(|c| (id, c)))
            .filter(|(_, c)| matches!(c.flavor, CallFlavor::Exec | CallFlavor::Shell))
            .collect();
        if let Some((id, _)) = candidates
            .iter()
            .find(|(_, c)| c.cmds.iter().any(|cmd| same_command(cmd, &info.command)))
        {
            return Claim::Call((*id).clone());
        }
        let unknown: Vec<&String> = candidates
            .iter()
            .filter(|(_, c)| c.cmds.is_empty())
            .map(|(id, _)| *id)
            .collect();
        match unknown.as_slice() {
            [] => Claim::None,
            [id] => Claim::Call((*id).clone()),
            _ => Claim::Pool,
        }
    }

    /// 迟到的 CommandExecution：把退出码、耗时补进已输出的结果（只补缺的字段）
    fn backfill_finished_result(&mut self, call_id: &str, info: &CommandInfo) {
        for message in self.messages.iter_mut().rev() {
            for block in message.blocks.iter_mut() {
                if let SessionBlock::ToolResult {
                    call_id: id,
                    status,
                    exit_code,
                    duration_ms,
                    ..
                } = block
                {
                    if id == call_id {
                        if exit_code.is_none() {
                            *exit_code = info.exit_code;
                        }
                        if duration_ms.is_none() {
                            *duration_ms = info.duration_ms;
                        }
                        // 输出先到时只能判为成功，失败 / 中断以迟到的结构化项为准
                        if *status == ToolStatus::Success {
                            if matches!(info.status, ToolStatus::Error | ToolStatus::Interrupted) {
                                *status = info.status;
                            } else if exit_code.is_some_and(|c| c != 0) {
                                *status = ToolStatus::Error;
                            }
                        }
                        return;
                    }
                }
            }
        }
    }

    /// 找不到调用的结构化项：自己成一条助手消息（调用 + 结果）
    fn emit_standalone(&mut self, record: ItemRecord, ts: Option<i64>) {
        let (id, blocks) = match record {
            ItemRecord::Command(info) => {
                let blocks = vec![
                    info.call_spec().into_block(),
                    info.result_spec(info.id.clone()).into_block(),
                ];
                (info.id, blocks)
            }
            ItemRecord::FileChange(info) => {
                let call = CallSpec {
                    diff: Some(info.diff.clone()),
                    ..CallSpec::new(
                        info.id.clone(),
                        "FileChange",
                        diff_kind(&info.diff),
                        diff_title(&info.diff),
                        "",
                    )
                };
                let result = ResultSpec::empty(info.id.clone(), info.status);
                (info.id, vec![call.into_block(), result.into_block()])
            }
            ItemRecord::Mcp(info) => {
                let call = CallSpec {
                    detail: info.detail.clone(),
                    server: Some(info.server.clone()),
                    ..CallSpec::new(
                        info.id.clone(),
                        info.tool.clone(),
                        ToolKind::Mcp,
                        title_mcp(&info.server, &info.tool),
                        &info.input,
                    )
                };
                let result = ResultSpec {
                    output: info.output.clone(),
                    full: info.output_ref.clone(),
                    duration_ms: info.duration_ms,
                    images: info.images.clone(),
                    ..ResultSpec::empty(info.id.clone(), info.status)
                };
                let blocks = vec![call.into_block(), result.into_block()];
                (info.id, blocks)
            }
            ItemRecord::ImageView { id, path } => (
                id,
                vec![SessionBlock::Image {
                    image: local_image(&path),
                }],
            ),
            ItemRecord::Web(info) => {
                let call = CallSpec::new(
                    info.id.clone(),
                    "web_search",
                    ToolKind::Web,
                    info.title.clone(),
                    &info.input,
                );
                let result = ResultSpec::empty(info.id.clone(), ToolStatus::Success);
                let blocks = vec![call.into_block(), result.into_block()];
                (info.id, blocks)
            }
        };
        let id = (!id.is_empty()).then_some(id);
        self.push_assistant(ts, id, blocks);
    }

    fn flush_pool(&mut self, ts: Option<i64>) {
        for info in std::mem::take(&mut self.pool) {
            self.emit_standalone(ItemRecord::Command(info), ts);
        }
    }

    // ─── 输出与回填 ──────────────────────────────────────────────────

    fn on_output(&mut self, p: OutputPayload<'_>, ts: Option<i64>, span: JsonlSpan) {
        let call_id = owned(&p.call_id).unwrap_or_default();
        let out = OutputInfo::from_contents(p.output.as_ref(), span);
        let id = owned(&p.id);
        match self.calls.remove(&call_id) {
            Some(call) => {
                self.open.retain(|c| *c != call_id);
                self.finish_call(call_id, call, Some(out), ts, id, false);
            }
            None => {
                let status = output_status(&out, None);
                let result = ResultSpec {
                    output: preview(&out.text),
                    full: out.full,
                    exit_code: out.exit_code,
                    duration_ms: out.duration_ms,
                    images: out.images,
                    ..ResultSpec::empty(call_id, status)
                };
                self.push("tool", ts, id, vec![result.into_block()], false);
            }
        }
    }

    /// 用挂上的结构化项回填调用块，并输出结果。
    /// `out = None` 时：`aborted` 为真输出中断结果，否则（文件末尾仍未完成）只回填调用块。
    fn finish_call(
        &mut self,
        call_id: String,
        call: PendingCall,
        out: Option<OutputInfo>,
        ts: Option<i64>,
        msg_id: Option<String>,
        aborted: bool,
    ) {
        self.finished.insert(call_id.clone());
        let PendingCall {
            msg_idx,
            block_idx,
            flavor,
            items,
            ..
        } = call;
        let is_shell = matches!(flavor, CallFlavor::Exec | CallFlavor::Shell);

        let mut commands = Vec::new();
        let mut file_change: Option<FileChangeInfo> = None;
        let mut mcp = None;
        let mut web = None;
        let mut views = Vec::new();
        for item in items {
            match item {
                ItemRecord::Command(info) => commands.push(info),
                ItemRecord::FileChange(info) => match &mut file_change {
                    Some(existing) => {
                        let mut files = std::mem::take(&mut existing.diff.files);
                        files.extend(info.diff.files);
                        existing.diff = diff_summary(files, None);
                    }
                    None => file_change = Some(info),
                },
                ItemRecord::Mcp(info) => mcp = Some(info),
                ItemRecord::Web(info) => web = Some(info),
                ItemRecord::ImageView { path, .. } => views.push(path),
            }
        }
        // 兜底：输出内容里包含池中某条命令输出的开头 → 认领
        if let (Some(out), true, true) = (&out, is_shell, commands.is_empty()) {
            if let Some(pos) = self
                .pool
                .iter()
                .position(|c| !c.output_head.is_empty() && out.text.contains(&c.output_head))
            {
                commands.push(self.pool.remove(pos));
            }
        }

        // 1. 回填调用块
        if let Some(SessionBlock::ToolCall {
            kind,
            title,
            detail,
            server,
            diff,
            ..
        }) = self.messages[msg_idx].blocks.get_mut(block_idx)
        {
            if is_shell {
                if let [command] = commands.as_slice() {
                    *kind = command.kind;
                    *title = title_shell(&command.command);
                    *detail = command.cwd.clone().or(detail.take());
                } else if commands.is_empty() {
                    if let Some(fc) = &file_change {
                        *kind = diff_kind(&fc.diff);
                        *title = diff_title(&fc.diff);
                        *diff = Some(fc.diff.clone());
                    } else if let Some(m) = &mcp {
                        *kind = ToolKind::Mcp;
                        *title = title_mcp(&m.server, &m.tool);
                        *server = Some(m.server.clone());
                        *detail = m.detail.clone();
                    } else if let Some(w) = web.as_ref().filter(|w| !w.title.is_empty()) {
                        *kind = ToolKind::Web;
                        *title = w.title.clone();
                    }
                }
            } else if let (CallFlavor::ApplyPatch, Some(fc)) = (flavor, &file_change) {
                let mut merged = fc.diff.clone();
                if merged.full.is_none() {
                    merged.full = diff.as_ref().and_then(|d| d.full.clone());
                }
                *kind = diff_kind(&merged);
                *diff = Some(merged);
            }
        }
        // 多条命令：拆成 `call_id#1`、`call_id#2`… 多个子调用
        let result_ids: Vec<String> = if is_shell && commands.len() > 1 {
            let template = self.messages[msg_idx].blocks[block_idx].clone();
            let subs: Vec<SessionBlock> = commands
                .iter()
                .enumerate()
                .map(|(k, command)| {
                    let mut block = template.clone();
                    if let SessionBlock::ToolCall {
                        id,
                        kind,
                        title,
                        detail,
                        ..
                    } = &mut block
                    {
                        *id = format!("{call_id}#{}", k + 1);
                        *kind = command.kind;
                        *title = title_shell(&command.command);
                        *detail = command.cwd.clone().or(detail.take());
                    }
                    block
                })
                .collect();
            self.messages[msg_idx]
                .blocks
                .splice(block_idx..=block_idx, subs);
            (1..=commands.len())
                .map(|k| format!("{call_id}#{k}"))
                .collect()
        } else {
            vec![call_id.clone()]
        };

        if out.is_none() && !aborted {
            return;
        }

        // 2. 结果：前面的子命令用 CommandExecution 自带的输出，最后一个用工具输出
        let mut results: Vec<SessionBlock> = Vec::new();
        if result_ids.len() > 1 {
            for (command, id) in commands.iter().zip(&result_ids) {
                if Some(id) == result_ids.last() {
                    break;
                }
                results.push(command.result_spec(id.clone()).into_block());
            }
        }
        let last_id = result_ids.last().cloned().unwrap_or(call_id);
        let last_command = if is_shell { commands.last() } else { None };
        let extra_status = file_change
            .as_ref()
            .map(|f| f.status)
            .or(mcp.as_ref().map(|m| m.status));
        let spec = match out {
            Some(out) => {
                let mut status = output_status(&out, last_command);
                if let Some(extra @ (ToolStatus::Error | ToolStatus::Interrupted)) = extra_status {
                    status = extra;
                }
                let images = if !out.images.is_empty() {
                    out.images
                } else {
                    let mut images = mcp.as_ref().map(|m| m.images.clone()).unwrap_or_default();
                    images.extend(views.iter().map(|p| local_image(p)));
                    images
                };
                ResultSpec {
                    output: preview(&out.text),
                    full: out.full,
                    exit_code: last_command.and_then(|c| c.exit_code).or(out.exit_code),
                    duration_ms: last_command
                        .and_then(|c| c.duration_ms)
                        .or(out.duration_ms)
                        .or(mcp.as_ref().and_then(|m| m.duration_ms)),
                    images,
                    ..ResultSpec::empty(last_id, status)
                }
            }
            None => match last_command {
                Some(command) => {
                    let mut spec = command.result_spec(last_id);
                    if matches!(spec.status, ToolStatus::Unknown | ToolStatus::Pending) {
                        spec.status = ToolStatus::Interrupted;
                    }
                    spec
                }
                None => ResultSpec::empty(last_id, ToolStatus::Interrupted),
            },
        };
        results.push(spec.into_block());
        self.push("tool", ts, msg_id, results, false);
    }

    // ─── 收尾 ────────────────────────────────────────────────────────

    fn finish(mut self) -> Vec<SessionMessage> {
        // 文件末尾仍未完成的调用：只回填标题等，不伪造结果（前端显示进行中）
        for call_id in std::mem::take(&mut self.open) {
            if let Some(call) = self.calls.remove(&call_id) {
                self.finish_call(call_id, call, None, None, None, false);
            }
        }
        self.flush_pool(None);
        self.flush_model_event();

        // 每轮最后一条带正文的助手消息挂上模型与 token 用量
        let mut final_by_turn: HashMap<&str, usize> = HashMap::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            let has_text = msg
                .blocks
                .iter()
                .any(|b| matches!(b, SessionBlock::Text { .. }));
            if let (true, true, Some(turn)) = (msg.role == "assistant", has_text, &msg.turn_id) {
                final_by_turn.insert(turn.as_str(), idx);
            }
        }
        let targets: Vec<(String, usize)> = final_by_turn
            .into_iter()
            .map(|(turn, idx)| (turn.to_string(), idx))
            .collect();
        for (turn, idx) in targets {
            let model = self.turn_models.get(&turn).cloned();
            let usage = self.turn_usage.get(&turn).copied();
            if model.is_none() && usage.is_none() {
                continue;
            }
            let meta = self.messages[idx]
                .meta
                .get_or_insert_with(MessageMeta::default);
            meta.model = model;
            if let Some(usage) = usage {
                let nonzero = |n: u64| (n > 0).then_some(n);
                meta.input_tokens = Some(usage.input_tokens);
                meta.output_tokens = Some(usage.output_tokens);
                meta.cache_read_tokens = nonzero(usage.cached_input_tokens);
                meta.cache_write_tokens = nonzero(usage.cache_write_input_tokens);
                meta.reasoning_tokens = nonzero(usage.reasoning_output_tokens);
            }
        }

        let phases = std::mem::take(&mut self.phases);
        let mut messages = self.messages;
        for msg in &mut messages {
            if msg.role == "assistant" {
                if let Some(phase) = msg.id.as_ref().and_then(|id| phases.get(id)) {
                    let meta = msg.meta.get_or_insert_with(MessageMeta::default);
                    if meta.stop_reason.is_none() {
                        meta.stop_reason = Some(phase.clone());
                    }
                }
            }
            msg.content = project_content(&msg.blocks);
        }
        messages.retain(|m| !m.is_empty());
        messages
    }
}

enum Claim {
    Call(String),
    Pool,
    None,
}

impl From<Option<String>> for Claim {
    fn from(call_id: Option<String>) -> Self {
        call_id.map_or(Claim::None, Claim::Call)
    }
}

/// 结果状态：CommandExecution 的状态优先，其次输出头（Script failed / 非零退出码）
fn output_status(out: &OutputInfo, command: Option<&CommandInfo>) -> ToolStatus {
    let mut status = match command.map(|c| c.status) {
        Some(ToolStatus::Unknown | ToolStatus::Pending) | None => ToolStatus::Success,
        Some(status) => status,
    };
    let exit_code = command.and_then(|c| c.exit_code).or(out.exit_code);
    if status == ToolStatus::Success && (out.failed || exit_code.is_some_and(|c| c != 0)) {
        status = ToolStatus::Error;
    }
    status
}

/// 整段被同一个 XML 风格标签包住（`<recommended_plugins>…</recommended_plugins>` 之类）：
/// 是客户端注入的上下文，不是用户手打的提问
fn is_wrapped_in_tag(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('<') else {
        return false;
    };
    let name: &str = rest
        .split(|c: char| c == '>' || c.is_whitespace())
        .next()
        .unwrap_or_default();
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && text.ends_with(&format!("</{name}>"))
}

/// Codex 给图片包的 `<image name=…>` / `</image>` 标签行
fn is_image_wrapper(text: &str) -> bool {
    text == "</image>"
        || (text.starts_with("<image ") && text.ends_with('>') && !text.contains('\n'))
}

pub fn delete_session(_root: &Path, path: &Path, session_id: &str) -> Result<bool, String> {
    let meta = parse_session(path)
        .ok_or_else(|| format!("Failed to parse Codex session metadata: {}", path.display()))?;

    if meta.session_id != session_id {
        return Err(format!(
            "Codex session ID mismatch: expected {session_id}, found {}",
            meta.session_id
        ));
    }

    std::fs::remove_file(path).map_err(|e| {
        format!(
            "Failed to delete Codex session file {}: {e}",
            path.display()
        )
    })?;

    Ok(true)
}

fn parse_session(path: &Path) -> Option<SessionMeta> {
    parse_session_with_titles(path, &HashMap::new())
}

/// 列表扫描用：读不了返回 `Err`，不进解析缓存、下轮重试；读到了但不是会话返回 `Ok(None)`。
/// 线程标题由调用方在拿到结果后覆盖，这里不带。
fn scan_session_file(path: &Path) -> std::io::Result<Option<SessionMeta>> {
    let (head, tail) = read_head_tail_lines(path, 10, 30)?;
    Ok(parse_session_lines(path, head, tail, &HashMap::new()))
}

fn parse_session_with_titles(
    path: &Path,
    thread_titles: &HashMap<String, String>,
) -> Option<SessionMeta> {
    let (head, tail) = read_head_tail_lines(path, 10, 30).ok()?;
    parse_session_lines(path, head, tail, thread_titles)
}

fn parse_session_lines(
    path: &Path,
    head: Vec<String>,
    tail: Vec<String>,
    thread_titles: &HashMap<String, String>,
) -> Option<SessionMeta> {
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
        if created_at.is_none() {
            created_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(payload) = value.get("payload") {
                if is_subagent_source(payload.get("source")) {
                    return None;
                }
                if session_id.is_none() {
                    session_id = payload
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                }
                if project_dir.is_none() {
                    project_dir = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                }
                if let Some(ts) = payload.get("timestamp").and_then(parse_timestamp_to_ms) {
                    created_at.get_or_insert(ts);
                }
            }
        }
        // Extract first user message as title candidate
        if first_user_message.is_none()
            && value.get("type").and_then(Value::as_str) == Some("response_item")
        {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(Value::as_str) == Some("message")
                    && payload.get("role").and_then(Value::as_str) == Some("user")
                {
                    let text = payload.get("content").map(extract_text).unwrap_or_default();
                    if let Some(title) = title_candidate_from_user_message(&text) {
                        first_user_message = Some(title);
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

    // Extract last_active_at and summary from tail lines (reverse order)
    let mut last_active_at: Option<i64> = None;
    let mut summary: Option<String> = None;

    for line in tail.iter().rev() {
        let value: Value = match serde_json::from_str(line) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        if last_active_at.is_none() {
            last_active_at = value.get("timestamp").and_then(parse_timestamp_to_ms);
        }
        if summary.is_none() && value.get("type").and_then(Value::as_str) == Some("response_item") {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(Value::as_str) == Some("message") {
                    let text = payload.get("content").map(extract_text).unwrap_or_default();
                    if !text.trim().is_empty() {
                        summary = Some(text);
                    }
                }
            }
        }
        if last_active_at.is_some() && summary.is_some() {
            break;
        }
    }

    let session_id = session_id.or_else(|| infer_session_id_from_filename(path));
    let session_id = session_id?;

    let title = thread_titles
        .get(&session_id)
        .map(|t| truncate_summary(t, TITLE_MAX_CHARS))
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
        resume_command: Some(format!("codex resume {session_id}")),
    })
}

fn is_subagent_source(source: Option<&Value>) -> bool {
    source
        .and_then(|value| value.as_object())
        .map(|source| source.contains_key("subagent"))
        .unwrap_or(false)
}

fn title_candidate_from_user_message(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("# AGENTS.md")
        || trimmed.starts_with("<environment_context>")
    {
        return None;
    }

    if trimmed.starts_with(VSCODE_CONTEXT_PREFIX) {
        return extract_codex_prompt_from_ide_context(trimmed);
    }

    Some(trimmed.to_string())
}

fn extract_codex_prompt_from_ide_context(text: &str) -> Option<String> {
    let normalized = text.replace("\r\n", "\n");
    let lines = normalized.lines().collect::<Vec<_>>();

    // VS Code injects the real prompt as the LAST "## My request for Codex:"
    // section, so keep the final matching heading. Earlier matches can be
    // headings that live inside the active selection / open file content.
    // Trade-off: if the request body itself repeats the heading, the title
    // truncates to its trailing part (rare; covered by tests below).
    let mut prompt: Option<String> = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(inline_prompt) = codex_request_heading_payload(line) else {
            continue;
        };

        if !inline_prompt.is_empty() {
            prompt = Some(inline_prompt.to_string());
            continue;
        }

        let following_prompt = lines[index + 1..].join("\n").trim().to_string();
        prompt = (!following_prompt.is_empty()).then_some(following_prompt);
    }

    prompt
}

fn codex_request_heading_payload(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }

    let heading = trimmed.trim_start_matches('#').trim_start();
    let lowered = heading.to_ascii_lowercase();
    if !lowered.starts_with(CODEX_REQUEST_MARKER) {
        return None;
    }

    let suffix = heading[CODEX_REQUEST_MARKER.len()..].trim_start();
    if suffix.is_empty() {
        return Some("");
    }

    let Some(separator) = suffix.chars().next() else {
        return Some("");
    };
    if !matches!(separator, ':' | '：' | '-' | '—') {
        return None;
    }

    Some(
        suffix
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '：' | '-' | '—'))
            .trim(),
    )
}

fn infer_session_id_from_filename(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    UUID_RE.find(&file_name).map(|mat| mat.as_str().to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_state_db::CODEX_STATE_DB_FILENAME;
    use crate::session_manager::model::{DiffOp, ImageSource};
    use tempfile::tempdir;

    fn write_codex_session(path: &Path, session_id: &str, message: &str) {
        std::fs::write(
            path,
            format!(
                "{{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"/tmp/project\"}}}}\n\
                 {{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"{message}\"}}}}\n",
            ),
        )
        .expect("write session");
    }

    #[test]
    fn scan_sessions_in_roots_includes_active_and_archived_files() {
        let temp = tempdir().expect("tempdir");
        let active = temp.path().join("sessions");
        let archived = temp.path().join("archived_sessions");
        std::fs::create_dir_all(&active).expect("active dir");
        std::fs::create_dir_all(&archived).expect("archived dir");

        write_codex_session(&active.join("active.jsonl"), "active-id", "Active session");
        write_codex_session(
            &archived.join("archived.jsonl"),
            "archived-id",
            "Archived session",
        );

        let sessions = scan_sessions_in_roots(&[active, archived]);
        let ids = sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect::<Vec<_>>();

        assert!(ids.contains(&"active-id".to_string()));
        assert!(ids.contains(&"archived-id".to_string()));
    }

    #[test]
    fn delete_session_removes_jsonl_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("rollout-2026-03-06T21-50-12-019cc369-bd7c-7891-b371-7b20b4fe0b18.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019cc369-bd7c-7891-b371-7b20b4fe0b18\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}\n"
            ),
        )
        .expect("write session");

        delete_session(temp.path(), &path, "019cc369-bd7c-7891-b371-7b20b4fe0b18")
            .expect("delete session");

        assert!(!path.exists());
    }

    #[test]
    fn parse_session_uses_first_user_message_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"How do I deploy?\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Here is how...\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("How do I deploy?"));
    }

    #[test]
    fn parse_session_prefers_thread_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"How do I deploy?\"}}\n"
            ),
        )
        .expect("write");

        let mut thread_titles = HashMap::new();
        thread_titles.insert(
            "test-id".to_string(),
            "Renamed deployment thread".to_string(),
        );

        let meta = parse_session_with_titles(&path, &thread_titles).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Renamed deployment thread"));
    }

    #[test]
    fn load_thread_titles_from_state_db_trims_and_filters_titles() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open sqlite db");
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, first_user_message TEXT NOT NULL)",
            [],
        )
        .expect("create threads table");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            ("thread-1", "  Renamed Codex thread  ", "First prompt"),
        )
        .expect("insert renamed thread");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            ("thread-2", "   ", "First prompt"),
        )
        .expect("insert blank thread");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            ("thread-3", "  First prompt  ", "First prompt"),
        )
        .expect("insert first-message title");
        drop(conn);

        let titles = load_thread_titles_from_db(&db_path);

        assert_eq!(
            titles.get("thread-1").map(String::as_str),
            Some("Renamed Codex thread")
        );
        assert!(!titles.contains_key("thread-2"));
        assert!(!titles.contains_key("thread-3"));
    }

    #[test]
    fn load_thread_titles_from_state_db_keeps_title_when_first_user_message_null() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open sqlite db");
        // Codex stores first_user_message as a nullable column (Option<String>);
        // a renamed thread can have a title before any first message is synced.
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, first_user_message TEXT)",
            [],
        )
        .expect("create threads table");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, NULL)",
            ("thread-1", "Renamed thread"),
        )
        .expect("insert renamed thread without first message");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            ("thread-2", "First prompt", "First prompt"),
        )
        .expect("insert first-message title");
        drop(conn);

        let titles = load_thread_titles_from_db(&db_path);

        // Kept: title present and no first message to compare against.
        assert_eq!(
            titles.get("thread-1").map(String::as_str),
            Some("Renamed thread")
        );
        // Filtered: title equals the first user message.
        assert!(!titles.contains_key("thread-2"));
    }

    #[test]
    fn load_thread_titles_from_session_index_uses_latest_name() {
        let temp = tempdir().expect("tempdir");
        let index_path = temp.path().join(CODEX_SESSION_INDEX_FILENAME);
        std::fs::write(
            &index_path,
            concat!(
                "{\"id\":\"thread-1\",\"thread_name\":\"Old name\",\"updated_at\":\"2026-07-01T00:00:00Z\"}\n",
                "{\"id\":\"thread-2\",\"thread_name\":\"   \",\"updated_at\":\"2026-07-01T00:00:00Z\"}\n",
                "not json\n",
                "{\"id\":\"thread-1\",\"thread_name\":\"  New name  \",\"updated_at\":\"2026-07-02T00:00:00Z\"}\n"
            ),
        )
        .expect("write session index");

        let titles = load_thread_titles_from_session_index(&index_path);

        assert_eq!(titles.get("thread-1").map(String::as_str), Some("New name"));
        assert!(!titles.contains_key("thread-2"));
    }

    #[test]
    fn load_thread_titles_prefers_state_db_explicit_title_over_session_index() {
        let temp = tempdir().expect("tempdir");
        let index_path = temp.path().join(CODEX_SESSION_INDEX_FILENAME);
        std::fs::write(
            &index_path,
            concat!(
                "{\"id\":\"thread-1\",\"thread_name\":\"Legacy name\",\"updated_at\":\"2026-07-01T00:00:00Z\"}\n",
                "{\"id\":\"thread-2\",\"thread_name\":\"Legacy fallback\",\"updated_at\":\"2026-07-01T00:00:00Z\"}\n"
            ),
        )
        .expect("write session index");

        let db_path = temp.path().join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open sqlite db");
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT NOT NULL, first_user_message TEXT NOT NULL)",
            [],
        )
        .expect("create threads table");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            ("thread-1", "SQLite name", "First prompt"),
        )
        .expect("insert sqlite title");
        conn.execute(
            "INSERT INTO threads (id, title, first_user_message) VALUES (?1, ?2, ?3)",
            ("thread-2", "First prompt", "First prompt"),
        )
        .expect("insert first-message sqlite title");
        drop(conn);

        let titles = load_thread_titles_from_paths(&index_path, &[db_path]);

        assert_eq!(
            titles.get("thread-1").map(String::as_str),
            Some("SQLite name")
        );
        assert_eq!(
            titles.get("thread-2").map(String::as_str),
            Some("Legacy fallback")
        );
    }

    #[test]
    fn parse_session_skips_agents_md_injection() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":\"<permissions>\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# AGENTS.md instructions for /tmp/project\\n<INSTRUCTIONS>Do stuff</INSTRUCTIONS>\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"Fix the login bug\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        // Should skip AGENTS.md injection and use the real user message
        assert_eq!(meta.title.as_deref(), Some("Fix the login bug"));
    }

    #[test]
    fn parse_session_skips_subagent_sessions() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-04-28T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"subagent-id\",\"cwd\":\"/tmp/project\",\"originator\":\"codex-tui\",\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"parent-id\",\"depth\":1,\"agent_role\":\"explorer\"}}}}}\n",
                "{\"timestamp\":\"2026-04-28T10:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"Inspect the project\"}}\n"
            ),
        )
        .expect("write");

        assert!(parse_session(&path).is_none());
    }

    #[test]
    fn parse_session_skips_environment_context_injection() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"<environment_context>\\n  <cwd>/tmp/project</cwd>\\n</environment_context>\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"Fix the login bug\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        // Should skip environment_context injection and use the real user message
        assert_eq!(meta.title.as_deref(), Some("Fix the login bug"));
    }

    #[test]
    fn parse_session_extracts_vscode_ide_request_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# Context from my IDE setup:\\n\\n## Active file: src/main.ts\\n\\n## My request for Codex:\\nFix the session title preview\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Fix the session title preview"));
    }

    #[test]
    fn parse_session_extracts_inline_vscode_ide_request_as_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# Context from my IDE setup:\\n\\n## My request for Codex: Fix the TOC preview\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Fix the TOC preview"));
    }

    #[test]
    fn parse_session_ignores_marker_mentions_before_request_heading() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# Context from my IDE setup:\\n\\n## Active selection:\\nMy request for Codex: not the prompt\\n\\n## My request for Codex:\\nUse the real request heading\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Use the real request heading"));
    }

    #[test]
    fn parse_session_uses_last_request_heading_when_selection_has_one() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# Context from my IDE setup:\\n\\n## Active selection: docs/codex-format.md\\n## My request for Codex:\\nselected document content, not the real request\\n\\n## My request for Codex:\\nUse the last request heading\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Use the last request heading"));
    }

    // Known limitation: the IDE marker is matched purely by text, so a
    // "## My request for Codex:" line inside the real request body is treated as
    // a new boundary and only the trailing part is kept. This pins the
    // best-effort behavior; fully fixing it needs structured IDE section data
    // that the Codex VS Code context does not provide.
    #[test]
    fn parse_session_keeps_trailing_part_when_request_body_repeats_heading() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# Context from my IDE setup:\\n\\n## Active file: foo.ts\\n\\n## My request for Codex:\\nDocument the format, for example:\\n## My request for Codex:\\nand the rest follows.\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("and the rest follows."));
    }

    #[test]
    fn parse_session_skips_vscode_ide_context_without_request() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"# Context from my IDE setup:\\n\\n## Active file: src/main.ts\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"Fix the login bug\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Fix the login bug"));
    }

    #[test]
    fn parse_session_falls_back_to_dir_basename() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp/my-project\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Hello\"}}\n"
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        // No user message → falls back to dir basename
        assert_eq!(meta.title.as_deref(), Some("my-project"));
    }

    #[test]
    fn parse_session_truncates_long_title() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        let long_msg = "a".repeat(200);
        std::fs::write(
            &path,
            format!(
                "{{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"test-id\",\"cwd\":\"/tmp/p\"}}}}\n\
                 {{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"{long_msg}\"}}}}\n",
            ),
        )
        .expect("write");

        let meta = parse_session(&path).unwrap();
        let title = meta.title.unwrap();
        assert!(title.len() <= TITLE_MAX_CHARS + 3); // +3 for "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn load_messages_includes_function_call_and_output() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"test-id\",\"cwd\":\"/tmp\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"list files\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"shell\",\"arguments\":\"{\\\"cmd\\\":[\\\"ls\\\"]}\",\"call_id\":\"call_1\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:15Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call_1\",\"output\":\"file1.txt\\nfile2.txt\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:16Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Done.\"}]}}\n",
            ),
        )
        .expect("write");

        let msgs = load_messages(&path).expect("load");
        assert_eq!(msgs.len(), 4);

        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "list files");

        assert_eq!(msgs[1].role, "assistant");
        assert!(msgs[1].content.contains("[Tool: shell]"));

        assert_eq!(msgs[2].role, "tool");
        assert!(msgs[2].content.contains("file1.txt"));

        assert_eq!(msgs[3].role, "assistant");
        assert_eq!(msgs[3].content, "Done.");
    }

    // ─── load_messages：结构化 blocks（§4.2）────────────────────────────

    const T1: &str = "turn-1";
    const T2: &str = "turn-2";
    const T3: &str = "turn-3";
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    fn ts(sec: u32) -> String {
        format!("2026-10-03T08:00:{sec:02}.000Z")
    }

    fn line(sec: u32, ty: &str, payload: Value) -> String {
        serde_json::json!({ "timestamp": ts(sec), "ordinal": sec, "type": ty, "payload": payload })
            .to_string()
    }

    fn item(sec: u32, item: Value) -> String {
        line(
            sec,
            "event_msg",
            serde_json::json!({ "type": "item_completed", "turn_id": T1, "item": item }),
        )
    }

    /// 覆盖 §4.2 各映射的小型 rollout
    fn rollout_fixture() -> Vec<String> {
        use serde_json::json;
        let exec_read = "const r = await tools.exec_command({cmd:\"sed -n '1,80p' README.md\", workdir:\"/p\"});\ntext(r.output);";
        let exec_rg = "text((await tools.exec_command({cmd:\"rg -n \\\"--legacy\\\" docs\",\"max_output_tokens\":2000})).output)";
        let exec_multi = "text((await tools.exec_command({cmd:\"git status\"})).output);\ntext((await tools.exec_command({cmd:\"cargo test\"})).output);";
        let patch = "*** Begin Patch\n*** Update File: README.md\n@@\n ```bash\n-brew install demo\n-demo init --legacy\n+cargo install demo\n+demo init\n+demo doctor\n ```\n*** End Patch";
        vec![
            line(0, "session_meta", json!({ "id": "s1", "cwd": "/p" })),
            line(
                0,
                "event_msg",
                json!({ "type": "task_started", "turn_id": T1 }),
            ),
            line(
                0,
                "turn_context",
                json!({ "turn_id": T1, "model": "gpt-6.1", "cwd": "/p", "summary": "auto" }),
            ),
            line(
                0,
                "response_item",
                json!({ "type": "message", "id": "msg_dev", "role": "developer",
                "content": [{ "type": "input_text", "text": "<permissions instructions>\nsandbox\n</permissions instructions>" }] }),
            ),
            line(
                0,
                "response_item",
                json!({ "type": "message", "role": "user",
                "content": [{ "type": "input_text", "text": "# AGENTS.md instructions for /p\n\n<INSTRUCTIONS>\n- 中文\n</INSTRUCTIONS>" }] }),
            ),
            line(
                0,
                "response_item",
                json!({ "type": "message", "role": "user",
                "content": [{ "type": "input_text", "text": "<environment_context>\n  <cwd>/p</cwd>\n</environment_context>" }] }),
            ),
            line(
                2,
                "response_item",
                json!({ "type": "message", "id": "msg_user", "role": "user", "content": [
                { "type": "input_text", "text": "按截图更新 README" },
                { "type": "input_text", "text": "<image name=[Image #1] path=\"/tmp/a.png\">" },
                { "type": "input_image", "image_url": PNG, "detail": "high" },
                { "type": "input_text", "text": "</image>" },
            ] }),
            ),
            line(
                6,
                "response_item",
                json!({ "type": "reasoning", "id": "rs_1",
                "summary": [{ "type": "summary_text", "text": "**Inspecting README**" }], "encrypted_content": "gAAA" }),
            ),
            // exec + CommandExecution（parsed_cmd=read）
            line(
                8,
                "response_item",
                json!({ "type": "custom_tool_call", "id": "ctc_1", "status": "completed",
                "call_id": "call_read", "name": "exec", "input": exec_read }),
            ),
            item(
                8,
                json!({ "type": "CommandExecution", "id": "exec-1", "command": ["/bin/zsh", "-lc", "sed -n '1,80p' README.md"],
                "cwd": "file:///p", "parsed_cmd": [{ "type": "read", "cmd": "sed -n '1,80p' README.md", "name": "README.md" }],
                "source": "unified_exec_startup", "status": "completed", "exit_code": 0,
                "duration": { "secs": 0, "nanos": 118_000_000 }, "aggregated_output": "# demo\n", "formatted_output": "# demo\n" }),
            ),
            line(
                9,
                "response_item",
                json!({ "type": "custom_tool_call_output", "id": "ctco_1", "call_id": "call_read", "output": [
                { "type": "input_text", "text": "Script completed\nWall time 0.1 seconds\nOutput:\n" },
                { "type": "input_text", "text": "# demo\n\n## Install\n" },
            ] }),
            ),
            // exec 没有 CommandExecution：正则兜底取 cmd
            line(
                10,
                "response_item",
                json!({ "type": "custom_tool_call", "id": "ctc_2", "call_id": "call_rg", "name": "exec", "input": exec_rg }),
            ),
            line(
                11,
                "response_item",
                json!({ "type": "custom_tool_call_output", "id": "ctco_2", "call_id": "call_rg",
                "output": "Script completed\nWall time 0.042 seconds\nOutput:\n" }),
            ),
            // exec 一次跑两条命令：拆成 #1、#2，第二条失败
            line(
                12,
                "response_item",
                json!({ "type": "custom_tool_call", "id": "ctc_3", "call_id": "call_multi", "name": "exec", "input": exec_multi }),
            ),
            item(
                12,
                json!({ "type": "CommandExecution", "id": "exec-2", "command": ["/bin/zsh", "-lc", "git status"], "cwd": "file:///p",
                "parsed_cmd": [{ "type": "unknown", "cmd": "git status" }], "status": "completed", "exit_code": 0,
                "duration": { "secs": 0, "nanos": 5_000_000 }, "aggregated_output": "On branch main\n" }),
            ),
            item(
                13,
                json!({ "type": "CommandExecution", "id": "exec-3", "command": ["/bin/zsh", "-lc", "cargo test"], "cwd": "file:///p",
                "parsed_cmd": [{ "type": "unknown", "cmd": "cargo test" }], "status": "failed", "exit_code": 101,
                "duration": { "secs": 3, "nanos": 0 }, "aggregated_output": "test failed\n" }),
            ),
            line(
                13,
                "response_item",
                json!({ "type": "custom_tool_call_output", "id": "ctco_3", "call_id": "call_multi", "output": [
                { "type": "input_text", "text": "Script completed\nWall time 3.1 seconds\nOutput:\n" },
                { "type": "input_text", "text": "On branch main\ntest failed\n" },
            ] }),
            ),
            // 两条加密思考只留一条；apply_patch + FileChange
            line(
                14,
                "response_item",
                json!({ "type": "reasoning", "id": "rs_2", "summary": [], "encrypted_content": "gAAA" }),
            ),
            line(
                14,
                "response_item",
                json!({ "type": "reasoning", "id": "rs_3", "summary": [], "encrypted_content": "gAAB" }),
            ),
            line(
                15,
                "response_item",
                json!({ "type": "custom_tool_call", "id": "ctc_4", "call_id": "call_patch", "name": "apply_patch", "input": patch }),
            ),
            item(
                15,
                json!({ "type": "FileChange", "id": "exec-4", "changes": { "/p/README.md": { "type": "update",
                "unified_diff": "@@ -1,4 +1,5 @@\n ```bash\n-brew install demo\n-demo init --legacy\n+cargo install demo\n+demo init\n+demo doctor\n ```\n", "move_path": null } },
                "status": "completed", "stdout": "Success.", "stderr": "" }),
            ),
            line(
                16,
                "response_item",
                json!({ "type": "custom_tool_call_output", "id": "ctco_4", "call_id": "call_patch",
                "output": "Exit code: 0\nWall time: 0.064 seconds\nOutput:\nSuccess. Updated the following files:\nM README.md\n" }),
            ),
            // function_call namespace=mcp__* + McpToolCall + 输出里的图片
            line(
                20,
                "response_item",
                json!({ "type": "function_call", "id": "fc_1", "name": "js", "namespace": "mcp__cua_repl",
                "arguments": "{\"code\":\"await page.goto('http://localhost:4173');\",\"title\":\"打开页面\"}", "call_id": "call_mcp" }),
            ),
            item(
                22,
                json!({ "type": "McpToolCall", "id": "call_mcp", "server": "cua_repl", "tool": "js", "arguments": { "code": "x" },
                "status": "completed", "result": { "content": [{ "type": "text", "text": "Navigated" }], "isError": false },
                "duration": { "secs": 2, "nanos": 810_000_000 } }),
            ),
            line(
                23,
                "response_item",
                json!({ "type": "function_call_output", "id": "fco_1", "call_id": "call_mcp", "output": [
                { "type": "input_text", "text": "Wall time: 2.8 seconds\nOutput:\nNavigated to http://localhost:4173" },
                { "type": "input_image", "image_url": PNG },
            ] }),
            ),
            // web_search_call 没有 id：借用前面 WebSearch 项的 id
            item(
                25,
                json!({ "type": "WebSearch", "id": "ws_1", "query": "demo doctor", "action": { "type": "search", "query": "demo doctor" } }),
            ),
            line(
                26,
                "response_item",
                json!({ "type": "web_search_call", "status": "completed", "action": { "type": "search", "query": "demo doctor" } }),
            ),
            // 后台进程结束：没有可归属的调用，单独成块
            item(
                27,
                json!({ "type": "CommandExecution", "id": "exec-bg", "command": ["/bin/zsh", "-lc", "pnpm install"], "cwd": "file:///p",
                "parsed_cmd": [], "status": "completed", "exit_code": 0, "duration": { "secs": 9, "nanos": 0 }, "aggregated_output": "done\n" }),
            ),
            item(
                28,
                json!({ "type": "ImageView", "id": "exec-5", "path": "file:///p/shot.png" }),
            ),
            item(
                29,
                json!({ "type": "AgentMessage", "id": "msg_final", "content": [{ "type": "Text", "text": "已更新" }], "phase": "final_answer" }),
            ),
            line(
                30,
                "response_item",
                json!({ "type": "message", "id": "msg_final", "role": "assistant",
                "content": [{ "type": "output_text", "text": "README 已更新。" }] }),
            ),
            line(
                30,
                "token_usage_record",
                json!({ "turn_id": T1, "usage": { "input_tokens": 10 },
                "turn_token_usage": { "input_tokens": 48211, "cached_input_tokens": 40960, "cache_write_input_tokens": 0, "output_tokens": 1290, "reasoning_output_tokens": 512, "total_tokens": 49501 } }),
            ),
            line(
                31,
                "event_msg",
                json!({ "type": "task_complete", "turn_id": T1, "last_agent_message": "README 已更新。" }),
            ),
            // 第二轮：模型切换 + 中断
            line(
                40,
                "event_msg",
                json!({ "type": "thread_settings_applied", "thread_settings": { "model": "gpt-6.1-codex" } }),
            ),
            line(
                40,
                "event_msg",
                json!({ "type": "task_started", "turn_id": T2 }),
            ),
            line(
                41,
                "response_item",
                json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "再跑一遍 pnpm build" }] }),
            ),
            line(
                45,
                "response_item",
                json!({ "type": "function_call", "id": "fc_2", "name": "exec_command",
                "arguments": "{\"cmd\":\"pnpm build\",\"workdir\":\"/p/docs\"}", "call_id": "call_build" }),
            ),
            line(
                50,
                "event_msg",
                json!({ "type": "turn_aborted", "turn_id": T2, "reason": "interrupted", "duration_ms": 95000 }),
            ),
            // 第三轮：压缩、IDE 上下文提问、子代理消息
            line(
                55,
                "compacted",
                json!({ "message": "Summary: README updated.", "replacement_history": [{ "type": "message", "role": "user", "content": [] }] }),
            ),
            item(55, json!({ "type": "ContextCompaction", "id": "cmp-1" })),
            line(
                56,
                "event_msg",
                json!({ "type": "task_started", "turn_id": T3 }),
            ),
            line(
                57,
                "response_item",
                json!({ "type": "message", "role": "user", "content": [{ "type": "input_text",
                "text": "# Context from my IDE setup:\n\n## Open tabs:\n- README.md\n\n## My request for Codex:\n检查 docs 死链" }] }),
            ),
            line(
                58,
                "response_item",
                json!({ "type": "agent_message", "id": "amsg_1", "author": "/root/docs", "recipient": "/root",
                "content": [{ "type": "input_text", "text": "Found 2 broken links\ndocs/a.md:14" }, { "type": "encrypted_content", "encrypted_content": "gAAA" }] }),
            ),
        ]
    }

    /// 审查 #7825：Codex 0.119–0.128 的写入顺序是「调用 → 输出 → item_completed」，
    /// 迟到的 CommandExecution 不能再单独成一个步骤，只回填退出码和耗时
    #[test]
    fn late_item_completed_after_output_does_not_duplicate_the_command() {
        use serde_json::json;
        let lines = vec![
            line(0, "session_meta", json!({ "id": "s1", "cwd": "/p" })),
            line(
                0,
                "event_msg",
                json!({ "type": "task_started", "turn_id": T1 }),
            ),
            line(
                0,
                "response_item",
                json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "list files" }] }),
            ),
            line(
                1,
                "response_item",
                json!({ "type": "function_call", "id": "fc_1", "name": "exec_command",
                "arguments": "{\"cmd\":\"ls -la\",\"workdir\":\"/p\"}", "call_id": "call_ls" }),
            ),
            line(
                2,
                "response_item",
                json!({ "type": "function_call_output", "call_id": "call_ls", "output": "total 0\nREADME.md\n" }),
            ),
            item(
                3,
                json!({ "type": "CommandExecution", "id": "call_ls", "command": ["/bin/zsh", "-lc", "ls -la"], "cwd": "file:///p",
                "parsed_cmd": [{ "type": "unknown", "cmd": "ls -la" }], "status": "failed", "exit_code": 2,
                "duration": { "secs": 1, "nanos": 500_000_000 }, "aggregated_output": "total 0\nREADME.md\n" }),
            ),
        ];
        let (_temp, _path, msgs) = load_fixture(&lines);

        assert_eq!(calls(&msgs).len(), 1, "同一条命令只能有一个步骤");
        let results: Vec<_> = msgs
            .iter()
            .flat_map(|m| &m.blocks)
            .filter(|b| matches!(b, SessionBlock::ToolResult { .. }))
            .collect();
        assert_eq!(results.len(), 1);
        match find_result(&msgs, "call_ls") {
            SessionBlock::ToolResult {
                status,
                exit_code,
                duration_ms,
                ..
            } => {
                assert_eq!(*exit_code, Some(2), "退出码从迟到的 item_completed 回填");
                assert_eq!(*duration_ms, Some(1500));
                assert_eq!(
                    *status,
                    ToolStatus::Error,
                    "失败状态从迟到的 item_completed 回填"
                );
            }
            other => panic!("unexpected block {other:?}"),
        }
    }

    fn load_fixture(lines: &[String]) -> (tempfile::TempDir, PathBuf, Vec<SessionMessage>) {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("rollout.jsonl");
        std::fs::write(&path, lines.join("\n") + "\n").expect("write");
        let msgs = load_messages(&path).expect("load");
        (temp, path, msgs)
    }

    fn calls(msgs: &[SessionMessage]) -> Vec<&SessionBlock> {
        msgs.iter()
            .flat_map(|m| &m.blocks)
            .filter(|b| matches!(b, SessionBlock::ToolCall { .. }))
            .collect()
    }

    fn find_call<'m>(msgs: &'m [SessionMessage], call_id: &str) -> &'m SessionBlock {
        calls(msgs)
            .into_iter()
            .find(|b| matches!(b, SessionBlock::ToolCall { id, .. } if id == call_id))
            .unwrap_or_else(|| panic!("tool_call {call_id} 不存在"))
    }

    fn find_result<'m>(msgs: &'m [SessionMessage], id: &str) -> &'m SessionBlock {
        msgs.iter()
            .flat_map(|m| &m.blocks)
            .find(|b| matches!(b, SessionBlock::ToolResult { call_id, .. } if call_id == id))
            .unwrap_or_else(|| panic!("tool_result {id} 不存在"))
    }

    /// 按 ContentRef 回读原文件（与 P2 的读取约定一致：行区间 + JSON Pointer）
    fn resolve(path: &Path, content: &ContentRef) -> Value {
        let ContentRef::Jsonl {
            offset,
            len,
            pointer,
        } = content
        else {
            panic!("不是 jsonl 引用: {content:?}");
        };
        let bytes = std::fs::read(path).unwrap();
        let slice = &bytes[*offset as usize..(*offset + u64::from(*len)) as usize];
        assert!(
            offset == &0 || bytes[*offset as usize - 1] == b'\n',
            "起点不在行首"
        );
        assert!(
            slice.ends_with(b"\n") && !slice[..slice.len() - 1].contains(&b'\n'),
            "区间不是完整一行"
        );
        let value: Value = serde_json::from_slice(slice).unwrap();
        value
            .pointer(pointer)
            .cloned()
            .unwrap_or_else(|| panic!("pointer {pointer} 无法解析"))
    }

    #[test]
    fn load_messages_maps_messages_and_injections() {
        let (_tmp, path, msgs) = load_fixture(&rollout_fixture());

        // developer → system 注入；AGENTS.md / environment_context → user 注入
        assert_eq!((msgs[0].role.as_str(), msgs[0].injected), ("system", true));
        assert!(msgs[1].injected && msgs[1].content.starts_with("# AGENTS.md"));
        assert!(msgs[2].injected && msgs[2].content.starts_with("<environment_context>"));
        assert_eq!(msgs[0].turn_id.as_deref(), Some(T1));

        // 用户提问：文字 + 图片（剥掉 <image> 包装标签）
        let question = &msgs[3];
        assert_eq!((question.role.as_str(), question.injected), ("user", false));
        assert!(
            matches!(&question.blocks[0], SessionBlock::Text { text, .. } if text == "按截图更新 README")
        );
        let SessionBlock::Image { image } = &question.blocks[1] else {
            panic!("缺图片块: {:?}", question.blocks);
        };
        assert_eq!(image.media_type, "image/png");
        let ImageSource::Inline { content } = &image.source else {
            panic!("应为内联图片");
        };
        assert_eq!(resolve(&path, content), Value::String(PNG.to_string()));
        assert!(question.content.contains("[Image: image/png"));

        // reasoning summary 并入下一条工具调用消息
        let thinking = &msgs[4];
        assert!(matches!(
            &thinking.blocks[0],
            SessionBlock::Thinking { summary: Some(s), redacted: false, .. } if s == "**Inspecting README**"
        ));
        assert!(
            matches!(&thinking.blocks[1], SessionBlock::ToolCall { id, .. } if id == "call_read")
        );

        // IDE 上下文提取出真正的提问
        let ide = msgs
            .iter()
            .find(|m| m.role == "user" && m.turn_id.as_deref() == Some(T3))
            .unwrap();
        assert_eq!(ide.content, "检查 docs 死链");
        assert!(!ide.injected);

        // 最终回复挂上模型、token 与 phase
        let reply = msgs
            .iter()
            .find(|m| m.content == "README 已更新。")
            .unwrap();
        let meta = reply.meta.as_ref().unwrap();
        assert_eq!(meta.model.as_deref(), Some("gpt-6.1"));
        assert_eq!(
            (
                meta.input_tokens,
                meta.output_tokens,
                meta.cache_read_tokens,
                meta.cache_write_tokens,
                meta.reasoning_tokens
            ),
            (Some(48211), Some(1290), Some(40960), None, Some(512))
        );
        assert_eq!(meta.stop_reason.as_deref(), Some("final_answer"));

        // 子代理消息
        let sub = msgs.last().unwrap();
        assert!(matches!(
            &sub.blocks[0],
            SessionBlock::Event { kind: EventKind::SubAgent, text: Some(t), .. } if t == "/root/docs: Found 2 broken links"
        ));
        assert!(
            matches!(&sub.blocks[1], SessionBlock::Text { text, .. } if text == "docs/a.md:14")
        );
    }

    #[test]
    fn load_messages_pairs_exec_with_command_execution() {
        let (_tmp, path, msgs) = load_fixture(&rollout_fixture());

        let SessionBlock::ToolCall {
            raw_name,
            kind,
            title,
            detail,
            input_preview,
            ..
        } = find_call(&msgs, "call_read")
        else {
            unreachable!()
        };
        assert_eq!(raw_name, "exec");
        assert_eq!(*kind, ToolKind::Read, "parsed_cmd=read");
        assert_eq!(
            title, "sed -n '1,80p' README.md",
            "取 CommandExecution 命令并去掉 zsh -lc"
        );
        assert_eq!(detail.as_deref(), Some("/p"));
        assert!(input_preview.starts_with("const r = await tools.exec_command"));

        let SessionBlock::ToolResult {
            status,
            preview,
            exit_code,
            duration_ms,
            full,
            truncated,
            ..
        } = find_result(&msgs, "call_read")
        else {
            unreachable!()
        };
        assert_eq!(*status, ToolStatus::Success);
        assert_eq!(
            preview, "# demo\n\n## Install",
            "去掉 Script completed 输出头"
        );
        assert_eq!((*exit_code, *duration_ms), (Some(0), Some(118)));
        assert!(!truncated && full.is_none());

        // 结果消息独立成 tool 角色
        let result_msg = msgs
            .iter()
            .find(|m| m.blocks.iter().any(|b| matches!(b, SessionBlock::ToolResult { call_id, .. } if call_id == "call_read")))
            .unwrap();
        assert_eq!(result_msg.role, "tool");
        assert_eq!(result_msg.id.as_deref(), Some("ctco_1"));
        let _ = path;
    }

    #[test]
    fn load_messages_exec_falls_back_to_regex_and_splits_multi_commands() {
        let (_tmp, _path, msgs) = load_fixture(&rollout_fixture());

        // 正则兜底（D12）：rg 开头 → search
        let SessionBlock::ToolCall { kind, title, .. } = find_call(&msgs, "call_rg") else {
            unreachable!()
        };
        assert_eq!(
            (*kind, title.as_str()),
            (ToolKind::Search, "rg -n \"--legacy\" docs")
        );
        let SessionBlock::ToolResult {
            status,
            preview,
            duration_ms,
            ..
        } = find_result(&msgs, "call_rg")
        else {
            unreachable!()
        };
        assert_eq!(
            (*status, preview.as_str(), *duration_ms),
            (ToolStatus::Success, "", Some(42))
        );

        // 两条 CommandExecution → call_multi#1 / #2
        assert!(calls(&msgs)
            .iter()
            .all(|b| !matches!(b, SessionBlock::ToolCall { id, .. } if id == "call_multi")));
        let SessionBlock::ToolCall { title, .. } = find_call(&msgs, "call_multi#1") else {
            unreachable!()
        };
        assert_eq!(title, "git status");
        let SessionBlock::ToolCall { title, .. } = find_call(&msgs, "call_multi#2") else {
            unreachable!()
        };
        assert_eq!(title, "cargo test");
        let SessionBlock::ToolResult {
            status, preview, ..
        } = find_result(&msgs, "call_multi#1")
        else {
            unreachable!()
        };
        assert_eq!(
            (*status, preview.as_str()),
            (ToolStatus::Success, "On branch main")
        );
        let SessionBlock::ToolResult {
            status,
            exit_code,
            duration_ms,
            ..
        } = find_result(&msgs, "call_multi#2")
        else {
            unreachable!()
        };
        assert_eq!(
            (*status, *exit_code, *duration_ms),
            (ToolStatus::Error, Some(101), Some(3000))
        );

        // 没有调用可归属的 CommandExecution 单独成块；ImageView 成图片消息
        let SessionBlock::ToolCall {
            raw_name, title, ..
        } = find_call(&msgs, "exec-bg")
        else {
            unreachable!()
        };
        assert_eq!(
            (raw_name.as_str(), title.as_str()),
            ("CommandExecution", "pnpm install")
        );
        assert!(matches!(
            find_result(&msgs, "exec-bg"),
            SessionBlock::ToolResult {
                status: ToolStatus::Success,
                ..
            }
        ));
        assert!(msgs.iter().flat_map(|m| &m.blocks).any(|b| matches!(
            b,
            SessionBlock::Image { image } if matches!(&image.source, ImageSource::LocalFile { path } if path == "/p/shot.png")
        )));
    }

    #[test]
    fn load_messages_apply_patch_uses_file_change_diff() {
        let (_tmp, path, msgs) = load_fixture(&rollout_fixture());

        // 两条加密思考合并为一个 redacted 块，并入 apply_patch 调用消息
        let patch_msg = msgs
            .iter()
            .find(|m| {
                m.blocks
                    .iter()
                    .any(|b| matches!(b, SessionBlock::ToolCall { id, .. } if id == "call_patch"))
            })
            .unwrap();
        assert_eq!(patch_msg.blocks.len(), 2);
        assert!(
            matches!(&patch_msg.blocks[0], SessionBlock::Thinking { redacted: true, text, .. } if text.is_empty())
        );
        assert_eq!(patch_msg.content, "[Tool: apply_patch] README.md");

        let SessionBlock::ToolCall {
            kind, title, diff, ..
        } = find_call(&msgs, "call_patch")
        else {
            unreachable!()
        };
        assert_eq!((*kind, title.as_str()), (ToolKind::Edit, "README.md"));
        let diff = diff.as_ref().unwrap();
        assert_eq!((diff.added, diff.removed), (3, 2));
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "/p/README.md");
        assert_eq!(diff.files[0].op, DiffOp::Update);
        let full = resolve(&path, diff.full.as_ref().unwrap());
        assert!(full.as_str().unwrap().starts_with("@@ -1,4 +1,5 @@"));

        let SessionBlock::ToolResult {
            status,
            preview,
            exit_code,
            duration_ms,
            ..
        } = find_result(&msgs, "call_patch")
        else {
            unreachable!()
        };
        assert_eq!(*status, ToolStatus::Success);
        assert_eq!(
            preview,
            "Success. Updated the following files:\nM README.md"
        );
        assert_eq!((*exit_code, *duration_ms), (Some(0), Some(64)));
    }

    #[test]
    fn load_messages_maps_mcp_web_search_and_events() {
        let (_tmp, path, msgs) = load_fixture(&rollout_fixture());

        let SessionBlock::ToolCall {
            kind,
            title,
            server,
            detail,
            ..
        } = find_call(&msgs, "call_mcp")
        else {
            unreachable!()
        };
        assert_eq!((*kind, title.as_str()), (ToolKind::Mcp, "cua_repl.js"));
        assert_eq!(server.as_deref(), Some("cua_repl"));
        assert_eq!(
            detail.as_deref(),
            Some("await page.goto('http://localhost:4173');")
        );
        let SessionBlock::ToolResult {
            status,
            preview,
            duration_ms,
            images,
            ..
        } = find_result(&msgs, "call_mcp")
        else {
            unreachable!()
        };
        assert_eq!(*status, ToolStatus::Success);
        assert_eq!(preview, "Navigated to http://localhost:4173");
        assert_eq!(*duration_ms, Some(2800));
        assert_eq!(images.len(), 1);
        let ImageSource::Inline { content } = &images[0].source else {
            panic!("应为内联图片");
        };
        assert_eq!(resolve(&path, content), Value::String(PNG.to_string()));

        // web_search_call：调用 + 空结果在同一条消息
        let web = msgs
            .iter()
            .find(|m| {
                m.blocks
                    .iter()
                    .any(|b| matches!(b, SessionBlock::ToolCall { id, .. } if id == "ws_1"))
            })
            .unwrap();
        assert!(matches!(
            &web.blocks[..],
            [SessionBlock::ToolCall { kind: ToolKind::Web, title, raw_name, .. }, SessionBlock::ToolResult { status: ToolStatus::Success, .. }]
                if title == "demo doctor" && raw_name == "web_search"
        ));

        // 模型切换事件归到新一轮开头
        let change = msgs
            .iter()
            .position(|m| {
                matches!(
                    m.blocks.first(),
                    Some(SessionBlock::Event {
                        kind: EventKind::ModelChange,
                        ..
                    })
                )
            })
            .unwrap();
        assert_eq!(msgs[change].content, "gpt-6.1-codex");
        assert_eq!(msgs[change].turn_id.as_deref(), Some(T2));
        assert_eq!(msgs[change + 1].content, "再跑一遍 pnpm build");

        // turn_aborted：未完成的调用记为中断，再接 Aborted 事件
        let SessionBlock::ToolCall {
            kind,
            title,
            detail,
            ..
        } = find_call(&msgs, "call_build")
        else {
            unreachable!()
        };
        assert_eq!(
            (*kind, title.as_str(), detail.as_deref()),
            (ToolKind::Shell, "pnpm build", Some("/p/docs"))
        );
        assert!(matches!(
            find_result(&msgs, "call_build"),
            SessionBlock::ToolResult {
                status: ToolStatus::Interrupted,
                ..
            }
        ));
        let aborted = msgs
            .iter()
            .find(|m| {
                matches!(
                    m.blocks.first(),
                    Some(SessionBlock::Event {
                        kind: EventKind::Aborted,
                        ..
                    })
                )
            })
            .unwrap();
        assert_eq!(
            (aborted.role.as_str(), aborted.content.as_str()),
            ("system", "interrupted")
        );

        // compacted + ContextCompaction 只出一条压缩事件
        let compactions: Vec<_> = msgs
            .iter()
            .filter(|m| {
                matches!(
                    m.blocks.first(),
                    Some(SessionBlock::Event {
                        kind: EventKind::Compaction,
                        ..
                    })
                )
            })
            .collect();
        assert_eq!(compactions.len(), 1);
        assert_eq!(compactions[0].content, "Summary: README updated.");
    }

    /// 输出与契约 fixture 同形：能被模型无损往返，content 等于 blocks 投影，
    /// 且每种块用到的字段都在共享 fixture 出现过（或是可选的引用字段）。
    #[test]
    fn load_messages_output_matches_fixture_shape() {
        let (_tmp, _path, msgs) = load_fixture(&rollout_fixture());
        let json = serde_json::to_value(&msgs).unwrap();
        let round: Vec<SessionMessage> = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&round).unwrap(), json);
        for msg in &msgs {
            assert_eq!(msg.content, project_content(&msg.blocks));
            assert!(!msg.blocks.is_empty(), "Codex 消息都应有 blocks");
        }

        let mut known: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
        for raw in [
            include_str!("../../../../tests/fixtures/sessions/codex.messages.json"),
            include_str!("../../../../tests/fixtures/sessions/claude.messages.json"),
            include_str!("../../../../tests/fixtures/sessions/opencode.messages.json"),
        ] {
            let fixture: Value = serde_json::from_str(raw).unwrap();
            for block in fixture
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|m| m["blocks"].as_array().cloned().unwrap_or_default())
            {
                let ty = block["type"].as_str().unwrap().to_string();
                known
                    .entry(ty)
                    .or_default()
                    .extend(block.as_object().unwrap().keys().cloned());
            }
        }
        for block in json
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["blocks"].as_array().cloned().unwrap_or_default())
        {
            let ty = block["type"].as_str().unwrap();
            let keys = known
                .get(ty)
                .unwrap_or_else(|| panic!("fixture 里没有块类型 {ty}"));
            for key in block.as_object().unwrap().keys() {
                assert!(keys.contains(key), "{ty}.{key} 不在 fixture 字段集合里");
            }
        }
    }

    #[test]
    fn wrapped_tag_text_is_injected() {
        assert!(is_wrapped_in_tag(
            "<recommended_plugins>\nx\n</recommended_plugins>"
        ));
        assert!(is_wrapped_in_tag(
            "<in-app-browser-context src=\"a\">x</in-app-browser-context>"
        ));
        assert!(!is_wrapped_in_tag("<div> 为什么不渲染"));
        assert!(!is_wrapped_in_tag("修一下 <b>x</b>"));
        assert!(!is_wrapped_in_tag("<a>x</b>"));
    }

    #[test]
    fn load_messages_handles_legacy_shell_output_and_truncation() {
        use serde_json::json;
        let long: String = (1..=40).map(|i| format!("line {i}\n")).collect();
        let legacy =
            json!({ "output": long, "metadata": { "exit_code": 2, "duration_seconds": 1.5 } })
                .to_string();
        let lines = vec![
            line(1, "response_item", json!({ "type": "message", "role": "user", "content": "run it" })),
            line(2, "response_item", json!({ "type": "function_call", "name": "shell",
                "arguments": "{\"command\":[\"bash\",\"-lc\",\"make\"]}", "call_id": "c1" })),
            line(3, "response_item", json!({ "type": "function_call_output", "call_id": "c1", "output": legacy })),
            // 键序不同的行：嗅探失败后走完整解析
            json!({ "payload": { "role": "assistant", "type": "message", "content": [{ "type": "output_text", "text": "ok" }] },
                "type": "response_item", "timestamp": ts(4) }).to_string(),
            "not json".to_string(),
        ];
        let (_tmp, path, msgs) = load_fixture(&lines);
        assert_eq!(msgs.len(), 4);
        let SessionBlock::ToolCall { title, .. } = find_call(&msgs, "c1") else {
            unreachable!()
        };
        assert_eq!(title, "make");
        let SessionBlock::ToolResult {
            status,
            exit_code,
            duration_ms,
            truncated,
            line_count,
            full,
            preview,
            ..
        } = find_result(&msgs, "c1")
        else {
            unreachable!()
        };
        assert_eq!(
            (*status, *exit_code, *duration_ms),
            (ToolStatus::Error, Some(2), Some(1500))
        );
        assert!(*truncated);
        assert_eq!(*line_count, 40);
        assert_eq!(preview.lines().count(), 12);
        assert!(resolve(&path, full.as_ref().unwrap())
            .as_str()
            .unwrap()
            .contains("line 40"));
        assert_eq!(msgs[3].content, "ok");
    }
}
