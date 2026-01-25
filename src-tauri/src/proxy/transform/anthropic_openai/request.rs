//! Anthropic → OpenAI 请求转换器
//!
//! 将 Anthropic Messages API 请求转换为 OpenAI Chat Completions API 格式

use crate::proxy::error::ProxyError;
use crate::proxy::transform::{format::ApiFormat, traits::FormatTransformer};
use bytes::Bytes;
use futures::stream::Stream;
use serde_json::{json, Value};
use std::pin::Pin;

/// Anthropic → OpenAI 请求转换器
pub struct AnthropicToOpenAITransformer;

impl AnthropicToOpenAITransformer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AnthropicToOpenAITransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatTransformer for AnthropicToOpenAITransformer {
    fn name(&self) -> &'static str {
        "Anthropic→OpenAI"
    }

    fn source_format(&self) -> ApiFormat {
        ApiFormat::Anthropic
    }

    fn target_format(&self) -> ApiFormat {
        ApiFormat::OpenAI
    }

    fn transform_request(&self, body: Value) -> Result<Value, ProxyError> {
        anthropic_to_openai(body)
    }

    fn transform_response(&self, body: Value) -> Result<Value, ProxyError> {
        // 请求转换器不处理响应，直接透传
        Ok(body)
    }

    fn transform_stream(
        &self,
        _stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
        // 请求转换器不处理流
        Box::pin(futures::stream::empty())
    }

    fn transform_endpoint(&self, endpoint: &str) -> String {
        // /v1/messages → /v1/chat/completions
        if endpoint == "/v1/messages" {
            "/v1/chat/completions".to_string()
        } else {
            endpoint.to_string()
        }
    }
}

/// Anthropic 请求 → OpenAI 请求
fn anthropic_to_openai(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // 模型直接透传（模型映射由 model_mapper 模块独立处理）
    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    let mut messages = Vec::new();

    // 处理 system prompt
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            // 单个字符串
            messages.push(json!({"role": "system", "content": text}));
        } else if let Some(arr) = system.as_array() {
            // 多个 system message
            for msg in arr {
                if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                    messages.push(json!({"role": "system", "content": text}));
                }
            }
        }
    }

    // 转换 messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");
            let converted = convert_message_to_openai(role, content)?;
            messages.extend(converted);
        }
    }

    result["messages"] = json!(messages);

    // 转换参数
    if let Some(v) = body.get("max_tokens") {
        result["max_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stop_sequences") {
        result["stop"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // 转换 tools (过滤 BatchTool)
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "description": t.get("description"),
                        "parameters": clean_schema(t.get("input_schema").cloned().unwrap_or(json!({})))
                    }
                })
            })
            .collect();

        if !openai_tools.is_empty() {
            result["tools"] = json!(openai_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = v.clone();
    }

    Ok(result)
}

/// 转换单条消息到 OpenAI 格式（可能产生多条消息）
fn convert_message_to_openai(
    role: &str,
    content: Option<&Value>,
) -> Result<Vec<Value>, ProxyError> {
    let mut result = Vec::new();

    let content = match content {
        Some(c) => c,
        None => {
            result.push(json!({"role": role, "content": null}));
            return Ok(result);
        }
    };

    // 字符串内容
    if let Some(text) = content.as_str() {
        result.push(json!({"role": role, "content": text}));
        return Ok(result);
    }

    // 数组内容（多模态/工具调用）
    if let Some(blocks) = content.as_array() {
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        content_parts.push(json!({"type": "text", "text": text}));
                    }
                }
                "image" => {
                    if let Some(source) = block.get("source") {
                        let media_type = source
                            .get("media_type")
                            .and_then(|m| m.as_str())
                            .unwrap_or("image/png");
                        let data = source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                        content_parts.push(json!({
                            "type": "image_url",
                            "image_url": {"url": format!("data:{};base64,{}", media_type, data)}
                        }));
                    }
                }
                "tool_use" => {
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&input).unwrap_or_default()
                        }
                    }));
                }
                "tool_result" => {
                    // tool_result 变成单独的 tool role 消息
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    let content_val = block.get("content");
                    let content_str = match content_val {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => serde_json::to_string(v).unwrap_or_default(),
                        None => String::new(),
                    };
                    result.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content_str
                    }));
                }
                "thinking" => {
                    // 跳过 thinking blocks
                }
                _ => {}
            }
        }

        // 添加带内容和/或工具调用的消息
        if !content_parts.is_empty() || !tool_calls.is_empty() {
            let mut msg = json!({"role": role});

            // 内容处理
            if content_parts.is_empty() {
                msg["content"] = Value::Null;
            } else if content_parts.len() == 1 {
                if let Some(text) = content_parts[0].get("text") {
                    msg["content"] = text.clone();
                } else {
                    msg["content"] = json!(content_parts);
                }
            } else {
                msg["content"] = json!(content_parts);
            }

            // 工具调用
            if !tool_calls.is_empty() {
                msg["tool_calls"] = json!(tool_calls);
            }

            result.push(msg);
        }

        return Ok(result);
    }

    // 其他情况直接透传
    result.push(json!({"role": role, "content": content}));
    Ok(result)
}

/// 清理 JSON schema（移除不支持的 format）
fn clean_schema(mut schema: Value) -> Value {
    if let Some(obj) = schema.as_object_mut() {
        // 移除 "format": "uri"
        if obj.get("format").and_then(|v| v.as_str()) == Some("uri") {
            obj.remove("format");
        }

        // 递归清理嵌套 schema
        if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
            for (_, value) in properties.iter_mut() {
                *value = clean_schema(value.clone());
            }
        }

        if let Some(items) = obj.get_mut("items") {
            *items = clean_schema(items.clone());
        }
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_to_openai_simple() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["model"], "claude-3-opus");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_with_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_with_tools() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_anthropic_to_openai_tool_use() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me check"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_tool_result() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_123", "content": "Sunny, 25°C"}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "call_123");
        assert_eq!(msg["content"], "Sunny, 25°C");
    }

    #[test]
    fn test_transform_endpoint() {
        let transformer = AnthropicToOpenAITransformer::new();
        assert_eq!(
            transformer.transform_endpoint("/v1/messages"),
            "/v1/chat/completions"
        );
        assert_eq!(transformer.transform_endpoint("/v1/other"), "/v1/other");
    }
}
