//! 供应商冷却管理
//!
//! 管理供应商在拒绝触发后的冷却状态和自动切回机制

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// 供应商冷却状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCooldownState {
    /// 供应商 ID
    pub provider_id: String,

    /// 冷却开始时间
    pub cooldown_start: DateTime<Utc>,

    /// 冷却持续时间（秒）
    pub cooldown_duration_secs: u64,

    /// 冷却原因（拒绝原因）
    pub reason: String,

    /// 是否在冷却中
    pub is_cooling: bool,

    /// 触发冷却的拒绝次数
    pub refusal_count: u32,
}

impl ProviderCooldownState {
    /// 创建新的冷却状态
    pub fn new(provider_id: String, reason: String, cooldown_duration_secs: u64) -> Self {
        Self {
            provider_id,
            cooldown_start: Utc::now(),
            cooldown_duration_secs,
            reason,
            is_cooling: true,
            refusal_count: 1,
        }
    }

    /// 检查冷却是否已结束
    pub fn is_cooldown_finished(&self) -> bool {
        if !self.is_cooling {
            return true;
        }

        let elapsed = Utc::now()
            .signed_duration_since(self.cooldown_start)
            .num_seconds();

        elapsed >= self.cooldown_duration_secs as i64
    }

    /// 获取剩余冷却时间（秒）
    pub fn remaining_seconds(&self) -> i64 {
        if !self.is_cooling {
            return 0;
        }

        let elapsed = Utc::now()
            .signed_duration_since(self.cooldown_start)
            .num_seconds();

        let remaining = self.cooldown_duration_secs as i64 - elapsed;
        remaining.max(0)
    }

    /// 增加拒绝计数
    pub fn increment_refusal_count(&mut self) {
        self.refusal_count += 1;
    }

    /// 重置冷却状态
    pub fn reset(&mut self) {
        self.is_cooling = false;
        self.refusal_count = 0;
    }
}

/// 冷却管理器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownManagerConfig {
    /// 默认冷却持续时间（秒）
    pub default_cooldown_secs: u64,

    /// 最大冷却持续时间（秒）（频繁拒绝时逐次增加）
    pub max_cooldown_secs: u64,

    /// 冷却递增因子（每次拒绝后冷却时间 × factor）
    pub cooldown_multiplier: f32,

    /// 是否启用自动切回
    pub auto_recovery_enabled: bool,

    /// 切回重试间隔（秒）
    pub recovery_retry_interval_secs: u64,
}

impl Default for CooldownManagerConfig {
    fn default() -> Self {
        Self {
            default_cooldown_secs: 1800, // 30 分钟
            max_cooldown_secs: 86400,     // 24 小时
            cooldown_multiplier: 2.0,
            auto_recovery_enabled: true,
            recovery_retry_interval_secs: 300, // 5 分钟
        }
    }
}

/// 供应商冷却管理器
pub struct ProviderCooldownManager {
    config: CooldownManagerConfig,
    /// 供应商冷却状态（provider_id -> state）
    cooldown_states: Arc<RwLock<HashMap<String, ProviderCooldownState>>>,
}

impl ProviderCooldownManager {
    /// 创建新的冷却管理器
    pub fn new(config: CooldownManagerConfig) -> Self {
        Self {
            config,
            cooldown_states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 触发供应商冷却
    pub async fn trigger_cooldown(&self, provider_id: &str, reason: &str) -> ProviderCooldownState {
        let mut states = self.cooldown_states.write().await;

        let cooldown_duration = if let Some(existing) = states.get(provider_id) {
            // 递增冷却时间（避免频繁拒绝的供应商立即重试）
            let new_duration = (existing.cooldown_duration_secs as f32 * self.config.cooldown_multiplier) as u64;
            new_duration.min(self.config.max_cooldown_secs)
        } else {
            self.config.default_cooldown_secs
        };

        let state = ProviderCooldownState::new(
            provider_id.to_string(),
            reason.to_string(),
            cooldown_duration,
        );

        states.insert(provider_id.to_string(), state.clone());

        log::info!(
            "[Cooldown] Provider '{}' entered cooldown for {}s: {}",
            provider_id,
            cooldown_duration,
            reason
        );

        state
    }

    /// 检查供应商是否在冷却中
    pub async fn is_cooling_down(&self, provider_id: &str) -> bool {
        let states = self.cooldown_states.read().await;

        if let Some(state) = states.get(provider_id) {
            if !state.is_cooldown_finished() {
                return true;
            }
        }

        false
    }

    /// 获取供应商冷却状态
    pub async fn get_cooldown_state(&self, provider_id: &str) -> Option<ProviderCooldownState> {
        let states = self.cooldown_states.read().await;
        states.get(provider_id).cloned()
    }

    /// 尝试恢复供应商（检查冷却是否已结束）
    pub async fn try_recover_provider(&self, provider_id: &str) -> bool {
        let mut states = self.cooldown_states.write().await;

        if let Some(state) = states.get_mut(provider_id) {
            if state.is_cooldown_finished() {
                log::info!(
                    "[Cooldown] Provider '{}' cooldown finished, ready for recovery",
                    provider_id
                );
                state.reset();
                return true;
            }
        }

        false
    }

    /// 强制结束供应商冷却
    pub async fn end_cooldown(&self, provider_id: &str) {
        let mut states = self.cooldown_states.write().await;

        if let Some(state) = states.get_mut(provider_id) {
            log::info!(
                "[Cooldown] Provider '{}' cooldown manually ended",
                provider_id
            );
            state.reset();
        }
    }

    /// 获取所有冷却中的供应商
    pub async fn get_cooling_providers(&self) -> Vec<String> {
        let states = self.cooldown_states.read().await;

        states
            .iter()
            .filter(|(_, state)| state.is_cooling && !state.is_cooldown_finished())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 获取冷却统计信息
    pub async fn get_cooldown_stats(&self) -> CooldownStats {
        let states = self.cooldown_states.read().await;

        let total_providers = states.len();
        let cooling_providers = states
            .iter()
            .filter(|(_, state)| state.is_cooling && !state.is_cooldown_finished())
            .count();

        let total_refusals = states.values().map(|s| s.refusal_count).sum();

        CooldownStats {
            total_providers,
            cooling_providers,
            total_refusals,
        }
    }

    /// 清理已结束的冷却状态（定期调用）
    pub async fn cleanup_finished_cooldowns(&self) {
        let mut states = self.cooldown_states.write().await;

        states.retain(|_, state| {
            if state.is_cooldown_finished() {
                log::debug!(
                    "[Cooldown] Cleaning up finished cooldown for provider '{}'",
                    state.provider_id
                );
                false
            } else {
                true
            }
        });
    }
}

/// 冷却统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownStats {
    /// 总供应商数
    pub total_providers: usize,

    /// 当前冷却中的供应商数
    pub cooling_providers: usize,

    /// 总拒绝次数
    pub total_refusals: u32,
}

/// 自动恢复任务
///
/// 定期尝试恢复冷却中的供应商
pub struct AutoRecoveryTask {
    manager: Arc<ProviderCooldownManager>,
}

impl AutoRecoveryTask {
    /// 创建新的自动恢复任务
    pub fn new(manager: Arc<ProviderCooldownManager>) -> Self {
        Self { manager }
    }

    /// 执行恢复检查
    pub async fn run_recovery(&self) -> Vec<String> {
        let mut recovered = Vec::new();

        // 获取所有冷却中的供应商
        let cooling_providers = self.manager.get_cooling_providers().await;

        for provider_id in cooling_providers {
            if self.manager.try_recover_provider(&provider_id).await {
                recovered.push(provider_id);
            }
        }

        if !recovered.is_empty() {
            log::info!(
                "[Cooldown] Auto-recovered {} providers: {:?}",
                recovered.len(),
                recovered
            );
        }

        recovered
    }

    /// 启动定期恢复任务
    pub async fn start_periodic_recovery(
        &self,
    interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let manager = self.manager.clone();
        let interval = Duration::from_secs(interval_secs);

        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;

                let task = AutoRecoveryTask::new(manager.clone());
                task.run_recovery().await;

                // 清理已结束的冷却状态
                manager.cleanup_finished_cooldowns().await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cooldown_state() {
        let state = ProviderCooldownState::new(
            "test_provider".to_string(),
            "Test refusal".to_string(),
            60,
        );

        assert!(state.is_cooling);
        assert_eq!(state.refusal_count, 1);
    }

    #[test]
    fn test_cooldown_finished() {
        let mut state = ProviderCooldownState::new(
            "test_provider".to_string(),
            "Test refusal".to_string(),
            0, // 立即结束
        );

        assert!(state.is_cooldown_finished());
    }

    #[test]
    fn test_refusal_count_increment() {
        let mut state = ProviderCooldownState::new(
            "test_provider".to_string(),
            "Test refusal".to_string(),
            60,
        );

        state.increment_refusal_count();
        assert_eq!(state.refusal_count, 2);
    }

    #[tokio::test]
    async fn test_cooldown_manager() {
        let config = CooldownManagerConfig {
            default_cooldown_secs: 60,
            ..Default::default()
        };

        let manager = ProviderCooldownManager::new(config);

        // 触发冷却
        manager.trigger_cooldown("provider1", "Refusal detected").await;

        // 检查冷却状态
        assert!(manager.is_cooling_down("provider1").await);

        // 获取冷却状态
        let state = manager.get_cooldown_state("provider1").await;
        assert!(state.is_some());
        assert!(state.unwrap().is_cooling);

        // 强制结束冷却
        manager.end_cooldown("provider1").await;
        assert!(!manager.is_cooling_down("provider1").await);
    }

    #[tokio::test]
    async fn test_cooldown_progressive_duration() {
        let config = CooldownManagerConfig {
            default_cooldown_secs: 60,
            cooldown_multiplier: 2.0,
            ..Default::default()
        };

        let manager = ProviderCooldownManager::new(config);

        // 第一次触发
        manager.trigger_cooldown("provider1", "Refusal 1").await;
        let state1 = manager.get_cooldown_state("provider1").await.unwrap();
        assert_eq!(state1.cooldown_duration_secs, 60);

        // 结束第一次冷却
        manager.end_cooldown("provider1").await;

        // 第二次触发（应该翻倍）
        manager.trigger_cooldown("provider1", "Refusal 2").await;
        let state2 = manager.get_cooldown_state("provider1").await.unwrap();
        assert_eq!(state2.cooldown_duration_secs, 120);
    }
}
