//! 活动回退状态跟踪
//!
//! 在单次请求/重试周期内跟踪回退链路的当前位置和状态。

use crate::fallback::fallback_chain::FallbackSelector;
use crate::fallback::error_classifier::ErrorFlags;
use std::time::Instant;

/// 单次请求的回退链路线状态。
///
/// 在请求处理期间跟踪：
/// - 当前在链路中的位置
/// - 原始主 Provider（用于自动恢复）
/// - 是否已固定（pinned）
#[derive(Debug, Clone)]
pub struct ActiveFallbackState {
    /// 链路Key
    pub chain_key: String,
    /// 当前候选索引
    pub current_candidate_index: usize,
    /// 原始主 Selector（回退前使用的）
    pub original_selector: FallbackSelector,
    /// 原始主模型ID
    pub original_model_id: String,
    /// 是否已固定（永不自动切回）
    pub pinned: bool,
    /// 连续失败次数
    pub consecutive_failures: u32,
    /// 最近一次错误的标志
    pub last_error_flags: ErrorFlags,
    /// 状态创建时间
    pub started_at: Instant,
    /// 总尝试次数
    pub total_attempts: u32,
}

impl ActiveFallbackState {
    /// 创建新的回退状态。
    pub fn new(
        chain_key: String,
        original_selector: FallbackSelector,
    ) -> Self {
        Self {
            chain_key,
            current_candidate_index: 0,
            original_model_id: original_selector.id.clone(),
            original_selector,
            pinned: false,
            consecutive_failures: 0,
            last_error_flags: ErrorFlags::empty(),
            started_at: Instant::now(),
            total_attempts: 0,
        }
    }

    /// 推进到下一个候选。
    pub fn advance(&mut self, error_flags: ErrorFlags) {
        self.current_candidate_index += 1;
        self.total_attempts += 1;
        self.last_error_flags = error_flags;
        self.consecutive_failures += 1;
    }

    /// 固定状态 — 永不自动切回主模型。
    pub fn pin(&mut self) {
        self.pinned = true;
    }

    /// 记录成功。
    pub fn record_success(&mut self) {
        self.total_attempts += 1;
        self.consecutive_failures = 0;
    }

    /// 已完成的回退步数。
    pub fn steps_taken(&self) -> usize {
        self.current_candidate_index
    }

    /// 检查是否需要切回主模型。
    ///
    /// 条件：
    /// - 未被固定
    /// - 至少执行过一次回退（index > 0）
    /// - 不是持续性错误（ContentBlocked/ClassifierRefusal 固定后不再恢复）
    pub fn should_attempt_revert(&self) -> bool {
        !self.pinned && self.current_candidate_index > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_selector(provider: &str, id: &str) -> FallbackSelector {
        FallbackSelector {
            raw: format!("{}/{}", provider, id),
            provider: provider.to_string(),
            id: id.to_string(),
        }
    }

    #[test]
    fn test_new_state() {
        let state = ActiveFallbackState::new(
            "default".to_string(),
            make_selector("anthropic", "claude-sonnet"),
        );
        assert_eq!(state.chain_key, "default");
        assert_eq!(state.current_candidate_index, 0);
        assert!(!state.pinned);
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.total_attempts, 0);
    }

    #[test]
    fn test_advance() {
        let mut state = ActiveFallbackState::new(
            "default".to_string(),
            make_selector("anthropic", "claude-sonnet"),
        );
        state.advance(ErrorFlags::SERVER_ERROR);
        assert_eq!(state.current_candidate_index, 1);
        assert_eq!(state.total_attempts, 1);
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_error_flags, ErrorFlags::SERVER_ERROR);
    }

    #[test]
    fn test_pin() {
        let mut state = ActiveFallbackState::new(
            "default".to_string(),
            make_selector("anthropic", "claude-sonnet"),
        );
        state.advance(ErrorFlags::CLASSIFIER_REFUSAL);
        state.pin();
        assert!(state.pinned);
        assert!(!state.should_attempt_revert());
    }

    #[test]
    fn test_record_success_resets_failures() {
        let mut state = ActiveFallbackState::new(
            "default".to_string(),
            make_selector("anthropic", "claude-sonnet"),
        );
        state.advance(ErrorFlags::NETWORK_ERROR);
        state.advance(ErrorFlags::NETWORK_ERROR);
        assert_eq!(state.consecutive_failures, 2);

        state.record_success();
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.total_attempts, 3);
    }

    #[test]
    fn test_should_attempt_revert_normal() {
        let mut state = ActiveFallbackState::new(
            "default".to_string(),
            make_selector("anthropic", "claude-sonnet"),
        );
        assert!(!state.should_attempt_revert()); // 还在第 0 位

        state.advance(ErrorFlags::SERVER_ERROR);
        assert!(state.should_attempt_revert()); // 已回退，可以尝试恢复
    }

    #[test]
    fn test_should_not_revert_when_pinned() {
        let mut state = ActiveFallbackState::new(
            "default".to_string(),
            make_selector("anthropic", "claude-sonnet"),
        );
        state.advance(ErrorFlags::CLASSIFIER_REFUSAL);
        state.pin();
        assert!(!state.should_attempt_revert());
    }
}
