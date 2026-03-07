//! 熔断器模块
//!
//! 实现熔断器模式，用于防止向不健康的供应商发送请求

use super::log_codes::cb as log_cb;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// 关闭状态 - 正常工作
    Closed,
    /// 打开状态 - 熔断激活，拒绝请求
    Open,
    /// 半开状态 - 尝试恢复，允许部分请求通过
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half_open"),
        }
    }
}

/// 熔断器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakerConfig {
    /// 失败阈值 - 连续失败多少次后打开熔断器
    pub failure_threshold: u32,
    /// 成功阈值 - 半开状态下成功多少次后关闭熔断器
    pub success_threshold: u32,
    /// 超时时间 - 熔断器打开后多久尝试半开（秒）
    pub timeout_seconds: u64,
    /// 错误率阈值 - 错误率超过此值时打开熔断器 (0.0-1.0)
    pub error_rate_threshold: f64,
    /// 最小请求数 - 计算错误率前的最小请求数
    pub min_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 4,
            success_threshold: 2,
            timeout_seconds: 60,
            error_rate_threshold: 0.6,
            min_requests: 10,
        }
    }
}

/// 熔断器实例
pub struct CircuitBreaker {
    /// 当前状态
    state: Arc<RwLock<CircuitState>>,
    /// 连续失败计数
    consecutive_failures: Arc<AtomicU32>,
    /// 连续成功计数（半开状态）
    consecutive_successes: Arc<AtomicU32>,
    /// 总请求计数
    total_requests: Arc<AtomicU32>,
    /// 失败请求计数
    failed_requests: Arc<AtomicU32>,
    /// 上次打开时间
    last_opened_at: Arc<RwLock<Option<Instant>>>,
    /// 配置（支持热更新）
    config: Arc<RwLock<CircuitBreakerConfig>>,
    /// 半开状态已放行的请求数（用于限流）
    half_open_requests: Arc<AtomicU32>,
}

/// 熔断器放行结果
///
/// `used_half_open_permit` 表示本次放行是否占用了 HalfOpen 探测名额。
/// 调用方应在请求结束后把该值传回 `record_success` / `record_failure` 用于正确释放名额。
#[derive(Debug, Clone, Copy)]
pub struct AllowResult {
    pub allowed: bool,
    pub used_half_open_permit: bool,
}

impl CircuitBreaker {
    /// 创建新的熔断器
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            consecutive_successes: Arc::new(AtomicU32::new(0)),
            total_requests: Arc::new(AtomicU32::new(0)),
            failed_requests: Arc::new(AtomicU32::new(0)),
            last_opened_at: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            half_open_requests: Arc::new(AtomicU32::new(0)),
        }
    }

    /// 更新熔断器配置（热更新，不重置状态）
    pub async fn update_config(&self, new_config: CircuitBreakerConfig) {
        *self.config.write().await = new_config;
    }

    /// 判断当前 Provider 是否“可被纳入候选链路”
    ///
    /// 这个方法不会占用 HalfOpen 探测名额，仅用于路由选择阶段的“可用性判断”：
    /// - Closed / HalfOpen：可用（返回 true）
    /// - Open：若超时到达则切到 HalfOpen 并返回 true，否则返回 false
    ///
    /// 注意：真正发起请求前仍需调用 `allow_request()` 来获取 HalfOpen 探测名额，
    /// 并在请求结束后通过 `record_success()` / `record_failure()` 释放。
    pub async fn is_available(&self) -> bool {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        match state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(opened_at) = *self.last_opened_at.read().await {
                    if opened_at.elapsed().as_secs() >= config.timeout_seconds {
                        drop(config); // 释放读锁再转换状态
                        log::info!(
                            "[{}] 熔断器 Open → HalfOpen (超时恢复)",
                            log_cb::OPEN_TO_HALF_OPEN
                        );
                        self.transition_to_half_open().await;
                        return true;
                    }
                }
                false
            }
        }
    }

    /// 检查是否允许请求通过
    pub async fn allow_request(&self) -> AllowResult {
        let state = *self.state.read().await;

        match state {
            CircuitState::Closed => AllowResult {
                allowed: true,
                used_half_open_permit: false,
            },
            CircuitState::Open => {
                let config = self.config.read().await;
                // 检查是否应该尝试半开
                if let Some(opened_at) = *self.last_opened_at.read().await {
                    if opened_at.elapsed().as_secs() >= config.timeout_seconds {
                        drop(config); // 释放读锁再转换状态
                        log::info!(
                            "[{}] 熔断器 Open → HalfOpen (超时恢复)",
                            log_cb::OPEN_TO_HALF_OPEN
                        );
                        self.transition_to_half_open().await;

                        // 转换后按当前状态决定是否需要获取 HalfOpen 探测名额
                        let current_state = *self.state.read().await;
                        return match current_state {
                            CircuitState::Closed => AllowResult {
                                allowed: true,
                                used_half_open_permit: false,
                            },
                            CircuitState::HalfOpen => self.allow_half_open_probe(),
                            CircuitState::Open => AllowResult {
                                allowed: false,
                                used_half_open_permit: false,
                            },
                        };
                    }
                }

                AllowResult {
                    allowed: false,
                    used_half_open_permit: false,
                }
            }
            CircuitState::HalfOpen => self.allow_half_open_probe(),
        }
    }

    /// 记录成功
    pub async fn record_success(&self, used_half_open_permit: bool) {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        if used_half_open_permit {
            self.release_half_open_permit();
        }

        // 重置失败计数
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.total_requests.fetch_add(1, Ordering::SeqCst);

        if state == CircuitState::HalfOpen {
            let successes = self.consecutive_successes.fetch_add(1, Ordering::SeqCst) + 1;

            if successes >= config.success_threshold {
                drop(config); // 释放读锁再转换状态
                log::info!(
                    "[{}] 熔断器 HalfOpen → Closed (恢复正常)",
                    log_cb::HALF_OPEN_TO_CLOSED
                );
                self.transition_to_closed().await;
            }
        }
    }

    /// 记录失败
    pub async fn record_failure(&self, used_half_open_permit: bool) {
        let state = *self.state.read().await;
        let config = self.config.read().await;

        if used_half_open_permit {
            self.release_half_open_permit();
        }

        // 更新计数器
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.failed_requests.fetch_add(1, Ordering::SeqCst);

        // 重置成功计数
        self.consecutive_successes.store(0, Ordering::SeqCst);

        // 检查是否应该打开熔断器
        match state {
            CircuitState::HalfOpen => {
                // HalfOpen 状态下失败，立即转为 Open
                log::warn!(
                    "[{}] 熔断器 HalfOpen 探测失败 → Open",
                    log_cb::HALF_OPEN_PROBE_FAILED
                );
                drop(config);
                self.transition_to_open().await;
            }
            CircuitState::Closed => {
                // 检查连续失败次数
                if failures >= config.failure_threshold {
                    log::warn!(
                        "[{}] 熔断器触发: 连续失败 {failures} 次 → Open",
                        log_cb::TRIGGERED_FAILURES
                    );
                    drop(config); // 释放读锁再转换状态
                    self.transition_to_open().await;
                } else {
                    // 检查错误率
                    let total = self.total_requests.load(Ordering::SeqCst);
                    let failed = self.failed_requests.load(Ordering::SeqCst);

                    if total >= config.min_requests {
                        let error_rate = failed as f64 / total as f64;

                        if error_rate >= config.error_rate_threshold {
                            log::warn!(
                                "[{}] 熔断器触发: 错误率 {:.1}% → Open",
                                log_cb::TRIGGERED_ERROR_RATE,
                                error_rate * 100.0
                            );
                            drop(config); // 释放读锁再转换状态
                            self.transition_to_open().await;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// 获取当前状态
    #[allow(dead_code)]
    pub async fn get_state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// 获取统计信息
    #[allow(dead_code)]
    pub async fn get_stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: *self.state.read().await,
            consecutive_failures: self.consecutive_failures.load(Ordering::SeqCst),
            consecutive_successes: self.consecutive_successes.load(Ordering::SeqCst),
            total_requests: self.total_requests.load(Ordering::SeqCst),
            failed_requests: self.failed_requests.load(Ordering::SeqCst),
        }
    }

    /// 重置熔断器（手动恢复）
    #[allow(dead_code)]
    pub async fn reset(&self) {
        log::info!("[{}] 熔断器手动重置 → Closed", log_cb::MANUAL_RESET);
        self.transition_to_closed().await;
    }

    fn allow_half_open_probe(&self) -> AllowResult {
        // 半开状态限流：只允许有限请求通过进行探测
        let max_half_open_requests = 1u32;
        let current = self.half_open_requests.fetch_add(1, Ordering::SeqCst);

        if current < max_half_open_requests {
            AllowResult {
                allowed: true,
                used_half_open_permit: true,
            }
        } else {
            // 超过限额，回退计数，拒绝请求
            self.half_open_requests.fetch_sub(1, Ordering::SeqCst);
            AllowResult {
                allowed: false,
                used_half_open_permit: false,
            }
        }
    }

    /// 仅释放 HalfOpen permit，不影响健康统计
    ///
    /// 用于整流器等场景：请求结果不应计入 Provider 健康度，
    /// 但仍需释放占用的探测名额，避免 HalfOpen 状态卡死
    pub fn release_half_open_permit(&self) {
        let mut current = self.half_open_requests.load(Ordering::SeqCst);
        loop {
            if current == 0 {
                return;
            }

            match self.half_open_requests.compare_exchange(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    /// 转换到打开状态
    async fn transition_to_open(&self) {
        *self.state.write().await = CircuitState::Open;
        *self.last_opened_at.write().await = Some(Instant::now());
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
    }

    /// 转换到半开状态
    async fn transition_to_half_open(&self) {
        let mut state = self.state.write().await;
        if *state != CircuitState::Open {
            return;
        }

        *state = CircuitState::HalfOpen;
        self.consecutive_successes.store(0, Ordering::SeqCst);
        // 重置半开状态的请求限流计数
        self.half_open_requests.store(0, Ordering::SeqCst);
    }

    /// 转换到关闭状态
    async fn transition_to_closed(&self) {
        *self.state.write().await = CircuitState::Closed;
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
        // 重置计数器
        self.total_requests.store(0, Ordering::SeqCst);
        self.failed_requests.store(0, Ordering::SeqCst);
    }
}

/// 熔断器统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakerStats {
    pub state: CircuitState,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub total_requests: u32,
    pub failed_requests: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed_to_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 初始状态应该是关闭
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);

        // 记录 3 次失败
        for _ in 0..3 {
            breaker.record_failure(false).await;
        }

        // 应该转换到打开状态
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        assert!(!breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_to_closed() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 打开熔断器
        breaker.record_failure(false).await;
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 手动转换到半开状态
        breaker.transition_to_half_open().await;
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 记录 2 次成功
        breaker.record_success(false).await;
        breaker.record_success(false).await;

        // 应该转换到关闭状态
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_half_open_transition_does_not_reset_inflight_permit() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 进入 Open，然后由于 timeout_seconds=0，allow_request 会立即切换到 HalfOpen 并占用探测名额
        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 模拟并发下的“重复 HalfOpen 转换调用”，不应重置 in-flight 计数
        breaker.transition_to_half_open().await;

        // 由于名额仍被占用，第二次请求应被拒绝
        let second = breaker.allow_request().await;
        assert!(!second.allowed);
        assert!(!second.used_half_open_permit);
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 打开熔断器
        breaker.record_failure(false).await;
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 重置
        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);
    }
}
