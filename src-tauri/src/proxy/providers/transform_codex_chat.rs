//! Codex Responses ↔ OpenAI Chat Completions conversion.
//!
//! This module is used when the Codex client talks to CC Switch through the
//! Responses API, while the selected upstream provider only exposes an
//! OpenAI-compatible Chat Completions endpoint.

use super::codex_chat_common::{
    append_reasoning_content, extract_reasoning_field_text, extract_reasoning_summary_text,
    response_function_call_item, response_function_call_item_with_namespace,
    split_leading_think_block,
};
use crate::provider::CodexChatReasoningConfig;
use crate::proxy::{
    error::ProxyError,
    json_canonical::{
        canonical_json_string, canonicalize_json_string_if_parseable, canonicalize_tool_arguments,
        short_sha256_hex,
    },
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const EXTRA_CHAT_PASSTHROUGH_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream_options",
    "top_logprobs",
    "user",
];

const TOOL_SEARCH_PROXY_NAME: &str = "tool_search";
const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
const CHAT_TOOL_NAME_MAX_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexToolKind {
    Function,
    Namespace,
    Custom,
    ToolSearch,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexToolSpec {
    pub(crate) kind: CodexToolKind,
    pub(crate) name: String,
    pub(crate) namespace: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexToolContext {
    chat_tools: Vec<Value>,
    seen_chat_names: HashSet<String>,
    chat_name_to_spec: HashMap<String, CodexToolSpec>,
    namespace_name_to_chat_name: HashMap<(String, String), String>,
}

impl CodexToolContext {
    pub(crate) fn chat_tools(&self) -> &[Value] {
        &self.chat_tools
    }

    pub(crate) fn lookup_chat_name(&self, chat_name: &str) -> Option<&CodexToolSpec> {
        self.chat_name_to_spec.get(chat_name)
    }

    pub(crate) fn is_custom_tool_chat_name(&self, chat_name: &str) -> bool {
        self.lookup_chat_name(chat_name)
            .is_some_and(|spec| matches!(&spec.kind, CodexToolKind::Custom))
    }

    fn chat_name_for_response_function(&self, name: &str, namespace: Option<&str>) -> String {
        if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
            if let Some(chat_name) = self
                .namespace_name_to_chat_name
                .get(&(namespace.to_string(), name.to_string()))
            {
                return chat_name.clone();
            }
            return flatten_namespace_tool_name(namespace, name);
        }

        name.to_string()
    }

    fn add_chat_tool(&mut self, chat_name: String, spec: CodexToolSpec, chat_tool: Value) {
        if chat_name.trim().is_empty() || self.seen_chat_names.contains(&chat_name) {
            return;
        }
        self.seen_chat_names.insert(chat_name.clone());
        if let Some(namespace) = spec.namespace.as_ref() {
            self.namespace_name_to_chat_name
                .insert((namespace.clone(), spec.name.clone()), chat_name.clone());
        }
        self.chat_name_to_spec.insert(chat_name, spec);
        self.chat_tools.push(chat_tool);
    }

    fn add_function_tool(&mut self, tool: &Value, namespace: Option<&str>) {
        let Some(original_name) = responses_tool_name(tool) else {
            return;
        };
        let chat_name = namespace
            .map(|namespace| flatten_namespace_tool_name(namespace, &original_name))
            .unwrap_or_else(|| original_name.clone());

        let Some(chat_tool) = responses_function_tool_to_chat_tool(tool, &chat_name) else {
            return;
        };
        let spec = CodexToolSpec {
            kind: if namespace.is_some() {
                CodexToolKind::Namespace
            } else {
                CodexToolKind::Function
            },
            name: original_name,
            namespace: namespace.map(ToString::to_string),
        };
        self.add_chat_tool(chat_name, spec, chat_tool);
    }

    fn add_custom_tool(&mut self, tool: &Value) {
        let Some(name) = responses_tool_name(tool) else {
            return;
        };
        let description = tool
            .get("description")
            .cloned()
            .unwrap_or_else(|| json!("Custom Codex tool."));
        let chat_tool = json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        CUSTOM_TOOL_INPUT_FIELD: {
                            "type": "string",
                            "description": "Input to pass to the custom Codex tool."
                        }
                    },
                    "required": [CUSTOM_TOOL_INPUT_FIELD]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::Custom,
            name: name.clone(),
            namespace: None,
        };
        self.add_chat_tool(name, spec, chat_tool);
    }

    fn add_tool_search_tool(&mut self) {
        let chat_tool = json!({
            "type": "function",
            "function": {
                "name": TOOL_SEARCH_PROXY_NAME,
                "description": "Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query for tools or connectors to load."
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of tool groups to return."
                        }
                    },
                    "required": ["query"]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::ToolSearch,
            name: TOOL_SEARCH_PROXY_NAME.to_string(),
            namespace: None,
        };
        self.add_chat_tool(TOOL_SEARCH_PROXY_NAME.to_string(), spec, chat_tool);
    }

    fn add_namespace_tool(&mut self, namespace_tool: &Value) {
        let Some(namespace) = namespace_tool.get("name").and_then(|v| v.as_str()) else {
            return;
        };
        let Some(children) = namespace_tool
            .get("tools")
            .or_else(|| namespace_tool.get("children"))
            .and_then(|v| v.as_array())
        else {
            return;
        };

        for child in children {
            if child.get("type").and_then(|v| v.as_str()) == Some("function") {
                self.add_function_tool(child, Some(namespace));
            }
        }
    }

    fn add_response_tool(&mut self, tool: &Value) {
        match tool {
            Value::String(name) => {
                self.add_custom_tool(&json!({
                    "type": "custom",
                    "name": name
                }));
            }
            Value::Object(_) => match tool.get("type").and_then(|v| v.as_str()) {
                Some("function") => self.add_function_tool(tool, None),
                Some("custom") => self.add_custom_tool(tool),
                Some("tool_search") => self.add_tool_search_tool(),
                Some("namespace") => self.add_namespace_tool(tool),
                _ => {}
            },
            _ => {}
        }
    }
}

pub(crate) fn build_codex_tool_context_from_request(body: &Value) -> CodexToolContext {
    let mut context = CodexToolContext::default();

    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        for tool in tools {
            context.add_response_tool(tool);
        }
    }

    if let Some(input) = body.get("input") {
        collect_tool_search_output_tools(input, &mut context);
    }

    context
}

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request.
#[allow(dead_code)]
pub fn responses_to_chat_completions(body: Value) -> Result<Value, ProxyError> {
    responses_to_chat_completions_with_reasoning(body, None)
}

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request,
/// using provider-declared Codex Chat reasoning capabilities when available.
pub fn responses_to_chat_completions_with_reasoning(
    body: Value,
    reasoning_config: Option<&CodexChatReasoningConfig>,
) -> Result<Value, ProxyError> {
    let mut result = json!({});
    let tool_context = build_codex_tool_context_from_request(&body);

    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let instructions = instruction_text(instructions);
        if !instructions.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }
    }

    if let Some(input) = body.get("input") {
        append_responses_input_as_chat_messages(input, &mut messages, &tool_context)?;
    }
    let messages = collapse_system_messages_to_head(messages);
    result["messages"] = json!(messages);

    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(max_tokens) = body.get("max_output_tokens") {
        if super::transform::is_openai_o_series(model) {
            result["max_completion_tokens"] = max_tokens.clone();
        } else {
            result["max_tokens"] = max_tokens.clone();
        }
    }
    if let Some(max_tokens) = body.get("max_tokens") {
        result["max_tokens"] = max_tokens.clone();
    }
    if let Some(max_tokens) = body.get("max_completion_tokens") {
        result["max_completion_tokens"] = max_tokens.clone();
    }

    for key in ["temperature", "top_p", "stream"] {
        if let Some(value) = body.get(key) {
            result[key] = value.clone();
        }
    }

    apply_reasoning_options(&mut result, &body, model, reasoning_config);

    let tools = tool_context.chat_tools();
    if !tools.is_empty() {
        result["tools"] = json!(tools);
    }

    if let Some(tool_choice) = body.get("tool_choice") {
        result["tool_choice"] = responses_tool_choice_to_chat(tool_choice, &tool_context);
    }

    for key in EXTRA_CHAT_PASSTHROUGH_FIELDS {
        if let Some(value) = body.get(*key) {
            result[*key] = value.clone();
        }
    }

    // OpenAI 兼容上游在流式下默认不在 SSE 里返回 usage，必须显式声明
    // include_usage 才会在末尾吐 usage chunk。Codex CLI 用 Responses 协议、
    // 自身不带 stream_options，缺这一注入会导致 kimi/MiniMax 等第三方流式请求的
    // token/成本/缓存命中率全部漏记（input/output/cache 全为 0）。
    let is_stream = result
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_stream {
        match result.get_mut("stream_options") {
            // 保留客户端可能透传的其它 stream_options 字段，仅补 include_usage。
            Some(Value::Object(opts)) => {
                opts.insert("include_usage".to_string(), json!(true));
            }
            _ => {
                result["stream_options"] = json!({ "include_usage": true });
            }
        }
    }

    Ok(result)
}

fn apply_reasoning_options(
    result: &mut Value,
    body: &Value,
    model: &str,
    config: Option<&CodexChatReasoningConfig>,
) {
    let Some(config) = config else {
        if super::transform::supports_reasoning_effort(model) {
            if let Some(effort) = body.pointer("/reasoning/effort") {
                result["reasoning_effort"] = effort.clone();
            }
        }
        return;
    };

    let supports_effort = config.supports_effort.unwrap_or(false);
    let supports_thinking = config.supports_thinking.unwrap_or(false) || supports_effort;
    let Some(reasoning_enabled) = reasoning_requested(body) else {
        return;
    };

    if supports_thinking {
        match config
            .thinking_param
            .as_deref()
            .unwrap_or("thinking")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "thinking" => {
                result["thinking"] = json!({
                    "type": if reasoning_enabled { "enabled" } else { "disabled" }
                });
            }
            "enable_thinking" => {
                result["enable_thinking"] = json!(reasoning_enabled);
            }
            "reasoning_split" => {
                result["reasoning_split"] = json!(reasoning_enabled);
            }
            _ => {}
        }
    }

    // effort_param 在 early return 之前算出：reasoning.effort 形态的「显式关闭」分支要用到。
    let effort_param = config
        .effort_param
        .as_deref()
        .unwrap_or("reasoning_effort")
        .trim()
        .to_ascii_lowercase();

    if !reasoning_enabled {
        // OpenRouter 原生 reasoning.effort 支持显式 "none"（语义：彻底关闭推理）。
        // 上游显式发 effort=none/off/disabled（或 reasoning=null）时 reasoning_enabled 为 false，
        // 直接 return 会丢失关闭意图——OpenRouter 部分模型默认开思考，不带字段无法关闭，
        // 造成行为与成本偏差；故对该形态忠实转发 {"reasoning":{"effort":"none"}}。
        // 顶层 reasoning_effort 平台的枚举不含 none，仍走上方 thinking 关闭路径、不发 effort。
        // 注意：完全不带 reasoning 字段时 reasoning_requested 返回 None 已提前 return，
        // 不会走到这里，故只有上游「显式」表达关闭才透传 none。
        if effort_param == "reasoning.effort" {
            result["reasoning"] = json!({ "effort": "none" });
        }
        return;
    }

    if !supports_effort {
        return;
    }

    let Some(effort) = body.pointer("/reasoning/effort").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(mapped) = map_reasoning_effort(effort, config.effort_value_mode.as_deref()) else {
        return;
    };

    match effort_param.as_str() {
        // OpenAI 风格顶层字段（DeepSeek 官方、OpenAI o-series 等）。
        "reasoning_effort" => {
            result["reasoning_effort"] = json!(mapped);
        }
        // OpenRouter 原生归一化对象：reasoning.effort 会被 OpenRouter 翻译成各底层模型
        // （OpenAI/Grok/Gemini/Anthropic）的正确推理参数，覆盖面比顶层 OpenAI 别名更全。
        // 本转换从空对象构造、不残留原始 reasoning 对象，故不会出现 reasoning 与
        // reasoning_effort 并存触发 400 的情况（参见 openclaw#24119）。
        "reasoning.effort" => {
            result["reasoning"] = json!({ "effort": mapped });
        }
        _ => {}
    }
}

fn reasoning_requested(body: &Value) -> Option<bool> {
    if let Some(effort) = body.pointer("/reasoning/effort").and_then(|v| v.as_str()) {
        return Some(!matches!(
            effort.trim().to_ascii_lowercase().as_str(),
            "none" | "off" | "disabled"
        ));
    }

    body.get("reasoning").map(|value| !value.is_null())
}

fn map_reasoning_effort(effort: &str, mode: Option<&str>) -> Option<&'static str> {
    let effort = effort.trim().to_ascii_lowercase();
    if matches!(effort.as_str(), "none" | "off" | "disabled") {
        return None;
    }

    match mode.unwrap_or("passthrough") {
        "deepseek" => match effort.as_str() {
            "max" | "xhigh" => Some("max"),
            _ => Some("high"),
        },
        "low_high" => match effort.as_str() {
            "minimal" | "low" => Some("low"),
            _ => Some("high"),
        },
        // OpenRouter effort 枚举为 xhigh|high|medium|low|minimal（无 max）。max 是
        // Codex / 部分模型的扩展档位，对 OpenRouter 非法，会触发
        // `400 reasoning_effort: Invalid option`（见 openclaw#77350）；钳到最高合法档
        // xhigh，其余合法值透传，未知值丢弃以免被上游拒绝。
        "openrouter" => match effort.as_str() {
            "max" | "xhigh" => Some("xhigh"),
            "high" => Some("high"),
            "medium" => Some("medium"),
            "low" => Some("low"),
            "minimal" => Some("minimal"),
            _ => None,
        },
        _ => match effort.as_str() {
            "minimal" => Some("minimal"),
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        },
    }
}

/// MiniMax 严格要求 messages 中只能首条出现 `role=system`，
/// 否则返回 `invalid params, chat content has invalid message role: system (2013)`。
/// 把所有 system 消息合并到首位，避免中间 system（如 Codex 的 `developer` 指令）触发该约束；
/// 该重排对 OpenAI / DeepSeek 等宽松兼容层也是无损的。
fn collapse_system_messages_to_head(messages: Vec<Value>) -> Vec<Value> {
    let mut system_chunks: Vec<String> = Vec::new();
    let mut rest: Vec<Value> = Vec::with_capacity(messages.len());

    for msg in messages {
        if msg.get("role").and_then(|v| v.as_str()) == Some("system") {
            if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    system_chunks.push(text.to_string());
                }
                continue;
            }
        }
        rest.push(msg);
    }

    let mut out: Vec<Value> = Vec::with_capacity(rest.len() + 1);
    if !system_chunks.is_empty() {
        out.push(json!({
            "role": "system",
            "content": system_chunks.join("\n\n")
        }));
    }
    out.extend(rest);
    out
}

fn instruction_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| part.as_str())
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        other => other.as_str().unwrap_or_default().to_string(),
    }
}

fn append_responses_input_as_chat_messages(
    input: &Value,
    messages: &mut Vec<Value>,
    tool_context: &CodexToolContext,
) -> Result<(), ProxyError> {
    let mut pending_tool_calls = Vec::new();
    let mut pending_reasoning: Option<String> = None;
    let mut last_assistant_index: Option<usize> = None;

    match input {
        Value::String(text) => {
            messages.push(json!({
                "role": "user",
                "content": text
            }));
        }
        Value::Array(items) => {
            for item in items {
                append_responses_item_as_chat_message(
                    item,
                    messages,
                    &mut pending_tool_calls,
                    &mut pending_reasoning,
                    &mut last_assistant_index,
                    tool_context,
                )?;
            }
        }
        Value::Object(_) => {
            append_responses_item_as_chat_message(
                input,
                messages,
                &mut pending_tool_calls,
                &mut pending_reasoning,
                &mut last_assistant_index,
                tool_context,
            )?;
        }
        _ => {}
    }

    flush_pending_tool_calls(
        messages,
        &mut pending_tool_calls,
        &mut pending_reasoning,
        &mut last_assistant_index,
    );
    backfill_tool_call_reasoning_placeholders(messages);
    Ok(())
}

fn append_responses_item_as_chat_message(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
    tool_context: &CodexToolContext,
) -> Result<(), ProxyError> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_function_call_to_chat_tool_call(
                item,
                tool_context,
            ));
        }
        Some("custom_tool_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_custom_tool_call_to_chat_tool_call(item));
        }
        Some("tool_search_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            pending_tool_calls.push(responses_tool_search_call_to_chat_tool_call(item));
        }
        Some("function_call_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let output = match item.get("output") {
                Some(Value::String(s)) => canonicalize_json_string_if_parseable(s),
                Some(v) => canonical_json_string(v),
                None => String::new(),
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("custom_tool_call_output") | Some("tool_search_output") => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let output = canonical_json_string(item);
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("reasoning") => {
            let reasoning = responses_reasoning_item_text(item);
            let attached_to_previous = pending_tool_calls.is_empty()
                && attach_reasoning_to_last_assistant(messages, *last_assistant_index, &reasoning);
            if !attached_to_previous {
                append_pending_reasoning(pending_reasoning, reasoning);
            }
        }
        Some("message") | None => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            if item.get("role").is_some() || item.get("content").is_some() {
                let message = responses_message_item_to_chat_message(item, pending_reasoning);
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            }
        }
        _ => {
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_reasoning,
                last_assistant_index,
            );
            if item.get("role").is_some() || item.get("content").is_some() {
                let message = responses_message_item_to_chat_message(item, pending_reasoning);
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            }
        }
    }

    Ok(())
}

fn flush_pending_tool_calls(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }

    let mut message = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": std::mem::take(pending_tool_calls)
    });
    attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    *last_assistant_index = Some(messages.len());
    messages.push(message);
}

fn responses_message_item_to_chat_message(
    item: &Value,
    pending_reasoning: &mut Option<String>,
) -> Value {
    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let chat_role = responses_role_to_chat_role(role);
    let content = item
        .get("content")
        .map(|value| responses_content_to_chat_content(chat_role, value))
        .unwrap_or(Value::Null);

    let mut message = json!({
        "role": chat_role,
        "content": content
    });

    if chat_role == "assistant" {
        append_pending_reasoning(pending_reasoning, responses_message_reasoning_text(item));
        attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    } else if pending_reasoning.is_some() {
        pending_reasoning.take();
    }

    message
}

fn responses_role_to_chat_role(role: &str) -> &'static str {
    match role {
        "system" | "developer" => "system",
        "assistant" => "assistant",
        "tool" => "tool",
        "user" | "latest_reminder" => "user",
        _ => "user",
    }
}

fn update_last_assistant_index(
    messages: &[Value],
    message: &Value,
    last_assistant_index: &mut Option<usize>,
) {
    match message.get("role").and_then(|v| v.as_str()) {
        Some("assistant") => {
            *last_assistant_index = Some(messages.len());
        }
        Some("tool") => {}
        _ => {
            *last_assistant_index = None;
        }
    }
}

fn append_pending_reasoning(pending_reasoning: &mut Option<String>, reasoning: Option<String>) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    match pending_reasoning {
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            *pending_reasoning = Some(reasoning.to_string());
        }
    }
}

fn append_unique_pending_reasoning(
    pending_reasoning: &mut Option<String>,
    reasoning: Option<String>,
) {
    let Some(reasoning) = reasoning else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    match pending_reasoning {
        Some(existing) if existing.contains(reasoning) => {}
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => {
            *pending_reasoning = Some(reasoning.to_string());
        }
    }
}

fn attach_pending_reasoning_to_assistant(
    message: &mut Value,
    pending_reasoning: &mut Option<String>,
) {
    let Some(reasoning) = pending_reasoning.take() else {
        return;
    };
    if reasoning.trim().is_empty() {
        return;
    }

    if let Some(obj) = message.as_object_mut() {
        append_reasoning_content(obj, &reasoning);
    }
}

/// 在所有 input 处理完毕后，对仍缺 `reasoning_content` 的 assistant tool-call 消息补占位。
/// 必须作为管线末端的最终兜底执行：真实 reasoning 可能以尾随 `reasoning` item 的形式经
/// `attach_reasoning_to_last_assistant` 回填，过早注入占位会被 `append_reasoning_content`
/// 追加而污染真实思考。
fn backfill_tool_call_reasoning_placeholders(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        let is_assistant_tool_call = message.get("role").and_then(|value| value.as_str())
            == Some("assistant")
            && message
                .get("tool_calls")
                .and_then(|value| value.as_array())
                .is_some_and(|calls| !calls.is_empty());
        if is_assistant_tool_call {
            ensure_tool_call_reasoning_content(message);
        }
    }
}

/// kimi/Moonshot、DeepSeek 等 thinking 模型要求每条带 `tool_calls` 的 assistant
/// 消息都必须携带非空 `reasoning_content`。跨轮历史恢复 miss（如代理重启丢失内存缓存、
/// call_id 歧义无法恢复、上游某轮未产出思考）时，这里补一个占位，避免上游返回
/// `reasoning_content is missing in assistant tool call message`。
/// 与 `transform::anthropic_to_openai_with_reasoning_content` 的占位行为保持对称。
fn ensure_tool_call_reasoning_content(message: &mut Value) {
    let Some(obj) = message.as_object_mut() else {
        return;
    };
    let has_reasoning = obj
        .get("reasoning_content")
        .and_then(|value| value.as_str())
        .is_some_and(|text| !text.trim().is_empty());
    if !has_reasoning {
        obj.insert(
            "reasoning_content".to_string(),
            Value::String("tool call".to_string()),
        );
    }
}

fn attach_reasoning_to_last_assistant(
    messages: &mut [Value],
    last_assistant_index: Option<usize>,
    reasoning: &Option<String>,
) -> bool {
    let Some(reasoning) = reasoning
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return true;
    };
    let Some(index) = last_assistant_index else {
        return false;
    };
    let Some(message) = messages.get_mut(index) else {
        return false;
    };
    if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return false;
    }

    if let Some(obj) = message.as_object_mut() {
        append_reasoning_content(obj, reasoning);
        return true;
    }

    false
}

fn responses_message_reasoning_text(item: &Value) -> Option<String> {
    responses_item_reasoning_text(item)
}

fn responses_item_reasoning_text(item: &Value) -> Option<String> {
    extract_reasoning_field_text(item)
}

fn responses_reasoning_item_text(item: &Value) -> Option<String> {
    extract_reasoning_summary_text(item)
}

fn responses_content_to_chat_content(_role: &str, content: &Value) -> Value {
    if content.is_null() || content.is_string() {
        return content.clone();
    }

    let Some(parts) = content.as_array() else {
        return content.clone();
    };

    let mut chat_parts: Vec<Value> = Vec::new();
    let mut has_non_text_part = false;

    for part in parts {
        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match part_type {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "refusal" => {
                if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chat_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                }
            }
            "input_image" => {
                if let Some(image_url) = part.get("image_url") {
                    let image_url = if image_url.is_object() {
                        image_url.clone()
                    } else {
                        json!({ "url": image_url.as_str().unwrap_or_default() })
                    };
                    chat_parts.push(json!({
                        "type": "image_url",
                        "image_url": image_url
                    }));
                    has_non_text_part = true;
                }
            }
            _ => {}
        }
    }

    if !has_non_text_part {
        return Value::String(
            chat_parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    Value::Array(chat_parts)
}

fn collect_tool_search_output_tools(value: &Value, context: &mut CodexToolContext) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_search_output_tools(item, context);
            }
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(|v| v.as_str()) == Some("tool_search_output") {
                if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
                    for tool in tools {
                        context.add_response_tool(tool);
                    }
                }
            }
            for value in obj.values() {
                collect_tool_search_output_tools(value, context);
            }
        }
        _ => {}
    }
}

fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full_name = format!("{namespace}__{name}");
    if full_name.len() <= CHAT_TOOL_NAME_MAX_LEN {
        return full_name;
    }

    let hash = short_sha256_hex(full_name.as_bytes());
    let suffix = format!("__{hash}");
    let prefix_len = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(suffix.len());
    let mut prefix = String::new();
    for ch in full_name.chars() {
        if prefix.len() + ch.len_utf8() > prefix_len {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}{suffix}")
}

fn responses_tool_name(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn responses_function_tool_to_chat_tool(tool: &Value, chat_name: &str) -> Option<Value> {
    if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
        return None;
    }

    if let Some(function) = tool.get("function") {
        let mut chat_tool = json!({
            "type": "function",
            "function": function.clone()
        });
        if let Some(obj) = chat_tool
            .get_mut("function")
            .and_then(|value| value.as_object_mut())
        {
            obj.insert("name".to_string(), json!(chat_name));
            if let Some(strict) = tool.get("strict").cloned() {
                obj.entry("strict".to_string()).or_insert(strict);
            }
        }
        return Some(chat_tool);
    }

    let mut function = json!({
        "name": chat_name,
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": tool.get("parameters").cloned().unwrap_or_else(|| json!({}))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }

    Some(json!({
        "type": "function",
        "function": function
    }))
}

fn responses_function_call_to_chat_tool_call(
    item: &Value,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let namespace = item.get("namespace").and_then(|v| v.as_str());
    let chat_name = tool_context.chat_name_for_response_function(name, namespace);
    let arguments = canonicalize_tool_arguments(item.get("arguments"));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": chat_name,
            "arguments": arguments
        }
    })
}

fn responses_custom_tool_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let input = item.get("input").cloned().unwrap_or_else(|| json!(""));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": canonical_json_string(&json!({ CUSTOM_TOOL_INPUT_FIELD: input }))
        }
    })
}

fn responses_tool_search_call_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = item
        .get("arguments")
        .map(canonical_json_string)
        .unwrap_or_else(|| "{}".to_string());

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": TOOL_SEARCH_PROXY_NAME,
            "arguments": arguments
        }
    })
}

fn responses_tool_choice_to_chat(tool_choice: &Value, tool_context: &CodexToolContext) -> Value {
    match tool_choice {
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("function") => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let namespace = obj.get("namespace").and_then(|v| v.as_str());
            let chat_name = tool_context.chat_name_for_response_function(name, namespace);
            json!({
                "type": "function",
                "function": {
                    "name": chat_name
                }
            })
        }
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("tool_search") => {
            json!({
                "type": "function",
                "function": {
                    "name": TOOL_SEARCH_PROXY_NAME
                }
            })
        }
        Value::Object(obj) if obj.get("type").and_then(|v| v.as_str()) == Some("custom") => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            json!({
                "type": "function",
                "function": {
                    "name": name
                }
            })
        }
        _ => tool_choice.clone(),
    }
}

/// Convert a non-streaming Chat Completions response into a Responses response.
#[allow(dead_code)]
pub fn chat_completion_to_response(body: Value) -> Result<Value, ProxyError> {
    chat_completion_to_response_with_context(body, &CodexToolContext::default())
}

/// Convert a non-streaming Chat Completions response into a Responses response,
/// restoring Codex-specific tool names using the original Responses request.
pub(crate) fn chat_completion_to_response_with_context(
    body: Value,
    tool_context: &CodexToolContext,
) -> Result<Value, ProxyError> {
    let choices = body
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ProxyError::TransformError("No choices in chat response".to_string()))?;
    let choice = choices
        .first()
        .ok_or_else(|| ProxyError::TransformError("Empty choices in chat response".to_string()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| ProxyError::TransformError("No message in chat choice".to_string()))?;

    let response_id = response_id_from_chat_id(body.get("id").and_then(|v| v.as_str()));
    let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let created_at = body.get("created").and_then(|v| v.as_u64()).unwrap_or(0);
    let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

    let reasoning = chat_reasoning_text(message);
    let mut output = Vec::new();
    if let Some(reasoning_item) =
        chat_reasoning_to_response_output_item(reasoning.as_deref(), &response_id)
    {
        output.push(reasoning_item);
    }
    if let Some(message_item) = chat_message_to_response_output_item(message, &response_id) {
        output.push(message_item);
    }
    output.extend(chat_tool_calls_to_response_output_items(
        message,
        reasoning.as_deref(),
        tool_context,
    ));

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": response_status_from_finish_reason(finish_reason),
        "model": model,
        "output": output,
        "usage": chat_usage_to_responses_usage(body.get("usage"))
    });

    if finish_reason == Some("length") {
        response["incomplete_details"] = json!({ "reason": "max_output_tokens" });
    }

    Ok(response)
}

fn chat_reasoning_to_response_output_item(
    reasoning: Option<&str>,
    response_id: &str,
) -> Option<Value> {
    let reasoning = reasoning?;
    if reasoning.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("rs_{response_id}"),
        "type": "reasoning",
        "summary": [{
            "type": "summary_text",
            "text": reasoning
        }]
    }))
}

fn chat_reasoning_text(message: &Value) -> Option<String> {
    if let Some(reasoning) = extract_reasoning_field_text(message) {
        return Some(reasoning);
    }

    if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
        if let Some((reasoning, _answer)) = split_leading_think_block(content) {
            if !reasoning.is_empty() {
                return Some(reasoning);
            }
        }
    }

    None
}

fn chat_message_to_response_output_item(message: &Value, response_id: &str) -> Option<Value> {
    let mut content = Vec::new();

    if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
        let text = split_leading_think_block(text)
            .map(|(_reasoning, answer)| answer)
            .unwrap_or_else(|| text.to_string());
        if !text.is_empty() {
            content.push(json!({
                "type": "output_text",
                "text": text,
                "annotations": []
            }));
        }
    } else if let Some(parts) = message.get("content").and_then(|v| v.as_array()) {
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match part_type {
                "text" | "output_text" => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(json!({
                                "type": "output_text",
                                "text": text,
                                "annotations": []
                            }));
                        }
                    }
                }
                "refusal" => {
                    if let Some(text) = part.get("refusal").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(json!({
                                "type": "refusal",
                                "refusal": text
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(refusal) = message.get("refusal").and_then(|v| v.as_str()) {
        if !refusal.is_empty() {
            content.push(json!({
                "type": "refusal",
                "refusal": refusal
            }));
        }
    }

    if content.is_empty() {
        return None;
    }

    Some(json!({
        "id": format!("{response_id}_msg"),
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": content
    }))
}

fn chat_tool_calls_to_response_output_items(
    message: &Value,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let mut output = Vec::new();

    if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            output.push(chat_tool_call_to_response_item(
                tool_call,
                index,
                reasoning,
                tool_context,
            ));
        }
    } else if let Some(function_call) = message.get("function_call") {
        output.push(chat_legacy_function_call_to_response_item(
            function_call,
            reasoning,
            tool_context,
        ));
    }

    output
}

fn chat_tool_call_to_response_item(
    tool_call: &Value,
    index: usize,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = tool_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_{index}"));
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = canonicalize_tool_arguments(function.get("arguments"));

    let item_id = response_tool_call_item_id_from_chat_name(&call_id, name, tool_context);
    response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        &call_id,
        name,
        &arguments,
        reasoning,
        tool_context,
    )
}

fn chat_legacy_function_call_to_response_item(
    function_call: &Value,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = function_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("call_0");
    let name = function_call
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = canonicalize_tool_arguments(function_call.get("arguments"));

    let item_id = response_tool_call_item_id_from_chat_name(call_id, name, tool_context);
    response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        call_id,
        name,
        &arguments,
        reasoning,
        tool_context,
    )
}

pub(crate) fn response_tool_call_item_id_from_chat_name(
    call_id: &str,
    chat_name: &str,
    tool_context: &CodexToolContext,
) -> String {
    if tool_context.is_custom_tool_chat_name(chat_name) {
        format!("ctc_{call_id}")
    } else {
        format!("fc_{call_id}")
    }
}

pub(crate) fn response_tool_call_item_from_chat_name(
    item_id: &str,
    status: &str,
    call_id: &str,
    chat_name: &str,
    arguments: &str,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    match tool_context.lookup_chat_name(chat_name) {
        Some(spec) if spec.kind == CodexToolKind::ToolSearch => {
            response_tool_search_call_item(call_id, status, arguments, reasoning)
        }
        Some(spec) if spec.kind == CodexToolKind::Custom => response_custom_tool_call_item(
            item_id, status, call_id, &spec.name, arguments, reasoning,
        ),
        Some(spec) => response_function_call_item_with_namespace(
            item_id,
            status,
            call_id,
            &spec.name,
            spec.namespace.as_deref(),
            arguments,
            reasoning,
        ),
        None => {
            response_function_call_item(item_id, status, call_id, chat_name, arguments, reasoning)
        }
    }
}

fn response_tool_search_call_item(
    call_id: &str,
    status: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let parsed_arguments = parse_tool_arguments_object(arguments);
    let mut item = json!({
        "type": "tool_search_call",
        "call_id": call_id,
        "status": status,
        "execution": "client",
        "arguments": parsed_arguments
    });
    super::codex_chat_common::attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

fn response_custom_tool_call_item(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let input = custom_tool_input_from_chat_arguments(arguments);
    let mut item = json!({
        "id": item_id,
        "type": "custom_tool_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "input": input
    });
    super::codex_chat_common::attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

fn parse_tool_arguments_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({ "query": arguments }))
}

pub(crate) fn custom_tool_input_from_chat_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(obj)) => obj
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(|value| value.as_str())
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}

pub(crate) fn chat_usage_to_responses_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object() && !value.is_null()) else {
        return json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        });
    };

    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens
    });

    if let Some(cached) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .and_then(|v| v.as_u64())
    {
        result["input_tokens_details"] = json!({ "cached_tokens": cached });
    }

    if let Some(details) = usage.get("completion_tokens_details") {
        result["output_tokens_details"] = details.clone();
    }

    if let Some(cache_read) = usage.get("cache_read_input_tokens") {
        result["cache_read_input_tokens"] = cache_read.clone();
    }
    if let Some(cache_creation) = usage.get("cache_creation_input_tokens") {
        result["cache_creation_input_tokens"] = cache_creation.clone();
    }

    result
}

pub(crate) fn response_id_from_chat_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("ccswitch");
    if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    }
}

pub(crate) fn response_status_from_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("length") => "incomplete",
        _ => "completed",
    }
}

/// 把 Chat Completions 上游的错误体规整成 OpenAI Responses API 风格的错误对象。
///
/// 兼容三类输入：
/// 1. 标准 OpenAI 形式 `{"error": {"message": "...", "type": "...", "code": ...}}`
/// 2. MiniMax 等非标形式（如 `{"base_resp": {"status_code": 2013, "status_msg": "..."}}`）
/// 3. 顶层只有 `message` / `detail` / 裸字符串的最小错误
///
/// 输出统一为 `{"error": {"message", "type", "code", "param"}}`，与 OpenAI Responses
/// API 错误响应一致；Codex 客户端的错误处理只识别这个形状。
pub fn chat_error_to_response_error(body: Option<&Value>) -> Value {
    let Some(value) = body else {
        return json!({
            "error": {
                "message": "Upstream returned an empty error response",
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    };

    if let Some(text) = value.as_str() {
        return json!({
            "error": {
                "message": text,
                "type": "upstream_error",
                "code": serde_json::Value::Null,
                "param": serde_json::Value::Null,
            }
        });
    }

    let source = value.get("error").unwrap_or(value);

    let message = source
        .get("message")
        .or_else(|| source.get("detail"))
        .or_else(|| source.get("status_msg"))
        .or_else(|| source.pointer("/base_resp/status_msg"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| source.as_str().map(ToString::to_string))
        .unwrap_or_else(|| {
            // 没法从字段提取出文本，就把整个 JSON 序列化回去，方便用户排查。
            serde_json::to_string(source).unwrap_or_else(|_| "Upstream error".to_string())
        });

    let error_type = source
        .get("type")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "upstream_error".to_string());

    let code = source
        .get("code")
        .cloned()
        .or_else(|| source.pointer("/base_resp/status_code").cloned())
        .unwrap_or(serde_json::Value::Null);

    let param = source
        .get("param")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
            "param": param,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_request_with_stream_injects_include_usage() {
        let input = json!({
            "model": "kimi-k2.6",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
            "stream": true
        });

        let result = responses_to_chat_completions(input).unwrap();

        assert_eq!(result["stream"], true);
        assert_eq!(result["stream_options"]["include_usage"], true);
    }

    #[test]
    fn responses_request_without_stream_omits_stream_options() {
        let input = json!({
            "model": "kimi-k2.6",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}]
        });

        let result = responses_to_chat_completions(input).unwrap();

        assert!(result.get("stream_options").is_none());
    }

    #[test]
    fn responses_request_merges_include_usage_into_existing_stream_options() {
        let input = json!({
            "model": "kimi-k2.6",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
            "stream": true,
            "stream_options": {"continuous_usage_stats": true}
        });

        let result = responses_to_chat_completions(input).unwrap();

        // 既补上 include_usage，又保留客户端原有的 stream_options 字段。
        assert_eq!(result["stream_options"]["include_usage"], true);
        assert_eq!(result["stream_options"]["continuous_usage_stats"], true);
    }

    #[test]
    fn responses_request_to_chat_maps_messages_tools_and_limits() {
        let input = json!({
            "model": "gpt-5.4",
            "instructions": "You are concise.",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "Weather?"},
                        {"type": "input_image", "image_url": "data:image/png;base64,abc"},
                        {"type": "input_text", "text": "Use Celsius."}
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{\"city\":\"Tokyo\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Sunny"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object"},
                "strict": true
            }],
            "tool_choice": {"type": "function", "name": "get_weather"},
            "max_output_tokens": 100,
            "reasoning": {"effort": "high"},
            "stream": true
        });

        let result = responses_to_chat_completions(input).unwrap();

        assert_eq!(result["model"], "gpt-5.4");
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(result["messages"][1]["role"], "user");
        assert_eq!(result["messages"][1]["content"][0]["type"], "text");
        assert_eq!(result["messages"][1]["content"][1]["type"], "image_url");
        assert_eq!(result["messages"][1]["content"][2]["type"], "text");
        assert_eq!(result["messages"][1]["content"][2]["text"], "Use Celsius.");
        assert_eq!(result["messages"][2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(result["messages"][3]["role"], "tool");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(result["tools"][0]["function"]["strict"], true);
        assert_eq!(result["tool_choice"]["function"]["name"], "get_weather");
        assert_eq!(result["max_tokens"], 100);
        assert_eq!(result["reasoning_effort"], "high");
    }

    #[test]
    fn responses_request_to_chat_exposes_tool_search_and_loaded_namespace_tools() {
        let input = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search"}],
            "input": [
                {
                    "type": "tool_search_call",
                    "call_id": "call_tool_search_1",
                    "status": "completed",
                    "execution": "client",
                    "arguments": {"query": "Gmail search emails", "limit": 5}
                },
                {
                    "type": "tool_search_output",
                    "call_id": "call_tool_search_1",
                    "status": "completed",
                    "execution": "client",
                    "tools": [{
                        "type": "namespace",
                        "name": "mcp__codex_apps__gmail",
                        "description": "Find and reference emails from your inbox.",
                        "tools": [{
                            "type": "function",
                            "name": "_search_emails",
                            "description": "Search Gmail for emails matching a query.",
                            "strict": false,
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "query": {"type": "string"},
                                    "max_results": {"type": "integer"}
                                },
                                "required": ["query"]
                            }
                        }]
                    }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": "Search unread inbox mail."
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let tools = result["tools"].as_array().unwrap();
        let tool_names = tools
            .iter()
            .filter_map(|tool| tool.pointer("/function/name").and_then(|v| v.as_str()))
            .collect::<Vec<_>>();

        assert!(tool_names.contains(&"tool_search"));
        assert!(tool_names.contains(&"mcp__codex_apps__gmail___search_emails"));
        assert_eq!(
            result["messages"][0]["tool_calls"][0]["function"]["name"],
            "tool_search"
        );
        assert_eq!(result["messages"][1]["role"], "tool");
        assert_eq!(result["messages"][1]["tool_call_id"], "call_tool_search_1");
        assert!(result["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("mcp__codex_apps__gmail"));
    }

    #[test]
    fn responses_request_to_chat_maps_custom_tool_and_choice() {
        let input = json!({
            "model": "gpt-5.4",
            "tools": [{
                "type": "custom",
                "name": "apply_patch",
                "description": "Apply a patch to files."
            }],
            "tool_choice": {"type": "custom", "name": "apply_patch"},
            "input": [{
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_patch",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** End Patch"
            }]
        });

        let result = responses_to_chat_completions(input).unwrap();

        assert_eq!(result["tools"][0]["function"]["name"], "apply_patch");
        assert_eq!(
            result["tools"][0]["function"]["parameters"]["required"][0],
            "input"
        );
        assert_eq!(result["tool_choice"]["function"]["name"], "apply_patch");
        assert_eq!(
            result["messages"][0]["tool_calls"][0]["function"]["arguments"],
            r#"{"input":"*** Begin Patch\n*** End Patch"}"#
        );
    }

    #[test]
    fn responses_request_to_chat_uses_provider_reasoning_effort_for_deepseek_model() {
        let input = json!({
            "model": "deepseek-v4-pro",
            "input": "hello",
            "reasoning": {"effort": "xhigh"}
        });
        let config = CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("deepseek".to_string()),
            output_format: Some("reasoning_content".to_string()),
        };

        let result = responses_to_chat_completions_with_reasoning(input, Some(&config)).unwrap();

        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["reasoning_effort"], "max");
    }

    #[test]
    fn responses_request_to_chat_maps_openrouter_to_native_reasoning_object() {
        // OpenRouter 平台形态：原生 reasoning:{effort} 对象 + "openrouter" 值映射
        // （与 infer_aggregator_platform_config 推断出的配置保持一致）。
        let config = CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning.effort".to_string()),
            effort_value_mode: Some("openrouter".to_string()),
            output_format: Some("auto".to_string()),
        };

        // max 不在 OpenRouter 枚举内（见 openclaw#77350），必须钳成 xhigh，
        // 且写进原生 reasoning 对象，而非顶层 reasoning_effort 别名。
        let input = json!({
            "model": "deepseek/deepseek-chat-v3.1",
            "input": "hello",
            "reasoning": {"effort": "max"}
        });
        let result = responses_to_chat_completions_with_reasoning(input, Some(&config)).unwrap();

        assert_eq!(result["reasoning"]["effort"], "xhigh");
        assert!(result.get("reasoning_effort").is_none());
        // thinking_param=none：即使 supports_effort 把 supports_thinking 带成 true，
        // 也不写任何 thinking 字段（OpenRouter 不认 thinking:{type}）。
        assert!(result.get("thinking").is_none());

        // 合法档位原样透传。
        let input_high = json!({
            "model": "deepseek/deepseek-chat-v3.1",
            "input": "hello",
            "reasoning": {"effort": "high"}
        });
        let result_high =
            responses_to_chat_completions_with_reasoning(input_high, Some(&config)).unwrap();
        assert_eq!(result_high["reasoning"]["effort"], "high");
        assert!(result_high.get("reasoning_effort").is_none());
    }

    #[test]
    fn responses_request_to_chat_passes_explicit_none_through_for_openrouter() {
        // OpenRouter 原生 reasoning 对象支持显式关闭：effort=none 应忠实转发为
        // {"reasoning":{"effort":"none"}}，而非被吞掉——否则默认开思考的模型无法关闭，
        // 带来行为与成本偏差。
        let config = CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning.effort".to_string()),
            effort_value_mode: Some("openrouter".to_string()),
            output_format: Some("auto".to_string()),
        };

        let input = json!({
            "model": "openai/gpt-5",
            "input": "hello",
            "reasoning": {"effort": "none"}
        });
        let result = responses_to_chat_completions_with_reasoning(input, Some(&config)).unwrap();

        assert_eq!(result["reasoning"]["effort"], "none");
        // none 不是 OpenAI 顶层 reasoning_effort 的合法枚举，不写顶层别名；也不写 thinking。
        assert!(result.get("reasoning_effort").is_none());
        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn responses_request_to_chat_drops_explicit_none_for_top_level_effort_provider() {
        // 对照：顶层 reasoning_effort 平台（DeepSeek/OpenAI 风格）的 effort 枚举不含 none，
        // 显式 none 不应透传成 reasoning_effort:"none"（会被上游拒），仅走 thinking 关闭路径。
        // 锁定「none 透传仅限 reasoning.effort 形态」的边界，防止回归。
        let config = CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("deepseek".to_string()),
            output_format: Some("reasoning_content".to_string()),
        };

        let input = json!({
            "model": "deepseek-v4-pro",
            "input": "hello",
            "reasoning": {"effort": "none"}
        });
        let result = responses_to_chat_completions_with_reasoning(input, Some(&config)).unwrap();

        // thinking 关闭信号照发；但不写 reasoning_effort，也不写原生 reasoning 对象。
        assert_eq!(result["thinking"]["type"], "disabled");
        assert!(result.get("reasoning_effort").is_none());
        assert!(result.get("reasoning").is_none());
    }

    #[test]
    fn responses_request_to_chat_maps_thinking_only_provider_without_effort() {
        let input = json!({
            "model": "kimi-k2.6",
            "input": "hello",
            "reasoning": {"effort": "high"}
        });
        let config = CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        };

        let result = responses_to_chat_completions_with_reasoning(input, Some(&config)).unwrap();

        assert_eq!(result["thinking"]["type"], "enabled");
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn responses_request_to_chat_maps_enable_thinking_provider() {
        let input = json!({
            "model": "qwen3-max",
            "input": "hello",
            "reasoning": {"effort": "medium"}
        });
        let config = CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            output_format: Some("reasoning_content".to_string()),
        };

        let result = responses_to_chat_completions_with_reasoning(input, Some(&config)).unwrap();

        assert_eq!(result["enable_thinking"], true);
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn chat_response_to_responses_extracts_reasoning_details() {
        let input = json!({
            "id": "chatcmpl_minimax",
            "object": "chat.completion",
            "created": 123,
            "model": "MiniMax-M2.7",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_details": [
                        {"type": "reasoning_text", "text": "Need to inspect the code."}
                    ],
                    "content": "Done"
                },
                "finish_reason": "stop"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["output"][0]["type"], "reasoning");
        assert_eq!(
            result["output"][0]["summary"][0]["text"],
            "Need to inspect the code."
        );
        assert_eq!(result["output"][1]["content"][0]["text"], "Done");
    }

    #[test]
    fn responses_request_to_chat_normalizes_codex_internal_roles() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [
                        {"type": "input_text", "text": "Follow project instructions."}
                    ]
                },
                {
                    "type": "message",
                    "role": "latest_reminder",
                    "content": "Keep the reply brief."
                },
                {
                    "type": "message",
                    "role": "unknown_codex_role",
                    "content": "Fallback content."
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "Follow project instructions.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Keep the reply brief.");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"], "Fallback content.");
    }

    #[test]
    fn responses_request_to_chat_merges_mid_stream_system_into_head() {
        let input = json!({
            "model": "MiniMax-M2.7",
            "instructions": "You are Codex.",
            "input": [
                {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Permissions block"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "AGENTS.md"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]},
                {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Collaboration Mode: Default"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "你好"}]}
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        for (idx, msg) in messages.iter().enumerate() {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap();
            if idx == 0 {
                assert_eq!(role, "system", "first message must be system");
            } else {
                assert_ne!(
                    role, "system",
                    "no system role allowed past index 0 (got at {idx})"
                );
            }
        }

        let head_content = messages[0]["content"].as_str().unwrap();
        assert!(head_content.contains("You are Codex."));
        assert!(head_content.contains("Permissions block"));
        assert!(head_content.contains("Collaboration Mode: Default"));
    }

    #[test]
    fn collapse_system_messages_preserves_non_system_order() {
        let input = vec![
            json!({"role": "system", "content": "S1"}),
            json!({"role": "user", "content": "U1"}),
            json!({"role": "assistant", "content": "A1"}),
            json!({"role": "system", "content": "S2"}),
            json!({"role": "user", "content": "U2"}),
        ];
        let out = collapse_system_messages_to_head(input);

        assert_eq!(out.len(), 4);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[0]["content"], "S1\n\nS2");
        assert_eq!(out[1]["content"], "U1");
        assert_eq!(out[2]["content"], "A1");
        assert_eq!(out[3]["content"], "U2");
    }

    #[test]
    fn responses_request_to_chat_passes_reasoning_content_back_to_assistant_message() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "reasoning",
                    "summary": [
                        {"type": "summary_text", "text": "Need to inspect the repo."}
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "I will check the files."}
                    ]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": "Continue"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"], "I will check the files.");
        assert_eq!(
            messages[0]["reasoning_content"],
            "Need to inspect the repo."
        );
        assert_eq!(messages[1]["role"], "user");
        assert!(messages[1].get("reasoning_content").is_none());
    }

    #[test]
    fn responses_request_to_chat_attaches_trailing_reasoning_to_previous_assistant() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": "I checked the files."
                },
                {
                    "type": "reasoning",
                    "summary": [
                        {"type": "summary_text", "text": "The answer came from README."}
                    ]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": "Continue"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"], "I checked the files.");
        assert_eq!(
            messages[0]["reasoning_content"],
            "The answer came from README."
        );
        assert_eq!(messages[1]["role"], "user");
        assert!(messages[1].get("reasoning_content").is_none());
    }

    #[test]
    fn responses_request_to_chat_keeps_embedded_assistant_reasoning() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "assistant",
                    "reasoning_content": "I need to preserve thinking history.",
                    "content": "Done."
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"], "Done.");
        assert_eq!(
            messages[0]["reasoning_content"],
            "I need to preserve thinking history."
        );
    }

    #[test]
    fn responses_request_to_chat_attaches_reasoning_to_tool_call_message() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "reasoning",
                    "summary": "Need to read a file."
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"README.md\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Readme content"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["reasoning_content"], "Need to read a file.");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[1]["role"], "tool");
    }

    #[test]
    fn responses_request_to_chat_recovers_reasoning_from_function_call_item() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"README.md\"}",
                    "reasoning_content": "Need to read a file."
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Readme content"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["reasoning_content"], "Need to read a file.");
        assert_eq!(messages[1]["role"], "tool");
    }

    #[test]
    fn responses_request_to_chat_injects_placeholder_reasoning_for_bare_tool_call() {
        // 历史恢复 miss 时，带 tool_calls 的 assistant 消息没有任何可用 reasoning，
        // 必须补占位，否则 kimi/Moonshot thinking 模型会拒绝整个请求。
        let input = json!({
            "model": "kimi-k2-thinking",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"README.md\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Readme content"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["reasoning_content"], "tool call");
        assert_eq!(messages[1]["role"], "tool");
    }

    #[test]
    fn responses_request_to_chat_attaches_trailing_reasoning_to_tool_call_message() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"README.md\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Readme content"
                },
                {
                    "type": "reasoning",
                    "summary": "Need to read a file."
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["reasoning_content"], "Need to read a file.");
        assert_eq!(messages[1]["role"], "tool");
    }

    #[test]
    fn responses_request_to_chat_keeps_multiple_tool_calls_adjacent_to_outputs() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"README.md\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "list_files",
                    "arguments": "{\"path\":\"src\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "Readme content"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": ["main.rs", "lib.rs"]
                },
                {
                    "role": "user",
                    "content": "Continue"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["tool_calls"][1]["id"], "call_2");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_2");
        assert_eq!(messages[2]["content"], "[\"main.rs\",\"lib.rs\"]");
        assert_eq!(messages[3]["role"], "user");
    }

    #[test]
    fn responses_request_to_chat_canonicalizes_json_string_tool_payloads() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "lookup",
                    "arguments": "{ \"b\": 2, \"a\": 1 }"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "{ \"z\": true, \"a\": [2, 1] }"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(
            messages[0]["tool_calls"][0]["function"]["arguments"],
            r#"{"a":1,"b":2}"#
        );
        assert_eq!(messages[1]["content"], r#"{"a":[2,1],"z":true}"#);
    }

    #[test]
    fn responses_request_to_chat_preserves_plain_text_tool_output() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "not json"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "plain text result"
                }
            ]
        });

        let result = responses_to_chat_completions(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(
            messages[0]["tool_calls"][0]["function"]["arguments"],
            "not json"
        );
        assert_eq!(messages[1]["content"], "plain text result");
    }

    #[test]
    fn chat_response_to_responses_maps_text_tool_calls_and_usage() {
        let input = json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "created": 123,
            "model": "gpt-5.4",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "I should check the weather before answering.",
                    "content": "Let me check.",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Tokyo\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "prompt_tokens_details": {"cached_tokens": 3}
            }
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["id"], "resp_chatcmpl_1");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["output"][0]["type"], "reasoning");
        assert_eq!(
            result["output"][0]["summary"][0]["text"],
            "I should check the weather before answering."
        );
        assert_eq!(result["output"][1]["type"], "message");
        assert_eq!(result["output"][1]["content"][0]["text"], "Let me check.");
        assert_eq!(result["output"][2]["type"], "function_call");
        assert_eq!(result["output"][2]["call_id"], "call_1");
        assert_eq!(
            result["output"][2]["reasoning_content"],
            "I should check the weather before answering."
        );
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
        assert_eq!(result["usage"]["input_tokens_details"]["cached_tokens"], 3);
    }

    #[test]
    fn chat_response_to_responses_restores_loaded_namespace_tool_call() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "tool_search_output",
                "call_id": "call_tool_search_1",
                "status": "completed",
                "execution": "client",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__codex_apps__gmail",
                    "description": "Find and reference emails from your inbox.",
                    "tools": [{
                        "type": "function",
                        "name": "_search_emails",
                        "description": "Search Gmail for emails matching a query.",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "query": {"type": "string"},
                                "label_ids": {"type": "array", "items": {"type": "string"}},
                                "max_results": {"type": "integer"}
                            }
                        }
                    }]
                }]
            }]
        });
        let context = build_codex_tool_context_from_request(&request);
        let chat = json!({
            "id": "chatcmpl_gmail",
            "object": "chat.completion",
            "created": 123,
            "model": "gpt-5.4",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_gmail",
                        "type": "function",
                        "function": {
                            "name": "mcp__codex_apps__gmail___search_emails",
                            "arguments": "{\"query\":\"-in:spam -in:trash\",\"label_ids\":[\"UNREAD\"],\"max_results\":5}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_completion_to_response_with_context(chat, &context).unwrap();

        assert_eq!(result["output"][0]["type"], "function_call");
        assert_eq!(result["output"][0]["call_id"], "call_gmail");
        assert_eq!(result["output"][0]["namespace"], "mcp__codex_apps__gmail");
        assert_eq!(result["output"][0]["name"], "_search_emails");
        assert_eq!(
            result["output"][0]["arguments"],
            r#"{"label_ids":["UNREAD"],"max_results":5,"query":"-in:spam -in:trash"}"#
        );
    }

    #[test]
    fn chat_response_to_responses_restores_tool_search_call() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "tool_search"}],
            "input": "Find tools."
        });
        let context = build_codex_tool_context_from_request(&request);
        let chat = json!({
            "id": "chatcmpl_tool_search",
            "object": "chat.completion",
            "created": 123,
            "model": "gpt-5.4",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_tool_search_1",
                        "type": "function",
                        "function": {
                            "name": "tool_search",
                            "arguments": "{\"query\":\"Gmail search emails\",\"limit\":10}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_completion_to_response_with_context(chat, &context).unwrap();

        assert_eq!(result["output"][0]["type"], "tool_search_call");
        assert_eq!(result["output"][0]["call_id"], "call_tool_search_1");
        assert_eq!(result["output"][0]["execution"], "client");
        assert_eq!(
            result["output"][0]["arguments"]["query"],
            "Gmail search emails"
        );
        assert_eq!(result["output"][0]["arguments"]["limit"], 10);
    }

    #[test]
    fn chat_response_to_responses_restores_custom_tool_call() {
        let request = json!({
            "model": "gpt-5.4",
            "tools": [{"type": "custom", "name": "apply_patch"}],
            "input": "Patch it."
        });
        let context = build_codex_tool_context_from_request(&request);
        let chat = json!({
            "id": "chatcmpl_custom",
            "object": "chat.completion",
            "created": 123,
            "model": "gpt-5.4",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "apply_patch",
                            "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_completion_to_response_with_context(chat, &context).unwrap();

        assert_eq!(result["output"][0]["type"], "custom_tool_call");
        assert_eq!(result["output"][0]["id"], "ctc_call_patch");
        assert_eq!(result["output"][0]["call_id"], "call_patch");
        assert_eq!(result["output"][0]["name"], "apply_patch");
        assert_eq!(
            result["output"][0]["input"],
            "*** Begin Patch\n*** End Patch"
        );
    }

    #[test]
    fn chat_response_to_responses_canonicalizes_json_string_tool_arguments() {
        let input = json!({
            "id": "chatcmpl_args",
            "object": "chat.completion",
            "created": 123,
            "model": "gpt-5.4",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{ \"b\": 2, \"a\": 1 }"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["output"][0]["type"], "function_call");
        assert_eq!(result["output"][0]["arguments"], r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn chat_response_to_responses_splits_inline_think_content() {
        let input = json!({
            "id": "chatcmpl_think",
            "object": "chat.completion",
            "created": 123,
            "model": "MiniMax-M2.7",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "<think>\nI should answer with pong.\n</think>\n\npong"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30,
                "completion_tokens_details": {"reasoning_tokens": 18}
            }
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["output"][0]["type"], "reasoning");
        assert_eq!(
            result["output"][0]["summary"][0]["text"],
            "I should answer with pong."
        );
        assert_eq!(result["output"][1]["type"], "message");
        assert_eq!(result["output"][1]["content"][0]["text"], "pong");
        assert_eq!(
            result["usage"]["output_tokens_details"]["reasoning_tokens"],
            18
        );
    }

    #[test]
    fn chat_response_length_maps_to_incomplete_response() {
        let input = json!({
            "id": "chatcmpl_2",
            "model": "gpt-5.4",
            "choices": [{
                "message": {"role": "assistant", "content": "partial"},
                "finish_reason": "length"
            }]
        });

        let result = chat_completion_to_response(input).unwrap();

        assert_eq!(result["status"], "incomplete");
        assert_eq!(result["incomplete_details"]["reason"], "max_output_tokens");
    }

    #[test]
    fn chat_error_to_response_error_normalizes_standard_openai_shape() {
        let input = json!({
            "error": {
                "message": "Invalid API key",
                "type": "invalid_request_error",
                "code": "invalid_api_key",
                "param": "api_key"
            }
        });

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(result["error"]["message"], "Invalid API key");
        assert_eq!(result["error"]["type"], "invalid_request_error");
        assert_eq!(result["error"]["code"], "invalid_api_key");
        assert_eq!(result["error"]["param"], "api_key");
    }

    #[test]
    fn chat_error_to_response_error_normalizes_minimax_base_resp() {
        // MiniMax 把错误塞在 base_resp 里，code 是数字而不是字符串
        let input = json!({
            "base_resp": {
                "status_code": 2013,
                "status_msg": "invalid params, chat content has invalid message role: system"
            }
        });

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(
            result["error"]["message"],
            "invalid params, chat content has invalid message role: system"
        );
        assert_eq!(result["error"]["code"], 2013);
        // type 没有显式给出，应该回落到 upstream_error
        assert_eq!(result["error"]["type"], "upstream_error");
    }

    #[test]
    fn chat_error_to_response_error_handles_plain_text_body() {
        let input = json!("Upstream timeout");

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(result["error"]["message"], "Upstream timeout");
        assert_eq!(result["error"]["type"], "upstream_error");
        assert!(result["error"]["code"].is_null());
        assert!(result["error"]["param"].is_null());
    }

    #[test]
    fn chat_error_to_response_error_handles_missing_body() {
        let result = chat_error_to_response_error(None);

        assert_eq!(
            result["error"]["message"],
            "Upstream returned an empty error response"
        );
        assert_eq!(result["error"]["type"], "upstream_error");
    }

    #[test]
    fn chat_error_to_response_error_falls_back_to_detail_field() {
        // 部分中转把错误塞在顶层 detail 字段（OpenAI 兼容层常见）
        let input = json!({
            "detail": "rate limit exceeded"
        });

        let result = chat_error_to_response_error(Some(&input));

        assert_eq!(result["error"]["message"], "rate limit exceeded");
        assert_eq!(result["error"]["type"], "upstream_error");
    }
}
