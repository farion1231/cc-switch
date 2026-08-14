//! Fallback Chain Architecture
//!
//! 将 oh-my-pi 的回退链路系统移植到 cc-switch。
//! 提供错误类型感知的 Provider 切换、Selector 级冷却和自动恢复机制。

pub mod backoff;
pub mod error_classifier;
pub mod fallback_chain;
pub mod fallback_state;
pub mod selector_suppression;
