//! Selector 级抑制（冷却）管理器
//!
//! 替换原有的 `cooldown_manager.rs`。
//! 按 Selector 标识精确冷却，支持渐进式持续时间和自动清理。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 抑制配置
#[derive(Debug, Clone)]
pub struct SuppressionConfig {
    /// 基础抑制时长
    pub base_duration: Duration,
    /// 最大抑制时长
    pub max_duration: Duration,
    /// 渐进式抑制乘数（2.0 = 每次翻倍）
    pub backoff_multiplier: f64,
}

impl Default for SuppressionConfig {
    fn default() -> Self {
        Self {
            base_duration: Duration::from_secs(30 * 60), // 30 分钟
            max_duration: Duration::from_secs(24 * 60 * 60), // 24 小时
            backoff_multiplier: 2.0,
        }
    }
}

/// 抑制条目
#[derive(Debug, Clone)]
struct SuppressionEntry {
    /// 抑制过期时间
    until: Instant,
    /// 连续抑制次数
    consecutive_count: u32,
}

/// Selector 抑制管理器。
///
/// 当 Selector 连续失败时，将其抑制一段时间。
/// 被抑制的 Selector 不会被任何回退链路选中。
pub struct SelectorSuppression {
    entries: Arc<RwLock<HashMap<String, SuppressionEntry>>>,
}

impl SelectorSuppression {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 抑制一个 Selector。
    ///
    /// `consecutive_count` 用于实现渐进式抑制：
    /// - 第 1 次：base_duration
    /// - 第 2 次：base_duration * multiplier
    /// - 以此类推，最多到 max_duration
    pub async fn suppress(
        &self,
        selector_identity: &str,
        consecutive_count: u32,
        config: &SuppressionConfig,
    ) {
        let duration = compute_progressive_duration(consecutive_count, config);
        let until = Instant::now() + duration;
        let mut entries = self.entries.write().await;

        entries.entry(selector_identity.to_string())
            .and_modify(|entry| {
                entry.until = until;
                entry.consecutive_count = consecutive_count;
            })
            .or_insert(SuppressionEntry {
                until,
                consecutive_count,
            });

        log::info!(
            "[Suppression] Selector '{}' suppressed for {:?} (count={})",
            selector_identity,
            duration,
            consecutive_count
        );
    }

    /// 检查 Selector 是否被抑制。
    pub async fn is_suppressed(&self, selector_identity: &str) -> bool {
        let entries = self.entries.read().await;
        match entries.get(selector_identity) {
            Some(entry) => Instant::now() < entry.until,
            None => false,
        }
    }

    /// 获取 Selector 的抑制过期时间（如果正在被抑制）。
    pub async fn suppression_until(&self, selector_identity: &str) -> Option<Instant> {
        let entries = self.entries.read().await;
        entries.get(selector_identity)
            .filter(|entry| Instant::now() < entry.until)
            .map(|entry| entry.until)
    }

    /// 立即清除 Selector 的抑制状态。
    pub async fn clear(&self, selector_identity: &str) {
        let mut entries = self.entries.write().await;
        if entries.remove(selector_identity).is_some() {
            log::info!("[Suppression] Selector '{}' unsuppressed", selector_identity);
        }
    }

    /// 清理过期的抑制条目。
    ///
    /// 应当定期调用（如每 60 秒）。
    pub async fn cleanup_expired(&self) -> usize {
        let mut entries = self.entries.write().await;
        let before = entries.len();
        let now = Instant::now();
        entries.retain(|_, entry| now < entry.until);
        let removed = before - entries.len();
        if removed > 0 {
            log::debug!("[Suppression] Cleaned up {} expired suppression(s)", removed);
        }
        removed
    }

    /// 获取当前抑制的条目数量。
    pub async fn entry_count(&self) -> usize {
        self.entries.read().await.len()
    }
}

/// 计算渐进式抑制时长。
fn compute_progressive_duration(consecutive_count: u32, config: &SuppressionConfig) -> Duration {
    if consecutive_count <= 1 {
        return config.base_duration;
    }

    let multiplier = config.backoff_multiplier.powi(consecutive_count as i32 - 1);
    let duration_secs = (config.base_duration.as_secs() as f64 * multiplier) as u64;
    Duration::from_secs(duration_secs.min(config.max_duration.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progressive_duration() {
        let config = SuppressionConfig {
            base_duration: Duration::from_secs(60),
            max_duration: Duration::from_secs(600),
            backoff_multiplier: 2.0,
        };

        assert_eq!(compute_progressive_duration(1, &config).as_secs(), 60);
        assert_eq!(compute_progressive_duration(2, &config).as_secs(), 120);
        assert_eq!(compute_progressive_duration(3, &config).as_secs(), 240);
        assert_eq!(compute_progressive_duration(5, &config).as_secs(), 600); // capped
    }

    #[tokio::test]
    async fn test_suppress_and_check() {
        let suppression = SelectorSuppression::new();
        let config = SuppressionConfig::default();

        suppression.suppress("test:model", 1, &config).await;
        assert!(suppression.is_suppressed("test:model").await);
        assert!(!suppression.is_suppressed("other:model").await);
    }

    #[tokio::test]
    async fn test_clear_suppression() {
        let suppression = SelectorSuppression::new();
        let config = SuppressionConfig::default();

        suppression.suppress("test:model", 1, &config).await;
        assert!(suppression.is_suppressed("test:model").await);

        suppression.clear("test:model").await;
        assert!(!suppression.is_suppressed("test:model").await);
    }

    #[tokio::test]
    async fn test_cleanup_expired_with_short_duration() {
        let suppression = SelectorSuppression::new();
        let config = SuppressionConfig {
            base_duration: Duration::from_millis(1),
            max_duration: Duration::from_secs(600),
            backoff_multiplier: 2.0,
        };

        suppression.suppress("test:model", 1, &config).await;

        // 等 10ms 让抑制过期
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(!suppression.is_suppressed("test:model").await);
        let removed = suppression.cleanup_expired().await;
        assert_eq!(removed, 1);
        assert_eq!(suppression.entry_count().await, 0);
    }

    #[tokio::test]
    async fn test_suppression_until() {
        let suppression = SelectorSuppression::new();
        let config = SuppressionConfig::default();

        suppression.suppress("test:model", 1, &config).await;
        let until = suppression.suppression_until("test:model").await;
        assert!(until.is_some());
    }

    #[tokio::test]
    async fn test_concurrent_suppression_updates_count() {
        let suppression = SelectorSuppression::new();
        let config = SuppressionConfig::default();

        suppression.suppress("test:model", 1, &config).await;
        // 第二次抑制应该更新计数
        suppression.suppress("test:model", 2, &config).await;

        let entries = suppression.entries.read().await;
        assert_eq!(entries.get("test:model").unwrap().consecutive_count, 2);
    }
}
