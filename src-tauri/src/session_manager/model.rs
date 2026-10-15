//! 会话阅读页的统一数据模型。
//!
//! 各 provider 解析器把源记录转成 [`SessionMessage`]：结构化内容放 `blocks`，
//! `content` 是由 blocks 派生的纯文本投影（[`project_content`]），供旧的搜索 /
//! 复制 / 目录逻辑继续使用。前端类型在 `src/types.ts`，两边字段一一对应；
//! `tests/fixtures/sessions/*.messages.json` 是两边共用的契约样例。

use serde::{Deserialize, Serialize, Serializer};

/// 序列化规则见下方手写的 [`Serialize`] 实现：有 blocks 时不下发 `content`。
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    /// user | assistant | tool | system（保持旧值，前端已有 roleLabel）
    pub role: String,
    /// 纯文本投影（规则见 [`project_content`]），后端内部（目录预览等）使用；
    /// blocks 非空时不序列化，前端从 blocks 推导，旧后端（无 blocks）才靠它兜底
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    /// 源记录 id（Claude uuid / Codex payload.id / OpenCode message id / Pi entry id）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 同一轮（turn）的标识：Codex turn_id、其余由解析器按 user 消息递增生成 `t{n}`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// 注入型内容（AGENTS.md、environment_context、system-reminder、developer 角色…）
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub injected: bool,
    /// 为空表示解析器尚未输出结构化内容，前端按 `content` 兜底
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<SessionBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<MessageMeta>,
}

/// 有 blocks 时省略 `content`（它只是 blocks 的投影，大会话里约占 payload 的四分之一）。
impl Serialize for SessionMessage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            role: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            content: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            ts: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            id: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            turn_id: Option<&'a str>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            injected: bool,
            #[serde(skip_serializing_if = "<[SessionBlock]>::is_empty")]
            blocks: &'a [SessionBlock],
            #[serde(skip_serializing_if = "Option::is_none")]
            meta: Option<&'a MessageMeta>,
        }
        Wire {
            role: &self.role,
            content: self.blocks.is_empty().then_some(self.content.as_str()),
            ts: self.ts,
            id: self.id.as_deref(),
            turn_id: self.turn_id.as_deref(),
            injected: self.injected,
            blocks: &self.blocks,
            meta: self.meta.as_ref(),
        }
        .serialize(serializer)
    }
}

impl SessionMessage {
    /// 由 blocks 构造消息，`content` 按 [`project_content`] 派生。
    pub fn from_blocks(
        role: impl Into<String>,
        ts: Option<i64>,
        blocks: Vec<SessionBlock>,
    ) -> Self {
        Self {
            role: role.into(),
            content: project_content(&blocks),
            ts,
            blocks,
            ..Self::default()
        }
    }

    /// 是否没有任何可显示内容（替代旧的 `content.trim().is_empty()` 判断）
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.content.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

/// 消息内容块。`type` 标签用 snake_case，块内字段用 camelCase（与前端一致）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SessionBlock {
    Text {
        /// 正文；带 `full` 时只是预览（超大注入文本，见 `blocks::text_block`）
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<ContentRef>,
    },
    Thinking {
        /// 可见正文的预览（可能为空：Claude 只有 signature / Codex 只有 encrypted_content）
        text: String,
        /// Codex summary_text、Gemini thoughts.subject 之类的短摘要
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        /// 正文不可见（加密/只留签名）
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        redacted: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// 正文超过预览长度时给引用，按需取全文
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<ContentRef>,
    },
    ToolCall {
        /// 配对键：Claude tool_use.id / Codex call_id / OpenCode callID / Pi toolCall.id
        id: String,
        raw_name: String,
        kind: ToolKind,
        /// 标题主体（命令、路径、搜索词、URL、问题…），已按 kind 提炼，≤ 200 字符
        title: String,
        /// 次要信息（workdir、行范围、description、agent 描述…）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// MCP 服务器名（kind = mcp 时）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server: Option<String>,
        /// 参数的预览（≤ 1200 字符）与全文引用；`input_total_len` 为全文字符数
        input_preview: String,
        input_total_len: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_full: Option<ContentRef>,
        /// 文件改动摘要（Edit/Write/apply_patch/FileChange）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<DiffSummary>,
        /// 用户自己运行的命令（Pi bashExecution、Codex "You ran"）
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        by_user: bool,
    },
    ToolResult {
        /// 对应 ToolCall.id；为空表示源数据没有配对信息（前端显示为通用「工具输出」步骤）
        call_id: String,
        status: ToolStatus,
        /// 预览：前 12 行且 ≤ 1200 字符
        preview: String,
        /// 全文字符数与行数
        total_len: u32,
        line_count: u32,
        truncated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<ContentRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// 结果里夹带的图片（MCP 截图、Read 图片）
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageRef>,
        /// 工具落盘的完整输出路径（Claude persistedOutputPath / Pi fullOutputPath）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        saved_path: Option<String>,
    },
    Image {
        image: ImageRef,
    },
    Event {
        kind: EventKind,
        /// 说明文字；带 `full` 时只是预览（压缩摘要等长文本）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<ContentRef>,
    },
    /// OpenCode step-start / step-finish；其他 Agent 不产生
    Step {
        phase: StepPhase,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl SessionBlock {
    /// 完整正文的 Text 块
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            full: None,
        }
    }

    /// 不带引用的 Event 块
    pub fn event(kind: EventKind, text: Option<String>, url: Option<String>) -> Self {
        Self::Event {
            kind,
            text,
            url,
            full: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Shell,
    Read,
    Search,
    Edit,
    Write,
    Web,
    Mcp,
    Agent,
    Ask,
    Todo,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Success,
    Error,
    Interrupted,
    Pending,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// 用户中断 / turn_aborted
    Aborted,
    /// 上下文压缩（Codex compaction/compacted、Pi compaction、Claude /compact）
    Compaction,
    /// Pi model_change、Codex thread_settings_applied
    ModelChange,
    /// Pi thinking_level_change
    ThinkingLevel,
    /// Claude system.stop_hook_summary（有错误时才产出）
    Hook,
    /// Claude pr-link
    PrLink,
    /// Claude <command-name>
    SlashCommand,
    /// Gemini info
    Info,
    /// Gemini error
    Error,
    /// Codex agent_message / SubAgentActivity
    SubAgent,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepPhase {
    Start,
    Finish,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub files: Vec<DiffFile>,
    pub added: u32,
    pub removed: u32,
    /// 完整 unified diff 的引用（有则可展开看 diff）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<ContentRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub op: DiffOp,
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffOp {
    Add,
    Update,
    Delete,
    Rename,
}

/// 大内容的「按需取」引用。前端原样回传，后端校验后读取。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContentRef {
    /// JSONL：行的字节区间 + 行内 JSON Pointer（RFC 6901）
    Jsonl {
        offset: u64,
        len: u32,
        pointer: String,
    },
    /// SQLite：表 + 主键 + 列 + 列内 JSON Pointer（空串表示整列，列本身不是 JSON 时用）
    Sqlite {
        table: String,
        id: String,
        column: String,
        pointer: String,
    },
    /// 独立 JSON 文件（OpenCode 文件存储 part）：相对于会话 sourcePath 所属 storage 根的路径
    File { rel_path: String, pointer: String },
    /// 工具落盘的完整输出文件（只允许会话附属目录内）
    Sidecar { rel_path: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub source: ImageSource,
    pub media_type: String,
    /// 解码后的字节数估算（base64 长度 × 3/4）；本地文件为文件大小，未知为 0
    pub size: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ImageSource {
    /// base64 内联在源记录里（Claude image / tool_result image、Codex input_image data URL、Pi toolResult image）
    Inline { content: ContentRef },
    /// 本地文件（Codex ImageView、Claude tool-results/*.jpg、Markdown 里的 file://）
    LocalFile { path: String },
}

/// 分块传输（§5）：`stream_session_messages` 通过 Channel 依次发送。
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TranscriptChunk {
    /// 第一包：总数与轮次索引，让前端先画骨架
    Header {
        total: usize,
        turns: Vec<TurnIndex>,
        cached: bool,
        parse_ms: u64,
    },
    Messages {
        start: usize,
        messages: Vec<SessionMessage>,
    },
    Done {
        payload_bytes: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnIndex {
    pub turn_id: String,
    pub first_message_index: usize,
    pub last_message_index: usize,
    /// ≤ 80 字符
    pub question_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
    pub step_count: u32,
    pub error_count: u32,
    pub has_final_reply: bool,
    pub aborted: bool,
}

/// 由 blocks 派生纯文本 `content`（§3.4）：
///
/// - `Text` → 原文
/// - `ToolCall` → `[Tool: {raw_name}] {title}`（保留旧前缀，旧前端仍能识别为工具行）
/// - `ToolResult` → 预览（不是全文）
/// - `Image` → `[Image: {media_type} {size}]`
/// - `Event` → `text`（没有则跳过）
/// - `Thinking` / `Step` → 不进入
///
/// 各部分之间以空行连接；空部分跳过。
pub fn project_content(blocks: &[SessionBlock]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for block in blocks {
        let part = match block {
            SessionBlock::Text { text, .. } => text.clone(),
            SessionBlock::ToolCall {
                raw_name, title, ..
            } => {
                if title.is_empty() {
                    format!("[Tool: {raw_name}]")
                } else {
                    format!("[Tool: {raw_name}] {title}")
                }
            }
            SessionBlock::ToolResult { preview, .. } => preview.clone(),
            SessionBlock::Image { image } => {
                format!("[Image: {} {}]", image.media_type, image.size)
            }
            SessionBlock::Event { text, .. } => text.clone().unwrap_or_default(),
            SessionBlock::Thinking { .. } | SessionBlock::Step { .. } => continue,
        };
        if !part.trim().is_empty() {
            parts.push(part);
        }
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn text(s: &str) -> SessionBlock {
        SessionBlock::text(s)
    }

    fn tool_call(raw_name: &str, title: &str) -> SessionBlock {
        SessionBlock::ToolCall {
            id: "call_1".into(),
            raw_name: raw_name.into(),
            kind: ToolKind::Shell,
            title: title.into(),
            detail: None,
            server: None,
            input_preview: "{}".into(),
            input_total_len: 2,
            input_full: None,
            diff: None,
            by_user: false,
        }
    }

    #[test]
    fn project_content_follows_projection_rules() {
        let blocks = vec![
            SessionBlock::Thinking {
                text: "secret plan".into(),
                summary: None,
                redacted: false,
                duration_ms: None,
                full: None,
            },
            text("先看看构建输出。"),
            tool_call("Bash", "cargo build"),
            tool_call("Write", ""),
            SessionBlock::ToolResult {
                call_id: "call_1".into(),
                status: ToolStatus::Error,
                preview: "error[E0433]".into(),
                total_len: 12,
                line_count: 1,
                truncated: false,
                full: None,
                exit_code: Some(101),
                duration_ms: None,
                images: vec![],
                saved_path: None,
            },
            SessionBlock::Image {
                image: ImageRef {
                    source: ImageSource::LocalFile {
                        path: "/tmp/a.png".into(),
                    },
                    media_type: "image/png".into(),
                    size: 2048,
                    alt: None,
                },
            },
            SessionBlock::event(EventKind::Aborted, None, None),
            SessionBlock::Step {
                phase: StepPhase::Finish,
                tokens: Some(10),
                cost_usd: None,
                reason: None,
            },
        ];
        assert_eq!(
            project_content(&blocks),
            "先看看构建输出。\n\n[Tool: Bash] cargo build\n\n[Tool: Write]\n\nerror[E0433]\n\n[Image: image/png 2048]"
        );
    }

    #[test]
    fn serializes_with_snake_case_tags_and_camel_case_fields() {
        let msg = SessionMessage::from_blocks(
            "assistant",
            Some(1),
            vec![SessionBlock::ToolResult {
                call_id: "c".into(),
                status: ToolStatus::Interrupted,
                preview: String::new(),
                total_len: 0,
                line_count: 0,
                truncated: false,
                full: Some(ContentRef::Sidecar {
                    rel_path: "tool-results/c.txt".into(),
                }),
                exit_code: None,
                duration_ms: Some(5),
                images: vec![],
                saved_path: None,
            }],
        );
        let value = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            value,
            json!({
                "role": "assistant",
                "ts": 1,
                "blocks": [{
                    "type": "tool_result",
                    "callId": "c",
                    "status": "interrupted",
                    "preview": "",
                    "totalLen": 0,
                    "lineCount": 0,
                    "truncated": false,
                    "full": { "kind": "sidecar", "relPath": "tool-results/c.txt" },
                    "durationMs": 5
                }]
            })
        );

        let chunk = TranscriptChunk::Header {
            total: 0,
            turns: vec![],
            cached: true,
            parse_ms: 3,
        };
        assert_eq!(
            serde_json::to_value(&chunk).unwrap(),
            json!({ "type": "header", "total": 0, "turns": [], "cached": true, "parseMs": 3 })
        );
    }

    #[test]
    fn legacy_message_serializes_like_before() {
        let legacy = |content: &str| SessionMessage {
            role: "user".into(),
            content: content.into(),
            ..SessionMessage::default()
        };
        let msg = legacy("hi");
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({ "role": "user", "content": "hi" })
        );
        assert!(!msg.is_empty());
        assert!(legacy("  ").is_empty());
    }

    /// 前后端共用的 fixture 必须能被 Rust 类型无损往返（含「有 blocks 不带 content」的省略规则）。
    /// 注意：f64 字段（costUsd）在 fixture 里不要写成整数，否则往返后变成 `x.0` 导致不相等。
    #[test]
    fn shared_fixtures_round_trip() {
        let fixtures = [
            (
                "claude",
                include_str!("../../../tests/fixtures/sessions/claude.messages.json"),
            ),
            (
                "codex",
                include_str!("../../../tests/fixtures/sessions/codex.messages.json"),
            ),
            (
                "gemini",
                include_str!("../../../tests/fixtures/sessions/gemini.messages.json"),
            ),
            (
                "opencode",
                include_str!("../../../tests/fixtures/sessions/opencode.messages.json"),
            ),
            (
                "pi",
                include_str!("../../../tests/fixtures/sessions/pi.messages.json"),
            ),
            (
                "generic",
                include_str!("../../../tests/fixtures/sessions/generic.messages.json"),
            ),
        ];
        for (name, raw) in fixtures {
            let original: Value = serde_json::from_str(raw).unwrap();
            let messages: Vec<SessionMessage> =
                serde_json::from_value(original.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
            let round_trip = serde_json::to_value(&messages).unwrap();
            assert_eq!(round_trip, original, "{name}: fixture 往返后不一致");

            // 有 blocks 的消息不带 content（前端从 blocks 推导），只有旧格式消息才带
            for (i, msg) in original.as_array().unwrap().iter().enumerate() {
                let has_blocks = msg.get("blocks").is_some();
                assert_eq!(
                    msg.get("content").is_some(),
                    !has_blocks,
                    "{name}[{i}]: 有 blocks 时不应带 content"
                );
            }
        }
    }
}
