//! Repair native Responses tool history for strict DeepSeek endpoints.
//!
//! DeepSeek requires each `function_call_output` to be adjacent to its
//! `function_call`. Codex can replay an injected developer message or
//! reasoning item between the two even though the matching output is present,
//! or a turn can be aborted before one of the parallel tool outputs is
//! recorded. Repair both cases without modifying the client's stored history.

use serde_json::Value;
use std::collections::HashSet;

const TOOL_CALL_TYPES: &[&str] = &["function_call", "custom_tool_call", "tool_search_call"];
const TOOL_OUTPUT_TYPES: &[&str] = &[
    "function_call_output",
    "custom_tool_call_output",
    "tool_search_output",
];
const INTERRUPTED_TOOL_OUTPUT: &str = "Tool execution interrupted before a result was recorded.";

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

/// Adds explicit interrupted-result outputs for tool calls that have no
/// matching output anywhere in the replayed request.
pub(crate) fn synthesize_missing_tool_outputs(body: &mut Value) -> usize {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };
    if input.is_empty() {
        return 0;
    }

    let output_ids: HashSet<String> = input
        .iter()
        .filter(|item| is_tool_output(item))
        .filter_map(tool_call_id)
        .map(ToString::to_string)
        .collect();
    let mut queued_missing = HashSet::new();
    let mut pending = Vec::new();
    let mut output = Vec::with_capacity(input.len());
    let mut synthesized = 0;

    for item in std::mem::take(input) {
        if is_tool_call(&item) {
            if let Some(call_id) = tool_call_id(&item) {
                if !output_ids.contains(call_id) && queued_missing.insert(call_id.to_string()) {
                    if let Some(item_type) = item.get("type").and_then(Value::as_str) {
                        pending.push((call_id.to_string(), tool_output_type_for_call(item_type)));
                    }
                }
            }
            output.push(item);
            continue;
        }

        if is_tool_output(&item) {
            output.push(item);
            continue;
        }

        if !pending.is_empty() {
            synthesized += append_interrupted_outputs(&mut output, &mut pending);
        }
        output.push(item);
    }

    if !pending.is_empty() {
        synthesized += append_interrupted_outputs(&mut output, &mut pending);
    }
    *input = output;
    synthesized
}

fn append_interrupted_outputs(
    output: &mut Vec<Value>,
    pending: &mut Vec<(String, &'static str)>,
) -> usize {
    let count = pending.len();
    output.extend(pending.drain(..).map(|(call_id, output_type)| {
        serde_json::json!({
            "type": output_type,
            "call_id": call_id,
            "output": INTERRUPTED_TOOL_OUTPUT
        })
    }));
    count
}

fn tool_output_type_for_call(item_type: &str) -> &'static str {
    match item_type {
        "custom_tool_call" => "custom_tool_call_output",
        "tool_search_call" => "tool_search_output",
        _ => "function_call_output",
    }
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

    #[test]
    fn synthesizes_a_missing_parallel_tool_output_before_the_next_user_turn() {
        let mut body = json!({
            "model": "deepseek-flash",
            "input": [
                {"type": "message", "role": "user", "content": "run all"},
                call("call_00"),
                call("call_01"),
                call("call_02"),
                call("call_03"),
                call("call_04_vkINLflFSPuPzckq6qdn6671"),
                output("call_00"),
                output("call_01"),
                output("call_02"),
                output("call_03"),
                {"type": "message", "role": "user", "content": "continue"}
            ]
        });

        assert_eq!(synthesize_missing_tool_outputs(&mut body), 1);
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 12);
        assert_eq!(input[10]["type"], "function_call_output");
        assert_eq!(input[10]["call_id"], "call_04_vkINLflFSPuPzckq6qdn6671");
        assert_eq!(input[10]["output"], INTERRUPTED_TOOL_OUTPUT);
        assert_eq!(input[11]["role"], "user");
    }

    #[test]
    fn synthesizes_before_interleaved_context_for_an_interrupted_call() {
        let developer = json!({
            "type": "message",
            "role": "developer",
            "content": "hook context"
        });
        let mut body = json!({
            "model": "deepseek-flash",
            "input": [
                call("call_interrupted"),
                developer.clone()
            ]
        });

        assert_eq!(synthesize_missing_tool_outputs(&mut body), 1);
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_interrupted");
        assert_eq!(body["input"][2], developer);
    }

    #[test]
    fn maps_custom_tool_calls_to_their_output_type() {
        let mut body = json!({
            "model": "deepseek-flash",
            "input": [
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom",
                    "name": "apply_patch",
                    "input": "patch"
                }
            ]
        });

        assert_eq!(synthesize_missing_tool_outputs(&mut body), 1);
        assert_eq!(body["input"][1]["type"], "custom_tool_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_custom");
    }

    #[test]
    fn does_not_synthesize_when_a_matching_output_exists() {
        let body = json!({
            "model": "deepseek-flash",
            "input": [
                call("call_1"),
                output("call_1")
            ]
        });
        let original = serde_json::to_vec(&body).unwrap();
        let mut repaired = body;

        assert_eq!(synthesize_missing_tool_outputs(&mut repaired), 0);
        assert_eq!(serde_json::to_vec(&repaired).unwrap(), original);
    }
}
