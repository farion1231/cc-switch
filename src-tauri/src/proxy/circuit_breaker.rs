//! 熔断器模块
//!
//! 实现熔断器模式，用于防止向不健康的供应商发送请求

use super::log_codes::cb as log_cb;
use super::types::AppProxyConfig;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// 每个供应商最多保留多少条失败证据。
///
/// 证据是给人看的「哪几个请求让我判它熔断」，不是审计日志：保留得比判定阈值
/// 所需的多一些（阈值默认 4，错误率窗口 min_requests 默认 10），够看清来龙去脉
/// 即可。上限固定，避免长时间运行后无界增长——这个结构在每条请求的失败路径上
/// 都会写一次。
const MAX_FAILURE_EVIDENCE: usize = 20;

/// 上游 Retry-After 能把熔断撑多久的天花板（24 小时）。
///
/// 头本身合法的巨大值照样被接受——「日配额明天恢复」是上游的正当表达——但
/// `Retry-After: 999999999999`（约三万年）也完全符合语法，照单全收等于把供应商
/// 永久移出调度。钳到一天：真实的日配额不受影响，恶意/出错的头最多损失一天，
/// 且面板上的「清除计数」随时可以提前拉回来。
const MAX_RETRY_AFTER_OPEN_SECONDS: u64 = 24 * 60 * 60;

/// 一条失败证据：一次真正计入了熔断器的请求失败。
///
/// 只记录**计入健康度**的失败。客户端错误（400/422 等）走 neutral 路径，不写
/// 这里——它们本来就不该影响判定，出现在证据里只会让人误以为是被它们熔断的。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureEvidence {
    /// 失败发生的时刻（本地时间，带时区，便于 UI 直接展示）
    pub at: String,
    /// 上游 HTTP 状态码；网络层失败（超时、连接重置）没有状态码
    pub status: Option<u16>,
    /// 给用户看的一句话原因（上游错误消息或错误类型）
    pub reason: String,
    /// 这次失败是否直接触发了熔断（连续失败达阈值 / 错误率越线 / HalfOpen 探测失败）
    pub tripped: bool,
}

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

impl From<&AppProxyConfig> for CircuitBreakerConfig {
    fn from(config: &AppProxyConfig) -> Self {
        Self {
            failure_threshold: config.circuit_failure_threshold,
            success_threshold: config.circuit_success_threshold,
            timeout_seconds: config.circuit_timeout_seconds as u64,
            error_rate_threshold: config.circuit_error_rate_threshold,
            min_requests: config.circuit_min_requests,
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
    /// 本次 Open 的时长（秒），来自上游 429 的 Retry-After。
    /// `None` = 用配置里的 `timeout_seconds`。
    open_for_seconds: Arc<RwLock<Option<u64>>>,
    /// 配置（支持热更新）
    config: Arc<RwLock<CircuitBreakerConfig>>,
    /// 半开状态已放行的请求数（用于限流）
    half_open_requests: Arc<AtomicU32>,
    /// 最近的失败证据（新的在前），供 UI 解释「为什么判它不健康」
    failure_evidence: Arc<RwLock<VecDeque<FailureEvidence>>>,
}

/// 一次失败的说明，由调用方在记录失败时提供。
///
/// 与 [`FailureEvidence`] 分开：这个只是「这次失败长什么样」，还没有时间戳和
/// `tripped` 标记——那两样由熔断器自己填，调用方无从得知。
#[derive(Debug, Clone)]
pub struct FailureDetail {
    pub status: Option<u16>,
    pub reason: String,
}

impl FailureDetail {
    pub fn new(status: Option<u16>, reason: impl Into<String>) -> Self {
        Self {
            status,
            reason: reason.into(),
        }
    }
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
            open_for_seconds: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            half_open_requests: Arc::new(AtomicU32::new(0)),
            failure_evidence: Arc::new(RwLock::new(VecDeque::new())),
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

        match state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                let open_timeout = self.effective_open_timeout_seconds().await;
                if let Some(opened_at) = *self.last_opened_at.read().await {
                    if opened_at.elapsed().as_secs() >= open_timeout {
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
                // 用这次 Open 实际生效的时长（上游 Retry-After 优先于配置值）
                let open_timeout = self.effective_open_timeout_seconds().await;
                // 检查是否应该尝试半开
                if let Some(opened_at) = *self.last_opened_at.read().await {
                    if opened_at.elapsed().as_secs() >= open_timeout {
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
    ///
    /// `detail` 是给用户看的失败说明（状态码 + 原因）；`None` 表示调用方没有可展示的
    /// 细节，此时仍会记录一条通用证据，保证「判定过但看不到任何依据」不会发生。
    pub async fn record_failure(
        &self,
        used_half_open_permit: bool,
        detail: Option<FailureDetail>,
        open_for: Option<u64>,
    ) {
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
        let tripped = match state {
            CircuitState::HalfOpen => {
                // HalfOpen 状态下失败，立即转为 Open
                log::warn!(
                    "[{}] 熔断器 HalfOpen 探测失败 → Open",
                    log_cb::HALF_OPEN_PROBE_FAILED
                );
                drop(config);
                self.transition_to_open(open_for).await;
                true
            }
            CircuitState::Closed => {
                // 上游明确说「我限流了，N 秒后再来」时立刻熔断，不等连续失败阈值。
                // 这不是猜测：429 + Retry-After 是上游自己给出的恢复时间，继续按
                // 阈值放请求过去只会把限流窗口拖长，还会让后续请求白烧配额。
                if let Some(seconds) = open_for {
                    log::warn!(
                        "[{}] 熔断器触发: 上游 429 要求等待 {seconds}s → Open",
                        log_cb::TRIGGERED_RATE_LIMITED
                    );
                    drop(config); // 释放读锁再转换状态
                    self.transition_to_open(open_for).await;
                    true
                } else if failures >= config.failure_threshold {
                    log::warn!(
                        "[{}] 熔断器触发: 连续失败 {failures} 次 → Open",
                        log_cb::TRIGGERED_FAILURES
                    );
                    drop(config); // 释放读锁再转换状态
                    self.transition_to_open(open_for).await;
                    true
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
                            self.transition_to_open(open_for).await;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
            _ => false,
        };

        self.push_failure_evidence(detail, tripped).await;
    }

    /// 追加一条失败证据（新的在前，超过上限丢最旧的）。
    ///
    /// 放在所有状态判定之后调用：`tripped` 要等判定做完才知道，而证据里必须如实
    /// 标出「就是这一次把熔断器打开的」——用户正是靠这一条把判定和请求对上号。
    async fn push_failure_evidence(&self, detail: Option<FailureDetail>, tripped: bool) {
        let evidence = FailureEvidence {
            at: chrono::Local::now().to_rfc3339(),
            status: detail.as_ref().and_then(|d| d.status),
            reason: detail
                .map(|d| d.reason)
                .unwrap_or_else(|| "上游请求失败（调用方未提供详情）".to_string()),
            tripped,
        };

        let mut evidence_log = self.failure_evidence.write().await;
        evidence_log.push_front(evidence);
        evidence_log.truncate(MAX_FAILURE_EVIDENCE);
    }

    /// 读取失败证据快照（新的在前）。
    pub async fn failure_evidence(&self) -> Vec<FailureEvidence> {
        self.failure_evidence.read().await.iter().cloned().collect()
    }

    /// 清空失败证据（手动恢复时调用，避免旧证据继续解释一个已经被清掉的状态）。
    async fn clear_failure_evidence(&self) {
        self.failure_evidence.write().await.clear();
    }

    /// 获取当前状态
    #[allow(dead_code)]
    pub async fn get_state(&self) -> CircuitState {
        *self.state.read().await
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            state: *self.state.read().await,
            consecutive_failures: self.consecutive_failures.load(Ordering::SeqCst),
            consecutive_successes: self.consecutive_successes.load(Ordering::SeqCst),
            total_requests: self.total_requests.load(Ordering::SeqCst),
            failed_requests: self.failed_requests.load(Ordering::SeqCst),
            failure_evidence: self.failure_evidence().await,
        }
    }

    /// 重置熔断器（手动恢复）
    ///
    /// 除了回到 Closed，还要清掉失败证据：证据的唯一用途是解释当前这个判定，
    /// 判定被用户主动推翻后继续留着，下次看到的就是一段与现状无关的历史。
    pub async fn reset(&self) {
        log::info!("[{}] 熔断器手动重置 → Closed", log_cb::MANUAL_RESET);
        self.transition_to_closed().await;
        self.clear_failure_evidence().await;
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
    ///
    /// `open_for` 是这次熔断要持续多久。`None` 表示用配置里的 `timeout_seconds`
    /// ——绝大多数失败都走这条路径，行为与以前一致。上游用 429 + Retry-After
    /// 明确告诉你「多久之后再来」时，调用方会传 `Some`，让熔断时长跟着上游走，
    /// 而不是傻等配置值。上游给的值按 [`MAX_RETRY_AFTER_OPEN_SECONDS`] 钳制。
    async fn transition_to_open(&self, open_for: Option<u64>) {
        *self.state.write().await = CircuitState::Open;
        *self.last_opened_at.write().await = Some(Instant::now());
        *self.open_for_seconds.write().await =
            open_for.map(|seconds| seconds.min(MAX_RETRY_AFTER_OPEN_SECONDS));
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.consecutive_successes.store(0, Ordering::SeqCst);
    }

    /// 当前这次 Open 应该持续多少秒：优先用上游给的，否则用配置值。
    async fn effective_open_timeout_seconds(&self) -> u64 {
        if let Some(seconds) = *self.open_for_seconds.read().await {
            return seconds;
        }
        self.config.read().await.timeout_seconds
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
        // 关掉这次 Open 时上游给的时长承诺：它只描述「刚才那次 429 要等多久」，
        // 状态已经回到 Closed 再留着，下一次普通失败打开的熔断器会莫名其妙地
        // 沿用一段早该过期的等待时间。
        *self.open_for_seconds.write().await = None;
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
    /// 最近导致（或接近导致）熔断的失败请求，新的在前。
    ///
    /// 有了它，UI 才能回答「是哪几个请求让你判它熔断的」——否则用户在界面上只看到
    /// 一个「熔断」徽章，却无从判断是不是上游真的挂了，还是本地网络抖了一下。
    #[serde(default)]
    pub failure_evidence: Vec<FailureEvidence>,
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
            breaker.record_failure(false, None, None).await;
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
        breaker.record_failure(false, None, None).await;
        breaker.record_failure(false, None, None).await;
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
        breaker.transition_to_open(None).await;
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
        breaker.record_failure(false, None, None).await;
        breaker.record_failure(false, None, None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 重置
        breaker.reset().await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn failure_evidence_records_status_and_reason() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
        breaker
            .record_failure(false, Some(FailureDetail::new(Some(503), "upstream overloaded")), None)
            .await;

        let evidence = breaker.failure_evidence().await;
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].status, Some(503));
        assert_eq!(evidence[0].reason, "upstream overloaded");
        // 阈值默认 4，第一次失败不该标成「就是它触发的」
        assert!(!evidence[0].tripped);
    }

    #[tokio::test]
    async fn failure_evidence_marks_the_request_that_tripped_the_breaker() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker
            .record_failure(false, Some(FailureDetail::new(Some(500), "first")), None)
            .await;
        breaker
            .record_failure(false, Some(FailureDetail::new(Some(500), "second")), None)
            .await;
        breaker
            .record_failure(false, Some(FailureDetail::new(Some(502), "third")), None)
            .await;

        let evidence = breaker.failure_evidence().await;
        assert_eq!(evidence.len(), 3);
        // 新的在前
        assert_eq!(evidence[0].reason, "third");
        assert_eq!(evidence[2].reason, "first");
        // 只有第 3 次（达阈值那一次）被标为触发
        assert!(evidence[0].tripped);
        assert!(!evidence[1].tripped);
        assert!(!evidence[2].tripped);
    }

    #[tokio::test]
    async fn failure_evidence_is_capped_so_a_long_outage_cannot_grow_without_bound() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
        for i in 0..(MAX_FAILURE_EVIDENCE + 15) {
            breaker
                .record_failure(
                    false,
                    Some(FailureDetail::new(Some(500), format!("fail-{i}"))),
                    None,
                )
                .await;
        }

        let evidence = breaker.failure_evidence().await;
        assert_eq!(evidence.len(), MAX_FAILURE_EVIDENCE);
        // 保留的是最新的那一批
        assert_eq!(
            evidence[0].reason,
            format!("fail-{}", MAX_FAILURE_EVIDENCE + 14)
        );
    }

    #[tokio::test]
    async fn failure_evidence_falls_back_when_the_caller_has_no_detail() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig::default());
        breaker.record_failure(false, None, None).await;

        let evidence = breaker.failure_evidence().await;
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].status, None);
        // 空理由会让 UI 显示一条没有内容的证据，等于没给证据
        assert!(!evidence[0].reason.is_empty());
    }

    #[tokio::test]
    async fn manual_reset_clears_failure_evidence() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        breaker
            .record_failure(false, Some(FailureDetail::new(Some(500), "boom")), None)
            .await;
        breaker
            .record_failure(false, Some(FailureDetail::new(Some(500), "boom again")), None)
            .await;
        assert_eq!(breaker.failure_evidence().await.len(), 2);

        breaker.reset().await;

        // 判定被用户推翻后，旧证据继续留着只会解释一个已经不存在的状态
        assert!(breaker.failure_evidence().await.is_empty());
    }

    #[tokio::test]
    async fn success_does_not_clear_evidence_before_the_breaker_closes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 5,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        breaker
            .record_failure(false, Some(FailureDetail::new(Some(500), "one failure")), None)
            .await;
        breaker.record_success(false).await;

        // 一次成功只把连续失败清零（状态因此从「降级」回到「正常」），
        // 但证据仍在——它记录的是「发生过什么」，不是「现在是什么」。
        assert_eq!(breaker.get_stats().await.consecutive_failures, 0);
        assert_eq!(breaker.failure_evidence().await.len(), 1);
    }

    #[tokio::test]
    async fn upstream_retry_after_trips_the_breaker_immediately() {
        // 阈值是 5 次连续失败；一次 429 + Retry-After 就该立刻熔断，
        // 因为上游已经把「什么时候能再来」说死了。
        let config = CircuitBreakerConfig {
            failure_threshold: 5,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure(false, None, Some(120)).await;

        assert_eq!(breaker.get_state().await, CircuitState::Open);
        let evidence = breaker.failure_evidence().await;
        assert_eq!(evidence.len(), 1);
        // 证据里必须标出「就是这一条触发的」，否则用户看到的是 5 次阈值
        // 却只有 1 条记录，会怀疑是不是漏记了
        assert!(evidence[0].tripped);
    }

    #[tokio::test]
    async fn a_plain_failure_still_needs_the_threshold() {
        let config = CircuitBreakerConfig {
            failure_threshold: 5,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        for _ in 0..4 {
            breaker.record_failure(false, None, None).await;
        }
        // 没有 Retry-After 的失败必须照旧按阈值来
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        breaker.record_failure(false, None, None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn an_absurdly_large_retry_after_is_clamped_to_one_day() {
        // 头完全合法但值是三万年：照单全收等于永久熔断。钳到一天，
        // 「清除计数」仍是提前拉回的出口。
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 3600,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker
            .record_failure(false, None, Some(999_999_999_999))
            .await;

        assert_eq!(breaker.get_state().await, CircuitState::Open);
        // 钳制值 24h 赢过配置的 1h，又远远小于头里的原始值
        assert_eq!(
            breaker.effective_open_timeout_seconds().await,
            MAX_RETRY_AFTER_OPEN_SECONDS
        );
    }

    #[tokio::test]
    async fn a_retry_after_below_the_cap_is_taken_verbatim() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 30,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure(false, None, Some(120)).await;

        assert_eq!(breaker.effective_open_timeout_seconds().await, 120);
    }

    #[tokio::test]
    async fn upstream_retry_after_extends_the_open_window_past_the_config() {
        // 配置说 0 秒后就可以重试；上游说 429 之后等 3600 秒。
        // 上游的话必须赢，否则熔断器会立刻放行一个明确被限流的供应商。
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure(false, None, Some(3600)).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // timeout_seconds = 0 意味着配置层面「立刻可恢复」，但这次 Open 应该按
        // Retry-After 撑住，仍然拒绝放行。
        assert!(!breaker.allow_request().await.allowed);
        assert_eq!(breaker.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn a_failure_without_retry_after_uses_the_configured_timeout() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure(false, None, None).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);

        // 没有 Retry-After 时行为照旧：0 秒配置下立即进入 HalfOpen 并放行探测
        assert!(breaker.allow_request().await.allowed);
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn closing_clears_the_upstream_retry_after_override() {
        // 上游说过等 3600 秒，之后这次 Open 正常走完、回到 Closed。
        // 再下一次普通失败必须用配置时长——不能沿用一段早该过期的承诺。
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure(false, None, Some(3600)).await;
        // 探测放行 → 半开 → 一次成功 → 回到 Closed（同时清掉 open_for）
        assert!(breaker.allow_request().await.allowed);
        breaker.record_success(true).await;
        assert_eq!(breaker.get_state().await, CircuitState::Closed);

        breaker.record_failure(false, None, None).await;
        // 配置的 0 秒重新生效，而不是残留的 3600
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn a_later_failure_without_retry_after_clears_the_previous_override() {
        // 上一次是 429 带 Retry-After，这一次是普通 5xx。第二次 Open 必须回到
        // 配置时长，不能继续沿用上一次那个已经过期的上游承诺。
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);

        breaker.record_failure(false, None, Some(3600)).await;
        assert!(!breaker.allow_request().await.allowed);

        // 手动回到 Closed 模拟半开探测成功，再用一次无 Retry-After 的失败打开
        breaker.reset().await;
        breaker.record_failure(false, None, None).await;

        assert!(breaker.allow_request().await.allowed);
    }
}
