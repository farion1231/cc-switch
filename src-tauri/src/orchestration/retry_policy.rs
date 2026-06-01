//! Temperature-decay retry policy for LLM calls.
//!
//! Ported from MiroFish `simulation_config_generator.py` line 451:
//! ```python
//! temperature=0.7 - (attempt * 0.1)  # 每次重试降低温度
//! ```
//!
//! Each successive retry lowers the sampling temperature, steering the model
//! toward more deterministic output after an initial creative attempt fails.

use serde::{Deserialize, Serialize};

/// Configuration for temperature-decay retries against an LLM.
///
/// The temperature starts at `base_temperature` and decreases by
/// `decay_per_attempt` on every retry, floored at 0.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureDecayRetry {
    /// Initial sampling temperature (default 0.7).
    pub base_temperature: f64,
    /// Amount to subtract from temperature per attempt (default 0.1).
    pub decay_per_attempt: f64,
    /// Maximum number of attempts before giving up (default 3).
    pub max_attempts: u32,
}

impl Default for TemperatureDecayRetry {
    fn default() -> Self {
        Self {
            base_temperature: 0.7,
            decay_per_attempt: 0.1,
            max_attempts: 3,
        }
    }
}

impl TemperatureDecayRetry {
    /// Returns the temperature that should be used for the given attempt.
    ///
    /// Attempt 0 returns `base_temperature`, attempt 1 returns
    /// `base_temperature - decay`, and so on, floored at 0.1.
    pub fn temperature_for_attempt(&self, attempt: u32) -> f64 {
        (self.base_temperature - attempt as f64 * self.decay_per_attempt).max(0.1)
    }

    /// Returns `true` when `attempt` is still below `max_attempts`.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }

    /// Given a current temperature, returns the next lower temperature
    /// (current - decay), floored at 0.1.
    pub fn next_temperature(&self, current: f64) -> f64 {
        (current - self.decay_per_attempt).max(0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> TemperatureDecayRetry {
        TemperatureDecayRetry::default()
    }

    // --- temperature_for_attempt ---

    #[test]
    fn temperature_sequence_follows_decay() {
        let policy = default_policy();
        // 0.7, 0.6, 0.5, 0.4
        let expected = [0.7, 0.6, 0.5, 0.4];
        for (attempt, &expect) in expected.iter().enumerate() {
            let got = policy.temperature_for_attempt(attempt as u32);
            assert!(
                (got - expect).abs() < 1e-10,
                "attempt {attempt}: expected {expect}, got {got}"
            );
        }
    }

    #[test]
    fn temperature_floors_at_minimum() {
        let policy = TemperatureDecayRetry {
            base_temperature: 0.3,
            decay_per_attempt: 0.2,
            max_attempts: 10,
        };
        // attempt 0: 0.3, attempt 1: 0.1, attempt 2+: still 0.1
        assert!((policy.temperature_for_attempt(0) - 0.3).abs() < 1e-10);
        assert!((policy.temperature_for_attempt(1) - 0.1).abs() < 1e-10);
        assert!((policy.temperature_for_attempt(5) - 0.1).abs() < 1e-10);
        assert!((policy.temperature_for_attempt(100) - 0.1).abs() < 1e-10);
    }

    // --- should_retry ---

    #[test]
    fn should_retry_within_limit() {
        let policy = default_policy(); // max_attempts = 3
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
        assert!(!policy.should_retry(4));
    }

    #[test]
    fn should_retry_boundary() {
        let policy = TemperatureDecayRetry {
            max_attempts: 1,
            ..Default::default()
        };
        assert!(policy.should_retry(0));
        assert!(!policy.should_retry(1));
    }

    // --- next_temperature ---

    #[test]
    fn next_temperature_decrements() {
        let policy = default_policy();
        assert!((policy.next_temperature(0.7) - 0.6).abs() < 1e-10);
        assert!((policy.next_temperature(0.6) - 0.5).abs() < 1e-10);
        assert!((policy.next_temperature(0.5) - 0.4).abs() < 1e-10);
    }

    #[test]
    fn next_temperature_floors_at_minimum() {
        let policy = default_policy();
        assert!((policy.next_temperature(0.15) - 0.1).abs() < 1e-10);
        assert!((policy.next_temperature(0.1) - 0.1).abs() < 1e-10);
        assert!((policy.next_temperature(0.05) - 0.1).abs() < 1e-10);
    }

    // --- serde round-trip ---

    #[test]
    fn serde_round_trip() {
        let policy = TemperatureDecayRetry {
            base_temperature: 0.85,
            decay_per_attempt: 0.15,
            max_attempts: 5,
        };
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: TemperatureDecayRetry = serde_json::from_str(&json).unwrap();
        assert!((deserialized.base_temperature - 0.85).abs() < 1e-10);
        assert!((deserialized.decay_per_attempt - 0.15).abs() < 1e-10);
        assert_eq!(deserialized.max_attempts, 5);
    }

    // --- clone ---

    #[test]
    fn clone_is_independent() {
        let policy = default_policy();
        let cloned = policy.clone();
        assert!((policy.base_temperature - cloned.base_temperature).abs() < 1e-10);
    }

    // --- default ---

    #[test]
    fn default_values() {
        let policy = TemperatureDecayRetry::default();
        assert!((policy.base_temperature - 0.7).abs() < 1e-10);
        assert!((policy.decay_per_attempt - 0.1).abs() < 1e-10);
        assert_eq!(policy.max_attempts, 3);
    }
}
