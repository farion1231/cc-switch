//! 各解析器共用的 block 构造工具：工具名归一化（§3.3）、预览截断、标题提炼。
//!
//! 这里只放与具体 Agent 无关的规则；需要上下文的细化（Codex parsed_cmd、
//! apply_patch 的增删、Claude structuredPatch 等）由各解析器在拿到
//! [`normalize_tool`] 的结果后自行修正。

// P1 各解析器接入前，部分函数还没有调用方
#![allow(dead_code)]

use serde_json::Value;

use crate::session_manager::model::ToolKind;

/// 工具结果预览的最大行数
pub const PREVIEW_LINES: usize = 12;
/// 工具结果预览的最大字符数
pub const PREVIEW_CHARS: usize = 1200;
/// 工具参数预览的最大字符数
pub const INPUT_PREVIEW_CHARS: usize = 1200;
/// 思考正文预览的最大字符数
pub const THINKING_PREVIEW_CHARS: usize = 400;
/// 工具标题的最大字符数
pub const TITLE_CHARS: usize = 200;

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

fn saturating_u32(n: usize) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
