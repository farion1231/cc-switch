//! Regroup parallel tool outputs for strict native-Responses upstreams.
//!
//! Codex Desktop interleaves developer notices (e.g. `<image_resize_notice>`)
//! between the outputs of a parallel `view_image` batch. Strict gateways
//! (Kimi `api.kimi.com/coding/v1/responses`, and the same family as the
//! DeepSeek "No tool output found" case fixed on the Chat bridge) require the
//! tool outputs to follow their `tool_calls` batch contiguously; anything
//! wedged in between makes them drop the remaining outputs, leaving orphan
//! `tool_calls` that the upstream rejects with HTTP 400 on every later replay
//! of the thread (see #7504).
//!
//! The replay stays semantically identical: while a batch of
//! `function_call`-style items still has unanswered `call_id`s, non-tool items
//! are held back and flushed right after the batch completes. Already
//! contiguous input is passed through untouched.

use serde_json::Value;
use std::collections::HashSet;

fn call_id_of(item: &Value) -> Option<&str> {
    item.get("call_id").and_then(Value::as_str)
}

/// Regroup `input` so every parallel tool-call batch is followed contiguously
/// by its outputs. Returns `true` when the order actually changed.
pub fn normalize_tool_output_grouping(body: &mut Value) -> bool {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };
    let original = std::mem::take(input);
    let mut result: Vec<Value> = Vec::with_capacity(original.len());
    let mut pending: HashSet<String> = HashSet::new();
    let mut deferred: Vec<Value> = Vec::new();
    let mut changed = false;

    for item in original {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "function_call" | "custom_tool_call" | "local_shell_call" => {
                if let Some(call_id) = call_id_of(&item) {
                    pending.insert(call_id.to_string());
                }
                result.push(item);
            }
            "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
                if let Some(call_id) = call_id_of(&item) {
                    pending.remove(call_id);
                }
                result.push(item);
                if pending.is_empty() && !deferred.is_empty() {
                    changed = true;
                    result.append(&mut deferred);
                }
            }
            _ => {
                if pending.is_empty() {
                    result.push(item);
                } else {
                    deferred.push(item);
                }
            }
        }
    }
    if !deferred.is_empty() {
        // Batch never completed (already-malformed history): keep every item,
        // just append what is left.
        changed = true;
        result.append(&mut deferred);
    }
    *input = result;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(i: i32) -> Value {
        json!({"type":"function_call","name":"view_image","arguments":"{}","call_id":format!("c{i}")})
    }
    fn output(i: i32) -> Value {
        json!({"type":"function_call_output","call_id":format!("c{i}"),"output":"[image]"})
    }
    fn notice(i: i32) -> Value {
        json!({"type":"message","role":"developer","content":[{"type":"input_text","text":format!("<image_resize_notice>{i}</image_resize_notice>")}]})
    }
    fn user(text: &str) -> Value {
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})
    }

    #[test]
    fn interleaved_notices_are_flushed_after_the_batch() {
        let mut body = json!({"input": [
            user("look"),
            call(0), call(1), call(2),
            output(0), notice(0), output(1), notice(1), output(2),
            user("continue"),
        ]});
        assert!(normalize_tool_output_grouping(&mut body));
        let types: Vec<String> = body["input"].as_array().unwrap().iter().map(|i| i["call_id"].as_str().unwrap_or(i["type"].as_str().unwrap_or("")).to_string()).collect();
        // 批次输出必须紧跟调用批次，提示全部后置。
        assert_eq!(types, vec![
            "message", "c0", "c1", "c2", "c0", "c1", "c2",
            "message", "message", "message",
        ]);
    }

    #[test]
    fn contiguous_input_is_untouched() {
        let mut body = json!({"input": [
            user("look"),
            call(0), call(1), call(2),
            output(0), output(1), output(2),
            notice(0),
            user("continue"),
        ]});
        let before = body.clone();
        assert!(!normalize_tool_output_grouping(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn notice_between_sequential_pairs_is_kept_in_place() {
        let mut body = json!({"input": [
            user("look"),
            call(0), output(0), notice(0),
            call(1), output(1),
            user("continue"),
        ]});
        let before = body.clone();
        assert!(!normalize_tool_output_grouping(&mut body));
        assert_eq!(body, before);
    }
}
