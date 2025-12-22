//! 格式转换模块
//!
//! 实现 Anthropic ↔ OpenAI 格式转换，用于 OpenRouter 支持
//! 参考: anthropic-proxy-rs

use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// 从 Provider 配置中获取模型映射
fn get_model_from_provider(model: &str, provider: &Provider, body: &Value) -> String {
    let env = provider.settings_config.get("env");
    let model_lower = model.to_lowercase();

    // 检测 thinking 参数
    let has_thinking = body
        .get("thinking")
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("type"))
        .and_then(|t| t.as_str())
        == Some("enabled");

    if let Some(env) = env {
        // 如果启用 thinking，优先使用推理模型
        if has_thinking {
            if let Some(m) = env
                .get("ANTHROPIC_REASONING_MODEL")
                .and_then(|v| v.as_str())
            {
                log::debug!("[Transform] 使用推理模型: {m}");
                return m.to_string();
            }
        }

        // 根据模型类型选择配置模型
        if model_lower.contains("haiku") {
            if let Some(m) = env
                .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|v| v.as_str())
            {
                return m.to_string();
            }
        }
        if model_lower.contains("opus") {
            if let Some(m) = env
                .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(|v| v.as_str())
            {
                return m.to_string();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(m) = env
                .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(|v| v.as_str())
            {
                return m.to_string();
            }
        }
        // 默认使用 ANTHROPIC_MODEL
        if let Some(m) = env.get("ANTHROPIC_MODEL").and_then(|v| v.as_str()) {
            return m.to_string();
        }
    }

    model.to_string()
}

/// Anthropic 请求 → OpenAI 请求
pub fn anthropic_to_openai(body: Value, provider: &Provider) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // 模型映射：使用 Provider 配置中的模型（支持 thinking 参数）
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        let mapped_model = get_model_from_provider(model, provider, &body);
        result["model"] = json!(mapped_model);
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
            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

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

    fn create_provider(env_config: Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Provider".to_string(),
            settings_config: json!({"env": env_config}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn create_openrouter_provider() -> Provider {
        create_provider(json!({
            "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
            "ANTHROPIC_MODEL": "anthropic/claude-sonnet-4.5",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "anthropic/claude-haiku-4.5",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "anthropic/claude-sonnet-4.5",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "anthropic/claude-opus-4.5"
        }))
    }

    #[test]
    fn test_anthropic_to_openai_simple() {
        let provider = create_openrouter_provider();
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, &provider).unwrap();
        // opus 模型映射到配置的 ANTHROPIC_DEFAULT_OPUS_MODEL
        assert_eq!(result["model"], "anthropic/claude-opus-4.5");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_with_system() {
        let provider = create_openrouter_provider();
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, &provider).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_with_tools() {
        let provider = create_openrouter_provider();
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

        let result = anthropic_to_openai(input, &provider).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_anthropic_to_openai_tool_use() {
        let provider = create_openrouter_provider();
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

        let result = anthropic_to_openai(input, &provider).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_tool_result() {
        let provider = create_openrouter_provider();
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

        let result = anthropic_to_openai(input, &provider).unwrap();
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
    fn test_model_mapping_from_provider() {
        let provider = create_openrouter_provider();
        let body = json!({"model": "test"});

        // sonnet 模型
        assert_eq!(
            get_model_from_provider("claude-sonnet-4-5-20250929", &provider, &body),
            "anthropic/claude-sonnet-4.5"
        );

        // haiku 模型
        assert_eq!(
            get_model_from_provider("claude-haiku-4-5-20250929", &provider, &body),
            "anthropic/claude-haiku-4.5"
        );

        // opus 模型
        assert_eq!(
            get_model_from_provider("claude-opus-4-5", &provider, &body),
            "anthropic/claude-opus-4.5"
        );
    }

    #[test]
    fn test_anthropic_to_openai_model_mapping() {
        let provider = create_openrouter_provider();
        let input = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, &provider).unwrap();
        assert_eq!(result["model"], "anthropic/claude-sonnet-4.5");
    }

    #[test]
    fn test_thinking_parameter_detection() {
        let mut provider = create_openrouter_provider();
        // 添加推理模型配置
        if let Some(env) = provider.settings_config.get_mut("env") {
            env["ANTHROPIC_REASONING_MODEL"] = json!("anthropic/claude-sonnet-4.5:extended");
        }

        let input = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 1024,
            "thinking": {"type": "enabled"},
            "messages": [{"role": "user", "content": "Solve this problem"}]
        });

        let result = anthropic_to_openai(input, &provider).unwrap();
        // 应该使用推理模型
        assert_eq!(result["model"], "anthropic/claude-sonnet-4.5:extended");
    }

    #[test]
    fn test_thinking_parameter_disabled() {
        let mut provider = create_openrouter_provider();
        if let Some(env) = provider.settings_config.get_mut("env") {
            env["ANTHROPIC_REASONING_MODEL"] = json!("anthropic/claude-sonnet-4.5:extended");
        }

        let input = json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 1024,
            "thinking": {"type": "disabled"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input, &provider).unwrap();
        // 应该使用普通模型
        assert_eq!(result["model"], "anthropic/claude-sonnet-4.5");
    }
}
