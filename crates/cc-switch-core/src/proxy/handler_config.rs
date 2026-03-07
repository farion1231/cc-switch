//! Handler 配置模块
//!
//! 定义各 API 处理器的配置结构和使用量解析器

use crate::app_config::AppType;
use crate::proxy::usage::parser::TokenUsage;
use serde_json::Value;

/// 使用量解析器类型别名
pub type StreamUsageParser = fn(&[Value]) -> Option<TokenUsage>;
pub type ResponseUsageParser = fn(&Value) -> Option<TokenUsage>;

/// 模型提取器类型别名
/// 参数: (流式事件列表, 请求中的模型名称) -> 最终使用的模型名称
pub type StreamModelExtractor = fn(&[Value], &str) -> String;

/// 各 API 的使用量解析配置
#[derive(Clone, Copy)]
pub struct UsageParserConfig {
    /// 流式响应解析器
    pub stream_parser: StreamUsageParser,
    /// 非流式响应解析器
    pub response_parser: ResponseUsageParser,
    /// 流式响应中的模型提取器
    pub model_extractor: StreamModelExtractor,
    /// 应用类型字符串（用于日志记录）
    pub app_type_str: &'static str,
}

// ============================================================================
// 模型提取器实现
// ============================================================================

/// Claude 流式响应模型提取（优先使用 usage.model）
fn claude_model_extractor(events: &[Value], request_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_claude_stream_events(events) {
        if let Some(model) = usage.model {
            return model;
        }
    }
    request_model.to_string()
}

/// OpenAI Chat Completions 流式响应模型提取（优先使用 usage.model）
fn openai_model_extractor(events: &[Value], request_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_openai_stream_events(events) {
        if let Some(model) = usage.model {
            return model;
        }
    }
    // 回退：从事件中直接提取
    events
        .iter()
        .find_map(|e| e.get("model")?.as_str())
        .unwrap_or(request_model)
        .to_string()
}

/// Codex 智能流式响应模型提取（自动检测格式）
fn codex_auto_model_extractor(events: &[Value], request_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_codex_stream_events_auto(events) {
        if let Some(model) = usage.model {
            return model;
        }
    }
    // 回退：从 response.completed 事件中提取
    events
        .iter()
        .find_map(|e| {
            if e.get("type")?.as_str()? == "response.completed" {
                e.get("response")?.get("model")?.as_str()
            } else {
                None
            }
        })
        .or_else(|| {
            // 再回退：从 OpenAI 格式事件中提取
            events.iter().find_map(|e| e.get("model")?.as_str())
        })
        .unwrap_or(request_model)
        .to_string()
}

/// Gemini 流式响应模型提取（优先使用 usage.model）
fn gemini_model_extractor(events: &[Value], request_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_gemini_stream_chunks(events) {
        if let Some(model) = usage.model {
            return model;
        }
    }
    request_model.to_string()
}

// ============================================================================
// 预定义配置
// ============================================================================

/// Claude API 解析配置
pub const CLAUDE_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_claude_stream_events,
    response_parser: TokenUsage::from_claude_response,
    model_extractor: claude_model_extractor,
    app_type_str: "claude",
};

/// OpenAI Chat Completions API 解析配置（用于 Codex /v1/chat/completions）
pub const OPENAI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_openai_stream_events,
    response_parser: TokenUsage::from_openai_response,
    model_extractor: openai_model_extractor,
    app_type_str: "codex",
};

/// Codex 智能解析配置（自动检测 OpenAI 或 Codex 格式）
pub const CODEX_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_codex_stream_events_auto,
    response_parser: TokenUsage::from_codex_response_auto,
    model_extractor: codex_auto_model_extractor,
    app_type_str: "codex",
};

/// Gemini API 解析配置
pub const GEMINI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_gemini_stream_chunks,
    response_parser: TokenUsage::from_gemini_response,
    model_extractor: gemini_model_extractor,
    app_type_str: "gemini",
};

// ============================================================================
// Handler 配置（预留，用于进一步简化）
// ============================================================================

/// Handler 基础配置
///
/// 预留结构，可用于进一步统一各 handler 的配置
#[allow(dead_code)]
#[derive(Clone)]
pub struct HandlerConfig {
    /// 应用类型
    pub app_type: AppType,
    /// 日志标签
    pub tag: &'static str,
    /// 应用类型字符串
    pub app_type_str: &'static str,
    /// 使用量解析配置
    pub parser_config: &'static UsageParserConfig,
}

/// Claude Handler 配置
#[allow(dead_code)]
pub const CLAUDE_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Claude,
    tag: "Claude",
    app_type_str: "claude",
    parser_config: &CLAUDE_PARSER_CONFIG,
};

/// Codex Chat Completions Handler 配置
#[allow(dead_code)]
pub const CODEX_CHAT_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Codex,
    tag: "Codex",
    app_type_str: "codex",
    parser_config: &OPENAI_PARSER_CONFIG,
};

/// Codex Responses Handler 配置
#[allow(dead_code)]
pub const CODEX_RESPONSES_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Codex,
    tag: "Codex",
    app_type_str: "codex",
    parser_config: &CODEX_PARSER_CONFIG,
};

/// Gemini Handler 配置
#[allow(dead_code)]
pub const GEMINI_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Gemini,
    tag: "Gemini",
    app_type_str: "gemini",
    parser_config: &GEMINI_PARSER_CONFIG,
};
