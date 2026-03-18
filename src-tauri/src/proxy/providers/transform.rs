//! 格式转换模块
//!
//! 实现 Anthropic ↔ OpenAI 格式转换，用于 OpenRouter 支持
//! 参考: anthropic-proxy-rs

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// Detect OpenAI o-series reasoning models (o1, o3, o4-mini, etc.)
/// These models require `max_completion_tokens` instead of `max_tokens`.
pub fn is_openai_o_series(model: &str) -> bool {
    model.len() > 1
        && model.starts_with('o')
        && model.as_bytes().get(1).is_some_and(|b| b.is_ascii_digit())
}

/// Anthropic 请求 → OpenAI 请求
///
/// `cache_key`: optional prompt_cache_key to inject for improved cache routing
pub fn anthropic_to_openai(body: Value, cache_key: Option<&str>) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut messages = Vec::new();

    // 处理 system prompt
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            // 单个字符串
            messages.push(json!({"role": "system", "content": text}));
        } else if let Some(arr) = system.as_array() {
            // 多个 system message — preserve cache_control for compatible proxies
            for msg in arr {
                if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                    let mut sys_msg = json!({"role": "system", "content": text});
                    if let Some(cc) = msg.get("cache_control") {
                        sys_msg["cache_control"] = cc.clone();
                    }
                    messages.push(sys_msg);
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

    // 转换参数 — o-series 模型需要 max_completion_tokens
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    if let Some(v) = body.get("max_tokens") {
        if is_openai_o_series(model) {
            result["max_completion_tokens"] = v.clone();
        } else {
            result["max_tokens"] = v.clone();
        }
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
                let mut tool = json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "description": t.get("description"),
                        "parameters": clean_schema(t.get("input_schema").cloned().unwrap_or(json!({})))
                    }
                });
                if let Some(cc) = t.get("cache_control") {
                    tool["cache_control"] = cc.clone();
                }
                tool
            })
            .collect();

        if !openai_tools.is_empty() {
            result["tools"] = json!(openai_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = v.clone();
    }

    // Inject prompt_cache_key for improved cache routing on OpenAI-compatible endpoints
    if let Some(key) = cache_key {
        result["prompt_cache_key"] = json!(key);
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
                        let mut part = json!({"type": "text", "text": text});
                        if let Some(cc) = block.get("cache_control") {
                            part["cache_control"] = cc.clone();
                        }
                        content_parts.push(part);
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
                // When cache_control is present, keep array format to preserve it
                let has_cache_control = content_parts[0].get("cache_control").is_some();
                if !has_cache_control {
                    if let Some(text) = content_parts[0].get("text") {
                        msg["content"] = text.clone();
                    } else {
                        msg["content"] = json!(content_parts);
                    }
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
pub fn clean_schema(mut schema: Value) -> Value {
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

/// OpenAI 响应 → Anthropic 响应
pub fn openai_to_anthropic(body: Value) -> Result<Value, ProxyError> {
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
    let mut has_tool_use = false;

    // 文本/拒绝内容
    if let Some(msg_content) = message.get("content") {
        if let Some(text) = msg_content.as_str() {
            if !text.is_empty() {
                content.push(json!({"type": "text", "text": text}));
            }
        } else if let Some(parts) = msg_content.as_array() {
            for part in parts {
                let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match part_type {
                    "text" | "output_text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                content.push(json!({"type": "text", "text": text}));
                            }
                        }
                    }
                    "refusal" => {
                        if let Some(refusal) = part.get("refusal").and_then(|r| r.as_str()) {
                            if !refusal.is_empty() {
                                content.push(json!({"type": "text", "text": refusal}));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // Some providers put refusal at message-level.
    if let Some(refusal) = message.get("refusal").and_then(|r| r.as_str()) {
        if !refusal.is_empty() {
            content.push(json!({"type": "text", "text": refusal}));
        }
    }

    // 工具调用（tool_calls）
    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        if !tool_calls.is_empty() {
            has_tool_use = true;
        }
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let empty_obj = json!({});
            let func = tc.get("function").unwrap_or(&empty_obj);
            let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args_str = func
                .get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

            content.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }
    // 兼容旧格式（function_call）
    if !has_tool_use {
        if let Some(function_call) = message.get("function_call") {
            let id = function_call
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("");
            let name = function_call
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let has_arguments = function_call.get("arguments").is_some();

            let input = match function_call.get("arguments") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
                Some(v @ Value::Object(_)) | Some(v @ Value::Array(_)) => v.clone(),
                _ => json!({}),
            };

            if !name.is_empty() || has_arguments {
                content.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input
                }));
                has_tool_use = true;
            }
        }
    }

    // 映射 finish_reason → stop_reason
    let stop_reason = choice
        .get("finish_reason")
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" | "function_call" => "tool_use",
            "content_filter" => "end_turn",
            other => {
                log::warn!(
                    "[Claude/OpenAI] Unknown finish_reason in non-streaming response: {other}"
                );
                "end_turn"
            }
        })
        .or(if has_tool_use { Some("tool_use") } else { None });

    // usage — map cache tokens from OpenAI format to Anthropic format
    let usage = body.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });

    // OpenAI standard: prompt_tokens_details.cached_tokens
    if let Some(cached) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
    {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    // Some compatible servers return these fields directly
    if let Some(v) = usage.get("cache_read_input_tokens") {
        usage_json["cache_read_input_tokens"] = v.clone();
    }
    if let Some(v) = usage.get("cache_creation_input_tokens") {
        usage_json["cache_creation_input_tokens"] = v.clone();
    }

    let result = json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage_json
    });

    Ok(result)
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

        let result = anthropic_to_openai(input, None).unwrap();
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

        let result = anthropic_to_openai(input, None).unwrap();
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

        let result = anthropic_to_openai(input, None).unwrap();
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

        let result = anthropic_to_openai(input, None).unwrap();
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

        let result = anthropic_to_openai(input, None).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "call_123");
        assert_eq!(msg["content"], "Sunny, 25°C");
    }

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
    fn test_model_passthrough() {
        // 格式转换层只做结构转换，模型映射由上游 proxy::model_mapper 处理
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, None).unwrap();
        assert_eq!(result["model"], "gpt-4o");
    }

    #[test]
    fn test_anthropic_to_openai_with_cache_key() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, Some("provider-123")).unwrap();
        assert_eq!(result["prompt_cache_key"], "provider-123");
    }

    #[test]
    fn test_anthropic_to_openai_no_cache_key() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, None).unwrap();
        assert!(result.get("prompt_cache_key").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_cache_control_preserved() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "System prompt", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
                ]
            }],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            }]
        });

        let result = anthropic_to_openai(input, None).unwrap();
        // System message cache_control preserved
        assert_eq!(result["messages"][0]["cache_control"]["type"], "ephemeral");
        // Text block cache_control preserved
        assert_eq!(
            result["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(
            result["messages"][1]["content"][0]["cache_control"]["ttl"],
            "5m"
        );
        // Tool cache_control preserved
        assert_eq!(result["tools"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_openai_to_anthropic_with_cache_tokens() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["usage"]["input_tokens"], 100);
        assert_eq!(result["usage"]["output_tokens"], 50);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 80);
    }

    #[test]
    fn test_openai_to_anthropic_with_direct_cache_fields() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 20
            }
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 20);
    }

    #[test]
    fn test_openai_to_anthropic_finish_reason_content_filter_maps_end_turn() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Blocked"},
                "finish_reason": "content_filter"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 1}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "end_turn");
    }

    #[test]
    fn test_openai_to_anthropic_with_legacy_function_call() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "function_call": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Tokyo\"}"
                    }
                },
                "finish_reason": "function_call"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_openai_to_anthropic_with_content_parts_and_refusal() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "refusal", "refusal": "I can't do that"}
                    ]
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello");
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "I can't do that");
    }

    #[test]
    fn test_is_openai_o_series() {
        assert!(is_openai_o_series("o1"));
        assert!(is_openai_o_series("o1-preview"));
        assert!(is_openai_o_series("o1-mini"));
        assert!(is_openai_o_series("o3"));
        assert!(is_openai_o_series("o3-mini"));
        assert!(is_openai_o_series("o4-mini"));
        assert!(!is_openai_o_series("gpt-4o"));
        assert!(!is_openai_o_series("openai-gpt"));
        assert!(!is_openai_o_series("o"));
        assert!(!is_openai_o_series(""));
    }

    #[test]
    fn test_anthropic_to_openai_o_series_max_completion_tokens() {
        for model in &["o1", "o3-mini", "o4-mini"] {
            let input = json!({
                "model": model,
                "max_tokens": 4096,
                "messages": [{"role": "user", "content": "Hello"}]
            });

            let result = anthropic_to_openai(input, None).unwrap();
            assert!(
                result.get("max_tokens").is_none(),
                "{model} should not have max_tokens"
            );
            assert_eq!(
                result["max_completion_tokens"], 4096,
                "{model} should use max_completion_tokens"
            );
        }
    }

    #[test]
    fn test_anthropic_to_openai_non_o_series_keeps_max_tokens() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, None).unwrap();
        assert_eq!(result["max_tokens"], 1024);
        assert!(result.get("max_completion_tokens").is_none());
    }
}
