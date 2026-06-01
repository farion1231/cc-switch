use crate::orchestration::config::ModelConfig;
use crate::orchestration::engine::OrchestrationDecision;
use crate::orchestration::model_caller::{ModelCaller, ModelResponse};
use crate::orchestration::quality_gate::QualityGate;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct StrategyExecutor {
    caller: ModelCaller,
    quality_gate: QualityGate,
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
    pub fn new(models: HashMap<String, ModelConfig>) -> Self {
        Self {
            caller: ModelCaller::new(models),
            quality_gate: QualityGate::default(),
        }
    }

    pub async fn execute(
        &self,
        decision: &OrchestrationDecision,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        match decision {
            OrchestrationDecision::Passthrough => Err("Passthrough: no execution needed".to_string()),

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
        let mut last_error = None;
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

                    let quality_result = self.quality_gate.verify(
                        &resp.content,
                        None,
                        Some(&self.caller),
                        None,
                    ).await;
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

                    log::info!(
                        "[Cascade] Model '{}' below threshold (score={:.2} < {:.2}), escalating",
                        model_key,
                        score,
                        quality_threshold
                    );
                }
                Err(e) => {
                    log::warn!(
                        "[Cascade] Model '{}' failed: {}, trying next",
                        model_key,
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "All cascade models exhausted".to_string()))
    }

    pub async fn execute_debate(
        &self,
        debater_keys: &[String],
        judge_key: &str,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> Result<ExecutionResult, String> {
        let start = std::time::Instant::now();
        let mut total_input = 0u64;
        let mut total_output = 0u64;

        let mut responses: Vec<(String, ModelResponse)> = Vec::new();
        for model_key in debater_keys {
            match self
                .caller
                .call(model_key, messages.clone(), tools.clone(), None)
                .await
            {
                Ok(resp) => {
                    total_input += resp.usage.input_tokens;
                    total_output += resp.usage.output_tokens;
                    responses.push((model_key.clone(), resp));
                }
                Err(e) => {
                    log::warn!("[Debate] Debater '{}' failed: {}", model_key, e);
                }
            }
        }

        if responses.is_empty() {
            return Err("All debaters failed".to_string());
        }

        if responses.len() == 1 {
            let (key, resp) = responses.into_iter().next().unwrap();
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

        let debate_summary = Self::build_debate_prompt(&responses);
        let judge_messages = vec![json!({
            "role": "user",
            "content": debate_summary
        })];

        let judge_resp = self
            .caller
            .call_prompt(judge_key, DEBATE_JUDGE_SYSTEM, &debate_summary, Some(0.3))
            .await?;

        total_input += judge_resp.usage.input_tokens;
        total_output += judge_resp.usage.output_tokens;

        let score = Self::extract_score_from_judge(&judge_resp.content);

        Ok(ExecutionResult {
            content: judge_resp.content,
            model_used: judge_resp.model,
            strategy: "debate".to_string(),
            total_latency_ms: start.elapsed().as_millis() as u64,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            cascade_attempts: responses.len() as u32,
            verified: true,
            judge_score: score,
        })
    }

    fn build_debate_prompt(responses: &[(String, ModelResponse)]) -> String {
        let mut prompt = String::from("Multiple AI models have provided answers to the same question. Evaluate and synthesize the best response.\n\n");
        for (i, (_key, resp)) in responses.iter().enumerate() {
            prompt.push_str(&format!("--- Answer {} ---\n{}\n\n", i + 1, resp.content));
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
}

const DEBATE_JUDGE_SYSTEM: &str = "You are an impartial judge evaluating AI model outputs. \
    Synthesize the best answer from multiple candidates. \
    Be strict on factual accuracy and completeness.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_score_from_valid_judge_response() {
        let content = "SCORE: 0.85\nANSWER:\nThe best approach is...";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), Some(0.85));
    }

    #[test]
    fn extract_score_clamps_to_range() {
        let content = "SCORE: 1.5\nANSWER:\n...";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), Some(1.0));

        let content = "SCORE: -0.2\nANSWER:\n...";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), Some(0.0));
    }

    #[test]
    fn extract_score_missing_returns_none() {
        let content = "No score line here, just a regular response.";
        assert_eq!(StrategyExecutor::extract_score_from_judge(content), None);
    }

    #[test]
    fn build_debate_prompt_format() {
        let responses = vec![
            (
                "model_a".to_string(),
                ModelResponse {
                    content: "Answer from A".to_string(),
                    model: "model-a".to_string(),
                    usage: Default::default(),
                    latency_ms: 100,
                },
            ),
            (
                "model_b".to_string(),
                ModelResponse {
                    content: "Answer from B".to_string(),
                    model: "model-b".to_string(),
                    usage: Default::default(),
                    latency_ms: 200,
                },
            ),
        ];
        let prompt = StrategyExecutor::build_debate_prompt(&responses);
        assert!(prompt.contains("Answer 1"));
        assert!(prompt.contains("Answer 2"));
        assert!(prompt.contains("Answer from A"));
        assert!(prompt.contains("Answer from B"));
        assert!(prompt.contains("SCORE:"));
    }

    #[test]
    fn build_debate_prompt_hides_model_identity() {
        let responses = vec![
            (
                "secret_model_alpha".to_string(),
                ModelResponse {
                    content: "Answer A".to_string(),
                    model: "alpha-v2".to_string(),
                    usage: Default::default(),
                    latency_ms: 100,
                },
            ),
            (
                "secret_model_beta".to_string(),
                ModelResponse {
                    content: "Answer B".to_string(),
                    model: "beta-v1".to_string(),
                    usage: Default::default(),
                    latency_ms: 200,
                },
            ),
        ];
        let prompt = StrategyExecutor::build_debate_prompt(&responses);
        assert!(!prompt.contains("secret_model_alpha"), "must not leak model key");
        assert!(!prompt.contains("secret_model_beta"), "must not leak model key");
        assert!(!prompt.contains("alpha-v2"), "must not leak model name");
        assert!(!prompt.contains("beta-v1"), "must not leak model name");
        assert!(prompt.contains("Answer A"), "must include content");
        assert!(prompt.contains("Answer B"), "must include content");
    }
}
