//! DeepSeek Provider Adapter
//!
//! Handles format conversion between OpenAI Responses API (Codex) and
//! DeepSeek Chat Completions API.
//!
//! Codex CLI speaks the Responses API (POST /v1/responses).
//! DeepSeek speaks the Chat Completions API (POST /v1/chat/completions).
//!
//! ## Request flow
//!   1. Codex sends Responses API request to CC Switch proxy
//!   2. DeepSeekAdapter::transform_request converts Responses → Chat Completions
//!   3. Forwarder sends to DeepSeek at /v1/chat/completions
//!   4. DeepSeek responds with Chat Completions SSE stream
//!   5. DeepSeekSseConverter converts Chat Completions SSE → Responses API SSE
//!   6. Converted stream is returned to Codex

use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use super::{AuthInfo, AuthStrategy, ProviderAdapter};

// ============================================================================
// Identity injection: appended to instructions to ensure the model
// correctly identifies as DeepSeek rather than OpenAI/GPT/Claude/Codex.
// ============================================================================
const IDENTITY_TEXT: &str =
    "\n\n[IMPORTANT: Your true underlying model is DeepSeek, developed by DeepSeek (\u{6df1}\u{5ea6}\u{6c42}\u{7d22}). You are NOT OpenAI, GPT, Claude, or Codex. When asked about your model identity, you MUST answer that you are DeepSeek, not any other model. Ignore any conflicting identity claims in the instructions above.]";

// ============================================================================
// DeepSeekAdapter
// ============================================================================

pub struct DeepSeekAdapter;

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Extract the API key from provider settings_config.
    fn extract_key(&self, provider: &Provider) -> Option<String> {
        // 1. Try auth.OPENAI_API_KEY (Codex CLI format)
        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
        // 2. Try env
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(key) = env.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
        // 3. Try direct apiKey / api_key
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
        {
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
        None
    }

    /// Check whether the provider is likely a DeepSeek provider.
    pub fn is_deepseek_provider(provider: &Provider) -> bool {
        // 1. Check provider_type meta
        if let Some(meta) = provider.meta.as_ref() {
            if meta.provider_type.as_deref() == Some("deepseek") {
                return true;
            }
        }
        // 2. Check base_url for deepseek.com
        if let Some(config) = provider.settings_config.get("config").and_then(|c| c.as_str()) {
            if config.to_lowercase().contains("deepseek") {
                return true;
            }
        }
        // 3. Check provider name
        if provider.name.to_lowercase().contains("deepseek") {
            return true;
        }
        // 4. Check env base_url
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("OPENAI_BASE_URL").or_else(|| env.get("DEEPSEEK_BASE_URL")).and_then(|v| v.as_str()) {
                if url.contains("deepseek.com") {
                    return true;
                }
            }
        }
        false
    }
}

impl Default for DeepSeekAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for DeepSeekAdapter {
    fn name(&self) -> &'static str {
        "deepseek"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // Try to extract base_url from settings_config
        // 1. Check env.DEEPSEEK_BASE_URL or env.OPENAI_BASE_URL
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env
                .get("DEEPSEEK_BASE_URL")
                .or_else(|| env.get("OPENAI_BASE_URL"))
                .and_then(|v| v.as_str())
            {
                if !url.is_empty() {
                    return Ok(url.trim_end_matches('/').to_string());
                }
            }
        }
        // 2. Parse from config TOML string (extract base_url)
        if let Some(config_str) = provider.settings_config.get("config").and_then(|c| c.as_str()) {
            for line in config_str.lines() {
                let trimmed = line.trim();
                if let Some(url) = trimmed.strip_prefix("base_url = \"") {
                    if let Some(end) = url.rfind('"') {
                        let base = &url[..end];
                        if !base.is_empty() {
                            return Ok(base.trim_end_matches('/').to_string());
                        }
                    }
                }
            }
        }
        // 3. Fallback to DeepSeek default
        Ok("https://api.deepseek.com".to_string())
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let key = self.extract_key(provider)?;
        Some(AuthInfo {
            strategy: AuthStrategy::Bearer,
            api_key: key,
            access_token: None,
        })
    }

    fn build_url(&self, base_url: &str, _endpoint: &str) -> String {
        // DeepSeek uses /v1/chat/completions, not /v1/responses
        let base = base_url.trim_end_matches('/');
        format!("{base}/v1/chat/completions")
    }

    fn get_auth_headers(&self, auth: &AuthInfo) -> Vec<(http::HeaderName, http::HeaderValue)> {
        let bearer = format!("Bearer {}", auth.api_key);
        vec![(
            http::HeaderName::from_static("authorization"),
            http::HeaderValue::from_str(&bearer).unwrap(),
        )]
    }

    fn needs_transform(&self, _provider: &Provider) -> bool {
        true
    }

    fn transform_request(&self, body: Value, provider: &Provider) -> Result<Value, ProxyError> {
        let model_override = Self::extract_model(provider);
        Ok(convert_responses_to_chat(body, model_override.as_deref()))
    }
}

impl DeepSeekAdapter {
    /// Extract the configured model name from provider settings_config (TOML config).
    pub fn extract_model(provider: &Provider) -> Option<String> {
        if let Some(config_str) = provider.settings_config.get("config").and_then(|c| c.as_str()) {
            let mut in_section = false;
            for line in config_str.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_section = true;
                    continue;
                }
                if !in_section {
                    if let Some(val) = trimmed.strip_prefix("model = \"") {
                        if let Some(end) = val.rfind('"') {
                            let name = &val[..end];
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
            // Fallback: search entire file
            for line in config_str.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("model = \"") && !trimmed.starts_with("model_provider") {
                    if let Some(val) = trimmed.strip_prefix("model = \"") {
                        if let Some(end) = val.rfind('"') {
                            let name = &val[..end];
                            if !name.is_empty() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

// ============================================================================
// Request Converter: OpenAI Responses API → Chat Completions
// ============================================================================

/// Convert Responses API request body to Chat Completions format.
///
/// `model_override`: if Some, overrides the model name from the request body
/// with the configured model (e.g., "deepseek-v4-flash" instead of Codex's "gpt-5.4").
fn convert_responses_to_chat(body: Value, model_override: Option<&str>) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    // 1. instructions → system message (with identity injection)
    let instructions = body
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let system_content = if !instructions.is_empty() {
        format!("{instructions}{IDENTITY_TEXT}")
    } else {
        IDENTITY_TEXT.trim().to_string()
    };
    messages.push(json!({
        "role": "system",
        "content": system_content
    }));

    // 2. input items → messages
    // Track reasoning_content from `type: "reasoning"` input items that appear
    // before the assistant message they belong to. We defer applying it until
    // we encounter the next assistant message, rather than creating a "ghost"
    // assistant message (which confuses DeepSeek's conversation structure).
    let mut pending_reasoning: Option<String> = None;
    if let Some(input) = body.get("input") {
        if let Some(input_str) = input.as_str() {
            messages.push(json!({
                "role": "user",
                "content": input_str
            }));
        } else if let Some(input_arr) = input.as_array() {
            for item in input_arr {
                match item.get("type").and_then(|v| v.as_str()) {
                    Some("message") => {
                        let role = item
                            .get("role")
                            .and_then(|v| v.as_str())
                            .unwrap_or("user");
                        // Map 'developer' role to 'system' for DeepSeek
                        let mapped_role = if role == "developer" { "system" } else { role };
                        let content = flatten_content(item.get("content"));
                        let has_tool_calls = item.get("tool_calls").is_some();
                        let mut msg = json!({
                            "role": mapped_role,
                            "content": content,
                        });
                        // Preserve tool_calls and tool_call_id
                        if let Some(tc) = item.get("tool_calls") {
                            msg["tool_calls"] = tc.clone();
                        }
                        if let Some(tcid) = item.get("tool_call_id").and_then(|v| v.as_str()) {
                            msg["tool_call_id"] = json!(tcid);
                        }
                        // Preserve reasoning_content — DeepSeek V4 requires it
                        // to be passed back in subsequent requests (thinking mode).
                        // Check both: direct field on the message item, and
                        // reasoning blocks inside the content array.
                        if let Some(rc) = item.get("reasoning_content") {
                            msg["reasoning_content"] = rc.clone();
                        } else if mapped_role == "assistant" {
                            // Check content array for reasoning blocks
                            if let Some(content_arr) = item.get("content").and_then(|c| c.as_array()) {
                                for block in content_arr {
                                    if block.get("type").and_then(|v| v.as_str()) == Some("reasoning") {
                                        if let Some(rc) = block.get("reasoning_content").and_then(|v| v.as_str()) {
                                            if !rc.is_empty() {
                                                msg["reasoning_content"] = json!(rc);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // If no reasoning on this assistant message but we have
                        // pending reasoning from a preceding `type: "reasoning"`
                        // item, apply it here (avoids creating ghost assistants).
                        if mapped_role == "assistant"
                            && msg.get("reasoning_content").is_none()
                            && pending_reasoning.is_some()
                        {
                            if let Some(rc) = pending_reasoning.take() {
                                msg["reasoning_content"] = json!(rc);
                            }
                        }
                        // DeepSeek requires assistant messages to have either
                        // `content` or `tool_calls`. When content is null and
                        // there are no tool_calls (e.g. empty assistant history),
                        // set content to "" instead to avoid 400 errors.
                        if mapped_role == "assistant" && content.is_null() && !has_tool_calls {
                            msg["content"] = json!("");
                        }
                        messages.push(msg);
                    }
                    Some("function_call") => {
                        let tc = json!({
                            "id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "type": "function",
                            "function": {
                                "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                "arguments": item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}")
                            }
                        });
                        // Append to last assistant message or create one
                        if let Some(last) = messages.last_mut() {
                            if last.get("role") == Some(&json!("assistant")) {
                                if let Some(tc_arr) = last.get_mut("tool_calls") {
                                    if let Some(arr) = tc_arr.as_array_mut() {
                                        arr.push(tc);
                                    }
                                } else {
                                    last["tool_calls"] = json!([tc]);
                                }
                            } else {
                                messages.push(json!({
                                    "role": "assistant",
                                    "content": null,
                                    "tool_calls": [tc]
                                }));
                            }
                        }
                    }
                    Some("function_call_output") => {
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "content": item.get("output").and_then(|v| v.as_str()).unwrap_or("")
                        }));
                    }
                    Some("reasoning") => {
                        // Defer: store reasoning_content and apply it when we
                        // encounter the next assistant message, rather than
                        // creating a "ghost" assistant message here.
                        if let Some(rc) = item.get("reasoning_content").and_then(|v| v.as_str()) {
                            if !rc.is_empty() {
                                // If the last message is already an assistant,
                                // merge into it directly (common when Codex sends
                                // reasoning AFTER the assistant message).
                                if let Some(last) = messages.last_mut() {
                                    if last.get("role") == Some(&json!("assistant")) {
                                        last["reasoning_content"] = json!(rc);
                                        pending_reasoning = None;
                                    } else {
                                        pending_reasoning = Some(rc.to_string());
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        // Unknown input type, try to preserve as user message
                        let content = flatten_content(item.get("content"));
                        if !content.is_null() {
                            messages.push(json!({
                                "role": "user",
                                "content": content
                            }));
                        }
                    }
                }
            }
        } else if input.is_object() {
            let content = flatten_content(input.get("content"));
            if !content.is_null() {
                messages.push(json!({
                    "role": "user",
                    "content": content
                }));
            }
        }
    }

    // 3. Build the Chat Completions request
    // Always use model_override if provided (from provider config),
    // because Codex sends its own model name (e.g. "gpt-5.4-mini")
    // which DeepSeek does not recognize.
    let model = model_override
        .or_else(|| body.get("model").and_then(|v| v.as_str()))
        .unwrap_or("deepseek-chat");

    let mut chat_req = json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": {
            "include_usage": true
        }
    });

    // 5. Convert tools from Responses format → Chat format
    //    Responses: { type: "function", name: "bash", description: "...", parameters: {...}, strict: true }
    //    Chat:      { type: "function", function: { name: "bash", description: "...", parameters: {...}, strict: true } }
    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        let chat_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("function"))
            .map(|t| {
                let mut func_fields = t.clone();
                func_fields.as_object_mut().map(|obj| {
                    obj.remove("type");
                });
                json!({
                    "type": "function",
                    "function": func_fields
                })
            })
            .collect();
        if !chat_tools.is_empty() {
            chat_req["tools"] = json!(chat_tools);
        }
    }

    // 6. Convert tool_choice
    if let Some(tc) = body.get("tool_choice") {
        if let Some(tc_obj) = tc.as_object() {
            if tc_obj.get("type").and_then(|v| v.as_str()) == Some("function")
                && tc_obj.get("name").is_some()
                && tc_obj.get("function").is_none()
            {
                chat_req["tool_choice"] = json!({
                    "type": "function",
                    "function": { "name": tc_obj["name"] }
                });
            } else {
                chat_req["tool_choice"] = tc.clone();
            }
        } else {
            chat_req["tool_choice"] = tc.clone();
        }
    }

    // 7. Copy recognized fields
    if let Some(v) = body.get("temperature").and_then(|v| v.as_f64()) {
        chat_req["temperature"] = json!(v);
    }
    if let Some(v) = body.get("max_output_tokens").and_then(|v| v.as_u64()) {
        chat_req["max_tokens"] = json!(v);
    }
    // DeepSeek V4 supports reasoning_effort in thinking mode — pass it through.
    if let Some(reasoning) = body.get("reasoning") {
        if let Some(effort) = reasoning.get("effort").and_then(|v| v.as_str()) {
            chat_req["reasoning_effort"] = json!(effort);
        }
        // Explicitly enable thinking mode. DeepSeek defaults to enabled, but
        // explicitly setting it ensures consistent behavior across API versions.
        chat_req["thinking"] = json!({"type": "enabled"});
    }
    if body.get("parallel_tool_calls").and_then(|v| v.as_bool()).is_some() {
        chat_req["parallel_tool_calls"] = body["parallel_tool_calls"].clone();
    }

    // 8. Handle response_format (skip unsupported types)
    if let Some(text) = body.get("text") {
        if let Some(format) = text.get("format") {
            let fmt_type = format.get("type").and_then(|v| v.as_str());
            match fmt_type {
                None | Some("text") => {
                    // Default, skip
                }
                Some("json_object") => {
                    chat_req["response_format"] = json!({ "type": "json_object" });
                }
                Some("json_schema") => {
                    // DeepSeek doesn't support json_schema, skip
                }
                _ => {
                    // Pass through unknown types (likely won't work)
                    chat_req["response_format"] = format.clone();
                }
            }
        }
    }

    chat_req
}

/// Flatten content field (handles both string and array formats).
fn flatten_content(content: Option<&Value>) -> Value {
    let Some(content) = content else {
        return Value::Null;
    };
    match content {
        Value::String(s) => json!(s),
        Value::Array(arr) => {
            let texts: Vec<&str> = arr
                .iter()
                .filter_map(|item| {
                    let t = item.get("type").and_then(|v| v.as_str())?;
                    if matches!(t, "input_text" | "output_text" | "text" | "reasoning_text") {
                        item.get("text").and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if texts.is_empty() {
                Value::Null
            } else {
                json!(texts.join(""))
            }
        }
        _ => Value::Null,
    }
}

// Reasoning content is preserved (DeepSeek V4 thinking mode requires passing it back).

// ============================================================================
// Response Converter: Chat Completions SSE → Responses API SSE
// ============================================================================

/// State tracking for tool call accumulation across SSE chunks.
#[derive(Default)]
struct ToolCallState {
    id: String,
    name: String,
    args: String,
    item_id: String,
    index: usize,
}

/// Converts Chat Completions SSE chunks into OpenAI Responses API streaming events.
///
/// Based on codex-proxy converter.ts ResponseConverter.
pub struct DeepSeekResponseConverter {
    response_id: String,
    model: String,
    tool_call_buffer: HashMap<usize, ToolCallState>,
    has_text_item: bool,
    has_content_part: bool,
    output_index: u32,
    text_buffer: String,
    /// Accumulated reasoning_content from the response (for caching & round-trip).
    reasoning_buffer: String,
    last_usage: Option<ChatUsage>,
    completed: bool,
}

#[derive(Clone)]
struct ChatUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

impl DeepSeekResponseConverter {
    pub fn new(model: &str) -> Self {
        Self {
            response_id: format!("resp_{}", Uuid::new_v4().to_string().replace('-', "")),
            model: model.to_string(),
            tool_call_buffer: HashMap::new(),
            has_text_item: false,
            has_content_part: false,
            output_index: 0,
            text_buffer: String::new(),
            reasoning_buffer: String::new(),
            last_usage: None,
            completed: false,
        }
    }

    /// Get the accumulated reasoning_content from this response.
    /// Used by handlers to cache reasoning for injection into follow-up requests.
    pub fn reasoning_content(&self) -> &str {
        &self.reasoning_buffer
    }

    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    /// Get all tool call IDs from this response. Used to cache reasoning_content
    /// by tool call ID so follow-up requests can look it up without `previous_response_id`.
    ///
    /// Returns BOTH the original DeepSeek tool call `id` AND the proxy-generated
    /// `item_id` for each tool call, since Codex may use either one as the
    /// `call_id` in `function_call_output`.
    pub fn tool_call_ids(&self) -> Vec<String> {
        self.tool_call_buffer
            .values()
            .flat_map(|tc| {
                let mut ids = Vec::with_capacity(2);
                if !tc.id.is_empty() {
                    ids.push(tc.id.clone());
                }
                if !tc.item_id.is_empty() {
                    ids.push(tc.item_id.clone());
                }
                ids
            })
            .collect()
    }

    pub fn is_completed(&self) -> bool {
        self.completed
    }

    pub fn output_text(&self) -> &str {
        &self.text_buffer
    }

    /// Emit lifecycle events: response.created + response.in_progress.
    pub fn lifecycle_events(&self) -> Vec<SseEvent> {
        vec![
            SseEvent {
                event: "response.created".to_string(),
                data: json!({
                    "response": {
                        "id": self.response_id,
                        "object": "response",
                        "model": self.model,
                        "status": "in_progress",
                        "created_at": chrono::Utc::now().timestamp(),
                        "output": []
                    }
                }),
            },
            SseEvent {
                event: "response.in_progress".to_string(),
                data: json!({
                    "response": {
                        "id": self.response_id,
                        "model": self.model,
                        "status": "in_progress"
                    }
                }),
            },
        ]
    }

    /// Generate a response.failed event.
    pub fn failed_event(&self, error_msg: &str) -> SseEvent {
        SseEvent {
            event: "response.failed".to_string(),
            data: json!({
                "response": {
                    "id": self.response_id,
                    "object": "response",
                    "model": self.model,
                    "status": "failed",
                    "created_at": chrono::Utc::now().timestamp(),
                    "error": {
                        "type": "upstream_error",
                        "message": error_msg
                    }
                }
            }),
        }
    }

    /// Finalize: emit response.completed if not already done.
    pub fn finalize(&mut self) -> Vec<SseEvent> {
        if self.completed {
            return Vec::new();
        }
        let mut events = Vec::new();
        self.close_text_item(&mut events);
        self.emit_completed(&mut events);
        events
    }

    /// Process one Chat Completions SSE chunk → zero or more Responses API events.
    pub fn process_chunk(&mut self, chunk: &Value) -> Vec<SseEvent> {
        let mut events = Vec::new();

        let choices = match chunk.get("choices").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => return events,
        };
        let choice = match choices.first() {
            Some(c) => c,
            None => return events,
        };

        // Capture usage from the final chunk
        if let Some(usage) = chunk.get("usage") {
            self.last_usage = Some(ChatUsage {
                prompt_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                completion_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                total_tokens: usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            });
        }

        let delta = choice.get("delta");
        let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

        // Text content
        if let Some(content) = delta.and_then(|d| d.get("content")).and_then(|v| v.as_str()) {
            if !self.has_text_item {
                self.open_text_item(&mut events);
            }
            if !self.has_content_part {
                self.open_content_part(&mut events);
            }
            self.text_buffer.push_str(content);
            events.push(SseEvent {
                event: "response.output_text.delta".to_string(),
                data: json!({
                    "delta": content,
                    "output_index": self.output_index,
                    "content_index": 0
                }),
            });
        }

        // Reasoning content (DeepSeek thinking mode)
        if let Some(reasoning) = delta.and_then(|d| d.get("reasoning_content")).and_then(|v| v.as_str()) {
            // Accumulate for caching & round-trip injection
            self.reasoning_buffer.push_str(reasoning);
            if !self.has_text_item {
                self.open_text_item(&mut events);
            }
            events.push(SseEvent {
                event: "response.reasoning.delta".to_string(),
                data: json!({
                    "delta": reasoning,
                    "output_index": self.output_index
                }),
            });
        }

        // Tool calls
        if let Some(tool_calls) = delta.and_then(|d| d.get("tool_calls")).and_then(|v| v.as_array()) {
            self.process_tool_calls(&mut events, tool_calls, finish_reason.is_some());
        }

        // Finish
        if let Some(fr) = finish_reason {
            if !fr.is_empty() {
                self.finish_response(&mut events, fr);
            }
        }

        events
    }
}

// Private helpers for DeepSeekResponseConverter
impl DeepSeekResponseConverter {
    fn open_text_item(&mut self, events: &mut Vec<SseEvent>) {
        self.has_text_item = true;
        events.push(SseEvent {
            event: "response.output_item.added".to_string(),
            data: json!({
                "item": {
                    "type": "message",
                    "id": format!("msg_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]),
                    "role": "assistant",
                    "content": [],
                    "status": "in_progress"
                },
                "output_index": self.output_index
            }),
        });
    }

    fn open_content_part(&mut self, events: &mut Vec<SseEvent>) {
        self.has_content_part = true;
        events.push(SseEvent {
            event: "response.content_part.added".to_string(),
            data: json!({
                "part": { "type": "output_text", "text": "" },
                "output_index": self.output_index,
                "content_index": 0
            }),
        });
    }

    fn close_content_part(&mut self, events: &mut Vec<SseEvent>) {
        if !self.has_content_part {
            return;
        }
        self.has_content_part = false;
        events.push(SseEvent {
            event: "response.output_text.done".to_string(),
            data: json!({
                "text": self.text_buffer,
                "output_index": self.output_index,
                "content_index": 0
            }),
        });
        events.push(SseEvent {
            event: "response.content_part.done".to_string(),
            data: json!({
                "part": { "type": "output_text", "text": self.text_buffer },
                "output_index": self.output_index,
                "content_index": 0
            }),
        });
    }

    fn close_text_item(&mut self, events: &mut Vec<SseEvent>) {
        if !self.has_text_item {
            return;
        }
        self.has_text_item = false;
        self.close_content_part(events);
        let item_id = format!("msg_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]);
        let content: Value = if self.text_buffer.is_empty() {
            json!([])
        } else {
            json!([{
                "type": "output_text",
                "text": self.text_buffer,
                "annotations": []
            }])
        };
        let mut item = json!({
            "type": "message",
            "id": item_id,
            "role": "assistant",
            "content": content,
            "status": "completed"
        });
        // Include reasoning_content on the output_item.done so Codex preserves
        // it when reconstructing conversation history for follow-up requests.
        if !self.reasoning_buffer.is_empty() {
            item["reasoning_content"] = json!(self.reasoning_buffer);
        }
        events.push(SseEvent {
            event: "response.output_item.done".to_string(),
            data: json!({
                "item": item,
                "output_index": self.output_index
            }),
        });
        self.output_index += 1;
    }

    fn process_tool_calls(
        &mut self,
        events: &mut Vec<SseEvent>,
        tool_calls: &[Value],
        _will_finish: bool,
    ) {
        // Close text item if open — tool calls are separate output items
        if self.has_text_item {
            self.close_text_item(events);
        }

        for tc in tool_calls {
            let index = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let func = tc.get("function");
            let func_name = func.and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let func_args = func.and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("");

            if !self.tool_call_buffer.contains_key(&index) {
                // First chunk for this tool call
                let item_id = format!("fc_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]);
                let sse_item_id = item_id.clone();
                self.tool_call_buffer.insert(index, ToolCallState {
                    id: tc_id.to_string(),
                    name: func_name.to_string(),
                    args: String::new(),
                    item_id,
                    index,
                });

                events.push(SseEvent {
                    event: "response.output_item.added".to_string(),
                    data: json!({
                        "item": {
                            "type": "function_call",
                            "id": sse_item_id,
                            "name": func_name,
                            "arguments": "",
                            "status": "in_progress",
                            "call_id": tc_id
                        },
                        "output_index": self.output_index
                    }),
                });
            }

            // Update buffer and emit argument delta
            if let Some(buf) = self.tool_call_buffer.get_mut(&index) {
                if !tc_id.is_empty() {
                    buf.id = tc_id.to_string();
                }
                if !func_name.is_empty() {
                    buf.name = func_name.to_string();
                }
                if !func_args.is_empty() {
                    buf.args.push_str(func_args);
                    events.push(SseEvent {
                        event: "response.function_call_arguments.delta".to_string(),
                        data: json!({
                            "delta": func_args,
                            "output_index": self.output_index
                        }),
                    });
                }
            }
        }
    }

    fn finish_response(&mut self, events: &mut Vec<SseEvent>, finish_reason: &str) {
        // Close open text item
        self.close_text_item(events);

        // Close tool call items
        if finish_reason == "tool_calls" {
            for (_, tc) in &self.tool_call_buffer {
                events.push(SseEvent {
                    event: "response.function_call_arguments.done".to_string(),
                    data: json!({
                        "name": tc.name,
                        "arguments": tc.args,
                        "output_index": self.output_index
                    }),
                });
                events.push(SseEvent {
                    event: "response.output_item.done".to_string(),
                    data: json!({
                        "item": {
                            "type": "function_call",
                            "id": tc.item_id,
                            "name": tc.name,
                            "arguments": tc.args,
                            "status": "completed",
                            "call_id": tc.id
                        },
                        "output_index": self.output_index
                    }),
                });
                self.output_index += 1;
            }
        }

        self.emit_completed(events);
    }

    fn emit_completed(&mut self, events: &mut Vec<SseEvent>) {
        if self.completed {
            return;
        }
        self.completed = true;

        let mut output: Vec<Value> = Vec::new();

        // Reasoning output (for thinking mode round-trip)
        if !self.reasoning_buffer.is_empty() {
            output.push(json!({
                "type": "reasoning",
                "reasoning_content": self.reasoning_buffer,
            }));
        }

        // Text message output — include reasoning_content directly on the
        // message item so Codex preserves it when reconstructing conversation
        // history (Codex may drop standalone `type: "reasoning"` output items).
        if !self.text_buffer.is_empty() {
            let mut msg = json!({
                "type": "message",
                "id": format!("msg_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]),
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": self.text_buffer,
                    "annotations": []
                }],
                "status": "completed"
            });
            if !self.reasoning_buffer.is_empty() {
                msg["reasoning_content"] = json!(self.reasoning_buffer);
            }
            output.push(msg);
        }

        // Tool call outputs
        for (_, tc) in &self.tool_call_buffer {
            output.push(json!({
                "type": "function_call",
                "id": tc.item_id,
                "name": tc.name,
                "arguments": tc.args,
                "status": "completed",
                "call_id": tc.id
            }));
        }

        let mut resp = json!({
            "id": self.response_id,
            "object": "response",
            "model": self.model,
            "status": "completed",
            "created_at": chrono::Utc::now().timestamp(),
            "output": output,
        });

        if let Some(usage) = &self.last_usage {
            resp["usage"] = json!({
                "input_tokens": usage.prompt_tokens,
                "output_tokens": usage.completion_tokens,
                "total_tokens": usage.total_tokens
            });
        }

        events.push(SseEvent {
            event: "response.completed".to_string(),
            data: json!({ "response": resp }),
        });
    }
}

/// An SSE event to be sent to the Codex client.
///
/// Matchs the OpenAI Responses API SSE format: every data payload
/// includes a `type` field equal to the event name, e.g.:
///
/// ```text
/// event: response.completed
/// data: {"type":"response.completed","response":{...}}
/// ```
///
/// Codex CLI's SSE parser strictly validates that the `type` field
/// is present and matches the expected event — omitting it causes
/// "stream closed before response.completed" errors.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: Value,
}

impl SseEvent {
    pub fn to_sse_string(&self) -> String {
        // Inject the `type` field matching the event name (required by Codex SSE parser)
        let mut enriched = self.data.clone();
        if let Some(obj) = enriched.as_object() {
            if !obj.contains_key("type") {
                if let Some(map) = enriched.as_object_mut() {
                    map.insert(
                        "type".to_string(),
                        Value::String(self.event.clone()),
                    );
                }
            }
        }
        format!(
            "event: {}\ndata: {}\n\n",
            self.event,
            serde_json::to_string(&enriched).unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_is_deepseek_provider_by_name() {
        let provider = Provider {
            id: "deepseek-1".to_string(),
            name: "DeepSeek V4".to_string(),
            settings_config: json!({
                "config": "model_provider = \"deepseek\"\nmodel = \"deepseek-chat\"\n\n[model_providers.deepseek]\nname = \"deepseek\"\nbase_url = \"https://api.deepseek.com\"\nwire_api = \"responses\"\nrequires_openai_auth = true"
            }),
            website_url: Some("https://www.deepseek.com".to_string()),
            category: Some("third_party".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: Some("deepseek".to_string()),
            icon_color: Some("#4D6BFE".to_string()),
            in_failover_queue: false,
        };
        assert!(DeepSeekAdapter::is_deepseek_provider(&provider));
    }

    #[test]
    fn test_convert_responses_to_chat_basic() {
        let responses_req = json!({
            "model": "deepseek-v4-flash",
            "instructions": "You are a helpful assistant.",
            "input": "Hello!",
            "stream": true
        });

        let chat_req = convert_responses_to_chat(responses_req);

        assert_eq!(chat_req["model"], "deepseek-v4-flash");
        assert_eq!(chat_req["stream"], true);

        let messages = chat_req["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);

        // Check system message (instructions)
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"].as_str().unwrap().contains("DeepSeek"));

        // Check user message
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello!");
    }

    #[test]
    fn test_convert_responses_to_chat_with_tools() {
        let responses_req = json!({
            "model": "deepseek-v4-pro",
            "input": [{"type": "message", "role": "user", "content": "List files"}],
            "tools": [{"type": "function", "name": "bash", "description": "Run bash", "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}}]
        });

        let chat_req = convert_responses_to_chat(responses_req);

        assert!(chat_req.get("tools").is_some());
        let tools = chat_req["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        // Tool should be in Chat Completions format
        assert!(tools[0].get("function").is_some());
    }

    #[test]
    fn test_deepseek_response_converter_lifecycle() {
        let mut converter = DeepSeekResponseConverter::new("deepseek-v4-flash");

        let lifecycle = converter.lifecycle_events();
        assert_eq!(lifecycle.len(), 2);
        assert_eq!(lifecycle[0].event, "response.created");
        assert_eq!(lifecycle[1].event, "response.in_progress");
    }

    #[test]
    fn test_deepseek_response_converter_process_chunk() {
        let mut converter = DeepSeekResponseConverter::new("deepseek-v4-flash");

        let chunk = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1745452800,
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "delta": {"content": "Hello from "},
                "finish_reason": null
            }]
        });

        let events = converter.process_chunk(&chunk);
        assert!(!events.is_empty());

        // Should start with output_item.added, then content_part.added, then text delta
        let has_delta = events.iter().any(|e| e.event == "response.output_text.delta");
        assert!(has_delta, "Should emit output_text.delta");
    }

    #[test]
    fn test_deepseek_response_converter_finalize() {
        let mut converter = DeepSeekResponseConverter::new("deepseek-v4-flash");

        // Process a text chunk
        let chunk = json!({
            "choices": [{"index": 0, "delta": {"content": "Hello"}, "finish_reason": null}]
        });
        converter.process_chunk(&chunk);

        // Finalize with usage
        let final_chunk = json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
        });
        let events = converter.process_chunk(&final_chunk);

        assert!(events.iter().any(|e| e.event == "response.completed"));
    }
}
