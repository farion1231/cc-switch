use crate::orchestration::config::ModelConfig;
use crate::orchestration::cross_judge::{ConsensusLevel, CrossJudge, JudgeAggregation, JudgeModel};
use crate::orchestration::engine::OrchestrationDecision;
use crate::orchestration::model_caller::{ModelCaller, ModelResponse};
use crate::orchestration::quality_gate::QualityGate;
use crate::orchestration::shuffle::CandidateShuffler;
use futures::future::join_all;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct StrategyExecutor {
    caller: ModelCaller,
    quality_gate: QualityGate,
    cross_judge: Option<CrossJudge>,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub content: String,
    pub model_used: String,
    pub strategy: String,
    pub total_latency_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cascade_attempts: u32,
    pub verified: bool,
    pub judge_score: Option<f64>,
}

impl StrategyExecutor {
    pub fn new(models: HashMap<String, ModelConfig>) -> Result<Self, String> {
        Ok(Self {
            caller: ModelCaller::new(models)?,
            quality_gate: QualityGate::default(),
            cross_judge: None,
        })
    }

    pub fn with_cross_judge(mut self, judges: Vec<JudgeModel>, aggregation: JudgeAggregation) -> Self {
        self.cross_judge = Some(CrossJudge::new(judges, aggregation));
        self
    }

    pub async fn execute(
        &self,
        decision: &OrchestrationDecision,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        match decision {
            OrchestrationDecision::Passthrough => {
                Err("Passthrough: no execution needed".to_string())
            }

            OrchestrationDecision::Route { model } => {
                self.execute_route(model, messages, tools).await
            }

            OrchestrationDecision::Cascade {
                models,
                quality_threshold,
            } => {
                self.execute_cascade(models, *quality_threshold, messages, tools)
                    .await
            }

            OrchestrationDecision::Debate {
                debaters,
                judge,
                quality_threshold,
                max_rounds,
                critique,
                revision,
            } => {
                self.execute_debate(
                    debaters,
                    judge,
                    *quality_threshold,
                    *max_rounds,
                    *critique,
                    *revision,
                    messages,
                    tools,
                )
                .await
            }

            OrchestrationDecision::MoA {
                proposers,
                aggregator,
                quality_threshold,
            } => {
                self.execute_moa(proposers, aggregator, *quality_threshold, messages, tools)
                    .await
            }
        }
    }

    async fn execute_route(
        &self,
        model_key: &str,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        let start = std::time::Instant::now();
        let resp = self.caller.call(model_key, messages, tools, None).await?;

        Ok(ExecutionResult {
            content: resp.content,
            model_used: resp.model,
            strategy: "route".to_string(),
            total_latency_ms: start.elapsed().as_millis() as u64,
            total_input_tokens: resp.usage.input_tokens,
            total_output_tokens: resp.usage.output_tokens,
            cascade_attempts: 1,
            verified: false,
            judge_score: None,
        })
    }

    async fn execute_cascade(
        &self,
        model_keys: &[String],
        quality_threshold: f64,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        let start = std::time::Instant::now();
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut last_error: Option<String> = None;
        let mut last_success: Option<(String, ExecutionResult)> = None;
        let mut attempts = 0u32;

        for model_key in model_keys {
            attempts += 1;
            match self
                .caller
                .call(model_key, messages.clone(), tools.clone(), None)
                .await
            {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;

                    let quality_result = self
                        .quality_gate
                        .verify(&resp.content, None, Some(&self.caller), None)
                        .await;
                    let score = quality_result.score;

                    if score >= quality_threshold {
                        log::info!(
                            "[Cascade] Model '{}' passed quality check (score={:.2} >= {:.2})",
                            model_key,
                            score,
                            quality_threshold
                        );
                        return Ok(ExecutionResult {
                            content: resp.content,
                            model_used: resp.model,
                            strategy: "cascade".to_string(),
                            total_latency_ms: start.elapsed().as_millis() as u64,
                            total_input_tokens: total_input,
                            total_output_tokens: total_output,
                            cascade_attempts: attempts,
                            verified: true,
                            judge_score: Some(score),
                        });
                    }

                    // Save this response as fallback — even though quality is below threshold,
                    // it's better to return it than an error if nothing else works.
                    log::info!(
                        "[Cascade] Model '{}' below threshold (score={:.2} < {:.2}), escalating",
                        model_key,
                        score,
                        quality_threshold
                    );
                    let fallback_result = ExecutionResult {
                        content: resp.content,
                        model_used: resp.model,
                        strategy: "cascade".to_string(),
                        total_latency_ms: start.elapsed().as_millis() as u64,
                        total_input_tokens: total_input,
                        total_output_tokens: total_output,
                        cascade_attempts: attempts,
                        verified: false,
                        judge_score: Some(score),
                    };
                    last_success = Some((model_key.clone(), fallback_result));
                }
                Err(e) => {
                    log::warn!("[Cascade] Model '{}' failed: {}, trying next", model_key, e);
                    last_error = Some(e);
                }
            }
        }

        // Prefer returning the last (below-threshold) successful response over a hard error.
        if let Some((model_key, result)) = last_success {
            log::warn!(
                "[Cascade] All models below threshold. Returning last response from '{}' (verified=false)",
                model_key
            );
            return Ok(result);
        }

        Err(last_error.unwrap_or_else(|| "All cascade models exhausted".to_string()))
    }

    pub async fn execute_debate(
        &self,
        debater_keys: &[String],
        judge_key: &str,
        quality_threshold: f64,
        max_rounds: u32,
        critique: bool,
        revision: bool,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        let start = std::time::Instant::now();
        let mut total_input = 0u64;
        let mut total_output = 0u64;

        // Fire all debater calls in parallel for lower latency
        let debater_futures: Vec<_> = debater_keys
            .iter()
            .map(|model_key| async {
                let result = self
                    .caller
                    .call(model_key, messages.clone(), tools.clone(), None)
                    .await;
                (model_key.clone(), result)
            })
            .collect();
        let debater_results = join_all(debater_futures).await;

        let mut responses: Vec<(String, ModelResponse)> = Vec::new();
        for (model_key, result) in debater_results {
            match result {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    responses.push((model_key, resp));
                }
                Err(e) => {
                    log::warn!("[Debate] Debater failed: {}", e);
                }
            }
        }

        if responses.is_empty() {
            return Err("All debaters failed".to_string());
        }

        if responses.len() == 1 {
            let (_key, resp) = responses.into_iter().next().unwrap();
            return Ok(ExecutionResult {
                content: resp.content,
                model_used: resp.model,
                strategy: "debate".to_string(),
                total_latency_ms: start.elapsed().as_millis() as u64,
                total_input_tokens: total_input,
                total_output_tokens: total_output,
                cascade_attempts: 1,
                verified: false,
                judge_score: None,
            });
        }

        // Optional critique phase: judge produces critique bullets for the candidates.
        let mut critique_text: Option<String> = None;
        if critique {
            let critique_prompt = Self::build_debate_critique_prompt(&responses);
            let critique_resp = self
                .caller
                .call_prompt(judge_key, DEBATE_JUDGE_SYSTEM, &critique_prompt, Some(0.2))
                .await?;
            total_input += critique_resp.usage.input_tokens;
            total_output += critique_resp.usage.output_tokens;
            critique_text = Some(critique_resp.content);
        }

        // Optional revision phase: each debater revises its answer in light of the critique.
        let mut revised_responses = responses.clone();
        if revision && max_rounds > 0 {
            let critique_for_revision = critique_text.as_deref().unwrap_or("");
            let revision_futures: Vec<_> = responses
                .iter()
                .map(|(model_key, resp)| async {
                    let prompt =
                        Self::build_debate_revision_prompt(&resp.content, critique_for_revision);
                    let result = self
                        .caller
                        .call_prompt(model_key, "", &prompt, Some(0.2))
                        .await;
                    (model_key.clone(), result)
                })
                .collect();
            let revision_results = join_all(revision_futures).await;
            let mut revisions = Vec::new();
            for (model_key, result) in revision_results {
                if let Ok(resp) = result {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    revisions.push((model_key, resp));
                }
            }
            if !revisions.is_empty() {
                revised_responses = revisions;
            }
        }

        // Shuffle responses to prevent position bias in judge evaluation
        let candidates: Vec<crate::orchestration::shuffle::CandidateAnswer> = revised_responses
            .iter()
            .map(|(key, resp)| crate::orchestration::shuffle::CandidateAnswer {
                model_key: key.clone(),
                content: resp.content.clone(),
                quality_score: 0.5, // neutral baseline before judge scoring
                latency_ms: resp.latency_ms,
                cost_usd: 0.0,
            })
            .collect();
        let shuffled = CandidateShuffler::shuffle(candidates);

        // Use CrossJudge if configured, otherwise fall back to single judge
        if let Some(ref cj) = self.cross_judge {
            let original_prompt = messages
                .iter()
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .collect::<Vec<_>>()
                .join("\n");

            let cj_result = cj
                .evaluate(&original_prompt, &shuffled, &self.caller)
                .await
                .map_err(|e| format!("CrossJudge evaluation failed: {}", e))?;

            let best = &shuffled.candidates[cj_result.best_candidate_idx];
            log::info!(
                "[Debate] CrossJudge consensus={:?}, best_score={:.2}, best_model={}",
                cj_result.consensus_level,
                cj_result.final_score,
                best.model_key,
            );

            if cj_result.final_score < quality_threshold {
                log::warn!(
                    "[Debate] CrossJudge score {:.2} below threshold {:.2}; returning fallback error",
                    cj_result.final_score,
                    quality_threshold
                );
                return Err(format!(
                    "quality_threshold_failed: debate score {:.2} below {:.2}",
                    cj_result.final_score, quality_threshold
                ));
            }

            return Ok(ExecutionResult {
                content: best.content.clone(),
                model_used: best.model_key.clone(),
                strategy: "debate".to_string(),
                total_latency_ms: start.elapsed().as_millis() as u64,
                total_input_tokens: total_input,
                total_output_tokens: total_output,
                cascade_attempts: revised_responses.len() as u32,
                verified: cj_result.consensus_level != ConsensusLevel::Low,
                judge_score: Some(cj_result.final_score),
            });
        }

        let debate_summary =
            Self::build_debate_judge_prompt(&revised_responses, critique_text.as_deref());
        let _judge_messages = vec![json!({
            "role": "user",
            "content": debate_summary.clone()
        })];

        let judge_resp = self
            .caller
            .call_prompt(judge_key, DEBATE_JUDGE_SYSTEM, &debate_summary, Some(0.3))
            .await?;

        total_input += judge_resp.usage.input_tokens;
        total_output += judge_resp.usage.output_tokens;

        let score = Self::extract_score_from_judge(&judge_resp.content).unwrap_or(0.0);
        if score < quality_threshold {
            log::warn!(
                "[Debate] judge score {:.2} below threshold {:.2}; returning fallback error",
                score,
                quality_threshold
            );
            return Err(format!(
                "quality_threshold_failed: debate score {:.2} below {:.2}",
                score, quality_threshold
            ));
        }

        Ok(ExecutionResult {
            content: judge_resp.content,
            model_used: judge_resp.model,
            strategy: "debate".to_string(),
            total_latency_ms: start.elapsed().as_millis() as u64,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            cascade_attempts: revised_responses.len() as u32,
            verified: true,
            judge_score: Some(score),
        })
    }

    /// Execute Mixture of Agents (MoA): multiple proposers generate answers,
    /// then an aggregator synthesizes the best final response.
    pub async fn execute_moa(
        &self,
        proposer_keys: &[String],
        aggregator_key: &str,
        quality_threshold: f64,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        let start = std::time::Instant::now();
        let mut total_input = 0u64;
        let mut total_output = 0u64;

        // Phase 1: All proposers generate answers in parallel
        let proposer_futures: Vec<_> = proposer_keys
            .iter()
            .map(|model_key| async {
                let result = self
                    .caller
                    .call(model_key, messages.clone(), tools.clone(), None)
                    .await;
                (model_key.clone(), result)
            })
            .collect();
        let proposer_results = join_all(proposer_futures).await;

        let mut proposals: Vec<(String, ModelResponse)> = Vec::new();
        for (model_key, result) in proposer_results {
            match result {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    proposals.push((model_key, resp));
                }
                Err(e) => {
                    log::warn!("[MoA] Proposer failed: {}", e);
                }
            }
        }

        if proposals.is_empty() {
            return Err("All MoA proposers failed".to_string());
        }

        // Phase 2: Aggregator synthesizes the best answer
        let aggregation_prompt = Self::build_moa_aggregation_prompt(&proposals);
        let aggregator_resp = self
            .caller
            .call_prompt(
                aggregator_key,
                MOA_AGGREGATOR_SYSTEM,
                &aggregation_prompt,
                Some(0.3),
            )
            .await?;

        total_input += aggregator_resp.usage.input_tokens;
        total_output += aggregator_resp.usage.output_tokens;

        // Phase 3: Quality gate on the aggregated result
        let quality_result = self
            .quality_gate
            .verify(&aggregator_resp.content, None, Some(&self.caller), None)
            .await;

        Ok(ExecutionResult {
            content: aggregator_resp.content,
            model_used: aggregator_resp.model,
            strategy: "moa".to_string(),
            total_latency_ms: start.elapsed().as_millis() as u64,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            cascade_attempts: proposals.len() as u32,
            verified: quality_result.passed && quality_result.score >= quality_threshold,
            judge_score: Some(quality_result.score),
        })
    }

    fn build_moa_aggregation_prompt(proposals: &[(String, ModelResponse)]) -> String {
        let mut prompt = String::from(
            "You are synthesizing answers from multiple AI models. Combine the best elements from each proposal into a single superior answer.\n\n",
        );
        for (i, (_key, resp)) in proposals.iter().enumerate() {
            prompt.push_str(&format!(
                "--- Proposal {} ---\n{}\n\n",
                i + 1,
                resp.content
            ));
        }
        prompt.push_str(
            "Synthesize the above proposals into one comprehensive answer. \
             Incorporate the strongest arguments and correct any errors.\n\n\
             Format your response as:\n\
             SCORE: <0.0 to 1.0>\n\
             REASONING: <brief explanation of synthesis>\n\
             ANSWER:\n<your synthesized response>",
        );
        prompt
    }

    fn build_debate_prompt_from_candidates(
        candidates: &[crate::orchestration::shuffle::CandidateAnswer],
    ) -> String {
        let mut prompt = String::from("Multiple AI models have provided answers to the same question. Evaluate and synthesize the best response.\n\n");
        for (i, cand) in candidates.iter().enumerate() {
            prompt.push_str(&format!(
                "--- Answer {} ---\n{}\n\n",
                i + 1,
                cand.content
            ));
        }
        prompt.push_str(
            "Based on the above answers, provide:\n\
             1. A quality score from 0.0 to 1.0\n\
             2. Your synthesized answer combining the best parts\n\
             \n\
             Format:\n\
             SCORE: <number>\n\
             ANSWER:\n<your response>",
        );
        prompt
    }

    fn extract_score_from_judge(content: &str) -> Option<f64> {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("SCORE:") {
                if let Ok(score) = rest.trim().parse::<f64>() {
                    return Some(score.clamp(0.0, 1.0));
                }
            }
        }
        None
    }

    fn build_debate_critique_prompt(candidates: &[(String, ModelResponse)]) -> String {
        let mut prompt = String::from(
            "Review the candidate answers. Identify factual errors, missing requirements, unsafe claims, and format violations.\n\n",
        );
        for (i, (key, resp)) in candidates.iter().enumerate() {
            prompt.push_str(&format!(
                "Candidate {} ({})\n{}\n\n",
                i + 1,
                key,
                resp.content
            ));
        }
        prompt.push_str(
            "Return concise critique bullets and end with SCORE: <0.0 to 1.0> for the strongest candidate.",
        );
        prompt
    }

    fn build_debate_revision_prompt(original_answer: &str, critique: &str) -> String {
        format!(
            "Revise your answer using the critique. Keep correct parts, fix errors, and preserve the user's requested format.\n\nOriginal answer:\n{original_answer}\n\nCritique:\n{critique}\n\nRevised answer:"
        )
    }

    fn build_debate_judge_prompt(
        candidates: &[(String, ModelResponse)],
        critique_text: Option<&str>,
    ) -> String {
        let mut prompt = String::from(
            "You are judging revised candidate answers. Select the best final answer and enforce the quality threshold strictly.\n\n",
        );
        if let Some(critique) = critique_text {
            prompt.push_str("Critique summary:\n");
            prompt.push_str(critique);
            prompt.push_str("\n\n");
        }
        for (i, (key, resp)) in candidates.iter().enumerate() {
            prompt.push_str(&format!(
                "Candidate {} ({})\n{}\n\n",
                i + 1,
                key,
                resp.content
            ));
        }
        prompt.push_str(
            "Return exactly:\nSCORE: <0.0 to 1.0>\nBEST: <candidate number>\nANSWER:\n<final answer>",
        );
        prompt
    }
}

const DEBATE_JUDGE_SYSTEM: &str = "You are an impartial judge evaluating AI model outputs. \
    Synthesize the best answer from multiple candidates. \
    Be strict on factual accuracy and completeness.";

const MOA_AGGREGATOR_SYSTEM: &str = "You are an expert AI aggregator. Your task is to synthesize \
    multiple model proposals into the best possible answer. \
    Identify the strongest arguments, correct any errors, and produce a response \
    that is more accurate and complete than any individual proposal.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_score_from_valid_judge_response() {
        let content = "SCORE: 0.85\nANSWER:\nThe best approach is...";
        assert_eq!(
            StrategyExecutor::extract_score_from_judge(content),
            Some(0.85)
        );
    }

    #[test]
    fn extract_score_clamps_to_range() {
        let content = "SCORE: 1.5\nANSWER:\n...";
        assert_eq!(
            StrategyExecutor::extract_score_from_judge(content),
            Some(1.0)
        );

        let content = "SCORE: -0.2\nANSWER:\n...";
        assert_eq!(
            StrategyExecutor::extract_score_from_judge(content),
            Some(0.0)
        );
    }

    #[test]
    fn extract_score_missing_returns_none() {
        let content = "No score line here, just a regular response.";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), None);
    }
}
