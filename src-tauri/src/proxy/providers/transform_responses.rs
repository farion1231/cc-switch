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
use std::collections::{HashMap, HashSet};

fn is_anthropic_web_search_tool(tool: &Value) -> bool {
    matches!(
        (
            tool.get("name").and_then(Value::as_str),
            tool.get("type").and_then(Value::as_str),
        ),
        (Some("web_search"), Some(tool_type)) if tool_type.starts_with("web_search_")
    )
}

fn ensure_include(result: &mut Value, item: &str) {
    if let Some(obj) = result.as_object_mut() {
        let entry = obj
            .entry("include".to_string())
            .or_insert_with(|| json!([]));
        if !entry.is_array() {
            *entry = json!([]);
        }

        if let Some(includes) = entry.as_array_mut() {
            if !includes.iter().any(|v| v.as_str() == Some(item)) {
                includes.push(json!(item));
            }
        }
    }
}

fn normalized_non_empty_string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::trim))
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn map_anthropic_tool_to_responses(
    tool: &Value,
    allow_unsupported_max_uses: bool,
) -> Result<Option<Value>, ProxyError> {
    if tool.get("type").and_then(Value::as_str) == Some("BatchTool") {
        return Ok(None);
    }

    if is_anthropic_web_search_tool(tool) {
        let allowed_domains = normalized_non_empty_string_array(tool.get("allowed_domains"));
        let blocked_domains = normalized_non_empty_string_array(tool.get("blocked_domains"));

        if !allowed_domains.is_empty() && !blocked_domains.is_empty() {
            return Err(ProxyError::TransformError(
                "Cannot specify both allowed_domains and blocked_domains in the same request"
                    .to_string(),
            ));
        }

        if !blocked_domains.is_empty() {
            return Err(ProxyError::TransformError(
                "Anthropic blocked_domains has no direct OpenAI web_search equivalent".to_string(),
            ));
        }

        if tool.get("max_uses").is_some() {
            if allow_unsupported_max_uses {
                log::warn!(
                    "Anthropic web_search max_uses has no direct Codex OAuth Responses equivalent; ignoring it"
                );
            } else {
                return Err(ProxyError::TransformError(
                    "Anthropic web_search max_uses has no direct OpenAI web_search equivalent"
                        .to_string(),
                ));
            }
        }

        let mut mapped = serde_json::Map::new();
        mapped.insert("type".to_string(), json!("web_search"));

        if !allowed_domains.is_empty() {
            mapped.insert(
                "filters".to_string(),
                json!({ "allowed_domains": allowed_domains }),
            );
        }

        if let Some(location) = tool.get("user_location").cloned() {
            mapped.insert("user_location".to_string(), location);
        }

        return Ok(Some(Value::Object(mapped)));
    }

    Ok(Some(json!({
        "type": "function",
        "name": tool.get("name").and_then(|n| n.as_str()).unwrap_or(""),
        "description": tool.get("description"),
        "parameters": super::transform::clean_schema(
            tool.get("input_schema").cloned().unwrap_or(json!({}))
        )
    })))
}

fn extract_web_search_sources_from_response_item(item: &Value) -> Vec<Value> {
    let sources = item
        .pointer("/action/sources")
        .and_then(Value::as_array)
        .or_else(|| item.get("results").and_then(Value::as_array));

    sources
        .into_iter()
        .flatten()
        .filter_map(|source| {
            let url = source.get("url").and_then(Value::as_str)?;
            let mut mapped = serde_json::Map::new();
            mapped.insert("type".to_string(), json!("web_search_result"));
            mapped.insert("url".to_string(), json!(url));
            mapped.insert(
                "title".to_string(),
                source.get("title").cloned().unwrap_or_else(|| json!("")),
            );
            Some(Value::Object(mapped))
        })
        .collect()
}

fn extract_web_search_sources_from_anthropic_content(content: &Value) -> Vec<Value> {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| {
            let url = block.get("url").and_then(Value::as_str)?;
            let mut mapped = serde_json::Map::new();
            mapped.insert("url".to_string(), json!(url));
            mapped.insert(
                "title".to_string(),
                block.get("title").cloned().unwrap_or_else(|| json!("")),
            );
            Some(Value::Object(mapped))
        })
        .collect()
}

fn extract_anthropic_web_search_error_code(content: &Value) -> Option<String> {
    match content {
        Value::Object(obj)
            if obj.get("type").and_then(Value::as_str) == Some("web_search_tool_result_error") =>
        {
            obj.get("error_code")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        }
        _ => None,
    }
}

fn normalized_web_search_action_fields(value: &Value) -> serde_json::Map<String, Value> {
    let mut action = serde_json::Map::new();
    let source = value
        .get("action")
        .filter(|action| action.is_object())
        .or_else(|| value.get("input").filter(|input| input.is_object()))
        .unwrap_or(value);

    if let Some(action_type) = source
        .get("type")
        .and_then(Value::as_str)
        .filter(|action_type| !action_type.is_empty())
    {
        action.insert("type".to_string(), json!(action_type));
    }

    if let Some(query) = source
        .get("query")
        .and_then(Value::as_str)
        .filter(|query| !query.is_empty())
    {
        action.insert("query".to_string(), json!(query));
    }

    if let Some(queries) = source
        .get("queries")
        .and_then(Value::as_array)
        .filter(|queries| {
            queries
                .iter()
                .any(|query| query.as_str().is_some_and(|query| !query.trim().is_empty()))
        })
    {
        action.insert("queries".to_string(), Value::Array(queries.clone()));
    }

    if let Some(url) = source
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
    {
        action.insert("url".to_string(), json!(url));
    }

    if let Some(pattern) = source
        .get("pattern")
        .and_then(Value::as_str)
        .filter(|pattern| !pattern.is_empty())
    {
        action.insert("pattern".to_string(), json!(pattern));
    }

    action
}

fn build_responses_web_search_action(action_source: Option<&Value>, sources: Vec<Value>) -> Value {
    let mut action = action_source
        .map(normalized_web_search_action_fields)
        .unwrap_or_default();

    if !sources.is_empty() {
        action.insert("sources".to_string(), Value::Array(sources));
    }

    Value::Object(action)
}

fn build_anthropic_web_search_input(item: &Value) -> Value {
    Value::Object(normalized_web_search_action_fields(item))
}

fn response_web_search_status(item: &Value) -> &str {
    item.get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
}

fn extract_response_web_search_error_code(item: &Value) -> Option<String> {
    item.pointer("/error/code")
        .and_then(Value::as_str)
        .or_else(|| item.pointer("/error/type").and_then(Value::as_str))
        .or_else(|| {
            item.pointer("/incomplete_details/reason")
                .and_then(Value::as_str)
        })
        .map(ToString::to_string)
}

fn build_anthropic_web_search_error_content(error_code: Option<&str>) -> Value {
    json!({
        "type": "web_search_tool_result_error",
        "error_code": error_code.unwrap_or("unavailable")
    })
}

/// Anthropic 请求 → OpenAI Responses 请求
///
/// `cache_key`: optional prompt_cache_key to inject for improved cache routing
/// `is_codex_oauth`: 当目标后端是 ChatGPT Plus/Pro 反代 (`chatgpt.com/backend-api/codex`) 时为 true。
/// 该后端强制要求 `store: false`，并要求 `include` 包含 `reasoning.encrypted_content`
/// 以便在无服务端状态下保持多轮 reasoning 上下文。
pub fn anthropic_to_responses(
    body: Value,
    cache_key: Option<&str>,
    is_codex_oauth: bool,
) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // system → instructions (Responses API 使用 instructions 字段)
    if let Some(system) = body.get("system") {
        let instructions = if let Some(text) = system.as_str() {
            text.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|msg| msg.get("text").and_then(|t| t.as_str()))
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
    if let Some(v) = body.get("include").filter(|v| v.is_array()) {
        result["include"] = v.clone();
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
    let mut web_search_tool_names: HashSet<String> = HashSet::new();
    let mut has_web_search = false;
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let mut response_tools = Vec::new();
        for tool in tools {
            if is_anthropic_web_search_tool(tool) {
                has_web_search = true;
                if let Some(name) = tool.get("name").and_then(Value::as_str) {
                    web_search_tool_names.insert(name.to_string());
                }
            }

            if let Some(mapped) = map_anthropic_tool_to_responses(tool, is_codex_oauth)? {
                response_tools.push(mapped);
            }
        }

        if !response_tools.is_empty() {
            result["tools"] = json!(response_tools);
        }
    }

    if has_web_search {
        ensure_include(&mut result, "web_search_call.action.sources");
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice_to_responses(v, &web_search_tool_names);
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
    // - stream: 必须永远 true（codex-rs 硬编码 true，且 cc-switch 的
    //   SSE 解析层只处理流式响应，强制覆盖避免客户端误传 false）
    if is_codex_oauth {
        result["store"] = json!(false);
        ensure_include(&mut result, "reasoning.encrypted_content");

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

fn map_tool_choice_to_responses(
    tool_choice: &Value,
    web_search_tool_names: &HashSet<String>,
) -> Value {
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
                if web_search_tool_names.contains(name) {
                    log::warn!(
                        "Anthropic forced web_search tool_choice has no direct Responses equivalent; falling back to auto"
                    );
                    return json!("auto");
                }
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
/// Priority order:
/// 1. OpenAI nested details (`input_tokens_details.cached_tokens`, `prompt_tokens_details.cached_tokens`) as initial value
/// 2. Direct Anthropic-style fields (`cache_read_input_tokens`, `cache_creation_input_tokens`) override if present
pub(crate) fn build_anthropic_usage_from_responses(usage: Option<&Value>) -> Value {
    let u = match usage {
        Some(v) if !v.is_null() => v,
        _ => {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0
            })
        }
    };

    let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    let mut result = json!({
        "input_tokens": input,
        "output_tokens": output
    });

    // Step 1: OpenAI nested details as fallback
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
                let mut pending_web_searches: HashMap<String, Value> = HashMap::new();

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

                        "server_tool_use" => {
                            if block.get("name").and_then(Value::as_str) == Some("web_search") {
                                let id = block
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string();
                                if !id.is_empty() {
                                    pending_web_searches.insert(id, block.clone());
                                }
                            }
                        }

                        "web_search_tool_result" => {
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            let call_id = block
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let pending = pending_web_searches.remove(&call_id);
                            let error_code = extract_anthropic_web_search_error_code(
                                block.get("content").unwrap_or(&Value::Null),
                            );
                            let status = if error_code.is_some() {
                                "failed"
                            } else {
                                "completed"
                            };
                            let sources = if error_code.is_some() {
                                Vec::new()
                            } else {
                                extract_web_search_sources_from_anthropic_content(
                                    block.get("content").unwrap_or(&Value::Null),
                                )
                            };

                            input.push(json!({
                                "type": "web_search_call",
                                "id": call_id,
                                "status": status,
                                "action": build_responses_web_search_action(pending.as_ref(), sources)
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

                for (call_id, pending) in pending_web_searches {
                    input.push(json!({
                        "type": "web_search_call",
                        "id": call_id,
                        "status": "in_progress",
                        "action": build_responses_web_search_action(Some(&pending), Vec::new())
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
    let mut web_search_requests = 0_u64;
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

            "web_search_call" => {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                let status = response_web_search_status(item);

                content.push(json!({
                    "type": "server_tool_use",
                    "id": id,
                    "name": "web_search",
                    "input": build_anthropic_web_search_input(item)
                }));

                match status {
                    "completed" => {
                        content.push(json!({
                            "type": "web_search_tool_result",
                            "tool_use_id": id,
                            "content": extract_web_search_sources_from_response_item(item)
                        }));
                        web_search_requests += 1;
                    }
                    "failed" => {
                        content.push(json!({
                            "type": "web_search_tool_result",
                            "tool_use_id": id,
                            "content": build_anthropic_web_search_error_content(
                                extract_response_web_search_error_code(item).as_deref()
                            )
                        }));
                    }
                    _ => {}
                }
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

    let mut usage_json = build_anthropic_usage_from_responses(body.get("usage"));
    if web_search_requests > 0 {
        usage_json["server_tool_use"] = json!({
            "web_search_requests": web_search_requests
        });
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
    fn test_anthropic_to_responses_simple() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["instructions"], "You are a helpful assistant.");
        // system should not appear in input
        assert_eq!(result["input"].as_array().unwrap().len(), 1);
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

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["instructions"], "Part 1\n\nPart 2");
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

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["name"], "get_weather");
        assert!(result["tools"][0].get("parameters").is_some());
        // input_schema should not appear
        assert!(result["tools"][0].get("input_schema").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_maps_web_search_tool_and_include() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Search this"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "allowed_domains": ["openai.com"],
                "user_location": {"type": "approximate", "country": "US"}
            }]
        });

        let result = anthropic_to_responses(input, None, true).unwrap();
        assert_eq!(result["tools"][0]["type"], "web_search");
        assert_eq!(
            result["tools"][0]["filters"]["allowed_domains"],
            json!(["openai.com"])
        );
        assert_eq!(
            result["tools"][0]["user_location"],
            json!({"type": "approximate", "country": "US"})
        );

        let includes = result["include"]
            .as_array()
            .expect("include should be array");
        assert!(includes
            .iter()
            .any(|v| v.as_str() == Some("web_search_call.action.sources")));
        assert!(!includes
            .iter()
            .any(|v| v.as_str() == Some("web_search_call.results")));
        assert!(includes
            .iter()
            .any(|v| v.as_str() == Some("reasoning.encrypted_content")));
    }

    #[test]
    fn test_anthropic_to_responses_rejects_blocked_domains_web_search() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Search this"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "blocked_domains": ["example.com"]
            }]
        });

        let err = anthropic_to_responses(input, None, false).unwrap_err();
        assert!(
            matches!(err, ProxyError::TransformError(message) if message.contains("blocked_domains"))
        );
    }

    #[test]
    fn test_anthropic_to_responses_rejects_mixed_allowed_and_blocked_domains_web_search() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Search this"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "allowed_domains": ["openai.com"],
                "blocked_domains": ["example.com"]
            }]
        });

        let err = anthropic_to_responses(input, None, false).unwrap_err();
        assert!(
            matches!(err, ProxyError::TransformError(message) if message.contains("Cannot specify both allowed_domains and blocked_domains"))
        );
    }

    #[test]
    fn test_anthropic_to_responses_omits_empty_allowed_domains_web_search() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Search this"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "allowed_domains": []
            }]
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["tools"][0]["type"], "web_search");
        assert!(result["tools"][0].get("filters").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_rejects_max_uses_web_search() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Search this"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 1
            }]
        });

        let err = anthropic_to_responses(input, None, false).unwrap_err();
        assert!(matches!(err, ProxyError::TransformError(message) if message.contains("max_uses")));
    }

    #[test]
    fn test_anthropic_to_responses_codex_oauth_ignores_max_uses_web_search() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 5
            }]
        });

        let result = anthropic_to_responses(input, None, true).unwrap();
        assert_eq!(result["tools"][0]["type"], "web_search");
    }

    #[test]
    fn test_anthropic_to_responses_tool_choice_any_to_required() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tool_choice": {"type": "any"}
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["tool_choice"]["type"], "function");
        assert_eq!(result["tool_choice"]["name"], "get_weather");
    }

    #[test]
    fn test_anthropic_to_responses_web_search_tool_choice_falls_back_to_auto() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Search"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search"
            }],
            "tool_choice": {"type": "tool", "name": "web_search"}
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["tool_choice"], "auto");
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // Should produce: function_call_output item (lifted)
        assert_eq!(input_arr.len(), 1);
        assert_eq!(input_arr[0]["type"], "function_call_output");
        assert_eq!(input_arr[0]["call_id"], "call_123");
        assert_eq!(input_arr[0]["output"], "Sunny, 25°C");
    }

    #[test]
    fn test_anthropic_to_responses_web_search_history_round_trip() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {
                        "type": "server_tool_use",
                        "id": "ws_1",
                        "name": "web_search",
                        "input": {"query": "OpenAI latest"}
                    },
                    {
                        "type": "web_search_tool_result",
                        "tool_use_id": "ws_1",
                        "content": [
                            {"type": "web_search_result", "url": "https://openai.com", "title": "OpenAI"}
                        ]
                    },
                    {"type": "text", "text": "Here is what I found."}
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
        let items = result["input"].as_array().expect("input should be array");

        assert_eq!(items[0]["type"], "web_search_call");
        assert_eq!(items[0]["id"], "ws_1");
        assert_eq!(items[0]["action"]["query"], "OpenAI latest");
        assert_eq!(
            items[0]["action"]["sources"][0]["url"],
            "https://openai.com"
        );
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(items[1]["content"][0]["text"], "Here is what I found.");
    }

    #[test]
    fn test_anthropic_to_responses_web_search_error_history_preserved() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {
                        "type": "server_tool_use",
                        "id": "ws_err_1",
                        "name": "web_search",
                        "input": {"query": "OpenAI latest"}
                    },
                    {
                        "type": "web_search_tool_result",
                        "tool_use_id": "ws_err_1",
                        "content": {
                            "type": "web_search_tool_result_error",
                            "error_code": "max_uses_exceeded"
                        }
                    }
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
        let items = result["input"].as_array().expect("input should be array");

        assert_eq!(items[0]["type"], "web_search_call");
        assert_eq!(items[0]["id"], "ws_err_1");
        assert_eq!(items[0]["status"], "failed");
        assert_eq!(items[0]["action"]["query"], "OpenAI latest");
        assert!(items[0]["action"].get("sources").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_failed_non_query_web_search_history_preserved() {
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {
                        "type": "server_tool_use",
                        "id": "ws_open_err_1",
                        "name": "web_search",
                        "input": {
                            "type": "open_page",
                            "url": "https://openai.com/research"
                        }
                    },
                    {
                        "type": "web_search_tool_result",
                        "tool_use_id": "ws_open_err_1",
                        "content": {
                            "type": "web_search_tool_result_error",
                            "error_code": "unavailable"
                        }
                    }
                ]
            }]
        });

        let result = anthropic_to_responses(input, None, false).unwrap();
        let items = result["input"].as_array().expect("input should be array");

        assert_eq!(items[0]["type"], "web_search_call");
        assert_eq!(items[0]["id"], "ws_open_err_1");
        assert_eq!(items[0]["status"], "failed");
        assert_eq!(items[0]["action"]["type"], "open_page");
        assert_eq!(items[0]["action"]["url"], "https://openai.com/research");
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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
    fn test_responses_to_anthropic_with_web_search_call() {
        let input = json!({
            "id": "resp_ws",
            "status": "completed",
            "model": "gpt-5-codex",
            "output": [
                {
                    "type": "web_search_call",
                    "id": "ws_1",
                    "action": {
                        "query": "OpenAI latest",
                        "sources": [
                            {"url": "https://openai.com", "title": "OpenAI"}
                        ]
                    }
                },
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Here is what I found."}]
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "server_tool_use");
        assert_eq!(result["content"][0]["id"], "ws_1");
        assert_eq!(result["content"][0]["input"]["query"], "OpenAI latest");
        assert_eq!(result["content"][1]["type"], "web_search_tool_result");
        assert_eq!(result["content"][1]["tool_use_id"], "ws_1");
        assert_eq!(
            result["content"][1]["content"][0]["url"],
            "https://openai.com"
        );
        assert_eq!(result["content"][2]["type"], "text");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(
            result["usage"]["server_tool_use"]["web_search_requests"],
            json!(1)
        );
    }

    #[test]
    fn test_responses_to_anthropic_with_failed_web_search_call() {
        let input = json!({
            "id": "resp_ws_failed",
            "status": "completed",
            "model": "gpt-5-codex",
            "output": [{
                "type": "web_search_call",
                "id": "ws_failed_1",
                "status": "failed",
                "action": {
                    "query": "OpenAI latest"
                }
            }],
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "server_tool_use");
        assert_eq!(result["content"][0]["id"], "ws_failed_1");
        assert_eq!(result["content"][0]["input"]["query"], "OpenAI latest");
        assert_eq!(result["content"][1]["type"], "web_search_tool_result");
        assert_eq!(result["content"][1]["tool_use_id"], "ws_failed_1");
        assert_eq!(
            result["content"][1]["content"]["type"],
            "web_search_tool_result_error"
        );
        assert_eq!(result["content"][1]["content"]["error_code"], "unavailable");
        assert!(result["usage"].get("server_tool_use").is_none());
    }

    #[test]
    fn test_responses_to_anthropic_preserves_non_query_web_search_action_input() {
        let input = json!({
            "id": "resp_ws_open_failed",
            "status": "completed",
            "model": "gpt-5-codex",
            "output": [{
                "type": "web_search_call",
                "id": "ws_open_failed_1",
                "status": "failed",
                "action": {
                    "type": "open_page",
                    "url": "https://openai.com/research"
                }
            }],
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "server_tool_use");
        assert_eq!(result["content"][0]["input"]["type"], "open_page");
        assert_eq!(
            result["content"][0]["input"]["url"],
            "https://openai.com/research"
        );
        assert_eq!(result["content"][1]["type"], "web_search_tool_result");
        assert_eq!(
            result["content"][1]["content"]["type"],
            "web_search_tool_result_error"
        );
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

        let result = anthropic_to_responses(input, None, false).unwrap();
        assert_eq!(result["model"], "o3-mini");
    }

    #[test]
    fn test_anthropic_to_responses_with_cache_key() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, Some("my-provider-id"), false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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
        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, false).unwrap();
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

        let result = anthropic_to_responses(input, None, true).unwrap();

        // store 必须显式为 false（ChatGPT 后端拒绝 true）
        assert_eq!(result["store"], json!(false));

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

        let result = anthropic_to_responses(input, None, false).unwrap();

        assert!(result.get("store").is_none());
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

        let result = anthropic_to_responses(input, None, true).unwrap();
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
    fn test_anthropic_to_responses_codex_oauth_strips_max_output_tokens() {
        // ChatGPT Plus/Pro 反代不接受 max_output_tokens（OpenAI 官方 codex-rs 的
        // ResponsesApiRequest 结构体里也没有这个字段），必须删除，否则服务端 400：
        // "Unsupported parameter: max_output_tokens"
        let input = json!({
            "model": "gpt-5-codex",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input, None, true).unwrap();

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

        let result = anthropic_to_responses(input, None, false).unwrap();

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

        let result = anthropic_to_responses(input, None, true).unwrap();

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

        let result = anthropic_to_responses(input, None, true).unwrap();

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

        let result = anthropic_to_responses(input, None, true).unwrap();

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

        let result = anthropic_to_responses(input, None, true).unwrap();

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

        let result = anthropic_to_responses(input, None, true).unwrap();

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

        let result = anthropic_to_responses(input, None, false).unwrap();

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

        let result = anthropic_to_responses(input, None, false).unwrap();

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
}
