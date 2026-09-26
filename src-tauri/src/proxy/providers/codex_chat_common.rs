use serde_json::{json, Map, Value};

const THINK_TAG_PAIRS: [(&str, &str); 2] = [("<think>", "</think>"), ("<thinking>", "</thinking>")];

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

/// `strip_separator` 只对整段内容开头的 think 块成立：那里的换行是推理与正文之间的
/// 分隔噪音。正文中间的 think 块后面紧跟的空白属于正文本身，抹掉会把
/// `foo<thinking>x</thinking> bar` 粘成 `foobar`。
pub(crate) fn split_leading_think_block(
    text: &str,
    strip_separator: bool,
) -> Option<(String, String)> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    let open_tag = THINK_TAG_PAIRS
        .iter()
        .find_map(|(open_tag, _)| after_ws.starts_with(open_tag).then_some(*open_tag))?;

    let body_start = leading_ws_len + open_tag.len();
    // 上游偶尔会用不配对的标签收尾（如 <think> 开、</thinking> 闭）。只认配对的闭合标签
    // 会让整段连正文一起被当成推理吞掉，所以取最早出现的任一闭合标签。
    let (close_start, close_tag) = THINK_TAG_PAIRS
        .iter()
        .filter_map(|(_, close_tag)| {
            text[body_start..]
                .find(close_tag)
                .map(|relative| (body_start + relative, *close_tag))
        })
        .min_by_key(|(close_start, _)| *close_start)?;
    let answer_start = close_start + close_tag.len();

    let answer = &text[answer_start..];
    let answer = if strip_separator {
        strip_think_answer_separator(answer)
    } else {
        answer
    };

    Some((
        text[body_start..close_start].trim().to_string(),
        answer.to_string(),
    ))
}

pub(crate) fn strip_leading_think_open_tag(text: &str) -> Option<String> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    THINK_TAG_PAIRS.iter().find_map(|(open_tag, _)| {
        after_ws
            .strip_prefix(open_tag)
            .map(|value| value.trim().to_string())
    })
}

/// 最早出现的开标签位置。用于在正文中间发现新的 think 段（上游会在一次响应里
/// 交替输出「推理 → 正文 → 推理 → 正文」）。
pub(crate) fn find_think_open_tag(text: &str) -> Option<(usize, &'static str)> {
    THINK_TAG_PAIRS
        .iter()
        .filter_map(|(open_tag, _)| text.find(open_tag).map(|index| (index, *open_tag)))
        .min_by_key(|(index, _)| *index)
}

/// 末尾可能是半截开标签（如流式切到 `...正文<thin`）时，需要扣住不下发的字节数。
/// 否则先当正文发出去，等标签补全就晚了。
pub(crate) fn pending_think_open_prefix_len(text: &str) -> usize {
    let longest = THINK_TAG_PAIRS
        .iter()
        .map(|(open_tag, _)| open_tag.len())
        .max()
        .unwrap_or(0);

    (1..longest.saturating_sub(1).min(text.len()) + 1)
        .rev()
        .filter(|len| text.is_char_boundary(text.len() - len))
        .find(|len| {
            let suffix = &text[text.len() - len..];
            THINK_TAG_PAIRS
                .iter()
                .any(|(open_tag, _)| open_tag.len() > suffix.len() && open_tag.starts_with(suffix))
        })
        .unwrap_or(0)
}

/// 剥离正文里的全部 think 段，返回（合并后的推理, 剩余正文）。非流式路径用。
pub(crate) fn split_all_think_blocks(text: &str) -> Option<(String, String)> {
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut answer = String::new();
    let mut rest = text.to_string();
    let mut stripped_any = false;

    while let Some((open_at, _)) = find_think_open_tag(&rest) {
        // 开标签前只有空白时仍属于整段开头；这些空白和闭合标签后的空白都是
        // 推理/正文分隔噪音，不能进入可见正文。
        let prefix = &rest[..open_at];
        let is_leading = answer.is_empty() && prefix.trim().is_empty();
        let Some((reasoning, tail)) = split_leading_think_block(&rest[open_at..], is_leading)
        else {
            // 开标签没有闭合，剩下的整段留给正文，交由调用方兜底。
            break;
        };
        stripped_any = true;
        if !is_leading {
            answer.push_str(prefix);
        }
        if !reasoning.is_empty() {
            reasoning_parts.push(reasoning);
        }
        rest = tail;
    }

    if !stripped_any {
        return None;
    }

    answer.push_str(&rest);
    Some((reasoning_parts.join("\n\n"), answer))
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}
