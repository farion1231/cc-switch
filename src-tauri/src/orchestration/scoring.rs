//! CostQualityScorer -- score models by balancing expected quality against
//! cost, latency, risk, and verifiability.
//!
//! Formula (from MiroFish MOA design):
//!   Score = ExpectedQuality * 0.5 - Cost * 0.3 - Latency * 0.1
//!           - Risk * 0.2 + Verifiability * 0.15

use crate::orchestration::classifier::{RiskLevel, TaskProfile};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Tunable weights for the cost-quality scoring formula.
#[derive(Debug, Clone)]
pub struct CostQualityScorer {
    pub quality_weight: f64,      // 0.5
    pub cost_sensitivity: f64,    // 0.3
    pub latency_sensitivity: f64, // 0.1
    pub risk_penalty: f64,        // 0.2
    pub verifiability_bonus: f64, // 0.15
}

impl Default for CostQualityScorer {
    fn default() -> Self {
        Self {
            quality_weight: 0.5,
            cost_sensitivity: 0.3,
            latency_sensitivity: 0.1,
            risk_penalty: 0.2,
            verifiability_bonus: 0.15,
        }
    }
}

/// Per-model scoring result.
#[derive(Debug, Clone)]
pub struct ModelScore {
    pub model_key: String,
    pub score: f64,
    pub breakdown: ScoreBreakdown,
}

/// Breakdown of each term in the scoring formula.
#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    pub expected_quality: f64,
    pub cost_term: f64,
    pub latency_term: f64,
    pub risk_term: f64,
    pub verifiability_term: f64,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl CostQualityScorer {
    /// Score a single model against a task profile and observed averages.
    ///
    /// - `avg_quality`    -- historical average quality (0.0 - 1.0)
    /// - `avg_cost_usd`   -- historical average cost in USD
    /// - `avg_latency_ms` -- historical average latency in milliseconds
    /// - `budget_remaining` -- remaining budget in USD (used to normalize cost)
    pub fn score(
        &self,
        model_key: &str,
        profile: &TaskProfile,
        avg_quality: f64,
        avg_cost_usd: f64,
        avg_latency_ms: u64,
        budget_remaining: f64,
    ) -> ModelScore {
        // Normalize cost: fraction of remaining budget consumed.
        let cost_normalized = if budget_remaining > 0.0 {
            (avg_cost_usd / budget_remaining).min(1.0)
        } else {
            1.0 // budget exhausted => maximum cost penalty
        };

        // Normalize latency against a 30-second reference.
        let latency_normalized = (avg_latency_ms as f64) / 30000.0;

        // Risk mapped from enum to numeric penalty.
        let risk_value = Self::risk_to_value(&profile.risk);

        // Verifiability comes directly from the task profile.
        let verifiability = profile.verifiability;

        let quality_term = avg_quality * self.quality_weight;
        let cost_term = cost_normalized * self.cost_sensitivity;
        let latency_term = latency_normalized * self.latency_sensitivity;
        let risk_term = risk_value * self.risk_penalty;
        let verifiability_term = verifiability * self.verifiability_bonus;

        let total = quality_term - cost_term - latency_term - risk_term + verifiability_term;

        ModelScore {
            model_key: model_key.to_string(),
            score: total,
            breakdown: ScoreBreakdown {
                expected_quality: quality_term,
                cost_term,
                latency_term,
                risk_term,
                verifiability_term,
            },
        }
    }

    /// Map a RiskLevel to a numeric value for scoring.
    pub fn risk_to_value(risk: &RiskLevel) -> f64 {
        match risk {
            RiskLevel::Critical => 1.0,
            RiskLevel::High => 0.6,
            RiskLevel::Medium => 0.3,
            RiskLevel::Low => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::classifier::{RiskLevel, TaskType};

    fn make_profile(risk: RiskLevel, verifiability: f64) -> TaskProfile {
        TaskProfile {
            task_type: TaskType::Coding,
            complexity: 0.5,
            risk,
            verifiability,
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

    // --- Score calculation matches formula ---

    #[test]
    fn score_calculation_matches_formula() {
        let scorer = CostQualityScorer::default();
        let profile = make_profile(RiskLevel::Medium, 0.8);
        let avg_quality = 0.9;
        let avg_cost = 0.01;
        let avg_latency = 5000u64;
        let budget = 0.1;

        let result = scorer.score(
            "test-model",
            &profile,
            avg_quality,
            avg_cost,
            avg_latency,
            budget,
        );

        // Cost normalized: 0.01 / 0.1 = 0.1
        let cost_norm = 0.1_f64;
        // Latency normalized: 5000 / 30000 ≈ 0.1667
        let latency_norm = 5000.0_f64 / 30000.0;
        // Risk: Medium = 0.3
        let risk_val = 0.3_f64;

        let expected_quality_term = avg_quality * 0.5;
        let expected_cost_term = cost_norm * 0.3;
        let expected_latency_term = latency_norm * 0.1;
        let expected_risk_term = risk_val * 0.2;
        let expected_verifiability_term = 0.8 * 0.15;

        let expected_total =
            expected_quality_term - expected_cost_term - expected_latency_term - expected_risk_term
                + expected_verifiability_term;

        assert!(
            (result.score - expected_total).abs() < 1e-10,
            "Score mismatch: got {}, expected {}",
            result.score,
            expected_total
        );

        assert!((result.breakdown.expected_quality - expected_quality_term).abs() < 1e-10);
        assert!((result.breakdown.cost_term - expected_cost_term).abs() < 1e-10);
        assert!((result.breakdown.latency_term - expected_latency_term).abs() < 1e-10);
        assert!((result.breakdown.risk_term - expected_risk_term).abs() < 1e-10);
        assert!((result.breakdown.verifiability_term - expected_verifiability_term).abs() < 1e-10);
    }

    // --- Risk levels map correctly ---

    #[test]
    fn risk_level_critical() {
        assert!((CostQualityScorer::risk_to_value(&RiskLevel::Critical) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn risk_level_high() {
        assert!((CostQualityScorer::risk_to_value(&RiskLevel::High) - 0.6).abs() < 1e-10);
    }

    #[test]
    fn risk_level_medium() {
        assert!((CostQualityScorer::risk_to_value(&RiskLevel::Medium) - 0.3).abs() < 1e-10);
    }

    #[test]
    fn risk_level_low() {
        assert!((CostQualityScorer::risk_to_value(&RiskLevel::Low) - 0.0).abs() < 1e-10);
    }

    // --- Budget edge cases ---

    #[test]
    fn zero_budget_does_not_panic() {
        let scorer = CostQualityScorer::default();
        let profile = make_profile(RiskLevel::Low, 0.5);
        // Should not panic when budget is zero.
        let result = scorer.score("m", &profile, 0.8, 0.01, 1000, 0.0);
        // Cost normalized should be 1.0 (max penalty) when budget is zero.
        assert!((result.breakdown.cost_term - 1.0 * scorer.cost_sensitivity).abs() < 1e-10);
    }

    #[test]
    fn very_small_budget_clamps_cost_to_one() {
        let scorer = CostQualityScorer::default();
        let profile = make_profile(RiskLevel::Low, 0.5);
        // Cost exceeds budget => normalized to 1.0.
        let result = scorer.score("m", &profile, 0.8, 10.0, 1000, 0.01);
        assert!((result.breakdown.cost_term - 1.0 * scorer.cost_sensitivity).abs() < 1e-10);
    }

    #[test]
    fn large_budget_reduces_cost_penalty() {
        let scorer = CostQualityScorer::default();
        let profile = make_profile(RiskLevel::Low, 0.5);
        let result = scorer.score("m", &profile, 0.8, 0.01, 1000, 100.0);
        // Cost normalized: 0.01 / 100.0 = 0.0001 (very small).
        let expected_cost = 0.0001 * scorer.cost_sensitivity;
        assert!((result.breakdown.cost_term - expected_cost).abs() < 1e-10);
    }

    // --- Verifiability bonus ---

    #[test]
    fn high_verifiability_boosts_score() {
        let scorer = CostQualityScorer::default();
        let profile_low = make_profile(RiskLevel::Low, 0.1);
        let profile_high = make_profile(RiskLevel::Low, 0.9);

        let score_low = scorer.score("m", &profile_low, 0.8, 0.01, 1000, 1.0);
        let score_high = scorer.score("m", &profile_high, 0.8, 0.01, 1000, 1.0);

        assert!(
            score_high.score > score_low.score,
            "Higher verifiability should yield higher score"
        );
    }

    // --- Default weights ---

    #[test]
    fn default_weights_match_spec() {
        let s = CostQualityScorer::default();
        assert!((s.quality_weight - 0.5).abs() < 1e-10);
        assert!((s.cost_sensitivity - 0.3).abs() < 1e-10);
        assert!((s.latency_sensitivity - 0.1).abs() < 1e-10);
        assert!((s.risk_penalty - 0.2).abs() < 1e-10);
        assert!((s.verifiability_bonus - 0.15).abs() < 1e-10);
    }
}
