//! 熔断器模块
//!
//! 实现熔断器模式，用于防止向不健康的供应商发送请求

use super::log_codes::cb as log_cb;
use super::types::AppProxyConfig;
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
    /// HalfOpen permit 最大占用时长（秒）。
    ///
    /// 探测请求发出后若超过该时长仍未通过 `record_success` / `record_failure`
    /// 显式释放，则会在下一次 `allow_request` 被强制回收，防止上游挂起导致
    /// HalfOpen 状态卡死、新探测全部被拒。
    pub half_open_permit_max_age_seconds: u64,
}

impl From<&AppProxyConfig> for CircuitBreakerConfig {
    fn from(config: &AppProxyConfig) -> Self {
        Self {
            failure_threshold: config.circuit_failure_threshold,
            success_threshold: config.circuit_success_threshold,
            timeout_seconds: config.circuit_timeout_seconds as u64,
            error_rate_threshold: config.circuit_error_rate_threshold,
            min_requests: config.circuit_min_requests,
            half_open_permit_max_age_seconds: config.circuit_half_open_permit_max_age_seconds as u64,
        }
    }
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 4,
            success_threshold: 2,
            timeout_seconds: 60,
            error_rate_threshold: 0.6,
            min_requests: 10,
            half_open_permit_max_age_seconds: 30,
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
    /// 当前 HalfOpen permit 的获取时间（用于超时自释放）。
    ///
    /// 与 `half_open_requests` 计数器配合：探测请求超过
    /// `config.half_open_permit_max_age_seconds` 未显式释放时，
    /// 下一次 `allow_request` 会强制回收该名额，避免上游挂起导致卡死。
    half_open_permit_acquired_at: Arc<std::sync::Mutex<Option<Instant>>>,
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
            half_open_permit_acquired_at: Arc::new(std::sync::Mutex::new(None)),
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
                            CircuitState::HalfOpen => {
                                self.maybe_release_stale_half_open_permit().await;
                                self.allow_half_open_probe()
                            }
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
            CircuitState::HalfOpen => {
                // HalfOpen：先清理过期的探测名额，防止上游挂起导致状态卡死
                self.maybe_release_stale_half_open_permit().await;
                self.allow_half_open_probe()
            }
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
            // 记录 permit 获取时间，供 maybe_release_stale_half_open_permit 超时回收
            if let Ok(mut guard) = self.half_open_permit_acquired_at.lock() {
                *guard = Some(Instant::now());
            }
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
                // 计数器已为 0：保持时间戳与计数器一致（防御性清理）
                if let Ok(mut guard) = self.half_open_permit_acquired_at.lock() {
                    if guard.is_some() {
                        *guard = None;
                    }
                }
                return;
            }

            match self.half_open_requests.compare_exchange(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    // 真正释放了名额：清理时间戳
                    if let Ok(mut guard) = self.half_open_permit_acquired_at.lock() {
                        *guard = None;
                    }
                    return;
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// 若 HalfOpen permit 占用时长超过 `config.half_open_permit_max_age_seconds`，
    /// 强制回收该名额（清理时间戳并 CAS 递减计数器）。
    ///
    /// 这是 P0.2 的核心：探测请求若因上游挂起迟迟未触发 `record_success` /
    /// `record_failure`，permit 会一直占用、新探测全部被拒；自释放机制让
    /// 下一次 `allow_request` 能重新尝试探测，而不是等到外部 timeout 才回收。
    ///
    /// 调用方在 `allow_request` 进入 HalfOpen 分支前调用本方法。
    async fn maybe_release_stale_half_open_permit(&self) {
        let max_age = {
            let cfg = self.config.read().await;
            std::time::Duration::from_secs(cfg.half_open_permit_max_age_seconds)
        };

        // 第一阶段：在锁内读取时间戳，若未占用或未超时则直接返回
        let stale_age = {
            let mut guard = match self.half_open_permit_acquired_at.lock() {
                Ok(g) => g,
                Err(_) => return, // poisoned mutex, 保守不清理
            };
            match *guard {
                Some(t) if t.elapsed() > max_age => {
                    let age = t.elapsed();
                    *guard = None;
                    Some(age)
                }
                _ => None,
            }
        };

        let Some(age) = stale_age else {
            return;
        };

        // 第二阶段：CAS 递减计数器，避免与正常 release 路径双重递减
        let mut current = self.half_open_requests.load(Ordering::SeqCst);
        let decremented = loop {
            if current == 0 {
                break false;
            }
            match self.half_open_requests.compare_exchange_weak(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break true,
                Err(actual) => current = actual,
            }
        };

        if decremented {
            log::warn!(
                "[{}] HalfOpen permit 超时未释放 (age={:?} > max={:?})，强制回收",
                log_cb::HALF_OPEN_PERMIT_STALE,
                age,
                max_age,
            );
        } else {
            // 计数器已经为 0（被 record_success/failure 清掉了），但时间戳没及时清，
            // 此时仅日志告知，不重复递减
            log::debug!(
                "[{}] HalfOpen permit 超时但计数器已为 0（已通过 record_* 路径释放），仅清理时间戳",
                log_cb::HALF_OPEN_PERMIT_STALE,
            );
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
        // 重置半开状态的请求限流计数与 permit 时间戳
        self.half_open_requests.store(0, Ordering::SeqCst);
        if let Ok(mut guard) = self.half_open_permit_acquired_at.lock() {
            *guard = None;
        }
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

    /// P0.2 核心场景：HalfOpen permit 占用超过 max_age 后被强制回收，
    /// 下一次 allow_request 能再次拿到 permit（不被旧 permit 卡死）。
    #[tokio::test]
    async fn test_stale_half_open_permit_is_force_released() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            half_open_permit_max_age_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 进入 Open，再让 allow_request 触发 Open → HalfOpen 并占用 permit
        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 模拟探测挂起：稍等让 elapsed > max_age(=0)
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // 关键断言：第二次 allow_request 应被允许（旧 permit 超时自释放）
        // 这里模拟的是"探测挂起很久后新的探测请求"
        let second = breaker.allow_request().await;
        assert!(
            second.allowed,
            "stale HalfOpen permit must be force-released so new probe can proceed"
        );
        assert!(second.used_half_open_permit);
    }

    /// 反向场景：permit 尚未超时，不应被强制回收。
    #[tokio::test]
    async fn test_fresh_half_open_permit_is_not_released() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            half_open_permit_max_age_seconds: 300, // 5 分钟，足够长
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit);

        // permit 远未超时，第二次必须被拒
        let second = breaker.allow_request().await;
        assert!(!second.allowed);
        assert!(!second.used_half_open_permit);
    }

    /// record_success 必须清理 permit 时间戳，避免后续 allow_request 误判为 stale。
    #[tokio::test]
    async fn test_record_success_clears_half_open_permit_timestamp() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            success_threshold: 1,
            failure_threshold: 1,
            half_open_permit_max_age_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 进入 HalfOpen 并占用 permit
        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.used_half_open_permit);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);

        // 探测成功：应触发 HalfOpen → Closed；同时 time_stamp 已被清掉
        breaker.record_success(true).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        // 重新打开 + 切换到 HalfOpen：此时 half_open_permit_acquired_at 应为 None
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 再走一次 HalfOpen 探测：必须能正常拿到 permit（时间戳干净）
        let second = breaker.allow_request().await;
        assert!(second.allowed);
        assert!(second.used_half_open_permit);
    }

    /// release_half_open_permit 与 maybe_release_stale_half_open_permit 同时调用时，
    /// 计数器不能被双重递减（防计数变负）。
    #[tokio::test]
    async fn test_stale_release_and_explicit_release_do_not_double_decrement() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            failure_threshold: 1,
            half_open_permit_max_age_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 占用 permit
        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.used_half_open_permit);

        // 等 permit 超时
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // 先走 record_success 路径显式释放（counter: 1→0, ts: Some→None）
        breaker.record_success(true).await;

        // 再调用一次自释放检查：time_stamp 已为 None，应当 no-op 不再递减
        breaker.maybe_release_stale_half_open_permit().await;

        // 走一次完整 Closed → Open → HalfOpen 链路，验证计数没被错误减成负数
        breaker.reset().await;
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        let probe = breaker.allow_request().await;
        assert!(probe.allowed, "permit counter must not be left negative");
        assert!(probe.used_half_open_permit);
    }

    /// transition_to_half_open 必须清理 permit 时间戳，
    /// 否则同一熔断器在 Open → HalfOpen → Closed → Open → HalfOpen 后第二次切换会
    /// 带着上一次的 permit 时间戳，触发误判 stale。
    #[tokio::test]
    async fn test_transition_to_half_open_clears_permit_timestamp() {
        let config = CircuitBreakerConfig {
            timeout_seconds: 0,
            failure_threshold: 1,
            half_open_permit_max_age_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        // 第一次 Open → HalfOpen + permit
        breaker.transition_to_open().await;
        let first = breaker.allow_request().await;
        assert!(first.used_half_open_permit);

        // 等 permit 超时
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // 不走 record_success；直接 reset → Closed，模拟 Open→Closed
        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        // 再次 Open → HalfOpen：旧时间戳应已被 transition_to_half_open 清掉，
        // 否则 maybe_release_stale_half_open_permit 会基于"陈旧时间戳"误判 stale
        breaker.record_failure(false).await;
        let second = breaker.allow_request().await;
        assert!(
            second.allowed,
            "transition_to_half_open must reset permit timestamp so stale-check starts fresh"
        );
        assert!(second.used_half_open_permit);
    }
}
