//! OpenAI Responses API 格式转换模块
//!
//! 实现 Anthropic Messages ↔ OpenAI Responses API 格式转换。
//! Responses API 是 OpenAI 2025 年推出的新一代 API，采用扁平化的 input/output 结构。
//!
//! 与 Chat Completions 的主要差异：
//! - tool_use/tool_result 从 message content 中"提升"为顶层 input item
//! - system prompt 使用 `instructions` 字段而非 system role message
//! - usage 字段命名与 Anthropic 一致 (input_tokens/output_tokens)

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// Anthropic 请求 → OpenAI Responses 请求
///
/// `cache_key`: optional prompt_cache_key to inject for improved cache routing
/// `is_codex_oauth`: 当目标后端是 ChatGPT Plus/Pro 反代 (`chatgpt.com/backend-api/codex`) 时为 true。
/// 该后端强制要求 `store: false`，并要求 `include` 包含 `reasoning.encrypted_content`
/// 以便在无服务端状态下保持多轮 reasoning 上下文。
/// `codex_fast_mode`: 仅在 `is_codex_oauth` 为 true 时生效，控制是否注入
/// `service_tier = "priority"`。
pub fn anthropic_to_responses(
    body: Value,
    cache_key: Option<&str>,
    is_codex_oauth: bool,
    codex_fast_mode: bool,
) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // system → instructions (Responses API 使用 instructions 字段)
    if let Some(system) = body.get("system") {
        let instructions = if let Some(text) = system.as_str() {
            super::transform::strip_leading_anthropic_billing_header(text).to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|msg| msg.get("text").and_then(|t| t.as_str()))
                .map(super::transform::strip_leading_anthropic_billing_header)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            String::new()
        };
        if !instructions.is_empty() {
            result["instructions"] = json!(instructions);
        }
    }

    // messages → input
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        let input = convert_messages_to_input(msgs)?;
        result["input"] = json!(input);
    }

    // max_tokens → max_output_tokens (Responses API uses max_output_tokens for all models)
    if let Some(v) = body.get("max_tokens") {
        result["max_output_tokens"] = v.clone();
    }

    // 直接透传的参数
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // Map Anthropic thinking → OpenAI Responses reasoning.effort
    if let Some(model_name) = body.get("model").and_then(|m| m.as_str()) {
        if super::transform::supports_reasoning_effort(model_name) {
            if let Some(effort) = super::transform::resolve_reasoning_effort(&body) {
                result["reasoning"] = json!({ "effort": effort });
            }
        }
    }

    // stop_sequences → 丢弃 (Responses API 不支持)

    // 转换 tools (过滤 BatchTool)
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let response_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": t.get("description"),
                    "parameters": super::transform::clean_schema(
                        t.get("input_schema").cloned().unwrap_or(json!({}))
                    )
                })
            })
            .collect();

        if !response_tools.is_empty() {
            result["tools"] = json!(response_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice_to_responses(v);
    }

    // Inject prompt_cache_key for improved cache routing on OpenAI-compatible endpoints
    if let Some(key) = cache_key {
        result["prompt_cache_key"] = json!(key);
    }

    // Codex OAuth (ChatGPT Plus/Pro 反代) 特殊协议约束：
    // 整体依据：OpenAI 官方 codex-rs 的 `ResponsesApiRequest` 结构体
    // (codex-rs/codex-api/src/common.rs) 是 ChatGPT 反代后端的协议契约。
    // 任何不在该结构体里的字段都可能被 ChatGPT 后端以
    // "Unsupported parameter: ..." 400 拒绝；任何在结构体里的必填字段
    // 都需要在请求体里出现。
    //
    // 字段处理：
    // - store: 必须显式为 false（ChatGPT 消费级后端不允许服务端持久化）
    // - include: 必须包含 "reasoning.encrypted_content"，
    //   否则多轮 reasoning 中间态会丢失（无服务端状态 + 无加密回传 = 上下文断链）
    // - max_output_tokens / temperature / top_p: 必须删除
    //   （codex-rs 结构体根本没有这三个字段，OpenAI 自己的客户端不发它们）
    // - instructions / tools / parallel_tool_calls: 必填字段，缺则兜底默认值
    //   （cc-switch 的 transform 当前是"条件写入"，可能产生缺失）
    // - service_tier: 仅在 FAST mode 开启时写入 "priority"
    //   （与 OpenAI 官方 codex-rs 当前请求结构保持一致）
    // - stream: 必须永远 true（codex-rs 硬编码 true，且 cc-switch 的
    //   SSE 解析层只处理流式响应，强制覆盖避免客户端误传 false）
    if is_codex_oauth {
        result["store"] = json!(false);
        if codex_fast_mode {
            result["service_tier"] = json!("priority");
        }

        const REASONING_MARKER: &str = "reasoning.encrypted_content";
        let mut includes: Vec<Value> = body
            .get("include")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if !includes
            .iter()
            .any(|v| v.as_str() == Some(REASONING_MARKER))
        {
            includes.push(json!(REASONING_MARKER));
        }
        result["include"] = json!(includes);

        if let Some(obj) = result.as_object_mut() {
            // —— 删除 ChatGPT 反代不接受的字段 ——
            obj.remove("max_output_tokens");
            obj.remove("temperature");
            obj.remove("top_p");

            // —— 兜底必填字段（or_insert：客户端送了什么就保留，否则注入默认值）——
            obj.entry("instructions".to_string()).or_insert(json!(""));
            obj.entry("tools".to_string()).or_insert(json!([]));
            obj.entry("parallel_tool_calls".to_string())
                .or_insert(json!(false));

            // —— 强制覆盖 stream = true ——
            // 即便客户端误传 stream:false 也要覆盖，因为 codex-rs 永远 true，
            // 且 cc-switch SSE 解析层只支持流式响应。
            obj.insert("stream".to_string(), json!(true));
        }
    }

    Ok(result)
}

fn map_tool_choice_to_responses(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(_) => tool_choice.clone(),
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            // Anthropic "any" means at least one tool call is required
            Some("any") => json!("required"),
            Some("auto") => json!("auto"),
            Some("none") => json!("none"),
            // Anthropic forced tool -> Responses function tool selector
            Some("tool") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({
                    "type": "function",
                    "name": name
                })
            }
            _ => tool_choice.clone(),
        },
        _ => tool_choice.clone(),
    }
}

pub(crate) fn map_responses_stop_reason(
    status: Option<&str>,
    has_tool_use: bool,
    incomplete_reason: Option<&str>,
) -> Option<&'static str> {
    status.map(|s| match s {
        "completed" if has_tool_use => "tool_use",
        "incomplete"
            if matches!(
                incomplete_reason,
                Some("max_output_tokens") | Some("max_tokens")
            ) || incomplete_reason.is_none() =>
        {
            "max_tokens"
        }
        "incomplete" => "end_turn",
        _ => "end_turn",
    })
}

/// Build Anthropic-style usage JSON from Responses API usage, including cache tokens.
///
/// **Robustness Features**:
/// - Handles null, missing, empty objects, and partial objects gracefully
/// - Supports OpenAI field name variants (prompt_tokens/completion_tokens) as fallbacks
/// - Always returns valid structure: {"input_tokens": N, "output_tokens": N}
/// - Preserves cache token fields even when input/output tokens are missing
///
/// **Field Name Resolution Priority**:
/// 1. input_tokens: Anthropic `input_tokens` → OpenAI `prompt_tokens` → default 0
/// 2. output_tokens: Anthropic `output_tokens` → OpenAI `completion_tokens` → default 0
/// 3. cache_read_input_tokens: Direct field → nested input_tokens_details.cached_tokens → prompt_tokens_details.cached_tokens
/// 4. cache_creation_input_tokens: Direct field only
///
/// **Cache Token Priority Order**:
/// 1. OpenAI nested details (`input_tokens_details.cached_tokens`, `prompt_tokens_details.cached_tokens`) as initial value
/// 2. Direct Anthropic-style fields (`cache_read_input_tokens`, `cache_creation_input_tokens`) override if present
///
/// **Logging**:
/// - Warns on empty objects {} or partial objects (only one field present)
/// - Debug logs when using OpenAI field name fallbacks
pub(crate) fn build_anthropic_usage_from_responses(usage: Option<&Value>) -> Value {
    let u = match usage {
        Some(v) if !v.is_null() && v.is_object() => v,
        _ => {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0
            })
        }
    };

    // Detect empty object {} and log warning
    if u.as_object().map(|obj| obj.is_empty()).unwrap_or(false) {
        log::warn!("[Responses] Empty usage object received, using defaults");
        return json!({
            "input_tokens": 0,
            "output_tokens": 0
        });
    }

    // Extract input_tokens with OpenAI field name fallback
    // Priority: input_tokens (Anthropic) → prompt_tokens (OpenAI) → 0
    let input = u
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            let prompt_tokens = u.get("prompt_tokens").and_then(|v| v.as_u64());
            if prompt_tokens.is_some() {
                log::debug!(
                    "[Responses] Using OpenAI field name fallback 'prompt_tokens' for input_tokens"
                );
            }
            prompt_tokens
        })
        .unwrap_or(0);

    // Extract output_tokens with OpenAI field name fallback
    // Priority: output_tokens (Anthropic) → completion_tokens (OpenAI) → 0
    let output = u.get("output_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            let completion_tokens = u.get("completion_tokens").and_then(|v| v.as_u64());
            if completion_tokens.is_some() {
                log::debug!("[Responses] Using OpenAI field name fallback 'completion_tokens' for output_tokens");
            }
            completion_tokens
        })
        .unwrap_or(0);

    // Log if only one field present (partial object). Streaming chunks legitimately
    // arrive with partial usage, so this stays at debug level to avoid noise.
    if (input == 0 && output > 0) || (input > 0 && output == 0) {
        log::debug!("[Responses] Partial usage object: {:?}", u);
    }

    let mut result = json!({
        "input_tokens": input,
        "output_tokens": output
    });

    // Step 1: OpenAI nested details as fallback for cache tokens
    // OpenAI Responses API: input_tokens_details.cached_tokens
    if let Some(cached) = u
        .pointer("/input_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
    {
        result["cache_read_input_tokens"] = json!(cached);
    }
    // OpenAI standard: prompt_tokens_details.cached_tokens
    if let Some(cached) = u
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(|v| v.as_u64())
    {
        if result.get("cache_read_input_tokens").is_none() {
            result["cache_read_input_tokens"] = json!(cached);
        }
    }

    // Step 2: Direct Anthropic-style fields override (authoritative if present)
    // These preserve cache tokens even if input/output_tokens are missing
    if let Some(v) = u.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = v.clone();
    }
    if let Some(v) = u.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = v.clone();
    }

    result
}

/// 将 Anthropic messages 数组转换为 Responses API input 数组
///
/// 核心转换逻辑：
/// - user/assistant 的 text 内容 → 对应 role 的 message item
/// - tool_use 从 assistant message 中"提升"为独立的 function_call item
/// - tool_result 从 user message 中"提升"为独立的 function_call_output item
/// - thinking blocks → 丢弃
fn convert_messages_to_input(messages: &[Value]) -> Result<Vec<Value>, ProxyError> {
    let mut input = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let content = msg.get("content");

        match content {
            // 字符串内容
            Some(Value::String(text)) => {
                let content_type = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                input.push(json!({
                    "role": role,
                    "content": [{ "type": content_type, "text": text }]
                }));
            }

            // 数组内容（多模态/工具调用）
            Some(Value::Array(blocks)) => {
                let mut message_content = Vec::new();

                for block in blocks {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let content_type = if role == "assistant" {
                                    "output_text"
                                } else {
                                    "input_text"
                                };
                                // OpenAI Responses API does not accept Anthropic cache_control
                                // under input[].content[].
                                message_content.push(json!({ "type": content_type, "text": text }));
                            }
                        }

                        "image" => {
                            if let Some(source) = block.get("source") {
                                let media_type = source
                                    .get("media_type")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("image/png");
                                let data =
                                    source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                                message_content.push(json!({
                                    "type": "input_image",
                                    "image_url": format!("data:{media_type};base64,{data}")
                                }));
                            }
                        }

                        "tool_use" => {
                            // 先刷新已累积的消息内容
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            // 提升为独立的 function_call item
                            let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let arguments = block.get("input").cloned().unwrap_or(json!({}));

                            input.push(json!({
                                "type": "function_call",
                                "call_id": id,
                                "name": name,
                                "arguments": serde_json::to_string(&arguments).unwrap_or_default()
                            }));
                        }

                        "tool_result" => {
                            // 先刷新已累积的消息内容
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            // 提升为独立的 function_call_output item
                            let call_id = block
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("");
                            let output = match block.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(v) => serde_json::to_string(v).unwrap_or_default(),
                                None => String::new(),
                            };

                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": output
                            }));
                        }

                        "thinking" => {
                            // 丢弃 thinking blocks（与 openai_chat 一致）
                        }

                        _ => {}
                    }
                }

                // 刷新剩余的消息内容
                if !message_content.is_empty() {
                    input.push(json!({
                        "role": role,
                        "content": message_content
                    }));
                }
            }

            _ => {
                // 无内容或 null
                input.push(json!({ "role": role }));
            }
        }
    }

    Ok(input)
}

/// OpenAI Responses 响应 → Anthropic 响应
pub fn responses_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    let output = body
        .get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| ProxyError::TransformError("No output in response".to_string()))?;

    let mut content = Vec::new();

    let mut has_tool_use = false;
    for item in output {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match item_type {
            "message" => {
                if let Some(msg_content) = item.get("content").and_then(|c| c.as_array()) {
                    for block in msg_content {
                        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if block_type == "output_text" {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    content.push(json!({"type": "text", "text": text}));
                                }
                            }
                        } else if block_type == "refusal" {
                            if let Some(refusal) = block.get("refusal").and_then(|t| t.as_str()) {
                                if !refusal.is_empty() {
                                    content.push(json!({"type": "text", "text": refusal}));
                                }
                            }
                        }
                    }
                }
            }

            "function_call" => {
                let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args_str = item
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

                content.push(json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": input
                }));
                has_tool_use = true;
            }

            "reasoning" => {
                // 映射 reasoning summary → thinking block
                if let Some(summary) = item.get("summary").and_then(|s| s.as_array()) {
                    let thinking_text: String = summary
                        .iter()
                        .filter_map(|s| {
                            if s.get("type").and_then(|t| t.as_str()) == Some("summary_text") {
                                s.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    if !thinking_text.is_empty() {
                        content.push(json!({
                            "type": "thinking",
                            "thinking": thinking_text
                        }));
                    }
                }
            }

            _ => {}
        }
    }

    // status → stop_reason
    let stop_reason = map_responses_stop_reason(
        body.get("status").and_then(|s| s.as_str()),
        has_tool_use,
        body.pointer("/incomplete_details/reason")
            .and_then(|r| r.as_str()),
    );

    let usage_json = build_anthropic_usage_from_responses(body.get("usage"));

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
/// Validate and fix the Chat Completions message sequence for DeepSeek compliance.
///
/// DeepSeek strictly requires every assistant message with tool_calls to be
/// immediately followed by tool messages matching each tool_call_id — no other
/// message types may appear between the assistant and its tool results.
///
/// This function:
/// - Builds an index of all tool messages by tool_call_id.
/// - For each assistant with tool_calls, collects matching tool messages and
///   places them right after the assistant.
/// - Strips orphan tool_calls that have no matching tool message anywhere.
/// - Drops orphan tool messages that have no matching assistant tool_call.
fn reorder_tool_messages(messages: Vec<Value>) -> Vec<Value> {
    use std::collections::{HashMap, HashSet};

    // Phase 0: Build index of tool messages by tool_call_id.
    // Multi-map because a single tool_call_id may appear multiple times
    // (e.g. across different conversation turns).
    let mut tool_by_call_id: HashMap<String, Vec<Value>> = HashMap::new();
    let mut tool_indices: Vec<usize> = Vec::new(); // positions of tool messages

    for (idx, msg) in messages.iter().enumerate() {
        if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
            if let Some(call_id) = msg.get("tool_call_id").and_then(|i| i.as_str()) {
                tool_by_call_id
                    .entry(call_id.to_string())
                    .or_default()
                    .push(msg.clone());
            }
            tool_indices.push(idx);
        }
    }

    // Phase 1: Identify which tool_call_ids exist (have at least one tool message).
    let existing_tool_ids: HashSet<&str> = tool_by_call_id.keys().map(|s| s.as_str()).collect();

    // Phase 2: For each assistant, collect all tool_calls that have matching
    // tool messages, and record their positions.
    struct AssistantInfo {
        idx: usize,
        tool_call_ids: Vec<String>, // only those with matching tool messages
    }
    let mut assistant_infos: Vec<AssistantInfo> = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        if msg.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                let matching_ids: Vec<String> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        tc.get("id")
                            .and_then(|id| id.as_str())
                            .map(|id| id.to_string())
                    })
                    .filter(|id| existing_tool_ids.contains(&**id))
                    .collect();

                if !matching_ids.is_empty() {
                    assistant_infos.push(AssistantInfo {
                        idx,
                        tool_call_ids: matching_ids,
                    });
                }
            }
        }
    }

    // Phase 3: Build output — copy messages, inserting tool messages after
    // each assistant. Track which tool messages have been consumed.
    let tool_indices_set: HashSet<usize> = tool_indices.into_iter().collect();
    let mut consumed_tool_positions: HashSet<usize> = HashSet::new();
    let mut result: Vec<Value> = Vec::new();

    let mut assistant_iter = assistant_infos.iter().peekable();
    let mut pending_orphan_tools: usize = 0;

    for (idx, msg) in messages.iter().enumerate() {
        let is_tool = tool_indices_set.contains(&idx);

        if is_tool {
            // Tool messages are handled when their matching assistant is processed
            if !consumed_tool_positions.contains(&idx) {
                consumed_tool_positions.insert(idx);
                pending_orphan_tools += 1;
            }
            continue;
        }

        // Check if this is an assistant that needs tool messages inserted
        if let Some(info) = assistant_iter.next_if(|a| a.idx == idx) {
            // Clone and strip orphan tool_calls (no matching tool message)
            let mut msg = msg.clone();
            if let Some(tool_calls) = msg.get_mut("tool_calls").and_then(|t| t.as_array_mut()) {
                let before = tool_calls.len();
                tool_calls.retain(|tc| {
                    tc.get("id")
                        .and_then(|id| id.as_str())
                        .map(|id| existing_tool_ids.contains(id))
                        .unwrap_or(false)
                });
                let removed = before - tool_calls.len();
                if removed > 0 {
                    log::warn!(
                        "[Codex] 移除了 {} 个孤儿 tool_calls（无对应 tool 消息）",
                        removed
                    );
                }
                if tool_calls.is_empty() {
                    let content_is_null = msg.get("content").map_or(false, |c| c.is_null());
                if let Some(obj) = msg.as_object_mut() {
                    obj.remove("tool_calls");
                    if content_is_null {
                        obj.insert("content".to_string(), json!(""));
                    }
                }
                }
            }
            result.push(msg);

            // Insert matching tool messages immediately after
            for call_id in &info.tool_call_ids {
                if let Some(tool_msgs) = tool_by_call_id.get(call_id) {
                    for t in tool_msgs {
                        result.push(t.clone());
                    }
                }
            }
        } else {
            // Regular message (user, system, assistant without tool_calls, etc.)
            result.push(msg.clone());
        }
    }

    if pending_orphan_tools > 0 {
        log::warn!(
            "[Codex] 丢弃 {} 个孤儿 tool 消息（无前置 assistant 的 tool_calls 匹配）",
            pending_orphan_tools
        );
    }

    if result.len() != messages.len() {
        log::info!(
            "[Codex] Tool 消息清理: {} → {} 条消息",
            messages.len(),
            result.len()
        );
    }

    result
}/// Recursively strip all image content blocks from a content value,
/// replacing them with "[Image]" text placeholders. This is a safety net
/// for text-only models (DeepSeek, etc.) that reject image_url blocks.
fn sanitize_image_content(content: &Value) -> Value {
    match content {
        Value::Array(arr) => {
            let sanitized: Vec<Value> = arr
                .iter()
                .map(|item| {
                    if let Some(bt) = item.get("type").and_then(|t| t.as_str()) {
                        match bt {
                            // Chat Completions image format
                            "image_url" => {
                                json!({"type": "text", "text": "[Image]"})
                            }
                            // Responses API image format (should not appear here,
                            // but handled defensively)
                            "input_image" | "output_image" => {
                                json!({"type": "text", "text": "[Image]"})
                            }
                            // Pass through non-image blocks unchanged
                            _ => item.clone(),
                        }
                    } else {
                        item.clone()
                    }
                })
                .collect();
            Value::Array(sanitized)
        }
        // String content is always safe
        _ => content.clone(),
    }
}



/// OpenAI Responses 请求 → Chat Completions 请求
///
/// 将 Codex CLI 发出的 Responses API 格式转换为 Chat Completions 格式，
/// 用于连接不支持 Responses API 的上游（如 DeepSeek）。
///
/// 转换规则：
/// - `instructions` → system message（插入 messages 数组首位）
/// - `input[]` → `messages[]`
///   - role=user + input_text → user message
///   - role=assistant + output_text → assistant message
///   - function_call item → assistant message with tool_calls
///   - function_call_output item → tool message
/// - `max_output_tokens` → `max_tokens`
/// - `reasoning.effort` → `reasoning_effort` (DeepSeek 兼容)
/// - `tools` → 透传（Responses API 的 function tool 格式与 Chat Completions 兼容）
pub fn responses_to_chat_completions(body: Value) -> Result<Value, ProxyError> {
    // If the request is already in Chat Completions format (has "messages" not "input"),
    // Codex may have sent it directly for providers it knows support Chat Completions.
    // In that case, pass through without double-transforming.
    if body.get("messages").is_some() && body.get("input").is_none() {
        log::info!("[Codex] 请求已是 Chat Completions 格式，跳过转换，执行图片安全检查");
        let mut body = body;
        // Safety net: strip image content for text-only models (DeepSeek)
        if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for msg in messages.iter_mut() {
                if let Some(msg_content) = msg.get("content").cloned() {
                    msg["content"] = sanitize_image_content(&msg_content);
                }
            }
        }
        return Ok(body);
    }

    let mut result = json!({});

    // model
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // instructions → system message
    let mut messages: Vec<Value> = Vec::new();
    if let Some(instructions) = body.get("instructions").and_then(|i| i.as_str()) {
        if !instructions.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }
    }

    // input[] → messages[]
    // Accumulate reasoning text so it can be injected as `reasoning_content`
    // in the next assistant message. DeepSeek requires this for multi-turn
    // thinking mode — reasoning_content must be passed back to the API.
    let mut pending_reasoning: Option<String> = None;

    if let Some(input) = body.get("input").and_then(|i| i.as_array()) {
        for item in input {
            let item_type = item.get("type").and_then(|t| t.as_str());

            match item_type {
                // Reasoning items accumulate text for the next assistant message.
                Some("reasoning") => {
                    if let Some(summary) = item.get("summary").and_then(|s| s.as_array()) {
                        let text: String = summary
                            .iter()
                            .filter_map(|s| {
                                if s.get("type").and_then(|t| t.as_str()) == Some("summary_text") {
                                    s.get("text").and_then(|t| t.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        if !text.is_empty() {
                            pending_reasoning = Some(text);
                        }
                    }
                }

                Some("message") => {
                    let raw_role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    // Map Responses API roles to Chat Completions roles.
                    // DeepSeek does not support "developer" — convert to "system".
                    let role = match raw_role {
                        "developer" => "system",
                        other => other,
                    };
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        let mut text_parts: Vec<String> = Vec::new();
                                                for block in content {
                            let bt = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match bt {
                                "input_text" | "output_text" => {
                                    if let Some(text) =
                                        block.get("text").and_then(|t| t.as_str())
                                    {
                                        text_parts.push(text.to_string());
                                    }
                                }
                                "input_image" => {
                                    // DeepSeek and other text-only models don't support
                                    // multimodal input. Replace image with a text
                                    // placeholder so the request doesn't fail.
                                    if let Some(img_url) = block.get("image_url") {
                                        if let Some(url_str) = img_url.as_str() {
                                            if url_str.len() <= 120 {
                                                text_parts.push(format!("[Image: {}]", url_str));
                                            } else {
                                                // Truncate long data URIs to keep prompt size reasonable
                                                text_parts.push(format!(
                                                    "[Image: {}...]",
                                                    &url_str[..117]
                                                ));
                                            }
                                        } else {
                                            text_parts.push("[Image]".to_string());
                                        }
                                    } else {
                                        text_parts.push("[Image]".to_string());
                                    }
                                }
                                _ => {}
                            }
                        }
                        // Build content: always text-only (multimodal images
                        // are converted to [Image] placeholders above for
                        // text-only model compatibility)
                        if text_parts.len() <= 1 {
                            let text = text_parts.first().cloned().unwrap_or_default();
                            messages.push(json!({
                                "role": role,
                                "content": text
                            }));
                        } else {
                            let parts: Vec<Value> = text_parts
                                .iter()
                                .map(|t| json!({"type": "text", "text": t}))
                                .collect();
                            messages.push(json!({
                                "role": role,
                                "content": parts
                            }));
                        }
                    } else if let Some(text) = item.get("content").and_then(|c| c.as_str()) {
                        messages.push(json!({
                            "role": role,
                            "content": text
                        }));
                    } else {
                        messages.push(json!({"role": role, "content": ""}));
                    }

                    // Inject accumulated reasoning_content into assistant messages.
                    // DeepSeek requires reasoning_content to be passed back on subsequent turns.
                    if role == "assistant" {
                        if let Some(reasoning) = pending_reasoning.take() {
                            if let Some(last) = messages.last_mut() {
                                last["reasoning_content"] = json!(reasoning);
                            }
                        }
                    }
                }

                Some("function_call") => {
                    let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let arguments =
                        item.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                    // Validate arguments is parseable JSON; if not, use as-is
                    let args_str = if serde_json::from_str::<Value>(arguments).is_ok() {
                        arguments.to_string()
                    } else {
                        serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
                    };

                    let tc = json!({
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": args_str
                        }
                    });

                    // Merge into the previous assistant message if one exists.
                    // Responses API has function_call as a separate input item, but
                    // Chat Completions requires tool_calls to be in the SAME assistant message.
                    if let Some(last) = messages.last_mut() {
                        if last.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                            // Merge tool_calls into the existing assistant message
                            if let Some(existing_tcs) = last.get_mut("tool_calls") {
                                if let Some(arr) = existing_tcs.as_array_mut() {
                                    arr.push(tc);
                                }
                            } else {
                                last["tool_calls"] = json!([tc]);
                                // If content was a string, keep it; null means no text was sent
                                if last.get("content").is_none() {
                                    last["content"] = json!(null);
                                }
                            }
                            continue;
                        }
                    }
                    // No prior assistant message — create a new one
                    let mut new_msg = json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [tc]
                    });
                    if let Some(reasoning) = pending_reasoning.take() {
                        new_msg["reasoning_content"] = json!(reasoning);
                    }
                    messages.push(new_msg);
                }

                Some("function_call_output") => {
                    let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                    // Output may be a string, object, or array — serialize non-string outputs
                    let output = match item.get("output") {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => serde_json::to_string(v).unwrap_or_default(),
                        None => String::new(),
                    };
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": output
                    }));
                }

                _ => {
                    // Unknown item type — determine proper role mapping.
                    // Items with a call_id are tool outputs (e.g. web_search_call_output,
                    // computer_call_output, etc.) and must be mapped to tool messages
                    // so the Chat Completions tool_calls → tool sequence stays intact.
                    if let Some(call_id) = item.get("call_id").and_then(|c| c.as_str()) {
                        let output = item.get("output").and_then(|o| o.as_str()).unwrap_or("");
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": output
                        }));
                    } else if let Some(role) = item.get("role").and_then(|r| r.as_str()) {
                        let raw_content = item.get("content").cloned().unwrap_or(json!(""));
                        let safe_content = sanitize_image_content(&raw_content);
                        messages.push(json!({
                            "role": role,
                            "content": safe_content
                        }));
                    }
                }
            }
        }
    }
    // Final safety net: strip any image content from all messages.
    // Text-only models (DeepSeek, etc.) reject image_url / input_image blocks.
    for msg in &mut messages {
        if let Some(msg_content) = msg.get("content").cloned() {
            msg["content"] = sanitize_image_content(&msg_content);
        }
    }

    // Reorder tool messages to satisfy DeepSeek's strict validation:
    // Every assistant message with tool_calls must be immediately followed
    // by ALL matching tool messages (one per tool_call_id), with no
    // non-tool messages in between.
    messages = reorder_tool_messages(messages);

    result["messages"] = json!(messages);

    // max_output_tokens → max_tokens
    if let Some(v) = body.get("max_output_tokens") {
        result["max_tokens"] = v.clone();
    }

    // temperature
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }

    // top_p
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }

    // stream — force false for now (SSE streaming conversion not yet implemented)
    // When stream is false, DeepSeek returns a single JSON response that we can transform
    result["stream"] = json!(false);

    // reasoning.effort → reasoning_effort (DeepSeek)
    if let Some(effort) = body.pointer("/reasoning/effort").and_then(|e| e.as_str()) {
        result["reasoning_effort"] = json!(effort);
    }

    // tools — Responses format → Chat Completions format
    // Responses: {"type":"function","name":"...","parameters":{...}}
    // Chat Completions: {"type":"function","function":{"name":"...","parameters":{...}}}
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let cc_tools: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
                // Skip tools with empty names (Chat Completions requirement)
                if name.is_empty() {
                    return None;
                }
                let desc_val = t.get("description").cloned().unwrap_or(json!(null));
                // Ensure parameters is a valid JSON Schema object (Chat Completions requirement)
                let params = match t.get("parameters") {
                    Some(p) if p.is_object() && !p.as_object().map(|o| o.is_empty()).unwrap_or(true) => p.clone(),
                    _ => json!({"type": "object", "properties": {}}),
                };
                let mut function_obj = json!({
                    "name": name,
                    "parameters": params,
                });
                // Only include description if it is a non-empty string
                if let Some(d) = desc_val.as_str() {
                    if !d.is_empty() {
                        function_obj["description"] = json!(d);
                    }
                }
                if let Some(req) = t.get("required") {
                    if !req.is_null() {
                        function_obj["required"] = req.clone();
                    }
                }
                Some(json!({
                    "type": "function",
                    "function": function_obj
                }))
            })
            .collect();
        result["tools"] = json!(cc_tools);
    }

    // tool_choice
    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = v.clone();
    }

    Ok(result)
}

/// Chat Completions 响应 → OpenAI Responses 响应
///
/// 将上游 Chat Completions API 的响应转换回 Responses API 格式，
/// 使 Codex CLI 可以正常解析。
pub fn chat_completions_to_responses(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({
        "object": "response"
    });

    // id
    if let Some(id) = body.get("id").and_then(|i| i.as_str()) {
        result["id"] = json!(id);
    }

    // model
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // status — derive from finish_reason
    let mut status = "completed";
    let mut output: Vec<Value> = Vec::new();

    if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(msg) = choice.get("message") {
                let mut content: Vec<Value> = Vec::new();
                let mut tool_calls_out: Vec<Value> = Vec::new();

                // text content
                if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                    if !text.is_empty() {
                        content.push(json!({
                            "type": "output_text",
                            "text": text
                        }));
                    }
                }

                // reasoning_content (DeepSeek R1)
                if let Some(reasoning) = msg.get("reasoning_content").and_then(|r| r.as_str()) {
                    if !reasoning.is_empty() {
                        output.push(json!({
                            "type": "reasoning",
                            "summary": [{"type": "summary_text", "text": reasoning}]
                        }));
                    }
                }

                // tool_calls → function_call items
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tool_calls {
                        let tc_id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
                        if let Some(func) = tc.get("function") {
                            let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let args =
                                func.get("arguments").and_then(|a| a.as_str()).unwrap_or("{}");
                            tool_calls_out.push(json!({
                                "type": "function_call",
                                "call_id": tc_id,
                                "name": name,
                                "arguments": args
                            }));
                        }
                    }
                }

                // Build message output item
                if !content.is_empty() {
                    output.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": content
                    }));
                }

                // Append function_call items after the message
                output.extend(tool_calls_out);
            }

            // finish_reason → status
            if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                match reason {
                    "tool_calls" => status = "completed", // will be mapped via has_tool_use
                    "length" => status = "incomplete",
                    "content_filter" => status = "incomplete",
                    "stop" => status = "completed",
                    _ => {}
                }
            }
        }
    }

    // Detect if we have tool calls for proper status
    let has_tool_use = output.iter().any(|o| {
        o.get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "function_call")
            .unwrap_or(false)
    });

    // Override status when tool calls present
    if has_tool_use && status == "completed" {
        // status stays "completed" — Codex uses stop_reason for tool_use
    }

    result["status"] = json!(status);
    result["output"] = json!(output);

    // usage: Chat Completions → Responses
    if let Some(usage) = body.get("usage") {
        let input_tokens = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .cloned()
            .unwrap_or(json!(0));
        let output_tokens = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .cloned()
            .unwrap_or(json!(0));
        let total_tokens = usage.get("total_tokens").cloned().unwrap_or_else(|| {
            json!(input_tokens.as_u64().unwrap_or(0) + output_tokens.as_u64().unwrap_or(0))
        });

        result["usage"] = json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_to_responses_simple() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["model"], "gpt-4o");
        assert_eq!(result["max_output_tokens"], 1024);
        assert_eq!(result["input"][0]["role"], "user");
        assert_eq!(result["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(result["input"][0]["content"][0]["text"], "Hello");
        // stop_sequences should not appear
        assert!(result.get("stop_sequences").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_with_system_string() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["instructions"], "You are a helpful assistant.");
        // system should not appear in input
        assert_eq!(result["input"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_anthropic_to_responses_strips_leading_billing_header_from_system_string() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nYou are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["instructions"], "You are a helpful assistant.");
    }

    #[test]
    fn test_anthropic_to_responses_strips_billing_header_with_crlf() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\r\n\r\nYou are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["instructions"], "You are a helpful assistant.");
    }

    #[test]
    fn test_anthropic_to_responses_keeps_non_leading_billing_header_text() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": "Keep this literal:\nx-anthropic-billing-header: example",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(
            result["instructions"],
            "Keep this literal:\nx-anthropic-billing-header: example"
        );
    }

    #[test]
    fn test_anthropic_to_responses_with_system_array() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "Part 1"},
                {"type": "text", "text": "Part 2"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["instructions"], "Part 1\n\nPart 2");
    }

    #[test]
    fn test_anthropic_to_responses_strips_billing_header_from_system_array_parts() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n"},
                {"type": "text", "text": "Stable prompt"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["instructions"], "Stable prompt");
    }

    #[test]
    fn test_anthropic_to_responses_preserves_prompt_after_billing_header_in_same_part() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nStable prompt part 1"},
                {"type": "text", "text": "Stable prompt part 2"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(
            result["instructions"],
            "Stable prompt part 1\n\nStable prompt part 2"
        );
    }

    #[test]
    fn test_anthropic_to_responses_with_tools() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["name"], "get_weather");
        assert!(result["tools"][0].get("parameters").is_some());
        // input_schema should not appear
        assert!(result["tools"][0].get("input_schema").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_tool_choice_any_to_required() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tool_choice": {"type": "any"}
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["tool_choice"], "required");
    }

    #[test]
    fn test_anthropic_to_responses_tool_choice_tool_to_function() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tool_choice": {"type": "tool", "name": "get_weather"}
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["tool_choice"]["type"], "function");
        assert_eq!(result["tool_choice"]["name"], "get_weather");
    }

    #[test]
    fn test_anthropic_to_responses_tool_use_lifting() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me check"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // Should produce: assistant message (text) + function_call item
        assert_eq!(input_arr.len(), 2);

        // First: assistant message with output_text
        assert_eq!(input_arr[0]["role"], "assistant");
        assert_eq!(input_arr[0]["content"][0]["type"], "output_text");
        assert_eq!(input_arr[0]["content"][0]["text"], "Let me check");

        // Second: function_call item (lifted from message)
        assert_eq!(input_arr[1]["type"], "function_call");
        assert_eq!(input_arr[1]["call_id"], "call_123");
        assert_eq!(input_arr[1]["name"], "get_weather");
    }

    #[test]
    fn test_anthropic_to_responses_tool_result_lifting() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_123", "content": "Sunny, 25°C"}
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // Should produce: function_call_output item (lifted)
        assert_eq!(input_arr.len(), 1);
        assert_eq!(input_arr[0]["type"], "function_call_output");
        assert_eq!(input_arr[0]["call_id"], "call_123");
        assert_eq!(input_arr[0]["output"], "Sunny, 25°C");
    }

    #[test]
    fn test_anthropic_to_responses_thinking_discarded() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "Let me think..."},
                    {"type": "text", "text": "The answer is 42"}
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // thinking should be discarded, only text remains
        assert_eq!(input_arr.len(), 1);
        assert_eq!(input_arr[0]["content"][0]["type"], "output_text");
        assert_eq!(input_arr[0]["content"][0]["text"], "The answer is 42");
    }

    #[test]
    fn test_anthropic_to_responses_image() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let content = result["input"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "data:image/png;base64,abc123");
    }

    #[test]
    fn test_responses_to_anthropic_simple() {
        let input = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "id": "msg_123",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["id"], "resp_123");
        assert_eq!(result["type"], "message");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello!");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_responses_to_anthropic_with_function_call() {
        let input = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "function_call",
                "id": "fc_123",
                "call_id": "call_123",
                "name": "get_weather",
                "arguments": "{\"location\": \"Tokyo\"}",
                "status": "completed"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 15}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_123");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_responses_to_anthropic_with_refusal_block() {
        let input = json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "refusal", "refusal": "I can't help with that."}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "I can't help with that.");
        assert_eq!(result["stop_reason"], "end_turn");
    }

    #[test]
    fn test_responses_to_anthropic_with_reasoning() {
        let input = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [
                {
                    "type": "reasoning",
                    "id": "rs_123",
                    "summary": [
                        {"type": "summary_text", "text": "Thinking about the problem..."}
                    ]
                },
                {
                    "type": "message",
                    "id": "msg_123",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "The answer is 42"}]
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = responses_to_anthropic(input).unwrap();
        // Should have thinking + text
        assert_eq!(result["content"][0]["type"], "thinking");
        assert_eq!(
            result["content"][0]["thinking"],
            "Thinking about the problem..."
        );
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "The answer is 42");
    }

    #[test]
    fn test_responses_to_anthropic_incomplete_status() {
        let input = json!({
            "id": "resp_123",
            "status": "incomplete",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Partial..."}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 4096}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "max_tokens");
    }

    #[test]
    fn test_responses_to_anthropic_incomplete_non_token_reason() {
        let input = json!({
            "id": "resp_123",
            "status": "incomplete",
            "incomplete_details": {"reason": "content_filter"},
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Blocked"}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 1}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "end_turn");
    }

    #[test]
    fn test_model_passthrough() {
        let input = json!({
            "model": "o3-mini",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["model"], "o3-mini");
    }

    #[test]
    fn test_anthropic_to_responses_with_cache_key() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, Some("my-provider-id"), false, false).unwrap();
        assert_eq!(result["prompt_cache_key"], "my-provider-id");
    }

    #[test]
    fn test_anthropic_to_responses_strip_cache_control_on_tools() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert!(result["tools"][0].get("cache_control").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_strip_cache_control_on_text() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert!(result["input"][0]["content"][0]
            .get("cache_control")
            .is_none());
    }

    #[test]
    fn test_responses_to_anthropic_with_cache_tokens() {
        let input = json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "input_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["usage"]["input_tokens"], 100);
        assert_eq!(result["usage"]["output_tokens"], 50);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 80);
    }

    #[test]
    fn test_responses_to_anthropic_with_direct_cache_fields() {
        let input = json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 20
            }
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 20);
    }

    #[test]
    fn test_anthropic_to_responses_o_series_uses_max_output_tokens() {
        // Responses API always uses max_output_tokens, even for o-series models
        let input = json!({
            "model": "o3-mini",
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["max_output_tokens"], 4096);
        assert!(result.get("max_completion_tokens").is_none());
    }

    #[test]
    fn test_responses_output_config_max_sets_reasoning_xhigh() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "output_config": {"effort": "max"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "xhigh");
    }

    #[test]
    fn test_responses_output_config_takes_priority_over_thinking() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "output_config": {"effort": "low"},
            "thinking": {"type": "adaptive"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "low");
    }

    #[test]
    fn test_responses_thinking_enabled_small_budget_sets_reasoning_low() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "low");
    }

    #[test]
    fn test_responses_thinking_enabled_medium_budget_sets_reasoning_medium() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 8000},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "medium");
    }

    #[test]
    fn test_responses_thinking_enabled_large_budget_sets_reasoning_high() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 32000},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "high");
    }

    #[test]
    fn test_responses_thinking_adaptive_sets_reasoning_xhigh() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "thinking": {"type": "adaptive"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["reasoning"]["effort"], "xhigh");
    }

    #[test]
    fn test_responses_non_reasoning_model_no_reasoning() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert!(result.get("reasoning").is_none());
    }

    // ==================== Codex OAuth (ChatGPT 反代) 协议约束 ====================

    #[test]
    fn test_anthropic_to_responses_codex_oauth_sets_store_and_include() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        // store 必须显式为 false（ChatGPT 后端拒绝 true）
        assert_eq!(result["store"], json!(false));
        assert_eq!(result["service_tier"], json!("priority"));

        // include 必须包含 reasoning.encrypted_content（无服务端状态下保持多轮 reasoning）
        assert_eq!(result["include"], json!(["reasoning.encrypted_content"]));
    }

    #[test]
    fn test_anthropic_to_responses_non_codex_omits_store_and_include() {
        // 回归护栏：is_codex_oauth=false 时，行为必须与今日字节级一致
        // —— 不写 store、不写 include，OpenRouter / Azure / OpenAI 付费 API 路径不受影响
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();

        assert!(result.get("store").is_none());
        assert!(result.get("service_tier").is_none());
        assert!(result.get("include").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_codex_oauth_preserves_existing_include() {
        // 客户端预置了 include：union 保留原有项 + 添加 marker，不重复
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}],
            "include": ["something.else", "reasoning.encrypted_content"]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();
        let includes = result["include"]
            .as_array()
            .expect("include should be array");

        // 原有项必须保留
        assert!(includes
            .iter()
            .any(|v| v.as_str() == Some("something.else")));
        // marker 必须存在
        assert!(includes
            .iter()
            .any(|v| v.as_str() == Some("reasoning.encrypted_content")));
        // 不重复：marker 只出现一次
        let marker_count = includes
            .iter()
            .filter(|v| v.as_str() == Some("reasoning.encrypted_content"))
            .count();
        assert_eq!(marker_count, 1, "marker 不应被重复添加（idempotent 失败）");
    }

    #[test]
    fn test_anthropic_to_responses_codex_oauth_fast_mode_can_be_disabled() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, false).unwrap();

        assert_eq!(result["store"], json!(false));
        assert!(result.get("service_tier").is_none());
        assert_eq!(result["include"], json!(["reasoning.encrypted_content"]));
    }

    #[test]
    fn test_anthropic_to_responses_codex_oauth_strips_max_output_tokens() {
        // ChatGPT Plus/Pro 反代不接受 max_output_tokens（OpenAI 官方 codex-rs 的
        // ResponsesApiRequest 结构体里也没有这个字段），必须删除，否则服务端 400：
        // "Unsupported parameter: max_output_tokens"
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        assert!(
            result.get("max_output_tokens").is_none(),
            "Codex OAuth 路径必须删除 max_output_tokens"
        );
    }

    #[test]
    fn test_anthropic_to_responses_non_codex_keeps_max_output_tokens() {
        // 回归护栏：非 Codex OAuth 路径必须保留 max_output_tokens
        // —— OpenAI 付费 Responses API / Azure 等仍然依赖这个字段
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();

        assert_eq!(result["max_output_tokens"], json!(1024));
    }

    // ==================== 第二轮：P0 + P1 字段对齐 ====================

    #[test]
    fn test_codex_oauth_strips_temperature() {
        // P0: ChatGPT 反代不接受 temperature
        // 依据：OpenAI 官方 codex-rs 的 ResponsesApiRequest 结构体根本没有这个字段
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "temperature": 0.7,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        assert!(
            result.get("temperature").is_none(),
            "Codex OAuth 路径必须删除 temperature"
        );
    }

    #[test]
    fn test_codex_oauth_strips_top_p() {
        // P0: ChatGPT 反代不接受 top_p
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "top_p": 0.9,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        assert!(
            result.get("top_p").is_none(),
            "Codex OAuth 路径必须删除 top_p"
        );
    }

    #[test]
    fn test_codex_oauth_defaults_required_fields_when_absent() {
        // P1: 极简输入（无 system / 无 tools / 无 stream），断言四个必填字段都被注入默认值
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        assert_eq!(
            result["instructions"],
            json!(""),
            "instructions 缺失时应兜底为空字符串"
        );
        assert_eq!(result["tools"], json!([]), "tools 缺失时应兜底为空数组");
        assert_eq!(
            result["parallel_tool_calls"],
            json!(false),
            "parallel_tool_calls 应兜底为 false"
        );
        assert_eq!(result["stream"], json!(true), "stream 应被强制设为 true");
    }

    #[test]
    fn test_codex_oauth_preserves_existing_instructions_and_tools() {
        // P1: 客户端送了 system 和 tools，应保留原值，不被默认值覆盖
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "system": "You are a helpful assistant",
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}}
                }
            }],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        assert_eq!(
            result["instructions"],
            json!("You are a helpful assistant"),
            "client 已送的 instructions 必须保留"
        );

        let tools = result["tools"].as_array().expect("tools 应为数组");
        assert_eq!(tools.len(), 1, "client 已送的 tools 必须保留");
        assert_eq!(tools[0]["name"], json!("get_weather"));
    }

    #[test]
    fn test_codex_oauth_forces_stream_true_even_when_client_sends_false() {
        // 即使客户端误传 stream:false，也要强制覆盖为 true
        // 依据：cc-switch SSE 解析层只支持流式响应
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "stream": false,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true, true).unwrap();

        assert_eq!(
            result["stream"],
            json!(true),
            "Codex OAuth 路径下 stream 必须强制为 true"
        );
    }

    #[test]
    fn test_non_codex_keeps_temperature_and_top_p() {
        // 回归护栏：非 Codex OAuth 路径必须保留 temperature/top_p
        // —— 防止 P0 删除逻辑误扩散到 OpenRouter / Azure / 付费 OpenAI 路径
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "temperature": 0.7,
            "top_p": 0.9,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();

        assert_eq!(result["temperature"], json!(0.7));
        assert_eq!(result["top_p"], json!(0.9));
    }

    #[test]
    fn test_non_codex_does_not_inject_default_required_fields() {
        // 回归护栏：非 Codex OAuth 路径不应被 P1 默认值污染
        // —— OpenRouter / Azure / 付费 OpenAI 等保持原有"条件写入"语义
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();

        assert!(
            result.get("parallel_tool_calls").is_none(),
            "非 Codex OAuth 路径不应注入 parallel_tool_calls"
        );
        assert!(
            result.get("stream").is_none(),
            "非 Codex OAuth 路径不应注入 stream"
        );
        // instructions 和 tools 因为客户端没送，所以不应出现
        assert!(
            result.get("instructions").is_none(),
            "非 Codex OAuth 路径下 instructions 在客户端未送时不应被注入"
        );
        assert!(
            result.get("tools").is_none(),
            "非 Codex OAuth 路径下 tools 在客户端未送时不应被注入"
        );
    }

    // ==================== Usage Field Robustness Tests ====================

    #[test]
    fn test_build_usage_from_null_parameter() {
        let result = build_anthropic_usage_from_responses(None);
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_null_json_value() {
        let result = build_anthropic_usage_from_responses(Some(&json!(null)));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_empty_object() {
        let result = build_anthropic_usage_from_responses(Some(&json!({})));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_partial_input_only() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100
        })));
        assert_eq!(result["input_tokens"], json!(100));
        assert_eq!(result["output_tokens"], json!(0));
    }

    #[test]
    fn test_build_usage_from_partial_output_only() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "output_tokens": 50
        })));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(50));
    }

    #[test]
    fn test_build_usage_with_openai_field_names() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "prompt_tokens": 120,
            "completion_tokens": 45
        })));
        assert_eq!(result["input_tokens"], json!(120));
        assert_eq!(result["output_tokens"], json!(45));
    }

    #[test]
    fn test_build_usage_anthropic_names_precedence() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "prompt_tokens": 120,
            "output_tokens": 50,
            "completion_tokens": 45
        })));
        assert_eq!(result["input_tokens"], json!(100)); // Anthropic name takes precedence
        assert_eq!(result["output_tokens"], json!(50)); // Anthropic name takes precedence
    }

    #[test]
    fn test_build_usage_cache_tokens_from_nested_details() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "input_tokens_details": {
                "cached_tokens": 80
            }
        })));
        assert_eq!(result["input_tokens"], json!(100));
        assert_eq!(result["output_tokens"], json!(50));
        assert_eq!(result["cache_read_input_tokens"], json!(80));
    }

    #[test]
    fn test_build_usage_cache_tokens_direct_override() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 50,
            "input_tokens_details": {
                "cached_tokens": 80
            },
            "cache_read_input_tokens": 100
        })));
        assert_eq!(result["cache_read_input_tokens"], json!(100)); // Direct field overrides nested
    }

    #[test]
    fn test_build_usage_cache_tokens_without_input_output() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "cache_read_input_tokens": 60,
            "cache_creation_input_tokens": 20
        })));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["output_tokens"], json!(0));
        assert_eq!(result["cache_read_input_tokens"], json!(60));
        assert_eq!(result["cache_creation_input_tokens"], json!(20));
    }
}
