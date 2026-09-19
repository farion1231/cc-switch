//! Synthesize outputs for orphan tool calls in replayed Codex Responses
//! histories.
//!
//! When a Codex turn is interrupted between the model's tool call and the
//! tool's result, the thread history keeps a `function_call` /
//! `custom_tool_call` input item whose paired `*_output` never lands. Every
//! later replay of that thread then carries the orphan, and strict gateways
//! reject the whole request on the missing output (Kimi: "The following
//! tool_call_ids did not have response messages") — permanently locking the
//! conversation (#7504).
//!
//! Before the request is converted or passed through, walk the `input` array
//! and insert a placeholder `*_output` item right after each orphan call so
//! the replay stays valid on every path (native Responses passthrough, Chat
//! and Anthropic conversions share this gate). Well-formed histories are left
//! untouched: the walk only rewrites `input` when an orphan was actually
//! found, keeping healthy replays byte-identical for prompt-cache stability.

use serde_json::{json, Value};

/// The call item types the Responses spec pairs with a `*_output` item,
/// mirroring the public tool-call types the Chat/Anthropic transforms
/// recognize. Codex-private shapes (`namespace` tool search) are handled by
/// their own module and are intentionally not synthesized here.
const CALL_TYPES: [&str; 2] = ["function_call", "custom_tool_call"];

/// Placeholder payload for a synthesized output. Bracketed marker text
/// follows `TOOL_RESULT_ERROR_MARKER` conventions so logs and histories read
/// consistently.
pub(crate) const ORPHAN_CALL_OUTPUT_PLACEHOLDER: &str = "[cc-switch:tool-result-unavailable]";

fn call_type_of(item: &Value) -> Option<&'static str> {
    let item_type = item.get("type").and_then(Value::as_str)?;
    CALL_TYPES
        .iter()
        .find(|call_type| **call_type == item_type)
        .copied()
}

fn output_type_for(call_type: &str) -> String {
    format!("{call_type}_output")
}

/// Insert a placeholder output after every orphan tool call in `body.input`.
/// Returns `true` when the body was rewritten; bodies without an `input`
/// array or without orphans pass through untouched.
pub(crate) fn synthesize_missing_tool_call_outputs(body: &mut Value) -> bool {
    let Some(items) = body.get("input").and_then(Value::as_array) else {
        return false;
    };

    // An output item answers the call of its own family: a
    // `function_call_output` satisfies a `function_call`, a
    // `custom_tool_call_output` a `custom_tool_call`. Any output for the
    // call_id counts — duplicated calls with shared ids must not gain extra
    // outputs.
    let mut answered_call_ids: Vec<(String, &'static str)> = Vec::new();
    for item in items {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(call_type) = CALL_TYPES
            .iter()
            .find(|call_type| item_type == output_type_for(call_type))
            .copied()
        else {
            continue;
        };
        if let Some(call_id) = item.get("call_id").and_then(Value::as_str) {
            answered_call_ids.push((call_id.to_string(), call_type));
        }
    }

    let is_answered = |call_id: &str, call_type: &str| {
        answered_call_ids
            .iter()
            .any(|(answered_id, answered_type)| {
                answered_id == call_id && *answered_type == call_type
            })
    };

    let mut rewritten = false;
    let mut replayed: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        let call = call_type_of(item).and_then(|call_type| {
            item.get("call_id")
                .and_then(Value::as_str)
                .filter(|call_id| !call_id.is_empty())
                .map(|call_id| (call_type, call_id))
        });
        replayed.push(item.clone());
        if let Some((call_type, call_id)) = call {
            if !is_answered(call_id, call_type) {
                replayed.push(json!({
                    "type": output_type_for(call_type),
                    "call_id": call_id,
                    "output": ORPHAN_CALL_OUTPUT_PLACEHOLDER,
                }));
                rewritten = true;
            }
        }
    }
    if rewritten {
        body["input"] = Value::Array(replayed);
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_body(items: Value) -> Value {
        json!({ "model": "kimi-k3", "input": items })
    }

    #[test]
    fn healthy_history_is_left_byte_identical() {
        let mut body = history_body(json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
            {"type": "function_call", "call_id": "view_image:24", "name": "view_image", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "view_image:24", "output": "{}"},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "done"}]}
        ]));
        let before = serde_json::to_string(&body).unwrap();
        assert!(!synthesize_missing_tool_call_outputs(&mut body));
        assert_eq!(serde_json::to_string(&body).unwrap(), before);
    }

    #[test]
    fn orphan_function_call_gains_a_synthesized_output() {
        // Minimal repro from #7504: the interrupted turn leaves a call whose
        // output never landed; Kimi answers 400 on every later replay.
        let mut body = history_body(json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
            {"type": "function_call", "call_id": "view_image:24", "name": "view_image", "arguments": "{}"},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "and then?"}]}
        ]));
        assert!(synthesize_missing_tool_call_outputs(&mut body));
        let items = body["input"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        // The synthesized output lands directly after its orphan call.
        assert_eq!(items[2]["type"], "function_call_output");
        assert_eq!(items[2]["call_id"], "view_image:24");
        assert_eq!(items[2]["output"], ORPHAN_CALL_OUTPUT_PLACEHOLDER);
        // The following user message keeps its position.
        assert_eq!(items[3]["role"], "user");
    }

    #[test]
    fn orphan_custom_tool_call_gains_a_synthesized_output() {
        let mut body = history_body(json!([
            {"type": "custom_tool_call", "call_id": "ask_user:7", "name": "ask_user", "input": "{}"},
        ]));
        assert!(synthesize_missing_tool_call_outputs(&mut body));
        let items = body["input"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1]["type"], "custom_tool_call_output");
        assert_eq!(items[1]["call_id"], "ask_user:7");
    }

    #[test]
    fn output_of_a_different_family_does_not_answer_the_call() {
        // A custom_tool_call_output only satisfies a custom_tool_call; the
        // function_call sharing its call_id stays an orphan.
        let mut body = history_body(json!([
            {"type": "function_call", "call_id": "dup:1", "name": "fn", "arguments": "{}"},
            {"type": "custom_tool_call_output", "call_id": "dup:1", "output": "ok"},
        ]));
        assert!(synthesize_missing_tool_call_outputs(&mut body));
        let items = body["input"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[1]["type"], "function_call_output");
        assert_eq!(items[1]["call_id"], "dup:1");
    }

    #[test]
    fn each_orphan_in_a_run_gets_its_own_output() {
        let mut body = history_body(json!([
            {"type": "function_call", "call_id": "a:1", "name": "fn", "arguments": "{}"},
            {"type": "function_call", "call_id": "b:2", "name": "fn", "arguments": "{}"},
        ]));
        assert!(synthesize_missing_tool_call_outputs(&mut body));
        let items = body["input"].as_array().unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[1]["call_id"], "a:1");
        assert_eq!(items[3]["call_id"], "b:2");
        assert_eq!(items[1]["type"], "function_call_output");
        assert_eq!(items[3]["type"], "function_call_output");
    }

    #[test]
    fn answered_duplicate_call_is_not_answered_twice() {
        let mut body = history_body(json!([
            {"type": "function_call", "call_id": "same:1", "name": "fn", "arguments": "{}"},
            {"type": "function_call", "call_id": "same:1", "name": "fn", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "same:1", "output": "ok"},
        ]));
        assert!(!synthesize_missing_tool_call_outputs(&mut body));
    }

    #[test]
    fn call_without_call_id_is_left_alone() {
        // Nothing valid to synthesize against — repairs for id-less shapes
        // are the call-less-output domain (#7374), not this pass.
        let mut body = history_body(json!([
            {"type": "function_call", "name": "fn", "arguments": "{}"},
        ]));
        let before = serde_json::to_string(&body).unwrap();
        assert!(!synthesize_missing_tool_call_outputs(&mut body));
        assert_eq!(serde_json::to_string(&body).unwrap(), before);
    }

    #[test]
    fn body_without_input_array_is_untouched() {
        let mut body = json!({"model": "kimi-k3", "instructions": "hi"});
        let before = serde_json::to_string(&body).unwrap();
        assert!(!synthesize_missing_tool_call_outputs(&mut body));
        assert_eq!(serde_json::to_string(&body).unwrap(), before);
    }
}
