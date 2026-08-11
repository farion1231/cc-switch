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

/// Strip all complete `thinking…think` blocks **and** stray tag fragments
/// from arbitrary text (not just a leading block).
///
/// This is used in `InlineThinkMode::Text` to prevent reasoning tags that
/// arrive after the state machine has already committed to Text mode from
/// leaking into `output_text.delta`.
///
/// It also strips a trailing partial close-tag fragment (e.g. `</t` from a
/// `think` tag that was split across two SSE chunks) so that the fragment
/// does not appear verbatim in the visible output.
pub(crate) fn strip_all_think_tags(text: &str) -> String {
    strip_all_think_tags_with_pending(text).0
}

/// Like `strip_all_think_tags`, but also returns any trailing partial
/// close-tag fragment (e.g. `</t`, `</th`) as the second element.  The
/// caller should buffer this fragment and prepend it to the next delta
/// so that a close tag split across SSE chunks is reassembled correctly
/// instead of being dropped or leaked.
pub(crate) fn strip_all_think_tags_with_pending(text: &str) -> (String, String) {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    loop {
        match remaining.find(THINK_OPEN_TAG) {
            Some(open_start) => {
                // Emit text before the open tag.
                result.push_str(&remaining[..open_start]);

                let after_open = &remaining[open_start + THINK_OPEN_TAG.len()..];
                match after_open.find(THINK_CLOSE_TAG) {
                    Some(close_pos) => {
                        // Complete block — skip the reasoning inside.
                        remaining = &after_open[close_pos + THINK_CLOSE_TAG.len()..];
                    }
                    None => {
                        // Open tag without a close — treat the rest as
                        // reasoning and discard it.
                        remaining = "";
                    }
                }
            }
            None => {
                // No more open tags.  Remove any standalone close tags
                // (close tags without a matching open tag that were not
                // consumed by the main loop).  Then extract a trailing
                // partial close-tag fragment for buffering.
                let without_close_tags = remaining.replace(THINK_CLOSE_TAG, "");
                let (clean, pending) =
                    extract_trailing_close_tag_fragment(&without_close_tags);
                result.push_str(clean);
                return (result, pending.to_string());
            }
        }
    }

/// Split `text` into `(clean, pending)` where `pending` is a trailing
/// prefix of `THINK_CLOSE_TAG` (e.g. `<`, `</`, `</t`, …).  Returns
/// `(text, "")` if the text does not end with such a fragment.
fn extract_trailing_close_tag_fragment(text: &str) -> (&str, &str) {
    // THINK_CLOSE_TAG is pure ASCII (`think`), so any valid fragment
    // prefix is also ASCII.  We must only cut at char boundaries to
    // avoid slicing inside a multi-byte UTF-8 sequence.
    let max_cut = THINK_CLOSE_TAG.len().min(text.len());

    for cut in (1..=max_cut).rev() {
        let split_point = text.len() - cut;
        if !text.is_char_boundary(split_point) {
            continue;
        }
        let suffix = &text[split_point..];
        if THINK_CLOSE_TAG.starts_with(suffix) {
            return (&text[..split_point], suffix);
        }
    }
    (text, "")
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}
