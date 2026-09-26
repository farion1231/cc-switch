//! SSE 聚合器：把流式 SSE 事件聚合成一个完整的 response JSON。
//!
//! 三个协议聚合器：
//! - Anthropic Messages (`/v1/messages`)
//! - OpenAI Responses (`/v1/responses`)
//! - OpenAI Chat Completions (`/v1/chat/completions`)

use serde_json::{json, Map, Value};

/// 协议无关的 SSE 聚合器接口
pub trait SseAggregator: Send {
    /// 喂入一个 SSE event 的 `data:` JSON
    fn ingest_event(&mut self, value: &Value);
    /// 流结束后产出完整 response JSON
    fn finalize(self: Box<Self>) -> Value;
}

// ============================================================================
// Anthropic Messages 聚合器
// ============================================================================

/// 聚合 Anthropic `/v1/messages` 的 SSE 事件流。
///
/// 流式事件序列（典型）：
/// - `message_start`：携带初始 `message` 对象（含 model、usage 等）
/// - `content_block_start`：当前 block 的初始结构（text 或 tool_use）
/// - `content_block_delta`：text 增量 / input_json 增量
/// - `content_block_stop`：结束当前 block
/// - `message_delta`：携带 stop_reason 与最终 usage 增量
/// - `message_stop`
#[derive(Default)]
pub struct AnthropicMessagesAggregator {
    message: Option<Value>,
    blocks: Vec<Value>,
    /// tool_use 的 input 是流式 partial_json，需要按 block index 拼字符串
    tool_input_buffers: std::collections::BTreeMap<usize, String>,
    /// 终态 usage（来自 message_delta 的 usage 字段）
    final_usage: Option<Value>,
    stop_reason: Option<Value>,
    stop_sequence: Option<Value>,
}

impl SseAggregator for AnthropicMessagesAggregator {
    fn ingest_event(&mut self, value: &Value) {
        let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "message_start" => {
                if let Some(msg) = value.get("message").cloned() {
                    self.message = Some(msg);
                }
            }
            "content_block_start" => {
                let index = value
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or_else(|| self.blocks.len());
                let block = value
                    .get("content_block")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                while self.blocks.len() <= index {
                    self.blocks.push(json!({}));
                }
                self.blocks[index] = block;
            }
            "content_block_delta" => {
                let index = value
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or(0);
                if index >= self.blocks.len() {
                    // 防御性：没有对应的 start，跳过
                    return;
                }
                let Some(delta) = value.get("delta") else {
                    return;
                };
                let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                            let block = &mut self.blocks[index];
                            let existing = block
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            block["text"] = Value::String(existing + text);
                        }
                    }
                    "input_json_delta" => {
                        if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                            self.tool_input_buffers
                                .entry(index)
                                .or_default()
                                .push_str(partial);
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()) {
                            let block = &mut self.blocks[index];
                            let existing = block
                                .get("thinking")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            block["thinking"] = Value::String(existing + text);
                        }
                    }
                    "signature_delta" => {
                        if let Some(sig) = delta.get("signature").and_then(|v| v.as_str()) {
                            let block = &mut self.blocks[index];
                            let existing = block
                                .get("signature")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            block["signature"] = Value::String(existing + sig);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = value
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or(0);
                // 把 tool_input_buffers 落到对应的 block.input
                if let Some(buffer) = self.tool_input_buffers.remove(&index) {
                    if index < self.blocks.len() {
                        let parsed: Value = if buffer.is_empty() {
                            json!({})
                        } else {
                            serde_json::from_str(&buffer)
                                .unwrap_or_else(|_| Value::String(buffer.clone()))
                        };
                        self.blocks[index]["input"] = parsed;
                    }
                }
            }
            "message_delta" => {
                if let Some(delta) = value.get("delta") {
                    if let Some(sr) = delta.get("stop_reason") {
                        self.stop_reason = Some(sr.clone());
                    }
                    if let Some(ss) = delta.get("stop_sequence") {
                        self.stop_sequence = Some(ss.clone());
                    }
                }
                if let Some(usage) = value.get("usage") {
                    self.final_usage = Some(usage.clone());
                }
            }
            _ => {}
        }
    }

    fn finalize(self: Box<Self>) -> Value {
        let mut message = self.message.unwrap_or_else(|| json!({}));
        let obj = match message.as_object_mut() {
            Some(o) => o,
            None => return message,
        };

        // content blocks
        obj.insert("content".to_string(), Value::Array(self.blocks));

        // stop_reason / stop_sequence
        if let Some(sr) = self.stop_reason {
            obj.insert("stop_reason".to_string(), sr);
        }
        if let Some(ss) = self.stop_sequence {
            obj.insert("stop_sequence".to_string(), ss);
        }

        // usage 合并：message_start 的 usage 作为基准，message_delta 的 usage 覆盖（output_tokens 等）
        if let Some(final_usage) = self.final_usage {
            let base = obj.get("usage").cloned().unwrap_or_else(|| json!({}));
            let merged = merge_usage(base, final_usage);
            obj.insert("usage".to_string(), merged);
        }

        message
    }
}

fn merge_usage(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut b), Value::Object(o)) => {
            for (k, v) in o {
                b.insert(k, v);
            }
            Value::Object(b)
        }
        (_, overlay) => overlay,
    }
}

// ============================================================================
// OpenAI Responses 聚合器
// ============================================================================

/// 聚合 OpenAI `/v1/responses` 的 SSE 事件流。
///
/// 关键事件：
/// - `response.created` / `response.in_progress`：携带初始 response 对象
/// - `response.output_item.done`：完整 output item（message / function_call / reasoning）
/// - `response.completed`：携带完整 response（含 output、usage、status）
/// - `response.failed`：错误
#[derive(Default)]
pub struct OpenAiResponsesAggregator {
    completed_response: Option<Value>,
    output_items: Vec<Value>,
    initial_response: Option<Value>,
    failed_error: Option<Value>,
}

impl SseAggregator for OpenAiResponsesAggregator {
    fn ingest_event(&mut self, value: &Value) {
        let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match event_type {
            "response.created" | "response.in_progress" if self.initial_response.is_none() => {
                self.initial_response = value.get("response").cloned();
            }
            "response.output_item.done" => {
                if let Some(item) = value.get("item").cloned() {
                    self.output_items.push(item);
                }
            }
            "response.completed" => {
                self.completed_response = value.get("response").cloned();
            }
            "response.failed" => {
                if let Some(err) = value.pointer("/response/error").cloned() {
                    self.failed_error = Some(err);
                } else if let Some(err) = value.get("error").cloned() {
                    self.failed_error = Some(err);
                }
                // 即便失败也尽量保留 response 对象
                if self.completed_response.is_none() {
                    self.completed_response = value.get("response").cloned();
                }
            }
            _ => {}
        }
    }

    fn finalize(self: Box<Self>) -> Value {
        let mut response = self
            .completed_response
            .or(self.initial_response)
            .unwrap_or_else(|| json!({}));

        if let Some(obj) = response.as_object_mut() {
            // 上游 response.completed 通常已包含完整 output；若缺失则用我们累计的
            // output_items 兜底，避免聚合结果丢失工具调用 / message。
            let has_output = obj
                .get("output")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty());
            if !has_output && !self.output_items.is_empty() {
                obj.insert("output".to_string(), Value::Array(self.output_items));
            }
            if let Some(err) = self.failed_error {
                obj.insert("error".to_string(), err);
            }
        }

        response
    }
}

// ============================================================================
// OpenAI Chat Completions 聚合器
// ============================================================================

/// 聚合 OpenAI `/v1/chat/completions` 的 SSE 事件流。
///
/// 每个 chunk 的 `choices[0].delta` 携带增量字段：role / content / reasoning_content
/// / tool_calls。最后一个 chunk 通常带 `finish_reason` 与 `usage`。
#[derive(Default)]
pub struct OpenAiChatAggregator {
    id: Option<Value>,
    object: Option<Value>,
    created: Option<Value>,
    model: Option<Value>,
    system_fingerprint: Option<Value>,
    content: String,
    reasoning_content: String,
    role: Option<String>,
    tool_calls: std::collections::BTreeMap<usize, Value>,
    finish_reason: Option<Value>,
    usage: Option<Value>,
}

impl SseAggregator for OpenAiChatAggregator {
    fn ingest_event(&mut self, value: &Value) {
        // envelope 字段：首个非空值锁定
        for (slot, key) in [
            (&mut self.id, "id"),
            (&mut self.object, "object"),
            (&mut self.created, "created"),
            (&mut self.model, "model"),
            (&mut self.system_fingerprint, "system_fingerprint"),
        ] {
            if slot.is_none() {
                if let Some(v) = value.get(key) {
                    if !v.is_null() {
                        *slot = Some(v.clone());
                    }
                }
            }
        }

        if let Some(u) = value.get("usage") {
            if !u.is_null() {
                self.usage = Some(u.clone());
            }
        }

        let Some(choice) = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
        else {
            return;
        };

        if self.finish_reason.is_none() {
            if let Some(fr) = choice.get("finish_reason") {
                if !fr.is_null() {
                    self.finish_reason = Some(fr.clone());
                }
            }
        }

        let payload = choice
            .get("delta")
            .or_else(|| choice.get("message"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        if let Some(role) = payload.get("role").and_then(|v| v.as_str()) {
            self.role = Some(role.to_string());
        }
        match payload.get("content") {
            Some(Value::String(s)) => self.content.push_str(s),
            Some(Value::Array(parts)) => {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        self.content.push_str(text);
                    }
                }
            }
            _ => {}
        }
        // 思考增量：reasoning_content（DeepSeek 风格）优先，reasoning（OpenRouter/Kimi
        // /GLM 等）次之。与 streaming.rs 的 #[serde(alias = "reasoning_content")] 对齐，
        // 否则上游用 `reasoning` 字段回传思考时，聚合结果会丢失思考内容。
        for key in ["reasoning_content", "reasoning"] {
            if let Some(rc) = payload.get(key).and_then(|v| v.as_str()) {
                if !rc.is_empty() {
                    self.reasoning_content.push_str(rc);
                    break;
                }
            }
        }
        if let Some(tcs) = payload.get("tool_calls").and_then(|v| v.as_array()) {
            for (pos, tc) in tcs.iter().enumerate() {
                let index = tc
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or(pos);
                let entry = self.tool_calls.entry(index).or_insert_with(|| {
                    json!({
                        "id": "",
                        "type": "function",
                        "function": {"name": "", "arguments": ""}
                    })
                });
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        entry["id"] = json!(id);
                    }
                }
                if let Some(tp) = tc.get("type").and_then(|v| v.as_str()) {
                    entry["type"] = json!(tp);
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            entry["function"]["name"] = json!(name);
                        }
                    }
                    if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                        let existing = entry["function"]["arguments"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        entry["function"]["arguments"] = json!(existing + args);
                    }
                }
            }
        }
    }

    fn finalize(self: Box<Self>) -> Value {
        let mut message = Map::new();
        message.insert(
            "role".to_string(),
            json!(self.role.unwrap_or_else(|| "assistant".to_string())),
        );
        message.insert("content".to_string(), json!(self.content));
        if !self.reasoning_content.is_empty() {
            message.insert(
                "reasoning_content".to_string(),
                json!(self.reasoning_content),
            );
        }
        if !self.tool_calls.is_empty() {
            let tcs: Vec<Value> = self.tool_calls.into_values().collect();
            message.insert("tool_calls".to_string(), Value::Array(tcs));
        }

        let mut response = json!({
            "id": self.id.unwrap_or(Value::Null),
            "object": self
                .object
                .unwrap_or_else(|| Value::String("chat.completion".to_string())),
            "created": self.created.unwrap_or(Value::Null),
            "model": self.model.unwrap_or(Value::Null),
            "choices": [{
                "index": 0,
                "message": Value::Object(message),
                "finish_reason": self.finish_reason.unwrap_or(Value::Null),
            }],
        });
        if let Some(fp) = self.system_fingerprint {
            response["system_fingerprint"] = fp;
        }
        if let Some(u) = self.usage {
            response["usage"] = u;
        }
        response
    }
}

// ============================================================================
// 兜底：原样保留 SSE 文本
// ============================================================================

/// 协议未知时的兜底聚合器：保留每个 event 的原始 JSON，方便事后排查。
#[derive(Default)]
pub struct PassthroughAggregator {
    events: Vec<Value>,
}

impl SseAggregator for PassthroughAggregator {
    fn ingest_event(&mut self, value: &Value) {
        self.events.push(value.clone());
    }
    fn finalize(self: Box<Self>) -> Value {
        json!({ "raw_events": self.events })
    }
}

// ============================================================================
// 选择聚合器
// ============================================================================

/// 根据 app_type 与 endpoint 选择对应的聚合器。
///
/// 规则（endpoint 可能带或不带 `/v1` 前缀——Codex 路径在 handler 里已 strip 成
/// `/responses`、`/chat/completions`，Claude 保留完整 `/v1/messages`）：
/// - claude / claude-desktop 且 endpoint 含 `messages` → Anthropic Messages
/// - endpoint 含 `chat/completions` → OpenAI Chat Completions（先判，避免被 responses 误吞）
/// - endpoint 含 `responses` → OpenAI Responses
/// - 其它（Gemini 等）→ Passthrough
pub fn select_aggregator(app_type_str: &str, endpoint: &str) -> Box<dyn SseAggregator + Send> {
    let is_claude_app = matches!(app_type_str, "claude" | "claude-desktop");
    if is_claude_app && endpoint.contains("/messages") {
        return Box::new(AnthropicMessagesAggregator::default());
    }
    if endpoint.contains("/chat/completions") {
        return Box::new(OpenAiChatAggregator::default());
    }
    if endpoint.contains("/responses") {
        return Box::new(OpenAiResponsesAggregator::default());
    }
    Box::new(PassthroughAggregator::default())
}

/// 按 Claude transform 路径的 `api_format` 选择聚合器。
///
/// transform 路径下客户端 endpoint 始终是 `/v1/messages`，但上游真实协议由
/// provider 的 `api_format` 决定（openai / openai_responses / gemini_native）。
/// 这里直接按 api_format 选对应聚合器，以便记录**转换前的上游原始报文**。
pub fn aggregator_for_api_format(api_format: &str) -> Box<dyn SseAggregator + Send> {
    match api_format {
        "openai_responses" => Box::new(OpenAiResponsesAggregator::default()),
        // 上游是 OpenAI Chat Completions（OpenRouter 等中转的默认形态）
        "openai" | "openai_chat" | "" => Box::new(OpenAiChatAggregator::default()),
        // 上游是 Anthropic Messages（codex anthropic→responses 转换路径）
        "anthropic" => Box::new(AnthropicMessagesAggregator::default()),
        // gemini_native 等暂无专用聚合器，原样保留事件
        _ => Box::new(PassthroughAggregator::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run<A: SseAggregator + Default + 'static>(events: Vec<Value>) -> Value {
        let mut agg: Box<dyn SseAggregator + Send> = Box::new(A::default());
        for e in &events {
            agg.ingest_event(e);
        }
        agg.finalize()
    }

    #[test]
    fn anthropic_aggregates_text_and_usage() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-opus-4-7",
                    "content": [],
                    "stop_reason": null,
                    "usage": {"input_tokens": 10, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": " world"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 5}
            }),
            json!({"type": "message_stop"}),
        ];

        let result = run::<AnthropicMessagesAggregator>(events);
        assert_eq!(result["id"], "msg_1");
        assert_eq!(result["model"], "claude-opus-4-7");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello world");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn anthropic_aggregates_tool_use_block() {
        let events = vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_2",
                    "model": "claude-opus-4-7",
                    "content": [],
                    "usage": {"input_tokens": 5, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "get_weather",
                    "input": {}
                }
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"city\":"}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "\"SF\"}"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use"},
                "usage": {"output_tokens": 12}
            }),
        ];

        let result = run::<AnthropicMessagesAggregator>(events);
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["city"], "SF");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn openai_responses_uses_completed_payload() {
        let events = vec![
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "model": "gpt-5", "output": []}
            }),
            json!({
                "type": "response.output_item.done",
                "item": {"type": "message", "role": "assistant",
                         "content": [{"type": "output_text", "text": "hi"}]}
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "model": "gpt-5",
                    "status": "completed",
                    "output": [{"type": "message", "role": "assistant",
                                "content": [{"type": "output_text", "text": "hi"}]}],
                    "usage": {"input_tokens": 3, "output_tokens": 1}
                }
            }),
        ];

        let result = run::<OpenAiResponsesAggregator>(events);
        assert_eq!(result["id"], "resp_1");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["output"][0]["content"][0]["text"], "hi");
        assert_eq!(result["usage"]["input_tokens"], 3);
    }

    #[test]
    fn openai_responses_falls_back_to_collected_items() {
        // 没有 completed 事件时，至少保留累计的 output_items
        let events = vec![
            json!({
                "type": "response.created",
                "response": {"id": "resp_x", "model": "gpt-5"}
            }),
            json!({
                "type": "response.output_item.done",
                "item": {"type": "message", "role": "assistant"}
            }),
        ];

        let result = run::<OpenAiResponsesAggregator>(events);
        assert_eq!(result["id"], "resp_x");
        assert_eq!(result["output"][0]["type"], "message");
    }

    #[test]
    fn openai_chat_aggregates_content_and_tool_calls() {
        let events = vec![
            json!({
                "id": "chatcmpl-1",
                "model": "gpt-5",
                "created": 123,
                "choices": [{
                    "index": 0,
                    "delta": {"role": "assistant", "content": "Hel"},
                    "finish_reason": null
                }]
            }),
            json!({
                "id": "chatcmpl-1",
                "choices": [{
                    "index": 0,
                    "delta": {"content": "lo"},
                    "finish_reason": null
                }]
            }),
            json!({
                "id": "chatcmpl-1",
                "choices": [{
                    "index": 0,
                    "delta": {"tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "f", "arguments": "{}"}
                    }]},
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 5, "completion_tokens": 2}
            }),
        ];

        let result = run::<OpenAiChatAggregator>(events);
        assert_eq!(result["id"], "chatcmpl-1");
        assert_eq!(result["choices"][0]["message"]["role"], "assistant");
        assert_eq!(result["choices"][0]["message"]["content"], "Hello");
        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
        let tc = &result["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(tc["id"], "call_1");
        assert_eq!(tc["function"]["name"], "f");
        assert_eq!(result["usage"]["prompt_tokens"], 5);
    }

    #[test]
    fn openai_chat_collects_reasoning_content() {
        let events = vec![
            json!({
                "id": "c1",
                "model": "deepseek-r1",
                "choices": [{
                    "index": 0,
                    "delta": {"reasoning_content": "think"},
                    "finish_reason": null
                }]
            }),
            json!({
                "id": "c1",
                "choices": [{
                    "index": 0,
                    "delta": {"reasoning_content": "ing", "content": "answer"},
                    "finish_reason": "stop"
                }]
            }),
        ];

        let result = run::<OpenAiChatAggregator>(events);
        assert_eq!(
            result["choices"][0]["message"]["reasoning_content"],
            "thinking"
        );
        assert_eq!(result["choices"][0]["message"]["content"], "answer");
    }

    #[test]
    fn openai_chat_collects_reasoning_field_alias() {
        // 上游用 `reasoning`（而非 `reasoning_content`）回传思考时也必须聚合，
        // 与 streaming.rs 的 #[serde(alias = "reasoning_content")] 对齐。
        let events = vec![
            json!({
                "id": "c1",
                "model": "glm-5.2",
                "choices": [{
                    "index": 0,
                    "delta": {"reasoning": "think"},
                    "finish_reason": null
                }]
            }),
            json!({
                "id": "c1",
                "choices": [{
                    "index": 0,
                    "delta": {"reasoning": "ing", "content": "answer"},
                    "finish_reason": "stop"
                }]
            }),
        ];

        let result = run::<OpenAiChatAggregator>(events);
        assert_eq!(
            result["choices"][0]["message"]["reasoning_content"],
            "thinking"
        );
        assert_eq!(result["choices"][0]["message"]["content"], "answer");
    }

    #[test]
    fn select_aggregator_routes_by_endpoint() {
        let a = select_aggregator("claude", "/v1/messages");
        assert_eq!(a.finalize(), json!({"content": []}));

        // Codex 路径在 handler 里已 strip 成不带 /v1 的形态——必须能正确路由
        for ep in ["/responses", "/v1/responses", "/responses/compact"] {
            let mut a = select_aggregator("codex", ep);
            a.ingest_event(&json!({
                "type": "response.completed",
                "response": {"id": "r"}
            }));
            assert_eq!(a.finalize()["id"], "r", "endpoint={ep}");
        }

        for ep in ["/chat/completions", "/v1/chat/completions"] {
            let mut a = select_aggregator("codex", ep);
            a.ingest_event(&json!({
                "id": "c", "choices": [{"delta": {"content": "x"}, "finish_reason": "stop"}]
            }));
            let v = a.finalize();
            assert_eq!(v["id"], "c", "endpoint={ep}");
            assert_eq!(v["choices"][0]["message"]["content"], "x");
        }

        // Gemini fallback to passthrough
        let mut a = select_aggregator("gemini", "/v1beta/models/gemini-pro:generateContent");
        a.ingest_event(&json!({"foo": 1}));
        let v = a.finalize();
        assert_eq!(v["raw_events"][0]["foo"], 1);
    }

    #[test]
    fn aggregator_for_api_format_maps_transform_upstreams() {
        // openai_responses → Responses 聚合器
        let mut a = aggregator_for_api_format("openai_responses");
        a.ingest_event(&json!({
            "type": "response.completed",
            "response": {"id": "r1"}
        }));
        assert_eq!(a.finalize()["id"], "r1");

        // openai / 空串 → Chat Completions 聚合器
        for fmt in ["openai", "openai_chat", ""] {
            let mut a = aggregator_for_api_format(fmt);
            a.ingest_event(&json!({
                "id": "c1",
                "choices": [{"delta": {"content": "hi"}, "finish_reason": "stop"}]
            }));
            let v = a.finalize();
            assert_eq!(v["id"], "c1", "api_format={fmt}");
            assert_eq!(v["choices"][0]["message"]["content"], "hi");
        }

        // gemini_native 等 → passthrough
        let mut a = aggregator_for_api_format("gemini_native");
        a.ingest_event(&json!({"foo": 2}));
        assert_eq!(a.finalize()["raw_events"][0]["foo"], 2);

        // anthropic → Anthropic Messages 聚合器
        let mut a = aggregator_for_api_format("anthropic");
        a.ingest_event(&json!({
            "type": "message_start",
            "message": {"id": "msg_2", "model": "claude-opus-4-7", "usage": {"input_tokens": 5}}
        }));
        a.ingest_event(&json!({
            "type": "content_block_start", "index": 0,
            "content_block": {"type": "text", "text": ""}
        }));
        a.ingest_event(&json!({
            "type": "content_block_delta", "index": 0,
            "delta": {"type": "text_delta", "text": "hi"}
        }));
        a.ingest_event(&json!({"type": "content_block_stop", "index": 0}));
        a.ingest_event(&json!({"type": "message_stop"}));
        let v = a.finalize();
        assert_eq!(v["id"], "msg_2");
        assert_eq!(v["content"][0]["text"], "hi");
    }
}
