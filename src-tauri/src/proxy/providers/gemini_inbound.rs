//! Gemini Inbound 协议转换模块
//!
//! 当 Gemini CLI 通过本地代理连接到非 Gemini 原生供应商（如 OpenAI Chat、OpenAI Responses、
//! Anthropic Messages）时，把 Gemini `generateContent` / `streamGenerateContent` 请求转换为
//! 目标协议格式，再把上游响应（JSON 或 SSE）转换回 Gemini `GenerateContentResponse`。

use std::pin::Pin;

use bytes::Bytes;
use futures::stream::{self, Stream, StreamExt};
use serde_json::{json, Value};

use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use crate::proxy::sse::SseBuffer;

/// 返回供应商配置中用于 Gemini inbound 转发的目标协议格式（默认 `"gemini_native"`）。
pub fn gemini_target_api_format(provider: &Provider) -> &'static str {
    match provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .map(str::trim)
    {
        Some("openai_chat") => "openai_chat",
        Some("openai_responses") => "openai_responses",
        Some("anthropic") => "anthropic",
        _ => "gemini_native",
    }
}

pub fn gemini_needs_inbound_transform(provider: &Provider) -> bool {
    gemini_target_api_format(provider) != "gemini_native"
}

/// 从 Gemini 请求或供应商配置中确定发往上游的模型名。
pub fn resolve_gemini_inbound_model(provider: &Provider, uri_model: Option<&str>) -> String {
    if let Some(configured) = provider
        .settings_config
        .pointer("/env/GEMINI_MODEL")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return configured.to_string();
    }
    uri_model
        .map(|m| m.strip_prefix("models/").unwrap_or(m).trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("gpt-4o")
        .to_string()
}

/// 把 Gemini `generateContent` 请求转换为 OpenAI Chat Completions 请求。
pub fn gemini_to_openai_chat(body: &Value, model: &str, stream: bool) -> Value {
    let mut messages = Vec::new();

    if let Some(sys_parts) = body
        .pointer("/systemInstruction/parts")
        .and_then(Value::as_array)
    {
        let sys_text: Vec<&str> = sys_parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect();
        if !sys_text.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": sys_text.join("\n")
            }));
        }
    }

    if let Some(contents) = body.get("contents").and_then(Value::as_array) {
        let mut tool_call_seq = 0usize;
        for content in contents {
            let role = match content.get("role").and_then(Value::as_str) {
                Some("model") => "assistant",
                _ => "user",
            };
            let Some(parts) = content.get("parts").and_then(Value::as_array) else {
                continue;
            };

            let mut text_chunks = Vec::new();
            let mut tool_calls = Vec::new();
            let mut tool_responses = Vec::new();

            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !part.get("thought").and_then(Value::as_bool).unwrap_or(false) {
                        text_chunks.push(text.to_string());
                    }
                } else if let Some(fc) = part.get("functionCall") {
                    let name = fc.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                    tool_call_seq += 1;
                    let id = fc
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("call_{tool_call_seq}"));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
                        }
                    }));
                } else if let Some(fr) = part.get("functionResponse") {
                    let name = fr.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let resp = fr.get("response").cloned().unwrap_or_else(|| json!({}));
                    let call_id = fr
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("call_{name}"));
                    tool_responses.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": serde_json::to_string(&resp).unwrap_or_default(),
                    }));
                }
            }

            if !tool_responses.is_empty() {
                messages.extend(tool_responses);
            }
            if !text_chunks.is_empty() || !tool_calls.is_empty() {
                let mut msg = json!({ "role": role });
                if !text_chunks.is_empty() {
                    msg["content"] = Value::String(text_chunks.join(""));
                } else if role == "assistant" {
                    msg["content"] = Value::Null;
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = Value::Array(tool_calls);
                }
                messages.push(msg);
            }
        }
    }

    let mut out = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if let Some(gen_cfg) = body.get("generationConfig") {
        if let Some(temp) = gen_cfg.get("temperature") {
            out["temperature"] = temp.clone();
        }
        if let Some(top_p) = gen_cfg.get("topP") {
            out["top_p"] = top_p.clone();
        }
        if let Some(max_tokens) = gen_cfg.get("maxOutputTokens") {
            out["max_tokens"] = max_tokens.clone();
        }
        if let Some(stop) = gen_cfg.get("stopSequences") {
            out["stop"] = stop.clone();
        }
    }

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let mut openai_tools = Vec::new();
        for tool in tools {
            if let Some(decls) = tool.get("functionDeclarations").and_then(Value::as_array) {
                for decl in decls {
                    openai_tools.push(json!({
                        "type": "function",
                        "function": {
                            "name": decl.get("name").and_then(Value::as_str).unwrap_or(""),
                            "description": decl.get("description").and_then(Value::as_str).unwrap_or(""),
                            "parameters": decl.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                        }
                    }));
                }
            }
        }
        if !openai_tools.is_empty() {
            out["tools"] = Value::Array(openai_tools);
        }
    }

    out
}

/// 把 Gemini `generateContent` 请求转换为 Anthropic Messages 请求。
pub fn gemini_to_anthropic(body: &Value, model: &str, stream: bool) -> Value {
    let mut chat = gemini_to_openai_chat(body, model, stream);
    let mut system_text = None;
    let mut anthropic_messages = Vec::new();

    if let Some(msgs) = chat.get_mut("messages").and_then(Value::as_array_mut) {
        for msg in msgs.iter() {
            let role = msg.get("role").and_then(Value::as_str).unwrap_or("user");
            if role == "system" {
                system_text = msg.get("content").and_then(Value::as_str).map(str::to_string);
                continue;
            }
            if role == "tool" {
                let tool_use_id = msg
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("call_1");
                let content = msg.get("content").and_then(Value::as_str).unwrap_or("");
                anthropic_messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }]
                }));
                continue;
            }
            let mut blocks = Vec::new();
            if let Some(text) = msg
                .get("content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                blocks.push(json!({ "type": "text", "text": text }));
            }
            if let Some(tcs) = msg.get("tool_calls").and_then(Value::as_array) {
                for tc in tcs {
                    let id = tc.get("id").and_then(Value::as_str).unwrap_or("call_1");
                    let name = tc
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool");
                    let args_str = tc
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
            }
            if !blocks.is_empty() {
                anthropic_messages.push(json!({
                    "role": role,
                    "content": blocks,
                }));
            }
        }
    }

    let max_tokens = chat
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(8192);
    let mut out = json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": anthropic_messages,
        "stream": stream,
    });
    if let Some(sys) = system_text {
        out["system"] = Value::String(sys);
    }
    if let Some(temp) = chat.get("temperature") {
        out["temperature"] = temp.clone();
    }
    if let Some(tools) = chat.get("tools").and_then(Value::as_array) {
        let anthropic_tools: Vec<Value> = tools
            .iter()
            .filter_map(|t| t.get("function"))
            .map(|f| {
                json!({
                    "name": f.get("name").and_then(Value::as_str).unwrap_or(""),
                    "description": f.get("description").and_then(Value::as_str).unwrap_or(""),
                    "input_schema": f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"})),
                })
            })
            .collect();
        if !anthropic_tools.is_empty() {
            out["tools"] = Value::Array(anthropic_tools);
        }
    }
    out
}

/// 把 OpenAI Chat Completions 响应（非流式）转换为 Gemini `GenerateContentResponse`。
pub fn openai_chat_response_to_gemini(resp: &Value) -> Value {
    let mut parts = Vec::new();
    let choice = resp.pointer("/choices/0");
    if let Some(content) = choice
        .and_then(|c| c.pointer("/message/content"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        parts.push(json!({ "text": content }));
    }
    if let Some(tcs) = choice
        .and_then(|c| c.pointer("/message/tool_calls"))
        .and_then(Value::as_array)
    {
        for tc in tcs {
            let name = tc
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args_str = tc
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let args: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
            parts.push(json!({
                "functionCall": {
                    "name": name,
                    "args": args,
                }
            }));
        }
    }
    let finish_reason = match choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
    {
        Some("length") => "MAX_TOKENS",
        _ => "STOP",
    };

    let prompt_tokens = resp
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completion_tokens = resp
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": parts,
            },
            "finishReason": finish_reason,
            "index": 0,
        }],
        "usageMetadata": {
            "promptTokenCount": prompt_tokens,
            "candidatesTokenCount": completion_tokens,
            "totalTokenCount": prompt_tokens + completion_tokens,
        }
    })
}

/// 把 Anthropic Messages 响应（非流式）转换为 Gemini `GenerateContentResponse`。
pub fn anthropic_response_to_gemini(resp: &Value) -> Value {
    let mut parts = Vec::new();
    if let Some(content) = resp.get("content").and_then(Value::as_array) {
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        parts.push(json!({ "text": text }));
                    }
                }
                Some("tool_use") => {
                    parts.push(json!({
                        "functionCall": {
                            "name": block.get("name").and_then(Value::as_str).unwrap_or(""),
                            "args": block.get("input").cloned().unwrap_or_else(|| json!({})),
                        }
                    }));
                }
                _ => {}
            }
        }
    }
    let finish_reason = match resp.get("stop_reason").and_then(Value::as_str) {
        Some("max_tokens") => "MAX_TOKENS",
        _ => "STOP",
    };
    let input_tokens = resp
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = resp
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": parts,
            },
            "finishReason": finish_reason,
            "index": 0,
        }],
        "usageMetadata": {
            "promptTokenCount": input_tokens,
            "candidatesTokenCount": output_tokens,
            "totalTokenCount": input_tokens + output_tokens,
        }
    })
}

/// 把 OpenAI Chat SSE 流转换为 Gemini SSE 流。
pub fn create_gemini_sse_stream_from_openai_chat(
    upstream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    let mut sse = SseBuffer::new();
    let s = upstream.flat_map(move |chunk| {
        let mut out: Vec<Result<Bytes, std::io::Error>> = Vec::new();
        match chunk {
            Ok(bytes) => {
                sse.push_bytes(&bytes);
                while let Some(block) = sse.next_block() {
                    for line in block.lines() {
                        let Some(data) = line.strip_prefix("data:") else {
                            continue;
                        };
                        let data = data.trim();
                        if data == "[DONE]" {
                            continue;
                        }
                        if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                            let mut parts = Vec::new();
                            if let Some(delta_text) = parsed
                                .pointer("/choices/0/delta/content")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                            {
                                parts.push(json!({ "text": delta_text }));
                            }
                            let finish = parsed
                                .pointer("/choices/0/finish_reason")
                                .and_then(Value::as_str);
                            if !parts.is_empty() || finish.is_some() {
                                let mut cand = json!({
                                    "content": { "role": "model", "parts": parts },
                                    "index": 0,
                                });
                                if let Some(reason) = finish {
                                    cand["finishReason"] = Value::String(
                                        if reason == "length" {
                                            "MAX_TOKENS"
                                        } else {
                                            "STOP"
                                        }
                                        .to_string(),
                                    );
                                }
                                let payload = json!({ "candidates": [cand] });
                                out.push(Ok(Bytes::from(format!("data: {payload}\n\n"))));
                            }
                        }
                    }
                }
            }
            Err(e) => out.push(Err(e)),
        }
        stream::iter(out)
    });
    Box::pin(s)
}

/// 把 Anthropic Messages SSE 流转换为 Gemini SSE 流。
pub fn create_gemini_sse_stream_from_anthropic(
    upstream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
    let mut sse = SseBuffer::new();
    let s = upstream.flat_map(move |chunk| {
        let mut out: Vec<Result<Bytes, std::io::Error>> = Vec::new();
        match chunk {
            Ok(bytes) => {
                sse.push_bytes(&bytes);
                while let Some(block) = sse.next_block() {
                    for line in block.lines() {
                        let Some(data) = line.strip_prefix("data:") else {
                            continue;
                        };
                        let data = data.trim();
                        if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                            match parsed.get("type").and_then(Value::as_str) {
                                Some("content_block_delta") => {
                                    if let Some(text) = parsed
                                        .pointer("/delta/text")
                                        .and_then(Value::as_str)
                                        .filter(|s| !s.is_empty())
                                    {
                                        let payload = json!({
                                            "candidates": [{
                                                "content": { "role": "model", "parts": [{ "text": text }] },
                                                "index": 0,
                                            }]
                                        });
                                        out.push(Ok(Bytes::from(format!("data: {payload}\n\n"))));
                                    }
                                }
                                Some("message_delta") => {
                                    if parsed.pointer("/delta/stop_reason").is_some() {
                                        let payload = json!({
                                            "candidates": [{
                                                "content": { "role": "model", "parts": [] },
                                                "finishReason": "STOP",
                                                "index": 0,
                                            }]
                                        });
                                        out.push(Ok(Bytes::from(format!("data: {payload}\n\n"))));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Err(e) => out.push(Err(e)),
        }
        stream::iter(out)
    });
    Box::pin(s)
}

pub fn endpoint_is_gemini_stream(endpoint: &str) -> bool {
    endpoint.contains(":streamGenerateContent")
}

pub fn endpoint_is_gemini_models_list(method: &http::Method, endpoint: &str) -> bool {
    *method == http::Method::GET
        && endpoint
            .split('?')
            .next()
            .is_some_and(|p| p.ends_with("/models"))
}

pub fn synthetic_gemini_models_response(model: &str) -> Value {
    json!({
        "models": [{
            "name": format!("models/{model}"),
            "displayName": model,
            "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
        }]
    })
}

pub fn convert_gemini_inbound_request(
    provider: &Provider,
    body: &Value,
    uri_model: Option<&str>,
    is_stream: bool,
) -> Result<(String, Value), ProxyError> {
    let target_format = gemini_target_api_format(provider);
    let model = resolve_gemini_inbound_model(provider, uri_model);
    match target_format {
        "anthropic" => Ok(("/v1/messages".to_string(), gemini_to_anthropic(body, &model, is_stream))),
        "openai_chat" => Ok((
            "/v1/chat/completions".to_string(),
            gemini_to_openai_chat(body, &model, is_stream),
        )),
        "openai_responses" => {
            // 复用 Chat → Responses 或 Anthropic → Responses 链路：先转 OpenAI Chat
            Ok((
                "/v1/chat/completions".to_string(),
                gemini_to_openai_chat(body, &model, is_stream),
            ))
        }
        _ => Err(ProxyError::ConfigError(format!(
            "Unsupported Gemini inbound target format: {target_format}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_gemini_request_to_openai_chat_and_back() {
        let req = json!({
            "systemInstruction": { "parts": [{ "text": "Be concise." }] },
            "contents": [{ "role": "user", "parts": [{ "text": "Hello" }] }],
            "generationConfig": { "temperature": 0.2, "maxOutputTokens": 128 }
        });
        let chat = gemini_to_openai_chat(&req, "gpt-4o", false);
        assert_eq!(chat["model"], "gpt-4o");
        assert_eq!(chat["messages"][0]["role"], "system");
        assert_eq!(chat["messages"][1]["content"], "Hello");

        let resp = json!({
            "choices": [{ "message": { "content": "Hi!" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
        });
        let gemini = openai_chat_response_to_gemini(&resp);
        assert_eq!(
            gemini["candidates"][0]["content"]["parts"][0]["text"],
            "Hi!"
        );
        assert_eq!(gemini["candidates"][0]["finishReason"], "STOP");
    }

    #[test]
    fn converts_gemini_request_to_anthropic_and_back() {
        let req = json!({
            "systemInstruction": { "parts": [{ "text": "System prompt" }] },
            "contents": [
                { "role": "user", "parts": [{ "text": "Hi" }] },
                { "role": "model", "parts": [{ "text": "Hello" }] }
            ],
            "generationConfig": { "temperature": 0.5, "maxOutputTokens": 256 }
        });
        let anth = gemini_to_anthropic(&req, "claude-sonnet-4-20250514", false);
        assert_eq!(anth["model"], "claude-sonnet-4-20250514");
        assert_eq!(anth["system"], "System prompt");
        assert_eq!(anth["messages"][0]["role"], "user");
        assert_eq!(anth["messages"][1]["role"], "assistant");

        let resp = json!({
            "content": [{ "type": "text", "text": "World" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 4 }
        });
        let gemini = anthropic_response_to_gemini(&resp);
        assert_eq!(
            gemini["candidates"][0]["content"]["parts"][0]["text"],
            "World"
        );
        assert_eq!(gemini["candidates"][0]["finishReason"], "STOP");
    }

    #[tokio::test]
    async fn converts_openai_and_anthropic_sse_to_gemini_sse() {
        let openai_chunks = stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(
            "data: {\"choices\":[{\"delta\":{\"content\":\"abc\"},\"finish_reason\":\"stop\"}]}\n\n",
        ))]);
        let mut out_openai = create_gemini_sse_stream_from_openai_chat(openai_chunks);
        let mut text_openai = String::new();
        while let Some(chunk) = out_openai.next().await {
            text_openai.push_str(&String::from_utf8(chunk.unwrap().to_vec()).unwrap());
        }
        assert!(text_openai.contains("\"text\":\"abc\""));
        assert!(text_openai.contains("\"finishReason\":\"STOP\""));

        let anth_chunks = stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"xyz\"}}\n\n",
            )),
            Ok::<Bytes, std::io::Error>(Bytes::from(
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            )),
        ]);
        let mut out_anth = create_gemini_sse_stream_from_anthropic(anth_chunks);
        let mut text_anth = String::new();
        while let Some(chunk) = out_anth.next().await {
            text_anth.push_str(&String::from_utf8(chunk.unwrap().to_vec()).unwrap());
        }
        assert!(text_anth.contains("\"text\":\"xyz\""));
        assert!(text_anth.contains("\"finishReason\":\"STOP\""));
    }
}
