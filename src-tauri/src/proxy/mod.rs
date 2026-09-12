//! 代理服务器模块
//!
//! 提供本地HTTP代理服务，支持多Provider故障转移和请求透传

pub mod body_filter;
pub mod cache_injector;
pub mod circuit_breaker;
pub(crate) mod content_encoding;
pub mod copilot_optimizer;
pub mod cooldown_manager;
pub mod error;
pub mod error_mapper;
pub(crate) mod failover_switch;
mod forwarder;
pub mod gemini_url;
pub mod guardrail_detector;
pub mod handler_config;
pub mod handler_context;
mod handlers;
pub mod http_client;
pub mod hyper_client;
pub(crate) mod json_canonical;
pub mod log_codes;
pub mod media_sanitizer;
pub mod model_alias_manager;
pub mod model_mapper;
pub mod provider_router;
pub mod providers;
pub mod response_processor;
pub(crate) mod server;
pub mod session;
pub(crate) mod sse;
pub mod streaming_refusal_detector;
pub(crate) mod switch_lock;
pub mod thinking_budget_rectifier;
pub mod thinking_optimizer;
pub mod thinking_rectifier;
pub(crate) mod tool_media;
pub(crate) mod types;
pub mod usage;
pub mod fingerprint_detector;

// 公开导出给外部使用（commands, services等模块需要）
#[allow(unused_imports)]
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerStats, CircuitState,
};
#[allow(unused_imports)]
pub use cooldown_manager::{CooldownManagerConfig, CooldownStats, ProviderCooldownManager, ProviderCooldownState, AutoRecoveryTask};
#[allow(unused_imports)]
pub use error::ProxyError;
#[allow(unused_imports)]
pub use fingerprint_detector::{AsyncFingerprintVerification, FingerprintDetector, FingerprintDetectorConfig, FingerprintVerificationResult, VerificationTask};
#[allow(unused_imports)]
pub use guardrail_detector::GuardrailDetector;
#[allow(unused_imports)]
pub use model_alias_manager::{ModelAliasManager, ProviderCapabilities, SessionCompatibilityManager, UnifiedAliasConfig};
#[allow(unused_imports)]
pub use provider_router::ProviderRouter;
#[allow(unused_imports)]
pub use session::{extract_session_id, SessionIdResult, SessionIdSource};
#[allow(unused_imports)]
pub use streaming_refusal_detector::{StreamingRefusalBuffer, StreamingRefusalDetector, StreamingRefusalResult};
#[allow(unused_imports)]
pub use types::{ProxyConfig, ProxyServerInfo, ProxyStatus, GuardrailConfig};

// 内部模块间共享（供子模块使用）
// 注意：这个导出用于模块内部，编译器可能警告未使用但实际被子模块使用
#[allow(unused_imports)]
pub(crate) use types::*;
