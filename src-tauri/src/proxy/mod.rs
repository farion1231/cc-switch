//! 代理服务器模块
//!
//! 提供本地HTTP代理服务，支持多Provider故障转移和请求透传

pub mod body_filter;
mod browser_guard;
pub mod cache_injector;
pub mod circuit_breaker;
mod classifier_guard;
mod codex_models_passthrough;
pub(crate) mod codex_quota_protection;
pub(crate) mod content_encoding;
pub mod copilot_optimizer;
pub mod error;
pub mod error_mapper;
pub(crate) mod failover_switch;
pub(crate) mod forwarder;
pub mod gemini_url;
pub mod handler_config;
pub mod handler_context;
mod handlers;
pub mod http_client;
pub mod hyper_client;
pub(crate) mod json_canonical;
pub mod log_codes;
pub(crate) mod loopback_probe;
pub mod media_sanitizer;
pub mod model_mapper;
pub mod opaque_state_rectifier;
pub mod privacy_filter;
pub mod provider_router;
pub mod providers;
pub mod response_processor;
pub(crate) mod server;
pub mod session;
pub(crate) mod sse;
pub(crate) mod switch_lock;
pub mod thinking_budget_rectifier;
pub mod thinking_optimizer;
pub mod thinking_rectifier;
pub(crate) mod tool_media;
pub(crate) mod types;
pub(crate) mod upstream_guard;
pub(crate) mod upstream_pool;
pub mod usage;

#[cfg(test)]
mod tests_issue_7271;

pub use circuit_breaker::CircuitBreakerConfig;
pub use error::ProxyError;
pub use session::extract_session_id;
