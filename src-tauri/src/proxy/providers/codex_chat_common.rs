use serde_json::{json, Map, Value};

const THINK_OPEN_TAG: &str = "<think>";
const THINK_CLOSE_TAG: &str = "</think>";

// 穷举上游可能的 reasoning 回传字段，优先级：reasoning_content > reasoning(字符串/对象) > reasoning_details。
// 不依赖 provider meta 的 outputFormat 声明，因此对各家 Chat 兼容接口都能兜底提取。
pub(crate) fn extract_reasoning_field_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(reasoning) = value.get("reasoning") {
        for key in ["content", "text", "summary"] {
            if let Some(text) = reasoning.get(key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }

    if let Some(details) = value.get("reasoning_details") {
        if let Some(text) = extract_reasoning_details_text(details) {
            return Some(text);
        }
    }

    None
}

fn extract_reasoning_details_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(extract_reasoning_detail_part_text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(_) => extract_reasoning_detail_part_text(value),
        _ => None,
    }
}

fn extract_reasoning_detail_part_text(value: &Value) -> Option<String> {
    for key in ["text", "content", "summary"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    if let Some(parts) = value.get("parts").and_then(|v| v.as_array()) {
        let text = parts
            .iter()
            .filter_map(extract_reasoning_detail_part_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        return (!text.is_empty()).then_some(text);
    }

    None
}

pub(crate) fn extract_reasoning_summary_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "content", "text"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    let summary = value.get("summary")?;
    if let Some(text) = summary.as_str() {
        return (!text.is_empty()).then(|| text.to_string());
    }

    let parts = summary.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .and_then(|v| v.as_str())
                .or_else(|| part.get("content").and_then(|v| v.as_str()))
                .or_else(|| part.as_str())
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    (!text.is_empty()).then_some(text)
}

pub(crate) fn append_reasoning_content(message: &mut Map<String, Value>, reasoning: &str) -> bool {
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return false;
    }

    match message.get_mut("reasoning_content") {
        Some(Value::String(existing)) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(reasoning.to_string()),
            );
        }
    }
    true
}

pub(crate) fn attach_reasoning_content_field(item: &mut Value, reasoning: &str) -> bool {
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return false;
    }

    if let Some(obj) = item.as_object_mut() {
        obj.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning.to_string()),
        );
        return true;
    }

    false
}

pub(crate) fn attach_optional_reasoning_content_field(
    item: &mut Value,
    reasoning: Option<&str>,
) -> bool {
    let Some(reasoning) = reasoning else {
        return false;
    };
    attach_reasoning_content_field(item, reasoning)
}

pub(crate) fn response_function_call_item(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let mut item = json!({
        "id": item_id,
        "type": "function_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "arguments": arguments
    });
    attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

pub(crate) fn response_function_call_item_with_namespace(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    namespace: Option<&str>,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let mut item =
        response_function_call_item(item_id, status, call_id, name, arguments, reasoning);
    if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
        if let Some(obj) = item.as_object_mut() {
            obj.insert("namespace".to_string(), json!(namespace));
        }
    }
    item
}

pub(crate) fn response_item_call_id(item: &Value) -> Option<String> {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

pub(crate) fn split_leading_think_block(text: &str) -> Option<(String, String)> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    if !after_ws.starts_with(THINK_OPEN_TAG) {
        return None;
    }

    let body_start = leading_ws_len + THINK_OPEN_TAG.len();
    let close_relative = text[body_start..].find(THINK_CLOSE_TAG)?;
    let close_start = body_start + close_relative;
    let answer_start = close_start + THINK_CLOSE_TAG.len();

    Some((
        text[body_start..close_start].trim().to_string(),
        strip_think_answer_separator(&text[answer_start..]).to_string(),
    ))
}

pub(crate) fn strip_leading_think_open_tag(text: &str) -> Option<String> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    after_ws
        .strip_prefix(THINK_OPEN_TAG)
        .map(|value| value.trim().to_string())
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}

/// 内联思考前缀（`<think>`）的判定结果
pub(crate) enum ThinkPrefixDecision {
    /// 前缀尚未确定（可能正处在 `<think>` 的前几个字符上），继续缓冲
    NeedMore,
    /// 确认是内联思考
    Reasoning,
    /// 不是内联思考，按正文处理
    Text,
}

pub(crate) fn leading_think_prefix_decision(buffer: &str) -> ThinkPrefixDecision {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return ThinkPrefixDecision::NeedMore;
    }

    if trimmed.starts_with(THINK_OPEN_TAG) {
        return ThinkPrefixDecision::Reasoning;
    }

    if THINK_OPEN_TAG.starts_with(trimmed) {
        return ThinkPrefixDecision::NeedMore;
    }

    ThinkPrefixDecision::Text
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum InlineThinkMode {
    #[default]
    Detecting,
    Reasoning,
    Text,
}

/// 拆分出的片段：`true` = 思考内容，`false` = 正文
pub(crate) type InlineThinkPart = (bool, String);

/// 把内联了思考（`<think>…</think>`）的正文流拆成「思考 / 正文」片段序列。
///
/// 部分 OpenAI Chat 兼容上游（MiniMax M3、OpenCode Go 等）不用
/// `reasoning_content` 字段回传思考，而是把它内联在 `delta.content` /
/// `message.content` 文本前部。下游的 Responses / Anthropic 协议都要求思考
/// 是独立内容块，直接透传会让思考当正文明文显示（issue #7271），因此两条
/// 转换路径共用本拆分器。
///
/// 开标签可能被切分到多个 chunk，`Detecting` 阶段先缓冲最短前缀，判定不了
/// 就等下一段；`Reasoning` 期间持续缓冲，直到出现完整 `</think>`。
#[derive(Debug, Default)]
pub(crate) struct InlineThinkSplitter {
    mode: InlineThinkMode,
    buffer: String,
}

impl InlineThinkSplitter {
    /// 送入一段正文，返回按序可直接下发的片段（可能为空）。
    pub(crate) fn push(&mut self, delta: &str) -> Vec<InlineThinkPart> {
        match self.mode {
            InlineThinkMode::Text => inline_think_part(false, delta),
            InlineThinkMode::Detecting => {
                self.buffer.push_str(delta);
                match leading_think_prefix_decision(&self.buffer) {
                    ThinkPrefixDecision::NeedMore => Vec::new(),
                    ThinkPrefixDecision::Reasoning => {
                        self.mode = InlineThinkMode::Reasoning;
                        self.drain_complete_block()
                    }
                    ThinkPrefixDecision::Text => {
                        self.mode = InlineThinkMode::Text;
                        inline_think_part(false, &std::mem::take(&mut self.buffer))
                    }
                }
            }
            InlineThinkMode::Reasoning => {
                self.buffer.push_str(delta);
                self.drain_complete_block()
            }
        }
    }

    /// 缓冲里是否还有未判定的内容（`Detecting` 期间的前缀或未闭合的思考）
    pub(crate) fn has_buffered(&self) -> bool {
        !self.buffer.trim().is_empty()
    }

    /// 边界（工具调用 / 流结束）时冲出缓冲：未闭合的 `<think>` 按当前模式解读
    pub(crate) fn flush(&mut self) -> Vec<InlineThinkPart> {
        match self.mode {
            InlineThinkMode::Text => Vec::new(),
            InlineThinkMode::Detecting => {
                self.mode = InlineThinkMode::Text;
                inline_think_part(false, &std::mem::take(&mut self.buffer))
            }
            InlineThinkMode::Reasoning => {
                let buffered = std::mem::take(&mut self.buffer);
                self.mode = InlineThinkMode::Text;
                if let Some((reasoning, answer)) = split_leading_think_block(&buffered) {
                    let mut parts = inline_think_part(true, &reasoning);
                    parts.extend(inline_think_part(false, &answer));
                    return parts;
                }

                let reasoning = strip_leading_think_open_tag(&buffered).unwrap_or(buffered);
                inline_think_part(true, &reasoning)
            }
        }
    }

    /// 缓冲区出现完整 `</think>` 时拆出思考与正文，否则继续等待
    fn drain_complete_block(&mut self) -> Vec<InlineThinkPart> {
        let Some((reasoning, answer)) = split_leading_think_block(&self.buffer) else {
            return Vec::new();
        };

        self.mode = InlineThinkMode::Text;
        self.buffer.clear();

        let mut parts = inline_think_part(true, &reasoning);
        parts.extend(inline_think_part(false, &answer));
        parts
    }
}

/// 空片段不下发
fn inline_think_part(is_thinking: bool, text: &str) -> Vec<InlineThinkPart> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![(is_thinking, text.to_string())]
    }
}
