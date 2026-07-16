//! 518-grid continuation decision + continue-request construction.
//!
//! CodexElves-compatible: reasoning_tokens = n * 518 - 2 for n >= 1.
//! Continue only when n ∈ {1, 2} (strictly below MIN_GRID_MULTIPLE).

use serde_json::{json, Value};

use crate::error::AppError;

pub const GRID_STEP: u64 = 518;
pub const GRID_OFFSET: u64 = 2;
pub const MIN_GRID_MULTIPLE: u64 = 3;
pub const MAX_CONTINUE_ROUNDS: u8 = 3;

/// Models known to support encrypted reasoning continuation.
const SUPPORTED_MODEL_PREFIXES: &[&str] = &[
    "gpt-5",
    "o3",
    "o4",
    "codex",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationStopReason {
    Disabled,
    UnsupportedModel,
    UnsupportedProtocol,
    MissingReasoningTokens,
    NotLowGrid,
    ToolCallPresent,
    EncryptedReasoningMissing,
    MaximumRoundsReached,
}

impl ContinuationStopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::UnsupportedModel => "unsupported_model",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::MissingReasoningTokens => "missing_reasoning_tokens",
            Self::NotLowGrid => "not_low_grid",
            Self::ToolCallPresent => "tool_call_present",
            Self::EncryptedReasoningMissing => "encrypted_reasoning_missing",
            Self::MaximumRoundsReached => "maximum_rounds_reached",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContinuationDecision {
    Continue { grid_multiple: u64 },
    Stop(ContinuationStopReason),
}

#[derive(Debug, Clone)]
pub struct ContinuationEligibility {
    pub enabled: bool,
    pub model: String,
    pub native_responses: bool,
    pub completed_rounds: u8,
    pub max_rounds: u8,
}

/// Map observed reasoning_tokens onto the CodexElves 518-grid.
///
/// `tokens + GRID_OFFSET` must be an exact multiple of `GRID_STEP`.
pub fn grid_multiple(reasoning_tokens: u64) -> Option<u64> {
    if reasoning_tokens == 0 {
        return None;
    }
    let shifted = reasoning_tokens.checked_add(GRID_OFFSET)?;
    if shifted % GRID_STEP != 0 {
        return None;
    }
    let n = shifted / GRID_STEP;
    if n == 0 {
        None
    } else {
        Some(n)
    }
}

fn model_supports_continuation(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    // Strip common provider prefixes like "openai/" or "azure/"
    let bare = lower
        .rsplit('/')
        .next()
        .unwrap_or(lower.as_str());
    SUPPORTED_MODEL_PREFIXES
        .iter()
        .any(|p| bare.starts_with(p) || bare.contains(p))
}

fn extract_reasoning_tokens(terminal: &Value) -> Option<u64> {
    // usage.output_tokens_details.reasoning_tokens (Responses)
    // usage.completion_tokens_details.reasoning_tokens (Chat)
    // usage.reasoning_tokens (flat)
    let usage = terminal.get("usage")?;
    if let Some(n) = usage
        .pointer("/output_tokens_details/reasoning_tokens")
        .and_then(|v| v.as_u64())
    {
        return Some(n);
    }
    if let Some(n) = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .and_then(|v| v.as_u64())
    {
        return Some(n);
    }
    usage.get("reasoning_tokens").and_then(|v| v.as_u64())
}

fn output_items(terminal: &Value) -> &[Value] {
    terminal
        .get("output")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

fn has_tool_call(terminal: &Value) -> bool {
    for item in output_items(terminal) {
        let ty = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if matches!(
            ty,
            "function_call"
                | "custom_tool_call"
                | "computer_call"
                | "web_search_call"
                | "file_search_call"
                | "code_interpreter_call"
                | "mcp_call"
                | "tool_call"
        ) {
            return true;
        }
        // Nested content tool_use (Anthropic-style residual)
        if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
            if content.iter().any(|c| {
                matches!(
                    c.get("type").and_then(|t| t.as_str()),
                    Some("tool_use" | "tool_call" | "function_call")
                )
            }) {
                return true;
            }
        }
    }
    false
}

fn has_encrypted_reasoning(terminal: &Value) -> bool {
    for item in output_items(terminal) {
        let ty = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "reasoning" {
            // encrypted_content present and non-empty
            if let Some(enc) = item.get("encrypted_content").and_then(|v| v.as_str()) {
                if !enc.is_empty() {
                    return true;
                }
            }
            // Some payloads nest under content[]
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                if content.iter().any(|c| {
                    c.get("encrypted_content")
                        .and_then(|v| v.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
                        || matches!(
                            c.get("type").and_then(|t| t.as_str()),
                            Some("encrypted_content" | "reasoning_text")
                        )
                }) {
                    return true;
                }
            }
        }
    }
    false
}

/// Decide whether to continue based on terminal response + eligibility.
pub fn decide_continuation(
    terminal: &Value,
    eligibility: &ContinuationEligibility,
) -> ContinuationDecision {
    if !eligibility.enabled {
        return ContinuationDecision::Stop(ContinuationStopReason::Disabled);
    }
    if !eligibility.native_responses {
        return ContinuationDecision::Stop(ContinuationStopReason::UnsupportedProtocol);
    }
    if !model_supports_continuation(&eligibility.model) {
        return ContinuationDecision::Stop(ContinuationStopReason::UnsupportedModel);
    }
    let max = eligibility.max_rounds.min(MAX_CONTINUE_ROUNDS);
    if eligibility.completed_rounds >= max {
        return ContinuationDecision::Stop(ContinuationStopReason::MaximumRoundsReached);
    }
    if has_tool_call(terminal) {
        return ContinuationDecision::Stop(ContinuationStopReason::ToolCallPresent);
    }
    if !has_encrypted_reasoning(terminal) {
        return ContinuationDecision::Stop(ContinuationStopReason::EncryptedReasoningMissing);
    }
    let Some(tokens) = extract_reasoning_tokens(terminal) else {
        return ContinuationDecision::Stop(ContinuationStopReason::MissingReasoningTokens);
    };
    let Some(n) = grid_multiple(tokens) else {
        return ContinuationDecision::Stop(ContinuationStopReason::NotLowGrid);
    };
    // Only continue for low-grid multiples strictly below MIN_GRID_MULTIPLE.
    if n < MIN_GRID_MULTIPLE {
        ContinuationDecision::Continue { grid_multiple: n }
    } else {
        ContinuationDecision::Stop(ContinuationStopReason::NotLowGrid)
    }
}

/// Build a continue request by appending previous output items as input.
///
/// - Keeps original effective request fields (model, tools, instructions, …)
/// - Replaces/extends `input` with prior input + previous_output
/// - Forces `store: false` (ephemeral) and strips previous_response_id
/// - Injects a lightweight continue signal as the last user message
pub fn build_continue_request(
    original_effective_request: &Value,
    previous_output: &[Value],
    round: u8,
) -> Result<Value, AppError> {
    if previous_output.is_empty() {
        return Err(AppError::InvalidInput(
            "continuation requires non-empty previous_output".into(),
        ));
    }

    let mut req = original_effective_request.clone();
    if !req.is_object() {
        return Err(AppError::InvalidInput(
            "original_effective_request must be a JSON object".into(),
        ));
    }

    // Collect prior input items
    let mut input: Vec<Value> = req
        .get("input")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Append previous model output items (reasoning + message) as-is
    for item in previous_output {
        input.push(item.clone());
    }

    // Explicit continue cue for the next round
    input.push(json!({
        "type": "message",
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": format!(
                "[continuation round {}] Continue your incomplete reasoning and finish the response.",
                round
            )
        }]
    }));

    if let Some(obj) = req.as_object_mut() {
        obj.insert("input".into(), Value::Array(input));
        obj.insert("store".into(), Value::Bool(false));
        obj.remove("previous_response_id");
        // Ensure streaming is preserved if original had it; default leave as-is
    }

    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eligible() -> ContinuationEligibility {
        ContinuationEligibility {
            enabled: true,
            model: "gpt-5.1-codex".into(),
            native_responses: true,
            completed_rounds: 0,
            max_rounds: 3,
        }
    }

    fn terminal(reasoning_tokens: u64, with_encrypted: bool, with_tool: bool) -> Value {
        let mut output = vec![];
        if with_encrypted {
            output.push(json!({
                "type": "reasoning",
                "encrypted_content": "enc-abc",
                "summary": []
            }));
        } else {
            output.push(json!({
                "type": "reasoning",
                "summary": []
            }));
        }
        if with_tool {
            output.push(json!({
                "type": "function_call",
                "name": "bash",
                "arguments": "{}"
            }));
        } else {
            output.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "hi"}]
            }));
        }
        json!({
            "output": output,
            "usage": {
                "input_tokens": 100,
                "output_tokens": reasoning_tokens + 10,
                "output_tokens_details": {
                    "reasoning_tokens": reasoning_tokens
                }
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
        let d1 = decide_continuation(&terminal(516, true, false), &eligible());
        assert!(matches!(
            d1,
            ContinuationDecision::Continue { grid_multiple: 1 }
        ));
        let d2 = decide_continuation(&terminal(1034, true, false), &eligible());
        assert!(matches!(
            d2,
            ContinuationDecision::Continue { grid_multiple: 2 }
        ));
        let d3 = decide_continuation(&terminal(1552, true, false), &eligible());
        assert!(matches!(
            d3,
            ContinuationDecision::Stop(ContinuationStopReason::NotLowGrid)
        ));
    }

    #[test]
    fn tool_call_or_missing_encrypted_reasoning_skips_continuation() {
        let tool = decide_continuation(&terminal(516, true, true), &eligible());
        assert_eq!(
            tool,
            ContinuationDecision::Stop(ContinuationStopReason::ToolCallPresent)
        );
        let no_enc = decide_continuation(&terminal(516, false, false), &eligible());
        assert_eq!(
            no_enc,
            ContinuationDecision::Stop(ContinuationStopReason::EncryptedReasoningMissing)
        );
    }

    #[test]
    fn disabled_or_max_rounds_stop() {
        let mut e = eligible();
        e.enabled = false;
        assert_eq!(
            decide_continuation(&terminal(516, true, false), &e),
            ContinuationDecision::Stop(ContinuationStopReason::Disabled)
        );
        e.enabled = true;
        e.completed_rounds = 3;
        assert_eq!(
            decide_continuation(&terminal(516, true, false), &e),
            ContinuationDecision::Stop(ContinuationStopReason::MaximumRoundsReached)
        );
    }

    #[test]
    fn unsupported_protocol_or_model_stop() {
        let mut e = eligible();
        e.native_responses = false;
        assert_eq!(
            decide_continuation(&terminal(516, true, false), &e),
            ContinuationDecision::Stop(ContinuationStopReason::UnsupportedProtocol)
        );
        e.native_responses = true;
        e.model = "gpt-4o".into();
        assert_eq!(
            decide_continuation(&terminal(516, true, false), &e),
            ContinuationDecision::Stop(ContinuationStopReason::UnsupportedModel)
        );
    }

    #[test]
    fn build_continue_request_appends_output_and_cue() {
        let original = json!({
            "model": "gpt-5.1-codex",
            "input": [{"type": "message", "role": "user", "content": [{"type":"input_text","text":"hi"}]}],
            "store": true,
            "previous_response_id": "resp_old"
        });
        let prev = vec![json!({
            "type": "reasoning",
            "encrypted_content": "enc"
        })];
        let req = build_continue_request(&original, &prev, 1).unwrap();
        let input = req.get("input").unwrap().as_array().unwrap();
        assert_eq!(input.len(), 3); // original user + reasoning + continue cue
        assert_eq!(req.get("store"), Some(&json!(false)));
        assert!(req.get("previous_response_id").is_none());
        let last = input.last().unwrap();
        assert_eq!(last.get("role").and_then(|r| r.as_str()), Some("user"));
    }

    #[test]
    fn build_continue_request_rejects_empty_output() {
        let original = json!({"model": "x", "input": []});
        let err = build_continue_request(&original, &[], 1).unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }
}
