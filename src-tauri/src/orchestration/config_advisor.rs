//! ConfigAdvisor — LLM-powered orchestration configuration tuning.
//!
//! Analyzes orchestration request history and suggests strategy improvements:
//! threshold adjustments, model substitutions, and strategy ordering changes.

use crate::orchestration::model_caller::ModelCaller;
use serde::{Deserialize, Serialize};

/// A tuning suggestion from the LLM advisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningSuggestion {
    pub field: String,
    pub current_value: String,
    pub suggested_value: String,
    pub reason: String,
    pub confidence: f64,
}

/// Summary of recent orchestration performance data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    pub total_requests: u64,
    pub route_success_rate: f64,
    pub cascade_escalation_rate: f64,
    pub debate_consensus_rate: f64,
    pub moa_verification_rate: f64,
    pub avg_latency_ms: u64,
    pub avg_cost_usd: f64,
    pub most_used_model: String,
    pub failure_distribution: Vec<(String, u64)>,
}

/// LLM-powered config tuner that analyzes performance data and suggests improvements.
pub struct ConfigAdvisor {
    advisor_model: String,
}

impl ConfigAdvisor {
    pub fn new(advisor_model: &str) -> Self {
        Self {
            advisor_model: advisor_model.to_string(),
        }
    }

    /// Build the prompt that asks the LLM to analyze performance and suggest changes.
    pub fn build_analysis_prompt(summary: &PerformanceSummary) -> String {
        format!(
            r#"Analyze this orchestration engine performance data and suggest concrete configuration improvements.

## Current Performance
- Total requests: {}
- Route success rate: {:.3}
- Cascade escalation rate: {:.3}
- Debate consensus rate: {:.3}
- MoA verification rate: {:.3}
- Avg latency: {}ms
- Avg cost: ${:.4}
- Most used model: {}
- Failures by model: {:?}

## Suggestions Format
For each suggestion, output ONE line:
FIELD: <field_path> | CURRENT: <current> | SUGGESTED: <new> | REASON: <why> | CONFIDENCE: <0.0-1.0>

Fields to consider:
- threshold adjustments (route complexity range, cascade quality_threshold)
- model substitutions (replace underperforming models)
- strategy ordering (promote/demote strategies)

Output ONLY the suggestion lines, one per line. No preamble."#,
            summary.total_requests,
            summary.route_success_rate,
            summary.cascade_escalation_rate,
            summary.debate_consensus_rate,
            summary.moa_verification_rate,
            summary.avg_latency_ms,
            summary.avg_cost_usd,
            summary.most_used_model,
            summary.failure_distribution,
        )
    }

    /// Parse tuning suggestions from the LLM response.
    pub fn parse_suggestions(response: &str) -> Vec<TuningSuggestion> {
        response
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if !line.starts_with("FIELD:") {
                    return None;
                }
                let mut field = "";
                let mut current = "";
                let mut suggested = "";
                let mut reason = "";
                let mut confidence = 0.5;

                for part in line.split('|') {
                    let part = part.trim();
                    if let Some(v) = part.strip_prefix("FIELD: ") {
                        field = v.trim();
                    } else if let Some(v) = part.strip_prefix("CURRENT: ") {
                        current = v.trim();
                    } else if let Some(v) = part.strip_prefix("SUGGESTED: ") {
                        suggested = v.trim();
                    } else if let Some(v) = part.strip_prefix("REASON: ") {
                        reason = v.trim();
                    } else if let Some(v) = part.strip_prefix("CONFIDENCE: ") {
                        confidence = v.trim().parse().unwrap_or(0.5);
                    }
                }

                if field.is_empty() || suggested.is_empty() {
                    return None;
                }

                Some(TuningSuggestion {
                    field: field.to_string(),
                    current_value: current.to_string(),
                    suggested_value: suggested.to_string(),
                    reason: reason.to_string(),
                    confidence,
                })
            })
            .collect()
    }

    /// Run the advisor: call the LLM with performance data and parse suggestions.
    pub async fn analyze(
        &self,
        caller: &ModelCaller,
        summary: &PerformanceSummary,
    ) -> Result<Vec<TuningSuggestion>, String> {
        let prompt = Self::build_analysis_prompt(summary);
        let resp = caller
            .call_prompt(
                &self.advisor_model,
                "You are an expert AI orchestration config tuner.",
                &prompt,
                Some(0.3),
            )
            .await?;
        Ok(Self::parse_suggestions(&resp.content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_suggestion() {
        let resp = "FIELD: strategies.route.action.quality_threshold | CURRENT: 0.65 | SUGGESTED: 0.75 | REASON: Too many low-quality passes | CONFIDENCE: 0.82";
        let suggestions = ConfigAdvisor::parse_suggestions(resp);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].field, "strategies.route.action.quality_threshold");
        assert_eq!(suggestions[0].suggested_value, "0.75");
        assert!((suggestions[0].confidence - 0.82).abs() < 0.01);
    }

    #[test]
    fn ignores_non_suggestion_lines() {
        let resp = "Here are my suggestions:\nFIELD: x | CURRENT: a | SUGGESTED: b | REASON: test | CONFIDENCE: 0.9\nSome commentary.";
        let suggestions = ConfigAdvisor::parse_suggestions(resp);
        assert_eq!(suggestions.len(), 1);
    }

    #[test]
    fn handles_malformed_lines() {
        let resp = "FIELD: incomplete\nFIELD: a | CURRENT: b | SUGGESTED: c | REASON: d | CONFIDENCE: 0.5";
        let suggestions = ConfigAdvisor::parse_suggestions(resp);
        assert_eq!(suggestions.len(), 1); // incomplete line skipped
    }
}
