//! SpotChecker -- stochastic strong-model inspection of judge results.
//!
//! With a configurable probability (default 10%), a powerful "inspector" model
//! is asked to verify whether the judge's selection was reasonable.  This acts
//! as a lightweight audit layer that catches systematic judge bias without the
//! cost of inspecting every decision.

use crate::orchestration::cross_judge::CrossJudgeResult;
use crate::orchestration::model_caller::ModelCaller;
use crate::orchestration::shuffle::CandidateAnswer;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Internal PRNG (same approach as shuffle.rs to avoid `rand` dependency)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn from_time() -> Self {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xcafe_beef);
        Self {
            state: if ns == 0 { 1 } else { ns },
        }
    }

    fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Return a pseudo-random f64 in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of a spot-check attempt.
#[derive(Debug, Clone)]
pub struct SpotCheckResult {
    /// Whether the spot-check was triggered.
    pub triggered: bool,
    /// `Some(true)` if triggered and inspector agreed with the judge,
    /// `Some(false)` if triggered and inspector disagreed.
    /// `None` if not triggered.
    pub judge_correct: Option<bool>,
    /// The inspector's reasoning (only when triggered).
    pub inspector_reasoning: Option<String>,
}

// ---------------------------------------------------------------------------
// SpotChecker
// ---------------------------------------------------------------------------

/// Probabilistically triggers a strong-model verification of judge results.
pub struct SpotChecker {
    /// The model key for the inspector (e.g. "frontier").
    pub inspector_model: String,
    /// Probability of triggering a check (0.0 to 1.0).  Default 0.1 = 10%.
    pub check_probability: f64,
}

impl SpotChecker {
    /// Create a new spot-checker.
    ///
    /// `check_probability` is clamped to `[0.0, 1.0]`.
    pub fn new(inspector_model: String, check_probability: f64) -> Self {
        Self {
            inspector_model,
            check_probability: check_probability.clamp(0.0, 1.0),
        }
    }

    /// Maybe trigger a spot-check of the judge's decision.
    ///
    /// With `check_probability` chance, calls the inspector model to verify
    /// whether the judge's best-candidate selection was correct.
    pub async fn maybe_check(
        &self,
        prompt: &str,
        candidates: &[CandidateAnswer],
        judge_result: &CrossJudgeResult,
        model_caller: &ModelCaller,
    ) -> Result<SpotCheckResult, String> {
        let mut rng = Xorshift64::from_time();
        self.maybe_check_with_rng(prompt, candidates, judge_result, model_caller, &mut rng)
            .await
    }

    /// Deterministic version for testing (accepts an explicit RNG).
    pub async fn maybe_check_with_rng(
        &self,
        prompt: &str,
        candidates: &[CandidateAnswer],
        judge_result: &CrossJudgeResult,
        model_caller: &ModelCaller,
        rng: &mut Xorshift64,
    ) -> Result<SpotCheckResult, String> {
        if candidates.is_empty() {
            return Ok(SpotCheckResult {
                triggered: false,
                judge_correct: None,
                inspector_reasoning: None,
            });
        }

        let roll = rng.next_f64();
        if roll >= self.check_probability {
            return Ok(SpotCheckResult {
                triggered: false,
                judge_correct: None,
                inspector_reasoning: None,
            });
        }

        // Triggered -- ask the inspector.
        let inspector_prompt =
            Self::format_inspector_prompt(prompt, candidates, judge_result.best_candidate_idx);

        let resp = model_caller
            .call_prompt(&self.inspector_model, "", &inspector_prompt, Some(0.0))
            .await
            .map_err(|e| {
                format!(
                    "Inspector model '{}' call failed: {}",
                    self.inspector_model, e
                )
            })?;

        let (agreed, reasoning) = Self::parse_inspector_response(&resp.content);

        Ok(SpotCheckResult {
            triggered: true,
            judge_correct: Some(agreed),
            inspector_reasoning: Some(reasoning),
        })
    }

    /// Synchronous check whether the spot-check *would* trigger (for testing
    /// probability logic without making model calls).
    pub fn should_trigger(&self, rng: &mut Xorshift64) -> bool {
        rng.next_f64() < self.check_probability
    }

    // -----------------------------------------------------------------------
    // Prompt formatting
    // -----------------------------------------------------------------------

    /// Build the inspector prompt.
    pub fn format_inspector_prompt(
        original_prompt: &str,
        candidates: &[CandidateAnswer],
        judge_best_idx: usize,
    ) -> String {
        let mut body =
            String::from("You are an impartial inspector verifying an AI judge's decision.\n\n");
        body.push_str("The original question was:\n");
        body.push_str(original_prompt);
        body.push_str("\n\n");

        body.push_str(&format!(
            "A judge selected Answer {} as the best response.\n\n",
            judge_best_idx + 1
        ));

        body.push_str("Here are all candidates:\n\n");
        for (i, cand) in candidates.iter().enumerate() {
            body.push_str(&format!(
                "Answer {} (model: hidden_{}):\n{}\n\n",
                i + 1,
                i + 1,
                cand.content
            ));
        }

        body.push_str(
            "Do you agree with the judge's choice? \
             Reply with YES or NO on the first line, followed by your reasoning.\n\
             Example:\n\
             YES\n\
             The selected answer is more accurate because...",
        );

        body
    }

    // -----------------------------------------------------------------------
    // Response parsing
    // -----------------------------------------------------------------------

    /// Parse the inspector's YES/NO response.
    fn parse_inspector_response(text: &str) -> (bool, String) {
        let text = text.trim();
        let first_line = text.lines().next().unwrap_or("").trim().to_uppercase();

        let agreed = first_line.starts_with("YES");

        // Everything after the first line is reasoning.
        let reasoning = text
            .lines()
            .skip(1)
            .collect::<Vec<&str>>()
            .join("\n")
            .trim()
            .to_string();

        (agreed, reasoning)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::cross_judge::{ConsensusLevel, IndividualJudgeScore};

    fn make_candidates(n: usize) -> Vec<CandidateAnswer> {
        (0..n)
            .map(|i| CandidateAnswer {
                model_key: format!("model_{}", i),
                content: format!("answer {}", i),
                quality_score: 0.5,
                latency_ms: 100,
                cost_usd: 0.01,
            })
            .collect()
    }

    fn make_judge_result(best_idx: usize) -> CrossJudgeResult {
        CrossJudgeResult {
            final_score: 0.9,
            best_candidate_idx: best_idx,
            judge_scores: vec![IndividualJudgeScore {
                judge_model: "judge".into(),
                scores: vec![0.9, 0.5],
                ranking: vec![0, 1],
            }],
            consensus_level: ConsensusLevel::High,
        }
    }

    // --- Probability 0.0 never triggers ---

    #[test]
    fn probability_zero_never_triggers() {
        let checker = SpotChecker::new("frontier".into(), 0.0);
        let mut rng = Xorshift64::from_seed(42);
        for _ in 0..100 {
            assert!(
                !checker.should_trigger(&mut rng),
                "Probability 0.0 should never trigger"
            );
        }
    }

    // --- Probability 1.0 always triggers ---

    #[test]
    fn probability_one_always_triggers() {
        let checker = SpotChecker::new("frontier".into(), 1.0);
        let mut rng = Xorshift64::from_seed(42);
        for _ in 0..100 {
            assert!(
                checker.should_trigger(&mut rng),
                "Probability 1.0 should always trigger"
            );
        }
    }

    // --- Probability clamped ---

    #[test]
    fn probability_clamped_to_range() {
        let checker = SpotChecker::new("frontier".into(), -1.0);
        assert_eq!(checker.check_probability, 0.0);

        let checker = SpotChecker::new("frontier".into(), 2.0);
        assert_eq!(checker.check_probability, 1.0);
    }

    // --- Default 10% triggers sometimes but not always ---

    #[test]
    fn ten_percent_triggers_sometimes() {
        let checker = SpotChecker::new("frontier".into(), 0.1);
        let mut rng = Xorshift64::from_seed(12345);
        let mut triggered = 0usize;
        for _ in 0..1000 {
            if checker.should_trigger(&mut rng) {
                triggered += 1;
            }
        }
        // Should be roughly 100 out of 1000, but we accept a wide range.
        assert!(
            triggered > 50 && triggered < 200,
            "Expected ~100 triggers, got {}",
            triggered
        );
    }

    // --- Result structure correct (not triggered) ---

    #[test]
    fn result_structure_not_triggered() {
        let checker = SpotChecker::new("frontier".into(), 0.0);
        let mut rng = Xorshift64::from_seed(42);
        // should_trigger is false -> the result should reflect that.
        assert!(!checker.should_trigger(&mut rng));
    }

    // --- parse_inspector_response ---

    #[test]
    fn parse_yes_response() {
        let (agreed, reasoning) =
            SpotChecker::parse_inspector_response("YES\nThe answer is correct.");
        assert!(agreed);
        assert!(reasoning.contains("The answer is correct."));
    }

    #[test]
    fn parse_no_response() {
        let (agreed, reasoning) =
            SpotChecker::parse_inspector_response("NO\nA different answer is better.");
        assert!(!agreed);
        assert!(reasoning.contains("A different answer is better."));
    }

    #[test]
    fn parse_yes_with_explanation() {
        let (agreed, _) = SpotChecker::parse_inspector_response(
            "YES, I agree with the judge's selection.\nReasoning here.",
        );
        assert!(agreed);
    }

    #[test]
    fn parse_no_with_explanation() {
        let (agreed, _) =
            SpotChecker::parse_inspector_response("NO, I disagree.\nBecause reasons.");
        assert!(!agreed);
    }

    // --- format_inspector_prompt ---

    #[test]
    fn inspector_prompt_includes_question_and_candidates() {
        let cands = make_candidates(2);
        let prompt = SpotChecker::format_inspector_prompt("What is 2+2?", &cands, 0);
        assert!(prompt.contains("What is 2+2?"));
        assert!(prompt.contains("Answer 1"));
        assert!(prompt.contains("Answer 2"));
        assert!(prompt.contains("selected Answer 1"));
        assert!(prompt.contains("hidden_1"));
    }

    #[test]
    fn inspector_prompt_hides_model_identity() {
        let cands = vec![CandidateAnswer {
            model_key: "gpt4".into(),
            content: "Test".into(),
            quality_score: 0.9,
            latency_ms: 100,
            cost_usd: 0.01,
        }];
        let prompt = SpotChecker::format_inspector_prompt("Q", &cands, 0);
        assert!(!prompt.contains("gpt4"));
    }
}
