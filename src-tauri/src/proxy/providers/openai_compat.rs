//! OpenAI v1 compatible bridge for Codex OAuth upstreams.
//!
//! The public side of CC Switch accepts ordinary OpenAI Chat Completions
//! requests.  When a routed Codex provider uses managed ChatGPT/Codex OAuth,
//! the real upstream only accepts Codex Responses SSE, so this module performs
//! the narrow wire conversion needed to keep external agents OpenAI-compatible.

use crate::proxy::{
    error::ProxyError,
    json_canonical::{canonical_json_string, canonicalize_json_string_if_parseable},
    sse::{append_utf8_safe, strip_sse_field, take_sse_block},
};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Value};

/// 将 OpenAI Chat Completions 请求转换为 ChatGPT Codex 后端接受的 Responses 请求。
///
/// 参数:
/// - `body`: 外部 agent 发送的 `/v1/chat/completions` JSON。
///
/// 返回:
/// - 可直接转发到 `/backend-api/codex/responses` 的 JSON。
///
/// 副作用:
/// - 无。该函数只做结构转换，不读取凭据，也不访问外部服务。
pub fn chat_completions_request_to_codex_responses(body: Value) -> Result<Value, ProxyError> {
    let messages = body
        .get("messages")
        .and_then(|value| value.as_array())
        .ok_or_else(|| ProxyError::TransformError("Chat request missing messages".to_string()))?;

    let mut instructions = Vec::new();
    let mut input = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("user");

        match role {
            "system" | "developer" => {
                if let Some(text) = chat_message_text(message.get("content")) {
                    if !text.trim().is_empty() {
                        instructions.push(text);
                    }
                }
            }
            "tool" => input.push(chat_tool_message_to_response_item(message)),
            "assistant" => append_chat_assistant_message(message, &mut input),
            _ => input.push(chat_user_message_to_response_message(role, message)),
        }
    }

    // ChatGPT Codex Responses 后端要求 instructions 非空；很多第三方 OpenAI SDK
    // 只发送 user message，没有 system/developer 消息，所以这里补一个最小默认值。
    let instructions = if instructions.is_empty() {
        "You are a helpful assistant.".to_string()
    } else {
        instructions.join("\n\n")
    };

    let mut result = json!({
        "model": body.get("model").cloned().unwrap_or_else(|| json!("gpt-5.4-mini")),
        "instructions": instructions,
        "input": input,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "tools": chat_tools_to_responses_tools(body.get("tools")),
        "parallel_tool_calls": body
            .get("parallel_tool_calls")
            .cloned()
            .unwrap_or_else(|| json!(false)),
        "stream": true
    });

    if let Some(tool_choice) = body.get("tool_choice") {
        result["tool_choice"] = chat_tool_choice_to_responses(tool_choice);
    }
    if let Some(service_tier) = body.get("service_tier") {
        result["service_tier"] = service_tier.clone();
    }
    if let Some(metadata) = body.get("metadata") {
        result["metadata"] = metadata.clone();
    }

    Ok(result)
}

/// 将非流式 Responses 响应转换为标准 OpenAI Chat Completions 响应。
///
/// 参数:
/// - `response`: 已聚合出的 Responses JSON。
/// - `fallback_model`: 上游响应缺少 model 时使用的请求模型名。
///
/// 返回:
/// - OpenAI SDK 可直接解析的 `chat.completion` JSON。
pub fn codex_responses_to_chat_completion(
    response: Value,
    fallback_model: &str,
) -> Result<Value, ProxyError> {
    if response.get("error").is_some_and(|error| !error.is_null()) {
        return Ok(responses_error_to_chat_error(Some(&response)));
    }

    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_parts = Vec::new();

    if let Some(output) = response.get("output").and_then(|value| value.as_array()) {
        for item in output {
            collect_response_output_item(
                item,
                &mut content_parts,
                &mut tool_calls,
                &mut reasoning_parts,
            );
        }
    }

    let mut message = json!({
        "role": "assistant",
        "content": content_parts.join("")
    });

    if content_parts.is_empty() && !tool_calls.is_empty() {
        message["content"] = Value::Null;
    }
    let has_tool_calls = !tool_calls.is_empty();
    if has_tool_calls {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    if !reasoning_parts.is_empty() {
        message["reasoning_content"] = json!(reasoning_parts.join("\n\n"));
    }

    let finish_reason = finish_reason_from_response(&response, has_tool_calls);
    let model = response
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_model);

    Ok(json!({
        "id": chat_id_from_response_id(response.get("id").and_then(|value| value.as_str())),
        "object": "chat.completion",
        "created": response
            .get("created_at")
            .or_else(|| response.get("created"))
            .and_then(|value| value.as_u64())
            .unwrap_or_else(current_unix_timestamp),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": responses_usage_to_chat_usage(response.get("usage"))
    }))
}

/// 将 Responses API 错误体规整为 OpenAI-compatible 错误体。
///
/// 参数:
/// - `body`: 上游错误 JSON；为空时生成代理层兜底错误。
///
/// 返回:
/// - `{"error": {...}}` 形状的 JSON。
pub fn responses_error_to_chat_error(body: Option<&Value>) -> Value {
    let Some(body) = body else {
        return json!({
            "error": {
                "message": "Upstream returned an empty error response",
                "type": "upstream_error",
                "code": null,
                "param": null
            }
        });
    };

    if let Some(error) = body.get("error") {
        return json!({ "error": normalize_error_object(error) });
    }
    if let Some(error) = body.pointer("/response/error") {
        return json!({ "error": normalize_error_object(error) });
    }

    json!({
        "error": {
            "message": body
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("Upstream Codex OAuth request failed"),
            "type": body
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("upstream_error"),
            "code": body.get("code").cloned().unwrap_or(Value::Null),
            "param": body.get("param").cloned().unwrap_or(Value::Null)
        }
    })
}

/// 创建把 Responses SSE 实时转换成 OpenAI Chat Completions SSE 的流。
///
/// 参数:
/// - `stream`: 上游 Codex Responses SSE 字节流。
/// - `fallback_model`: 上游早期事件缺少 model 时使用的请求模型。
///
/// 返回:
/// - 对外兼容 OpenAI SDK streaming parser 的 SSE 字节流。
pub fn create_chat_sse_stream_from_codex_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    fallback_model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    async_stream::stream! {
        let mut state = ResponsesToChatSseState::new(fallback_model);
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut failed = false;

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, bytes.as_ref());
                    while let Some(block) = take_sse_block(&mut buffer) {
                        match state.handle_sse_block(&block) {
                            Ok(events) => {
                                for event in events {
                                    yield Ok(event);
                                }
                            }
                            Err(err) => {
                                failed = true;
                                yield Ok(chat_sse_data(json!({
                                    "error": {
                                        "message": err.to_string(),
                                        "type": "stream_error",
                                        "code": null,
                                        "param": null
                                    }
                                })));
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    failed = true;
                    yield Ok(chat_sse_data(json!({
                        "error": {
                            "message": err.to_string(),
                            "type": "stream_error",
                            "code": null,
                            "param": null
                        }
                    })));
                    break;
                }
            }
        }

        if !failed && !state.done {
            for event in state.finish("stop") {
                yield Ok(event);
            }
        }
        yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));
    }
}

#[derive(Debug)]
struct ResponsesToChatSseState {
    id: String,
    model: String,
    created: u64,
    role_sent: bool,
    done: bool,
    next_tool_index: usize,
    emitted_tool_call: bool,
    usage: Option<Value>,
}

impl ResponsesToChatSseState {
    /// 创建 Responses SSE 转 Chat SSE 的状态机。
    ///
    /// 参数:
    /// - `fallback_model`: response.created 到达前用于 chunk 的模型名。
    fn new(fallback_model: String) -> Self {
        Self {
            id: "chatcmpl_ccswitch".to_string(),
            model: fallback_model,
            created: current_unix_timestamp(),
            role_sent: false,
            done: false,
            next_tool_index: 0,
            emitted_tool_call: false,
            usage: None,
        }
    }

    /// 处理一个完整 SSE block，并返回需要下发给 OpenAI 客户端的 chunks。
    ///
    /// 参数:
    /// - `block`: 不含空行分隔符的 SSE 文本块。
    fn handle_sse_block(&mut self, block: &str) -> Result<Vec<Bytes>, ProxyError> {
        let (event_name, data) = parse_sse_block(block)?;
        match event_name.as_deref() {
            Some("response.created") => {
                if let Some(response) = data.get("response") {
                    self.apply_response_metadata(response);
                } else {
                    self.apply_response_metadata(&data);
                }
                Ok(self.ensure_role_chunk())
            }
            Some("response.output_text.delta") => {
                let delta = data
                    .get("delta")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let mut events = self.ensure_role_chunk();
                if !delta.is_empty() {
                    events.push(self.delta_chunk(json!({ "content": delta }), None, None));
                }
                Ok(events)
            }
            Some("response.output_item.done") => {
                let Some(item) = data.get("item") else {
                    return Ok(Vec::new());
                };
                let mut events = self.ensure_role_chunk();
                if let Some(tool_call) = self.tool_call_delta_from_item(item) {
                    events.push(self.delta_chunk(json!({ "tool_calls": [tool_call] }), None, None));
                }
                Ok(events)
            }
            Some("response.completed") => {
                if let Some(response) = data.get("response") {
                    self.apply_response_metadata(response);
                    self.usage = Some(responses_usage_to_chat_usage(response.get("usage")));
                }
                let finish_reason = if self.emitted_tool_call {
                    "tool_calls"
                } else {
                    "stop"
                };
                Ok(self.finish(finish_reason))
            }
            Some("response.failed") => {
                self.done = true;
                Ok(vec![chat_sse_data(responses_error_to_chat_error(Some(
                    &data,
                )))])
            }
            _ => Ok(Vec::new()),
        }
    }

    /// 从 response 元数据事件里补齐 chunk 级别的 id/model/created。
    fn apply_response_metadata(&mut self, response: &Value) {
        if let Some(id) = response.get("id").and_then(|value| value.as_str()) {
            self.id = chat_id_from_response_id(Some(id));
        }
        if let Some(model) = response.get("model").and_then(|value| value.as_str()) {
            if !model.is_empty() {
                self.model = model.to_string();
            }
        }
        if let Some(created) = response
            .get("created_at")
            .or_else(|| response.get("created"))
            .and_then(|value| value.as_u64())
        {
            self.created = created;
        }
    }

    /// 确保流式响应先发送一次 assistant role chunk，兼容 OpenAI SDK。
    fn ensure_role_chunk(&mut self) -> Vec<Bytes> {
        if self.role_sent {
            return Vec::new();
        }
        self.role_sent = true;
        vec![self.delta_chunk(json!({ "role": "assistant" }), None, None)]
    }

    /// 从 Responses output item 生成 OpenAI Chat tool_call delta。
    fn tool_call_delta_from_item(&mut self, item: &Value) -> Option<Value> {
        let item_type = item.get("type").and_then(|value| value.as_str())?;
        if !matches!(
            item_type,
            "function_call" | "custom_tool_call" | "tool_search_call"
        ) {
            return None;
        }

        let index = self.next_tool_index;
        self.next_tool_index += 1;
        self.emitted_tool_call = true;

        let id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch");
        let name = item
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("tool_call");
        let arguments = item
            .get("arguments")
            .or_else(|| item.get("input"))
            .map(response_tool_arguments_to_chat)
            .unwrap_or_else(|| "{}".to_string());

        Some(json!({
            "index": index,
            "id": id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments
            }
        }))
    }

    /// 生成一个标准 Chat Completions stream chunk。
    fn delta_chunk(
        &self,
        delta: Value,
        finish_reason: Option<&str>,
        usage: Option<Value>,
    ) -> Bytes {
        chat_sse_data(json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }],
            "usage": usage.unwrap_or(Value::Null)
        }))
    }

    /// 结束当前流，并在有 usage 时追加 OpenAI SDK 可识别的 usage chunk。
    fn finish(&mut self, finish_reason: &str) -> Vec<Bytes> {
        if self.done {
            return Vec::new();
        }
        self.done = true;
        let mut events = self.ensure_role_chunk();
        events.push(self.delta_chunk(json!({}), Some(finish_reason), None));
        if let Some(usage) = self.usage.take() {
            events.push(chat_sse_data(json!({
                "id": self.id,
                "object": "chat.completion.chunk",
                "created": self.created,
                "model": self.model,
                "choices": [],
                "usage": usage
            })));
        }
        events
    }
}

/// 将 Chat message content 提取为纯文本，供 system/developer 合并 instructions 使用。
fn chat_message_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(|value| value.as_str())
                        .or_else(|| part.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        other => other.as_str().map(ToString::to_string),
    }
}

/// 将 user/developer 以外的普通 Chat message 转换为 Responses message item。
fn chat_user_message_to_response_message(role: &str, message: &Value) -> Value {
    json!({
        "type": "message",
        "role": match role {
            "assistant" => "assistant",
            _ => "user",
        },
        "content": chat_content_to_responses_content(message.get("content"), "input_text")
    })
}

/// 将 assistant 历史消息拆成 Responses message 和 function_call items。
fn append_chat_assistant_message(message: &Value, input: &mut Vec<Value>) {
    let content = chat_content_to_responses_content(message.get("content"), "output_text");
    if !content.as_array().is_some_and(|parts| parts.is_empty()) {
        input.push(json!({
            "type": "message",
            "role": "assistant",
            "content": content
        }));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|value| value.as_array()) {
        for tool_call in tool_calls {
            input.push(chat_tool_call_to_response_item(tool_call));
        }
    }
}

/// 将 Chat tool result message 转换为 Responses function_call_output item。
fn chat_tool_message_to_response_item(message: &Value) -> Value {
    json!({
        "type": "function_call_output",
        "call_id": message
            .get("tool_call_id")
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "output": message
            .get("content")
            .and_then(|value| value.as_str())
            .map(canonicalize_json_string_if_parseable)
            .unwrap_or_else(String::new)
    })
}

/// 将 Chat tool_call 转换为 Responses function_call item。
fn chat_tool_call_to_response_item(tool_call: &Value) -> Value {
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    json!({
        "type": "function_call",
        "id": tool_call
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "call_id": tool_call
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "name": function
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("tool_call"),
        "arguments": function
            .get("arguments")
            .and_then(|value| value.as_str())
            .unwrap_or("{}")
    })
}

/// 将 Chat content parts 转换为 Responses content parts。
fn chat_content_to_responses_content(content: Option<&Value>, text_type: &str) -> Value {
    match content {
        Some(Value::String(text)) => json!([{ "type": text_type, "text": text }]),
        Some(Value::Array(parts)) => Value::Array(
            parts
                .iter()
                .filter_map(|part| chat_content_part_to_response_part(part, text_type))
                .collect(),
        ),
        Some(Value::Null) | None => json!([]),
        Some(other) => json!([{ "type": text_type, "text": other.to_string() }]),
    }
}

/// 将单个 Chat content part 转换为 Responses content part。
fn chat_content_part_to_response_part(part: &Value, text_type: &str) -> Option<Value> {
    match part.get("type").and_then(|value| value.as_str()) {
        Some("text") => Some(json!({
            "type": text_type,
            "text": part.get("text").and_then(|value| value.as_str()).unwrap_or("")
        })),
        Some("image_url") => Some(json!({
            "type": "input_image",
            "image_url": part
                .pointer("/image_url/url")
                .or_else(|| part.get("image_url"))
                .cloned()
                .unwrap_or(Value::Null)
        })),
        Some("input_audio") => Some(json!({
            "type": "input_audio",
            "input_audio": part.get("input_audio").cloned().unwrap_or(Value::Null)
        })),
        Some("file") => {
            let file = part.get("file")?;
            let mut mapped = Map::new();
            for key in ["file_id", "file_data", "filename"] {
                if let Some(value) = file.get(key) {
                    mapped.insert(key.to_string(), value.clone());
                }
            }
            Some(Value::Object({
                let mut object = Map::new();
                object.insert("type".to_string(), json!("input_file"));
                for (key, value) in mapped {
                    object.insert(key, value);
                }
                object
            }))
        }
        _ => None,
    }
}

/// 将 OpenAI Chat tools 转换为 Responses tools。
fn chat_tools_to_responses_tools(tools: Option<&Value>) -> Value {
    let Some(tools) = tools.and_then(|value| value.as_array()) else {
        return json!([]);
    };

    Value::Array(
        tools
            .iter()
            .filter_map(|tool| {
                if tool.get("type").and_then(|value| value.as_str()) != Some("function") {
                    return None;
                }
                let function = tool.get("function")?;
                let mut mapped = Map::new();
                mapped.insert("type".to_string(), json!("function"));
                mapped.insert(
                    "name".to_string(),
                    function
                        .get("name")
                        .cloned()
                        .unwrap_or_else(|| json!("tool_call")),
                );
                if let Some(description) = function.get("description") {
                    mapped.insert("description".to_string(), description.clone());
                }
                mapped.insert(
                    "parameters".to_string(),
                    function
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                );
                if let Some(strict) = function.get("strict").or_else(|| tool.get("strict")) {
                    mapped.insert("strict".to_string(), strict.clone());
                }
                Some(Value::Object(mapped))
            })
            .collect(),
    )
}

/// 将 Chat tool_choice 转换为 Responses tool_choice。
fn chat_tool_choice_to_responses(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(value) => match value.as_str() {
            "required" => json!("required"),
            "none" => json!("none"),
            _ => json!("auto"),
        },
        Value::Object(object) => {
            if object.get("type").and_then(|value| value.as_str()) == Some("function") {
                json!({
                    "type": "function",
                    "name": object
                        .get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                })
            } else {
                tool_choice.clone()
            }
        }
        _ => json!("auto"),
    }
}

/// 收集 Responses output item 中的文本、推理和工具调用。
fn collect_response_output_item(
    item: &Value,
    content_parts: &mut Vec<String>,
    tool_calls: &mut Vec<Value>,
    reasoning_parts: &mut Vec<String>,
) {
    match item.get("type").and_then(|value| value.as_str()) {
        Some("message") => collect_response_message_content(item, content_parts, reasoning_parts),
        Some("reasoning") => {
            if let Some(text) = response_reasoning_text(item) {
                reasoning_parts.push(text);
            }
        }
        Some("function_call" | "custom_tool_call" | "tool_search_call") => {
            tool_calls.push(response_item_to_chat_tool_call(item, tool_calls.len()));
            if let Some(text) = response_reasoning_text(item) {
                reasoning_parts.push(text);
            }
        }
        _ => {}
    }
}

/// 收集 Responses message item 内的 content 文本。
fn collect_response_message_content(
    item: &Value,
    content_parts: &mut Vec<String>,
    reasoning_parts: &mut Vec<String>,
) {
    if let Some(text) = response_reasoning_text(item) {
        reasoning_parts.push(text);
    }

    let Some(content) = item.get("content") else {
        return;
    };
    match content {
        Value::String(text) => content_parts.push(text.clone()),
        Value::Array(parts) => {
            for part in parts {
                if let Some(text) = part
                    .get("text")
                    .or_else(|| part.get("refusal"))
                    .and_then(|value| value.as_str())
                {
                    content_parts.push(text.to_string());
                }
            }
        }
        _ => {}
    }
}

/// 提取 Responses item 上可能存在的 reasoning 文本。
fn response_reasoning_text(item: &Value) -> Option<String> {
    item.get("summary")
        .and_then(|value| value.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            item.get("reasoning_content")
                .or_else(|| item.get("reasoning"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
}

/// 将 Responses function_call/custom/tool_search item 转成 Chat tool_call。
fn response_item_to_chat_tool_call(item: &Value, index: usize) -> Value {
    json!({
        "index": index,
        "id": item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "type": "function",
        "function": {
            "name": item
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("tool_call"),
            "arguments": item
                .get("arguments")
                .or_else(|| item.get("input"))
                .map(response_tool_arguments_to_chat)
                .unwrap_or_else(|| "{}".to_string())
        }
    })
}

/// 将 Responses 工具参数规整为 Chat Completions 要求的 JSON 字符串。
fn response_tool_arguments_to_chat(value: &Value) -> String {
    match value {
        Value::String(text) => canonicalize_json_string_if_parseable(text),
        other => canonical_json_string(other),
    }
}

/// 从 Responses status/incomplete_details 推断 OpenAI finish_reason。
fn finish_reason_from_response(response: &Value, has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        return "tool_calls";
    }
    match response.get("status").and_then(|value| value.as_str()) {
        Some("incomplete") => "length",
        _ => "stop",
    }
}

/// 将 Responses usage 转成 Chat Completions usage。
fn responses_usage_to_chat_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object()) else {
        return json!({
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        });
    };

    let prompt_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let mut mapped = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": usage
            .get("total_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(prompt_tokens + completion_tokens)
    });

    if let Some(cached) = usage
        .pointer("/input_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cache_read_input_tokens"))
    {
        mapped["prompt_tokens_details"] = json!({ "cached_tokens": cached });
    }
    mapped
}

/// 解析一个 SSE 文本块中的 event 和 data。
fn parse_sse_block(block: &str) -> Result<(Option<String>, Value), ProxyError> {
    let mut event_name = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = strip_sse_field(line, "event") {
            event_name = Some(event.to_string());
        } else if let Some(data) = strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }

    let data_str = data_lines.join("\n");
    if data_str.trim().is_empty() || data_str.trim() == "[DONE]" {
        return Ok((event_name, Value::Null));
    }
    let data = serde_json::from_str(&data_str).map_err(|err| {
        ProxyError::TransformError(format!("Failed to parse Responses SSE data: {err}"))
    })?;
    Ok((event_name, data))
}

/// 生成一条 OpenAI SSE data 事件。
fn chat_sse_data(value: Value) -> Bytes {
    let data = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("data: {data}\n\n"))
}

/// 将 Responses id 映射成 Chat Completions id。
fn chat_id_from_response_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("resp_ccswitch");
    if id.starts_with("chatcmpl_") {
        id.to_string()
    } else {
        format!("chatcmpl_{id}")
    }
}

/// 生成当前 Unix 时间戳，作为缺省 created 值。
fn current_unix_timestamp() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// 规整不同 Responses 错误形状里的 error 对象。
fn normalize_error_object(error: &Value) -> Value {
    json!({
        "message": error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("Upstream Codex OAuth request failed"),
        "type": error
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("upstream_error"),
        "code": error.get("code").cloned().unwrap_or(Value::Null),
        "param": error.get("param").cloned().unwrap_or(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{stream, StreamExt};

    #[test]
    fn chat_request_maps_to_codex_responses_contract() {
        let input = json!({
            "model": "gpt-5.4-mini",
            "messages": [
                {"role": "system", "content": "Be concise."},
                {"role": "user", "content": "ping"}
            ],
            "stream": false,
            "temperature": 0.2,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "Lookup data",
                    "parameters": {"type": "object"}
                }
            }]
        });

        let result = chat_completions_request_to_codex_responses(input).unwrap();

        assert_eq!(result["model"], "gpt-5.4-mini");
        assert_eq!(result["instructions"], "Be concise.");
        assert_eq!(result["input"][0]["role"], "user");
        assert_eq!(result["input"][0]["content"][0]["text"], "ping");
        assert_eq!(result["store"], false);
        assert_eq!(result["stream"], true);
        assert!(result.get("temperature").is_none());
        assert_eq!(result["tools"][0]["name"], "lookup");
        assert_eq!(result["include"][0], "reasoning.encrypted_content");
    }

    #[test]
    fn chat_request_without_system_prompt_still_sets_codex_instructions() {
        let input = json!({
            "model": "gpt-5.4-mini",
            "messages": [
                {"role": "user", "content": "ping"}
            ],
            "stream": false
        });

        let result = chat_completions_request_to_codex_responses(input).unwrap();

        assert_eq!(result["instructions"], "You are a helpful assistant.");
        assert_eq!(result["input"][0]["content"][0]["text"], "ping");
    }

    #[test]
    fn responses_json_maps_to_chat_completion() {
        let response = json!({
            "id": "resp_123",
            "model": "gpt-5.4-mini",
            "created_at": 123,
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "pong"}]
            }],
            "usage": {"input_tokens": 4, "output_tokens": 2}
        });

        let result = codex_responses_to_chat_completion(response, "fallback").unwrap();

        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["id"], "chatcmpl_resp_123");
        assert_eq!(result["choices"][0]["message"]["content"], "pong");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
        assert_eq!(result["usage"]["prompt_tokens"], 4);
        assert_eq!(result["usage"]["completion_tokens"], 2);
    }

    #[test]
    fn responses_json_with_null_error_maps_to_chat_completion() {
        let response = json!({
            "id": "resp_null_error",
            "model": "gpt-5.4-mini",
            "created_at": 123,
            "status": "completed",
            "error": null,
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "pong"}]
            }]
        });

        let result = codex_responses_to_chat_completion(response, "fallback").unwrap();

        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["choices"][0]["message"]["content"], "pong");
        assert!(result.get("error").is_none());
    }

    #[test]
    fn responses_tool_call_maps_to_chat_tool_call() {
        let response = json!({
            "id": "resp_tools",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{\"q\":\"x\"}"
            }]
        });

        let result = codex_responses_to_chat_completion(response, "gpt-5.4-mini").unwrap();

        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(result["choices"][0]["message"]["content"], Value::Null);
        assert_eq!(
            result["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
    }

    #[tokio::test]
    async fn responses_sse_maps_to_chat_sse() {
        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_stream\",\"model\":\"gpt-5.4-mini\",\"created_at\":123}}\n\n",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_stream\",\"model\":\"gpt-5.4-mini\",\"created_at\":123,\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
            )),
        ]);

        let output = create_chat_sse_stream_from_codex_responses(upstream, "fallback".to_string())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
            .collect::<String>();

        assert!(output.contains("\"object\":\"chat.completion.chunk\""));
        assert!(output.contains("\"content\":\"Hel\""));
        assert!(output.contains("\"content\":\"lo\""));
        assert!(output.contains("\"finish_reason\":\"stop\""));
        assert!(output.contains("\"completion_tokens\":2"));
        assert!(output.contains("\"prompt_tokens\":1"));
        assert!(output.contains("data: [DONE]"));
    }
}
