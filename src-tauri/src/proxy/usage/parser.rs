//! Response Parser - 从 API 响应中提取 token 使用量
//!
//! 支持多种 API 格式：
//! - Claude API (非流式和流式)
//! - OpenRouter (OpenAI 格式)
//! - Codex API (非流式和流式)
//! - Gemini API (非流式和流式)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Token 使用量统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    /// 从响应中提取的实际模型名称（如果可用）
    pub model: Option<String>,
}

/// API 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ApiType {
    Claude,
    OpenRouter,
    Codex,
    Gemini,
}

impl TokenUsage {
    /// 从 Claude API 非流式响应解析
    pub fn from_claude_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            input_tokens: usage.get("input_tokens")?.as_u64()? as u32,
            output_tokens: usage.get("output_tokens")?.as_u64()? as u32,
            cache_read_tokens: usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            model,
        })
    }

    /// 从 Claude API 流式响应解析
    #[allow(dead_code)]
    pub fn from_claude_stream_events(events: &[Value]) -> Option<Self> {
        let mut usage = Self::default();
        let mut model: Option<String> = None;

        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                match event_type {
                    "message_start" => {
                        // 从 message_start 提取模型名称
                        if model.is_none() {
                            if let Some(message) = event.get("message") {
                                if let Some(m) = message.get("model").and_then(|v| v.as_str()) {
                                    model = Some(m.to_string());
                                }
                            }
                        }
                        if let Some(msg_usage) = event.get("message").and_then(|m| m.get("usage")) {
                            // 从 message_start 获取 input_tokens（原生 Claude API）
                            if let Some(input) =
                                msg_usage.get("input_tokens").and_then(|v| v.as_u64())
                            {
                                usage.input_tokens = input as u32;
                            }
                            usage.cache_read_tokens = msg_usage
                                .get("cache_read_input_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as u32;
                            usage.cache_creation_tokens = msg_usage
                                .get("cache_creation_input_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as u32;
                        }
                    }
                    "message_delta" => {
                        if let Some(delta_usage) = event.get("usage") {
                            // 从 message_delta 获取 output_tokens
                            if let Some(output) =
                                delta_usage.get("output_tokens").and_then(|v| v.as_u64())
                            {
                                usage.output_tokens = output as u32;
                            }
                            // OpenRouter 转换后的流式响应：input_tokens 也在 message_delta 中
                            // 如果 message_start 中没有 input_tokens，则从 message_delta 获取
                            if usage.input_tokens == 0 {
                                if let Some(input) =
                                    delta_usage.get("input_tokens").and_then(|v| v.as_u64())
                                {
                                    usage.input_tokens = input as u32;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if usage.input_tokens > 0 || usage.output_tokens > 0 {
            usage.model = model;
            Some(usage)
        } else {
            None
        }
    }

    /// 从 OpenRouter 响应解析 (OpenAI 格式)
    #[allow(dead_code)]
    pub fn from_openrouter_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        Some(Self {
            input_tokens: usage.get("prompt_tokens")?.as_u64()? as u32,
            output_tokens: usage.get("completion_tokens")?.as_u64()? as u32,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
        })
    }

    /// 从 Codex API 非流式响应解析
    pub fn from_codex_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage");
        if usage.is_none() {
            log::debug!(
                "[Codex] 响应中没有 usage 字段，body keys: {:?}",
                body.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
            return None;
        }
        let usage = usage?;

        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64());
        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64());

        if input_tokens.is_none() || output_tokens.is_none() {
            log::debug!("[Codex] usage 字段缺少 input_tokens 或 output_tokens，usage: {usage:?}");
            return None;
        }

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cached_tokens = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                usage
                    .get("input_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(|v| v.as_u64())
            })
            .unwrap_or(0) as u32;

        Some(Self {
            input_tokens: input_tokens? as u32,
            output_tokens: output_tokens? as u32,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            model,
        })
    }

    /// 从 Codex API 响应解析并调整 input_tokens
    ///
    /// Codex 的 input_tokens 需要减去 cached_tokens 以获得实际计费的 token 数
    /// 公式: adjusted_input = max(input_tokens - cached_tokens, 0)
    #[allow(dead_code)]
    pub fn from_codex_response_adjusted(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
        let input_tokens = usage.get("input_tokens")?.as_u64()? as u32;
        let output_tokens = usage.get("output_tokens")?.as_u64()? as u32;

        // 获取 cached_tokens (可能在 cache_read_input_tokens 或 input_tokens_details 中)
        let cached_tokens = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                usage
                    .get("input_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(|v| v.as_u64())
            })
            .unwrap_or(0) as u32;

        // 调整 input_tokens: 减去 cached_tokens
        let adjusted_input = input_tokens.saturating_sub(cached_tokens);

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            input_tokens: adjusted_input,
            output_tokens,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            model,
        })
    }

    /// 从 Codex API 流式响应解析
    #[allow(dead_code)]
    pub fn from_codex_stream_events(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 解析流式事件，共 {} 个事件", events.len());
        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                log::debug!("[Codex] 事件类型: {event_type}");
                if event_type == "response.completed" {
                    if let Some(response) = event.get("response") {
                        log::debug!("[Codex] 找到 response.completed 事件，解析 usage");
                        return Self::from_codex_response_adjusted(response);
                    }
                }
            }
        }
        log::debug!("[Codex] 未找到 response.completed 事件");
        None
    }

    /// 智能 Codex 响应解析 - 自动检测 OpenAI 或 Codex 格式
    ///
    /// Codex 支持两种 API 格式：
    /// - `/v1/responses`: 使用 input_tokens/output_tokens
    /// - `/v1/chat/completions`: 使用 prompt_tokens/completion_tokens (OpenAI 格式)
    ///
    /// 注意：记录原始 input_tokens，费用计算时再减去 cached_tokens
    pub fn from_codex_response_auto(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // 检测格式：OpenAI 使用 prompt_tokens，Codex 使用 input_tokens
        if usage.get("prompt_tokens").is_some() {
            log::debug!("[Codex] 检测到 OpenAI 格式 (prompt_tokens)");
            Self::from_openai_response(body)
        } else if usage.get("input_tokens").is_some() {
            log::debug!("[Codex] 检测到 Codex 格式 (input_tokens)");
            // 使用非调整版本，记录原始 input_tokens
            Self::from_codex_response(body)
        } else {
            log::debug!("[Codex] 无法识别响应格式，usage: {usage:?}");
            None
        }
    }

    /// 智能 Codex 流式响应解析 - 自动检测 OpenAI 或 Codex 格式
    pub fn from_codex_stream_events_auto(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 智能解析流式事件，共 {} 个事件", events.len());

        // 先尝试 Codex Responses API 格式 (response.completed 事件)
        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                if event_type == "response.completed" {
                    if let Some(response) = event.get("response") {
                        log::debug!("[Codex] 找到 response.completed 事件");
                        return Self::from_codex_response_auto(response);
                    }
                }
            }
        }

        // 回退到 OpenAI Chat Completions 格式 (最后一个 chunk 包含 usage)
        log::debug!("[Codex] 尝试 OpenAI 流式格式");
        Self::from_openai_stream_events(events)
    }

    /// 从 OpenAI Chat Completions API 响应解析 (prompt_tokens, completion_tokens)
    pub fn from_openai_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // OpenAI 使用 prompt_tokens 和 completion_tokens
        let prompt_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64())?;
        let completion_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64())?;

        // 获取 cached_tokens (可能在 prompt_tokens_details 中)
        let cached_tokens = usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            input_tokens: prompt_tokens as u32,
            output_tokens: completion_tokens as u32,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: 0,
            model,
        })
    }

    /// 从 OpenAI Chat Completions API 流式响应解析
    pub fn from_openai_stream_events(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 解析 OpenAI 流式事件，共 {} 个事件", events.len());
        // OpenAI 流式响应在最后一个 chunk 中包含 usage
        for event in events.iter().rev() {
            if let Some(usage) = event.get("usage") {
                if !usage.is_null() {
                    log::debug!("[Codex] 找到 usage: {usage:?}");
                    return Self::from_openai_response(event);
                }
            }
        }
        log::debug!("[Codex] 未找到 usage 信息");
        None
    }

    /// 从 Gemini API 非流式响应解析
    pub fn from_gemini_response(body: &Value) -> Option<Self> {
        let usage = body.get("usageMetadata")?;
        // 提取实际使用的模型名称（modelVersion 字段）
        let model = body
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let prompt_tokens = usage.get("promptTokenCount")?.as_u64()? as u32;
        let total_tokens = usage.get("totalTokenCount")?.as_u64()? as u32;

        // 输出 tokens = 总 tokens - 输入 tokens
        // 这包含了 candidatesTokenCount + thoughtsTokenCount
        let output_tokens = total_tokens.saturating_sub(prompt_tokens);

        Some(Self {
            input_tokens: prompt_tokens,
            output_tokens,
            cache_read_tokens: usage
                .get("cachedContentTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: 0,
            model,
        })
    }

    /// 从 Gemini API 流式响应解析
    #[allow(dead_code)]
    pub fn from_gemini_stream_chunks(chunks: &[Value]) -> Option<Self> {
        let mut total_input = 0u32;
        let mut total_tokens = 0u32;
        let mut total_cache_read = 0u32;
        let mut model: Option<String> = None;

        for chunk in chunks {
            if let Some(usage) = chunk.get("usageMetadata") {
                // 输入 tokens (通常在所有 chunk 中保持不变)
                total_input = usage
                    .get("promptTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                // 总 tokens (包含输入 + 输出 + 思考)
                total_tokens = usage
                    .get("totalTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                // 缓存读取 tokens
                total_cache_read = usage
                    .get("cachedContentTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }

            // 提取实际使用的模型名称（modelVersion 字段）
            if model.is_none() {
                if let Some(model_version) = chunk.get("modelVersion").and_then(|v| v.as_str()) {
                    model = Some(model_version.to_string());
                }
            }
        }

        // 输出 tokens = 总 tokens - 输入 tokens
        let total_output = total_tokens.saturating_sub(total_input);

        if total_input > 0 || total_output > 0 {
            Some(Self {
                input_tokens: total_input,
                output_tokens: total_output,
                cache_read_tokens: total_cache_read,
                cache_creation_tokens: 0,
                model,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_claude_response_parsing() {
        let response = json!({
            "model": "claude-sonnet-4-20250514",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        });

        let usage = TokenUsage::from_claude_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_response_parsing_no_model() {
        let response = json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10
            }
        });

        let usage = TokenUsage::from_claude_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_claude_stream_parsing() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "cache_creation_input_tokens": 10
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_stream_parsing_no_model() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "usage": {
                        "input_tokens": 100,
                        "cache_read_input_tokens": 20,
                        "cache_creation_input_tokens": 10
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_openrouter_response_parsing() {
        let response = json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50
            }
        });

        let usage = TokenUsage::from_openrouter_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
    }

    #[test]
    fn test_gemini_response_parsing() {
        let response = json!({
            "modelVersion": "gemini-3-pro-high",
            "usageMetadata": {
                "promptTokenCount": 8383,
                "candidatesTokenCount": 50,
                "thoughtsTokenCount": 114,
                "totalTokenCount": 8547,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 8383);
        // output_tokens = totalTokenCount - promptTokenCount = 8547 - 8383 = 164
        assert_eq!(usage.output_tokens, 164);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, Some("gemini-3-pro-high".to_string()));
    }

    #[test]
    fn test_gemini_response_parsing_no_model() {
        // 测试没有 modelVersion 字段的情况
        let response = json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "totalTokenCount": 150,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        // output_tokens = totalTokenCount - promptTokenCount = 150 - 100 = 50
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_gemini_response_with_thoughts() {
        // 测试包含 thoughtsTokenCount 的实际响应
        // 这是用户报告的真实场景
        let response = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "text": "",
                                "thoughtSignature": "EvcECvQE..."
                            }
                        ],
                        "role": "model"
                    },
                    "finishReason": "STOP"
                }
            ],
            "modelVersion": "gemini-3-pro-high",
            "responseId": "yupTafqLDu-PjMcPhrOx4QQ",
            "usageMetadata": {
                "candidatesTokenCount": 50,
                "promptTokenCount": 8383,
                "thoughtsTokenCount": 114,
                "totalTokenCount": 8547
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 8383);
        // output_tokens = totalTokenCount - promptTokenCount
        // = 8547 - 8383 = 164 (包含 candidatesTokenCount 50 + thoughtsTokenCount 114)
        assert_eq!(usage.output_tokens, 164);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, Some("gemini-3-pro-high".to_string()));
    }

    #[test]
    fn test_codex_response_parsing_cached_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        // 非调整模式：input_tokens 保持原值，但应记录缓存命中
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
    }

    #[test]
    fn test_codex_response_adjusted() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        // input_tokens 应该被调整: 1000 - 300 = 700
        assert_eq!(usage.input_tokens, 700);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
    }

    #[test]
    fn test_codex_response_adjusted_no_cache() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        // 没有 cached_tokens，input_tokens 保持不变
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 0);
    }

    #[test]
    fn test_codex_response_adjusted_cache_read_input_tokens() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "cache_read_input_tokens": 200
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        assert_eq!(usage.input_tokens, 800);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
    }

    #[test]
    fn test_codex_response_adjusted_saturating_sub() {
        // 测试 cached_tokens > input_tokens 的边界情况
        let response = json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "input_tokens_details": {
                    "cached_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response_adjusted(&response).unwrap();
        // saturating_sub 确保不会下溢
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 200);
    }

    #[test]
    fn test_openrouter_stream_parsing() {
        // 测试 OpenRouter 转换后的流式响应解析
        // OpenRouter 流式响应经过转换后，input_tokens 在 message_delta 中
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 0,
                        "output_tokens": 0
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": "end_turn"
                },
                "usage": {
                    "input_tokens": 150,
                    "output_tokens": 75
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 150);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_native_claude_stream_parsing() {
        // 测试原生 Claude API 流式响应解析
        // 原生 Claude API 的 input_tokens 在 message_start 中
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-20250514",
                    "usage": {
                        "input_tokens": 200,
                        "cache_read_input_tokens": 50
                    }
                }
            }),
            json!({
                "type": "message_delta",
                "usage": {
                    "output_tokens": 100
                }
            }),
        ];

        let usage = TokenUsage::from_claude_stream_events(&events).unwrap();
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cache_read_tokens, 50);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    // ============================================================================
    // 智能 Codex 解析测试
    // ============================================================================

    #[test]
    fn test_codex_response_auto_openai_format() {
        // OpenAI 格式 (prompt_tokens/completion_tokens)
        let response = json!({
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 500,
                "prompt_tokens_details": {
                    "cached_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_codex_response_auto_codex_format() {
        // Codex 格式 (input_tokens/output_tokens)
        let response = json!({
            "model": "o3",
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.model, Some("o3".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_codex_format() {
        // Codex Responses API 流式格式 (response.completed 事件)
        let events = vec![
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_123"
                }
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "model": "o3",
                    "usage": {
                        "input_tokens": 1000,
                        "output_tokens": 500,
                        "input_tokens_details": {
                            "cached_tokens": 200
                        }
                    }
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("o3".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_openai_format() {
        // OpenAI Chat Completions 流式格式 (最后一个 chunk 包含 usage)
        let events = vec![
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {"content": "Hello"}}]
            }),
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {}}],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }
}
