//! 跨 Provider 故障转移的 anti-thundering-herd 延迟。
//!
//! ## 背景
//!
//! `RequestForwarder::forward_with_retry_inner` 在 P1 失败后会**立刻**轮到 P2。
//! 当一个客户端持有 N 个并发请求、且 P1 同时挂掉时，N 个请求会同步砸向 P2
//! —— 把 P2 也击穿。同理 P2 挂了，N 个请求再同步砸向 P3。
//!
//! ## 修复
//!
//! 在跨 Provider 切换的"下一次尝试"前插入一个**小固定延迟 + 抖动**：
//!
//! ```text
//! delay_ms = base_ms × (1 + jitter_pct × (2·r − 1))   // r ∈ [0, 1)
//! ```
//!
//! 默认 `base_ms = 60`、`jitter_pct = 0.30`，典型范围 `[42ms, 78ms]`。
//! 60ms 足够让上游抖动被吸收，又不至于明显拉长单次客户端延迟。
//!
//! ## 关闭
//!
//! `base_ms = 0` → `compute_failover_delay` 直接返回 0，保留旧行为。
//!
//! ## 范围限定
//!
//! **只**作用于"已经实际发起过请求"之后的跨 provider 切换：
//! - 熔断器拒绝（Open）导致的 `continue` **不**加延迟（避免全部熔断时排队浪费）；
//! - 客户端 4xx 错误直接返回客户端的路径 **不**加延迟；
//! - 同 provider 内的整流器重试（thinking signature / budget / media）**不**走本路径。
//!
//! ## 测试
//!
//! `compute_failover_delay` 是纯函数，`rand_unit` 由调用方注入，生产端用
//! 线程局部 xorshift；测试端用固定序列断言边界。

use std::cell::Cell;
use std::time::SystemTime;

/// 默认基础延迟（毫秒）。`0` 表示关闭该特性。
pub const FAILOVER_JITTER_BASE_MS: u32 = 60;

/// 默认抖动比例（± 30%）。
pub const FAILOVER_JITTER_PCT: f64 = 0.30;

/// 计算一次跨 provider 切换的延迟（毫秒）。
///
/// - `base_ms == 0` → 返回 0，关闭延迟。
/// - `jitter_pct < 0` 被钳到 0（即无 jitter 的固定延迟）。
/// - `jitter_pct > 1.0` 被钳到 1.0（即最多 ±100% 抖动，下界 0）。
/// - `rand_unit` 应满足 `0.0 ≤ rand_unit < 1.0`，越界值被钳到边界。
///
/// 公式（与 llm-gateway-go `applyBackoffJitter` 对齐）：
///
/// ```text
/// factor   = 1 + jitter_pct × (2·r − 1)
/// delay_ms = base_ms × factor
/// ```
///
/// `factor ∈ [1 − jitter_pct, 1 + jitter_pct)`，`delay_ms` 不会为负。
pub fn compute_failover_delay(base_ms: u32, jitter_pct: f64, rand_unit: f64) -> u32 {
    if base_ms == 0 {
        return 0;
    }
    let pct = jitter_pct.clamp(0.0, 1.0);
    let r = rand_unit.clamp(0.0, 1.0 - f64::EPSILON);
    let factor = 1.0 + pct * (2.0 * r - 1.0);
    let delay_ms = (base_ms as f64) * factor;
    if delay_ms < 0.0 {
        0
    } else if delay_ms > u32::MAX as f64 {
        u32::MAX
    } else {
        delay_ms as u32
    }
}

// ---------------------------------------------------------------------------
// 线程局部 PRNG（xorshift64*）——避免引入 `rand` 依赖
// ---------------------------------------------------------------------------

thread_local! {
    /// 每线程一个 xorshift64* 状态；种子基于线程 ID + 启动时间。
    static XORSHIFT_STATE: Cell<u64> = Cell::new(seed_from_time());
}

fn seed_from_time() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    // splitmix64：把 nanos 揉成非零 64bit
    let mut z = nanos ^ 0x9E37_79B9_7F4A_7C15 ^ (nanos >> 30);
    z = (z ^ (z >> 31)).wrapping_mul(0x62BD_5E3A_A6F1_8C2D);
    z ^= z >> 27;
    z = z.wrapping_mul(0x07A4_B8CE_5BF1_8B19);
    z ^= z >> 31;
    z = z.wrapping_mul(0x3BD1_53D7_C2AC_FA13);
    z ^= z >> 31;
    // xorshift64* 的非零状态要求
    z | 1
}

/// 线程局部 xorshift64* 的下一个 `u64`。
fn next_xorshift() -> u64 {
    XORSHIFT_STATE.with(|cell| {
        let mut x = cell.get();
        if x == 0 {
            x = seed_from_time();
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        cell.set(x);
        // xorshift64*: 乘 Golden Ratio 再取高 64 位（u64 * u64 = u128 的高 64 位）
        let wide = (x as u128).wrapping_mul(0x2545_F491_4F6C_DD1D);
        (wide >> 64) as u64
    })
}

/// 返回 `[0.0, 1.0)` 的伪随机浮点（线程局部 xorshift64*）。
///
/// **不**用于安全敏感场景；只用于 anti-thundering-herd 抖动。
#[inline]
pub fn thread_local_rand_unit() -> f64 {
    // 取 53 位精度（IEEE 754 double mantissa），确保分布足够均匀
    let bits = next_xorshift() >> 11;
    (bits as f64) / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_zero_disables_delay() {
        assert_eq!(compute_failover_delay(0, 0.30, 0.5), 0);
        assert_eq!(compute_failover_delay(0, 0.0, 0.0), 0);
    }

    #[test]
    fn zero_jitter_is_fixed_delay() {
        // jitter_pct = 0 → factor = 1，无论 r 多少
        assert_eq!(compute_failover_delay(100, 0.0, 0.0), 100);
        assert_eq!(compute_failover_delay(100, 0.0, 0.5), 100);
        assert_eq!(compute_failover_delay(100, 0.0, 0.9999), 100);
    }

    #[test]
    fn default_params_span_documented_range() {
        // base=60, jitter=0.30, factor ∈ [0.70, 1.30) → delay ∈ [42, 78)
        // 注意：右开区间，r 被钳到 1.0 - EPSILON，浮点乘积 60 * 1.30 = 78.0 是
        // 闭区间右端点；本函数保证 r 严格 < 1 → factor 严格 < 1.30 → 截断为 77。
        let lo = compute_failover_delay(60, 0.30, 0.0);
        let hi = compute_failover_delay(60, 0.30, 1.0 - f64::EPSILON);
        assert_eq!(lo, 42, "lower bound r=0 → factor=0.70 → 60×0.70=42");
        assert_eq!(hi, 77, "upper bound r→1⁻ → factor<1.30 → 60×<1.30=<77");
        // mid
        let mid = compute_failover_delay(60, 0.30, 0.5);
        assert_eq!(mid, 60, "r=0.5 → factor=1.0 → 60ms");
    }

    #[test]
    fn rand_unit_out_of_range_is_clamped() {
        // r = -0.5 应被钳到 0 → factor = 1 - 0.30 = 0.70
        let neg = compute_failover_delay(100, 0.30, -0.5);
        assert_eq!(neg, 70);
        // r = 1.5 应被钳到 1-eps → factor 严格 < 1.30 → 截断 129（不是 130）
        let over = compute_failover_delay(100, 0.30, 1.5);
        assert_eq!(over, 129);
    }

    #[test]
    fn jitter_pct_out_of_range_is_clamped() {
        // pct < 0 → 钳到 0 → 固定延迟
        assert_eq!(compute_failover_delay(100, -1.0, 0.5), 100);
        // pct > 1 → 钳到 1 → factor ∈ [0, 2)
        let lo = compute_failover_delay(100, 2.0, 0.0);
        let hi = compute_failover_delay(100, 2.0, 1.0 - f64::EPSILON);
        assert_eq!(lo, 0, "pct=1, r=0 → factor=0 → 0ms");
        // 100 × (<2.0) → 严格 < 200 → 截断 199
        assert_eq!(hi, 199, "pct=1, r→1⁻ → factor<2 → 100×<2.0=<199");
    }

    #[test]
    fn monotonic_in_rand_unit() {
        // factor 随 r 单调递增；因此 delay 也随 r 单调递增
        let mut prev = 0u32;
        for i in 0..=10 {
            let r = i as f64 / 10.0;
            let d = compute_failover_delay(1000, 0.50, r);
            assert!(
                d >= prev,
                "r={r}: delay {d} should be >= previous {prev}"
            );
            prev = d;
        }
    }

    #[test]
    fn thread_local_rand_unit_in_range() {
        for _ in 0..10_000 {
            let r = thread_local_rand_unit();
            assert!((0.0..1.0).contains(&r), "r={r} out of [0,1)");
        }
    }

    #[test]
    fn thread_local_rand_unit_is_not_constant() {
        // 不应是常量（除非种子失败，但 splitmix64 几乎不会连续给出同一值）
        let samples: Vec<f64> = (0..100).map(|_| thread_local_rand_unit()).collect();
        let unique: std::collections::HashSet<_> = samples
            .iter()
            .map(|v| v.to_bits())
            .collect();
        assert!(
            unique.len() > 50,
            "thread_local_rand_unit 输出退化：唯一值仅 {} 个",
            unique.len()
        );
    }
}