//! 供应商级请求削峰（客户端限流）
//!
//! 背景：某些订阅制网关（例如火山方舟 Coding Plan）按**滚动一分钟的请求数**
//! 放行，超出后直接返回 429，而 Codex 对自定义 provider 的 429 不做退避重试，
//! 于是一次瞬时超限就变成用户可见的 `exceeded retry limit`。
//!
//! 这里在转发前做一次令牌桶排队：桶满时可以连续放行 `burst` 个请求，之后按
//! `rpm` 的速率逐个补充。排队超过 `max_wait` 后**放行**（fail-open），保证
//! 削峰只会增加延迟，永远不会因为本地排队而让请求失败。
//!
//! 配置放在供应商级（`ProviderMeta::proxy_rate_limit`），默认关闭。

use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;

use crate::provider::ProxyRateLimit;

/// 未显式配置时的每分钟请求数。
pub const DEFAULT_RPM: u32 = 18;
/// 未显式配置时的突发容量。
pub const DEFAULT_BURST: u32 = 2;
/// 未显式配置时的最长排队时间。
pub const DEFAULT_MAX_WAIT_MS: u64 = 30_000;

const MIN_RPM: u32 = 1;
const MAX_RPM: u32 = 600;
const MAX_BURST: u32 = 50;
const MAX_WAIT_MS: u64 = 300_000;

/// 归一化后的限流参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitConfig {
    pub rpm: u32,
    pub burst: u32,
    pub max_wait: Duration,
}

impl RateLimitConfig {
    /// 从供应商 meta 解析；`enabled = false` 或字段缺失返回 `None`。
    ///
    /// 越界值一律 clamp，避免手改配置文件把代理卡死（例如 rpm = 0 会让间隔无穷大）。
    pub fn from_meta(meta: Option<&ProxyRateLimit>) -> Option<Self> {
        let meta = meta?;
        if !meta.enabled {
            return None;
        }
        Some(Self {
            rpm: meta.rpm.unwrap_or(DEFAULT_RPM).clamp(MIN_RPM, MAX_RPM),
            burst: meta.burst.unwrap_or(DEFAULT_BURST).clamp(1, MAX_BURST),
            max_wait: Duration::from_millis(
                meta.max_wait_ms
                    .unwrap_or(DEFAULT_MAX_WAIT_MS)
                    .min(MAX_WAIT_MS),
            ),
        })
    }

    fn rate_per_sec(&self) -> f64 {
        self.rpm as f64 / 60.0
    }
}

/// 一次排队的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcquireOutcome {
    /// 实际等待时长（未排队时为 0）。
    pub waited: Duration,
    /// 是否排队超时后放行。
    pub timed_out: bool,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// 单个供应商的令牌桶。
///
/// 配置与桶状态分开存储：改配置（例如把 rpm 从 18 调到 30）时保留已消耗的令牌，
/// 不重置桶，避免"改一次配置就白送一批突发额度"。
pub struct ProviderRateLimiter {
    config: RwLock<RateLimitConfig>,
    bucket: Mutex<Bucket>,
}

impl ProviderRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config: RwLock::new(config),
            bucket: Mutex::new(Bucket {
                // 新桶是满的：启动后第一批请求（如并行审核）不必先等一个间隔。
                tokens: config.burst as f64,
                last_refill: Instant::now(),
            }),
        }
    }

    /// 申请一个请求配额。超时返回 `timed_out = true`，由调用方放行。
    pub async fn acquire(&self, next: RateLimitConfig) -> AcquireOutcome {
        let config = self.apply_config(next).await;
        let started = Instant::now();
        let deadline = started + config.max_wait;

        loop {
            let wait = {
                let mut bucket = self.bucket.lock().await;
                refill(&mut bucket, &config);

                if bucket.tokens >= 1.0 {
                    bucket.tokens -= 1.0;
                    return AcquireOutcome {
                        waited: started.elapsed(),
                        timed_out: false,
                    };
                }

                if config.max_wait.is_zero() {
                    return AcquireOutcome {
                        waited: started.elapsed(),
                        timed_out: true,
                    };
                }

                let now = Instant::now();
                if now >= deadline {
                    return AcquireOutcome {
                        waited: started.elapsed(),
                        timed_out: true,
                    };
                }

                let missing = (1.0 - bucket.tokens) / config.rate_per_sec();
                Duration::from_secs_f64(missing.max(0.001))
            };

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return AcquireOutcome {
                    waited: started.elapsed(),
                    timed_out: true,
                };
            }
            tokio::time::sleep(wait.min(remaining)).await;
        }
    }

    /// 同步最新配置并返回生效值。
    ///
    /// 配置没变时只取一次读锁（每个请求都会走到这里），变了才升级为写锁并收紧桶容量。
    async fn apply_config(&self, next: RateLimitConfig) -> RateLimitConfig {
        {
            let current = self.config.read().await;
            if *current == next {
                return *current;
            }
        }

        {
            let mut current = self.config.write().await;
            *current = next;
        }

        // 配置变更后再收紧桶容量，避免保留超过新 burst 的令牌。
        let mut bucket = self.bucket.lock().await;
        if bucket.tokens > next.burst as f64 {
            bucket.tokens = next.burst as f64;
        }

        next
    }
}

fn refill(bucket: &mut Bucket, config: &RateLimitConfig) {
    let now = Instant::now();
    let elapsed = now
        .saturating_duration_since(bucket.last_refill)
        .as_secs_f64();
    if elapsed > 0.0 {
        bucket.tokens = (bucket.tokens + elapsed * config.rate_per_sec()).min(config.burst as f64);
        bucket.last_refill = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(rpm: u32, burst: u32, max_wait_ms: u64) -> RateLimitConfig {
        RateLimitConfig {
            rpm,
            burst,
            max_wait: Duration::from_millis(max_wait_ms),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn burst_is_immediate_then_requests_are_spaced() {
        let limiter = ProviderRateLimiter::new(config(60, 2, 5_000));

        let first = limiter.acquire(config(60, 2, 5_000)).await;
        let second = limiter.acquire(config(60, 2, 5_000)).await;
        assert!(!first.timed_out && !second.timed_out);
        assert!(first.waited < Duration::from_millis(10));
        assert!(second.waited < Duration::from_millis(10));

        let started = Instant::now();
        let third = limiter.acquire(config(60, 2, 5_000)).await;
        let elapsed = started.elapsed();

        assert!(!third.timed_out);
        // rpm=60 → 1 秒一个令牌，第三个请求应当等约 1 秒。
        assert!(
            elapsed >= Duration::from_millis(950) && elapsed < Duration::from_millis(1_200),
            "unexpected wait: {elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn acquisition_fails_open_after_max_wait() {
        // rpm=6 → 10 秒一个令牌；第二个请求最多等 2 秒就被放行。
        let limiter = ProviderRateLimiter::new(config(6, 1, 2_000));
        let first = limiter.acquire(config(6, 1, 2_000)).await;
        assert!(!first.timed_out);

        let started = Instant::now();
        let second = limiter.acquire(config(6, 1, 2_000)).await;
        let elapsed = started.elapsed();

        assert!(second.timed_out, "should time out instead of waiting 10s");
        assert!(
            elapsed >= Duration::from_millis(1_950) && elapsed < Duration::from_millis(2_200),
            "unexpected wait: {elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn zero_max_wait_passes_through_immediately() {
        let limiter = ProviderRateLimiter::new(config(6, 1, 0));
        let _ = limiter.acquire(config(6, 1, 0)).await;

        let started = Instant::now();
        let second = limiter.acquire(config(6, 1, 0)).await;
        assert!(second.timed_out);
        assert!(started.elapsed() < Duration::from_millis(10));
    }

    #[tokio::test(start_paused = true)]
    async fn raising_rpm_takes_effect_on_the_next_request() {
        let limiter = ProviderRateLimiter::new(config(6, 1, 5_000));
        let _ = limiter.acquire(config(6, 1, 5_000)).await;

        // 从 6 rpm（10 秒/个）改成 600 rpm（0.1 秒/个），无需重建限流器。
        let started = Instant::now();
        let next = limiter.acquire(config(600, 1, 5_000)).await;
        let elapsed = started.elapsed();

        assert!(!next.timed_out);
        assert!(
            elapsed < Duration::from_millis(200),
            "unexpected wait: {elapsed:?}"
        );
    }

    #[test]
    fn disabled_config_is_ignored() {
        assert!(RateLimitConfig::from_meta(None).is_none());
        assert!(RateLimitConfig::from_meta(Some(&ProxyRateLimit {
            enabled: false,
            rpm: Some(20),
            burst: Some(2),
            max_wait_ms: Some(1_000),
        }))
        .is_none());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let config = RateLimitConfig::from_meta(Some(&ProxyRateLimit {
            enabled: true,
            rpm: Some(0),
            burst: Some(0),
            max_wait_ms: Some(u64::MAX),
        }))
        .expect("enabled config should resolve");

        assert_eq!(config.rpm, MIN_RPM);
        assert_eq!(config.burst, 1);
        assert_eq!(config.max_wait, Duration::from_millis(MAX_WAIT_MS));
    }
}
