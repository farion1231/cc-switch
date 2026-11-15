//! Recover malformed tool-call history for the Codex Responses-to-Chat bridge.

use serde_json::{json, Value};
use std::collections::HashMap;

/// A failed call retained by the client must not make every subsequent turn
/// fail at strict Chat upstreams. Keep its raw arguments and matching results
/// as historical context instead of guessing arguments or discarding evidence.
///
/// Fold results into the originating assistant message so a mixed parallel
/// batch still has its valid tool results immediately after its tool calls.
/// Run after normal input conversion, which already extracts tool-result media.
pub(super) fn preserve_invalid_tool_calls_as_text(messages: &mut Vec<Value>) -> usize {
    let mut recovered = 0;
    let mut output = Vec::with_capacity(messages.len());
    let mut invalid_owners: HashMap<String, usize> = HashMap::new();

    for mut message in std::mem::take(messages) {
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            let mut notes = Vec::new();
            if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
                calls.retain(|call| {
                    let id = call.get("id").and_then(Value::as_str);
                    // A later valid attempt can reuse an id. Its result must
                    // not be consumed by an earlier malformed attempt.
                    if let Some(id) = id {
                        invalid_owners.remove(id);
                    }
                    if call.get("type").and_then(Value::as_str) != Some("function")
                        || arguments_are_object(call.pointer("/function/arguments"))
                    {
                        return true;
                    }
                    if let Some(id) = id.filter(|id| !id.is_empty()) {
                        invalid_owners.insert(id.to_string(), output.len());
                    }
                    let name = call.pointer("/function/name")
                        .and_then(Value::as_str).unwrap_or("<unknown>");
                    let arguments = context_text(call.pointer("/function/arguments"));
                    notes.push(format!(
                        "[Historical tool call with invalid arguments; context only, not a new tool invocation]\nname: {name}\ncall_id: {}\narguments: {arguments}",
                        id.unwrap_or("<missing>")
                    ));
                    recovered += 1;
                    false
                });
            }
            if !notes.is_empty() {
                if message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
                {
                    message.as_object_mut().unwrap().remove("tool_calls");
                }
                for note in notes {
                    append_context(&mut message, note);
                }
            }
        } else if message.get("role").and_then(Value::as_str) == Some("tool") {
            if let Some((id, owner)) = message
                .get("tool_call_id")
                .and_then(Value::as_str)
                .and_then(|id| invalid_owners.get(id).map(|owner| (id, *owner)))
            {
                let result = context_text(message.get("content"));
                append_context(
                    &mut output[owner],
                    format!(
                        "[Historical tool result for invalid call {id}; context only]\n{result}"
                    ),
                );
                continue;
            }
        }
        output.push(message);
    }
    *messages = output;
    recovered
}

fn arguments_are_object(arguments: Option<&Value>) -> bool {
    match arguments {
        Some(Value::Object(_)) => true,
        Some(Value::String(text)) => {
            serde_json::from_str::<Value>(text).is_ok_and(|value| value.is_object())
        }
        _ => false,
    }
}

fn context_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
        None => "<missing>".to_string(),
    }
}

fn append_context(message: &mut Value, text: String) {
    match message.get_mut("content") {
        Some(Value::String(content)) => {
            if !content.is_empty() {
                content.push_str("\n\n");
            }
            content.push_str(&text);
        }
        Some(Value::Array(parts)) => parts.push(json!({"type": "text", "text": text})),
        _ => message["content"] = json!(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, arguments: &str) -> Value {
        json!({"role": "assistant", "content": null, "tool_calls": [
            {"type": "function", "id": id, "function": {"name": "lookup", "arguments": arguments}}
        ]})
    }

    #[test]
    fn invalid_history_repair_is_idempotent_and_preserves_structured_content() {
        let mut assistant = call("call_failed", "{broken");
        assistant["content"] = json!([{"type": "text", "text": "Existing commentary."}]);
        assistant["reasoning_content"] = json!("Existing reasoning.");
        let mut messages = vec![
            assistant,
            json!({
                "role": "tool", "tool_call_id": "call_failed", "content": {"error": "parse failed"}
            }),
        ];
        assert_eq!(preserve_invalid_tool_calls_as_text(&mut messages), 1);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].get("tool_calls").is_none());
        assert_eq!(
            messages[0]["content"][0],
            json!({"type": "text", "text": "Existing commentary."})
        );
        assert_eq!(messages[0]["reasoning_content"], "Existing reasoning.");
        assert!(messages[0]["content"][1]["text"]
            .as_str()
            .unwrap()
            .contains("{broken"));
        assert!(messages[0]["content"][2]["text"]
            .as_str()
            .unwrap()
            .contains("parse failed"));
        let once = messages.clone();
        assert_eq!(preserve_invalid_tool_calls_as_text(&mut messages), 0);
        assert_eq!(messages, once);
    }

    #[test]
    fn invalid_history_repair_keeps_later_valid_results_with_reused_ids() {
        let valid = call("call_reused", r#"{"query":"weather"}"#);
        let valid_result =
            json!({"role": "tool", "tool_call_id": "call_reused", "content": "sunny"});
        let mut messages = vec![
            call("call_reused", "{broken"),
            json!({"role": "tool", "tool_call_id": "call_reused", "content": "parse failed"}),
            json!({"role": "user", "content": "try again"}),
            valid.clone(),
            valid_result.clone(),
        ];
        assert_eq!(preserve_invalid_tool_calls_as_text(&mut messages), 1);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[2], valid);
        assert_eq!(messages[3], valid_result);
    }

    #[test]
    fn invalid_history_repair_leaves_valid_history_byte_identical() {
        let mut messages = vec![
            call("call_valid", " { \"query\": \"weather\" } "),
            json!({"role": "tool", "tool_call_id": "call_valid", "content": "sunny"}),
            json!({"role": "user", "content": "continue"}),
        ];
        let original = serde_json::to_vec(&messages).unwrap();
        assert_eq!(preserve_invalid_tool_calls_as_text(&mut messages), 0);
        assert_eq!(serde_json::to_vec(&messages).unwrap(), original);
    }
}
