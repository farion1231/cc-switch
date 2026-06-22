//! 反向格式转换：OpenAI → Anthropic（请求）与 Anthropic → OpenAI（响应）。
//!
//! 这是 Stage A（让 **Codex** 应用对接 **Claude/Anthropic** 上游）的转换基石，
//! 与 [`super::transform`] 里的正向转换（Anthropic → OpenAI 请求、OpenAI →
//! Anthropic 响应）互为逆操作。
//!
//! 目前仅实现并单测了 **非流式 Chat Completions** 方向。要让 Codex 端到端走通，
//! 还需补齐 Responses API 的反向转换以及双向流式 SSE 转换，并在 forwarder 里接好
//! 路由（endpoint 改写 + Anthropic 鉴权）。这些剩余部分必须对接真实的 Codex /
//! Claude 流量做验证，因此在能够端到端联调之前先以独立、可单测的纯函数形式落地。
#![allow(dead_code)]

use crate::proxy::error::ProxyError;
use crate::proxy::json_canonical::canonical_json_string;
use serde_json::{json, Value};

/// Anthropic 要求 `max_tokens` 必填；OpenAI 客户端常省略，给一个保守默认值。
const DEFAULT_ANTHROPIC_MAX_TOKENS: u64 = 4096;

/// OpenAI Chat Completions 请求 → Anthropic Messages 请求。
///
/// 与 [`super::transform::anthropic_to_openai`] 互为逆操作。模型映射由
/// proxy::model_mapper 统一处理，这里只做结构转换。
pub fn openai_chat_request_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();

    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            match role {
                // OpenAI 的 system / developer 指令折叠进 Anthropic 顶层 system。
                "system" | "developer" => {
                    if let Some(text) = openai_message_text(msg.get("content")) {
                        if !text.is_empty() {
                            system_parts.push(text);
                        }
                    }
                }
                // OpenAI tool 结果消息 → Anthropic user 消息里的 tool_result 块。
                "tool" => {
                    let tool_call_id = msg
                        .get("tool_call_id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    let content_text = openai_message_text(msg.get("content")).unwrap_or_default();
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": content_text
                        }]
                    }));
                }
                _ => {
                    let blocks = openai_message_to_anthropic_blocks(msg);
                    let anth_role = if role == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    };
                    if !blocks.is_empty() {
                        messages.push(json!({ "role": anth_role, "content": blocks }));
                    }
                }
            }
        }
    }

    if !system_parts.is_empty() {
        result["system"] = json!(system_parts.join("\n"));
    }
    result["messages"] = json!(messages);

    // Anthropic 必填 max_tokens；接受 max_tokens / max_completion_tokens(o-series)。
    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| body.get("max_completion_tokens").and_then(|v| v.as_u64()))
        .unwrap_or(DEFAULT_ANTHROPIC_MAX_TOKENS);
    result["max_tokens"] = json!(max_tokens);

    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(stop) = body.get("stop") {
        let seqs = match stop {
            Value::String(s) => vec![json!(s)],
            Value::Array(arr) => arr.clone(),
            _ => Vec::new(),
        };
        if !seqs.is_empty() {
            result["stop_sequences"] = json!(seqs);
        }
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // tools: OpenAI function → Anthropic tool（parameters → input_schema）。
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let anth_tools: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let func = t.get("function")?;
                let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                Some(json!({
                    "name": name,
                    "description": func.get("description"),
                    "input_schema": func
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object" }))
                }))
            })
            .collect();
        if !anth_tools.is_empty() {
            result["tools"] = json!(anth_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        if let Some(mapped) = map_tool_choice_to_anthropic(v) {
            result["tool_choice"] = mapped;
        }
    }

    Ok(result)
}

/// Anthropic Messages 响应 → OpenAI Chat Completions 响应（非流式）。
///
/// 与 [`super::transform::openai_to_anthropic`] 互为逆操作。
pub fn anthropic_response_to_openai_chat(body: Value) -> Result<Value, ProxyError> {
    let mut text_acc = String::new();
    let mut reasoning_acc = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();

    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_acc.push_str(text);
                    }
                }
                Some("thinking") => {
                    if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                        reasoning_acc.push_str(text);
                    }
                }
                Some("tool_use") => {
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": canonical_json_string(&input)
                        }
                    }));
                }
                _ => {}
            }
        }
    }

    // stop_reason → finish_reason（与 openai_to_anthropic 的逆映射一致）。
    let finish_reason = match body.get("stop_reason").and_then(|r| r.as_str()) {
        Some("end_turn") | Some("stop_sequence") => "stop",
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        _ if !tool_calls.is_empty() => "tool_calls",
        _ => "stop",
    };

    let mut message = json!({ "role": "assistant" });
    message["content"] = if text_acc.is_empty() {
        Value::Null
    } else {
        json!(text_acc)
    };
    if !reasoning_acc.is_empty() {
        message["reasoning_content"] = json!(reasoning_acc);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }

    // usage：Anthropic input/output(+cache) → OpenAI prompt/completion/total。
    // OpenAI prompt_tokens 含缓存命中，故把 cache_read/cache_creation 加回 input。
    let usage = body.get("usage").cloned().unwrap_or_else(|| json!({}));
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let prompt_tokens = input_tokens
        .saturating_add(cache_read)
        .saturating_add(cache_creation);
    let mut usage_json = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": prompt_tokens.saturating_add(output_tokens)
    });
    if cache_read > 0 {
        usage_json["prompt_tokens_details"] = json!({ "cached_tokens": cache_read });
    }

    Ok(json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "object": "chat.completion",
        "created": current_unix_secs(),
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": usage_json
    }))
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 从 OpenAI 消息 content（字符串或多段数组）中提取纯文本。
fn openai_message_text(content: Option<&Value>) -> Option<String> {
    match content {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|p| match p.get("type").and_then(|t| t.as_str()) {
                    Some("text") | Some("output_text") | None => {
                        p.get("text").and_then(|t| t.as_str()).map(str::to_string)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(text)
        }
        _ => None,
    }
}

/// 把一条 OpenAI user/assistant 消息转换成 Anthropic content 块。
fn openai_message_to_anthropic_blocks(msg: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();

    match msg.get("content") {
        Some(Value::String(s)) if !s.is_empty() => {
            blocks.push(json!({ "type": "text", "text": s }));
        }
        Some(Value::Array(parts)) => {
            for part in parts {
                match part.get("type").and_then(|t| t.as_str()) {
                    Some("text") | Some("output_text") => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                blocks.push(json!({ "type": "text", "text": text }));
                            }
                        }
                    }
                    Some("image_url") => {
                        if let Some(block) = openai_image_url_to_anthropic(part) {
                            blocks.push(block);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    // assistant 的 tool_calls → tool_use 块。
    if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let empty = json!({});
            let func = tc.get("function").unwrap_or(&empty);
            let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args_str = func
                .get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
            blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    blocks
}

/// OpenAI `image_url` 内容段 → Anthropic image 块。
/// 支持 `data:<media_type>;base64,<data>`；远程 URL 透传为 url source。
fn openai_image_url_to_anthropic(part: &Value) -> Option<Value> {
    let url = part.pointer("/image_url/url").and_then(|u| u.as_str())?;
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((media_type, data)) = rest.split_once(";base64,") {
            return Some(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": data
                }
            }));
        }
    }
    Some(json!({
        "type": "image",
        "source": { "type": "url", "url": url }
    }))
}

/// [`super::transform`] 里 `map_tool_choice_to_chat` 的逆操作：
/// OpenAI Chat tool_choice → Anthropic。
fn map_tool_choice_to_anthropic(tool_choice: &Value) -> Option<Value> {
    match tool_choice {
        Value::String(s) => match s.as_str() {
            "auto" => Some(json!({ "type": "auto" })),
            "required" => Some(json!({ "type": "any" })),
            "none" => Some(json!({ "type": "none" })),
            _ => None,
        },
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("function") {
                let name = obj
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                Some(json!({ "type": "tool", "name": name }))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::transform::{anthropic_to_openai, openai_to_anthropic};

    #[test]
    fn openai_chat_request_to_anthropic_maps_system_and_messages() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 256,
            "temperature": 0.5,
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"}
            ]
        });

        let out = openai_chat_request_to_anthropic(input).unwrap();
        assert_eq!(out["model"], "gpt-4o");
        assert_eq!(out["system"], "You are helpful.");
        assert_eq!(out["max_tokens"], 256);
        assert_eq!(out["temperature"], 0.5);
        assert_eq!(out["messages"][0]["role"], "user");
        assert_eq!(out["messages"][0]["content"][0]["type"], "text");
        assert_eq!(out["messages"][0]["content"][0]["text"], "Hi");
        assert_eq!(out["messages"][1]["role"], "assistant");
        assert_eq!(out["messages"][1]["content"][0]["text"], "Hello!");
    }

    #[test]
    fn openai_chat_request_to_anthropic_defaults_max_tokens() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let out = openai_chat_request_to_anthropic(input).unwrap();
        assert_eq!(out["max_tokens"], DEFAULT_ANTHROPIC_MAX_TOKENS);
    }

    #[test]
    fn openai_chat_request_to_anthropic_reads_max_completion_tokens() {
        let input = json!({
            "model": "o3",
            "max_completion_tokens": 999,
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let out = openai_chat_request_to_anthropic(input).unwrap();
        assert_eq!(out["max_tokens"], 999);
    }

    #[test]
    fn openai_chat_request_to_anthropic_converts_tools_and_tool_choice() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "weather?"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            }],
            "tool_choice": "required"
        });
        let out = openai_chat_request_to_anthropic(input).unwrap();
        assert_eq!(out["tools"][0]["name"], "get_weather");
        assert_eq!(out["tools"][0]["description"], "Get weather");
        assert_eq!(out["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(out["tool_choice"], json!({"type": "any"}));
    }

    #[test]
    fn openai_chat_request_to_anthropic_named_tool_choice() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}}
        });
        let out = openai_chat_request_to_anthropic(input).unwrap();
        assert_eq!(
            out["tool_choice"],
            json!({"type": "tool", "name": "get_weather"})
        );
    }

    #[test]
    fn openai_chat_request_to_anthropic_assistant_tool_calls_and_tool_result() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "weather in NYC?"},
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1", "content": "72F"}
            ]
        });
        let out = openai_chat_request_to_anthropic(input).unwrap();
        let assistant = &out["messages"][1];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["content"][0]["type"], "tool_use");
        assert_eq!(assistant["content"][0]["id"], "call_1");
        assert_eq!(assistant["content"][0]["name"], "get_weather");
        assert_eq!(assistant["content"][0]["input"]["city"], "NYC");
        let tool_msg = &out["messages"][2];
        assert_eq!(tool_msg["role"], "user");
        assert_eq!(tool_msg["content"][0]["type"], "tool_result");
        assert_eq!(tool_msg["content"][0]["tool_use_id"], "call_1");
        assert_eq!(tool_msg["content"][0]["content"], "72F");
    }

    #[test]
    fn openai_chat_request_to_anthropic_data_url_image() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "what is this?"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ]}]
        });
        let out = openai_chat_request_to_anthropic(input).unwrap();
        let blocks = &out["messages"][0]["content"];
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
        assert_eq!(blocks[1]["source"]["data"], "AAAA");
    }

    #[test]
    fn anthropic_response_to_openai_chat_text_and_usage() {
        let input = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-5-sonnet",
            "content": [{"type": "text", "text": "Hello there"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let out = anthropic_response_to_openai_chat(input).unwrap();
        assert_eq!(out["id"], "msg_123");
        assert_eq!(out["object"], "chat.completion");
        assert_eq!(out["model"], "claude-3-5-sonnet");
        assert_eq!(out["choices"][0]["message"]["role"], "assistant");
        assert_eq!(out["choices"][0]["message"]["content"], "Hello there");
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
        assert_eq!(out["usage"]["prompt_tokens"], 10);
        assert_eq!(out["usage"]["completion_tokens"], 5);
        assert_eq!(out["usage"]["total_tokens"], 15);
    }

    #[test]
    fn anthropic_response_to_openai_chat_tool_use_and_thinking() {
        let input = json!({
            "id": "msg_1",
            "model": "claude-3-5-sonnet",
            "content": [
                {"type": "thinking", "thinking": "let me think"},
                {"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {"city": "NYC"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 3, "output_tokens": 7}
        });
        let out = anthropic_response_to_openai_chat(input).unwrap();
        let msg = &out["choices"][0]["message"];
        assert!(msg["content"].is_null());
        assert_eq!(msg["reasoning_content"], "let me think");
        assert_eq!(msg["tool_calls"][0]["id"], "tu_1");
        assert_eq!(msg["tool_calls"][0]["type"], "function");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "get_weather");
        let args = msg["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let parsed: Value = serde_json::from_str(args).unwrap();
        assert_eq!(parsed["city"], "NYC");
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn anthropic_response_to_openai_chat_cache_tokens_folded_into_prompt() {
        let input = json!({
            "id": "msg_1",
            "model": "claude-3-5-sonnet",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 2,
                "cache_read_input_tokens": 90,
                "cache_creation_input_tokens": 0
            }
        });
        let out = anthropic_response_to_openai_chat(input).unwrap();
        assert_eq!(out["usage"]["prompt_tokens"], 100);
        assert_eq!(out["usage"]["prompt_tokens_details"]["cached_tokens"], 90);
        assert_eq!(out["usage"]["total_tokens"], 102);
    }

    #[test]
    fn reverse_chat_request_roundtrip_preserves_core_fields() {
        // Anthropic request -> OpenAI (forward) -> Anthropic (reverse) keeps essentials.
        let anth = json!({
            "model": "claude-3-5-sonnet",
            "max_tokens": 512,
            "system": "be terse",
            "messages": [{"role": "user", "content": "hello"}],
            "tools": [{
                "name": "get_weather",
                "description": "w",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}
            }]
        });
        let openai = anthropic_to_openai(anth).unwrap();
        let back = openai_chat_request_to_anthropic(openai).unwrap();
        assert_eq!(back["model"], "claude-3-5-sonnet");
        assert_eq!(back["system"], "be terse");
        assert_eq!(back["max_tokens"], 512);
        assert_eq!(back["messages"][0]["role"], "user");
        assert_eq!(back["messages"][0]["content"][0]["text"], "hello");
        assert_eq!(back["tools"][0]["name"], "get_weather");
        assert_eq!(back["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn reverse_chat_response_roundtrip_preserves_core_fields() {
        // OpenAI response -> Anthropic (forward) -> OpenAI (reverse) keeps essentials.
        let openai = json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hi back"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 8, "completion_tokens": 3, "total_tokens": 11}
        });
        let anth = openai_to_anthropic(openai).unwrap();
        let back = anthropic_response_to_openai_chat(anth).unwrap();
        assert_eq!(back["choices"][0]["message"]["content"], "Hi back");
        assert_eq!(back["choices"][0]["finish_reason"], "stop");
        assert_eq!(back["usage"]["prompt_tokens"], 8);
        assert_eq!(back["usage"]["completion_tokens"], 3);
    }
}
