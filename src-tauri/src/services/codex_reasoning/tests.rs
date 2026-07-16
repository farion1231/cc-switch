//! Integration-style tests for the codex_reasoning continuation core (T13).

use serde_json::json;

use super::continuation::{
    build_continue_request, decide_continuation, grid_multiple, ContinuationDecision,
    ContinuationEligibility, ContinuationStopReason,
};
use super::stream::{concat_sse_rounds, extract_terminal_output, strip_intermediate_completed};
use super::usage::{ContinuationRoundResult, RoundUsage, RoundUsageAccumulator};
use bytes::Bytes;

fn eligible_gpt5() -> ContinuationEligibility {
    ContinuationEligibility {
        enabled: true,
        model: "gpt-5.1-codex".into(),
        native_responses: true,
        completed_rounds: 0,
        max_rounds: 3,
    }
}

fn fixture(reasoning_tokens: u64, encrypted: bool, tool: bool) -> serde_json::Value {
    let mut output = vec![];
    if encrypted {
        output.push(json!({
            "type": "reasoning",
            "encrypted_content": "enc-xyz",
        }));
    } else {
        output.push(json!({"type": "reasoning"}));
    }
    if tool {
        output.push(json!({"type": "function_call", "name": "x", "arguments": "{}"}));
    } else {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "partial"}]
        }));
    }
    json!({
        "output": output,
        "usage": {
            "output_tokens_details": { "reasoning_tokens": reasoning_tokens }
        }
    })
}

#[test]
fn grid_matches_observed_values() {
    assert_eq!(grid_multiple(516), Some(1));
    assert_eq!(grid_multiple(1034), Some(2));
    assert_eq!(grid_multiple(1552), Some(3));
    assert_eq!(grid_multiple(500), None);
    assert_eq!(grid_multiple(0), None);
}

#[test]
fn only_low_grid_multiples_continue() {
    assert!(matches!(
        decide_continuation(&fixture(516, true, false), &eligible_gpt5()),
        ContinuationDecision::Continue { grid_multiple: 1 }
    ));
    assert!(matches!(
        decide_continuation(&fixture(1034, true, false), &eligible_gpt5()),
        ContinuationDecision::Continue { grid_multiple: 2 }
    ));
    assert!(matches!(
        decide_continuation(&fixture(1552, true, false), &eligible_gpt5()),
        ContinuationDecision::Stop(ContinuationStopReason::NotLowGrid)
    ));
}

#[test]
fn tool_call_or_missing_encrypted_reasoning_skips_continuation() {
    assert_eq!(
        decide_continuation(&fixture(516, true, true), &eligible_gpt5()),
        ContinuationDecision::Stop(ContinuationStopReason::ToolCallPresent)
    );
    assert_eq!(
        decide_continuation(&fixture(516, false, false), &eligible_gpt5()),
        ContinuationDecision::Stop(ContinuationStopReason::EncryptedReasoningMissing)
    );
}

#[test]
fn continue_request_preserves_model_and_appends_output() {
    let original = json!({
        "model": "gpt-5.1-codex",
        "tools": [{"type": "function", "name": "bash"}],
        "input": [{"type": "message", "role": "user", "content": [{"type":"input_text","text":"q"}]}],
        "stream": true
    });
    let prev = extract_terminal_output(&fixture(516, true, false));
    let req = build_continue_request(&original, &prev, 1).unwrap();
    assert_eq!(req["model"], "gpt-5.1-codex");
    assert!(req["tools"].is_array());
    assert_eq!(req["stream"], true);
    assert_eq!(req["store"], false);
    let input = req["input"].as_array().unwrap();
    // original 1 + 2 output items + 1 continue cue
    assert_eq!(input.len(), 4);
}

#[test]
fn stream_helpers_compose() {
    let a = Bytes::from_static(b"data: a\n\n");
    let b = Bytes::from_static(
        b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
    );
    let stripped = strip_intermediate_completed(&b, false);
    assert!(!std::str::from_utf8(&stripped)
        .unwrap()
        .contains("completed"));
    let merged = concat_sse_rounds(&[a, stripped]);
    assert!(std::str::from_utf8(&merged).unwrap().contains("data: a"));
}

#[test]
fn usage_accumulator_records_rounds() {
    let mut acc = RoundUsageAccumulator::new();
    let r = ContinuationRoundResult {
        round_index: 0,
        sse: Bytes::new(),
        usage: RoundUsage {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        },
        reasoning_tokens: Some(516),
        duration_ms: 10,
        terminal_output: vec![],
    };
    acc.add_round(&r, None).unwrap();
    assert_eq!(acc.usage.input_tokens, 1);
    assert_eq!(acc.reasoning_tokens, Some(516));
}
