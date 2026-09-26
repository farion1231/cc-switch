//! Native Responses tool-history compatibility.
//!
//! Strict third-party `/responses` endpoints (DeepSeek in particular) require
//! every group of parallel tool calls to be followed immediately by the
//! corresponding tool outputs. Codex can otherwise replay a history shaped
//! like:
//!
//! ```text
//! function_call(call_0)
//! function_call(call_1)
//! function_call_output(call_0)
//! message(developer: <image_resize_notice>…)
//! function_call_output(call_1)
//! ```
//!
//! Although both outputs are present, the developer message splits the group
//! and DeepSeek rejects the request with `No tool output found for tool call
//! call_1`. The Chat/Anthropic transforms have their own tool-history
//! normalization; native Responses passthrough previously had none.
//!
//! A small number of Codex-generated Items (notably
//! `send_message_to_thread`) can also be replayed as a tool output without a
//! `call_id`. There is no valid call to attach to such an output, so this
//! module converts only that malformed item into an equivalent user message.
//! The output text is preserved and no synthetic call id is invented.
//!
//! [`normalize_responses_tool_history`] only rewrites a request when it finds
//! such an interleaving. Calls and outputs that are already contiguous remain
//! byte-for-byte unchanged, preserving upstream prompt-cache prefixes. When a
//! rewrite is needed, only the items that interrupted the tool group move;
//! their relative order is preserved after the complete output batch.

use std::collections::HashMap;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolHistoryItemKind {
    Call,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassifiedItem<'a> {
    Call(&'a str),
    Output(&'a str),
    Other,
}

fn classify_item(item: &Value) -> ClassifiedItem<'_> {
    let Some(item_type) = item.get("type").and_then(Value::as_str) else {
        return ClassifiedItem::Other;
    };
    let Some(call_id) = item
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|call_id| !call_id.is_empty())
    else {
        return ClassifiedItem::Other;
    };

    match classify_item_type(item_type) {
        Some(ToolHistoryItemKind::Call) => ClassifiedItem::Call(call_id),
        Some(ToolHistoryItemKind::Output) => ClassifiedItem::Output(call_id),
        None => ClassifiedItem::Other,
    }
}

fn classify_item_type(item_type: &str) -> Option<ToolHistoryItemKind> {
    // Responses tool families currently use either `<name>_call` plus
    // `<name>_call_output`, or the special tool-search pair. Feature detection
    // by suffix also covers future carriers without treating unrelated input
    // items as tools.
    if item_type.ends_with("_call") {
        Some(ToolHistoryItemKind::Call)
    } else if item_type.ends_with("_call_output") || item_type == "tool_search_output" {
        Some(ToolHistoryItemKind::Output)
    } else {
        None
    }
}

fn output_text(item: &Value) -> String {
    let output = item
        .get("output")
        .or_else(|| item.get("tools"))
        .or_else(|| item.get("result"));

    match output {
        Some(Value::String(text)) => text.clone(),
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
        None => "Tool output call_id was missing.".to_string(),
    }
}

fn is_output_item_without_call_id(item: &Value) -> bool {
    let Some(item_type) = item.get("type").and_then(Value::as_str) else {
        return false;
    };
    if !(item_type.ends_with("_call_output") || item_type == "tool_search_output") {
        return false;
    }

    item.get("call_id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
}

/// Repair malformed tool outputs that have no call id. Such an item cannot be
/// represented as a Responses tool output, but its content can still be kept in
/// the conversation. Returns the number of repaired items.
fn repair_outputs_without_call_id(input: &mut [Value]) -> usize {
    let mut repaired = 0;

    for item in input {
        if !is_output_item_without_call_id(item) {
            continue;
        }

        let text = output_text(item);
        *item = serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": text
            }]
        });
        repaired += 1;
    }

    repaired
}

/// Return `(normalized_index_order, moved_item_count)` when a safe rewrite can
/// be determined. `None` means the history is malformed in a way this narrow
/// compatibility layer must not reinterpret.
fn plan_normalization(items: &[Value]) -> Option<(Vec<usize>, usize)> {
    let mut last_output_index: HashMap<&str, usize> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if let ClassifiedItem::Output(call_id) = classify_item(item) {
            last_output_index.insert(call_id, index);
        }
    }

    let mut order = Vec::with_capacity(items.len());
    let mut pending_calls: Vec<&str> = Vec::new();
    let mut deferred: Vec<usize> = Vec::new();
    let mut moved = 0usize;

    for (index, item) in items.iter().enumerate() {
        match classify_item(item) {
            // Only calls with a matching output can open a batch. An
            // unmatched call outside a batch is ordinary history; inside an
            // open batch it makes the intended chronology ambiguous.
            ClassifiedItem::Call(call_id)
                if last_output_index
                    .get(call_id)
                    .is_some_and(|output_index| *output_index > index) =>
            {
                order.push(index);
                pending_calls.push(call_id);
            }
            ClassifiedItem::Call(_) => {
                // An unmatched call inside an open batch is not something this
                // compatibility layer can safely reinterpret.
                if !pending_calls.is_empty() {
                    return None;
                }
                order.push(index);
            }
            ClassifiedItem::Output(call_id) => {
                if let Some(pending_index) = pending_calls
                    .iter()
                    .position(|pending_call_id| *pending_call_id == call_id)
                {
                    pending_calls.remove(pending_index);
                    order.push(index);

                    if pending_calls.is_empty() {
                        moved += deferred.len();
                        order.append(&mut deferred);
                    }
                } else if pending_calls.is_empty() {
                    order.push(index);
                } else {
                    // An orphan/duplicate output inside a batch also makes the
                    // intended chronology ambiguous. Refuse to rewrite it.
                    return None;
                }
            }
            _ => {
                if pending_calls.is_empty() {
                    order.push(index);
                } else {
                    // Messages, reasoning items, notices, and other calls that
                    // cannot satisfy the open batch are deferred until every
                    // matching output has been emitted.
                    deferred.push(index);
                }
            }
        }
    }

    // This should only happen for inconsistent duplicate call ids. Refuse the
    // rewrite rather than risk moving unrelated history after an unresolved
    // call.
    if !pending_calls.is_empty() {
        return None;
    }

    Some((order, moved))
}

/// Normalize malformed tool history in a native Responses request body.
///
/// Only the top-level `body.input` array is considered. Returns the number of
/// input items changed; zero means the body was not modified. The function is
/// idempotent.
pub(crate) fn normalize_responses_tool_history(body: &mut Value) -> usize {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return 0;
    };
    let repaired = repair_outputs_without_call_id(input);
    let Some((order, moved)) = plan_normalization(input) else {
        return repaired;
    };
    if moved == 0 {
        return repaired;
    }

    let mut slots: Vec<Option<Value>> = std::mem::take(input).into_iter().map(Some).collect();
    let mut normalized = Vec::with_capacity(slots.len());
    for index in order {
        if let Some(item) = slots.get_mut(index).and_then(Option::take) {
            normalized.push(item);
        }
    }
    *input = normalized;
    repaired + moved
}

#[cfg(test)]
mod tests {
    use super::normalize_responses_tool_history;
    use serde_json::{json, Value};

    fn call(call_id: &str) -> Value {
        json!({
            "type": "function_call",
            "call_id": call_id,
            "name": "view_image",
            "arguments": "{}"
        })
    }

    fn image_output(call_id: &str) -> Value {
        json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": [{
                "type": "input_image",
                "image_url": "data:image/png;base64,AAAA"
            }]
        })
    }

    fn text_output(call_id: &str) -> Value {
        json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": "Image viewed successfully."
        })
    }

    fn developer_notice() -> Value {
        json!({
            "type": "message",
            "role": "developer",
            "content": [{
                "type": "input_text",
                "text": "<image_resize_notice>Image resized.</image_resize_notice>"
            }]
        })
    }

    #[test]
    fn moves_parallel_outputs_before_image_resize_notice() {
        let notice = developer_notice();
        let mut body = json!({
            "input": [
                call("call_0"),
                call("call_1"),
                image_output("call_0"),
                notice.clone(),
                text_output("call_1")
            ]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(body["input"][0], call("call_0"));
        assert_eq!(body["input"][1], call("call_1"));
        assert_eq!(body["input"][2], image_output("call_0"));
        assert_eq!(body["input"][3], text_output("call_1"));
        assert_eq!(body["input"][4], notice);
    }

    #[test]
    fn groups_calls_separated_by_a_notice() {
        let notice = developer_notice();
        let mut body = json!({
            "input": [
                call("call_0"),
                notice.clone(),
                call("call_1"),
                text_output("call_0"),
                text_output("call_1")
            ]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(body["input"][0], call("call_0"));
        assert_eq!(body["input"][1], call("call_1"));
        assert_eq!(body["input"][2], text_output("call_0"));
        assert_eq!(body["input"][3], text_output("call_1"));
        assert_eq!(body["input"][4], notice);
    }

    #[test]
    fn supports_custom_and_tool_search_calls() {
        let custom_call = json!({
            "type": "custom_tool_call",
            "call_id": "call_custom",
            "name": "apply_patch",
            "input": "*** Begin Patch"
        });
        let custom_output = json!({
            "type": "custom_tool_call_output",
            "call_id": "call_custom",
            "output": "done"
        });
        let search_call = json!({
            "type": "tool_search_call",
            "call_id": "call_search"
        });
        let search_output = json!({
            "type": "tool_search_output",
            "call_id": "call_search",
            "tools": []
        });
        let notice = developer_notice();
        let mut body = json!({
            "input": [
                custom_call.clone(),
                search_call.clone(),
                custom_output.clone(),
                notice.clone(),
                search_output.clone()
            ]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(body["input"][0], custom_call);
        assert_eq!(body["input"][1], search_call);
        assert_eq!(body["input"][2], custom_output);
        assert_eq!(body["input"][3], search_output);
        assert_eq!(body["input"][4], notice);
    }

    #[test]
    fn keeps_multiple_batches_separate() {
        let first_notice = developer_notice();
        let second_notice = json!({
            "type": "message",
            "role": "developer",
            "content": [{"type": "input_text", "text": "second notice"}]
        });
        let boundary = json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "next turn"}]
        });
        let mut body = json!({
            "input": [
                call("call_0"),
                call("call_1"),
                text_output("call_0"),
                first_notice.clone(),
                text_output("call_1"),
                boundary.clone(),
                call("call_2"),
                call("call_3"),
                text_output("call_2"),
                second_notice.clone(),
                text_output("call_3")
            ]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 2);
        assert_eq!(body["input"][0], call("call_0"));
        assert_eq!(body["input"][1], call("call_1"));
        assert_eq!(body["input"][2], text_output("call_0"));
        assert_eq!(body["input"][3], text_output("call_1"));
        assert_eq!(body["input"][4], first_notice);
        assert_eq!(body["input"][5], boundary);
        assert_eq!(body["input"][6], call("call_2"));
        assert_eq!(body["input"][7], call("call_3"));
        assert_eq!(body["input"][8], text_output("call_2"));
        assert_eq!(body["input"][9], text_output("call_3"));
        assert_eq!(body["input"][10], second_notice);
    }

    #[test]
    fn preserves_contiguous_history_exactly() {
        let notice = developer_notice();
        let mut body = json!({
            "input": [
                call("call_0"),
                call("call_1"),
                image_output("call_0"),
                text_output("call_1"),
                notice
            ],
            "other": {"untouched": true}
        });
        let original = body.clone();

        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn leaves_orphan_output_and_incomplete_call_unchanged() {
        let notice = developer_notice();
        let mut orphan_output = json!({"input": [text_output("call_ghost"), notice.clone()]});
        let orphan_output_original = orphan_output.clone();
        assert_eq!(normalize_responses_tool_history(&mut orphan_output), 0);
        assert_eq!(orphan_output, orphan_output_original);

        let mut incomplete_call = json!({"input": [call("call_open"), notice]});
        let incomplete_call_original = incomplete_call.clone();
        assert_eq!(normalize_responses_tool_history(&mut incomplete_call), 0);
        assert_eq!(incomplete_call, incomplete_call_original);
    }

    #[test]
    fn leaves_reversed_pair_unchanged() {
        // A reversed pair is malformed, but there is no safe chronology to
        // infer without knowing whether it came from an earlier response ID or
        // a corrupted replay. Do not rewrite it speculatively.
        let mut body = json!({"input": [text_output("call_0"), call("call_0")]});
        let original = body.clone();

        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn leaves_unmatched_call_in_open_batch_unchanged() {
        let mut body = json!({
            "input": [
                call("call_0"),
                call("call_incomplete"),
                developer_notice(),
                text_output("call_0")
            ]
        });
        let original = body.clone();

        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn leaves_unmatched_output_in_open_batch_unchanged() {
        let mut body = json!({
            "input": [
                call("call_0"),
                text_output("call_ghost"),
                developer_notice(),
                text_output("call_0")
            ]
        });
        let original = body.clone();

        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn only_processes_array_input() {
        let mut body = json!({
            "input": {
                "type": "function_call",
                "call_id": "call_0"
            }
        });
        let original = body.clone();

        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, original);
    }

    #[test]
    fn normalization_is_idempotent() {
        let mut body = json!({
            "input": [
                call("call_0"),
                call("call_1"),
                text_output("call_0"),
                developer_notice(),
                text_output("call_1")
            ]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        let normalized = body.clone();
        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, normalized);
    }

    #[test]
    fn signatures_keep_original_output_order() {
        let mut body = json!({
            "input": [
                call("call_0"),
                call("call_1"),
                text_output("call_1"),
                developer_notice(),
                text_output("call_0")
            ]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(body["input"][2]["call_id"], "call_1");
        assert_eq!(body["input"][3]["call_id"], "call_0");
    }

    #[test]
    fn converts_output_without_call_id_to_message() {
        let mut body = json!({
            "input": [{
                "type": "function_call_output",
                "id": "fco_missing_call_id",
                "name": "send_message_to_thread",
                "namespace": "codex_app",
                "output": "<codex_delegation>continued</codex_delegation>",
                "internal_chat_message_metadata_passthrough": {
                    "turn_id": "turn_1"
                }
            }]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(
            body["input"][0],
            json!({
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "<codex_delegation>continued</codex_delegation>"
                }]
            })
        );
        assert_eq!(normalize_responses_tool_history(&mut body), 0);
    }

    #[test]
    fn repairs_empty_call_id_and_serializes_structured_output() {
        let output = json!([
            {"type": "input_text", "text": "first"},
            {"type": "input_text", "text": "second"}
        ]);
        let mut body = json!({
            "input": [{
                "type": "custom_tool_call_output",
                "call_id": "",
                "output": output.clone()
            }]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["content"][0]["text"], output.to_string());
    }

    #[test]
    fn converts_tool_search_output_without_call_id() {
        let tools = json!([{"type": "function", "name": "view_image"}]);
        let mut body = json!({
            "input": [{
                "type": "tool_search_output",
                "tools": tools.clone()
            }]
        });

        assert_eq!(normalize_responses_tool_history(&mut body), 1);
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["content"][0]["text"], tools.to_string());
    }

    #[test]
    fn leaves_output_with_call_id_unchanged() {
        let mut body = json!({
            "input": [
                text_output("call_valid"),
                {
                    "type": "function_call_output",
                    "call_id": "call_valid_2",
                    "output": "done",
                    "internal_chat_message_metadata_passthrough": {
                        "turn_id": "turn_1"
                    }
                }
            ]
        });
        let original = body.clone();

        assert_eq!(normalize_responses_tool_history(&mut body), 0);
        assert_eq!(body, original);
    }
}
