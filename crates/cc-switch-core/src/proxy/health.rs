//! Health checker for providers

use std::time::Duration;

pub struct HealthChecker {
    timeout: Duration,
}

impl HealthChecker {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    pub async fn check(&self, _url: &str) -> Result<u64, String> {
        Ok(0)
    }
}
