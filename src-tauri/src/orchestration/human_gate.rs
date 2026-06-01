//! HumanGate -- determine when human review is required and create review
//! requests.
//!
//! This module provides the data model and trigger logic only.  The actual
//! Tauri IPC integration (waiting for user response) will be wired later.

use crate::orchestration::classifier::RiskLevel;
use crate::orchestration::cross_judge::ConsensusLevel;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration for the human-gate trigger.
pub struct HumanGate {
    /// Default timeout in seconds before the gate auto-resolves.
    pub default_timeout_secs: u32,
}

/// A request for human review.
#[derive(Debug, Clone)]
pub struct GateRequest {
    pub task_id: String,
    pub prompt_summary: String,
    pub candidates: Vec<String>,
    pub reason: String,
    pub created_at_ms: u64,
}

/// The human's decision on a gate request.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// Human approved a specific candidate.
    Approved { selected_idx: usize },
    /// Human requested revision with feedback.
    Revision { feedback: String },
    /// Human aborted the workflow.
    Aborted,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl HumanGate {
    /// Create a new HumanGate with the specified default timeout.
    pub fn new(timeout_secs: u32) -> Self {
        Self {
            default_timeout_secs: timeout_secs,
        }
    }

    /// Determine whether human review should be triggered.
    ///
    /// Triggers when the task risk is Critical AND judge consensus is Low.
    /// Also triggers on Critical risk regardless of consensus (safety first).
    pub fn should_trigger(&self, risk: &RiskLevel, consensus: Option<&ConsensusLevel>) -> bool {
        match risk {
            RiskLevel::Critical => {
                // Critical risk always triggers, but Low consensus makes it
                // absolutely mandatory.
                match consensus {
                    Some(ConsensusLevel::Low) | None => true,
                    _ => true,
                }
            }
            RiskLevel::High => {
                // High risk + low consensus also triggers.
                matches!(consensus, Some(ConsensusLevel::Low))
            }
            RiskLevel::Medium | RiskLevel::Low => false,
        }
    }

    /// Create a gate request with a timestamp.
    pub fn create_request(
        &self,
        task_id: &str,
        prompt_summary: &str,
        candidates: &[String],
        reason: &str,
    ) -> GateRequest {
        let created_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        log::info!(
            "[HumanGate] Created gate request for task '{}': {}",
            task_id,
            reason
        );

        GateRequest {
            task_id: task_id.to_string(),
            prompt_summary: prompt_summary.to_string(),
            candidates: candidates.to_vec(),
            reason: reason.to_string(),
            created_at_ms,
        }
    }
}

impl Default for HumanGate {
    fn default() -> Self {
        Self::new(300) // 5 minutes
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- should_trigger ---

    #[test]
    fn critical_risk_always_triggers() {
        let gate = HumanGate::default();
        assert!(gate.should_trigger(&RiskLevel::Critical, None));
        assert!(gate.should_trigger(&RiskLevel::Critical, Some(&ConsensusLevel::High)));
        assert!(gate.should_trigger(&RiskLevel::Critical, Some(&ConsensusLevel::Medium)));
        assert!(gate.should_trigger(&RiskLevel::Critical, Some(&ConsensusLevel::Low)));
    }

    #[test]
    fn high_risk_triggers_on_low_consensus() {
        let gate = HumanGate::default();
        assert!(gate.should_trigger(&RiskLevel::High, Some(&ConsensusLevel::Low)));
    }

    #[test]
    fn high_risk_does_not_trigger_on_high_consensus() {
        let gate = HumanGate::default();
        assert!(!gate.should_trigger(&RiskLevel::High, Some(&ConsensusLevel::High)));
    }

    #[test]
    fn high_risk_does_not_trigger_on_medium_consensus() {
        let gate = HumanGate::default();
        assert!(!gate.should_trigger(&RiskLevel::High, Some(&ConsensusLevel::Medium)));
    }

    #[test]
    fn medium_risk_never_triggers() {
        let gate = HumanGate::default();
        assert!(!gate.should_trigger(&RiskLevel::Medium, None));
        assert!(!gate.should_trigger(&RiskLevel::Medium, Some(&ConsensusLevel::Low)));
    }

    #[test]
    fn low_risk_never_triggers() {
        let gate = HumanGate::default();
        assert!(!gate.should_trigger(&RiskLevel::Low, None));
        assert!(!gate.should_trigger(&RiskLevel::Low, Some(&ConsensusLevel::Low)));
    }

    // --- create_request ---

    #[test]
    fn create_request_populates_fields() {
        let gate = HumanGate::new(600);
        let candidates = vec!["answer A".to_string(), "answer B".to_string()];
        let req = gate.create_request(
            "task-1",
            "Summarize X",
            &candidates,
            "Critical risk detected",
        );

        assert_eq!(req.task_id, "task-1");
        assert_eq!(req.prompt_summary, "Summarize X");
        assert_eq!(req.candidates.len(), 2);
        assert_eq!(req.reason, "Critical risk detected");
        assert!(req.created_at_ms > 0);
    }

    #[test]
    fn create_request_empty_candidates() {
        let gate = HumanGate::default();
        let req = gate.create_request("task-2", "Hello", &[], "test");
        assert!(req.candidates.is_empty());
    }

    // --- default timeout ---

    #[test]
    fn default_timeout_is_300_seconds() {
        let gate = HumanGate::default();
        assert_eq!(gate.default_timeout_secs, 300);
    }

    #[test]
    fn custom_timeout() {
        let gate = HumanGate::new(120);
        assert_eq!(gate.default_timeout_secs, 120);
    }
}
