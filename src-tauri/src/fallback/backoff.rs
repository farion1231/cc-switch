//! 带抖动的指数退避引擎
//!
//! 实现 oh-my-pi 的退避算法：
//! - 指数增长：`base * 2^(attempt-1)`
//! - 上限 8 秒
//! - 25% 向下抖动（Uniform[0.75, 1.0]）
//! - 支持 Retry-After 头
//! - Fail-fast 上限防止无限等待

use rand::Rng;
use std::time::Duration;

/// 退避计算引擎。
///
/// 公式：`min(base * 2^(attempt-1), max_delay) * Uniform(0.75, 1.0)`
pub struct BackoffEngine {
    /// 基础延迟（毫秒），默认 500ms
    pub base_delay_ms: u64,
    /// 最大延迟（毫秒），默认 8000ms
    pub max_delay_ms: u64,
}

impl Default for BackoffEngine {
    fn default() -> Self {
        Self {
            base_delay_ms: 500,
            max_delay_ms: 8_000,
        }
    }
}

impl BackoffEngine {
    /// 创建新的退避引擎。
    pub fn new(base_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            base_delay_ms: base_delay_ms.max(10),
            max_delay_ms: max_delay_ms.max(100),
        }
    }

    /// 计算指定尝试次数的退避延迟。
    ///
    /// `attempt` 为 1-based（第 1 次尝试返回 0，无需等待）。
    pub fn compute(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return Duration::ZERO;
        }

        let exponential = self
            .base_delay_ms
            .saturating_mul(2u64.saturating_pow(attempt - 1));
        let capped = exponential.min(self.max_delay_ms);

        // 25% 向下抖动：Uniform[0.75, 1.0]
        let jitter = {
            let mut rng = rand::thread_rng();
            rng.gen_range(0.75_f64..1.0)
        };

        let ms = (capped as f64 * jitter) as u64;
        Duration::from_millis(ms)
    }

    /// 计算退避延迟，同时尊重服务端返回的 Retry-After。
    ///
    /// 取指数退避和服务端建议值中的较大者。
    /// 服务端值会被钳制到 `max_delay_ms` 范围内。
    pub fn compute_with_retry_after(
        &self,
        attempt: u32,
        retry_after_seconds: Option<u64>,
    ) -> Duration {
        let exponential = self.compute(attempt);

        if let Some(server_seconds) = retry_after_seconds {
            if server_seconds > 0 {
                let server_ms = (server_seconds * 1000).min(self.max_delay_ms);
                let server_dur = Duration::from_millis(server_ms);
                return exponential.max(server_dur);
            }
        }

        exponential
    }

    /// 检查累积延迟是否超过 fail-fast 上限。
    ///
    /// `cumulative_delay` — 本回退周期内已累积的等待时间
    /// `max_cap` — fail-fast 上限（如 5 分钟）
    ///
    /// 返回 `true` 表示已超过上限，应停止重试。
    pub fn would_exceed_cap(
        &self,
        cumulative_delay: Duration,
        max_cap: Duration,
    ) -> bool {
        cumulative_delay >= max_cap
    }

    /// 计算下次等待后的累积延迟。
    pub fn cumulative_after_next(
        &self,
        current_cumulative: Duration,
        attempt: u32,
        retry_after_seconds: Option<u64>,
    ) -> Duration {
        current_cumulative + self.compute_with_retry_after(attempt, retry_after_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_attempt_no_delay() {
        let engine = BackoffEngine::default();
        assert_eq!(engine.compute(1), Duration::ZERO);
        assert_eq!(engine.compute(0), Duration::ZERO);
    }

    #[test]
    fn test_exponential_growth() {
        let engine = BackoffEngine::new(500, 10_000);

        let d2 = engine.compute(2).as_millis() as u64;
        let d3 = engine.compute(3).as_millis() as u64;
        let d4 = engine.compute(4).as_millis() as u64;

        // d2 ≈ 500 * 2^1 * jitter = 1000 * [0.75, 1.0] → [750, 1000]
        assert!(d2 >= 375 && d2 <= 1000, "d2={d2}");
        // d3 ≈ 500 * 2^2 * jitter = 2000 * [0.75, 1.0] → [1500, 2000]
        assert!(d3 >= 750 && d3 <= 2000, "d3={d3}");
        // d4 ≈ 500 * 2^3 * jitter = 4000 * [0.75, 1.0] → [3000, 4000]
        assert!(d4 >= 1500 && d4 <= 4000, "d4={d4}");
    }

    #[test]
    fn test_max_delay_cap() {
        let engine = BackoffEngine::new(500, 1_000);

        // 高尝试次数应对应被上限钳制
        let d10 = engine.compute(10).as_millis() as u64;
        // 500 * 2^9 = 256000，被钳制到 1000ms
        assert!(d10 <= 1000, "d10={d10}");
    }

    #[test]
    fn test_retry_after_override() {
        let engine = BackoffEngine::new(500, 8_000);

        // 服务端要求 5 秒 = 5000ms
        let delay = engine.compute_with_retry_after(2, Some(5));
        // 指数退避 ≈ 1000ms，服务端 5000ms → 取 5000ms * jitter → [3750, 5000]
        assert!(delay.as_millis() >= 3500, "delay={}", delay.as_millis());
    }

    #[test]
    fn test_retry_after_capped() {
        let engine = BackoffEngine::new(500, 5_000);

        // 服务端要求 10 秒，但 max_delay 只有 5 秒
        let delay = engine.compute_with_retry_after(2, Some(10));
        assert!(delay.as_millis() <= 5_000, "delay={}", delay.as_millis());
    }

    #[test]
    fn test_would_exceed_cap() {
        let engine = BackoffEngine::default();
        let cap = Duration::from_secs(300); // 5 分钟

        assert!(!engine.would_exceed_cap(Duration::from_secs(60), cap));
        assert!(engine.would_exceed_cap(Duration::from_secs(300), cap));
        assert!(engine.would_exceed_cap(Duration::from_secs(500), cap));
    }

    #[test]
    fn test_cumulative_after_next() {
        let engine = BackoffEngine::new(500, 8_000);
        let current = Duration::from_millis(2000);

        let next = engine.cumulative_after_next(current, 4, None);
        // next = 2000 + compute(4) = 2000 + [3000, 4000] = [5000, 6000]
        assert!(next.as_millis() >= 5000 && next.as_millis() <= 6000,
            "next={}", next.as_millis());
    }
}
