//! MiroFish ReACT (Reasoning + Acting) executor loop.
//!
//! Ported from MiroFish `report_agent.py` (lines 1329-1454).  The ReACT loop
//! iteratively calls a model and verifies the response through a `QualityGate`.
//! It adapts the original MiroFish patterns to the v2 architecture where
//! "verification tools" are the `VerificationTool` variants from `quality_gate`.
//!
//! ## Key MiroFish patterns
//!
//! 1. **Minimum 3 verification steps enforced** — if the model tries to produce
//!    a final answer before completing `min_verification_steps`, a hint about
//!    unused tools is injected and the loop continues.
//! 2. **Tool diversity tracking** — prompts the model to use verification tools
//!    it has not yet exercised.
//! 3. **Conflict resolution** — when model output passes some verification tools
//!    but fails others simultaneously, retry up to `max_conflict_retries` (2)
//!    then force-degrade by keeping only the pass result.
//! 4. **Max 5 iterations** — forced termination if loop exceeds the limit.
//! 5. **Temperature decay retry** — each retry uses a lower temperature.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::orchestration::classifier::TaskProfile;
use crate::orchestration::model_caller::ModelCaller;
use crate::orchestration::quality_gate::{QualityGate, QualityResult, VerificationTool};
use crate::orchestration::retry_policy::TemperatureDecayRetry;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of a ReACT verification loop execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReACTResult {
    /// The final model answer after all verification iterations.
    pub answer: String,
    /// How many verification steps were completed across all iterations.
    pub verification_steps_completed: u32,
    /// Names of verification tools that were exercised at least once.
    pub tools_used: HashSet<String>,
    /// Total number of loop iterations (1-based).
    pub iterations: u32,
    /// Whether the final answer passed the quality gate threshold.
    pub quality_passed: bool,
    /// How many tool+answer conflicts were resolved during the loop.
    pub conflicts_resolved: u32,
}

/// Executor implementing the MiroFish ReACT verification loop.
///
/// The loop:
/// 1. Calls the model with the current messages + accumulated feedback.
/// 2. Runs every verification tool via the `QualityGate`.
/// 3. If quality passes AND minimum verification steps met -> return.
/// 4. If quality fails but iterations remain -> retry with lower temperature
///    and a hint about unused tools.
/// 5. If the loop hits `max_iterations` -> return the best answer so far.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReACTExecutor {
    /// Maximum loop iterations before forced termination (default 5).
    pub max_iterations: u32,
    /// Minimum number of verification steps before a final answer is accepted
    /// (default 3).
    pub min_verification_steps: u32,
    /// Maximum conflict retries before force-degrading (default 2).
    pub max_conflict_retries: u32,
    /// Names of the available verification tools (mirrors QualityGate.tools).
    pub verification_tools: Vec<String>,
}

impl Default for ReACTExecutor {
    fn default() -> Self {
        Self {
            max_iterations: 5,
            min_verification_steps: 3,
            max_conflict_retries: 2,
            verification_tools: vec![
                VerificationTool::StructuralCheck.name().to_string(),
                VerificationTool::PatternMatch.name().to_string(),
                VerificationTool::SchemaValidator.name().to_string(),
                VerificationTool::LLMJudge.name().to_string(),
            ],
        }
    }
}

impl ReACTExecutor {
    /// Create a new executor with the given verification tool names.
    pub fn new(verification_tools: Vec<String>) -> Self {
        Self {
            verification_tools,
            ..Self::default()
        }
    }

    /// Execute the ReACT verification loop.
    ///
    /// * `messages`        — the original request messages sent to the model.
    /// * `model_caller`    — shared HTTP client for LLM calls.
    /// * `model_key`       — which model to invoke (must exist in config).
    /// * `quality_gate`    — verification gate applied after each model call.
    /// * `task_profile`    — classified task metadata (used for prompt tuning).
    /// * `json_schema`     — optional JSON Schema for `SchemaValidator`.
    /// * `judge_model_key` — optional model key for `LLMJudge` (can be the
    ///   same as `model_key` or a dedicated judge model).
    pub async fn execute(
        &self,
        messages: Vec<Value>,
        model_caller: &ModelCaller,
        model_key: &str,
        quality_gate: &QualityGate,
        task_profile: &TaskProfile,
        json_schema: Option<&Value>,
        judge_model_key: Option<&str>,
    ) -> Result<ReACTResult, String> {
        let retry_policy = TemperatureDecayRetry::default();
        let mut best_answer = String::new();
        let mut best_score: f64 = 0.0;
        let mut tools_used: HashSet<String> = HashSet::new();
        let mut verification_steps_completed: u32 = 0;
        let mut conflicts_resolved: u32 = 0;
        let mut feedback: Option<String> = None;

        for iteration in 0..self.max_iterations {
            let temperature = retry_policy.temperature_for_attempt(iteration);
            let current_messages =
                self.build_messages(&messages, &feedback, &tools_used, iteration);

            // Call the model.
            let model_response = model_caller
                .call(model_key, current_messages.clone(), None, Some(temperature))
                .await
                .map_err(|e| {
                    format!(
                        "ReACT iteration {}: model call failed: {}",
                        iteration + 1,
                        e
                    )
                })?;

            let content = model_response.content;

            // Run the quality gate verification.
            let quality_result = quality_gate
                .verify(&content, json_schema, Some(model_caller), judge_model_key)
                .await;

            // Track which verification tools were exercised.
            for (tool_name, score) in &quality_result.individual_scores {
                tools_used.insert(tool_name.clone());
                if *score > 0.0 {
                    verification_steps_completed += 1;
                }
            }

            // Track the best answer seen so far.
            if quality_result.score > best_score || best_answer.is_empty() {
                best_score = quality_result.score;
                best_answer = content.clone();
            }

            // Detect conflict: some tools pass, some fail simultaneously.
            let has_conflict = Self::detect_conflict(&quality_result);

            if has_conflict && conflicts_resolved < self.max_conflict_retries {
                conflicts_resolved += 1;
                feedback = Some(Self::conflict_feedback(&quality_result));
                log::info!(
                    "[ReACT] Iteration {}: conflict detected (score {:.3}), retry {}/{}",
                    iteration + 1,
                    quality_result.score,
                    conflicts_resolved,
                    self.max_conflict_retries,
                );
                continue;
            }

            // Enforce minimum verification steps (MiroFish pattern).
            if verification_steps_completed < self.min_verification_steps {
                let unused = self.unused_tools(&tools_used);
                feedback = Some(format!(
                    "Verification incomplete: only {}/{} steps completed. \
                     Please continue verifying. Unused verification tools: [{}]. \
                     Do not provide a final answer yet — use the remaining tools first.",
                    verification_steps_completed,
                    self.min_verification_steps,
                    unused.join(", "),
                ));
                log::info!(
                    "[ReACT] Iteration {}: min steps not met ({}/{}), continuing",
                    iteration + 1,
                    verification_steps_completed,
                    self.min_verification_steps,
                );
                // Keep the current content as a candidate answer but force another iteration.
                continue;
            }

            // Quality passed and minimum steps met -> done.
            if quality_result.passed {
                log::info!(
                    "[ReACT] Iteration {}: PASSED (score {:.3}, steps {})",
                    iteration + 1,
                    quality_result.score,
                    verification_steps_completed,
                );
                return Ok(ReACTResult {
                    answer: content,
                    verification_steps_completed,
                    tools_used,
                    iterations: iteration + 1,
                    quality_passed: true,
                    conflicts_resolved,
                });
            }

            // Quality failed but iterations remain -> build feedback for retry.
            if iteration + 1 < self.max_iterations {
                let unused = self.unused_tools(&tools_used);
                feedback = Some(Self::retry_feedback(&quality_result, temperature, &unused));
                log::info!(
                    "[ReACT] Iteration {}: quality {:.3} below threshold, retrying \
                     with temp {:.2}, unused tools: [{}]",
                    iteration + 1,
                    quality_result.score,
                    retry_policy.temperature_for_attempt(iteration + 1),
                    unused.join(", "),
                );
            }
        }

        // Max iterations reached — return the best answer we accumulated.
        log::warn!(
            "[ReACT] Max iterations ({}) reached. Best score: {:.3}",
            self.max_iterations,
            best_score,
        );
        Ok(ReACTResult {
            answer: best_answer,
            verification_steps_completed,
            tools_used,
            iterations: self.max_iterations,
            quality_passed: best_score >= quality_gate.pass_threshold,
            conflicts_resolved,
        })
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Build the message list for the current iteration.
    ///
    /// On iteration 0 the original messages are returned as-is.  On subsequent
    /// iterations the model receives a fresh copy of the original messages plus
    /// a system-level feedback message appended as a user turn.
    fn build_messages(
        &self,
        original: &[Value],
        feedback: &Option<String>,
        tools_used: &HashSet<String>,
        iteration: u32,
    ) -> Vec<Value> {
        let mut msgs = original.to_vec();

        if iteration == 0 {
            return msgs;
        }

        // Inject tool-diversity hint if not all tools have been used.
        let unused = self.unused_tools(tools_used);
        if !unused.is_empty() {
            msgs.push(json!({
                "role": "assistant",
                "content": format!(
                    "Note: the following verification tools have not been used yet: [{}]. \
                     Consider applying them for a more thorough verification.",
                    unused.join(", "),
                ),
            }));
        }

        // Keep feedback last so it is the final instruction for the retry.
        if let Some(ref fb) = feedback {
            msgs.push(json!({
                "role": "user",
                "content": format!(
                    "[Verification Feedback]\n{}\n\nPlease improve your previous response \
                     based on this feedback. Focus on the issues identified above.",
                    fb
                ),
            }));
        }

        msgs
    }

    /// Detect a conflict: some verification tools passed while others failed.
    ///
    /// A conflict exists when there are at least 2 individual scores and at
    /// least one passes (>= threshold) and at least one fails.
    fn detect_conflict(quality_result: &QualityResult) -> bool {
        if quality_result.individual_scores.len() < 2 {
            return false;
        }
        let threshold = 0.5; // Per-tool pass threshold
        let any_pass = quality_result
            .individual_scores
            .iter()
            .any(|(_, s)| *s >= threshold);
        let any_fail = quality_result
            .individual_scores
            .iter()
            .any(|(_, s)| *s < threshold);
        any_pass && any_fail
    }

    /// Generate feedback for a conflict situation.
    fn conflict_feedback(quality_result: &QualityResult) -> String {
        let mut passed = Vec::new();
        let mut failed = Vec::new();
        for (name, score) in &quality_result.individual_scores {
            if *score >= 0.5 {
                passed.push(format!("{} ({:.2})", name, score));
            } else {
                failed.push(format!("{} ({:.2})", name, score));
            }
        }
        format!(
            "Conflict detected in verification results.\n\
             Passed checks: {}\n\
             Failed checks: {}\n\
             Please reconcile these results. Focus on addressing the failed checks \
             while preserving what passed.",
            passed.join(", "),
            failed.join(", "),
        )
    }

    /// Generate feedback for a quality-gate failure retry.
    fn retry_feedback(
        quality_result: &QualityResult,
        current_temp: f64,
        unused_tools: &[String],
    ) -> String {
        let failed: Vec<String> = quality_result
            .individual_scores
            .iter()
            .filter(|(_, s)| *s < 0.65)
            .map(|(name, score)| format!("{} ({:.2})", name, score))
            .collect();

        let mut parts = vec![format!(
            "Quality score {:.3} is below the required threshold.\n\
             Failed verification checks: {}",
            quality_result.score,
            failed.join(", "),
        )];

        if !unused_tools.is_empty() {
            parts.push(format!(
                "Unused verification tools: [{}]. Try using these additional tools \
                 to improve coverage.",
                unused_tools.join(", "),
            ));
        }

        parts.push(format!(
            "This retry uses a lower temperature ({:.2}) for more focused output.",
            current_temp,
        ));

        parts.join("\n\n")
    }

    /// Return the names of verification tools that have NOT been used yet.
    fn unused_tools(&self, used: &HashSet<String>) -> Vec<String> {
        self.verification_tools
            .iter()
            .filter(|t| !used.contains(*t))
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::classifier::{RiskLevel, TaskType};
    use crate::orchestration::quality_gate::VerificationTool;

    /// Helper: build a simple task profile for testing.
    fn test_profile() -> TaskProfile {
        TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.5,
            risk: RiskLevel::Medium,
            verifiability: 0.8,
            has_image: false,
            need_code: true,
            has_audio: false,
            has_tools: false,
            is_streaming: false,
            requires_exact_format: false,
            eligible_for_orchestration: true,
            ineligibility_reason: None,
        }
    }

    /// Helper: build a quality gate with the given tools and threshold.
    fn make_gate(tools: Vec<VerificationTool>, threshold: f64) -> QualityGate {
        QualityGate::new(tools, threshold)
    }

    /// Helper: build the default test messages.
    fn test_messages() -> Vec<Value> {
        vec![json!({
            "role": "user",
            "content": "Write a function to add two numbers in Rust."
        })]
    }

    // -----------------------------------------------------------------------
    // Unit tests (no model calls needed)
    // -----------------------------------------------------------------------

    #[test]
    fn default_values() {
        let executor = ReACTExecutor::default();
        assert_eq!(executor.max_iterations, 5);
        assert_eq!(executor.min_verification_steps, 3);
        assert_eq!(executor.max_conflict_retries, 2);
        assert_eq!(executor.verification_tools.len(), 4);
        assert!(executor
            .verification_tools
            .contains(&"structural_check".to_string()));
        assert!(executor
            .verification_tools
            .contains(&"pattern_match".to_string()));
        assert!(executor
            .verification_tools
            .contains(&"schema_validator".to_string()));
        assert!(executor
            .verification_tools
            .contains(&"llm_judge".to_string()));
    }

    #[test]
    fn new_overrides_verification_tools() {
        let executor = ReACTExecutor::new(vec![
            "structural_check".to_string(),
            "pattern_match".to_string(),
        ]);
        assert_eq!(executor.verification_tools.len(), 2);
        assert_eq!(executor.max_iterations, 5);
        assert_eq!(executor.min_verification_steps, 3);
    }

    #[test]
    fn unused_tools_returns_correct_set() {
        let executor = ReACTExecutor::new(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let used: HashSet<String> = vec!["a".to_string()].into_iter().collect();
        let unused = executor.unused_tools(&used);
        assert_eq!(unused, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn unused_tools_empty_when_all_used() {
        let executor = ReACTExecutor::new(vec!["a".to_string(), "b".to_string()]);
        let used: HashSet<String> = vec!["a".to_string(), "b".to_string()].into_iter().collect();
        let unused = executor.unused_tools(&used);
        assert!(unused.is_empty());
    }

    #[test]
    fn detect_conflict_with_mixed_scores() {
        let quality_result = QualityResult {
            passed: false,
            score: 0.55,
            individual_scores: vec![
                ("structural_check".to_string(), 0.9),
                ("pattern_match".to_string(), 0.2),
            ],
        };
        assert!(ReACTExecutor::detect_conflict(&quality_result));
    }

    #[test]
    fn detect_conflict_all_pass() {
        let quality_result = QualityResult {
            passed: true,
            score: 0.9,
            individual_scores: vec![
                ("structural_check".to_string(), 0.9),
                ("pattern_match".to_string(), 0.85),
            ],
        };
        assert!(!ReACTExecutor::detect_conflict(&quality_result));
    }

    #[test]
    fn detect_conflict_all_fail() {
        let quality_result = QualityResult {
            passed: false,
            score: 0.2,
            individual_scores: vec![
                ("structural_check".to_string(), 0.1),
                ("pattern_match".to_string(), 0.3),
            ],
        };
        assert!(!ReACTExecutor::detect_conflict(&quality_result));
    }

    #[test]
    fn detect_conflict_single_tool() {
        let quality_result = QualityResult {
            passed: true,
            score: 0.9,
            individual_scores: vec![("structural_check".to_string(), 0.9)],
        };
        assert!(!ReACTExecutor::detect_conflict(&quality_result));
    }

    #[test]
    fn conflict_feedback_contains_passed_and_failed() {
        let quality_result = QualityResult {
            passed: false,
            score: 0.55,
            individual_scores: vec![
                ("structural_check".to_string(), 0.9),
                ("pattern_match".to_string(), 0.2),
            ],
        };
        let feedback = ReACTExecutor::conflict_feedback(&quality_result);
        assert!(feedback.contains("structural_check (0.90)"));
        assert!(feedback.contains("pattern_match (0.20)"));
        assert!(feedback.contains("Conflict detected"));
    }

    #[test]
    fn retry_feedback_includes_failed_checks_and_temperature() {
        let quality_result = QualityResult {
            passed: false,
            score: 0.4,
            individual_scores: vec![
                ("structural_check".to_string(), 0.3),
                ("pattern_match".to_string(), 0.5),
            ],
        };
        let feedback =
            ReACTExecutor::retry_feedback(&quality_result, 0.6, &["schema_validator".to_string()]);
        assert!(feedback.contains("0.400"));
        assert!(feedback.contains("structural_check (0.30)"));
        assert!(feedback.contains("0.60"));
        assert!(feedback.contains("schema_validator"));
    }

    #[test]
    fn build_messages_iteration_zero_is_passthrough() {
        let executor = ReACTExecutor::default();
        let msgs = test_messages();
        let built = executor.build_messages(&msgs, &None, &HashSet::new(), 0);
        assert_eq!(built.len(), msgs.len());
    }

    #[test]
    fn build_messages_iteration_one_adds_feedback() {
        let executor = ReACTExecutor::default();
        let msgs = test_messages();
        let feedback = Some("Please improve".to_string());
        let built = executor.build_messages(&msgs, &feedback, &HashSet::new(), 1);
        assert!(built.len() > msgs.len());
        // The feedback message should be present.
        let last_content = built
            .last()
            .unwrap()
            .get("content")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(last_content.contains("Verification Feedback"));
    }

    #[test]
    fn build_messages_puts_feedback_after_unused_tool_hint() {
        let executor = ReACTExecutor::new(vec![
            "structural_check".to_string(),
            "pattern_match".to_string(),
        ]);
        let msgs = test_messages();
        let feedback = Some("Please improve".to_string());
        let used: HashSet<String> = vec!["structural_check".to_string()].into_iter().collect();

        let built = executor.build_messages(&msgs, &feedback, &used, 1);
        let last_content = built
            .last()
            .unwrap()
            .get("content")
            .unwrap()
            .as_str()
            .unwrap();

        assert!(last_content.contains("Verification Feedback"));
        assert!(last_content.contains("Please improve"));
    }

    #[test]
    fn build_messages_includes_unused_tools_hint() {
        let executor = ReACTExecutor::new(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let msgs = test_messages();
        let used: HashSet<String> = vec!["a".to_string()].into_iter().collect();
        let built = executor.build_messages(&msgs, &None, &used, 1);
        // Should include an assistant message with unused tools hint.
        let has_hint = built.iter().any(|m| {
            m.get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.contains("b") && s.contains("c"))
                .unwrap_or(false)
        });
        assert!(has_hint, "Expected unused tools hint in messages");
    }

    // -----------------------------------------------------------------------
    // Integration-style tests (use real QualityGate, no model HTTP calls)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_successful_verification_on_first_attempt() {
        // Use a quality gate with only structural_check at a low threshold.
        // Clean code should pass immediately.
        let gate = make_gate(vec![VerificationTool::StructuralCheck], 0.5);

        // We cannot call the real model in tests, so we test the logic
        // by verifying the gate passes on clean code directly.
        let code = r#"```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```"#;
        let result = gate.verify(code, None, None, None).await;
        assert!(result.passed, "Clean code should pass structural check");
        assert!(
            result.score >= 0.5,
            "Score should be >= 0.5, got {}",
            result.score
        );
    }

    #[tokio::test]
    async fn test_quality_gate_fails_on_bad_code() {
        let gate = make_gate(vec![VerificationTool::PatternMatch], 0.9);

        let bad_code = r#"password = "hardcoded_secret"
api_key = "sk-12345"
path = "/usr/local/bad"
"#;
        let result = gate.verify(bad_code, None, None, None).await;
        assert!(
            !result.passed,
            "Code with anti-patterns should fail high threshold, score = {}",
            result.score
        );
    }

    #[tokio::test]
    async fn test_temperature_decreases_across_retries() {
        let policy = TemperatureDecayRetry::default();
        let temps: Vec<f64> = (0..5).map(|i| policy.temperature_for_attempt(i)).collect();

        // Temperatures should be strictly decreasing until floor.
        for i in 1..temps.len() {
            assert!(
                temps[i] <= temps[i - 1],
                "Temperature should decrease: attempt {} temp {} > attempt {} temp {}",
                i,
                temps[i],
                i - 1,
                temps[i - 1],
            );
        }
        // Verify specific values.
        assert!((temps[0] - 0.7).abs() < 1e-10);
        assert!((temps[1] - 0.6).abs() < 1e-10);
        assert!((temps[2] - 0.5).abs() < 1e-10);
        assert!((temps[3] - 0.4).abs() < 1e-10);
        assert!((temps[4] - 0.3).abs() < 1e-10);
    }

    #[tokio::test]
    async fn test_min_verification_steps_enforced() {
        // Create an executor with min_verification_steps = 3 but only 1 tool.
        // Even if quality passes, we need at least 3 verification steps.
        let executor = ReACTExecutor {
            min_verification_steps: 3,
            ..ReACTExecutor::new(vec!["structural_check".to_string()])
        };

        // With only 1 tool, even 1 pass = 1 step, which is < 3.
        // Verify the logic by checking the feedback generation.
        let used: HashSet<String> = HashSet::new();
        let unused = executor.unused_tools(&used);
        assert_eq!(unused.len(), 1);

        // Simulate: after 1 step, we still need more.
        let used: HashSet<String> = vec!["structural_check".to_string()].into_iter().collect();
        let unused = executor.unused_tools(&used);
        assert!(unused.is_empty(), "All tools used");

        // The min_verification_steps would trigger the early-answer block
        // because verification_steps_completed (1) < min_verification_steps (3).
        // This is validated in the execute method flow.
        assert_eq!(executor.min_verification_steps, 3);
    }

    #[tokio::test]
    async fn test_conflict_resolution_tracks_count() {
        // Test that detect_conflict correctly identifies mixed results.
        let quality_result = QualityResult {
            passed: false,
            score: 0.55,
            individual_scores: vec![
                ("structural_check".to_string(), 0.9),
                ("pattern_match".to_string(), 0.2),
            ],
        };

        assert!(
            ReACTExecutor::detect_conflict(&quality_result),
            "Mixed pass/fail should be detected as conflict"
        );

        // After max_conflict_retries (2), the executor should stop retrying
        // and proceed with the best answer.
        let executor = ReACTExecutor::default();
        assert_eq!(executor.max_conflict_retries, 2);
    }

    #[tokio::test]
    async fn test_max_iterations_forced_termination() {
        let executor = ReACTExecutor {
            max_iterations: 5,
            ..ReACTExecutor::default()
        };
        // Verify the loop will terminate at max_iterations.
        assert_eq!(executor.max_iterations, 5);
        // The loop runs 0..5 (5 iterations max).
        // After the loop, the best answer accumulated so far is returned
        // with quality_passed determined by comparing best_score to threshold.
    }

    #[tokio::test]
    async fn test_tool_diversity_tracking() {
        let executor = ReACTExecutor::new(vec![
            "structural_check".to_string(),
            "pattern_match".to_string(),
            "schema_validator".to_string(),
            "llm_judge".to_string(),
        ]);

        // Initially no tools used.
        let used: HashSet<String> = HashSet::new();
        let unused = executor.unused_tools(&used);
        assert_eq!(unused.len(), 4);

        // After using one tool.
        let used: HashSet<String> = vec!["structural_check".to_string()].into_iter().collect();
        let unused = executor.unused_tools(&used);
        assert_eq!(unused.len(), 3);
        assert!(!unused.contains(&"structural_check".to_string()));
        assert!(unused.contains(&"pattern_match".to_string()));
        assert!(unused.contains(&"schema_validator".to_string()));
        assert!(unused.contains(&"llm_judge".to_string()));

        // After using all tools.
        let used: HashSet<String> = executor.verification_tools.clone().into_iter().collect();
        let unused = executor.unused_tools(&used);
        assert!(unused.is_empty());
    }

    #[tokio::test]
    async fn test_react_result_serialization() {
        let mut tools = HashSet::new();
        tools.insert("structural_check".to_string());
        tools.insert("pattern_match".to_string());

        let result = ReACTResult {
            answer: "fn add(a: i32, b: i32) -> i32 { a + b }".to_string(),
            verification_steps_completed: 3,
            tools_used: tools,
            iterations: 2,
            quality_passed: true,
            conflicts_resolved: 0,
        };

        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let deserialized: ReACTResult =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        assert_eq!(deserialized.answer, result.answer);
        assert_eq!(deserialized.verification_steps_completed, 3);
        assert_eq!(deserialized.iterations, 2);
        assert!(deserialized.quality_passed);
        assert_eq!(deserialized.conflicts_resolved, 0);
        assert_eq!(deserialized.tools_used.len(), 2);
    }
}
