//! 格式转换模块
//!
//! 实现 Anthropic ↔ OpenAI 格式转换，用于 OpenRouter 支持
//! 实现 Anthropic → Harmony 格式转换，用于 gpt-oss 模型支持
//! 参考: anthropic-proxy-rs

use crate::proxy::error::ProxyError;
use serde_json::{json, Map, Value};

/// Anthropic 请求 → OpenAI 请求
pub fn anthropic_to_openai(body: Value) -> Result<Value, ProxyError> {
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

    // 文本内容 (优先使用 content，其次使用 reasoning/reasoning_content)
    let text_content = message
        .get("content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| message.get("reasoning").and_then(|r| r.as_str()))
        .or_else(|| message.get("reasoning_content").and_then(|r| r.as_str()));

    if let Some(text) = text_content {
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

// ============================================================================
// Harmony Responses API Transformations
// ============================================================================

/// Anthropic Messages API → Harmony Responses API format
///
/// The Responses API is OpenAI's unified API used by gpt-oss models.
/// Key differences from Chat Completions:
/// - `input` instead of `messages` (can be string or array)
/// - `instructions` for system prompt (extracted from system message)
/// - `max_output_tokens` instead of `max_tokens`
/// - Response uses `output` array instead of `choices`
pub fn anthropic_to_harmony(body: Value) -> Result<Value, ProxyError> {
    let mut new_body = Map::new();

    // Copy model name
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        new_body.insert("model".to_string(), json!(model));
    }

    // Extract instructions from system message and transform messages to input
    let mut instructions = String::new();
    let mut input_messages: Vec<Value> = Vec::new();

    // Handle system prompt (Anthropic uses top-level "system" field)
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            instructions = text.to_string();
        } else if let Some(arr) = system.as_array() {
            // Multi-part system message
            let texts: Vec<&str> = arr
                .iter()
                .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                .collect();
            if !texts.is_empty() {
                instructions = texts.join("\n\n");
            }
        }
    }

    // Transform messages to input format
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content").cloned().unwrap_or(json!(""));

            // Skip system messages in input (they go to instructions)
            if role == "system" {
                if instructions.is_empty() {
                    if let Some(text) = content.as_str() {
                        instructions = text.to_string();
                    }
                }
                continue;
            }

            // Convert Anthropic content blocks to Harmony format
            let harmony_content = convert_anthropic_content_to_harmony(&content);
            input_messages.push(json!({
                "role": role,
                "content": harmony_content,
            }));
        }
    }

    // Set instructions if we found a system message
    if !instructions.is_empty() {
        new_body.insert("instructions".to_string(), json!(instructions));
    }

    // Set input (use array format for multi-turn conversations)
    if !input_messages.is_empty() {
        new_body.insert("input".to_string(), json!(input_messages));
    } else {
        new_body.insert("input".to_string(), json!(""));
    }

    // Transform parameter names
    if let Some(v) = body
        .get("max_tokens")
        .or_else(|| body.get("max_output_tokens"))
    {
        new_body.insert("max_output_tokens".to_string(), v.clone());
    }

    // Copy other standard parameters
    for key in ["temperature", "top_p", "stream", "stop"] {
        if let Some(v) = body.get(key) {
            new_body.insert(key.to_string(), v.clone());
        }
    }

    // Handle tools - Harmony uses similar tool definitions
    if let Some(tools) = body.get("tools") {
        let harmony_tools = convert_anthropic_tools_to_harmony(tools);
        if !harmony_tools.is_null() {
            new_body.insert("tools".to_string(), harmony_tools);
        }
    }

    // Handle tool_choice
    if let Some(v) = body.get("tool_choice") {
        new_body.insert(
            "tool_choice".to_string(),
            convert_anthropic_tool_choice_to_harmony(v),
        );
    }

    Ok(json!(new_body))
}

/// Convert Anthropic content to Harmony format
fn convert_anthropic_content_to_harmony(content: &Value) -> Value {
    // String content passes through
    if let Some(text) = content.as_str() {
        return json!(text);
    }

    // Array of content blocks
    if let Some(blocks) = content.as_array() {
        // Nvidia Responses API expects string input for plain text messages.
        // When Anthropic sends multi-part text blocks (common with injected reminders),
        // flatten them into one string to avoid `input` schema rejection.
        let mut all_text = true;
        let mut flattened = String::new();
        for block in blocks {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        flattened.push_str(text);
                    }
                }
                _ => {
                    all_text = false;
                    break;
                }
            }
        }

        if all_text {
            return json!(flattened);
        }

        let parts: Vec<Value> = blocks
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

                match block_type {
                    "text" => block
                        .get("text")
                        .map(|t| json!({"type": "text", "text": t})),
                    "image" => {
                        // Convert Anthropic image format to OpenAI format
                        if let Some(source) = block.get("source") {
                            let media_type = source
                                .get("media_type")
                                .and_then(|m| m.as_str())
                                .unwrap_or("image/png");
                            let data = source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                            Some(json!({
                                "type": "image_url",
                                "image_url": {"url": format!("data:{};base64,{}", media_type, data)}
                            }))
                        } else {
                            None
                        }
                    }
                    "tool_use" => {
                        // Tool calls in assistant messages
                        Some(block.clone())
                    }
                    "tool_result" => {
                        // Tool results need special handling - they become separate messages
                        // For now, pass through as-is
                        Some(block.clone())
                    }
                    _ => None,
                }
            })
            .collect();

        if parts.len() == 1 {
            // Single part - simplify
            if let Some(text) = parts[0].get("text") {
                return text.clone();
            }
        }

        return json!(parts);
    }

    // Default: pass through
    content.clone()
}

/// Convert Anthropic tools to Harmony format
fn convert_anthropic_tools_to_harmony(tools: &Value) -> Value {
    if let Some(tools_arr) = tools.as_array() {
        let harmony_tools: Vec<Value> = tools_arr
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|t| {
                // Anthropic format: {name, description, input_schema}
                // Harmony format: {type: "function", name, description, parameters}
                json!({
                    "type": "function",
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": t.get("description").cloned().unwrap_or(json!("")),
                    "parameters": clean_schema(t.get("input_schema").cloned().unwrap_or(json!({})))
                })
            })
            .collect();

        if harmony_tools.is_empty() {
            return Value::Null;
        }
        return json!(harmony_tools);
    }
    tools.clone()
}

/// Convert Anthropic tool_choice to Harmony format
fn convert_anthropic_tool_choice_to_harmony(tool_choice: &Value) -> Value {
    if let Some(choice_str) = tool_choice.as_str() {
        return match choice_str {
            "auto" | "none" | "required" => json!(choice_str),
            _ => tool_choice.clone(),
        };
    }

    if let Some(obj) = tool_choice.as_object() {
        match obj.get("type").and_then(|v| v.as_str()) {
            Some("auto") => return json!("auto"),
            Some("any") => return json!("required"),
            Some("tool") => {
                if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                    return json!({"type": "function", "name": name});
                }
            }
            _ => {}
        }
    }

    tool_choice.clone()
}

/// Harmony Responses API → Anthropic Messages API format
///
/// Converts the Harmony response back to Anthropic format for the client.
pub fn harmony_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    // Harmony response has `output` array instead of `choices`
    let output = body
        .get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| ProxyError::TransformError("No output in Harmony response".to_string()))?;

    // Find the main message in output
    let mut content: Vec<Value> = Vec::new();
    let mut stop_reason: Option<&str> = None;

    for item in output {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match item_type {
            "message" => {
                // Main assistant message
                if let Some(msg_content) = item.get("content") {
                    if let Some(arr) = msg_content.as_array() {
                        for part in arr {
                            let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match part_type {
                                "text" => {
                                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                        if !text.is_empty() {
                                            content.push(json!({"type": "text", "text": text}));
                                        }
                                    }
                                }
                                "tool_call" | "function_call" => {
                                    // Tool use
                                    let id = part.get("id").and_then(|i| i.as_str()).unwrap_or("");
                                    let name = part
                                        .get("name")
                                        .or_else(|| {
                                            part.get("function").and_then(|f| f.get("name"))
                                        })
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("");
                                    let args = part
                                        .get("arguments")
                                        .or_else(|| {
                                            part.get("function").and_then(|f| f.get("arguments"))
                                        })
                                        .cloned()
                                        .unwrap_or(json!({}));
                                    let input: Value = if let Some(args_str) = args.as_str() {
                                        serde_json::from_str(args_str).unwrap_or(json!({}))
                                    } else {
                                        args
                                    };
                                    content.push(json!({
                                        "type": "tool_use",
                                        "id": id,
                                        "name": name,
                                        "input": input
                                    }));
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(text) = msg_content.as_str() {
                        if !text.is_empty() {
                            content.push(json!({"type": "text", "text": text}));
                        }
                    }
                }
            }
            "function_call" => {
                // Standalone function call
                let id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = item.get("arguments").cloned().unwrap_or(json!({}));
                let input: Value = if let Some(args_str) = args.as_str() {
                    serde_json::from_str(args_str).unwrap_or(json!({}))
                } else {
                    args
                };
                content.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input
                }));
                stop_reason = Some("tool_use");
            }
            _ => {}
        }
    }

    // Get finish reason from status or content
    if stop_reason.is_none() {
        stop_reason = body
            .get("status")
            .and_then(|s| s.as_str())
            .map(|s| match s {
                "completed" => "end_turn",
                "incomplete" => "max_tokens",
                _ => s,
            });
    }

    // Usage
    let usage = body.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
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

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["model"], "gpt-4o");
    }

    #[test]
    fn test_anthropic_to_harmony_tools_use_top_level_function_fields() {
        let input = json!({
            "model": "openai/gpt-oss-20b",
            "messages": [{"role": "user", "content": "hello"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get current weather",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string", "format": "uri"}
                    }
                }
            }]
        });

        let result = anthropic_to_harmony(input).unwrap();
        let tool = &result["tools"][0];

        assert_eq!(tool["type"], "function");
        assert_eq!(tool["name"], "get_weather");
        assert_eq!(tool["description"], "Get current weather");
        assert_eq!(tool["parameters"]["type"], "object");
        assert!(tool.get("function").is_none());
        assert!(tool["parameters"]["properties"]["location"]
            .get("format")
            .is_none());
    }

    #[test]
    fn test_anthropic_to_harmony_tool_choice_mappings() {
        let mk_input = |tool_choice: Value| {
            json!({
                "model": "openai/gpt-oss-20b",
                "messages": [{"role": "user", "content": "hello"}],
                "tool_choice": tool_choice
            })
        };

        let auto = anthropic_to_harmony(mk_input(json!({"type": "auto"}))).unwrap();
        assert_eq!(auto["tool_choice"], "auto");

        let any = anthropic_to_harmony(mk_input(json!({"type": "any"}))).unwrap();
        assert_eq!(any["tool_choice"], "required");

        let specific =
            anthropic_to_harmony(mk_input(json!({"type": "tool", "name": "lookup"}))).unwrap();
        assert_eq!(specific["tool_choice"]["type"], "function");
        assert_eq!(specific["tool_choice"]["name"], "lookup");

        let pass_auto = anthropic_to_harmony(mk_input(json!("auto"))).unwrap();
        assert_eq!(pass_auto["tool_choice"], "auto");

        let pass_none = anthropic_to_harmony(mk_input(json!("none"))).unwrap();
        assert_eq!(pass_none["tool_choice"], "none");

        let pass_required = anthropic_to_harmony(mk_input(json!("required"))).unwrap();
        assert_eq!(pass_required["tool_choice"], "required");
    }

    #[test]
    fn test_anthropic_to_harmony_flattens_multi_text_blocks() {
        let input = json!({
            "model": "openai/gpt-oss-20b",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello "},
                    {"type": "text", "text": "world"}
                ]
            }]
        });

        let result = anthropic_to_harmony(input).unwrap();
        assert_eq!(result["input"][0]["content"], "Hello world");
    }
}
