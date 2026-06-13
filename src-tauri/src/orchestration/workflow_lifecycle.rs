//! WorkflowLifecycle — 6-state lifecycle machine for orchestration workflows.
//!
//! States: Idle → Classifying → Executing → Verifying → Completed/Failed
//! Transitions are validated and logged.

use serde::{Deserialize, Serialize};

/// The 6 lifecycle states of an orchestration workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    /// Initial state, no work started.
    Idle,
    /// Task classification in progress.
    Classifying,
    /// Strategy execution in progress (model calls happening).
    Executing,
    /// Quality verification in progress.
    Verifying,
    /// Workflow completed successfully.
    Completed,
    /// Workflow failed (all models failed or unrecoverable error).
    Failed,
}

impl WorkflowState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Classifying => "classifying",
            Self::Executing => "executing",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// Check if a transition from `self` to `next` is valid.
    pub fn can_transition_to(&self, next: WorkflowState) -> bool {
        matches!(
            (self, next),
            (Self::Idle, Self::Classifying)
                | (Self::Classifying, Self::Executing)
                | (Self::Executing, Self::Verifying)
                | (Self::Verifying, Self::Completed)
                | (Self::Verifying, Self::Failed)
                | (Self::Executing, Self::Failed) // immediate failure
                | (Self::Failed, Self::Idle) // allow retry
        )
    }
}

/// A state transition event, logged for observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: WorkflowState,
    pub to: WorkflowState,
    pub reason: String,
    pub timestamp_ms: u64,
}

/// Tracks the lifecycle of a single orchestration workflow.
#[derive(Debug, Clone)]
pub struct WorkflowLifecycle {
    state: WorkflowState,
    transitions: Vec<StateTransition>,
    start_ms: u64,
}

impl WorkflowLifecycle {
    pub fn new(now_ms: u64) -> Self {
        Self {
            state: WorkflowState::Idle,
            transitions: Vec::new(),
            start_ms: now_ms,
        }
    }

    /// Current state.
    pub fn state(&self) -> WorkflowState {
        self.state
    }

    /// Attempt a state transition. Returns `Ok` if valid, `Err` with reason otherwise.
    pub fn transition(
        &mut self,
        to: WorkflowState,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        if !self.state.can_transition_to(to) {
            return Err(format!(
                "Invalid transition: {} → {} (reason: {})",
                self.state.name(),
                to.name(),
                reason
            ));
        }
        let t = StateTransition {
            from: self.state,
            to,
            reason: reason.to_string(),
            timestamp_ms: now_ms,
        };
        log::info!(
            "[Lifecycle] {} → {} ({})",
            self.state.name(),
            to.name(),
            reason
        );
        self.state = to;
        self.transitions.push(t);
        Ok(())
    }

    /// Shorthand: mark classifying.
    pub fn start_classifying(&mut self, now_ms: u64) -> Result<(), String> {
        self.transition(WorkflowState::Classifying, "task received", now_ms)
    }

    /// Shorthand: mark executing.
    pub fn start_executing(&mut self, now_ms: u64) -> Result<(), String> {
        self.transition(WorkflowState::Executing, "strategy selected", now_ms)
    }

    /// Shorthand: mark verifying.
    pub fn start_verifying(&mut self, now_ms: u64) -> Result<(), String> {
        self.transition(WorkflowState::Verifying, "quality check", now_ms)
    }

    /// Shorthand: mark completed.
    pub fn complete(&mut self, now_ms: u64) -> Result<(), String> {
        self.transition(WorkflowState::Completed, "all checks passed", now_ms)
    }

    /// Shorthand: mark failed.
    pub fn fail(&mut self, reason: &str, now_ms: u64) -> Result<(), String> {
        self.transition(WorkflowState::Failed, reason, now_ms)
    }

    /// Total elapsed time since creation.
    pub fn elapsed_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.start_ms)
    }

    /// All transitions for audit purposes.
    pub fn transitions(&self) -> &[StateTransition] {
        &self.transitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_lifecycle_sequence() {
        let mut wf = WorkflowLifecycle::new(0);
        assert!(wf.start_classifying(10).is_ok());
        assert!(wf.start_executing(20).is_ok());
        assert!(wf.start_verifying(30).is_ok());
        assert!(wf.complete(40).is_ok());
        assert_eq!(wf.state(), WorkflowState::Completed);
        assert_eq!(wf.transitions().len(), 4);
    }

    #[test]
    fn rejects_invalid_transition() {
        let mut wf = WorkflowLifecycle::new(0);
        // Cannot go directly from Idle to Completed
        assert!(wf.complete(10).is_err());
    }

    #[test]
    fn allow_retry_from_failed() {
        let mut wf = WorkflowLifecycle::new(0);
        wf.start_classifying(10).unwrap();
        wf.start_executing(20).unwrap();
        wf.fail("model error", 30).unwrap();
        assert_eq!(wf.state(), WorkflowState::Failed);
        // Can retry from Failed back to Idle
        assert!(wf.transition(WorkflowState::Idle, "retry", 40).is_ok());
    }
}
