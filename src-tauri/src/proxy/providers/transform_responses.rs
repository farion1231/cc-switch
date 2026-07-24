//! OpenAI Responses API 格式转换模块
//!
//! 实现 Anthropic Messages ↔ OpenAI Responses API 格式转换。
//! Responses API 是 OpenAI 2025 年推出的新一代 API，采用扁平化的 input/output 结构。
//!
//! 与 Chat Completions 的主要差异：
//! - tool_use/tool_result 从 message content 中"提升"为顶层 input item
//! - system prompt 使用 `instructions` 字段而非 system role message
//! - usage 字段命名与 Anthropic 一致 (input_tokens/output_tokens)

use crate::proxy::{
    error::ProxyError,
    json_canonical::canonical_json_string,
    tool_media::{
        strip_and_clamp_media_from_tool_value, ToolMediaScope, TOOL_RESULT_MEDIA_ATTACHED_MARKER,
    },
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::reasoning_bridge::{
    anthropic_block_from_openai_reasoning_item, openai_reasoning_item_from_anthropic_block,
};

pub(crate) const TOOL_RESULT_ERROR_MARKER: &str = "[cc-switch:tool-result-error]";

fn anthropic_image_to_responses_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    match source.get("type").and_then(Value::as_str) {
        Some("url") => source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(|url| json!({"type":"input_image","image_url":url})),
        Some("base64") | None => {
            let data = source.get("data").and_then(Value::as_str)?;
            if data.is_empty() {
                return None;
            }
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            Some(json!({
                "type":"input_image",
                "image_url":format!("data:{media_type};base64,{data}")
            }))
        }
        _ => None,
    }
}

fn anthropic_document_to_responses_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    let filename = block
        .get("title")
        .or_else(|| block.get("filename"))
        .and_then(Value::as_str)
        .unwrap_or("document.pdf");
    match source.get("type").and_then(Value::as_str) {
        Some("url") => source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(|url| json!({"type":"input_file","file_url":url,"filename":filename})),
        Some("base64") => {
            let data = source.get("data").and_then(Value::as_str)?;
            if data.is_empty() {
                return None;
            }
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("application/pdf");
            Some(json!({
                "type":"input_file",
                "file_data":format!("data:{media_type};base64,{data}"),
                "filename":filename
            }))
        }
        _ => None,
    }
}

fn anthropic_tool_result_to_responses_output(block: &Value) -> Value {
    let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
    let content = block.get("content");

    if !is_error {
        if let Some(text @ Value::String(_)) = content {
            if let Some(output) = alternate_image_tool_result_to_responses(text) {
                return Value::Array(output);
            }
            return text.clone();
        }
    }

    let mut output = Vec::new();
    if is_error {
        output.push(json!({"type":"input_text","text":TOOL_RESULT_ERROR_MARKER}));
    }

    match content {
        Some(Value::String(text)) => {
            if let Some(mut alternate) =
                alternate_image_tool_result_to_responses(&Value::String(text.clone()))
            {
                output.append(&mut alternate);
            } else {
                output.push(json!({"type":"input_text","text":text}));
            }
        }
        Some(Value::Array(blocks)) => {
            for part in blocks {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            output.push(json!({"type":"input_text","text":text}));
                        }
                    }
                    Some("image") => {
                        if let Some(image) = anthropic_image_to_responses_part(part) {
                            output.push(image);
                        } else if let Some(mut alternate) =
                            alternate_image_tool_result_to_responses(part)
                        {
                            output.append(&mut alternate);
                        } else {
                            output.push(json!({
                                "type":"input_text",
                                "text":canonical_json_string(part)
                            }));
                        }
                    }
                    Some("document") => {
                        if let Some(file) = anthropic_document_to_responses_part(part) {
                            output.push(file);
                        } else {
                            output.push(json!({
                                "type":"input_text",
                                "text":canonical_json_string(part)
                            }));
                        }
                    }
                    _ => {
                        if let Some(mut alternate) = alternate_image_tool_result_to_responses(part)
                        {
                            output.append(&mut alternate);
                        } else {
                            output.push(json!({
                                "type":"input_text",
                                "text":canonical_json_string(part)
                            }));
                        }
                    }
                }
            }
        }
        Some(value) => {
            if let Some(mut alternate) = alternate_image_tool_result_to_responses(value) {
                output.append(&mut alternate);
            } else {
                output.push(json!({
                    "type":"input_text",
                    "text":canonical_json_string(value)
                }));
            }
        }
        None => {}
    }

    Value::Array(output)
}

fn alternate_image_tool_result_to_responses(value: &Value) -> Option<Vec<Value>> {
    let mut cleaned = value.clone();
    let replacement_block = json!({
        "type":"input_text",
        "text":TOOL_RESULT_MEDIA_ATTACHED_MARKER
    });
    let mut chat_media_parts = Vec::new();
    let replaced = strip_and_clamp_media_from_tool_value(
        &mut cleaned,
        &mut chat_media_parts,
        ToolMediaScope::ImagesOnly,
        &replacement_block,
        TOOL_RESULT_MEDIA_ATTACHED_MARKER,
    );
    if replaced == 0 {
        return None;
    }

    let mut output = Vec::new();
    append_sanitized_responses_tool_value(&cleaned, &mut output);
    output.extend(
        chat_media_parts
            .iter()
            .filter_map(responses_image_from_chat_media),
    );
    Some(output)
}

fn append_sanitized_responses_tool_value(value: &Value, output: &mut Vec<Value>) {
    match value {
        Value::String(text) if !text.is_empty() => {
            output.push(json!({"type":"input_text","text":text}));
        }
        Value::Array(parts) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            output.push(json!({"type":"input_text","text":text}));
                        }
                    }
                    _ => output.push(json!({
                        "type":"input_text",
                        "text":canonical_json_string(part)
                    })),
                }
            }
        }
        Value::Object(object)
            if matches!(
                object.get("type").and_then(Value::as_str),
                Some("input_text" | "output_text" | "text")
            ) =>
        {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                output.push(json!({"type":"input_text","text":text}));
            }
        }
        Value::Null | Value::String(_) => {}
        other => output.push(json!({
            "type":"input_text",
            "text":canonical_json_string(other)
        })),
    }
}

fn responses_image_from_chat_media(part: &Value) -> Option<Value> {
    let image_url = part
        .pointer("/image_url/url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())?;
    let mut image = json!({
        "type":"input_image",
        "image_url":image_url
    });
    if let Some(detail) = part.pointer("/image_url/detail") {
        image["detail"] = detail.clone();
    }
    Some(image)
}

pub(crate) fn sanitize_anthropic_tool_use_input(name: &str, input: Value) -> Value {
    if name != "Read" {
        return input;
    }

    match input {
        Value::Object(mut object) => {
            if matches!(object.get("pages"), Some(Value::String(value)) if value.is_empty()) {
                object.remove("pages");
            }
            Value::Object(object)
        }
        other => other,
    }
}

pub(crate) fn sanitize_anthropic_tool_use_input_json(name: &str, raw: &str) -> String {
    if name != "Read" || raw.is_empty() {
        return raw.to_string();
    }

    let Ok(input) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };

    serde_json::to_string(&sanitize_anthropic_tool_use_input(name, input))
        .unwrap_or_else(|_| raw.to_string())
}

/// Anthropic versions its hosted web-search tool in the `type` field
/// (`web_search_20250305`, and potentially newer date-suffixed variants).
/// Match the semantic tool family instead of pinning the bridge to one release.
fn is_anthropic_web_search_tool(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .is_some_and(|tool_type| tool_type == "web_search" || tool_type.starts_with("web_search_"))
}

pub(crate) fn anthropic_web_search_tool_name(body: &Value) -> Option<&str> {
    body.get("tools")
        .and_then(Value::as_array)?
        .iter()
        .find(|tool| is_anthropic_web_search_tool(tool))
        .and_then(|tool| tool.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

fn anthropic_web_search_to_responses(tool: &Value) -> Result<Value, ProxyError> {
    let blocked_domains = tool
        .get("blocked_domains")
        .and_then(Value::as_array)
        .filter(|domains| !domains.is_empty());
    if blocked_domains.is_some() {
        // OpenAI Responses currently exposes an allow-list but no deny-list.
        // Failing closed avoids silently searching domains the caller excluded.
        return Err(ProxyError::InvalidRequest(
            "Anthropic WebSearch blocked_domains cannot be represented by the Responses API"
                .to_string(),
        ));
    }

    let mut response_tool = json!({
        "type": "web_search",
        "external_web_access": true
    });

    if let Some(allowed_domains) = tool
        .get("allowed_domains")
        .and_then(Value::as_array)
        .filter(|domains| !domains.is_empty())
    {
        response_tool["filters"] = json!({
            "allowed_domains": allowed_domains
        });
    }

    if let Some(user_location) = tool
        .get("user_location")
        .filter(|location| location.is_object())
    {
        response_tool["user_location"] = user_location.clone();
    }

    Ok(response_tool)
}

pub(crate) fn web_search_action_input(item: &Value) -> Value {
    let Some(action) = item.get("action").and_then(Value::as_object) else {
        return json!({});
    };

    let mut input = serde_json::Map::new();
    for key in ["query", "queries", "url", "pattern"] {
        if let Some(value) = action.get(key) {
            input.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(input)
}

fn collect_web_search_results_from_content(content: &Value, results: &mut Vec<Value>) {
    let Some(blocks) = content.as_array() else {
        return;
    };

    for block in blocks {
        let Some(annotations) = block.get("annotations").and_then(Value::as_array) else {
            continue;
        };
        for annotation in annotations {
            if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
                continue;
            }
            let Some(url) = annotation
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty())
            else {
                continue;
            };
            let title = annotation
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .unwrap_or(url);
            results.push(json!({
                "type": "web_search_result",
                "url": url,
                "title": title,
                "encrypted_content": "",
                "page_age": null
            }));
        }
    }
}

pub(crate) fn web_search_results_from_output_item(item: &Value) -> Vec<Value> {
    let mut results = Vec::new();
    if item.get("type").and_then(Value::as_str) == Some("message") {
        if let Some(content) = item.get("content") {
            collect_web_search_results_from_content(content, &mut results);
        }
    } else if item.get("type").and_then(Value::as_str) == Some("output_text") {
        collect_web_search_results_from_content(&json!([item]), &mut results);
    }

    let mut seen = HashSet::new();
    results.retain(|result| {
        result
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| seen.insert(url.to_string()))
    });
    results
}

pub(crate) fn web_search_results_from_action(item: &Value) -> Vec<Value> {
    let mut results = Vec::new();
    let Some(sources) = item.pointer("/action/sources").and_then(Value::as_array) else {
        return results;
    };

    for source in sources {
        let Some(url) = source
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
        else {
            continue;
        };
        let title = source
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.is_empty())
            .unwrap_or(url);
        let page_age = source
            .get("page_age")
            .filter(|page_age| page_age.is_string())
            .cloned()
            .unwrap_or(Value::Null);
        results.push(json!({
            "type": "web_search_result",
            "url": url,
            "title": title,
            "encrypted_content": "",
            "page_age": page_age
        }));
    }

    let mut seen = HashSet::new();
    results.retain(|result| {
        result
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| seen.insert(url.to_string()))
    });
    results
}

fn responses_web_search_call_from_anthropic_blocks(
    tool_use: &Value,
    tool_result: &Value,
) -> Option<Value> {
    let id = tool_use
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let input = tool_use.get("input").and_then(Value::as_object);

    // Anthropic omits the Responses action discriminator from server_tool_use
    // input. Infer it from the action-specific fields while keeping search as
    // the safe default for current and future hosted WebSearch tool names.
    let action_type = if input.is_some_and(|input| {
        input.get("pattern").and_then(Value::as_str).is_some()
            && input.get("url").and_then(Value::as_str).is_some()
    }) {
        "find_in_page"
    } else if input.is_some_and(|input| {
        input.get("url").and_then(Value::as_str).is_some()
            && !input.contains_key("query")
            && !input.contains_key("queries")
    }) {
        "open_page"
    } else {
        "search"
    };

    let mut action = serde_json::Map::new();
    action.insert("type".to_string(), json!(action_type));
    if let Some(input) = input {
        let fields: &[&str] = match action_type {
            "find_in_page" => &["url", "pattern"],
            "open_page" => &["url"],
            _ => &["query", "queries"],
        };
        for field in fields {
            if let Some(value) = input.get(*field) {
                action.insert((*field).to_string(), value.clone());
            }
        }
    }

    if action_type == "search" {
        let mut seen_urls = HashSet::new();
        let sources: Vec<Value> = tool_result
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|result| {
                let url = result
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|url| !url.is_empty())?;
                seen_urls
                    .insert(url.to_string())
                    .then(|| json!({"type": "url", "url": url}))
            })
            .collect();
        if !sources.is_empty() {
            action.insert("sources".to_string(), Value::Array(sources));
        }
    }

    let failed = tool_result.get("is_error").and_then(Value::as_bool) == Some(true)
        || tool_result
            .pointer("/content/type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.ends_with("_error"));

    Some(json!({
        "type": "web_search_call",
        "id": id,
        "status": if failed { "failed" } else { "completed" },
        "action": Value::Object(action)
    }))
}

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
    let mut hosted_web_search_names = HashSet::new();
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let mut response_tools = Vec::new();
        for tool in tools
            .iter()
            .filter(|t| t.get("type").and_then(Value::as_str) != Some("BatchTool"))
        {
            if is_anthropic_web_search_tool(tool) {
                hosted_web_search_names.insert(
                    tool.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("web_search")
                        .to_string(),
                );
                response_tools.push(anthropic_web_search_to_responses(tool)?);
            } else {
                response_tools.push(json!({
                    "type": "function",
                    "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
                    "description": tool.get("description"),
                    "parameters": super::transform::clean_schema(
                        tool.get("input_schema").cloned().unwrap_or(json!({}))
                    )
                }));
            }
        }

        if !response_tools.is_empty() {
            result["tools"] = json!(response_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] =
            map_tool_choice_to_responses(v, &hosted_web_search_names, is_codex_oauth);
    }

    const WEB_SEARCH_SOURCES_MARKER: &str = "web_search_call.action.sources";
    // OpenAI otherwise returns citations only on the final message, without a
    // search-call ID. Request per-call sources so multiple hosted searches can
    // be paired with the corresponding Anthropic result blocks.
    if !hosted_web_search_names.is_empty() && !is_codex_oauth {
        result["include"] = json!([WEB_SEARCH_SOURCES_MARKER]);
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
        if !hosted_web_search_names.is_empty()
            && !includes
                .iter()
                .any(|v| v.as_str() == Some(WEB_SEARCH_SOURCES_MARKER))
        {
            includes.push(json!(WEB_SEARCH_SOURCES_MARKER));
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

fn map_tool_choice_to_responses(
    tool_choice: &Value,
    hosted_web_search_names: &HashSet<String>,
    is_codex_oauth: bool,
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
                if hosted_web_search_names.contains(name) {
                    // The ChatGPT Codex backend's canonical request schema accepts a
                    // string tool_choice. The WebSearch helper sends only this hosted
                    // tool, so `required` preserves the forced-tool behavior.
                    if is_codex_oauth {
                        json!("required")
                    } else {
                        json!({"type": "web_search"})
                    }
                } else {
                    json!({
                        "type": "function",
                        "name": name
                    })
                }
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

fn responses_error_message(body: &Value, fallback: &str) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .or_else(|| body.get("error").and_then(Value::as_str))
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn validate_responses_terminal_status(body: &Value) -> Result<(), ProxyError> {
    let status = body.get("status").and_then(Value::as_str);
    let has_error = body.get("error").is_some_and(|error| !error.is_null());

    match status {
        Some("failed") => Err(ProxyError::TransformError(format!(
            "Responses upstream failed: {}",
            responses_error_message(body, "response generation failed")
        ))),
        Some("cancelled") => Err(ProxyError::TransformError(format!(
            "Responses upstream cancelled the response: {}",
            responses_error_message(body, "response generation was cancelled")
        ))),
        _ if has_error => Err(ProxyError::TransformError(format!(
            "Responses upstream returned an error envelope: {}",
            responses_error_message(body, "unknown upstream error")
        ))),
        _ => Ok(()),
    }
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
/// 4. cache_creation_input_tokens: Direct field → nested
///    input_tokens_details.cache_write_tokens → prompt_tokens_details.cache_write_tokens
///
/// **Cache Token Priority Order**:
/// 1. OpenAI nested details (`cached_tokens`, `cache_write_tokens`) as initial values
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
    // GPT-5.6+ reports cache writes in the nested OpenAI token-details object.
    // Treat writes as Anthropic cache creation so the downstream client and
    // billing layer can distinguish them from fresh input.
    let nested_cache_write = u
        .pointer("/input_tokens_details/cache_write_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            u.pointer("/prompt_tokens_details/cache_write_tokens")
                .and_then(|v| v.as_u64())
        });
    if let Some(cache_write) = nested_cache_write {
        result["cache_creation_input_tokens"] = json!(cache_write);
    }

    // Step 2: Direct Anthropic-style fields override (authoritative if present)
    // These preserve cache tokens even if input/output_tokens are missing
    if let Some(v) = u.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = v.clone();
    }
    if let Some(v) = u.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = v.clone();
    }
    if let Some(v) = u.get("cache_creation") {
        result["cache_creation"] = v.clone();
    }

    // OpenAI/Responses 的 input(prompt_tokens/input_tokens)含缓存命中，Anthropic input_tokens 不含
    // → 减去 cache_read 与 cache_creation，使其成为 fresh input。本函数在计量意义上是 claude 专属
    // （Codex Responses 透传走 from_codex_response_*，不调用本函数），故可安全在此扣减。三桶互斥，
    // 恒等：input + cache_read + cache_creation == 上游 input(inclusive)。与 build_anthropic_usage_json
    // (#2774) 及 transform_gemini 的 saturating_sub 对称；一处同时覆盖非流式与流式(streaming_responses)。
    let cached = result
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation = result
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if cached > 0 || cache_creation > 0 {
        result["input_tokens"] = json!(input.saturating_sub(cached).saturating_sub(cache_creation));
    }

    result
}

/// 将 Anthropic messages 数组转换为 Responses API input 数组
///
/// 核心转换逻辑：
/// - user/assistant 的 text 内容 → 对应 role 的 message item
/// - tool_use 从 assistant message 中"提升"为独立的 function_call item
/// - tool_result 从 user message 中"提升"为独立的 function_call_output item
/// - hosted WebSearch call/result blocks → restore one Responses web_search_call item
/// - bridge-owned thinking blocks → restore the original Responses reasoning item
/// - unrelated native thinking blocks → 丢弃
fn convert_messages_to_input(messages: &[Value]) -> Result<Vec<Value>, ProxyError> {
    let mut input = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let content = msg.get("content");
        let message_input_start = input.len();

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
                let hosted_web_search_results: HashMap<&str, &Value> = blocks
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
                    })
                    .filter_map(|block| {
                        block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .map(|id| (id, block))
                    })
                    .collect();

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
                            if let Some(image) = anthropic_image_to_responses_part(block) {
                                message_content.push(image);
                            } else {
                                log::warn!(
                                    "[Responses] Unsupported or invalid Anthropic image block"
                                );
                            }
                        }

                        "document" => {
                            if let Some(file) = anthropic_document_to_responses_part(block) {
                                message_content.push(file);
                            } else {
                                log::warn!(
                                    "[Responses] Unsupported or invalid Anthropic document block"
                                );
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
                                "arguments": canonical_json_string(&arguments)
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
                            let output = anthropic_tool_result_to_responses_output(block);

                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": output
                            }));
                        }

                        "server_tool_use" => {
                            let tool_use_id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .filter(|id| !id.is_empty());
                            let web_search_call = tool_use_id
                                .and_then(|id| hosted_web_search_results.get(id))
                                .and_then(|result| {
                                    responses_web_search_call_from_anthropic_blocks(block, result)
                                });
                            if let Some(web_search_call) = web_search_call {
                                if !message_content.is_empty() {
                                    input.push(json!({
                                        "role": role,
                                        "content": message_content.clone()
                                    }));
                                    message_content.clear();
                                }
                                input.push(web_search_call);
                            }
                        }

                        // A Responses web_search_call embeds the hosted result
                        // sources in action.sources, so the paired result block
                        // is consumed together with server_tool_use above.
                        "web_search_tool_result" => {}

                        "thinking" | "redacted_thinking" => {
                            if let Some(reasoning_item) =
                                openai_reasoning_item_from_anthropic_block(block)
                            {
                                if !message_content.is_empty() {
                                    input.push(json!({
                                        "role": role,
                                        "content": message_content.clone()
                                    }));
                                    message_content.clear();
                                }
                                input.push(reasoning_item);
                            }
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

        // A replayed reasoning item is only valid when the same assistant
        // generation also contains a following message/function call item.
        // Reasoning-only incomplete turns otherwise brick the next request with
        // "reasoning item ... without its required following item".
        if role == "assistant" {
            let mut has_generated_follower = false;
            for index in (message_input_start..input.len()).rev() {
                let item_type = input[index].get("type").and_then(Value::as_str);
                let is_assistant_message =
                    input[index].get("role").and_then(Value::as_str) == Some("assistant");
                if item_type == Some("reasoning") {
                    if !has_generated_follower {
                        input.remove(index);
                    }
                } else if matches!(item_type, Some("function_call" | "web_search_call"))
                    || is_assistant_message
                {
                    has_generated_follower = true;
                }
            }
        }
    }

    Ok(input)
}

/// OpenAI Responses 响应 → Anthropic 响应
pub fn responses_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    responses_to_anthropic_with_web_search_name(body, None)
}

pub(crate) fn responses_to_anthropic_with_web_search_name(
    body: Value,
    hosted_web_search_name: Option<&str>,
) -> Result<Value, ProxyError> {
    // A Responses failure can arrive inside an HTTP 2xx response object. Reject it
    // before looking at `output`; otherwise `{status:"failed", output:[]}` becomes
    // a successful empty Anthropic `end_turn` and hides the upstream error.
    validate_responses_terminal_status(&body)?;

    let output = body
        .get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| ProxyError::TransformError("No output in response".to_string()))?;

    let mut content = Vec::new();
    let response_completed = body.get("status").and_then(Value::as_str) == Some("completed");
    let hosted_web_search_name = hosted_web_search_name
        .filter(|name| !name.is_empty())
        .unwrap_or("web_search");
    let web_search_indices: Vec<usize> = output
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            (item.get("type").and_then(Value::as_str) == Some("web_search_call")).then_some(index)
        })
        .collect();
    let last_web_search_index = web_search_indices.last().copied();
    let mut web_search_results_by_index = HashMap::new();
    let mut attributed_web_search_urls = HashSet::new();
    for &output_index in &web_search_indices {
        let results = web_search_results_from_action(&output[output_index]);
        for result in &results {
            if let Some(url) = result.get("url").and_then(Value::as_str) {
                attributed_web_search_urls.insert(url.to_string());
            }
        }
        web_search_results_by_index.insert(output_index, results);
    }

    let mut unassigned_web_search_results = Vec::new();
    let mut seen_web_search_urls = HashSet::new();
    for item in output {
        for result in web_search_results_from_output_item(item) {
            let Some(url) = result.get("url").and_then(Value::as_str) else {
                continue;
            };
            if seen_web_search_urls.insert(url.to_string())
                && !attributed_web_search_urls.contains(url)
            {
                unassigned_web_search_results.push(result);
            }
        }
    }
    // Compatible gateways may ignore the requested action.sources include.
    // Message annotations do not identify which call produced them, so keep
    // every call/result pair valid and attach only the unassigned citations to
    // the final call as a deterministic best-effort fallback.
    if let Some(last_web_search_index) = last_web_search_index {
        let results = web_search_results_by_index
            .entry(last_web_search_index)
            .or_insert_with(Vec::new);
        let mut seen_last_urls: HashSet<String> = results
            .iter()
            .filter_map(|result| result.get("url").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect();
        for result in unassigned_web_search_results {
            let Some(url) = result.get("url").and_then(Value::as_str) else {
                continue;
            };
            if seen_last_urls.insert(url.to_string()) {
                results.push(result);
            }
        }
    }

    let mut has_tool_use = false;
    let mut web_search_count = 0_u64;
    for (output_index, item) in output.iter().enumerate() {
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
                let input: Value = if args_str.trim().is_empty() {
                    json!({})
                } else {
                    match serde_json::from_str(args_str) {
                        Ok(value) => value,
                        Err(error) if !response_completed => {
                            log::warn!(
                                "[Responses] Replacing incomplete function_call '{name}' arguments with an empty object: {error}"
                            );
                            json!({})
                        }
                        Err(error) => {
                            return Err(ProxyError::TransformError(format!(
                                "Invalid function_call arguments for '{name}': {error}"
                            )))
                        }
                    }
                };
                if !input.is_object() {
                    if !response_completed {
                        log::warn!(
                            "[Responses] Replacing incomplete function_call '{name}' non-object arguments with an empty object"
                        );
                        content.push(json!({
                            "type": "tool_use",
                            "id": call_id,
                            "name": name,
                            "input": {}
                        }));
                        has_tool_use = true;
                        continue;
                    }
                    return Err(ProxyError::TransformError(format!(
                        "Function call arguments for '{name}' must be a JSON object"
                    )));
                }
                let input = sanitize_anthropic_tool_use_input(name, input);

                content.push(json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": input
                }));
                has_tool_use = true;
            }

            "web_search_call" => {
                web_search_count += 1;
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("ws_{output_index}"));
                content.push(json!({
                    "type": "server_tool_use",
                    "id": id,
                    "name": hosted_web_search_name,
                    "input": web_search_action_input(item),
                    "caller": {"type": "direct"}
                }));

                content.push(json!({
                    "type": "web_search_tool_result",
                    "tool_use_id": id,
                    "content": web_search_results_by_index
                        .remove(&output_index)
                        .unwrap_or_default(),
                    "caller": {"type": "direct"}
                }));
            }

            "reasoning" => {
                if let Some(block) = anthropic_block_from_openai_reasoning_item(item) {
                    content.push(block);
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
    if web_search_count > 0 {
        usage_json["server_tool_use"] = json!({
            "web_search_requests": web_search_count
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
        assert_eq!(result["tools"][0]["parameters"]["type"], json!("object"));
        assert_eq!(
            result["tools"][0]["parameters"]["properties"]["location"]["type"],
            json!("string")
        );
        // input_schema should not appear
        assert!(result["tools"][0].get("input_schema").is_none());
    }

    #[test]
    fn test_anthropic_hosted_web_search_maps_to_codex_tool_across_versions() {
        let input = json!({
            "model": "gpt-5.6",
            "messages": [{"role": "user", "content": "Search Rust docs"}],
            "tools": [{
                "type": "web_search_20991231",
                "name": "web_search_next",
                "max_uses": 8,
                "allowed_domains": ["rust-lang.org"],
                "blocked_domains": []
            }],
            "tool_choice": {"type": "tool", "name": "web_search_next"}
        });

        assert_eq!(
            anthropic_web_search_tool_name(&input),
            Some("web_search_next")
        );
        let result = anthropic_to_responses(input, None, true, false).unwrap();
        assert_eq!(
            result["tools"][0],
            json!({
                "type": "web_search",
                "external_web_access": true,
                "filters": {"allowed_domains": ["rust-lang.org"]}
            })
        );
        assert_eq!(result["tool_choice"], "required");
        assert_eq!(
            result["include"],
            json!([
                "reasoning.encrypted_content",
                "web_search_call.action.sources"
            ])
        );
    }

    #[test]
    fn test_anthropic_hosted_web_search_uses_responses_selector_for_api_key_backend() {
        let input = json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "Search"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search"}],
            "tool_choice": {"type": "tool", "name": "web_search"}
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        assert_eq!(result["tool_choice"], json!({"type": "web_search"}));
        assert_eq!(result["include"], json!(["web_search_call.action.sources"]));
    }

    #[test]
    fn test_anthropic_to_responses_replays_hosted_web_search_context() {
        let input = json!({
            "model": "gpt-5.6",
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "server_tool_use",
                            "id": "ws_previous",
                            "name": "web_search_next_version",
                            "input": {"queries": ["Rust ownership", "Rust borrowing"]},
                            "caller": {"type": "direct"}
                        },
                        {
                            "type": "web_search_tool_result",
                            "tool_use_id": "ws_previous",
                            "content": [
                                {
                                    "type": "web_search_result",
                                    "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html",
                                    "title": "Understanding Ownership",
                                    "encrypted_content": "",
                                    "page_age": null
                                },
                                {
                                    "type": "web_search_result",
                                    "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html",
                                    "title": "Duplicate",
                                    "encrypted_content": "",
                                    "page_age": null
                                }
                            ],
                            "caller": {"type": "direct"}
                        },
                        {"type": "text", "text": "Rust ownership is documented here."}
                    ]
                },
                {
                    "role": "user",
                    "content": "What did that source say about borrowing?"
                }
            ],
            "tools": [{
                "type": "web_search_20991231",
                "name": "web_search_next_version"
            }]
        });

        let result = anthropic_to_responses(input, None, true, false).unwrap();
        let replayed = result["input"].as_array().unwrap();

        assert_eq!(
            replayed[0],
            json!({
                "type": "web_search_call",
                "id": "ws_previous",
                "status": "completed",
                "action": {
                    "type": "search",
                    "queries": ["Rust ownership", "Rust borrowing"],
                    "sources": [{
                        "type": "url",
                        "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html"
                    }]
                }
            })
        );
        assert_eq!(replayed[1]["role"], "assistant");
        assert_eq!(
            replayed[1]["content"][0],
            json!({
                "type": "output_text",
                "text": "Rust ownership is documented here."
            })
        );
        assert_eq!(replayed[2]["role"], "user");
        assert_eq!(
            replayed[2]["content"][0]["text"],
            "What did that source say about borrowing?"
        );
    }

    #[test]
    fn test_custom_function_named_web_search_remains_a_function() {
        let input = json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "Call my tool"}],
            "tools": [{
                "name": "web_search",
                "description": "A user-defined function",
                "input_schema": {"type": "object"}
            }],
            "tool_choice": {"type": "tool", "name": "web_search"}
        });

        let result = anthropic_to_responses(input, None, true, false).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["name"], "web_search");
        assert_eq!(
            result["tool_choice"],
            json!({"type": "function", "name": "web_search"})
        );
    }

    #[test]
    fn test_hosted_web_search_blocked_domains_fails_closed() {
        let input = json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "Search"}],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "blocked_domains": ["example.com"]
            }]
        });

        let error = anthropic_to_responses(input, None, true, false).unwrap_err();
        assert!(error.to_string().contains("blocked_domains"));
    }

    #[test]
    fn test_anthropic_to_responses_defaults_missing_tool_schema_type() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let parameters = &result["tools"][0]["parameters"];
        assert_eq!(parameters["type"], json!("object"));
        assert_eq!(
            parameters["properties"]["location"]["type"],
            json!("string")
        );
    }

    #[test]
    fn test_anthropic_to_responses_defaults_empty_tool_schema() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Do work"}],
            "tools": [{"name": "do_work", "input_schema": {}}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let parameters = &result["tools"][0]["parameters"];
        assert_eq!(parameters, &json!({"type": "object", "properties": {}}));
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
    fn test_anthropic_to_responses_tool_result_preserves_blocks_and_error() {
        let input = json!({
            "model":"gpt-5",
            "messages":[{"role":"user","content":[{
                "type":"tool_result",
                "tool_use_id":"call_1",
                "is_error":true,
                "content":[
                    {"type":"text","text":"command failed"},
                    {"type":"image","source":{"type":"url","url":"https://example.com/error.png"}},
                    {"type":"document","title":"trace.pdf","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0="}}
                ]
            }]}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let output = result["input"][0]["output"].as_array().unwrap();
        assert_eq!(output[0]["text"], TOOL_RESULT_ERROR_MARKER);
        assert_eq!(
            output[1],
            json!({"type":"input_text","text":"command failed"})
        );
        assert_eq!(output[2]["image_url"], "https://example.com/error.png");
        assert_eq!(output[3]["type"], "input_file");
        assert_eq!(output[3]["filename"], "trace.pdf");
    }

    #[test]
    fn test_anthropic_to_responses_converts_mcp_tool_image() {
        let input = json!({
            "model":"gpt-5",
            "messages":[{"role":"user","content":[{
                "type":"tool_result",
                "tool_use_id":"call_1",
                "content":[{
                    "type":"image",
                    "mimeType":"image/webp",
                    "data":"MCP_RESPONSES_IMAGE_SENTINEL"
                }]
            }]}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let output = result["input"][0]["output"].as_array().unwrap();

        assert_eq!(output[0]["type"], "input_text");
        assert!(!output[0]["text"]
            .as_str()
            .unwrap()
            .contains("MCP_RESPONSES_IMAGE_SENTINEL"));
        assert_eq!(output[1]["type"], "input_image");
        assert_eq!(
            output[1]["image_url"],
            "data:image/webp;base64,MCP_RESPONSES_IMAGE_SENTINEL"
        );
    }

    #[test]
    fn test_anthropic_to_responses_converts_json_string_tool_image() {
        let residual_base64 = "A".repeat(20_000);
        let encoded = json!({
            "content":[
                {
                    "type":"image_url",
                    "image_url":{"url":"data:image/png;base64,STRING_RESPONSES_SENTINEL"}
                },
                {"type":"video","data":residual_base64}
            ]
        })
        .to_string();
        let input = json!({
            "model":"gpt-5",
            "messages":[{"role":"user","content":[{
                "type":"tool_result",
                "tool_use_id":"call_1",
                "content":encoded
            }]}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let output = result["input"][0]["output"].as_array().unwrap();
        let image = output
            .iter()
            .find(|part| part["type"] == "input_image")
            .expect("stringified image must stay a Responses image");

        assert_eq!(
            image["image_url"],
            "data:image/png;base64,STRING_RESPONSES_SENTINEL"
        );
        assert!(output
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .all(|text| !text.contains("STRING_RESPONSES_SENTINEL")));
        let serialized = result.to_string();
        assert!(serialized.contains("[cc-switch: omitted 20000 bytes]"));
        assert!(!serialized.contains(&"A".repeat(64)));
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
    fn test_anthropic_to_responses_url_image_and_document() {
        let input = json!({
            "model":"gpt-5",
            "messages":[{"role":"user","content":[
                {"type":"image","source":{"type":"url","url":"https://example.com/a.png"}},
                {"type":"document","title":"manual.pdf","source":{"type":"url","url":"https://example.com/manual.pdf"}}
            ]}]
        });

        let result = anthropic_to_responses(input, None, false, false).unwrap();
        let content = result["input"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "input_image");
        assert_eq!(content[0]["image_url"], "https://example.com/a.png");
        assert_eq!(content[1]["type"], "input_file");
        assert_eq!(content[1]["file_url"], "https://example.com/manual.pdf");
        assert_eq!(content[1]["filename"], "manual.pdf");
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
    fn test_responses_to_anthropic_hosted_web_search() {
        let input = json!({
            "id": "resp_search",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [
                {
                    "type": "web_search_call",
                    "id": "ws_123",
                    "status": "completed",
                    "action": {"type": "search", "query": "Rust official documentation"}
                },
                {
                    "type": "message",
                    "id": "msg_123",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "The Rust documentation is available online.",
                        "annotations": [
                            {
                                "type": "url_citation",
                                "url": "https://doc.rust-lang.org/",
                                "title": "The Rust Programming Language"
                            },
                            {
                                "type": "url_citation",
                                "url": "https://doc.rust-lang.org/",
                                "title": "Duplicate citation"
                            }
                        ]
                    }]
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result =
            responses_to_anthropic_with_web_search_name(input, Some("web_search_next")).unwrap();
        assert_eq!(result["content"][0]["type"], "server_tool_use");
        assert_eq!(result["content"][0]["id"], "ws_123");
        assert_eq!(result["content"][0]["name"], "web_search_next");
        assert_eq!(
            result["content"][0]["input"]["query"],
            "Rust official documentation"
        );
        assert_eq!(result["content"][1]["type"], "web_search_tool_result");
        assert_eq!(result["content"][1]["tool_use_id"], "ws_123");
        assert_eq!(result["content"][1]["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            result["content"][1]["content"][0]["url"],
            "https://doc.rust-lang.org/"
        );
        assert_eq!(result["content"][2]["type"], "text");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["server_tool_use"]["web_search_requests"], 1);
    }

    #[test]
    fn test_responses_to_anthropic_pairs_every_hosted_web_search_call() {
        let input = json!({
            "id": "resp_multi_search",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [
                {
                    "type": "web_search_call",
                    "id": "ws_rust",
                    "status": "completed",
                    "action": {
                        "type": "search",
                        "query": "Rust language",
                        "sources": [{
                            "type": "url",
                            "url": "https://www.rust-lang.org/",
                            "title": "Rust"
                        }]
                    }
                },
                {
                    "type": "web_search_call",
                    "id": "ws_cargo",
                    "status": "completed",
                    "action": {
                        "type": "search",
                        "query": "Cargo documentation",
                        "sources": [{
                            "type": "url",
                            "url": "https://doc.rust-lang.org/cargo/",
                            "title": "Cargo"
                        }]
                    }
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "Rust and Cargo both have official documentation.",
                        "annotations": [
                            {
                                "type": "url_citation",
                                "url": "https://www.rust-lang.org/",
                                "title": "Rust"
                            },
                            {
                                "type": "url_citation",
                                "url": "https://doc.rust-lang.org/cargo/",
                                "title": "Cargo"
                            }
                        ]
                    }]
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = responses_to_anthropic(input).unwrap();
        let content = result["content"].as_array().unwrap();
        assert_eq!(content.len(), 5);
        assert_eq!(content[0]["type"], "server_tool_use");
        assert_eq!(content[0]["id"], "ws_rust");
        assert_eq!(content[1]["type"], "web_search_tool_result");
        assert_eq!(content[1]["tool_use_id"], "ws_rust");
        assert_eq!(
            content[1]["content"][0]["url"],
            "https://www.rust-lang.org/"
        );
        assert_eq!(content[2]["type"], "server_tool_use");
        assert_eq!(content[2]["id"], "ws_cargo");
        assert_eq!(content[3]["type"], "web_search_tool_result");
        assert_eq!(content[3]["tool_use_id"], "ws_cargo");
        assert_eq!(
            content[3]["content"][0]["url"],
            "https://doc.rust-lang.org/cargo/"
        );
        assert_eq!(content[4]["type"], "text");
        assert_eq!(result["usage"]["server_tool_use"]["web_search_requests"], 2);
    }

    #[test]
    fn test_responses_to_anthropic_pairs_calls_when_sources_are_unavailable() {
        let input = json!({
            "id": "resp_multi_search_without_sources",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [
                {
                    "type": "web_search_call",
                    "id": "ws_first",
                    "status": "completed",
                    "action": {"type": "search", "query": "first query"}
                },
                {
                    "type": "web_search_call",
                    "id": "ws_second",
                    "status": "completed",
                    "action": {"type": "search", "query": "second query"}
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "Combined answer.",
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.com/result",
                            "title": "Combined result"
                        }]
                    }]
                }
            ]
        });

        let result = responses_to_anthropic(input).unwrap();
        let content = result["content"].as_array().unwrap();
        assert_eq!(content[1]["type"], "web_search_tool_result");
        assert_eq!(content[1]["tool_use_id"], "ws_first");
        assert_eq!(content[1]["content"], json!([]));
        assert_eq!(content[3]["type"], "web_search_tool_result");
        assert_eq!(content[3]["tool_use_id"], "ws_second");
        assert_eq!(
            content[3]["content"][0]["url"],
            "https://example.com/result"
        );
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
    fn test_completed_function_call_empty_arguments_normalizes_to_object() {
        let input = json!({
            "id": "resp_empty_args",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "ping",
                "arguments": ""
            }],
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });
        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["input"], json!({}));
    }

    #[test]
    fn test_incomplete_function_call_invalid_arguments_uses_empty_object() {
        let input = json!({
            "id": "resp_partial_args",
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "model": "gpt-5.6",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "dangerous_tool",
                "arguments": "{\"path\":"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });
        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["input"], json!({}));
        assert_eq!(result["stop_reason"], "max_tokens");
    }

    #[test]
    fn test_completed_function_call_invalid_arguments_is_error() {
        let input = json!({
            "id": "resp_bad_args",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "broken_tool",
                "arguments": "{\"path\":"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });
        assert!(matches!(
            responses_to_anthropic(input),
            Err(ProxyError::TransformError(_))
        ));
    }

    #[test]
    fn test_responses_to_anthropic_read_drops_empty_pages() {
        let input = json!({
            "id": "resp_read",
            "object": "response",
            "status": "completed",
            "model": "gpt-5.5",
            "output": [{
                "type": "function_call",
                "id": "fc_read",
                "call_id": "call_read",
                "name": "Read",
                "arguments": "{\"file_path\":\"/tmp/demo.py\",\"limit\":2000,\"offset\":0,\"pages\":\"\"}",
                "status": "completed"
            }]
        });

        let result = responses_to_anthropic(input).unwrap();
        let tool_input = &result["content"][0]["input"];

        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "Read");
        assert_eq!(tool_input["file_path"], "/tmp/demo.py");
        assert_eq!(tool_input["limit"], 2000);
        assert_eq!(tool_input["offset"], 0);
        assert!(tool_input.get("pages").is_none());
    }

    #[test]
    fn test_responses_to_anthropic_preserves_empty_strings_for_other_tools() {
        let input = json!({
            "id": "resp_other",
            "object": "response",
            "status": "completed",
            "model": "gpt-5.5",
            "output": [{
                "type": "function_call",
                "id": "fc_other",
                "call_id": "call_other",
                "name": "search",
                "arguments": "{\"query\":\"\"}",
                "status": "completed"
            }]
        });

        let result = responses_to_anthropic(input).unwrap();

        assert_eq!(result["content"][0]["input"]["query"], "");
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
    fn test_encrypted_reasoning_round_trips_through_anthropic_history() {
        let original = json!({
            "type": "reasoning",
            "id": "rs_123",
            "summary": [{"type": "summary_text", "text": "Need a tool."}],
            "encrypted_content": "opaque-ciphertext"
        });
        let response = json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [original.clone()],
            "usage": {"input_tokens": 10, "output_tokens": 2}
        });

        let anthropic = responses_to_anthropic(response).unwrap();
        let thinking = anthropic["content"][0].clone();
        assert_eq!(thinking["type"], "thinking");
        assert!(thinking["signature"]
            .as_str()
            .is_some_and(|value| value.starts_with("ccswitch-openai-reasoning-v1:")));

        let replay = anthropic_to_responses(
            json!({
                "model": "gpt-5.6",
                "messages": [{"role": "assistant", "content": [
                    thinking,
                    {"type": "tool_use", "id": "call_1", "name": "lookup", "input": {}}
                ]}]
            }),
            None,
            true,
            false,
        )
        .unwrap();
        assert_eq!(replay["input"][0], original);
        assert_eq!(replay["input"][1]["type"], "function_call");
    }

    #[test]
    fn test_reasoning_only_assistant_turn_is_not_replayed() {
        let item = json!({
            "type": "reasoning",
            "id": "rs_orphan",
            "summary": [],
            "encrypted_content": "opaque"
        });
        let block = anthropic_block_from_openai_reasoning_item(&item).unwrap();
        let replay = anthropic_to_responses(
            json!({
                "model": "gpt-5.6",
                "messages": [
                    {"role": "assistant", "content": [block]},
                    {"role": "user", "content": "continue"}
                ]
            }),
            None,
            true,
            false,
        )
        .unwrap();

        assert_eq!(replay["input"].as_array().unwrap().len(), 1);
        assert_eq!(replay["input"][0]["role"], "user");
    }

    #[test]
    fn test_responses_failed_status_is_not_silent_empty_success() {
        let input = json!({
            "id": "resp_failed",
            "status": "failed",
            "error": {"type": "server_error", "message": "backend exploded"},
            "output": [],
            "usage": {"input_tokens": 10, "output_tokens": 0}
        });

        let error = responses_to_anthropic(input).unwrap_err();
        assert!(
            matches!(error, ProxyError::TransformError(message) if message.contains("backend exploded"))
        );
    }

    #[test]
    fn test_responses_error_envelope_preserves_upstream_message() {
        let input = json!({
            "error": {"type": "rate_limit_error", "message": "too many requests"}
        });

        let error = responses_to_anthropic(input).unwrap_err();
        assert!(
            matches!(error, ProxyError::TransformError(message) if message.contains("too many requests"))
        );
    }

    #[test]
    fn test_responses_cancelled_status_is_not_end_turn() {
        let input = json!({
            "id": "resp_cancelled",
            "status": "cancelled",
            "output": []
        });

        let error = responses_to_anthropic(input).unwrap_err();
        assert!(
            matches!(error, ProxyError::TransformError(message) if message.contains("cancelled"))
        );
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
        // input_tokens(100) 含 cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
        assert_eq!(result["usage"]["input_tokens"], 20);
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
        // cache_read(60)+cache_creation(20) 均从 input(100) 扣除，fresh = 100 - 60 - 20 = 20
        // 守恒：input(20) + cache_read(60) + cache_creation(20) == 上游 input(100)
        assert_eq!(result["usage"]["input_tokens"], 20);
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
        // input_tokens(100) 含 nested cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
        assert_eq!(result["input_tokens"], json!(20));
        assert_eq!(result["output_tokens"], json!(50));
        assert_eq!(result["cache_read_input_tokens"], json!(80));
    }

    #[test]
    fn test_build_usage_cache_write_tokens_from_nested_details() {
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 10,
            "input_tokens_details": {
                "cached_tokens": 30,
                "cache_write_tokens": 20
            }
        })));
        assert_eq!(result["input_tokens"], json!(50));
        assert_eq!(result["cache_read_input_tokens"], json!(30));
        assert_eq!(result["cache_creation_input_tokens"], json!(20));
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
        // 直传 cache_read(100) 优先于 nested(80)；input(100) - 100 = 0（fresh）
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["cache_read_input_tokens"], json!(100)); // Direct field overrides nested
    }

    #[test]
    fn test_build_usage_clamps_input_when_cache_exceeds_input() {
        // input(100) < cache_read(60)+cache_creation(50)=110：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let result = build_anthropic_usage_from_responses(Some(&json!({
            "input_tokens": 100,
            "output_tokens": 10,
            "cache_read_input_tokens": 60,
            "cache_creation_input_tokens": 50
        })));
        assert_eq!(result["input_tokens"], json!(0));
        assert_eq!(result["cache_read_input_tokens"], json!(60));
        assert_eq!(result["cache_creation_input_tokens"], json!(50));
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
