//! 各解析器共用的 block 构造工具：工具名归一化（§3.3）、预览截断、标题提炼。
//!
//! 这里只放与具体 Agent 无关的规则；需要上下文的细化（Codex parsed_cmd、
//! apply_patch 的增删、Claude structuredPatch 等）由各解析器在拿到
//! [`normalize_tool`] 的结果后自行修正。

use serde_json::Value;

use crate::session_manager::model::ToolKind;

/// 工具结果预览的最大行数
pub const PREVIEW_LINES: usize = 12;
/// 工具结果预览的最大字符数
pub const PREVIEW_CHARS: usize = 1200;
/// 工具参数预览的最大字符数（标题 / detail 已提炼出要点，参数只需看个开头；
/// 大会话里 Bash 参数预览曾占 payload 的五分之一）
pub const INPUT_PREVIEW_CHARS: usize = 400;
/// 思考正文预览的最大字符数
pub const THINKING_PREVIEW_CHARS: usize = 400;
/// 工具标题的最大字符数
pub const TITLE_CHARS: usize = 200;
/// Text 块超过该字符数时只下发预览 + 引用（注入的 AGENTS.md / developer / task-notification 等）
pub const INLINE_TEXT_MAX_CHARS: usize = 8 * 1024;
/// 压缩摘要等长事件说明的预览字符数
pub const EVENT_PREVIEW_CHARS: usize = 400;

/// 工具调用来自哪家 Agent；同名工具在不同 Agent 下归类可能不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    Claude,
    Codex,
    Gemini,
    OpenCode,
    /// Pi 与 OpenClaw（同构）
    Pi,
    /// Hermes / Grok Build 等：按名称关键字推断
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedTool {
    pub kind: ToolKind,
    /// MCP 服务器名（kind = Mcp 时）
    pub server: Option<String>,
}

impl NormalizedTool {
    fn of(kind: ToolKind) -> Self {
        Self { kind, server: None }
    }

    fn mcp(server: impl Into<String>) -> Self {
        Self {
            kind: ToolKind::Mcp,
            server: Some(server.into()),
        }
    }
}

/// 按 §3.3 的映射表把原始工具名归一化为 [`ToolKind`]。
///
/// `namespace` 只有 Codex `function_call.namespace` 会传（如 `mcp__cua_repl`）。
pub fn normalize_tool(
    source: ToolSource,
    raw_name: &str,
    namespace: Option<&str>,
) -> NormalizedTool {
    // MCP：Codex 看 namespace，其余看 `mcp__<server>__<tool>` 名称
    if let Some(server) = namespace.and_then(|ns| ns.strip_prefix("mcp__")) {
        return NormalizedTool::mcp(server.trim_end_matches('_'));
    }
    if let Some((server, _)) = split_mcp_name(raw_name) {
        return NormalizedTool::mcp(server);
    }

    use ToolKind::*;
    let kind = match source {
        ToolSource::Claude => match raw_name {
            "Bash" => Shell,
            "Read" => Read,
            "Grep" | "Glob" | "ToolSearch" | "ListAgents" => Search,
            "Edit" | "MultiEdit" | "NotebookEdit" => Edit,
            "Write" => Write,
            "WebFetch" | "WebSearch" => Web,
            "Agent" | "Task" | "SendMessage" | "TaskStop" => Agent,
            "AskUserQuestion" => Ask,
            "TodoWrite" => Todo,
            _ => Other,
        },
        ToolSource::Codex => match raw_name {
            "exec" | "exec_command" | "shell" | "local_shell" | "write_stdin"
            | "CommandExecution" => Shell,
            "read_file" => Read,
            // apply_patch / FileChange 默认按 Edit；全是新增文件时由解析器改为 Write
            "apply_patch" | "FileChange" => Edit,
            "write_file" => Write,
            "web_search" | "web_search_call" | "open_page" | "WebSearch" => Web,
            "McpToolCall" => Mcp,
            "spawn_agent" | "send_message" | "agent_message" | "SubAgentActivity"
            | "create_thread" => Agent,
            "request_user_input_async" => Ask,
            "update_plan" => Todo,
            name if name.starts_with("web.") => Web,
            _ => Other,
        },
        ToolSource::Gemini => match raw_name {
            "run_shell_command" => Shell,
            "read_file" | "read_many_files" => Read,
            "grep_search" | "glob" | "list_directory" | "search_file_content" => Search,
            "replace" | "edit" => Edit,
            "write_file" => Write,
            "web_fetch" | "google_web_search" => Web,
            "ask_user" => Ask,
            "write_todos" => Todo,
            // 待核实：Gemini CLI 的 MCP 工具名形如 `server/tool`
            name if name.contains('/') => {
                let server = name.split('/').next().unwrap_or_default();
                return NormalizedTool::mcp(server);
            }
            _ => Other,
        },
        ToolSource::OpenCode => match raw_name {
            "bash" => Shell,
            "read" => Read,
            "grep" | "glob" | "list" | "ast_grep_search" | "codesearch" => Search,
            "edit" | "patch" => Edit,
            "write" => Write,
            "webfetch" | "websearch" => Web,
            "task" | "background_output" | "background_cancel" => Agent,
            "question" => Ask,
            "todowrite" | "todoread" => Todo,
            "invalid" => Other,
            // 待核实：OpenCode 的 MCP 工具名形如 `server_tool`（内置表之外含 `_` 的名字）
            name if name.contains('_') => {
                let server = name.split('_').next().unwrap_or_default();
                return NormalizedTool::mcp(server);
            }
            _ => Other,
        },
        ToolSource::Pi => match raw_name {
            "bash" | "bashExecution" => Shell,
            "read" => Read,
            "grep" | "find" | "ls" => Search,
            "edit" => Edit,
            "write" => Write,
            "fetch" => Web,
            _ => Other,
        },
        ToolSource::Generic => generic_kind(raw_name),
    };
    NormalizedTool::of(kind)
}

/// Hermes / Grok Build：没有固定工具表，按名称关键字推断。
fn generic_kind(raw_name: &str) -> ToolKind {
    let name = raw_name.to_ascii_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| name.contains(k));
    if name.starts_with("web") {
        ToolKind::Web
    } else if has(&["shell", "bash", "exec", "terminal"]) {
        ToolKind::Shell
    } else if name.starts_with("read") {
        ToolKind::Read
    } else if has(&["grep", "glob", "search"]) {
        ToolKind::Search
    } else if has(&["edit", "patch"]) {
        ToolKind::Edit
    } else if name.starts_with("write") {
        ToolKind::Write
    } else {
        ToolKind::Other
    }
}

/// 拆 `mcp__<server>__<tool>`；不是这种形式返回 `None`。
pub fn split_mcp_name(raw_name: &str) -> Option<(&str, &str)> {
    let rest = raw_name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// Shell 命令按首个程序名细化：`rg`/`grep`/`fd`/`find`/`ls` 开头算搜索，其余仍是 Shell。
pub fn refine_shell_kind(command: &str) -> ToolKind {
    let program = command
        .split_whitespace()
        .next()
        .map(|p| p.rsplit('/').next().unwrap_or(p))
        .unwrap_or_default();
    match program {
        "rg" | "grep" | "fd" | "find" | "ls" => ToolKind::Search,
        _ => ToolKind::Shell,
    }
}

/// Codex `CommandExecution.parsed_cmd[].type` → kind（`unknown` 返回 `None`，保持 Shell）
pub fn kind_from_codex_parsed_cmd(parsed_type: &str) -> Option<ToolKind> {
    match parsed_type {
        "read" => Some(ToolKind::Read),
        "search" | "list" | "list_files" => Some(ToolKind::Search),
        _ => None,
    }
}

// ─── 预览 ────────────────────────────────────────────────────────────────

/// 截断后的预览与全文统计（长度均按字符计）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    /// 全文字符数
    pub total_len: u32,
    /// 全文行数（空文本为 0；末尾换行不额外计一行）
    pub line_count: u32,
    /// 预览是否短于全文（忽略末尾换行）
    pub truncated: bool,
}

/// 工具结果预览：前 [`PREVIEW_LINES`] 行且不超过 [`PREVIEW_CHARS`] 字符。
pub fn preview(text: &str) -> Preview {
    preview_with(text, PREVIEW_LINES, PREVIEW_CHARS)
}

/// 只按字符截断（参数、思考正文）。
pub fn preview_chars(text: &str, max_chars: usize) -> Preview {
    preview_with(text, usize::MAX, max_chars)
}

pub fn preview_with(text: &str, max_lines: usize, max_chars: usize) -> Preview {
    let total_len = saturating_u32(text.chars().count());
    let line_count = saturating_u32(text.lines().count());

    let mut end = text.len();
    if max_lines > 0 {
        if let Some((idx, _)) = text.match_indices('\n').nth(max_lines - 1) {
            end = idx;
        }
    } else {
        end = 0;
    }
    if let Some((idx, _)) = text[..end].char_indices().nth(max_chars) {
        end = idx;
    }

    let truncated = !text[end..].trim_end_matches(['\r', '\n']).is_empty();
    let kept = text[..end].trim_end_matches(['\r', '\n']);
    Preview {
        text: kept.to_string(),
        total_len,
        line_count,
        truncated,
    }
}

pub(super) fn saturating_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// base64 长度 → 解码后字节数估算（`ImageRef.size`）
pub fn estimate_base64_size(base64_len: usize) -> u32 {
    saturating_u32(base64_len / 4 * 3)
}

// ─── 标题提炼（§3.3）：输出 ≤ TITLE_CHARS，多行取首行并加 `…` ─────────────────

/// 截成单行标题：取首个非空行，超过 [`TITLE_CHARS`] 或有后续行时加 `…`。
pub fn one_line_title(text: &str) -> String {
    let trimmed = text.trim();
    let mut lines = trimmed.lines();
    let first = lines.next().unwrap_or_default().trim_end();
    let more_lines = lines.any(|line| !line.trim().is_empty());
    let mut title: String = first.chars().take(TITLE_CHARS).collect();
    if more_lines || first.chars().count() > TITLE_CHARS {
        title.push('…');
    }
    title
}

/// shell：命令文本首行。Codex 的 shell 包装去除、exec JS 中取 cmd 等由 P1b 补充。
pub fn title_shell(command: &str) -> String {
    one_line_title(command)
}

/// read：标题为路径；`detail` 为行范围 `:start-end`（offset 从 1 起算，limit 为行数）。
pub fn title_read(path: &str, offset: Option<u64>, limit: Option<u64>) -> (String, Option<String>) {
    let detail = match (offset, limit) {
        (Some(start), Some(limit)) if limit > 0 => Some(format!(":{start}-{}", start + limit - 1)),
        (Some(start), None) => Some(format!(":{start}")),
        (None, Some(limit)) if limit > 0 => Some(format!(":1-{limit}")),
        _ => None,
    };
    (one_line_title(path), detail)
}

/// search：`pattern in path`（没有 path 时只给 pattern）。
pub fn title_search(pattern: &str, path: Option<&str>) -> String {
    match path.filter(|p| !p.is_empty()) {
        Some(path) => one_line_title(&format!("{pattern} in {path}")),
        None => one_line_title(pattern),
    }
}

/// edit / write：路径。
pub fn title_path(path: &str) -> String {
    one_line_title(path)
}

/// web：URL 或 query；多个用 `, ` 连接。
pub fn title_web(items: &[&str]) -> String {
    one_line_title(&items.join(", "))
}

/// mcp：`server.tool`。
pub fn title_mcp(server: &str, tool: &str) -> String {
    one_line_title(&format!("{server}.{tool}"))
}

/// agent：description / task_name。
pub fn title_agent(description: &str) -> String {
    one_line_title(description)
}

/// ask：首个问题文本。
pub fn title_ask(question: &str) -> String {
    one_line_title(question)
}

/// todo：条目数。
pub fn title_todo(count: usize) -> String {
    format!("{count} 项")
}

/// other：原名。
pub fn title_other(raw_name: &str) -> String {
    one_line_title(raw_name)
}

/// 参数里第一个非空字符串字段（mcp / other 的 `detail`）。
pub fn first_string_field(input: &Value) -> Option<String> {
    input
        .as_object()?
        .values()
        .find_map(|v| v.as_str().filter(|s| !s.trim().is_empty()))
        .map(one_line_title)
}

// ─── 由 JSON 参数构造 block（Gemini / OpenCode / Pi / OpenClaw / Hermes / Grok 共用）────

use crate::session_manager::model::{
    ContentRef, DiffFile, DiffOp, DiffSummary, EventKind, SessionBlock, SessionMessage, ToolStatus,
};

/// 参数里第一个存在的非空字符串字段。
pub fn str_field<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .filter(|s| !s.trim().is_empty())
}

/// 参数里第一个存在的非负整数字段（兼容写成字符串的数字）。
fn u64_field(input: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        let value = input.get(*key)?;
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
    })
}

/// OpenAI 形状的 `arguments` 是 JSON 字符串：能解析成对象/数组就解析，否则保持原样。
pub fn parse_arguments(raw: &Value) -> Value {
    match raw {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(parsed @ (Value::Object(_) | Value::Array(_))) => parsed,
            _ => raw.clone(),
        },
        other => other.clone(),
    }
}

/// 工具参数预览：字符串原样，其余序列化为紧凑 JSON；空参数为空串。
pub fn input_preview(input: &Value) -> Preview {
    match input {
        Value::Null => preview_chars("", INPUT_PREVIEW_CHARS),
        Value::String(text) => preview_chars(text, INPUT_PREVIEW_CHARS),
        other => preview_chars(&other.to_string(), INPUT_PREVIEW_CHARS),
    }
}

const PATH_KEYS: &[&str] = &[
    "file_path",
    "filePath",
    "path",
    "absolute_path",
    "file",
    "filename",
    "dir_path",
];

/// 按 kind 从参数里提炼 `(title, detail)`（§3.3）。找不到对应字段时 title 为空，
/// 投影退化为 `[Tool: name]`。
pub fn title_from_input(
    kind: ToolKind,
    raw_name: &str,
    server: Option<&str>,
    input: &Value,
) -> (String, Option<String>) {
    let detail_of = |keys: &[&str]| str_field(input, keys).map(one_line_title);
    match kind {
        ToolKind::Shell => (
            str_field(input, &["command", "cmd", "script"])
                .map(title_shell)
                .unwrap_or_default(),
            detail_of(&["description"]),
        ),
        ToolKind::Read => {
            if let Some(path) = str_field(input, PATH_KEYS) {
                title_read(
                    path,
                    u64_field(input, &["offset", "start_line"]),
                    u64_field(input, &["limit"]),
                )
            } else {
                let paths: Vec<&str> = input
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                (title_web(&paths), None)
            }
        }
        ToolKind::Search => {
            let path = str_field(input, &["path", "dir_path", "directory", "cwd"]);
            match str_field(input, &["pattern", "query", "regex", "glob", "include"]) {
                Some(pattern) => (title_search(pattern, path), None),
                None => (path.map(title_path).unwrap_or_default(), None),
            }
        }
        ToolKind::Edit | ToolKind::Write => (
            str_field(input, PATH_KEYS)
                .map(title_path)
                .unwrap_or_default(),
            None,
        ),
        ToolKind::Web => {
            let mut items: Vec<&str> = ["urls", "queries"]
                .iter()
                .filter_map(|key| input.get(*key).and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if items.is_empty() {
                items.extend(str_field(input, &["url", "query", "prompt"]));
            }
            (title_web(&items), None)
        }
        ToolKind::Mcp => {
            let server = server.unwrap_or_default();
            let tool = split_mcp_name(raw_name)
                .map(|(_, tool)| tool)
                .or_else(|| {
                    raw_name
                        .strip_prefix(server)
                        .map(|rest| rest.trim_start_matches(['_', '/', '.']))
                })
                .filter(|tool| !tool.is_empty())
                .unwrap_or(raw_name);
            (title_mcp(server, tool), first_string_field(input))
        }
        ToolKind::Agent => (
            str_field(input, &["description", "task_name", "prompt", "message"])
                .map(title_agent)
                .unwrap_or_default(),
            detail_of(&["subagent_type", "agent", "model"]),
        ),
        ToolKind::Ask => {
            let question = input
                .get("questions")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|first| {
                    first
                        .get("question")
                        .and_then(Value::as_str)
                        .or_else(|| first.as_str())
                })
                .or_else(|| str_field(input, &["question", "prompt"]));
            (question.map(title_ask).unwrap_or_default(), None)
        }
        ToolKind::Todo => {
            let count = input.get("todos").and_then(Value::as_array).map(Vec::len);
            (count.map(title_todo).unwrap_or_default(), None)
        }
        ToolKind::Other => (title_other(raw_name), first_string_field(input)),
    }
}

/// 文本行数（空文本为 0）。
fn text_lines(text: &str) -> u32 {
    saturating_u32(text.lines().count())
}

/// 统计 unified diff（或带行号的 `+12 xxx` 形式）里的增删行数，跳过 `+++`/`---` 文件头。
pub fn count_diff_lines(diff: &str) -> (u32, u32) {
    let (mut added, mut removed) = (0u32, 0u32);
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            added = added.saturating_add(1);
        } else if line.starts_with('-') {
            removed = removed.saturating_add(1);
        }
    }
    (added, removed)
}

/// 单文件改动摘要。
pub fn single_file_diff(path: &str, op: DiffOp, added: u32, removed: u32) -> DiffSummary {
    DiffSummary {
        files: vec![DiffFile {
            path: path.to_string(),
            op,
            added,
            removed,
        }],
        added,
        removed,
        full: None,
    }
}

/// 只凭参数估算 Edit/Write 的改动（`old/new` 字符串按行数计、`edits[]` 求和、`content` 视为新增）。
/// 拿到工具自己给的 diff 时应以那个为准。
pub fn diff_from_input(kind: ToolKind, input: &Value) -> Option<DiffSummary> {
    let path = str_field(input, PATH_KEYS)?;
    let pair = |item: &Value| -> Option<(u32, u32)> {
        let old = str_field_allow_empty(item, &["old_string", "oldString", "old_str", "oldText"])?;
        let new = str_field_allow_empty(item, &["new_string", "newString", "new_str", "newText"])?;
        Some((text_lines(new), text_lines(old)))
    };
    match kind {
        ToolKind::Edit => {
            let (added, removed) = if let Some(edits) = input.get("edits").and_then(Value::as_array)
            {
                edits
                    .iter()
                    .filter_map(pair)
                    .fold((0u32, 0u32), |(a, r), (x, y)| {
                        (a.saturating_add(x), r.saturating_add(y))
                    })
            } else {
                pair(input)?
            };
            Some(single_file_diff(path, DiffOp::Update, added, removed))
        }
        ToolKind::Write => {
            let content = str_field_allow_empty(input, &["content", "contents", "text"])?;
            Some(single_file_diff(path, DiffOp::Add, text_lines(content), 0))
        }
        _ => None,
    }
}

fn str_field_allow_empty<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
}

/// OpenAI 形状的 `tool_calls[]`（`{id, function{name, arguments}}`，也兼容扁平 `{id, name, arguments}`）
/// → ToolCall。`arguments_ref(pointer)` 为参数全文生成引用，`pointer` 是参数在 `tool_calls`
/// 数组内的 JSON Pointer（`/{i}/function/arguments` 或扁平形状的 `/{i}/arguments`）。
pub(super) fn openai_tool_calls(
    calls: Option<&Value>,
    arguments_ref: impl Fn(String) -> Option<ContentRef>,
) -> Vec<SessionBlock> {
    calls
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, call)| {
            let name = call
                .pointer("/function/name")
                .or_else(|| call.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
            let (pointer, raw) = match call.pointer("/function/arguments") {
                Some(raw) => (format!("/{i}/function/arguments"), Some(raw)),
                None => (format!("/{i}/arguments"), call.get("arguments")),
            };
            let input = raw.map(parse_arguments).unwrap_or(Value::Null);
            tool_call_block(ToolSource::Generic, id, name, &input, || {
                arguments_ref(pointer)
            })
        })
        .collect()
}

/// 由参数构造 `ToolCall`：归一化 kind、提炼标题、参数预览；参数超长时调用 `input_full` 生成引用。
/// Edit/Write 先按参数估算 diff，解析器拿到更准的数据再覆盖。
pub fn tool_call_block(
    source: ToolSource,
    id: impl Into<String>,
    raw_name: &str,
    input: &Value,
    input_full: impl FnOnce() -> Option<ContentRef>,
) -> SessionBlock {
    let NormalizedTool { kind, server } = normalize_tool(source, raw_name, None);
    let (title, detail) = title_from_input(kind, raw_name, server.as_deref(), input);
    let preview = input_preview(input);
    SessionBlock::ToolCall {
        id: id.into(),
        raw_name: raw_name.to_string(),
        kind,
        title,
        detail,
        server,
        input_preview: preview.text,
        input_total_len: preview.total_len,
        input_full: if preview.truncated {
            input_full()
        } else {
            None
        },
        diff: diff_from_input(kind, input),
        by_user: false,
    }
}

/// 构造 `ToolResult`：预览 12 行 / 1200 字，超出时调用 `full` 生成引用。
/// `exit_code` / `duration_ms` / `images` / `saved_path` 由解析器按需补上。
pub fn tool_result_block(
    call_id: impl Into<String>,
    status: ToolStatus,
    text: &str,
    full: impl FnOnce() -> Option<ContentRef>,
) -> SessionBlock {
    let p = preview(text);
    SessionBlock::ToolResult {
        call_id: call_id.into(),
        status,
        preview: p.text,
        total_len: p.total_len,
        line_count: p.line_count,
        truncated: p.truncated,
        full: if p.truncated { full() } else { None },
        exit_code: None,
        duration_ms: None,
        images: Vec::new(),
        saved_path: None,
    }
}

/// 构造 `Thinking`：正文按 [`THINKING_PREVIEW_CHARS`] 截断，超出时调用 `full` 生成引用；
/// 正文为空视为不可见（redacted）。
pub fn thinking_block(
    text: &str,
    summary: Option<String>,
    duration_ms: Option<u64>,
    full: impl FnOnce() -> Option<ContentRef>,
) -> SessionBlock {
    let p = preview_chars(text.trim_end(), THINKING_PREVIEW_CHARS);
    SessionBlock::Thinking {
        redacted: text.trim().is_empty(),
        text: p.text,
        summary,
        duration_ms,
        full: if p.truncated { full() } else { None },
    }
}

/// 构造 `Text`：超过 [`INLINE_TEXT_MAX_CHARS`] 且能给出引用时只放预览（12 行 / 1200 字）+ `full`。
/// 只用于默认折叠的内容（注入文本）；真人提问与最终回复要完整渲染 Markdown，不走这里。
pub fn large_text_block(
    text: impl Into<String>,
    full: impl FnOnce() -> Option<ContentRef>,
) -> SessionBlock {
    let text = text.into();
    if text.chars().nth(INLINE_TEXT_MAX_CHARS).is_none() {
        return SessionBlock::text(text);
    }
    match full() {
        Some(full) => SessionBlock::Text {
            text: preview(&text).text,
            full: Some(full),
        },
        None => SessionBlock::text(text),
    }
}

/// 构造带长说明的 `Event`（压缩摘要）：超过 [`EVENT_PREVIEW_CHARS`] 且能给出引用时只放预览 + `full`；
/// 拿不到引用时退回带 `…` 的预览，避免把整段摘要塞进 payload。
pub fn summary_event_block(
    kind: EventKind,
    text: &str,
    full: impl FnOnce() -> Option<ContentRef>,
) -> SessionBlock {
    let text = text.trim();
    let p = preview_chars(text, EVENT_PREVIEW_CHARS);
    if !p.truncated {
        return SessionBlock::event(kind, Some(p.text), None);
    }
    match full() {
        Some(full) => SessionBlock::Event {
            kind,
            text: Some(p.text),
            url: None,
            full: Some(full),
        },
        None => SessionBlock::event(kind, Some(format!("{}…", p.text)), None),
    }
}

/// 这条消息是否开启新一轮：非注入的 user 消息，且带真人输入——非空文本、图片或斜杠命令。
/// 只有中断事件、用户自己跑的命令（Claude `!cmd`、Pi bashExecution）或工具结果的
/// user 消息不开新轮，归入当前轮。旧格式消息（无 blocks）看 `content` 是否非空。
pub fn starts_turn(message: &SessionMessage) -> bool {
    if message.role != "user" || message.injected {
        return false;
    }
    if message.blocks.is_empty() {
        return !message.content.trim().is_empty();
    }
    message.blocks.iter().any(|block| match block {
        SessionBlock::Text { text, .. } => !text.trim().is_empty(),
        SessionBlock::Image { .. } => true,
        SessionBlock::Event { kind, .. } => *kind == EventKind::SlashCommand,
        _ => false,
    })
}

/// 没有原生轮次概念的解析器共用（§4）：按 [`starts_turn`] 递增生成 `t{n}`，
/// 首条提问之前的内容归 `t0`。已有 `turn_id` 的消息保持不变。
pub fn assign_turn_ids(messages: &mut [SessionMessage]) {
    let mut turn = 0u32;
    for message in messages {
        if starts_turn(message) {
            turn += 1;
        }
        if message.turn_id.is_none() {
            message.turn_id = Some(format!("t{turn}"));
        }
    }
}

// ─── diff 统计 ───────────────────────────────────────────────────────────

/// 由替换前后的文本估算 `(+added, -removed)` 行数：去掉首尾相同的行后，
/// 剩余的旧行算删除、新行算新增。没有结构化 patch 时（如 Claude Edit 的参数）用。
pub fn line_change_counts(old: &str, new: &str) -> (u32, u32) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (
        saturating_u32(new_lines.len() - prefix - suffix),
        saturating_u32(old_lines.len() - prefix - suffix),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn line_change_counts_ignores_shared_context() {
        assert_eq!(line_change_counts("a\nb\nc", "a\nb\nx\nc"), (1, 0));
        assert_eq!(line_change_counts("a\nb", "a\nc\nd"), (2, 1));
        assert_eq!(line_change_counts("", "x\ny"), (2, 0));
        assert_eq!(line_change_counts("same", "same"), (0, 0));
    }

    fn kind(source: ToolSource, name: &str) -> ToolKind {
        normalize_tool(source, name, None).kind
    }

    #[test]
    fn normalize_tool_claude() {
        use ToolKind::*;
        let cases = [
            ("Bash", Shell),
            ("Read", Read),
            ("Grep", Search),
            ("Glob", Search),
            ("ToolSearch", Search),
            ("Edit", Edit),
            ("MultiEdit", Edit),
            ("NotebookEdit", Edit),
            ("Write", Write),
            ("WebFetch", Web),
            ("WebSearch", Web),
            ("Agent", Agent),
            ("Task", Agent),
            ("AskUserQuestion", Ask),
            ("TodoWrite", Todo),
            ("Skill", Other),
            ("EnterWorktree", Other),
        ];
        for (name, expected) in cases {
            assert_eq!(kind(ToolSource::Claude, name), expected, "{name}");
        }
        assert_eq!(
            normalize_tool(ToolSource::Claude, "mcp__claude-in-chrome__computer", None),
            NormalizedTool::mcp("claude-in-chrome")
        );
    }

    #[test]
    fn normalize_tool_codex() {
        use ToolKind::*;
        for (name, expected) in [
            ("exec", Shell),
            ("exec_command", Shell),
            ("write_stdin", Shell),
            ("read_file", Read),
            ("apply_patch", Edit),
            ("write_file", Write),
            ("web_search", Web),
            ("web.search", Web),
            ("spawn_agent", Agent),
            ("request_user_input_async", Ask),
            ("update_plan", Todo),
            ("sleep", Other),
            ("js", Other),
        ] {
            assert_eq!(kind(ToolSource::Codex, name), expected, "{name}");
        }
        // namespace 以 mcp__ 开头 → MCP，server 去前缀
        assert_eq!(
            normalize_tool(ToolSource::Codex, "js", Some("mcp__cua_repl")),
            NormalizedTool::mcp("cua_repl")
        );
        // 非 MCP namespace 不影响
        assert_eq!(kind(ToolSource::Codex, "sleep"), Other);
        assert_eq!(
            normalize_tool(ToolSource::Codex, "send_message", Some("collaboration")).kind,
            Agent
        );
    }

    #[test]
    fn normalize_tool_gemini_opencode_pi_generic() {
        use ToolKind::*;
        assert_eq!(kind(ToolSource::Gemini, "run_shell_command"), Shell);
        assert_eq!(kind(ToolSource::Gemini, "read_many_files"), Read);
        assert_eq!(kind(ToolSource::Gemini, "list_directory"), Search);
        assert_eq!(kind(ToolSource::Gemini, "replace"), Edit);
        assert_eq!(kind(ToolSource::Gemini, "google_web_search"), Web);
        assert_eq!(kind(ToolSource::Gemini, "write_todos"), Todo);
        assert_eq!(
            normalize_tool(ToolSource::Gemini, "github/search_issues", None),
            NormalizedTool::mcp("github")
        );

        assert_eq!(kind(ToolSource::OpenCode, "bash"), Shell);
        assert_eq!(kind(ToolSource::OpenCode, "ast_grep_search"), Search);
        assert_eq!(kind(ToolSource::OpenCode, "patch"), Edit);
        assert_eq!(kind(ToolSource::OpenCode, "background_output"), Agent);
        assert_eq!(kind(ToolSource::OpenCode, "question"), Ask);
        assert_eq!(kind(ToolSource::OpenCode, "todowrite"), Todo);
        assert_eq!(kind(ToolSource::OpenCode, "invalid"), Other);
        assert_eq!(
            normalize_tool(ToolSource::OpenCode, "context7_resolve-library-id", None),
            NormalizedTool::mcp("context7")
        );

        assert_eq!(kind(ToolSource::Pi, "bashExecution"), Shell);
        assert_eq!(kind(ToolSource::Pi, "ls"), Search);
        assert_eq!(kind(ToolSource::Pi, "fetch"), Web);
        assert_eq!(kind(ToolSource::Pi, "custom"), Other);

        assert_eq!(kind(ToolSource::Generic, "terminal"), Shell);
        assert_eq!(kind(ToolSource::Generic, "read_file"), Read);
        assert_eq!(kind(ToolSource::Generic, "search_files"), Search);
        assert_eq!(kind(ToolSource::Generic, "web_search"), Web);
        assert_eq!(kind(ToolSource::Generic, "patch"), Edit);
        assert_eq!(kind(ToolSource::Generic, "write_file"), Write);
        assert_eq!(kind(ToolSource::Generic, "vision_analyze"), Other);
    }

    #[test]
    fn split_mcp_name_requires_server_and_tool() {
        assert_eq!(split_mcp_name("mcp__slack__post"), Some(("slack", "post")));
        assert_eq!(split_mcp_name("mcp__slack"), None);
        assert_eq!(split_mcp_name("mcp____x"), None);
        assert_eq!(split_mcp_name("Bash"), None);
    }

    #[test]
    fn refine_shell_kind_detects_search_programs() {
        assert_eq!(refine_shell_kind("rg -n foo src"), ToolKind::Search);
        assert_eq!(
            refine_shell_kind("/usr/bin/find . -name x"),
            ToolKind::Search
        );
        assert_eq!(refine_shell_kind("cargo build"), ToolKind::Shell);
        assert_eq!(refine_shell_kind(""), ToolKind::Shell);
        assert_eq!(kind_from_codex_parsed_cmd("read"), Some(ToolKind::Read));
        assert_eq!(kind_from_codex_parsed_cmd("list"), Some(ToolKind::Search));
        assert_eq!(kind_from_codex_parsed_cmd("unknown"), None);
    }

    #[test]
    fn preview_keeps_short_text_untouched() {
        let p = preview("a\nb\n");
        assert_eq!(p.text, "a\nb");
        assert_eq!((p.total_len, p.line_count, p.truncated), (4, 2, false));

        let empty = preview("");
        assert_eq!(
            (empty.text.as_str(), empty.line_count, empty.truncated),
            ("", 0, false)
        );
    }

    #[test]
    fn preview_cuts_at_line_limit() {
        let exact: String = (1..=12).map(|i| format!("l{i}\n")).collect();
        let p = preview(&exact);
        assert!(!p.truncated, "恰好 12 行不算截断");
        assert_eq!(p.line_count, 12);

        let over: String = (1..=13).map(|i| format!("l{i}\n")).collect();
        let p = preview(&over);
        assert!(p.truncated);
        assert_eq!(p.text.lines().count(), PREVIEW_LINES);
        assert!(p.text.ends_with("l12"));
        assert_eq!(p.line_count, 13);
    }

    #[test]
    fn preview_cuts_at_char_limit_on_char_boundary() {
        let exact = "字".repeat(PREVIEW_CHARS);
        let p = preview(&exact);
        assert!(!p.truncated);
        assert_eq!(p.total_len as usize, PREVIEW_CHARS);

        let over = "字".repeat(PREVIEW_CHARS + 1);
        let p = preview(&over);
        assert!(p.truncated);
        assert_eq!(p.text.chars().count(), PREVIEW_CHARS);
        assert_eq!(p.total_len as usize, PREVIEW_CHARS + 1);
        assert_eq!(p.line_count, 1);

        let p = preview_chars("abcdef", 3);
        assert_eq!((p.text.as_str(), p.truncated), ("abc", true));
    }

    #[test]
    fn titles_are_single_line_and_bounded() {
        assert_eq!(title_shell("cargo build"), "cargo build");
        assert_eq!(title_shell("cd a\ncargo test"), "cd a…");
        let long = "x".repeat(TITLE_CHARS + 5);
        let t = title_shell(&long);
        assert_eq!(t.chars().count(), TITLE_CHARS + 1);
        assert!(t.ends_with('…'));

        assert_eq!(
            title_read("/a/b.rs", Some(10), Some(31)),
            ("/a/b.rs".to_string(), Some(":10-40".to_string()))
        );
        assert_eq!(title_read("/a/b.rs", None, None).1, None);
        assert_eq!(title_search("foo", Some("src")), "foo in src");
        assert_eq!(title_search("foo", None), "foo");
        assert_eq!(title_web(&["a", "b"]), "a, b");
        assert_eq!(title_mcp("slack", "post"), "slack.post");
        assert_eq!(title_todo(3), "3 项");
        assert_eq!(
            first_string_field(&json!({"n": 1, "q": "", "url": "https://x"})),
            Some("https://x".to_string())
        );
        assert_eq!(estimate_base64_size(8), 6);
    }

    #[test]
    fn title_from_input_covers_each_kind() {
        use ToolKind::*;
        let t = |kind, name: &str, input: Value| title_from_input(kind, name, None, &input);
        assert_eq!(
            t(
                Shell,
                "bash",
                json!({"command": "ls -la", "description": "List"})
            ),
            ("ls -la".into(), Some("List".into()))
        );
        assert_eq!(
            t(
                Read,
                "read",
                json!({"filePath": "/a.rs", "offset": 10, "limit": 31})
            ),
            ("/a.rs".into(), Some(":10-40".into()))
        );
        assert_eq!(
            t(Read, "read_many_files", json!({"paths": ["a", "b"]})).0,
            "a, b"
        );
        assert_eq!(
            t(Search, "grep", json!({"pattern": "foo", "path": "src"})).0,
            "foo in src"
        );
        assert_eq!(
            t(Search, "list_directory", json!({"dir_path": "/p"})).0,
            "/p"
        );
        assert_eq!(t(Edit, "edit", json!({"file_path": "/p/a"})).0, "/p/a");
        assert_eq!(
            t(Web, "webfetch", json!({"url": "https://x"})).0,
            "https://x"
        );
        assert_eq!(
            t(
                Agent,
                "task",
                json!({"description": "Find", "subagent_type": "explore"})
            ),
            ("Find".into(), Some("explore".into()))
        );
        assert_eq!(
            t(
                Ask,
                "question",
                json!({"questions": [{"question": "删吗？"}]})
            )
            .0,
            "删吗？"
        );
        assert_eq!(t(Todo, "todowrite", json!({"todos": [1, 2]})).0, "2 项");
        assert_eq!(
            t(Other, "skill", json!({"name": "commit"})),
            ("skill".into(), Some("commit".into()))
        );
        assert_eq!(t(Shell, "bash", json!({})).0, "");
        assert_eq!(
            title_from_input(
                Mcp,
                "context7_resolve-library-id",
                Some("context7"),
                &json!({})
            )
            .0,
            "context7.resolve-library-id"
        );
    }

    #[test]
    fn diff_helpers_count_lines() {
        assert_eq!(
            count_diff_lines("--- a\n+++ b\n@@\n-x\n+y\n+z\n ctx"),
            (2, 1)
        );
        assert_eq!(count_diff_lines(" 10 a\n-11 x\n+11 y"), (1, 1));

        let edit = diff_from_input(
            ToolKind::Edit,
            &json!({"path": "/a", "edits": [{"oldText": "a\nb", "newText": "c"}, {"oldText": "d", "newText": "e\nf"}]}),
        )
        .unwrap();
        assert_eq!((edit.added, edit.removed), (3, 3));
        let write = diff_from_input(
            ToolKind::Write,
            &json!({"file_path": "/a", "content": "1\n2\n"}),
        )
        .unwrap();
        assert_eq!(
            (write.added, write.removed, write.files[0].op),
            (2, 0, DiffOp::Add)
        );
        assert!(diff_from_input(ToolKind::Edit, &json!({"path": "/a"})).is_none());
    }

    #[test]
    fn arguments_and_turns() {
        assert_eq!(parse_arguments(&json!("{\"a\":1}")), json!({"a": 1}));
        assert_eq!(parse_arguments(&json!("not json")), json!("not json"));
        assert_eq!(parse_arguments(&json!("42")), json!("42"));

        let msg = |role: &str, injected: bool, blocks: Vec<SessionBlock>| SessionMessage {
            role: role.into(),
            injected,
            blocks,
            ..SessionMessage::default()
        };
        let q = || vec![SessionBlock::text("q")];
        let by_user = tool_call_block(ToolSource::Pi, "u1", "bash", &json!({}), || None);
        let mut messages = vec![
            msg("system", false, q()),
            msg("user", false, q()),
            msg("assistant", false, q()),
            msg("user", true, q()),
            // 中断、用户自己跑的命令不开新轮
            msg(
                "user",
                false,
                vec![SessionBlock::event(EventKind::Aborted, None, None)],
            ),
            msg("user", false, vec![by_user]),
            msg("user", false, q()),
            msg(
                "user",
                false,
                vec![SessionBlock::event(
                    EventKind::SlashCommand,
                    Some("/compact".into()),
                    None,
                )],
            ),
        ];
        assign_turn_ids(&mut messages);
        let turns: Vec<_> = messages
            .iter()
            .map(|m| m.turn_id.clone().unwrap())
            .collect();
        assert_eq!(turns, ["t0", "t1", "t1", "t1", "t1", "t1", "t2", "t3"]);
    }
}
