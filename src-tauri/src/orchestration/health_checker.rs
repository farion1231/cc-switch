//! ModelHealthChecker -- reactive model health tracking.
//!
//! Tracks per-model availability, error rate, and latency after each call.
//! Does NOT run background health pings; it is updated reactively by the
//! orchestration layer after every model invocation.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Health snapshot for a single model.
#[derive(Debug, Clone)]
pub struct ModelHealth {
    pub model_key: String,
    pub is_available: bool,
    pub avg_latency_ms: u64,
    pub error_rate: f64,
    pub last_check_ms: u64,
    pub consecutive_errors: u32,
}

/// Reactive health tracker for all known models.
pub struct ModelHealthChecker {
    health: HashMap<String, ModelHealth>,
    check_interval_secs: u32,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl ModelHealthChecker {
    /// Create a new checker. All models start as available with zero errors.
    pub fn new(model_keys: &[String]) -> Self {
        let now_ms = Self::current_time_ms();

        let health = model_keys
            .iter()
            .map(|key| {
                (
                    key.clone(),
                    ModelHealth {
                        model_key: key.clone(),
                        is_available: true,
                        avg_latency_ms: 0,
                        error_rate: 0.0,
                        last_check_ms: now_ms,
                        consecutive_errors: 0,
                    },
                )
            })
            .collect();

        Self {
            health,
            check_interval_secs: 60,
        }
    }

    /// Update a model's health after a call completes.
    ///
    /// Uses an exponential moving average for error rate and latency.
    /// After enough consecutive successes the model is re-enabled.
    pub fn update_health(&mut self, model_key: &str, success: bool, latency_ms: u64) {
        let Some(entry) = self.health.get_mut(model_key) else {
            return;
        };

        let alpha = 0.3; // EMA smoothing factor after the first observation.

        // Update error rate (EMA).
        let error_val = if success { 0.0 } else { 1.0 };
        entry.error_rate = alpha * error_val + (1.0 - alpha) * entry.error_rate;

        // Update latency (EMA).
        entry.avg_latency_ms = if entry.avg_latency_ms == 0 {
            latency_ms.max(1)
        } else {
            ((alpha * latency_ms as f64 + (1.0 - alpha) * entry.avg_latency_ms as f64) as u64)
                .max(1)
        };

        // Update consecutive errors.
        if success {
            entry.consecutive_errors = 0;
        } else {
            entry.consecutive_errors += 1;
        }

        // Auto-disable after sustained errors.
        if entry.consecutive_errors >= 3 {
            entry.is_available = false;
        }

        // Auto-re-enable after recovery.
        if success && !entry.is_available && entry.error_rate < 0.1 {
            entry.is_available = true;
        }

        entry.last_check_ms = Self::current_time_ms();
    }

    /// Check whether a model is available.
    ///
    pub fn is_available(&self, model_key: &str) -> bool {
        self.health
            .get(model_key)
            .map(|h| h.is_available)
            .unwrap_or(false)
    }

    /// Return a list of all currently available model keys.
    pub fn available_models(&self) -> Vec<String> {
        self.health
            .values()
            .filter(|h| h.is_available)
            .map(|h| h.model_key.clone())
            .collect()
    }

    /// Force-mark a model as unavailable (e.g. after a critical error).
    pub fn mark_unavailable(&mut self, model_key: &str) {
        if let Some(entry) = self.health.get_mut(model_key) {
            entry.is_available = false;
            entry.last_check_ms = Self::current_time_ms();
            log::warn!(
                "[HealthChecker] Model '{}' force-marked as unavailable",
                model_key
            );
        }
    }

    /// Get a reference to the health record for a model (read-only).
    pub fn get_health(&self, model_key: &str) -> Option<&ModelHealth> {
        self.health.get(model_key)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn current_time_ms() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keys() -> Vec<String> {
        vec![
            "model-a".to_string(),
            "model-b".to_string(),
            "model-c".to_string(),
        ]
    }

    // --- Initial state: all available ---

    #[test]
    fn all_models_initially_available() {
        let checker = ModelHealthChecker::new(&make_keys());
        assert!(checker.is_available("model-a"));
        assert!(checker.is_available("model-b"));
        assert!(checker.is_available("model-c"));
        assert_eq!(checker.available_models().len(), 3);
    }

    // --- Unknown model is not available ---

    #[test]
    fn unknown_model_not_available() {
        let checker = ModelHealthChecker::new(&make_keys());
        assert!(!checker.is_available("nonexistent"));
    }

    // --- Successful call keeps model available ---

    #[test]
    fn success_keeps_available() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", true, 500);
        assert!(checker.is_available("model-a"));

        let health = checker.get_health("model-a").unwrap();
        assert_eq!(health.avg_latency_ms, 500);
        assert_eq!(health.consecutive_errors, 0);
    }

    // --- Single error does not disable ---

    #[test]
    fn single_error_does_not_disable() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", false, 1000);
        assert!(checker.is_available("model-a"));
        assert_eq!(checker.get_health("model-a").unwrap().consecutive_errors, 1);
    }

    #[test]
    fn first_latency_uses_observed_value_without_ema_dampening() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", true, 500);

        let health = checker.get_health("model-a").unwrap();
        assert_eq!(health.avg_latency_ms, 500);
        assert!(checker.is_available("model-a"));
    }

    #[test]
    fn single_error_records_error_without_disabling_model() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", false, 1000);

        let health = checker.get_health("model-a").unwrap();
        assert_eq!(health.consecutive_errors, 1);
        assert!(health.error_rate > 0.0);
        assert!(checker.is_available("model-a"));
    }

    // --- Three consecutive errors disables model ---

    #[test]
    fn three_errors_disables_model() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", false, 1000);
        checker.update_health("model-a", false, 1000);
        checker.update_health("model-a", false, 1000);
        assert!(!checker.is_available("model-a"));
    }

    // --- Available list filters correctly ---

    #[test]
    fn available_list_filters_unavailable() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        // Disable model-b with 3 consecutive errors.
        checker.update_health("model-b", false, 1000);
        checker.update_health("model-b", false, 1000);
        checker.update_health("model-b", false, 1000);

        let available = checker.available_models();
        assert_eq!(available.len(), 2);
        assert!(!available.contains(&"model-b".to_string()));
    }

    // --- Force mark unavailable ---

    #[test]
    fn force_mark_unavailable() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        assert!(checker.is_available("model-a"));
        checker.mark_unavailable("model-a");
        assert!(!checker.is_available("model-a"));
    }

    #[test]
    fn force_mark_nonexistent_is_noop() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.mark_unavailable("nonexistent");
        // Should not panic.
    }

    // --- Recovery after errors ---

    #[test]
    fn recovery_re_enables_model() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        // Disable via consecutive errors.
        checker.update_health("model-a", false, 1000);
        checker.update_health("model-a", false, 1000);
        checker.update_health("model-a", false, 1000);
        assert!(!checker.is_available("model-a"));

        // Successful calls should eventually re-enable (after error rate drops).
        // Need enough successes for EMA to bring error_rate below 0.1.
        for _ in 0..10 {
            checker.update_health("model-a", true, 500);
        }
        assert!(
            checker.is_available("model-a"),
            "Model should recover after sustained successes"
        );
    }

    // --- Latency EMA updates ---

    #[test]
    fn latency_updates_ema() {
        let mut checker = ModelHealthChecker::new(&make_keys());
        checker.update_health("model-a", true, 1000);
        let h1 = checker.get_health("model-a").unwrap().avg_latency_ms;

        checker.update_health("model-a", true, 200);
        let h2 = checker.get_health("model-a").unwrap().avg_latency_ms;

        // Second latency is lower, so EMA should decrease.
        assert!(h2 < h1, "EMA latency should decrease: {} vs {}", h2, h1);
    }

    // --- get_health returns None for unknown ---

    #[test]
    fn get_health_unknown_returns_none() {
        let checker = ModelHealthChecker::new(&make_keys());
        assert!(checker.get_health("nonexistent").is_none());
    }
}
