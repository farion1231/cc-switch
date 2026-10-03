//! Codex rollout 解析的底层零件：
//!
//! - 借用式反序列化类型：大行（9MB 的 `custom_tool_call_output`、10MB 的 `compacted`）
//!   只解析用得到的字段，base64 图片等不带转义的长字串直接借用行缓冲，不拷贝。
//! - `event_msg.item_completed` 结构化项（CommandExecution / FileChange / McpToolCall /
//!   ImageView / WebSearch / Extension）转成 [`ItemRecord`]，由 `codex.rs` 的状态机
//!   挂到对应的工具调用上。
//! - 工具输出头（`Script completed` / `Wall time` / `Process exited with code` …）、
//!   apply_patch 与 unified diff 的增删统计、data URL 图片引用。

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use serde::de::{self, Deserializer, IgnoredAny, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;

use crate::session_manager::model::{
    ContentRef, DiffFile, DiffOp, DiffSummary, ImageRef, ImageSource, SessionBlock, ToolKind,
    ToolStatus,
};

use super::blocks::{
    count_diff_lines, kind_from_codex_parsed_cmd, one_line_title, preview, preview_chars,
    refine_shell_kind, saturating_u32, title_mcp, title_path, title_shell, title_web, Preview,
    INPUT_PREVIEW_CHARS,
};
use super::utils::JsonlSpan;

// ─── 行位置与 ContentRef ─────────────────────────────────────────────────

/// RFC 6901：`~` → `~0`，`/` → `~1`
pub(super) fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

// ─── 借用式反序列化 ───────────────────────────────────────────────────────

/// 字符串字段：没有转义时借用行缓冲，有转义时才分配。
///
/// serde 只对字段类型恰好是 `Cow<str>` 的情况做借用特判，`Option<Cow<str>>`
/// 会退化成总是拷贝；包一层自定义 visitor 才能让 `Option<Str>` 也借用。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Str<'a>(pub Cow<'a, str>);

impl Str<'_> {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for Str<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrVisitor;
        impl<'de> Visitor<'de> for StrVisitor {
            type Value = Str<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(Str(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Str(Cow::Owned(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(Str(Cow::Owned(v)))
            }
        }
        deserializer.deserialize_str(StrVisitor)
    }
}

pub(super) fn opt_str<'s>(value: &'s Option<Str<'_>>) -> Option<&'s str> {
    value.as_ref().map(Str::as_str)
}

/// 内容数组里的一项：`input_text` / `output_text` / `input_image` / MCP `image` …
#[derive(Debug, Deserialize)]
pub(super) struct ContentItem<'a> {
    #[serde(rename = "type", default, borrow)]
    pub ty: Option<Str<'a>>,
    #[serde(default, borrow)]
    pub text: Option<Str<'a>>,
    #[serde(default, borrow)]
    pub image_url: Option<Str<'a>>,
    /// MCP 结果里的 base64 图片
    #[serde(default, borrow)]
    pub data: Option<Str<'a>>,
    #[serde(rename = "mimeType", default, borrow)]
    pub mime_type: Option<Str<'a>>,
}

impl ContentItem<'_> {
    pub fn kind(&self) -> &str {
        opt_str(&self.ty).unwrap_or_default()
    }
}

/// `content` / `output` 字段：字符串，或内容项数组。
#[derive(Debug)]
pub(super) enum Contents<'a> {
    Text(Str<'a>),
    Items(Vec<ContentItem<'a>>),
}

impl<'de: 'a, 'a> Deserialize<'de> for Contents<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ContentsVisitor;
        impl<'de> Visitor<'de> for ContentsVisitor {
            type Value = Contents<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string or an array of content items")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(Contents::Text(Str(Cow::Borrowed(v))))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Contents::Text(Str(Cow::Owned(v.to_owned()))))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(Contents::Text(Str(Cow::Owned(v))))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Contents::Items(Vec::new()))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Contents::Items(Vec::new()))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<ContentItem<'de>>()? {
                    items.push(item);
                }
                Ok(Contents::Items(items))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                // 未知形状：整体跳过
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Contents::Items(Vec::new()))
            }
        }
        deserializer.deserialize_any(ContentsVisitor)
    }
}

impl Contents<'_> {
    /// 所有文本项（`input_text` / `output_text` / `text` / `summary_text` …）的
    /// `(下标, 文本)`；字符串形式时下标为 `None`。
    pub fn texts(&self) -> Vec<(Option<usize>, &str)> {
        match self {
            Contents::Text(text) => vec![(None, text.as_str())],
            Contents::Items(items) => items
                .iter()
                .enumerate()
                .filter(|(_, item)| !matches!(item.kind(), "input_image" | "image"))
                .filter_map(|(i, item)| opt_str(&item.text).map(|t| (Some(i), t)))
                .collect(),
        }
    }

    /// 文本项以 `sep` 拼接（跳过空白项）
    pub fn joined_text(&self, sep: &str) -> String {
        self.texts()
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join(sep)
    }

    /// 文本全文的 JSON Pointer：只有一项时指向该项，否则指向整个字段
    /// （P2 取全文时需把内容项数组的 `text` 拼起来）。
    pub fn text_pointer(&self, field: &str) -> String {
        let texts = self.texts();
        match texts.as_slice() {
            [(Some(i), _)] => format!("{field}/{i}/text"),
            _ => field.to_string(),
        }
    }
}

// ─── item_completed ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct ItemDuration {
    #[serde(default)]
    pub secs: u64,
    #[serde(default)]
    pub nanos: u64,
}

impl ItemDuration {
    pub fn as_ms(&self) -> u64 {
        self.secs * 1000 + self.nanos / 1_000_000
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ParsedCmd<'a> {
    #[serde(rename = "type", default, borrow)]
    pub ty: Option<Str<'a>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct McpResult<'a> {
    #[serde(default, borrow)]
    pub content: Option<Contents<'a>>,
    #[serde(rename = "isError", default)]
    pub is_error: Option<bool>,
}

/// `event_msg.item_completed.item`：只声明各类型用得到的字段，其余跳过不分配。
#[derive(Debug, Deserialize)]
pub(super) struct RawItem<'a> {
    #[serde(rename = "type", borrow)]
    pub ty: Str<'a>,
    #[serde(default, borrow)]
    pub id: Option<Str<'a>>,
    // CommandExecution
    #[serde(default)]
    pub command: Option<Value>,
    #[serde(default, borrow)]
    pub cwd: Option<Str<'a>>,
    #[serde(default, borrow)]
    pub parsed_cmd: Option<Vec<ParsedCmd<'a>>>,
    #[serde(default, borrow)]
    pub status: Option<Str<'a>>,
    #[serde(default)]
    pub exit_code: Option<i64>,
    #[serde(default)]
    pub duration: Option<ItemDuration>,
    #[serde(default, borrow)]
    pub aggregated_output: Option<Str<'a>>,
    #[serde(default, borrow)]
    pub source: Option<Str<'a>>,
    // FileChange
    #[serde(default)]
    pub changes: Option<serde_json::Map<String, Value>>,
    // McpToolCall
    #[serde(default, borrow)]
    pub server: Option<Str<'a>>,
    #[serde(default, borrow)]
    pub tool: Option<Str<'a>>,
    #[serde(default)]
    pub arguments: Option<Value>,
    #[serde(default, borrow)]
    pub result: Option<McpResult<'a>>,
    // ImageView
    #[serde(default, borrow)]
    pub path: Option<Str<'a>>,
    // WebSearch / Extension
    #[serde(default, borrow)]
    pub kind: Option<Str<'a>>,
    #[serde(default, borrow)]
    pub query: Option<Str<'a>>,
    #[serde(default)]
    pub action: Option<Value>,
    // AgentMessage
    #[serde(default, borrow)]
    pub phase: Option<Str<'a>>,
}

/// 结构化项归一后的记录，挂到工具调用上或单独成消息。
#[derive(Debug, Clone)]
pub(super) enum ItemRecord {
    Command(CommandInfo),
    FileChange(FileChangeInfo),
    Mcp(McpInfo),
    ImageView { id: String, path: String },
    Web(WebInfo),
}

#[derive(Debug, Clone)]
pub(super) struct CommandInfo {
    pub id: String,
    /// 去掉 `/bin/zsh -lc` 包装后的命令
    pub command: String,
    pub cwd: Option<String>,
    pub kind: ToolKind,
    pub status: ToolStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub output: Preview,
    pub output_ref: Option<ContentRef>,
    /// `aggregated_output` 前 200 字符，用于按输出内容兜底配对
    pub output_head: String,
    pub by_user: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FileChangeInfo {
    pub id: String,
    pub diff: DiffSummary,
    pub status: ToolStatus,
}

#[derive(Debug, Clone)]
pub(super) struct McpInfo {
    pub id: String,
    pub server: String,
    pub tool: String,
    pub detail: Option<String>,
    pub input: String,
    pub status: ToolStatus,
    pub duration_ms: Option<u64>,
    pub output: Preview,
    pub output_ref: Option<ContentRef>,
    pub images: Vec<ImageRef>,
}

#[derive(Debug, Clone)]
pub(super) struct WebInfo {
    pub id: String,
    pub title: String,
    pub input: String,
}

/// Codex 的 status 字串 → [`ToolStatus`]
pub(super) fn status_from_str(status: &str) -> ToolStatus {
    match status {
        "completed" | "success" | "succeeded" => ToolStatus::Success,
        "failed" | "error" | "declined" | "rejected" => ToolStatus::Error,
        "interrupted" | "cancelled" | "canceled" | "aborted" => ToolStatus::Interrupted,
        "in_progress" | "pending" | "running" => ToolStatus::Pending,
        _ => ToolStatus::Unknown,
    }
}

impl RawItem<'_> {
    /// 转成 [`ItemRecord`]；Reasoning / AgentMessage / UserMessage 等由 response_item
    /// 承担的类型返回 `None`。
    pub fn into_record(self, span: JsonlSpan) -> Option<ItemRecord> {
        let id = opt_str(&self.id).unwrap_or_default().to_string();
        match self.ty.as_str() {
            "CommandExecution" => Some(ItemRecord::Command(self.command_info(id, span))),
            "FileChange" => self.file_change_info(id, span).map(ItemRecord::FileChange),
            "McpToolCall" => Some(ItemRecord::Mcp(self.mcp_info(id, span))),
            "ImageView" => {
                let path = opt_str(&self.path)?;
                Some(ItemRecord::ImageView {
                    id,
                    path: strip_file_url(path).to_string(),
                })
            }
            "WebSearch" => Some(ItemRecord::Web(self.web_info(id))),
            "Extension" if opt_str(&self.kind).is_some_and(|k| k.starts_with("web.")) => {
                Some(ItemRecord::Web(self.web_info(id)))
            }
            _ => None,
        }
    }

    fn command_info(&self, id: String, span: JsonlSpan) -> CommandInfo {
        let command = self
            .command
            .as_ref()
            .map(command_from_value)
            .unwrap_or_default();
        let parsed: Vec<&str> = self
            .parsed_cmd
            .iter()
            .flatten()
            .map(|p| opt_str(&p.ty).unwrap_or("unknown"))
            .collect();
        let kind = kind_from_parsed_cmds(&parsed).unwrap_or_else(|| refine_shell_kind(&command));

        let exit_code = self.exit_code.and_then(|c| i32::try_from(c).ok());
        let mut status = opt_str(&self.status)
            .map(status_from_str)
            .unwrap_or(ToolStatus::Unknown);
        if exit_code.is_some_and(|c| c != 0) && status != ToolStatus::Interrupted {
            status = ToolStatus::Error;
        } else if status == ToolStatus::Unknown && exit_code == Some(0) {
            status = ToolStatus::Success;
        }

        let output_text = opt_str(&self.aggregated_output).unwrap_or_default();
        let output = preview(output_text);
        let output_ref = output
            .truncated
            .then(|| span.content_ref("/payload/item/aggregated_output"));
        CommandInfo {
            id,
            command,
            cwd: opt_str(&self.cwd).map(|c| strip_file_url(c).to_string()),
            kind,
            status,
            exit_code,
            duration_ms: self.duration.as_ref().map(ItemDuration::as_ms),
            output,
            output_ref,
            output_head: output_text.trim().chars().take(200).collect(),
            by_user: opt_str(&self.source).is_some_and(|s| s.contains("user")),
        }
    }

    fn file_change_info(&self, id: String, span: JsonlSpan) -> Option<FileChangeInfo> {
        let changes = self.changes.as_ref()?;
        let mut files = Vec::new();
        let mut single_diff_pointer = None;
        for (path, change) in changes {
            let op = match change.get("type").and_then(Value::as_str) {
                Some("add") => DiffOp::Add,
                Some("delete") => DiffOp::Delete,
                _ if change
                    .get("move_path")
                    .and_then(Value::as_str)
                    .is_some_and(|p| !p.is_empty()) =>
                {
                    DiffOp::Rename
                }
                _ => DiffOp::Update,
            };
            let (added, removed) =
                if let Some(diff) = change.get("unified_diff").and_then(Value::as_str) {
                    single_diff_pointer = Some(format!(
                        "/payload/item/changes/{}/unified_diff",
                        escape_pointer_token(path)
                    ));
                    count_diff_lines(diff)
                } else {
                    let lines = change
                        .get("content")
                        .and_then(Value::as_str)
                        .map(|c| saturating_u32(c.lines().count()))
                        .unwrap_or(0);
                    match op {
                        DiffOp::Delete => (0, lines),
                        _ => (lines, 0),
                    }
                };
            files.push(DiffFile {
                path: path.clone(),
                op,
                added,
                removed,
            });
        }
        if files.is_empty() {
            return None;
        }
        // 只有一个文件且是 unified diff 时才能直接指向它
        let full = (files.len() == 1)
            .then_some(single_diff_pointer)
            .flatten()
            .map(|p| span.content_ref(p));
        let mut status = opt_str(&self.status)
            .map(status_from_str)
            .unwrap_or(ToolStatus::Success);
        if status == ToolStatus::Unknown {
            status = ToolStatus::Success;
        }
        Some(FileChangeInfo {
            id,
            diff: diff_summary(files, full),
            status,
        })
    }

    fn mcp_info(&self, id: String, span: JsonlSpan) -> McpInfo {
        let server = opt_str(&self.server).unwrap_or("mcp").to_string();
        let tool = opt_str(&self.tool).unwrap_or_default().to_string();
        let detail = self
            .arguments
            .as_ref()
            .and_then(super::blocks::first_string_field);
        let input = self
            .arguments
            .as_ref()
            .map(|a| a.to_string())
            .unwrap_or_default();

        let result = self.result.as_ref();
        let mut status = opt_str(&self.status)
            .map(status_from_str)
            .unwrap_or(ToolStatus::Unknown);
        if result.and_then(|r| r.is_error) == Some(true) {
            status = ToolStatus::Error;
        } else if status == ToolStatus::Unknown && result.is_some() {
            status = ToolStatus::Success;
        }

        let contents = result.and_then(|r| r.content.as_ref());
        let text = contents.map(|c| c.joined_text("\n")).unwrap_or_default();
        let output = preview(&text);
        let output_ref = match contents {
            Some(c) if output.truncated => {
                Some(span.content_ref(c.text_pointer("/payload/item/result/content")))
            }
            _ => None,
        };
        let images = match contents {
            Some(Contents::Items(items)) => items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.kind() == "image")
                .filter_map(|(i, item)| {
                    let data = opt_str(&item.data)?;
                    Some(ImageRef {
                        source: ImageSource::Inline {
                            content: span
                                .content_ref(format!("/payload/item/result/content/{i}/data")),
                        },
                        media_type: opt_str(&item.mime_type).unwrap_or("image/png").to_string(),
                        size: super::blocks::estimate_base64_size(data.len()),
                        alt: None,
                    })
                })
                .collect(),
            _ => Vec::new(),
        };

        McpInfo {
            id,
            server,
            tool,
            detail,
            input,
            status,
            duration_ms: self.duration.as_ref().map(ItemDuration::as_ms),
            output,
            output_ref,
            images,
        }
    }

    fn web_info(&self, id: String) -> WebInfo {
        let title = self
            .action
            .as_ref()
            .map(web_action_title)
            .filter(|t| !t.is_empty())
            .or_else(|| opt_str(&self.query).map(one_line_title))
            .unwrap_or_default();
        WebInfo {
            id,
            title,
            input: self
                .action
                .as_ref()
                .map(|a| a.to_string())
                .unwrap_or_default(),
        }
    }
}

/// parsed_cmd 全是 read → Read；全是 read/search/list（探索类）→ Search；含 unknown → `None`
pub(super) fn kind_from_parsed_cmds(types: &[&str]) -> Option<ToolKind> {
    if types.is_empty() {
        return None;
    }
    let kinds: Option<Vec<ToolKind>> = types
        .iter()
        .map(|t| kind_from_codex_parsed_cmd(t))
        .collect();
    let kinds = kinds?;
    if kinds.iter().all(|k| *k == ToolKind::Read) {
        Some(ToolKind::Read)
    } else {
        Some(ToolKind::Search)
    }
}

/// web_search_call / WebSearch / Extension 的 `action` → 标题（query、queries、url）
pub(super) fn web_action_title(action: &Value) -> String {
    let str_field = |key: &str| {
        action
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
    };
    if let Some(queries) = action.get("queries").and_then(Value::as_array) {
        let items: Vec<&str> = queries
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .collect();
        if !items.is_empty() {
            return title_web(&items);
        }
    }
    if let Some(query) = str_field("query") {
        return title_web(&[query]);
    }
    match (str_field("pattern"), str_field("url")) {
        (Some(pattern), Some(url)) => title_web(&[&format!("{pattern} in {url}")]),
        (None, Some(url)) => title_web(&[url]),
        _ => String::new(),
    }
}

// ─── 命令文本 ─────────────────────────────────────────────────────────────

/// 去掉 `file://` 前缀
pub(super) fn strip_file_url(path: &str) -> &str {
    path.strip_prefix("file://").unwrap_or(path)
}

/// CommandExecution.command / exec_command.cmd：数组形式的 `[zsh, -lc, script]`
/// 取 script，其余以空格拼接；字符串形式去掉 shell 包装前缀。
pub(super) fn command_from_value(value: &Value) -> String {
    match value {
        Value::String(cmd) => unwrap_shell_string(cmd).to_string(),
        Value::Array(parts) => {
            let parts: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            if let [program, flag, script] = parts.as_slice() {
                if is_shell_program(program) && matches!(*flag, "-lc" | "-c" | "-l -c") {
                    return script.to_string();
                }
            }
            parts.join(" ")
        }
        _ => String::new(),
    }
}

fn is_shell_program(program: &str) -> bool {
    let name = program.rsplit('/').next().unwrap_or(program);
    matches!(name, "zsh" | "bash" | "sh" | "fish" | "dash")
}

fn unwrap_shell_string(cmd: &str) -> &str {
    let trimmed = cmd.trim_start();
    let mut parts = trimmed.splitn(3, ' ');
    if let (Some(program), Some(flag), Some(rest)) = (parts.next(), parts.next(), parts.next()) {
        if is_shell_program(program) && matches!(flag, "-lc" | "-c") {
            let rest = rest.trim();
            // 去掉整体包裹的引号
            for quote in ['\'', '"'] {
                if let Some(inner) = rest.strip_prefix(quote).and_then(|r| r.strip_suffix(quote)) {
                    return inner;
                }
            }
            return rest;
        }
    }
    cmd
}

/// exec JS 里 `exec_command({cmd:"…"})` 的命令（D12：先正则取 cmd）
static EXEC_CMD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"exec_command\(\s*\{\s*"?cmd"?\s*:\s*("(?:[^"\\]|\\.)*")"#).unwrap()
});
static EXEC_WORKDIR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""?workdir"?\s*:\s*("(?:[^"\\]|\\.)*")"#).unwrap());
/// exec JS 里直接调用的 MCP 工具：`tools.mcp__server__tool(`
static EXEC_MCP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"tools\.(mcp__[A-Za-z0-9_\-]+)\s*\(").unwrap());

/// JS 字串字面量（双引号）→ 文本；JSON 解不了时按 JS 的 `\'` 再试，最后退回去引号原文。
fn decode_js_string(literal: &str) -> String {
    if let Ok(text) = serde_json::from_str::<String>(literal) {
        return text;
    }
    if let Ok(text) = serde_json::from_str::<String>(&literal.replace("\\'", "'")) {
        return text;
    }
    literal.trim_matches('"').to_string()
}

/// exec JS 源里能静态识别出的信息
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ExecScript {
    pub cmds: Vec<String>,
    pub workdir: Option<String>,
    /// `(server, tool)`
    pub mcp: Option<(String, String)>,
}

pub(super) fn parse_exec_script(input: &str) -> ExecScript {
    let cmds = EXEC_CMD_RE
        .captures_iter(input)
        .map(|c| decode_js_string(&c[1]))
        .filter(|c| !c.trim().is_empty())
        .collect();
    let workdir = EXEC_WORKDIR_RE
        .captures(input)
        .map(|c| decode_js_string(&c[1]));
    let mcp = EXEC_MCP_RE.captures(input).and_then(|c| {
        super::blocks::split_mcp_name(&c[1]).map(|(s, t)| (s.to_string(), t.to_string()))
    });
    ExecScript { cmds, workdir, mcp }
}

/// 两段命令是否是同一条（空白归一后相等，或较长者以较短者开头）
pub(super) fn same_command(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.starts_with(&b) || b.starts_with(&a)
}

/// exec 调用的标题：一条命令取其首行；多条命令首条加 `…`；都没有时退回 JS 源首行（D12）。
pub(super) fn exec_title(script: &ExecScript, input: &str) -> (ToolKind, String) {
    match script.cmds.as_slice() {
        [] => match &script.mcp {
            Some((server, tool)) => (ToolKind::Mcp, title_mcp(server, tool)),
            None => (ToolKind::Shell, title_shell(input)),
        },
        [cmd] => (refine_shell_kind(cmd), title_shell(cmd)),
        [first, ..] => {
            let mut title = title_shell(first);
            if !title.ends_with('…') {
                title.push('…');
            }
            (refine_shell_kind(first), title)
        }
    }
}

// ─── 工具输出头 ───────────────────────────────────────────────────────────

/// 输出头里提取的信息与正文
#[derive(Debug, Default, PartialEq)]
pub(super) struct OutputHeader<'t> {
    pub body: &'t str,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    /// `Script failed` 之类
    pub failed: bool,
}

/// 去掉 `Script completed\nWall time X seconds\nOutput:\n`、
/// `Chunk ID…\nWall time: X seconds\nProcess exited with code N\n…Output:\n`、
/// `Exit code: N\nWall time: X seconds\nOutput:\n` 这类输出头。
/// 只有前几行全部是已知头字段、且以 `Output:` 行结束时才剥离。
pub(super) fn strip_output_header(text: &str) -> OutputHeader<'_> {
    let mut header = OutputHeader {
        body: text,
        ..OutputHeader::default()
    };
    let mut rest = text;
    for _ in 0..8 {
        let (line, after) = match rest.split_once('\n') {
            Some((line, after)) => (line, after),
            None => (rest, ""),
        };
        let line = line.trim_end_matches('\r');
        if line == "Output:" {
            header.body = after;
            return header;
        }
        if let Some(status) = line.strip_prefix("Script ") {
            if status.contains("fail") || status.contains("error") {
                header.failed = true;
            }
        } else if let Some(secs) = line
            .strip_prefix("Wall time")
            .map(|s| s.trim_start_matches(':').trim())
            .and_then(|s| s.strip_suffix("seconds"))
        {
            header.duration_ms = secs
                .trim()
                .parse::<f64>()
                .ok()
                .map(|s| (s * 1000.0).round() as u64);
        } else if let Some(code) = line
            .strip_prefix("Process exited with code ")
            .or_else(|| line.strip_prefix("Exit code: "))
        {
            header.exit_code = code.trim().parse().ok();
        } else if !(line.starts_with("Chunk ID:")
            || line.starts_with("Original token count:")
            || line.starts_with("Process running with session ID"))
        {
            break;
        }
        if after.is_empty() {
            break;
        }
        rest = after;
    }
    OutputHeader {
        body: text,
        ..OutputHeader::default()
    }
}

/// 旧版 shell 输出：`{"output":"…","metadata":{"exit_code":0,"duration_seconds":0.1}}`
pub(super) fn parse_legacy_shell_output(text: &str) -> Option<(String, Option<i32>, Option<u64>)> {
    if !text.trim_start().starts_with("{\"output\"") {
        return None;
    }
    #[derive(Deserialize)]
    struct Legacy {
        output: String,
        #[serde(default)]
        metadata: Option<LegacyMeta>,
    }
    #[derive(Deserialize)]
    struct LegacyMeta {
        exit_code: Option<i32>,
        duration_seconds: Option<f64>,
    }
    let legacy: Legacy = serde_json::from_str(text).ok()?;
    let meta = legacy.metadata;
    Some((
        legacy.output,
        meta.as_ref().and_then(|m| m.exit_code),
        meta.and_then(|m| m.duration_seconds)
            .map(|s| (s * 1000.0).round() as u64),
    ))
}

// ─── diff ────────────────────────────────────────────────────────────────

pub(super) fn diff_summary(files: Vec<DiffFile>, full: Option<ContentRef>) -> DiffSummary {
    DiffSummary {
        added: files.iter().map(|f| f.added).sum(),
        removed: files.iter().map(|f| f.removed).sum(),
        files,
        full,
    }
}

/// apply_patch 文本（`*** Begin Patch` 格式）→ 文件列表与增删行数
pub(super) fn parse_apply_patch(patch: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    for line in patch.lines() {
        let header = |prefix: &str| line.strip_prefix(prefix).map(str::trim);
        if let Some(path) = header("*** Add File:") {
            files.push(DiffFile {
                path: path.to_string(),
                op: DiffOp::Add,
                added: 0,
                removed: 0,
            });
        } else if let Some(path) = header("*** Update File:") {
            files.push(DiffFile {
                path: path.to_string(),
                op: DiffOp::Update,
                added: 0,
                removed: 0,
            });
        } else if let Some(path) = header("*** Delete File:") {
            files.push(DiffFile {
                path: path.to_string(),
                op: DiffOp::Delete,
                added: 0,
                removed: 0,
            });
        } else if let Some(path) = header("*** Move to:") {
            if let Some(file) = files.last_mut() {
                file.op = DiffOp::Rename;
                file.path = path.to_string();
            }
        } else if line.starts_with("***") {
            continue;
        } else if let Some(file) = files.last_mut() {
            if line.starts_with('+') {
                file.added += 1;
            } else if line.starts_with('-') {
                file.removed += 1;
            }
        }
    }
    files
}

/// 文件改动的 kind：全是新增 → Write，其余 Edit
pub(super) fn diff_kind(diff: &DiffSummary) -> ToolKind {
    if !diff.files.is_empty() && diff.files.iter().all(|f| f.op == DiffOp::Add) {
        ToolKind::Write
    } else {
        ToolKind::Edit
    }
}

/// 文件改动的标题：单文件为路径，多文件为 `首个路径 等 N 个文件`
pub(super) fn diff_title(diff: &DiffSummary) -> String {
    match diff.files.as_slice() {
        [] => String::new(),
        [file] => title_path(&file.path),
        [first, ..] => one_line_title(&format!("{} 等 {} 个文件", first.path, diff.files.len())),
    }
}

// ─── 图片 ────────────────────────────────────────────────────────────────

/// `data:image/png;base64,…` → 内联图片引用；不是 data URL 时：本地路径 → LocalFile，其余忽略。
pub(super) fn image_from_url(url: &str, span: JsonlSpan, pointer: String) -> Option<ImageRef> {
    if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        let media_type = meta.split(';').next().filter(|m| !m.is_empty());
        return Some(ImageRef {
            source: ImageSource::Inline {
                content: span.content_ref(pointer),
            },
            media_type: media_type.unwrap_or("image/png").to_string(),
            size: super::blocks::estimate_base64_size(data.len()),
            alt: None,
        });
    }
    if url.starts_with("file://") || url.starts_with('/') {
        return Some(local_image(strip_file_url(url)));
    }
    None
}

/// 本地图片文件引用；大小取自文件元数据（文件已不存在时为 0）
pub(super) fn local_image(path: &str) -> ImageRef {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    };
    let size = std::fs::metadata(path)
        .map(|m| u32::try_from(m.len()).unwrap_or(u32::MAX))
        .unwrap_or(0);
    ImageRef {
        source: ImageSource::LocalFile {
            path: path.to_string(),
        },
        media_type: media_type.to_string(),
        size,
        alt: None,
    }
}

// ─── block 构造 ───────────────────────────────────────────────────────────

/// 工具调用块的公共部分
pub(super) struct CallSpec<'s> {
    pub id: String,
    pub raw_name: String,
    pub kind: ToolKind,
    pub title: String,
    pub detail: Option<String>,
    pub server: Option<String>,
    pub input: &'s str,
    /// 参数全文的引用（只在预览截断时带上）
    pub input_ref: Option<ContentRef>,
    pub diff: Option<DiffSummary>,
    pub by_user: bool,
}

impl<'s> CallSpec<'s> {
    /// 只填必需字段的调用，其余字段按需用 `..` 覆盖
    pub fn new(
        id: String,
        raw_name: impl Into<String>,
        kind: ToolKind,
        title: String,
        input: &'s str,
    ) -> Self {
        Self {
            id,
            raw_name: raw_name.into(),
            kind,
            title,
            detail: None,
            server: None,
            input,
            input_ref: None,
            diff: None,
            by_user: false,
        }
    }

    pub fn into_block(self) -> SessionBlock {
        let input = preview_chars(self.input, INPUT_PREVIEW_CHARS);
        SessionBlock::ToolCall {
            id: self.id,
            raw_name: self.raw_name,
            kind: self.kind,
            title: self.title,
            detail: self.detail,
            server: self.server,
            input_full: if input.truncated {
                self.input_ref
            } else {
                None
            },
            input_preview: input.text,
            input_total_len: input.total_len,
            diff: self.diff,
            by_user: self.by_user,
        }
    }
}

/// 工具结果块
pub(super) struct ResultSpec {
    pub call_id: String,
    pub status: ToolStatus,
    pub output: Preview,
    pub full: Option<ContentRef>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub images: Vec<ImageRef>,
}

impl ResultSpec {
    /// 没有输出的结果
    pub fn empty(call_id: String, status: ToolStatus) -> Self {
        Self {
            call_id,
            status,
            output: preview(""),
            full: None,
            exit_code: None,
            duration_ms: None,
            images: Vec::new(),
        }
    }

    pub fn into_block(self) -> SessionBlock {
        let truncated = self.output.truncated;
        SessionBlock::ToolResult {
            call_id: self.call_id,
            status: self.status,
            preview: self.output.text,
            total_len: self.output.total_len,
            line_count: self.output.line_count,
            truncated,
            full: if truncated { self.full } else { None },
            exit_code: self.exit_code,
            duration_ms: self.duration_ms,
            images: self.images,
            saved_path: None,
        }
    }
}

impl CommandInfo {
    /// 单独成块的 CommandExecution 工具调用（没有对应 exec 时）
    pub fn call_spec(&self) -> CallSpec<'_> {
        CallSpec {
            detail: self.cwd.clone(),
            by_user: self.by_user,
            ..CallSpec::new(
                self.id.clone(),
                "CommandExecution",
                self.kind,
                title_shell(&self.command),
                &self.command,
            )
        }
    }

    pub fn result_spec(&self, call_id: String) -> ResultSpec {
        ResultSpec {
            output: self.output.clone(),
            full: self.output_ref.clone(),
            exit_code: self.exit_code,
            duration_ms: self.duration_ms,
            ..ResultSpec::empty(call_id, self.status)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn str_borrows_unescaped_strings() {
        #[derive(Deserialize)]
        struct Probe<'a> {
            #[serde(borrow)]
            a: Option<Str<'a>>,
            #[serde(borrow)]
            b: Option<Str<'a>>,
        }
        let raw = r#"{"a":"plain","b":"line\nbreak"}"#;
        let probe: Probe = serde_json::from_str(raw).unwrap();
        assert!(matches!(probe.a.unwrap().0, Cow::Borrowed("plain")));
        assert!(matches!(probe.b.unwrap().0, Cow::Owned(ref s) if s == "line\nbreak"));
    }

    #[test]
    fn contents_accepts_string_or_items() {
        let text: Contents = serde_json::from_str(r#""hi""#).unwrap();
        assert_eq!(text.joined_text("\n"), "hi");
        assert_eq!(text.text_pointer("/payload/output"), "/payload/output");

        let items: Contents = serde_json::from_str(
            r#"[{"type":"input_text","text":"a"},{"type":"input_image","image_url":"data:x"},{"type":"input_text","text":"b"}]"#,
        )
        .unwrap();
        assert_eq!(items.joined_text("\n"), "a\nb");
        assert_eq!(items.text_pointer("/payload/output"), "/payload/output");

        let single: Contents = serde_json::from_str(
            r#"[{"type":"input_image","image_url":"x"},{"type":"input_text","text":"a"}]"#,
        )
        .unwrap();
        assert_eq!(
            single.text_pointer("/payload/output"),
            "/payload/output/1/text"
        );
    }

    #[test]
    fn strip_output_header_variants() {
        let h = strip_output_header("Script completed\nWall time 0.5 seconds\nOutput:\nhello\n");
        assert_eq!(
            (h.body, h.duration_ms, h.failed),
            ("hello\n", Some(500), false)
        );

        let h = strip_output_header(
            "Chunk ID: ab12\nWall time: 0.0012 seconds\nProcess exited with code 2\nOriginal token count: 5\nOutput:\nboom",
        );
        assert_eq!(
            (h.body, h.exit_code, h.duration_ms),
            ("boom", Some(2), Some(1))
        );

        let h = strip_output_header("Exit code: 0\nWall time: 0.2 seconds\nOutput:\nSuccess.");
        assert_eq!((h.body, h.exit_code), ("Success.", Some(0)));

        let h = strip_output_header("Script failed\nWall time 1 seconds\nOutput:\n");
        assert!(h.failed);
        assert_eq!(h.body, "");

        // 不是输出头：原样保留
        let plain = "Wall time is a nice phrase\nOutput:\nx";
        assert_eq!(strip_output_header(plain).body, plain);
        assert_eq!(strip_output_header("file1\nfile2").body, "file1\nfile2");
    }

    #[test]
    fn exec_script_extracts_cmds_workdir_and_mcp() {
        let script = parse_exec_script(
            "const r = await tools.exec_command({cmd:\"rg -n \\\"--legacy\\\" docs\", workdir:\"/p\"});\nconst s = await tools.exec_command({\"cmd\":\"ls\"});",
        );
        assert_eq!(script.cmds, vec!["rg -n \"--legacy\" docs", "ls"]);
        assert_eq!(script.workdir.as_deref(), Some("/p"));
        let (kind, title) = exec_title(&script, "");
        assert_eq!(kind, ToolKind::Search);
        assert_eq!(title, "rg -n \"--legacy\" docs…");

        let mcp = parse_exec_script("text(await tools.mcp__codex_app__list_projects({}));");
        assert_eq!(
            mcp.mcp,
            Some(("codex_app".to_string(), "list_projects".to_string()))
        );
        assert_eq!(
            exec_title(&mcp, ""),
            (ToolKind::Mcp, "codex_app.list_projects".to_string())
        );

        // D12：都取不到 → JS 源首行
        let js = "const x = 1;\ntext(x);";
        assert_eq!(
            exec_title(&parse_exec_script(js), js),
            (ToolKind::Shell, "const x = 1;…".to_string())
        );
    }

    #[test]
    fn command_from_value_unwraps_shell() {
        assert_eq!(
            command_from_value(&json!(["/bin/zsh", "-lc", "cargo test"])),
            "cargo test"
        );
        assert_eq!(command_from_value(&json!(["ls", "-la"])), "ls -la");
        assert_eq!(
            command_from_value(&json!("/bin/bash -lc 'echo hi'")),
            "echo hi"
        );
        assert!(same_command("git  status", "git status"));
        assert!(!same_command("git status", "ls"));
    }

    #[test]
    fn apply_patch_and_unified_diff_counts() {
        let files = parse_apply_patch(
            "*** Begin Patch\n*** Update File: a.rs\n@@\n-old\n+new\n+more\n*** Add File: b.rs\n+x\n*** Delete File: c.rs\n*** End Patch",
        );
        assert_eq!(files.len(), 3);
        assert_eq!(
            (files[0].op, files[0].added, files[0].removed),
            (DiffOp::Update, 2, 1)
        );
        assert_eq!((files[1].op, files[1].added), (DiffOp::Add, 1));
        assert_eq!(files[2].op, DiffOp::Delete);
        let diff = diff_summary(files, None);
        assert_eq!((diff.added, diff.removed), (3, 1));
        assert_eq!(diff_kind(&diff), ToolKind::Edit);
        assert_eq!(diff_title(&diff), "a.rs 等 3 个文件");

        assert_eq!(
            count_diff_lines("--- a\n+++ b\n@@\n-x\n+y\n+z\n ctx"),
            (2, 1)
        );
    }

    #[test]
    fn data_url_becomes_inline_image() {
        let span = JsonlSpan {
            offset: 10,
            len: 20,
        };
        let image =
            image_from_url("data:image/jpeg;base64,AAAABBBB", span, "/p".to_string()).unwrap();
        assert_eq!(image.media_type, "image/jpeg");
        assert_eq!(image.size, 6);
        assert!(matches!(
            image.source,
            ImageSource::Inline {
                content: ContentRef::Jsonl {
                    offset: 10,
                    len: 20,
                    ..
                }
            }
        ));
        assert!(image_from_url("https://x/y.png", span, String::new()).is_none());
        assert!(matches!(
            image_from_url("file:///tmp/a.png", span, String::new()).unwrap().source,
            ImageSource::LocalFile { ref path } if path == "/tmp/a.png"
        ));
        assert_eq!(escape_pointer_token("/a/b~c"), "~1a~1b~0c");
    }
}
