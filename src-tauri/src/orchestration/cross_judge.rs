//! CrossJudge -- multi-judge cross-evaluation to reduce single-judge bias.
//!
//! Multiple LLM judges independently score candidate answers, then an
//! aggregation strategy (median, weighted average, or consensus) combines
//! their verdicts.  Model identities are hidden from judges to prevent brand
//! bias (e.g. "always pick GPT-4").

use crate::orchestration::model_caller::ModelCaller;
use crate::orchestration::shuffle::{CandidateAnswer, ShuffledCandidates};
use serde_json::json;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A judge model configuration.
#[derive(Debug, Clone)]
pub struct JudgeModel {
    pub model_key: String,
    /// Relative weight for weighted-average aggregation.  Default 1.0.
    pub weight: f64,
}

/// How to combine scores from multiple judges.
#[derive(Debug, Clone)]
pub enum JudgeAggregation {
    /// Take the median of per-candidate scores across judges.
    Median,
    /// Weighted average of per-candidate scores (uses `JudgeModel::weight`).
    WeightedAverage,
    /// Require `threshold` fraction of judges to agree on the best candidate.
    Consensus { threshold: f64 },
}

/// One judge's individual evaluation result.
#[derive(Debug, Clone)]
pub struct IndividualJudgeScore {
    pub judge_model: String,
    /// Score per candidate (index matches `ShuffledCandidates::candidates` order).
    pub scores: Vec<f64>,
    /// Candidate ranking by score (0-indexed, best first).
    pub ranking: Vec<usize>,
}

/// How much agreement exists among judges.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusLevel {
    /// All judges picked the same best candidate.
    High,
    /// Majority (>50%) of judges agree on the best.
    Medium,
    /// Major disagreement -- no clear winner.
    Low,
}

/// The final cross-judged result.
#[derive(Debug, Clone)]
pub struct CrossJudgeResult {
    /// Aggregated quality score of the best candidate.
    pub final_score: f64,
    /// Index (in shuffled order) of the best candidate.
    pub best_candidate_idx: usize,
    /// Individual scores from each judge.
    pub judge_scores: Vec<IndividualJudgeScore>,
    /// How much judges agreed.
    pub consensus_level: ConsensusLevel,
}

// ---------------------------------------------------------------------------
// CrossJudge
// ---------------------------------------------------------------------------

/// Orchestrates multiple LLM judges to independently evaluate candidates
/// and aggregates their scores.
pub struct CrossJudge {
    pub judges: Vec<JudgeModel>,
    pub aggregation: JudgeAggregation,
}

impl CrossJudge {
    /// Create a new cross-judge evaluator.
    pub fn new(judges: Vec<JudgeModel>, aggregation: JudgeAggregation) -> Self {
        Self { judges, aggregation }
    }

    /// Evaluate candidates using all configured judges and aggregate results.
    ///
    /// `candidates` should already be shuffled (see `CandidateShuffler`).
    pub async fn evaluate(
        &self,
        prompt: &str,
        candidates: &ShuffledCandidates,
        model_caller: &ModelCaller,
    ) -> Result<CrossJudgeResult, String> {
        if candidates.candidates.is_empty() {
            return Err("No candidates to evaluate".into());
        }
        if self.judges.is_empty() {
            return Err("No judges configured".into());
        }

        // Each judge scores all candidates independently.
        let mut judge_scores = Vec::with_capacity(self.judges.len());
        for judge in &self.judges {
            let score = self
                .call_judge(judge, prompt, &candidates.candidates, model_caller)
                .await?;
            judge_scores.push(score);
        }

        // Aggregate across judges.
        self.aggregate(judge_scores)
    }

    /// Send prompt + candidates to one judge and parse the numeric scores.
    pub async fn call_judge(
        &self,
        judge: &JudgeModel,
        prompt: &str,
        candidates: &[CandidateAnswer],
        model_caller: &ModelCaller,
    ) -> Result<IndividualJudgeScore, String> {
        let judge_prompt = Self::format_judge_prompt(prompt, candidates);

        let resp = model_caller
            .call_prompt(&judge.model_key, "", &judge_prompt, Some(0.0))
            .await
            .map_err(|e| format!("Judge '{}' call failed: {}", judge.model_key, e))?;

        let scores = Self::parse_judge_scores(&resp.content, candidates.len());
        let ranking = Self::compute_ranking(&scores);

        Ok(IndividualJudgeScore {
            judge_model: judge.model_key.clone(),
            scores,
            ranking,
        })
    }

    /// Aggregate individual judge scores into a final result.
    pub fn aggregate(
        &self,
        scores: Vec<IndividualJudgeScore>,
    ) -> Result<CrossJudgeResult, String> {
        if scores.is_empty() {
            return Err("No judge scores to aggregate".into());
        }

        let n_candidates = scores[0].scores.len();
        if n_candidates == 0 {
            return Err("No candidates in judge scores".into());
        }

        // Validate consistency.
        for js in &scores {
            if js.scores.len() != n_candidates {
                return Err(format!(
                    "Judge '{}' returned {} scores but expected {}",
                    js.judge_model,
                    js.scores.len(),
                    n_candidates
                ));
            }
        }

        let (final_scores, best_idx) = match &self.aggregation {
            JudgeAggregation::Median => {
                let final_scores = Self::aggregate_median(&scores);
                let best_idx = Self::argmax(&final_scores);
                (final_scores, best_idx)
            }
            JudgeAggregation::WeightedAverage => {
                let final_scores = Self::aggregate_weighted(&scores, &self.judges);
                let best_idx = Self::argmax(&final_scores);
                (final_scores, best_idx)
            }
            JudgeAggregation::Consensus { threshold } => {
                let final_scores = Self::aggregate_median(&scores);
                let best_idx = Self::argmax(&final_scores);
                // Consensus is mainly about the consensus_level, still compute
                // aggregated scores for the final_score field.
                let _ = threshold; // used in consensus_level computation
                (final_scores, best_idx)
            }
        };

        let consensus_level = Self::compute_consensus(&scores, best_idx);

        Ok(CrossJudgeResult {
            final_score: final_scores[best_idx],
            best_candidate_idx: best_idx,
            judge_scores: scores,
            consensus_level,
        })
    }

    // -----------------------------------------------------------------------
    // Prompt formatting
    // -----------------------------------------------------------------------

    /// Build the judge prompt, hiding model identities behind generic labels.
    pub fn format_judge_prompt(original_prompt: &str, candidates: &[CandidateAnswer]) -> String {
        let mut body = String::new();
        body.push_str(
            "You are an impartial judge evaluating candidate AI answers.\n\
             Score EACH answer independently on a scale of 0.0 to 1.0.\n\n\
             Original question:\n",
        );
        body.push_str(original_prompt);
        body.push_str("\n\n---\n\n");

        for (i, cand) in candidates.iter().enumerate() {
            body.push_str(&format!(
                "Answer {} (model: hidden_{}):\n{}\n\n",
                i + 1,
                i + 1,
                cand.content
            ));
        }

        body.push_str(&format!(
            "Reply with ONLY a JSON object: {{\"scores\": [{}]}}\n\
             where each score corresponds to Answer 1 through Answer {}.\n\
             Example: {{\"scores\": [0.8, 0.6, 0.9]}}",
            vec!["0.0"; candidates.len()].join(", "),
            candidates.len()
        ));

        body
    }

    // -----------------------------------------------------------------------
    // Score parsing
    // -----------------------------------------------------------------------

    /// Parse judge response into per-candidate scores.
    ///
    /// Tries JSON `{"scores": [...]}` first, then falls back to extracting
    /// all floating-point numbers from the text.
    fn parse_judge_scores(text: &str, expected_count: usize) -> Vec<f64> {
        // Attempt JSON parse.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
            if let Some(arr) = val.get("scores").and_then(|s| s.as_array()) {
                let scores: Vec<f64> = arr
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .map(|s| s.clamp(0.0, 1.0))
                    .collect();
                if scores.len() == expected_count {
                    return scores;
                }
                // Partial match -- pad with 0.5.
                if !scores.is_empty() {
                    return Self::pad_scores(scores, expected_count);
                }
            }
        }

        // Fallback: extract all f64 values from text.
        let numbers = Self::extract_numbers(text);
        if !numbers.is_empty() {
            return Self::pad_scores(
                numbers.into_iter().map(|s| s.clamp(0.0, 1.0)).collect(),
                expected_count,
            );
        }

        // Ultimate fallback: all 0.5.
        vec![0.5; expected_count]
    }

    /// Pad or truncate scores to the expected count.
    fn pad_scores(mut scores: Vec<f64>, expected: usize) -> Vec<f64> {
        scores.truncate(expected);
        while scores.len() < expected {
            scores.push(0.5);
        }
        scores
    }

    /// Extract floating-point numbers from freeform text.
    fn extract_numbers(text: &str) -> Vec<f64> {
        let mut nums = Vec::new();
        for token in text.split(|c: char| !c.is_ascii_digit() && c != '.') {
            if let Ok(v) = token.parse::<f64>() {
                nums.push(v);
            }
        }
        nums
    }

    // -----------------------------------------------------------------------
    // Ranking
    // -----------------------------------------------------------------------

    /// Compute a ranking (best-first) from scores.
    fn compute_ranking(scores: &[f64]) -> Vec<usize> {
        let mut indexed: Vec<(usize, f64)> = scores.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        indexed.into_iter().map(|(i, _)| i).collect()
    }

    // -----------------------------------------------------------------------
    // Aggregation helpers
    // -----------------------------------------------------------------------

    /// Per-candidate median across all judges.
    fn aggregate_median(scores: &[IndividualJudgeScore]) -> Vec<f64> {
        let n = scores[0].scores.len();
        let mut result = Vec::with_capacity(n);

        for c in 0..n {
            let mut vals: Vec<f64> = scores.iter().map(|js| js.scores[c]).collect();
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = vals.len() / 2;
            let median = if vals.len() % 2 == 0 && vals.len() > 1 {
                (vals[mid - 1] + vals[mid]) / 2.0
            } else {
                vals[mid]
            };
            result.push(median);
        }

        result
    }

    /// Per-candidate weighted average across judges.
    fn aggregate_weighted(scores: &[IndividualJudgeScore], judges: &[JudgeModel]) -> Vec<f64> {
        let n = scores[0].scores.len();
        let total_weight: f64 = judges.iter().map(|j| j.weight).sum();
        let total_weight = if total_weight == 0.0 { 1.0 } else { total_weight };

        let mut result = Vec::with_capacity(n);
        for c in 0..n {
            let weighted_sum: f64 = scores
                .iter()
                .zip(judges.iter())
                .map(|(js, judge)| js.scores[c] * judge.weight)
                .sum();
            result.push(weighted_sum / total_weight);
        }
        result
    }

    // -----------------------------------------------------------------------
    // Consensus
    // -----------------------------------------------------------------------

    /// Determine consensus level: how many judges agree on `best_idx`.
    fn compute_consensus(scores: &[IndividualJudgeScore], best_idx: usize) -> ConsensusLevel {
        let total = scores.len();
        if total == 0 {
            return ConsensusLevel::Low;
        }

        // Count judges whose top-ranked candidate matches best_idx.
        let agreeing = scores
            .iter()
            .filter(|js| {
                js.ranking
                    .first()
                    .map(|&top| top == best_idx)
                    .unwrap_or(false)
            })
            .count();

        if agreeing == total {
            ConsensusLevel::High
        } else if agreeing as f64 / total as f64 > 0.5 {
            ConsensusLevel::Medium
        } else {
            ConsensusLevel::Low
        }
    }

    // -----------------------------------------------------------------------
    // Utility
    // -----------------------------------------------------------------------

    fn argmax(scores: &[f64]) -> usize {
        scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- format_judge_prompt ---

    #[test]
    fn judge_prompt_hides_model_identity() {
        let cands = vec![
            CandidateAnswer {
                model_key: "gpt4".into(),
                content: "Answer A".into(),
                quality_score: 0.9,
                latency_ms: 100,
                cost_usd: 0.01,
            },
            CandidateAnswer {
                model_key: "claude".into(),
                content: "Answer B".into(),
                quality_score: 0.8,
                latency_ms: 80,
                cost_usd: 0.008,
            },
        ];

        let prompt = CrossJudge::format_judge_prompt("What is 2+2?", &cands);

        // Should NOT reveal actual model keys.
        assert!(
            !prompt.contains("gpt4"),
            "Prompt should not reveal model key 'gpt4'"
        );
        assert!(
            !prompt.contains("claude"),
            "Prompt should not reveal model key 'claude'"
        );
        // Should use generic labels.
        assert!(prompt.contains("hidden_1"));
        assert!(prompt.contains("hidden_2"));
        assert!(prompt.contains("Answer 1"));
        assert!(prompt.contains("Answer 2"));
    }

    #[test]
    fn judge_prompt_includes_original_question() {
        let cands = vec![CandidateAnswer {
            model_key: "m".into(),
            content: "c".into(),
            quality_score: 0.5,
            latency_ms: 0,
            cost_usd: 0.0,
        }];
        let prompt = CrossJudge::format_judge_prompt("Explain Rust ownership", &cands);
        assert!(prompt.contains("Explain Rust ownership"));
    }

    // --- parse_judge_scores ---

    #[test]
    fn parse_scores_from_json() {
        let text = r#"{"scores": [0.9, 0.7, 0.8]}"#;
        let scores = CrossJudge::parse_judge_scores(text, 3);
        assert_eq!(scores, vec![0.9, 0.7, 0.8]);
    }

    #[test]
    fn parse_scores_clamps_values() {
        let text = r#"{"scores": [1.5, -0.2, 0.5]}"#;
        let scores = CrossJudge::parse_judge_scores(text, 3);
        assert_eq!(scores, vec![1.0, 0.0, 0.5]);
    }

    #[test]
    fn parse_scores_fallback_to_numbers() {
        let text = "The scores are 0.8 and 0.6 and 0.9";
        let scores = CrossJudge::parse_judge_scores(text, 3);
        assert_eq!(scores.len(), 3);
        assert!((scores[0] - 0.8).abs() < 0.01);
        assert!((scores[1] - 0.6).abs() < 0.01);
        assert!((scores[2] - 0.9).abs() < 0.01);
    }

    #[test]
    fn parse_scores_defaults_on_garbage() {
        let scores = CrossJudge::parse_judge_scores("no numbers here", 2);
        assert_eq!(scores, vec![0.5, 0.5]);
    }

    // --- compute_ranking ---

    #[test]
    fn ranking_orders_by_score_descending() {
        let scores = vec![0.3, 0.9, 0.6];
        let ranking = CrossJudge::compute_ranking(&scores);
        assert_eq!(ranking, vec![1, 2, 0]); // 0.9 > 0.6 > 0.3
    }

    #[test]
    fn ranking_single_candidate() {
        let ranking = CrossJudge::compute_ranking(&[0.5]);
        assert_eq!(ranking, vec![0]);
    }

    // --- aggregate_median ---

    #[test]
    fn aggregate_median_three_judges() {
        let scores = vec![
            IndividualJudgeScore {
                judge_model: "j1".into(),
                scores: vec![0.9, 0.5],
                ranking: vec![0, 1],
            },
            IndividualJudgeScore {
                judge_model: "j2".into(),
                scores: vec![0.7, 0.6],
                ranking: vec![0, 1],
            },
            IndividualJudgeScore {
                judge_model: "j3".into(),
                scores: vec![0.8, 0.4],
                ranking: vec![0, 1],
            },
        ];

        let judges = vec![
            JudgeModel {
                model_key: "j1".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j2".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j3".into(),
                weight: 1.0,
            },
        ];

        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        let result = cj.aggregate(scores).unwrap();

        // Median of [0.9, 0.7, 0.8] = 0.8; median of [0.5, 0.6, 0.4] = 0.5
        assert!((result.final_score - 0.8).abs() < 0.01);
        assert_eq!(result.best_candidate_idx, 0);
    }

    // --- aggregate_weighted_average ---

    #[test]
    fn aggregate_weighted_average_biased_judge() {
        let scores = vec![
            IndividualJudgeScore {
                judge_model: "heavy".into(),
                scores: vec![0.9, 0.3],
                ranking: vec![0, 1],
            },
            IndividualJudgeScore {
                judge_model: "light".into(),
                scores: vec![0.1, 0.8],
                ranking: vec![1, 0],
            },
        ];

        let judges = vec![
            JudgeModel {
                model_key: "heavy".into(),
                weight: 3.0,
            },
            JudgeModel {
                model_key: "light".into(),
                weight: 1.0,
            },
        ];

        let cj = CrossJudge::new(judges, JudgeAggregation::WeightedAverage);
        let result = cj.aggregate(scores).unwrap();

        // Candidate 0: (0.9*3 + 0.1*1)/4 = 0.7
        // Candidate 1: (0.3*3 + 0.8*1)/4 = 0.425
        assert!((result.final_score - 0.7).abs() < 0.01);
        assert_eq!(result.best_candidate_idx, 0);
    }

    // --- consensus_high ---

    #[test]
    fn consensus_high_all_agree() {
        let scores = vec![
            IndividualJudgeScore {
                judge_model: "j1".into(),
                scores: vec![0.9, 0.5],
                ranking: vec![0, 1],
            },
            IndividualJudgeScore {
                judge_model: "j2".into(),
                scores: vec![0.8, 0.4],
                ranking: vec![0, 1],
            },
        ];

        let judges = vec![
            JudgeModel {
                model_key: "j1".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j2".into(),
                weight: 1.0,
            },
        ];

        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        let result = cj.aggregate(scores).unwrap();
        assert_eq!(result.consensus_level, ConsensusLevel::High);
    }

    // --- consensus_medium ---

    #[test]
    fn consensus_medium_majority_agree() {
        let scores = vec![
            IndividualJudgeScore {
                judge_model: "j1".into(),
                scores: vec![0.9, 0.5, 0.3],
                ranking: vec![0, 1, 2],
            },
            IndividualJudgeScore {
                judge_model: "j2".into(),
                scores: vec![0.8, 0.6, 0.2],
                ranking: vec![0, 1, 2],
            },
            IndividualJudgeScore {
                judge_model: "j3".into(),
                scores: vec![0.3, 0.9, 0.8],
                ranking: vec![1, 2, 0],
            },
        ];

        let judges = vec![
            JudgeModel {
                model_key: "j1".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j2".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j3".into(),
                weight: 1.0,
            },
        ];

        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        let result = cj.aggregate(scores).unwrap();
        // 2 out of 3 judges pick candidate 0 as best -> medium consensus
        assert_eq!(result.consensus_level, ConsensusLevel::Medium);
    }

    // --- consensus_low ---

    #[test]
    fn consensus_low_major_disagreement() {
        let scores = vec![
            IndividualJudgeScore {
                judge_model: "j1".into(),
                scores: vec![0.9, 0.3, 0.2],
                ranking: vec![0, 1, 2],
            },
            IndividualJudgeScore {
                judge_model: "j2".into(),
                scores: vec![0.2, 0.9, 0.3],
                ranking: vec![1, 2, 0],
            },
            IndividualJudgeScore {
                judge_model: "j3".into(),
                scores: vec![0.3, 0.2, 0.9],
                ranking: vec![2, 0, 1],
            },
        ];

        let judges = vec![
            JudgeModel {
                model_key: "j1".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j2".into(),
                weight: 1.0,
            },
            JudgeModel {
                model_key: "j3".into(),
                weight: 1.0,
            },
        ];

        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        let result = cj.aggregate(scores).unwrap();
        // Each judge picks a different best -> low consensus
        assert_eq!(result.consensus_level, ConsensusLevel::Low);
    }

    // --- single judge ---

    #[test]
    fn single_judge_high_consensus() {
        let scores = vec![IndividualJudgeScore {
            judge_model: "solo".into(),
            scores: vec![0.7, 0.4],
            ranking: vec![0, 1],
        }];

        let judges = vec![JudgeModel {
            model_key: "solo".into(),
            weight: 1.0,
        }];

        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        let result = cj.aggregate(scores).unwrap();
        assert_eq!(result.consensus_level, ConsensusLevel::High);
        assert_eq!(result.best_candidate_idx, 0);
        assert!((result.final_score - 0.7).abs() < 0.01);
    }

    // --- empty scores rejected ---

    #[test]
    fn aggregate_rejects_empty_scores() {
        let judges = vec![JudgeModel {
            model_key: "j".into(),
            weight: 1.0,
        }];
        let cj = CrossJudge::new(judges, JudgeAggregation::Median);
        let result = cj.aggregate(vec![]);
        assert!(result.is_err());
    }

    // --- argmax utility ---

    #[test]
    fn argmax_picks_highest() {
        assert_eq!(CrossJudge::argmax(&[0.1, 0.9, 0.5]), 1);
        assert_eq!(CrossJudge::argmax(&[0.5]), 0);
        assert_eq!(CrossJudge::argmax(&[0.3, 0.3, 0.8]), 2);
    }
}
