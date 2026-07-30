use super::*;
use serde_json::json;

fn startup_probe() -> Value {
    json!({
        "model": "claude-haiku-4-5",
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "."}]
    })
}

#[test]
fn recognizes_claude_desktop_startup_probe() {
    assert!(is_startup_probe(&startup_probe()));

    let mut non_streaming_probe = startup_probe();
    non_streaming_probe["stream"] = json!(false);
    assert!(is_startup_probe(&non_streaming_probe));
}

#[test]
fn rejects_requests_that_are_not_the_exact_startup_probe() {
    let mutations = [
        ("model", json!("claude-sonnet-4-5")),
        ("max_tokens", json!(2)),
        ("messages", json!([{"role": "user", "content": "hello"}])),
        ("stream", json!(true)),
        ("system", json!("You are helpful")),
    ];

    for (field, value) in mutations {
        let mut body = startup_probe();
        body[field] = value;
        assert!(!is_startup_probe(&body), "field {field} must not match");
    }
}

#[test]
fn rejects_probe_shape_with_additional_message_fields() {
    let body = json!({
        "model": "claude-haiku-4-5",
        "max_tokens": 1,
        "messages": [{
            "role": "user",
            "content": ".",
            "cache_control": {"type": "ephemeral"}
        }]
    });

    assert!(!is_startup_probe(&body));
}

#[test]
fn builds_anthropic_compatible_local_response() {
    assert_eq!(
        response(&startup_probe()),
        json!({
            "id": "msg_cc_switch_startup_probe",
            "type": "message",
            "role": "assistant",
            "model": "claude-haiku-4-5",
            "content": [{"type": "text", "text": "."}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        })
    );
}
