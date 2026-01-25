//! OpenAI → Anthropic 响应转换器
//!
//! 将 OpenAI Chat Completions API 响应转换为 Anthropic Messages API 格式

use crate::proxy::error::ProxyError;
use crate::proxy::transform::{format::ApiFormat, traits::FormatTransformer};
use bytes::Bytes;
use futures::stream::Stream;
use serde_json::{json, Value};
use std::pin::Pin;

use super::streaming::create_anthropic_sse_stream;

/// OpenAI → Anthropic 响应转换器
pub struct OpenAIToAnthropicTransformer;

impl OpenAIToAnthropicTransformer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenAIToAnthropicTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatTransformer for OpenAIToAnthropicTransformer {
    fn name(&self) -> &'static str {
        "OpenAI→Anthropic"
    }

    fn source_format(&self) -> ApiFormat {
        ApiFormat::OpenAI
    }

    fn target_format(&self) -> ApiFormat {
        ApiFormat::Anthropic
    }

    fn transform_request(&self, body: Value) -> Result<Value, ProxyError> {
        // 响应转换器不处理请求，直接透传
        Ok(body)
    }

    fn transform_response(&self, body: Value) -> Result<Value, ProxyError> {
        openai_to_anthropic(body)
    }

    fn transform_stream(
        &self,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
        Box::pin(create_anthropic_sse_stream(stream))
    }
}

/// OpenAI 响应 → Anthropic 响应
fn openai_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    let choices = body
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| ProxyError::TransformError("No choices in response".to_string()))?;

    let choice = choices
        .first()
        .ok_or_else(|| ProxyError::TransformError("Empty choices array".to_string()))?;

    let message = choice
        .get("message")
        .ok_or_else(|| ProxyError::TransformError("No message in choice".to_string()))?;

    let mut content = Vec::new();

    // 文本内容
    if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
        if !text.is_empty() {
            content.push(json!({"type": "text", "text": text}));
        }
    }

    // 工具调用
    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let empty_obj = json!({});
            let func = tc.get("function").unwrap_or(&empty_obj);
            let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args_str = func
                .get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");

            // 解析 arguments JSON，失败时返回错误而不是静默使用空对象
            let input: Value = serde_json::from_str(args_str).map_err(|e| {
                log::error!("[Transform] tool_calls.arguments 解析失败: {e}, 原始内容: {args_str}");
                ProxyError::TransformError(format!(
                    "Failed to parse tool_calls.arguments: {e}, content: {args_str}"
                ))
            })?;

            content.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    // 映射 finish_reason → stop_reason
    let stop_reason = choice
        .get("finish_reason")
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "tool_use",
            other => other,
        });

    // usage
    let usage = body.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let result = json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    });

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_to_anthropic_simple() {
        let input = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["id"], "chatcmpl-123");
        assert_eq!(result["type"], "message");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello!");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_openai_to_anthropic_with_tool_calls() {
        let input = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"location\": \"Tokyo\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_123");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_stop_reason_mapping() {
        // stop → end_turn
        let input = json!({
            "choices": [{"message": {"content": "Hi"}, "finish_reason": "stop"}],
            "usage": {}
        });
        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "end_turn");

        // length → max_tokens
        let input = json!({
            "choices": [{"message": {"content": "Hi"}, "finish_reason": "length"}],
            "usage": {}
        });
        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "max_tokens");

        // tool_calls → tool_use
        let input = json!({
            "choices": [{"message": {"content": null, "tool_calls": []}, "finish_reason": "tool_calls"}],
            "usage": {}
        });
        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "tool_use");
    }
}
