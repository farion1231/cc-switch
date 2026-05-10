use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct SseBlockPolicyState {
    pub next_index: usize,
    pub by_upstream: HashMap<usize, UpstreamBlockState>,
    pub dropped_indexes: HashSet<usize>,
    pub pending_suppressed_stops: HashSet<usize>,
    pub message_stopped: bool,
    pub bypass: bool,
}

pub struct UpstreamBlockState {
    pub block_type: String,
    pub down_index: usize,
    pub open: bool,
    pub last_start_block: Option<Value>,
}

pub(crate) fn should_drop_block_type(block_type: &str, thinking_enabled: bool) -> bool {
    if block_type.starts_with("redacted_thinking") {
        return true;
    }
    !thinking_enabled && block_type.contains("thinking")
}

fn parse_event(event: &str) -> Option<(&str, Value)> {
    let mut event_type = None;
    let mut data_str = None;
    for line in event.lines() {
        if let Some(et) = line.strip_prefix("event: ") {
            event_type = Some(et);
        } else if let Some(ds) = line.strip_prefix("data: ") {
            data_str = Some(ds);
        }
    }
    let et = event_type?;
    let ds = data_str?;
    let val = serde_json::from_str(ds).ok()?;
    Some((et, val))
}

fn make_event_str(event_type: &str, data: &Value) -> String {
    format!("event: {}\ndata: {}", event_type, data)
}

pub fn transform_native_sse_block_event(
    event: &str,
    state: &mut SseBlockPolicyState,
    fake_model: &str,
    thinking_enabled: bool,
) -> Vec<String> {
    if state.bypass {
        return vec![event.to_string()];
    }

    let (event_type, mut payload) = match parse_event(event) {
        Some(v) => v,
        None => {
            state.bypass = true;
            return vec![event.to_string()];
        }
    };

    match event_type {
        "message_start" => {
            if let Some(model) = payload
                .get_mut("message")
                .and_then(|m| m.as_object_mut())
                .and_then(|m| m.get_mut("model"))
            {
                *model = Value::String(fake_model.to_string());
            }
            vec![make_event_str("message_start", &payload)]
        }

        "content_block_start" => {
            let up_index = match payload.get("index").and_then(|v| v.as_u64()) {
                Some(i) => i as usize,
                None => {
                    state.bypass = true;
                    return vec![event.to_string()];
                }
            };
            let block_type = match payload
                .get("content_block")
                .and_then(|b| b.get("type"))
                .and_then(|t| t.as_str())
            {
                Some(t) => t.to_string(),
                None => {
                    state.bypass = true;
                    return vec![event.to_string()];
                }
            };

            if should_drop_block_type(&block_type, thinking_enabled) {
                state.dropped_indexes.insert(up_index);
                return vec![];
            }

            let mut out = Vec::new();

            // Synthesize stop for any open blocks
            let open_ups: Vec<usize> = state
                .by_upstream
                .iter()
                .filter(|(_, b)| b.open)
                .map(|(k, _)| *k)
                .collect();
            for open_up in open_ups {
                let bs = state.by_upstream.get_mut(&open_up).unwrap();
                bs.open = false;
                state.pending_suppressed_stops.insert(open_up);
                let down_idx = bs.down_index;
                let stop = json!({"type": "content_block_stop", "index": down_idx});
                out.push(make_event_str("content_block_stop", &stop));
            }

            let down_index = state.next_index;
            state.next_index += 1;

            let mut start_payload = payload.clone();
            if let Some(obj) = start_payload.as_object_mut() {
                obj.insert("index".into(), json!(down_index));
            }

            state.by_upstream.insert(
                up_index,
                UpstreamBlockState {
                    block_type,
                    down_index,
                    open: true,
                    last_start_block: Some(start_payload.clone()),
                },
            );

            out.push(make_event_str("content_block_start", &start_payload));
            out
        }

        "content_block_delta" => {
            let up_index = match payload.get("index").and_then(|v| v.as_u64()) {
                Some(i) => i as usize,
                None => {
                    state.bypass = true;
                    return vec![event.to_string()];
                }
            };

            if state.dropped_indexes.contains(&up_index) {
                return vec![];
            }

            if let Some(delta_type) = payload
                .get("delta")
                .and_then(|d| d.get("type"))
                .and_then(|t| t.as_str())
            {
                if should_drop_block_type(delta_type, thinking_enabled) {
                    return vec![];
                }
            }

            let mut out = Vec::new();

            let down_index = if let Some(bs) = state.by_upstream.get_mut(&up_index) {
                if !bs.open {
                    let new_down = state.next_index;
                    state.next_index += 1;
                    let block_type = bs.block_type.clone();
                    let last_start = bs.last_start_block.clone();
                    bs.down_index = new_down;
                    bs.open = true;

                    let start_data = if let Some(mut s) = last_start {
                        if let Some(obj) = s.as_object_mut() {
                            obj.insert("index".into(), json!(new_down));
                        }
                        s
                    } else {
                        json!({"type": "content_block_start", "index": new_down, "content_block": {"type": block_type}})
                    };
                    out.push(make_event_str("content_block_start", &start_data));
                    new_down
                } else {
                    bs.down_index
                }
            } else {
                let new_down = state.next_index;
                state.next_index += 1;
                let start_data = json!({"type": "content_block_start", "index": new_down, "content_block": {"type": "text"}});
                out.push(make_event_str("content_block_start", &start_data));
                state.by_upstream.insert(
                    up_index,
                    UpstreamBlockState {
                        block_type: "text".into(),
                        down_index: new_down,
                        open: true,
                        last_start_block: None,
                    },
                );
                new_down
            };

            let mut delta_payload = payload.clone();
            if let Some(obj) = delta_payload.as_object_mut() {
                obj.insert("index".into(), json!(down_index));
            }
            out.push(make_event_str("content_block_delta", &delta_payload));
            out
        }

        "content_block_stop" => {
            let up_index = match payload.get("index").and_then(|v| v.as_u64()) {
                Some(i) => i as usize,
                None => {
                    state.bypass = true;
                    return vec![event.to_string()];
                }
            };

            if state.dropped_indexes.contains(&up_index) {
                return vec![];
            }
            if state.pending_suppressed_stops.remove(&up_index) {
                return vec![];
            }

            if let Some(bs) = state.by_upstream.get_mut(&up_index) {
                if bs.open {
                    let down_index = bs.down_index;
                    bs.open = false;
                    let mut stop_payload = payload.clone();
                    if let Some(obj) = stop_payload.as_object_mut() {
                        obj.insert("index".into(), json!(down_index));
                    }
                    return vec![make_event_str("content_block_stop", &stop_payload)];
                }
            }
            vec![]
        }

        _ => vec![event.to_string()],
    }
}

#[cfg(test)]
mod tests_sse_state {
    use super::*;
    use serde_json::json;

    fn make_state() -> SseBlockPolicyState {
        SseBlockPolicyState::default()
    }

    fn make_event(event_type: &str, data: serde_json::Value) -> String {
        format!("event: {}\ndata: {}", event_type, data)
    }

    #[test]
    fn test_bypass_passthrough() {
        let mut state = make_state();
        state.bypass = true;
        let raw = "event: content_block_start\ndata: {\"index\":0}";
        let result = transform_native_sse_block_event(raw, &mut state, "claude-opus-4-7", false);
        assert_eq!(result, vec![raw.to_string()]);
    }

    #[test]
    fn test_message_start_rewrites_model() {
        let mut state = make_state();
        let event = make_event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {"model": "deepseek-v4-pro", "usage": {}}
            }),
        );
        let result =
            transform_native_sse_block_event(&event, &mut state, "claude-opus-4-7", true);
        assert_eq!(result.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(
            result[0]
                .strip_prefix("event: message_start\ndata: ")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(parsed["message"]["model"], "claude-opus-4-7");
    }

    #[test]
    fn test_content_block_start_text_assigned_index() {
        let mut state = make_state();
        let event = make_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", true);
        assert_eq!(result.len(), 1);
        let data_str = result[0]
            .lines()
            .find(|l| l.starts_with("data:"))
            .unwrap()
            .strip_prefix("data: ")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(data_str).unwrap();
        assert_eq!(parsed["index"], 0);
        assert_eq!(state.next_index, 1);
    }

    #[test]
    fn test_thinking_block_dropped_when_disabled() {
        let mut state = make_state();
        let event = make_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "thinking"}
            }),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", false);
        assert!(result.is_empty(), "thinking block should be dropped when disabled");
        assert!(state.dropped_indexes.contains(&0));
    }

    #[test]
    fn test_thinking_block_kept_when_enabled() {
        let mut state = make_state();
        let event = make_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "thinking"}
            }),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", true);
        assert!(!result.is_empty());
        assert!(!state.dropped_indexes.contains(&0));
    }

    #[test]
    fn test_redacted_thinking_always_dropped() {
        let mut state = make_state();
        let event = make_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "redacted_thinking"}
            }),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", true);
        assert!(result.is_empty(), "redacted_thinking always dropped");
        assert!(state.dropped_indexes.contains(&0));
    }

    #[test]
    fn test_content_block_delta_dropped_for_dropped_index() {
        let mut state = make_state();
        state.dropped_indexes.insert(0);
        let event = make_event(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hello"}
            }),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_content_block_delta_remaps_index() {
        let mut state = make_state();
        {
            let event = make_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text"}
                }),
            );
            transform_native_sse_block_event(&event, &mut state, "fake", true);
        }
        let event = make_event(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "hi"}
            }),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", true);
        assert_eq!(result.len(), 1);
        let data_str = result[0]
            .lines()
            .find(|l| l.starts_with("data:"))
            .unwrap()
            .strip_prefix("data: ")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(data_str).unwrap();
        assert_eq!(parsed["index"], 0);
    }

    #[test]
    fn test_content_block_stop_for_dropped_is_empty() {
        let mut state = make_state();
        state.dropped_indexes.insert(2);
        let event = make_event(
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 2}),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_content_block_stop_open_block_remaps_index() {
        let mut state = make_state();
        {
            let event = make_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text"}
                }),
            );
            transform_native_sse_block_event(&event, &mut state, "fake", true);
        }
        let event = make_event(
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 0}),
        );
        let result = transform_native_sse_block_event(&event, &mut state, "fake", true);
        assert_eq!(result.len(), 1);
        assert!(!state.by_upstream.get(&0).unwrap().open);
    }

    #[test]
    fn test_unknown_event_passthrough() {
        let mut state = make_state();
        let raw = "event: ping\ndata: {}";
        let result = transform_native_sse_block_event(raw, &mut state, "fake", false);
        assert_eq!(result, vec![raw.to_string()]);
    }

    #[test]
    fn test_malformed_event_triggers_bypass() {
        let mut state = make_state();
        let raw = "data: {\"type\":\"content_block_start\"}";
        let result = transform_native_sse_block_event(raw, &mut state, "fake", false);
        assert_eq!(result, vec![raw.to_string()]);
        assert!(state.bypass, "malformed event should trigger bypass");
    }

    #[test]
    fn test_two_blocks_open_second_synthesizes_stop_for_first() {
        let mut state = make_state();
        {
            let e = make_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text"}
                }),
            );
            transform_native_sse_block_event(&e, &mut state, "fake", true);
        }
        let e2 = make_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {"type": "text"}
            }),
        );
        let result = transform_native_sse_block_event(&e2, &mut state, "fake", true);
        assert_eq!(result.len(), 2, "should emit synthetic stop + new start");
        assert!(result[0].contains("content_block_stop"));
        assert!(result[1].contains("content_block_start"));
    }

    #[test]
    fn test_index_remapping_is_sequential() {
        let mut state = make_state();
        {
            let e = make_event(
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "thinking"}
                }),
            );
            transform_native_sse_block_event(&e, &mut state, "fake", false);
        }
        let e = make_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {"type": "text"}
            }),
        );
        let result = transform_native_sse_block_event(&e, &mut state, "fake", false);
        assert_eq!(result.len(), 1);
        let data_str = result[0]
            .lines()
            .find(|l| l.starts_with("data:"))
            .unwrap()
            .strip_prefix("data: ")
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(data_str).unwrap();
        assert_eq!(parsed["index"], 0, "first non-dropped block should get index 0");
    }
}
