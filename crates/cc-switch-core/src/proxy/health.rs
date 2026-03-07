//! Health checker for providers

use std::time::Duration;

pub struct HealthChecker {
    _timeout: Duration,
}

impl HealthChecker {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            _timeout: Duration::from_secs(timeout_secs),
        }
    }

    pub async fn check(&self, _url: &str) -> Result<u64, String> {
        Ok(0)
    }
}
