//! Provider Adapters Module
//!
//! 供应商适配器模块，提供统一的接口抽象不同上游供应商的处理逻辑。
//!
//! ## 模块结构
//! - `adapter`: 定义 `ProviderAdapter` trait
//! - `auth`: 认证类型和策略
//! - `claude`: Claude (Anthropic) 适配器
//! - `codex`: Codex (OpenAI) 适配器
//! - `gemini`: Gemini (Google) 适配器
//! - `models`: API 数据模型
//! - `transform`: 格式转换

mod adapter;
mod auth;
mod claude;
mod claude_reasoning;
mod codex;
pub(crate) mod codex_chat_common;
pub mod codex_chat_history;
mod codex_chat_tool_history;
pub(crate) mod codex_compaction;
pub mod codex_oauth_auth;
pub(crate) mod codex_responses_arguments;
pub(crate) mod codex_responses_sse;
pub mod copilot_auth;
pub mod copilot_model_map;
pub(crate) mod copilot_responses;
mod gemini;
pub(crate) mod gemini_schema;
pub mod gemini_shadow;
pub(crate) mod inline_think;
pub mod models;
pub(crate) mod reasoning_bridge;
pub(crate) mod responses_late_arguments;
pub mod streaming;
pub mod streaming_codex_anthropic;
pub mod streaming_codex_chat;
pub mod streaming_gemini;
pub mod streaming_responses;
pub mod transform;
pub mod transform_codex_anthropic;
pub mod transform_codex_chat;
pub mod transform_codex_chat_moonshot_schema;
pub mod transform_codex_responses_kimi_history;
pub mod transform_codex_responses_namespace;
pub mod transform_codex_responses_xai_sanitize;
pub mod transform_gemini;
pub mod transform_responses;
pub mod xai_oauth_auth;

use crate::app_config::AppType;

pub const CHATGPT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const XAI_API_BASE_URL: &str = "https://api.x.ai/v1";

// 公开导出
pub use adapter::ProviderAdapter;
pub use auth::{AuthInfo, AuthStrategy};
pub use claude::{
    claude_api_format_needs_transform, get_claude_api_format,
    normalize_anthropic_messages_for_provider, transform_claude_request_for_api_format,
    ClaudeAdapter,
};
pub use codex::CodexAdapter;
pub use codex::{
    apply_codex_chat_upstream_model, apply_codex_upstream_model, codex_provider_upstream_model,
    codex_stack_upstream_rejects_web_search, inject_codex_chat_prompt_cache_key,
    is_codex_official_provider, is_codex_responses_endpoint,
    provider_needs_responses_late_arguments_repair, provider_needs_responses_namespace_flatten,
    resolve_codex_catalog_tool_profile, resolve_codex_chat_reasoning_config,
    should_convert_codex_responses_to_anthropic, should_convert_codex_responses_to_chat,
    strip_codex_hosted_web_search, strip_codex_openai_only_request_fields,
};
pub use gemini::GeminiAdapter;

/// 供应商类型枚举
///
/// 区分不同供应商的具体实现方式，决定认证和请求处理逻辑。
/// 比 AppType 更细粒度，支持同一 AppType 下的多种变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    /// Anthropic 官方 API (x-api-key + anthropic-version)
    Claude,
    /// Claude 中转服务 (仅 Bearer 认证，无 x-api-key)
    ClaudeAuth,
    /// Google Gemini API (x-goog-api-key)
    Gemini,
    /// Google Gemini CLI (OAuth Bearer)
    GeminiCli,
    /// OpenRouter（已支持 Claude Code 兼容接口，默认透传；保留旧转换逻辑备用）
    OpenRouter,
    /// GitHub Copilot (OAuth + Copilot Token，需要 Anthropic ↔ OpenAI 转换)
    GitHubCopilot,
    /// OpenAI Codex (ChatGPT Plus/Pro OAuth，需要 Anthropic ↔ Responses API 转换)
    CodexOAuth,
    /// xAI Grok OAuth（需要 Anthropic ↔ Responses API 转换）
    XaiOAuth,
}

/// 根据 AppType 获取对应的适配器
pub fn get_adapter(app_type: &AppType) -> Option<Box<dyn ProviderAdapter>> {
    Some(match app_type {
        AppType::Claude | AppType::ClaudeDesktop => Box::new(ClaudeAdapter::new()),
        AppType::Codex => Box::new(CodexAdapter::new()),
        AppType::Gemini => Box::new(GeminiAdapter::new()),
        AppType::GrokBuild => Box::new(CodexAdapter::new()),
        AppType::OpenCode | AppType::OpenClaw | AppType::Hermes => Box::new(CodexAdapter::new()),
        AppType::Pi
        | AppType::Mcode
        | AppType::Kimi
        | AppType::Dsh
        | AppType::Agy
        | AppType::Omp
        | AppType::Copilot
        | AppType::WorkBuddy => return None,
    })
}
