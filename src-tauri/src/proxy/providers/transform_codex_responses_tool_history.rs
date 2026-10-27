//! Repair native Responses tool history for strict DeepSeek endpoints.
//!
//! DeepSeek requires each `function_call_output` to be adjacent to its
//! `function_call`. Codex can replay an injected developer message or
//! reasoning item between the two even though the matching output is present.
//! Keep those interleaved items as context, but place them after the complete
//! tool-result batch so the call/output pairs stay adjacent.

use serde_json::Value;
use std::collections::HashSet;

const TOOL_CALL_TYPES: &[&str] = &["function_call", "custom_tool_call", "tool_search_call"];
const TOOL_OUTPUT_TYPES: &[&str] = &[
    "function_call_output",
    "custom_tool_call_output",
    "tool_search_output",
];

/// Returns the number of interleaved context items moved after their tool
/// output batch.
pub(crate) fn repair_interleaved_tool_turns(body: &mut Value) -> usize {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };
    if input.len() < 3 {
        return 0;
    }

    let known_output_ids: HashSet<String> = input
        .iter()
        .filter(|item| is_tool_output(item))
        .filter_map(tool_call_id)
        .map(ToString::to_string)
        .collect();
    if known_output_ids.is_empty() {
        return 0;
    }

    let mut pending_call_ids = HashSet::new();
    let mut deferred = Vec::new();
    let mut output = Vec::with_capacity(input.len());
    let mut moved = 0;

    for item in std::mem::take(input) {
        if is_deferred_context(&item) && !pending_call_ids.is_empty() {
            deferred.push(item);
            continue;
        }

        if is_tool_call(&item) {
            if let Some(call_id) = tool_call_id(&item).filter(|id| known_output_ids.contains(*id)) {
                pending_call_ids.insert(call_id.to_string());
            }
            output.push(item);
            continue;
        }

        if is_tool_output(&item) {
            if let Some(call_id) = tool_call_id(&item) {
                pending_call_ids.remove(call_id);
            }
            output.push(item);
            if pending_call_ids.is_empty() && !deferred.is_empty() {
                moved += deferred.len();
                output.append(&mut deferred);
            }
            continue;
        }

        if !pending_call_ids.is_empty() {
            // A real turn boundary. Do not move injected context across it;
            // preserve the original relative order even though the tool pair
            // remains malformed and still needs a client/session repair.
            output.append(&mut deferred);
            pending_call_ids.clear();
        }
        output.push(item);
    }

    if !deferred.is_empty() {
        output.append(&mut deferred);
    }
    *input = output;
    moved
}

fn is_tool_call(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(|item_type| TOOL_CALL_TYPES.contains(&item_type))
}

fn is_tool_output(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(|item_type| TOOL_OUTPUT_TYPES.contains(&item_type))
}

fn tool_call_id(item: &Value) -> Option<&str> {
    item.get("call_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

fn is_deferred_context(item: &Value) -> bool {
    let role = item.get("role").and_then(Value::as_str);
    if matches!(role, Some("system" | "developer")) {
        return true;
    }
    match item.get("type").and_then(Value::as_str) {
        Some("reasoning") => true,
        Some("message") => role == Some("assistant"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn item_types(body: &Value) -> Vec<&str> {
        body["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item.get("type").and_then(Value::as_str))
            .collect()
    }

    fn call(call_id: &str) -> Value {
        json!({
            "type": "function_call",
            "call_id": call_id,
            "name": "exec_command",
            "arguments": "{}"
        })
    }

    fn output(call_id: &str) -> Value {
        json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": "ok"
        })
    }

    #[test]
    fn moves_developer_message_after_its_tool_output() {
        let developer = json!({
            "type": "message",
            "role": "developer",
            "content": [{"type": "input_text", "text": "hook context"}]
        });
        let mut body = json!({
            "model": "deepseek-flash",
            "input": [
                {"type": "message", "role": "user", "content": "run"},
                call("call_1"),
                developer.clone(),
                output("call_1")
            ]
        });

        assert_eq!(repair_interleaved_tool_turns(&mut body), 1);
        assert_eq!(
            item_types(&body),
            vec![
                "message",
                "function_call",
                "function_call_output",
                "message"
            ]
        );
        assert_eq!(body["input"][3], developer);
    }

    #[test]
    fn keeps_parallel_outputs_contiguous_before_moved_context() {
        let mut body = json!({
            "model": "deepseek-flash",
            "input": [
                {"type": "message", "role": "user", "content": "run both"},
                call("call_a"),
                call("call_b"),
                {"type": "message", "role": "developer", "content": "hook"},
                output("call_b"),
                {"type": "reasoning", "id": "rs_1", "content": []},
                output("call_a")
            ]
        });

        assert_eq!(repair_interleaved_tool_turns(&mut body), 2);
        assert_eq!(
            item_types(&body),
            vec![
                "message",
                "function_call",
                "function_call",
                "function_call_output",
                "function_call_output",
                "message",
                "reasoning"
            ]
        );
        assert_eq!(body["input"][3]["call_id"], "call_b");
        assert_eq!(body["input"][4]["call_id"], "call_a");
    }

    #[test]
    fn leaves_valid_history_byte_identical() {
        let body = json!({
            "model": "deepseek-flash",
            "input": [
                {"type": "message", "role": "user", "content": "run"},
                call("call_1"),
                output("call_1"),
                {"type": "message", "role": "developer", "content": "after"}
            ]
        });
        let original = serde_json::to_vec(&body).unwrap();
        let mut repaired = body;

        assert_eq!(repair_interleaved_tool_turns(&mut repaired), 0);
        assert_eq!(serde_json::to_vec(&repaired).unwrap(), original);
    }

    #[test]
    fn does_not_move_context_across_a_real_turn_boundary() {
        let mut body = json!({
            "model": "deepseek-flash",
            "input": [
                call("call_1"),
                {"type": "message", "role": "developer", "content": "hook"},
                {"type": "message", "role": "user", "content": "new turn"},
                output("call_1")
            ]
        });
        let original = serde_json::to_vec(&body).unwrap();

        assert_eq!(repair_interleaved_tool_turns(&mut body), 0);
        assert_eq!(serde_json::to_vec(&body).unwrap(), original);
    }
}
