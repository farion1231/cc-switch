use serde_json::{json, Map, Value};

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

/// Gemini 的 OpenAI 兼容接口把 thought signature 放在 tool call 的
/// `extra_content.google.thought_signature` 里，下一轮必须原样带回，否则工具调用回放报 400。
/// 这里只做透传：从 `source` 复制非空的 `extra_content` 到 `target`。
pub(crate) fn copy_tool_call_extra_content(target: &mut Value, source: &Value) -> bool {
    let Some(extra) = tool_call_extra_content(source) else {
        return false;
    };
    let Some(obj) = target.as_object_mut() else {
        return false;
    };
    obj.insert("extra_content".to_string(), extra.clone());
    true
}

pub(crate) fn tool_call_extra_content(source: &Value) -> Option<&Value> {
    source
        .get("extra_content")
        .filter(|value| value.as_object().is_some_and(|obj| !obj.is_empty()))
}

/// Gemini 文档给的跳过签名校验的占位值，只在真签名找不回来时兜底用（会影响模型表现）。
pub(crate) const GEMINI_SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

/// 签名靠 CodexChatHistoryStore 在内存里补回，CC Switch 在一轮工具调用中途重启就丢了。
/// Gemini 只校验当前轮（最后一条 user 消息之后）每个 assistant 步骤里第一个工具调用的签名，
/// 这里只给这些缺签名的调用填占位值；并行的后续调用照原样不带。只对 Gemini 3 及以后的模型生效。
pub(crate) fn fill_gemini_thought_signature_fallback(chat_body: &mut Value) -> usize {
    if !chat_body
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(requires_gemini_thought_signature)
    {
        return 0;
    }
    let Some(messages) = chat_body.get_mut("messages").and_then(Value::as_array_mut) else {
        return 0;
    };
    let turn_start = messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map_or(0, |index| index + 1);
    let mut filled = 0;
    for message in &mut messages[turn_start..] {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(first_call) = message
            .get_mut("tool_calls")
            .and_then(Value::as_array_mut)
            .and_then(|calls| calls.first_mut())
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        let has_signature = first_call
            .get("extra_content")
            .and_then(|extra| extra.pointer("/google/thought_signature"))
            .and_then(Value::as_str)
            .is_some_and(|signature| !signature.is_empty());
        if has_signature {
            continue;
        }
        let extra = first_call
            .entry("extra_content")
            .or_insert_with(|| json!({}));
        if !extra.is_object() {
            *extra = json!({});
        }
        let google = extra
            .as_object_mut()
            .expect("extra_content is an object")
            .entry("google")
            .or_insert_with(|| json!({}));
        if !google.is_object() {
            *google = json!({});
        }
        google["thought_signature"] = Value::String(GEMINI_SKIP_THOUGHT_SIGNATURE.to_string());
        filled += 1;
    }
    filled
}

/// 模型名（可带 `google/` 之类的中转前缀）是 Gemini 3 及以后的版本：这些版本才强制校验工具调用签名。
pub(crate) fn requires_gemini_thought_signature(model: &str) -> bool {
    let name = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    let Some(version) = name.strip_prefix("gemini-") else {
        return false;
    };
    version
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 3)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_call(id: &str, extra: Option<Value>) -> Value {
        let mut call = json!({
            "id": id,
            "type": "function",
            "function": {"name": "exec_command", "arguments": "{}"}
        });
        if let Some(extra) = extra {
            call["extra_content"] = extra;
        }
        call
    }

    #[test]
    fn gemini_fallback_fills_first_call_of_each_current_turn_step() {
        let signed = json!({"google": {"thought_signature": "sig-real"}});
        let mut body = json!({
            "model": "gemini-4-argon",
            "messages": [
                {"role": "user", "content": "old turn"},
                {"role": "assistant", "tool_calls": [assistant_call("call_old", None)]},
                {"role": "tool", "tool_call_id": "call_old", "content": "x"},
                {"role": "user", "content": "current turn"},
                {"role": "assistant", "tool_calls": [
                    assistant_call("call_1", None),
                    assistant_call("call_2", None)
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "a"},
                {"role": "tool", "tool_call_id": "call_2", "content": "b"},
                {"role": "assistant", "tool_calls": [assistant_call("call_3", Some(signed.clone()))]},
                {"role": "tool", "tool_call_id": "call_3", "content": "c"}
            ]
        });

        assert_eq!(fill_gemini_thought_signature_fallback(&mut body), 1);
        let messages = body["messages"].as_array().unwrap();
        // 上一轮不校验，保持原样
        assert!(messages[1]["tool_calls"][0].get("extra_content").is_none());
        assert_eq!(
            messages[4]["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
            GEMINI_SKIP_THOUGHT_SIGNATURE
        );
        // 并行的后续调用不填
        assert!(messages[4]["tool_calls"][1].get("extra_content").is_none());
        // 真签名不覆盖
        assert_eq!(messages[7]["tool_calls"][0]["extra_content"], signed);
    }

    #[test]
    fn gemini_fallback_skips_models_without_signature_validation() {
        for model in ["gemini-2.5-pro", "gpt-5.5", "glm-5.3", "gemma-4-27b"] {
            let mut body = json!({
                "model": model,
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "tool_calls": [assistant_call("call_1", None)]}
                ]
            });
            assert_eq!(
                fill_gemini_thought_signature_fallback(&mut body),
                0,
                "{model}"
            );
            assert!(body["messages"][1]["tool_calls"][0]
                .get("extra_content")
                .is_none());
        }
    }

    #[test]
    fn gemini_fallback_accepts_relay_prefixed_model_names() {
        assert!(requires_gemini_thought_signature(
            "google/gemini-3-pro-preview"
        ));
        assert!(requires_gemini_thought_signature("Gemini-4-Argon"));
        assert!(requires_gemini_thought_signature("gemini-10-flash"));
        assert!(!requires_gemini_thought_signature("gemini-2.5-flash"));
        assert!(!requires_gemini_thought_signature("gemini-pro"));
    }
}
