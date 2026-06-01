use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Manages strategy escalation when quality verification fails.
///
/// Tracks how many escalation attempts have been made and decides whether to
/// escalate to a stronger model or give up after exhausting the configured
/// maximum number of escalations.
pub struct EscalationController {
    max_escalations: u32,
    notify_ui: bool,
}

/// Decision returned by `should_escalate`.
#[derive(Debug, Clone, PartialEq)]
pub enum EscalationAction {
    Escalate {
        from_model: String,
        to_model: String,
        reason: String,
        attempt: u32,
    },
    GiveUp {
        reason: String,
        total_attempts: u32,
    },
}

/// Record of a single escalation event for logging / audit purposes.
#[derive(Debug, Clone)]
pub struct EscalationRecord {
    pub request_id: String,
    pub from_model: String,
    pub to_model: String,
    pub reason: String,
    pub timestamp_ms: u64,
    pub attempt_number: u32,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl EscalationController {
    pub fn new(max_escalations: u32, notify_ui: bool) -> Self {
        Self {
            max_escalations,
            notify_ui,
        }
    }

    /// Evaluate whether an escalation should happen.
    ///
    /// - `current_attempt` is the 1-based attempt number that just failed.
    /// - `quality_score` is the score from quality verification.
    /// - `threshold` is the minimum passing score.
    ///
    /// Returns `Escalate` when quality is below threshold and the attempt
    /// count has not exceeded `max_escalations`.  Returns `GiveUp` when the
    /// quality is below threshold but all escalation attempts are exhausted.
    ///
    /// Note: this method is pure -- it does not track state internally.  The
    /// caller is responsible for incrementing the attempt counter.
    pub fn should_escalate(
        &self,
        current_attempt: u32,
        quality_score: f64,
        threshold: f64,
    ) -> EscalationAction {
        if quality_score >= threshold {
            // Quality is acceptable; no action needed.  This is expressed as
            // a no-op but callers should check the result before acting.
            // We still return GiveUp with a success reason to signal "done".
            return EscalationAction::GiveUp {
                reason: "quality_threshold_met".to_string(),
                total_attempts: current_attempt,
            };
        }

        if current_attempt > self.max_escalations {
            return EscalationAction::GiveUp {
                reason: "max_escalations_exceeded".to_string(),
                total_attempts: current_attempt,
            };
        }

        let next_attempt = current_attempt + 1;
        let from_model = format!("model_attempt_{}", current_attempt);
        let to_model = format!("model_attempt_{}", next_attempt);

        EscalationAction::Escalate {
            from_model,
            to_model,
            reason: format!(
                "quality_score {:.3} below threshold {:.3}",
                quality_score, threshold
            ),
            attempt: next_attempt,
        }
    }

    /// Log an escalation event.  In a full Tauri integration this would emit
    /// an IPC event to the frontend; for now we log via `log::info`.
    pub fn notify_escalation(&self, record: &EscalationRecord) {
        log::info!(
            "[Escalation] request={} attempt={} {} -> {} reason={}",
            record.request_id,
            record.attempt_number,
            record.from_model,
            record.to_model,
            record.reason,
        );

        if self.notify_ui {
            // Placeholder: in production this would call
            // `app_handle.emit("orchestration:escalation", &record)`.
            log::info!(
                "[Escalation] UI notification would be sent for request={}",
                record.request_id,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl EscalationRecord {
    pub fn new(
        request_id: String,
        from_model: String,
        to_model: String,
        reason: String,
        attempt_number: u32,
    ) -> Self {
        Self {
            request_id,
            from_model,
            to_model,
            reason,
            timestamp_ms: current_timestamp_ms(),
            attempt_number,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_controller() -> EscalationController {
        EscalationController::new(2, true)
    }

    // ---- should_escalate: escalate when quality below threshold ----

    #[test]
    fn escalate_when_quality_below_threshold() {
        let ctrl = make_controller();
        let action = ctrl.should_escalate(1, 0.3, 0.65);
        match action {
            EscalationAction::Escalate { attempt, .. } => {
                assert_eq!(attempt, 2, "Should escalate to attempt 2");
            }
            EscalationAction::GiveUp { .. } => {
                panic!("Expected Escalate, got GiveUp");
            }
        }
    }

    // ---- should_escalate: give up after max escalations ----

    #[test]
    fn give_up_after_max_escalations() {
        let ctrl = make_controller();
        // max_escalations = 2, so attempt 3 should give up
        let action = ctrl.should_escalate(3, 0.3, 0.65);
        match action {
            EscalationAction::GiveUp {
                reason,
                total_attempts,
            } => {
                assert_eq!(total_attempts, 3);
                assert!(
                    reason.contains("max_escalations_exceeded"),
                    "Expected max_escalations_exceeded, got: {}",
                    reason,
                );
            }
            EscalationAction::Escalate { .. } => {
                panic!("Expected GiveUp, got Escalate");
            }
        }
    }

    // ---- should_escalate: no escalation when quality above threshold ----

    #[test]
    fn no_escalation_when_quality_above_threshold() {
        let ctrl = make_controller();
        let action = ctrl.should_escalate(1, 0.8, 0.65);
        match action {
            EscalationAction::GiveUp { reason, .. } => {
                assert!(
                    reason.contains("quality_threshold_met"),
                    "Expected quality_threshold_met, got: {}",
                    reason,
                );
            }
            EscalationAction::Escalate { .. } => {
                panic!("Expected GiveUp (threshold met), got Escalate");
            }
        }
    }

    // ---- EscalationRecord creation with correct fields ----

    #[test]
    fn escalation_record_creation() {
        let record = EscalationRecord::new(
            "req-123".to_string(),
            "haiku".to_string(),
            "sonnet".to_string(),
            "low quality".to_string(),
            1,
        );
        assert_eq!(record.request_id, "req-123");
        assert_eq!(record.from_model, "haiku");
        assert_eq!(record.to_model, "sonnet");
        assert_eq!(record.reason, "low quality");
        assert_eq!(record.attempt_number, 1);
        assert!(record.timestamp_ms > 0, "Timestamp should be non-zero");
    }

    // ---- Edge: escalate at exactly the max boundary ----

    #[test]
    fn escalate_at_max_boundary() {
        let ctrl = make_controller();
        // max_escalations = 2, attempt 2 is still allowed
        let action = ctrl.should_escalate(2, 0.3, 0.65);
        match action {
            EscalationAction::Escalate { attempt, .. } => {
                assert_eq!(attempt, 3);
            }
            EscalationAction::GiveUp { .. } => {
                panic!("Attempt 2 should still escalate (max=2)");
            }
        }
    }

    // ---- Edge: quality exactly at threshold passes ----

    #[test]
    fn quality_exactly_at_threshold_passes() {
        let ctrl = make_controller();
        let action = ctrl.should_escalate(1, 0.65, 0.65);
        match action {
            EscalationAction::GiveUp { reason, .. } => {
                assert!(reason.contains("quality_threshold_met"));
            }
            EscalationAction::Escalate { .. } => {
                panic!("Quality at threshold should not escalate");
            }
        }
    }

    // ---- notify_escalation does not panic ----

    #[test]
    fn notify_escalation_logs_without_panic() {
        let ctrl = make_controller();
        let record = EscalationRecord::new(
            "req-test".to_string(),
            "a".to_string(),
            "b".to_string(),
            "test reason".to_string(),
            1,
        );
        // Should not panic
        ctrl.notify_escalation(&record);
    }

    // ---- notify_ui = false still logs ----

    #[test]
    fn notify_escalation_without_ui() {
        let ctrl = EscalationController::new(2, false);
        let record = EscalationRecord::new(
            "req-no-ui".to_string(),
            "a".to_string(),
            "b".to_string(),
            "no ui".to_string(),
            1,
        );
        ctrl.notify_escalation(&record);
    }
}
