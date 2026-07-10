//! Shared builders for the OpenAI Responses SSE envelope.
//!
//! The two Codex streaming converters — `streaming_codex_chat` (Chat Completions SSE →
//! Responses SSE) and `streaming_codex_anthropic` (Anthropic Messages SSE → Responses
//! SSE) — have completely different *input* state machines but must emit the identical
//! Responses event stream the Codex client understands. This module owns that output
//! envelope so the two converters cannot drift when an event's shape changes: a wire fix
//! lands here once instead of being mirrored in both files.
//!
//! Each function is pure — it takes primitives or a caller-built `item` `Value` and
//! returns the exact bytes the converters previously constructed inline. Item shapes that
//! vary per converter (including function, namespace, custom, and tool-search calls)
//! are supplied by the caller via the generic
//! `output_item_added` / `output_item_done` helpers.

use bytes::Bytes;
use serde_json::{json, Value};

/// Serialize one Responses SSE event with the standard `event:`/`data:` framing.
pub(crate) fn sse_event(event: &str, data: Value) -> Bytes {
    Bytes::from(format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap_or_default()
    ))
}

// ---------------------------------------------------------------------------
// Response lifecycle (created / in_progress / completed / failed)
// ---------------------------------------------------------------------------

/// `response.created`, wrapping a caller-built `response` object (usage/created_at differ
/// per converter, so the caller supplies the whole object).
pub(crate) fn response_created(response: &Value) -> Bytes {
    sse_event(
        "response.created",
        json!({ "type": "response.created", "response": response }),
    )
}

/// `response.in_progress`.
pub(crate) fn response_in_progress(response: &Value) -> Bytes {
    sse_event(
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": response }),
    )
}

/// `response.completed`.
pub(crate) fn response_completed(response: &Value) -> Bytes {
    sse_event(
        "response.completed",
        json!({ "type": "response.completed", "response": response }),
    )
}

/// `response.failed`.
pub(crate) fn response_failed(response: &Value) -> Bytes {
    sse_event(
        "response.failed",
        json!({ "type": "response.failed", "response": response }),
    )
}

// ---------------------------------------------------------------------------
// Generic output-item add/done (item value supplied by the caller)
// ---------------------------------------------------------------------------

/// `response.output_item.added` with a caller-built item (message / reasoning /
/// function_call / custom_tool_call).
pub(crate) fn output_item_added(output_index: u32, item: &Value) -> Bytes {
    sse_event(
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": output_index,
            "item": item
        }),
    )
}

/// `response.output_item.done` with a caller-built item.
pub(crate) fn output_item_done(output_index: u32, item: &Value) -> Bytes {
    sse_event(
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": output_index,
            "item": item
        }),
    )
}

// ---------------------------------------------------------------------------
// Assistant message (text) lifecycle
// ---------------------------------------------------------------------------

/// `response.output_item.added` for an in-progress assistant message.
pub(crate) fn message_item_added(output_index: u32, item_id: &str) -> Bytes {
    output_item_added(
        output_index,
        &json!({
            "id": item_id,
            "type": "message",
            "status": "in_progress",
            "role": "assistant",
            "content": []
        }),
    )
}

/// `response.content_part.added` for the (empty) output_text part of a message.
pub(crate) fn message_content_part_added(output_index: u32, item_id: &str) -> Bytes {
    sse_event(
        "response.content_part.added",
        json!({
            "type": "response.content_part.added",
            "item_id": item_id,
            "output_index": output_index,
            "content_index": 0,
            "part": { "type": "output_text", "text": "", "annotations": [] }
        }),
    )
}

/// `response.output_text.delta`.
pub(crate) fn output_text_delta(output_index: u32, item_id: &str, delta: &str) -> Bytes {
    sse_event(
        "response.output_text.delta",
        json!({
            "type": "response.output_text.delta",
            "item_id": item_id,
            "output_index": output_index,
            "content_index": 0,
            "delta": delta
        }),
    )
}

/// The completed assistant-message item value.
pub(crate) fn message_item(item_id: &str, text: &str) -> Value {
    json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }]
    })
}

/// Close an assistant message: emits `output_text.done` → `content_part.done` →
/// `output_item.done`, and returns the completed item so the caller can record it.
pub(crate) fn message_close(output_index: u32, item_id: &str, text: &str) -> (Vec<Bytes>, Value) {
    let item = message_item(item_id, text);
    let events = vec![
        sse_event(
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": 0,
                "text": text
            }),
        ),
        sse_event(
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": 0,
                "part": { "type": "output_text", "text": text, "annotations": [] }
            }),
        ),
        output_item_done(output_index, &item),
    ];
    (events, item)
}

// ---------------------------------------------------------------------------
// Reasoning (summary) lifecycle
// ---------------------------------------------------------------------------

/// `response.output_item.added` for an in-progress reasoning item.
pub(crate) fn reasoning_item_added(output_index: u32, item_id: &str) -> Bytes {
    output_item_added(
        output_index,
        &json!({
            "id": item_id,
            "type": "reasoning",
            "status": "in_progress",
            "summary": []
        }),
    )
}

/// `response.reasoning_summary_part.added` for the (empty) summary part.
pub(crate) fn reasoning_summary_part_added(output_index: u32, item_id: &str) -> Bytes {
    sse_event(
        "response.reasoning_summary_part.added",
        json!({
            "type": "response.reasoning_summary_part.added",
            "item_id": item_id,
            "output_index": output_index,
            "summary_index": 0,
            "part": { "type": "summary_text", "text": "" }
        }),
    )
}

/// `response.reasoning_summary_text.delta`.
pub(crate) fn reasoning_summary_text_delta(output_index: u32, item_id: &str, delta: &str) -> Bytes {
    sse_event(
        "response.reasoning_summary_text.delta",
        json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": item_id,
            "output_index": output_index,
            "summary_index": 0,
            "delta": delta
        }),
    )
}

/// The completed reasoning item value (note: no `status` field, matching both converters).
pub(crate) fn reasoning_item(item_id: &str, text: &str) -> Value {
    json!({
        "id": item_id,
        "type": "reasoning",
        "summary": [{ "type": "summary_text", "text": text }]
    })
}

/// Close a reasoning item: emits `reasoning_summary_text.done` →
/// `reasoning_summary_part.done` → `output_item.done`, and returns the completed item.
pub(crate) fn reasoning_close(output_index: u32, item_id: &str, text: &str) -> (Vec<Bytes>, Value) {
    let item = reasoning_item(item_id, text);
    let events = reasoning_close_with_item(output_index, item_id, text, &item, true);
    (events, item)
}

/// Close a reasoning item whose completed shape is supplied by the converter.
/// Anthropic uses this to attach opaque signed/redacted thinking in
/// `encrypted_content` while keeping the standard Responses event lifecycle.
pub(crate) fn reasoning_close_with_item(
    output_index: u32,
    item_id: &str,
    text: &str,
    item: &Value,
    has_visible_summary: bool,
) -> Vec<Bytes> {
    let mut events = Vec::new();
    if has_visible_summary {
        events.extend([
            sse_event(
                "response.reasoning_summary_text.done",
                json!({
                    "type": "response.reasoning_summary_text.done",
                    "item_id": item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "text": text
                }),
            ),
            sse_event(
                "response.reasoning_summary_part.done",
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": text }
                }),
            ),
        ]);
    }
    events.push(output_item_done(output_index, item));
    events
}

// ---------------------------------------------------------------------------
// Tool-call argument streaming (item value supplied by the caller)
// ---------------------------------------------------------------------------

/// `response.function_call_arguments.delta`.
pub(crate) fn function_call_arguments_delta(
    output_index: u32,
    item_id: &str,
    delta: &str,
) -> Bytes {
    sse_event(
        "response.function_call_arguments.delta",
        json!({
            "type": "response.function_call_arguments.delta",
            "item_id": item_id,
            "output_index": output_index,
            "delta": delta
        }),
    )
}

/// `response.function_call_arguments.done`.
pub(crate) fn function_call_arguments_done(
    output_index: u32,
    item_id: &str,
    arguments: &str,
) -> Bytes {
    sse_event(
        "response.function_call_arguments.done",
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": item_id,
            "output_index": output_index,
            "arguments": arguments
        }),
    )
}

/// `response.custom_tool_call_input.delta` (Chat freeform tools only).
pub(crate) fn custom_tool_call_input_delta(output_index: u32, item_id: &str, delta: &str) -> Bytes {
    sse_event(
        "response.custom_tool_call_input.delta",
        json!({
            "type": "response.custom_tool_call_input.delta",
            "item_id": item_id,
            "output_index": output_index,
            "delta": delta
        }),
    )
}

/// `response.custom_tool_call_input.done` (Chat freeform tools only).
pub(crate) fn custom_tool_call_input_done(output_index: u32, item_id: &str, input: &str) -> Bytes {
    sse_event(
        "response.custom_tool_call_input.done",
        json!({
            "type": "response.custom_tool_call_input.done",
            "item_id": item_id,
            "output_index": output_index,
            "input": input
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(bytes: &Bytes) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn sse_event_framing() {
        let ev = sse_event("response.created", json!({ "a": 1 }));
        assert_eq!(body(&ev), "event: response.created\ndata: {\"a\":1}\n\n");
    }

    #[test]
    fn message_close_shapes_match_legacy() {
        let (events, item) = message_close(2, "resp_1_msg", "hi");
        assert_eq!(events.len(), 3);
        assert!(body(&events[0]).contains("\"type\":\"response.output_text.done\""));
        assert!(body(&events[0]).contains("\"text\":\"hi\""));
        assert!(body(&events[1]).contains("\"type\":\"response.content_part.done\""));
        assert!(body(&events[2]).contains("\"type\":\"response.output_item.done\""));
        assert_eq!(item["type"], "message");
        assert_eq!(item["status"], "completed");
        assert_eq!(item["content"][0]["text"], "hi");
    }

    #[test]
    fn reasoning_close_item_has_no_status() {
        let (events, item) = reasoning_close(0, "rs_1", "because");
        assert_eq!(events.len(), 3);
        assert!(body(&events[0]).contains("\"type\":\"response.reasoning_summary_text.done\""));
        assert!(body(&events[1]).contains("\"type\":\"response.reasoning_summary_part.done\""));
        // The completed reasoning item intentionally carries no `status` field.
        assert!(item.get("status").is_none());
        assert_eq!(item["summary"][0]["text"], "because");
    }

    #[test]
    fn message_item_added_is_in_progress() {
        let ev = message_item_added(0, "m1");
        let s = body(&ev);
        assert!(s.contains("\"type\":\"response.output_item.added\""));
        assert!(s.contains("\"status\":\"in_progress\""));
        assert!(s.contains("\"role\":\"assistant\""));
    }

    #[test]
    fn function_call_argument_events() {
        assert!(body(&function_call_arguments_delta(1, "fc_x", "{\"a\":"))
            .contains("\"type\":\"response.function_call_arguments.delta\""));
        assert!(body(&function_call_arguments_done(1, "fc_x", "{\"a\":1}"))
            .contains("\"arguments\":\"{\\\"a\\\":1}\""));
    }
}
