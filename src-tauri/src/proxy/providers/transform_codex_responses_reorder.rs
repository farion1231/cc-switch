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
//! Defer developer and system notices until the last available output in the
//! batch, keeping model output items in their original order.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(super) fn call_id_of(item: &Value) -> Option<&str> {
    item.get("call_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

fn message_role(item: &Value) -> Option<&str> {
    if item.get("type").is_none_or(|kind| kind == "message") {
        item.get("role").and_then(Value::as_str)
    } else {
        None
    }
}

pub(super) fn is_user_message(item: &Value) -> bool {
    message_role(item) == Some("user")
}

pub(super) fn tool_output_positions(
    turn: &[Value],
    is_output: impl Fn(&str) -> bool,
) -> HashMap<String, usize> {
    // An output can close a call only if it comes later in the same turn.
    turn.iter()
        .enumerate()
        .filter(|(_, item)| item["type"].as_str().is_some_and(&is_output))
        .filter_map(|(index, item)| Some((call_id_of(item)?.to_owned(), index)))
        .collect()
}

fn is_tool_output(kind: &str) -> bool {
    kind == "local_shell_call_output" || super::codex_chat_history::is_call_output_item_type(kind)
}

/// Move notices out of parallel tool-call batches. Returns whether the order changed.
pub fn normalize_tool_output_grouping(body: &mut Value) -> bool {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut original = std::mem::take(input);
    let mut result = Vec::with_capacity(original.len());
    let mut deferred: Vec<Value> = Vec::new();
    let mut changed = false;

    for turn in original.split_inclusive_mut(is_user_message) {
        let output_positions = tool_output_positions(turn, is_tool_output);
        let mut pending = HashSet::new();
        for (index, slot) in turn.iter_mut().enumerate() {
            let item = slot.take();
            let kind = item["type"].as_str().unwrap_or_default();
            match kind {
                "function_call" | "custom_tool_call" | "local_shell_call" | "tool_search_call" => {
                    if let Some(id) = call_id_of(&item) {
                        if output_positions
                            .get(id)
                            .is_some_and(|&output| output > index)
                        {
                            pending.insert(id.to_owned());
                        }
                    }
                }
                kind if is_tool_output(kind) => {
                    if let Some(id) = call_id_of(&item) {
                        pending.remove(id);
                    }
                }
                _ if !pending.is_empty()
                    && matches!(message_role(&item), Some("developer" | "system")) =>
                {
                    deferred.push(item);
                    continue;
                }
                _ => {}
            }
            changed |= !deferred.is_empty();
            result.push(item);
            if pending.is_empty() {
                result.append(&mut deferred);
            }
        }
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
        let types: Vec<String> = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                i["call_id"]
                    .as_str()
                    .unwrap_or(i["type"].as_str().unwrap_or(""))
                    .to_string()
            })
            .collect();
        // 批次输出必须紧跟调用批次，提示全部后置。
        assert_eq!(
            types,
            vec!["message", "c0", "c1", "c2", "c0", "c1", "c2", "message", "message", "message",]
        );
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

    #[test]
    fn incomplete_batch_does_not_reorder_later_turns() {
        let mut body = json!({"input": [
            call(0), notice(0), user("stop"),
            call(1), output(1), user("next"),
        ]});
        let before = body.clone();
        assert!(!normalize_tool_output_grouping(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn incomplete_batch_at_end_reports_no_change_without_reordering() {
        let mut body = json!({"input": [call(0), notice(0)]});
        let before = body.clone();
        assert!(!normalize_tool_output_grouping(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn reasoning_stays_between_calls() {
        let mut body = json!({"input": [
            user("look"), call(0), {"type": "reasoning", "id": "rs_1"}, call(1),
            output(0), output(1),
        ]});
        let before = body.clone();
        assert!(!normalize_tool_output_grouping(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn notices_flush_after_available_outputs_in_each_batch() {
        let system_notice = json!({"role": "system", "content": "image resized"});
        let search = json!({"type": "tool_search_call", "call_id": "search"});
        let search_output = json!({"type": "tool_search_output", "call_id": "search", "tools": []});
        let mut body = json!({"input": [
            user("look"), call(0), call(1), call(2), output(0), system_notice, output(2),
            {"type": "reasoning", "id": "rs_next"}, search, notice(1), search_output,
        ]});
        assert!(normalize_tool_output_grouping(&mut body));
        assert_eq!(
            body["input"],
            json!([
                user("look"), call(0), call(1), call(2), output(0), output(2), system_notice,
                {"type": "reasoning", "id": "rs_next"}, search, search_output, notice(1),
            ])
        );
    }

    #[test]
    fn user_message_without_type_ends_the_turn() {
        let mut body = json!({"input": [
            call(0), notice(0), {"role": "user", "content": "stop"}, output(0),
        ]});
        let before = body.clone();
        assert!(!normalize_tool_output_grouping(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn kimi_history_repair_preserves_assistant_narration_before_regrouping_notices() {
        let first = json!({"role": "assistant", "content": "Checking both images."});
        let next = json!({"role": "assistant", "content": "Checking another image."});
        let done = json!({"role": "assistant", "content": "Done."});
        let mut body = json!({"input": [
            user("look"), call(0), first, call(1), call(2),
            output(0), notice(0), output(2), next, call(3), output(3), done,
        ]});
        assert!(
            super::super::transform_codex_responses_kimi_history::repair_tool_history(&mut body)
        );
        assert!(normalize_tool_output_grouping(&mut body));
        assert_eq!(
            body["input"],
            json!([
                user("look"),
                first,
                call(0),
                call(1),
                call(2),
                output(0),
                output(2),
                notice(0),
                next,
                call(3),
                output(3),
                done,
            ])
        );
    }
}
