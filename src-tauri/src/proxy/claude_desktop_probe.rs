use serde_json::{json, Value};

const STARTUP_PROBE_MODEL: &str = "claude-haiku-4-5";

pub(super) fn is_startup_probe(body: &Value) -> bool {
    let Some(object) = body.as_object() else {
        return false;
    };

    if !object
        .keys()
        .all(|key| matches!(key.as_str(), "model" | "max_tokens" | "messages" | "stream"))
    {
        return false;
    }

    if object.get("model").and_then(Value::as_str) != Some(STARTUP_PROBE_MODEL)
        || object.get("max_tokens").and_then(Value::as_u64) != Some(1)
        || object
            .get("stream")
            .is_some_and(|stream| stream.as_bool() != Some(false))
    {
        return false;
    }

    let Some(messages) = object.get("messages").and_then(Value::as_array) else {
        return false;
    };
    let [message] = messages.as_slice() else {
        return false;
    };
    let Some(message) = message.as_object() else {
        return false;
    };

    message.len() == 2
        && message.get("role").and_then(Value::as_str) == Some("user")
        && message.get("content").and_then(Value::as_str) == Some(".")
}

pub(super) fn response(body: &Value) -> Value {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(STARTUP_PROBE_MODEL);

    json!({
        "id": "msg_cc_switch_startup_probe",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [{"type": "text", "text": "."}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0
        }
    })
}

#[cfg(test)]
#[path = "claude_desktop_probe_tests.rs"]
mod tests;
