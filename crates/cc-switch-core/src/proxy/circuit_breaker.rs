//! Circuit breaker implementation

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    failure_threshold: u32,
    recovery_timeout: Duration,
    failure_count: AtomicU32,
    last_failure: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_timeout_secs: u64) -> Self {
        Self {
            failure_threshold,
            recovery_timeout: Duration::from_secs(recovery_timeout_secs),
            failure_count: AtomicU32::new(0),
            last_failure: Mutex::new(None),
        }
    }

    pub fn state(&self) -> CircuitState {
        let failures = self.failure_count.load(Ordering::Relaxed);
        if failures < self.failure_threshold {
            return CircuitState::Closed;
        }

        let last = self.last_failure.lock().unwrap();
        if let Some(time) = *last {
            if time.elapsed() > self.recovery_timeout {
                return CircuitState::HalfOpen;
            }
        }

        CircuitState::Open
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        let mut last = self.last_failure.lock().unwrap();
        *last = Some(Instant::now());
    }

    pub fn is_available(&self) -> bool {
        matches!(self.state(), CircuitState::Closed | CircuitState::HalfOpen)
    }
}
