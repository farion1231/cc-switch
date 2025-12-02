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
        })
    }

    /// 从 Claude API 流式响应解析
    #[allow(dead_code)]
    pub fn from_claude_stream_events(events: &[Value]) -> Option<Self> {
        let mut usage = Self::default();

        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                match event_type {
                    "message_start" => {
                        if let Some(msg_usage) = event.get("message").and_then(|m| m.get("usage")) {
                            usage.input_tokens = msg_usage.get("input_tokens")?.as_u64()? as u32;
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
                            usage.output_tokens =
                                delta_usage.get("output_tokens")?.as_u64()? as u32;
                        }
                    }
                    _ => {}
                }
            }
        }

        if usage.input_tokens > 0 || usage.output_tokens > 0 {
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
        })
    }

    /// 从 Codex API 非流式响应解析
    pub fn from_codex_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;
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
        })
    }

    /// 从 Codex API 流式响应解析
    #[allow(dead_code)]
    pub fn from_codex_stream_events(events: &[Value]) -> Option<Self> {
        for event in events {
            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                if event_type == "response.completed" {
                    if let Some(response) = event.get("response") {
                        return Self::from_codex_response(response);
                    }
                }
            }
        }
        None
    }

    /// 从 Gemini API 非流式响应解析
    pub fn from_gemini_response(body: &Value) -> Option<Self> {
        let usage = body.get("usageMetadata")?;
        Some(Self {
            input_tokens: usage.get("promptTokenCount")?.as_u64()? as u32,
            output_tokens: usage.get("candidatesTokenCount")?.as_u64()? as u32,
            cache_read_tokens: usage
                .get("cachedContentTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: 0,
        })
    }

    /// 从 Gemini API 流式响应解析
    #[allow(dead_code)]
    pub fn from_gemini_stream_chunks(chunks: &[Value]) -> Option<Self> {
        let mut total_input = 0u32;
        let mut total_output = 0u32;
        let mut total_cache_read = 0u32;

        for chunk in chunks {
            if let Some(usage) = chunk.get("usageMetadata") {
                total_input = usage
                    .get("promptTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                total_output += usage
                    .get("candidatesTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                total_cache_read = usage
                    .get("cachedContentTokenCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
            }
        }

        if total_input > 0 || total_output > 0 {
            Some(Self {
                input_tokens: total_input,
                output_tokens: total_output,
                cache_read_tokens: total_cache_read,
                cache_creation_tokens: 0,
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
    }

    #[test]
    fn test_claude_stream_parsing() {
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
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 50,
                "cachedContentTokenCount": 20
            }
        });

        let usage = TokenUsage::from_gemini_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_creation_tokens, 0);
    }
}
