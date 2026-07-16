//! Multi-round reasoning continuation orchestrator (T14).
//!
//! Pins the first successful provider for all continuation rounds.
//! Partial later-round failure returns the last complete successful SSE
//! with `continuation_status = partial_failed`.

use bytes::Bytes;
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::time::Instant;

use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::usage::calculator::CostBreakdown;
use crate::proxy::usage::parser::{CodexReasoningUsage, TokenUsage};

use super::continuation::{
    build_continue_request, decide_continuation, ContinuationDecision, ContinuationEligibility,
    ContinuationStopReason, MAX_CONTINUE_ROUNDS,
};
use super::stream::{concat_sse_rounds, strip_intermediate_completed};
use super::usage::{
    ContinuationRoundRecord, ContinuationRoundResult, RoundUsage, RoundUsageAccumulator,
};

/// Aggregated result of a logical (possibly multi-round) Codex request.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields consumed by forwarder / usage logger (T14 wiring)
pub struct LogicalCodexRequestResult {
    pub client_sse: Bytes,
    pub pinned_provider_id: String,
    pub aggregate_usage: TokenUsage,
    pub aggregate_cost: Option<CostBreakdown>,
    pub reasoning: CodexReasoningUsage,
    pub rounds: Vec<ContinuationRoundRecord>,
    pub first_token_ms: Option<u64>,
    pub duration_ms: u64,
    /// Provider id used for every successful round (same as pinned).
    pub upstream_provider_ids: Vec<String>,
}

/// Sends a single pinned Responses round. Forwarder implements this for real
/// HTTP; tests use in-memory mocks.
pub trait PinnedResponsesSender: Send + Sync {
    fn send_round<'a>(
        &'a self,
        provider: &'a Provider,
        body: Value,
        round_index: u8,
    ) -> BoxFuture<'a, Result<ContinuationRoundResult, AppError>>;
}

/// Optional per-round cost estimator (forwarder plugs real pricing).
pub trait RoundCostEstimator: Send + Sync {
    fn estimate(&self, round: &ContinuationRoundResult) -> Option<CostBreakdown>;
}

/// No-op cost estimator.
pub struct NoCost;

impl RoundCostEstimator for NoCost {
    fn estimate(&self, _round: &ContinuationRoundResult) -> Option<CostBreakdown> {
        None
    }
}

/// Run the multi-round continuation loop on a **pinned** provider.
///
/// - Round 0 is the initial (already-successful) response OR the first send
///   when `initial_round` is `None`.
/// - When `initial_round` is provided, it is treated as round 0 (no re-send).
/// - Later rounds always go to the same `provider` (pinning).
/// - On later-round error: stop, return previous complete SSE, mark
///   `partial_failed`.
pub async fn run_pinned_continuation_loop<S, C>(
    sender: &S,
    cost_estimator: &C,
    provider: &Provider,
    original_effective_request: &Value,
    mut eligibility: ContinuationEligibility,
    initial_round: Option<ContinuationRoundResult>,
    prompt_meta: PromptMeta,
) -> Result<LogicalCodexRequestResult, AppError>
where
    S: PinnedResponsesSender + ?Sized,
    C: RoundCostEstimator + ?Sized,
{
    let wall_start = Instant::now();
    let mut accumulator = RoundUsageAccumulator::new();
    let mut sse_chunks: Vec<Bytes> = Vec::new();
    let mut first_token_ms: Option<u64> = None;
    let mut continuation_status = "not_triggered".to_string();
    let mut partial_error: Option<String> = None;
    let mut upstream_ids: Vec<String> = Vec::new();
    let mut current_body = original_effective_request.clone();
    let max_rounds = eligibility.max_rounds.min(MAX_CONTINUE_ROUNDS);

    // ---- Round 0 ----
    let round0 = if let Some(r) = initial_round {
        r
    } else {
        sender.send_round(provider, current_body.clone(), 0).await?
    };

    first_token_ms = Some(round0.duration_ms.max(1));
    let cost0 = cost_estimator.estimate(&round0);
    accumulator.add_round(&round0, cost0.as_ref())?;
    let mut last_success_sse: Option<Bytes> = Some(round0.sse.clone());
    let mut last_terminal_output: Vec<Value> = round0.terminal_output.clone();
    sse_chunks.push(strip_intermediate_completed(&round0.sse, false));
    upstream_ids.push(provider.id.clone());

    // Decide after round 0
    eligibility.completed_rounds = 0;
    let mut decision = decide_continuation(&terminal_from_round(&round0), &eligibility);
    let record0 = ContinuationRoundRecord::from_result(&round0, &decision, "success", None);
    accumulator.rounds.push(record0);

    // ---- Continuation rounds (1..=max) ----
    let mut completed_extra: u8 = 0;
    while matches!(decision, ContinuationDecision::Continue { .. }) {
        let next_round = eligibility.completed_rounds.saturating_add(1);
        if next_round > max_rounds {
            decision = ContinuationDecision::Stop(ContinuationStopReason::MaximumRoundsReached);
            break;
        }

        // Build continue body from original + last terminal output
        let cont_body = match build_continue_request(
            original_effective_request,
            &last_terminal_output,
            next_round,
        ) {
            Ok(b) => b,
            Err(e) => {
                partial_error = Some(e.to_string());
                continuation_status = "partial_failed".into();
                break;
            }
        };
        current_body = cont_body;

        match sender
            .send_round(provider, current_body.clone(), next_round)
            .await
        {
            Ok(round) => {
                let cost = cost_estimator.estimate(&round);
                accumulator.add_round(&round, cost.as_ref())?;
                last_success_sse = Some(round.sse.clone());
                last_terminal_output = round.terminal_output.clone();
                // Intermediate rounds: strip completed so client only sees final
                let is_final_candidate = false;
                sse_chunks.push(strip_intermediate_completed(&round.sse, is_final_candidate));
                upstream_ids.push(provider.id.clone());
                completed_extra = completed_extra.saturating_add(1);
                eligibility.completed_rounds = next_round;

                decision = decide_continuation(&terminal_from_round(&round), &eligibility);
                let status = if matches!(decision, ContinuationDecision::Continue { .. }) {
                    "success"
                } else {
                    "success_final"
                };
                accumulator
                    .rounds
                    .push(ContinuationRoundRecord::from_result(
                        &round, &decision, status, None,
                    ));
                continuation_status = "continued".into();
            }
            Err(e) => {
                // Partial failure: keep last success, mark partial_failed
                let err_code = e.to_string();
                log::warn!(
                    "codex continuation round {next_round} failed on provider {}: {err_code}",
                    provider.id
                );
                accumulator.rounds.push(ContinuationRoundRecord {
                    round_index: next_round,
                    reasoning_tokens: None,
                    decision: format!("stop:{}", ContinuationStopReason::Disabled.as_str()),
                    status: "partial_failed".into(),
                    duration_ms: 0,
                    error_code: Some(err_code.clone()),
                });
                // Fix decision string for failed attempt
                if let Some(last) = accumulator.rounds.last_mut() {
                    last.decision = "error".into();
                    last.error_code = Some(err_code.clone());
                }
                partial_error = Some(err_code);
                continuation_status = "partial_failed".into();
                break;
            }
        }
    }

    // If we never continued and stopped for a skip reason, surface it
    if continuation_status == "not_triggered" {
        if let ContinuationDecision::Stop(reason) = &decision {
            continuation_status = match reason {
                ContinuationStopReason::Disabled
                | ContinuationStopReason::UnsupportedModel
                | ContinuationStopReason::UnsupportedProtocol
                | ContinuationStopReason::ToolCallPresent
                | ContinuationStopReason::EncryptedReasoningMissing
                | ContinuationStopReason::MissingReasoningTokens
                | ContinuationStopReason::NotLowGrid => "skipped".into(),
                ContinuationStopReason::MaximumRoundsReached => "continued".into(),
            };
        }
    }

    // Rebuild client SSE: intermediate stripped + final with completed
    let mut client_parts: Vec<Bytes> = Vec::new();
    let n = sse_chunks.len();
    for (i, chunk) in sse_chunks.iter().enumerate() {
        let is_last = i + 1 == n;
        if is_last {
            // Prefer last_success_sse with completed kept
            if let Some(ref full) = last_success_sse {
                client_parts.push(strip_intermediate_completed(full, true));
            } else {
                client_parts.push(chunk.clone());
            }
        } else {
            client_parts.push(chunk.clone());
        }
    }
    let client_sse = if client_parts.is_empty() {
        last_success_sse.unwrap_or_default()
    } else {
        concat_sse_rounds(&client_parts)
    };

    let aggregate_usage = TokenUsage {
        input_tokens: accumulator.usage.input_tokens,
        output_tokens: accumulator.usage.output_tokens,
        cache_read_tokens: accumulator.usage.cache_read_tokens,
        cache_creation_tokens: accumulator.usage.cache_creation_tokens,
        model: original_effective_request
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        message_id: None,
    };

    let reasoning = CodexReasoningUsage {
        reasoning_tokens: accumulator.reasoning_tokens,
        reasoning_source: accumulator
            .reasoning_tokens
            .map(|_| "proxy_response".to_string()),
        continuation_status: continuation_status.clone(),
        continuation_rounds: completed_extra as u32,
        turn_id: None,
        prompt_replaced: prompt_meta.prompt_replaced,
        identity_corrected: prompt_meta.identity_corrected,
        prompt_fingerprint: prompt_meta.prompt_fingerprint,
    };

    // Silence unused partial_error in non-log builds (already logged)
    let _ = partial_error;

    Ok(LogicalCodexRequestResult {
        client_sse,
        pinned_provider_id: provider.id.clone(),
        aggregate_usage,
        aggregate_cost: accumulator.total_cost,
        reasoning,
        rounds: accumulator.rounds,
        first_token_ms,
        duration_ms: wall_start.elapsed().as_millis() as u64,
        upstream_provider_ids: upstream_ids,
    })
}

/// Prompt rewrite metadata carried into the logical result.
#[derive(Debug, Clone, Default)]
pub struct PromptMeta {
    pub prompt_replaced: bool,
    pub identity_corrected: bool,
    pub prompt_fingerprint: Option<String>,
}

/// Build a minimal terminal Value from a round result for decide_continuation.
fn terminal_from_round(round: &ContinuationRoundResult) -> Value {
    // Prefer reconstructing from terminal_output + reasoning_tokens
    let mut terminal = serde_json::json!({
        "output": round.terminal_output,
    });
    if let Some(rt) = round.reasoning_tokens {
        terminal["usage"] = serde_json::json!({
            "output_tokens_details": {
                "reasoning_tokens": rt
            }
        });
    }
    // If terminal_output items already form a complete response object, use first
    // object with type response if present — otherwise the synthetic above is fine.
    terminal
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    fn provider_from_json(id: &str) -> Provider {
        serde_json::from_value(json!({
            "id": id,
            "name": id,
            "settingsConfig": {}
        }))
        .expect("provider")
    }

    fn reasoning_output(enc: &str) -> Vec<Value> {
        vec![json!({
            "type": "reasoning",
            "encrypted_content": enc,
        })]
    }

    fn sse_for(round: u8) -> Bytes {
        Bytes::from(format!(
            "event: response.output_item.done\ndata: {{\"round\":{round}}}\n\nevent: response.completed\ndata: {{\"id\":\"r{round}\"}}\n\n"
        ))
    }

    fn make_round(idx: u8, reasoning: u32) -> ContinuationRoundResult {
        ContinuationRoundResult {
            round_index: idx,
            sse: sse_for(idx),
            usage: RoundUsage {
                input_tokens: 10 + idx as u32,
                output_tokens: 20 + idx as u32,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
            reasoning_tokens: Some(reasoning),
            duration_ms: 50 + idx as u64 * 10,
            terminal_output: reasoning_output(&format!("enc-{idx}")),
        }
    }

    struct ScriptedSender {
        /// Pre-scripted results per round index; None = error
        script: Mutex<Vec<Option<ContinuationRoundResult>>>,
        calls: Mutex<Vec<(String, u8)>>,
    }

    impl ScriptedSender {
        fn new(script: Vec<Option<ContinuationRoundResult>>) -> Self {
            Self {
                script: Mutex::new(script),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl PinnedResponsesSender for ScriptedSender {
        fn send_round<'a>(
            &'a self,
            provider: &'a Provider,
            _body: Value,
            round_index: u8,
        ) -> BoxFuture<'a, Result<ContinuationRoundResult, AppError>> {
            Box::pin(async move {
                self.calls
                    .lock()
                    .unwrap()
                    .push((provider.id.clone(), round_index));
                let mut script = self.script.lock().unwrap();
                let idx = round_index as usize;
                if idx >= script.len() {
                    return Err(AppError::Message(format!(
                        "no script for round {round_index}"
                    )));
                }
                // Take ownership of this slot
                match script[idx].take() {
                    Some(r) => Ok(r),
                    None => Err(AppError::Message(format!(
                        "scripted failure round {round_index}"
                    ))),
                }
            })
        }
    }

    fn elig(enabled: bool, max: u8) -> ContinuationEligibility {
        ContinuationEligibility {
            enabled,
            model: "gpt-5.1".into(),
            native_responses: true,
            completed_rounds: 0,
            max_rounds: max,
        }
    }

    #[tokio::test]
    async fn continues_on_low_grid_and_pins_provider() {
        // 516 = n1, 1034 = n2, 1552 = n3 → stop after third success (2 extras)
        let sender = ScriptedSender::new(vec![
            Some(make_round(0, 516)),
            Some(make_round(1, 1034)),
            Some(make_round(2, 1552)),
        ]);
        let provider = provider_from_json("provider-b");
        let req = json!({"model": "gpt-5.1", "input": []});

        let result = run_pinned_continuation_loop(
            &sender,
            &NoCost,
            &provider,
            &req,
            elig(true, 3),
            None,
            PromptMeta::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.pinned_provider_id, "provider-b");
        assert_eq!(
            result.upstream_provider_ids,
            vec!["provider-b", "provider-b", "provider-b"]
        );
        assert_eq!(result.reasoning.continuation_rounds, 2);
        assert_eq!(result.reasoning.continuation_status, "continued");
        assert_eq!(result.reasoning.reasoning_tokens, Some(516 + 1034 + 1552));
        assert_eq!(result.aggregate_usage.input_tokens, 10 + 11 + 12);
        assert_eq!(result.rounds.len(), 3);
        // final SSE should keep completed
        let s = std::str::from_utf8(&result.client_sse).unwrap();
        assert!(s.contains("response.completed"));
    }

    #[tokio::test]
    async fn partial_failure_returns_first_success() {
        let sender = ScriptedSender::new(vec![
            Some(make_round(0, 516)),
            None, // round 1 fails
        ]);
        let provider = provider_from_json("provider-b");
        let req = json!({"model": "gpt-5.1", "input": []});

        let result = run_pinned_continuation_loop(
            &sender,
            &NoCost,
            &provider,
            &req,
            elig(true, 3),
            None,
            PromptMeta::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.reasoning.continuation_status, "partial_failed");
        assert_eq!(result.reasoning.continuation_rounds, 0);
        assert_eq!(result.upstream_provider_ids, vec!["provider-b"]);
        assert!(result.rounds.iter().any(|r| r.status == "partial_failed"));
        // client still gets first success SSE
        let s = std::str::from_utf8(&result.client_sse).unwrap();
        assert!(s.contains("response.completed"));
        assert_eq!(result.reasoning.reasoning_tokens, Some(516));
    }

    #[tokio::test]
    async fn disabled_skips_continuation() {
        let sender = ScriptedSender::new(vec![Some(make_round(0, 516))]);
        let provider = provider_from_json("p1");
        let req = json!({"model": "gpt-5.1", "input": []});

        let result = run_pinned_continuation_loop(
            &sender,
            &NoCost,
            &provider,
            &req,
            elig(false, 3),
            None,
            PromptMeta::default(),
        )
        .await
        .unwrap();

        assert_eq!(result.reasoning.continuation_status, "skipped");
        assert_eq!(result.reasoning.continuation_rounds, 0);
        assert_eq!(result.rounds.len(), 1);
        assert_eq!(sender.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn initial_round_avoids_resend() {
        let sender = ScriptedSender::new(vec![
            // index 0 unused because initial_round provided
            None,
            Some(make_round(1, 1552)), // high grid → stop after this if we continued; but 516 first
        ]);
        // Provide initial with 1552 (n=3) so no continue
        let provider = provider_from_json("p1");
        let req = json!({"model": "gpt-5.1", "input": []});
        let initial = make_round(0, 1552);

        let result = run_pinned_continuation_loop(
            &sender,
            &NoCost,
            &provider,
            &req,
            elig(true, 3),
            Some(initial),
            PromptMeta {
                prompt_replaced: true,
                identity_corrected: false,
                prompt_fingerprint: Some("abc".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(result.reasoning.continuation_rounds, 0);
        assert_eq!(result.reasoning.continuation_status, "skipped");
        assert!(result.reasoning.prompt_replaced);
        assert_eq!(result.reasoning.prompt_fingerprint.as_deref(), Some("abc"));
        // sender should NOT have been called (initial provided, no continue)
        assert!(sender.calls.lock().unwrap().is_empty());
    }
}
