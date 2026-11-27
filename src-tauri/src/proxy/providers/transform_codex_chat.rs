//! Codex Responses ↔ OpenAI Chat Completions conversion.
//!
//! This module is used when the Codex client talks to CC Switch through the
//! Responses API, while the selected upstream provider only exposes an
//! OpenAI-compatible Chat Completions endpoint.

use super::codex_chat_common::{
    append_reasoning_content, chat_thinking_part_text, copy_tool_call_extra_content,
    extract_reasoning_field_text, extract_reasoning_summary_text, response_function_call_item,
    response_function_call_item_with_namespace,
};
use super::codex_compaction;
use super::inline_think::split_leading_think_block;
use super::transform::unwrap_chat_completion_envelope;
use crate::provider::CodexChatReasoningConfig;
use crate::proxy::{
    error::ProxyError,
    json_canonical::{
        canonical_json_string, canonicalize_json_string_if_parseable, canonicalize_tool_arguments,
        short_sha256_hex,
    },
    tool_media::{
        chat_file_from_input_file, flush_pending_chat_tool_media, plan_chat_tool_output_media,
        plan_chat_tool_output_media_with_marker, queue_chat_tool_output_media,
        strip_and_clamp_media_from_tool_value, ToolMediaScope, TOOL_RESULT_MEDIA_ATTACHED_MARKER,
        TOOL_RESULT_MEDIA_MOVED_MARKER,
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
const ORPHAN_TOOL_OUTPUT_MARKER: &str = "[tool_result without a matching tool_call]";
const CHAT_TOOL_NAME_MAX_LEN: usize = 64;
const CUSTOM_TOOL_INPUT_DESCRIPTION: &str = "Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description.";
const CUSTOM_TOOL_PRESERVED_METADATA_HEADING: &str = "Original tool definition:";
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
    identity_to_chat_name: HashMap<(Option<String>, String), String>,
    /// 这个请求是 Codex 远程压缩（input 里有 `compaction_trigger`），见 `codex_compaction`。
    compaction_request: bool,
}

enum PendingToolPayload {
    Function { tool: Value },
    Custom { tool: Value },
    ToolSearch,
}

struct PendingCodexTool {
    namespace: Option<String>,
    name: String,
    spec_kind: CodexToolKind,
    preferred_chat_name: String,
    payload: PendingToolPayload,
}

fn normalize_tool_namespace(namespace: Option<&str>) -> Option<String> {
    namespace
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn canonical_tool_identity_bytes(namespace: Option<&str>, name: &str) -> Vec<u8> {
    match namespace {
        Some(ns) => format!("{}:{ns}:{name}", ns.len()).into_bytes(),
        None => format!("none::{name}").into_bytes(),
    }
}

fn is_valid_chat_tool_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn sanitize_chat_tool_prefix(raw: &str, max_len: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_len));
    for ch in raw.chars() {
        if out.len() >= max_len {
            break;
        }
        if is_valid_chat_tool_char(ch) {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() && max_len > 0 {
        out.push('t');
    }
    out
}

fn disambiguated_chat_tool_name(namespace: Option<&str>, name: &str, attempt: usize) -> String {
    let raw_candidate = match namespace {
        Some(ns) => format!("{ns}__{name}"),
        None => name.to_string(),
    };
    let hash = short_sha256_hex(&canonical_tool_identity_bytes(namespace, name));
    let suffix = if attempt == 0 {
        format!("__{hash}")
    } else {
        format!("__{hash}_{attempt}")
    };
    let prefix_len = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(suffix.len());
    let prefix = sanitize_chat_tool_prefix(&raw_candidate, prefix_len);
    format!("{prefix}{suffix}")
}

fn build_custom_chat_tool(tool: &Value, chat_name: &str) -> Value {
    let description = json!(responses_custom_tool_description(tool));
    json!({
        "type": "function",
        "function": {
            "name": chat_name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": {
                    CUSTOM_TOOL_INPUT_FIELD: {
                        "type": "string",
                        "description": CUSTOM_TOOL_INPUT_DESCRIPTION
                    }
                },
                "required": [CUSTOM_TOOL_INPUT_FIELD]
            }
        }
    })
}

fn build_tool_search_chat_tool(chat_name: &str) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": chat_name,
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
    })
}

fn collect_response_tool(
    tool: &Value,
    seen_identities: &mut HashSet<(Option<String>, String)>,
    out: &mut Vec<PendingCodexTool>,
) {
    match tool {
        Value::String(name) => {
            collect_response_tool(
                &json!({
                    "type": "custom",
                    "name": name
                }),
                seen_identities,
                out,
            );
        }
        Value::Object(_) => match tool.get("type").and_then(|v| v.as_str()) {
            Some("function") => {
                let Some(name) = responses_tool_name(tool) else {
                    return;
                };
                if !seen_identities.insert((None, name.clone())) {
                    return;
                }
                out.push(PendingCodexTool {
                    namespace: None,
                    preferred_chat_name: name.clone(),
                    name,
                    spec_kind: CodexToolKind::Function,
                    payload: PendingToolPayload::Function { tool: tool.clone() },
                });
            }
            Some("custom") => {
                let Some(name) = responses_tool_name(tool) else {
                    return;
                };
                if !seen_identities.insert((None, name.clone())) {
                    return;
                }
                out.push(PendingCodexTool {
                    namespace: None,
                    preferred_chat_name: name.clone(),
                    name,
                    spec_kind: CodexToolKind::Custom,
                    payload: PendingToolPayload::Custom { tool: tool.clone() },
                });
            }
            Some("tool_search") => {
                let name = TOOL_SEARCH_PROXY_NAME.to_string();
                if !seen_identities.insert((None, name.clone())) {
                    return;
                }
                out.push(PendingCodexTool {
                    namespace: None,
                    preferred_chat_name: name.clone(),
                    name,
                    spec_kind: CodexToolKind::ToolSearch,
                    payload: PendingToolPayload::ToolSearch,
                });
            }
            Some("namespace") => {
                let Some(namespace) =
                    normalize_tool_namespace(tool.get("name").and_then(|v| v.as_str()))
                else {
                    return;
                };
                let Some(children) = tool
                    .get("tools")
                    .or_else(|| tool.get("children"))
                    .and_then(|v| v.as_array())
                else {
                    return;
                };

                for child in children {
                    if child.get("type").and_then(|v| v.as_str()) != Some("function") {
                        continue;
                    }
                    let Some(name) = responses_tool_name(child) else {
                        continue;
                    };
                    let ns = Some(namespace.clone());
                    if !seen_identities.insert((ns.clone(), name.clone())) {
                        continue;
                    }
                    let preferred_chat_name = flatten_namespace_tool_name(&namespace, &name);
                    out.push(PendingCodexTool {
                        namespace: ns,
                        name,
                        spec_kind: CodexToolKind::Namespace,
                        preferred_chat_name,
                        payload: PendingToolPayload::Function {
                            tool: child.clone(),
                        },
                    });
                }
            }
            _ => {}
        },
        _ => {}
    }
}

impl CodexToolContext {
    pub(crate) fn chat_tools(&self) -> &[Value] {
        &self.chat_tools
    }

    pub(crate) fn is_compaction_request(&self) -> bool {
        self.compaction_request
    }

    pub(crate) fn lookup_chat_name(&self, chat_name: &str) -> Option<&CodexToolSpec> {
        self.chat_name_to_spec.get(chat_name)
    }

    pub(crate) fn is_custom_tool_chat_name(&self, chat_name: &str) -> bool {
        self.lookup_chat_name(chat_name)
            .is_some_and(|spec| matches!(&spec.kind, CodexToolKind::Custom))
    }

    pub(crate) fn chat_name_for_response_function(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> String {
        let normalized_ns = normalize_tool_namespace(namespace);
        let trimmed_name = name.trim();
        if !trimmed_name.is_empty() {
            if let Some(chat_name) = self
                .identity_to_chat_name
                .get(&(normalized_ns.clone(), trimmed_name.to_string()))
            {
                return chat_name.clone();
            }
        }

        if let Some(ns) = normalized_ns.as_deref() {
            return flatten_namespace_tool_name(ns, trimmed_name);
        }

        if trimmed_name.len() > CHAT_TOOL_NAME_MAX_LEN {
            return disambiguated_chat_tool_name(None, trimmed_name, 0);
        }

        name.to_string()
    }

    fn register_pending_tools(&mut self, pending: Vec<PendingCodexTool>) {
        let mut preferred_counts: HashMap<String, usize> = HashMap::new();
        for item in &pending {
            *preferred_counts
                .entry(item.preferred_chat_name.clone())
                .or_insert(0) += 1;
        }

        let mut assigned_names: Vec<Option<String>> = vec![None; pending.len()];

        // First reserve preferred names that fit within the 64-char limit and
        // are unambiguous (including legacy flattened long namespace names when
        // non-colliding, and literal top-level names on top-vs-namespace clashes).
        for (idx, item) in pending.iter().enumerate() {
            let count = preferred_counts
                .get(&item.preferred_chat_name)
                .copied()
                .unwrap_or(0);
            let keeps_preferred = !item.preferred_chat_name.trim().is_empty()
                && item.preferred_chat_name.len() <= CHAT_TOOL_NAME_MAX_LEN
                && (count == 1 || item.namespace.is_none())
                && !self.seen_chat_names.contains(&item.preferred_chat_name);
            if keeps_preferred {
                self.seen_chat_names
                    .insert(item.preferred_chat_name.clone());
                assigned_names[idx] = Some(item.preferred_chat_name.clone());
            }
        }

        // Deterministically disambiguate any colliding or >64-char tool names using
        // a length-prefixed canonical `(namespace, name)` SHA-256 suffix.
        for (idx, item) in pending.iter().enumerate() {
            if assigned_names[idx].is_some() {
                continue;
            }
            let mut attempt = 0usize;
            let chat_name = loop {
                let candidate =
                    disambiguated_chat_tool_name(item.namespace.as_deref(), &item.name, attempt);
                if self.seen_chat_names.insert(candidate.clone()) {
                    break candidate;
                }
                attempt += 1;
            };
            assigned_names[idx] = Some(chat_name);
        }

        for (item, maybe_chat_name) in pending.into_iter().zip(assigned_names) {
            let Some(chat_name) = maybe_chat_name else {
                continue;
            };
            let chat_tool = match &item.payload {
                PendingToolPayload::Function { tool } => {
                    let Some(chat_tool) = responses_function_tool_to_chat_tool(tool, &chat_name)
                    else {
                        continue;
                    };
                    chat_tool
                }
                PendingToolPayload::Custom { tool } => build_custom_chat_tool(tool, &chat_name),
                PendingToolPayload::ToolSearch => build_tool_search_chat_tool(&chat_name),
            };
            let identity = (item.namespace.clone(), item.name.clone());
            let spec = CodexToolSpec {
                kind: item.spec_kind,
                name: item.name,
                namespace: item.namespace,
            };
            self.identity_to_chat_name
                .insert(identity, chat_name.clone());
            self.chat_name_to_spec.insert(chat_name, spec);
            self.chat_tools.push(chat_tool);
        }
    }
}

pub(crate) fn build_codex_tool_context_from_request(body: &Value) -> CodexToolContext {
    let mut context = CodexToolContext::default();
    let mut seen_identities: HashSet<(Option<String>, String)> = HashSet::new();
    let mut pending: Vec<PendingCodexTool> = Vec::new();

    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        for tool in tools {
            collect_response_tool(tool, &mut seen_identities, &mut pending);
        }
    }

    if let Some(input) = body.get("input") {
        collect_input_declared_tools(input, &mut seen_identities, &mut pending);
    }

    context.register_pending_tools(pending);
    context.compaction_request = codex_compaction::is_compaction_request(body);

    context
}

/// Convert an OpenAI Responses request into an OpenAI Chat Completions request,
/// using provider-declared Codex Chat reasoning capabilities when available.
///
/// `preserve_reasoning_content` mirrors the Claude → Chat path
/// (`should_preserve_reasoning_content_for_openai_chat`): `reasoning_content`
/// is a DeepSeek / Kimi compatibility field that third-party gateways expect on
/// assistant history, and strict OpenAI-compatible upstreams such as Mistral
/// reject it with HTTP 422 (`extra_forbidden`). Callers pass `false` for those
/// upstreams so the field never reaches them.
pub fn responses_to_chat_completions_with_reasoning(
    body: Value,
    reasoning_config: Option<&CodexChatReasoningConfig>,
    preserve_reasoning_content: bool,
) -> Result<Value, ProxyError> {
    let mut result = json!({});
    let tool_context = build_codex_tool_context_from_request(&body);
    let model = body.get("model").and_then(Value::as_str).unwrap_or("");

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
    let recovered =
        super::codex_chat_tool_history::preserve_invalid_tool_calls_as_text(&mut messages);
    if recovered > 0 {
        // Counts only: historical arguments and tool results may contain secrets.
        log::warn!("[Codex Chat] Preserved {recovered} malformed historical tool call(s) as text");
    }
    // DeepSeek strictly validates tool-result adjacency. Limit this replay
    // repair to explicit DeepSeek model IDs: other providers may interpret
    // assistant turns differently, and rewriting their history changes caches.
    if is_deepseek_chat_model(model) {
        coalesce_replayed_tool_turn_messages(&mut messages);
    }
    backfill_tool_call_reasoning_placeholders(&mut messages);
    if !preserve_reasoning_content {
        let stripped = strip_assistant_reasoning_content(&mut messages);
        if stripped > 0 {
            log::debug!(
                "[Codex Chat] Dropped reasoning_content from {stripped} assistant message(s)"
            );
        }
    }
    let mut messages = collapse_system_messages_to_head(messages);
    // 放在折叠之后：折叠按「后面是否紧接 user」决定中途 developer 指令的位置，
    // 提前删掉空 user 会改变非空指令的角色和位置。
    let dropped = drop_empty_user_messages(&mut messages);
    if dropped > 0 {
        log::debug!("[Codex Chat] Dropped {dropped} empty user message(s)");
    }
    result["messages"] = json!(messages);

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

    // 压缩回合只要一段摘要：不带工具和结构化输出，与 Codex 本地压缩请求同形。
    let compaction = tool_context.is_compaction_request();
    let tools = tool_context.chat_tools();
    if !tools.is_empty() && !compaction {
        result["tools"] = json!(tools);
    }

    if let Some(tool_choice) = body.get("tool_choice") {
        result["tool_choice"] = responses_tool_choice_to_chat(tool_choice, &tool_context);
    }

    for key in EXTRA_CHAT_PASSTHROUGH_FIELDS {
        if compaction && *key == "response_format" {
            continue;
        }
        if let Some(value) = body.get(*key) {
            result[*key] = value.clone();
        }
    }
    drop_responses_only_stream_options(&mut result);

    // Strict OpenAI-compatible upstreams (vLLM, enterprise gateways) reject
    // requests that carry tool_choice or parallel_tool_calls without a non-empty
    // tools array. Drop both fields when tools ended up absent or empty after
    // conversion to avoid 503/400 from such providers.
    let has_tools = result
        .get("tools")
        .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()));
    if !has_tools {
        if let Some(obj) = result.as_object_mut() {
            obj.remove("tool_choice");
            obj.remove("parallel_tool_calls");
        }
    }
    // OpenAI 兼容上游在流式下默认不在 SSE 里返回 usage，必须显式声明
    // include_usage 才会在末尾吐 usage chunk。Codex CLI 用 Responses 协议、
    // 自身不带 stream_options，缺这一注入会导致 kimi/MiniMax 等第三方流式请求的
    // token/成本/缓存命中率全部漏记（input/output/cache 全为 0）。
    // 与 Claude→openai_chat 路径共用同一 helper，保证两个客户端方向一致。
    super::transform::inject_openai_stream_include_usage(&mut result);

    Ok(result)
}

/// `stream_options.reasoning_summary_delivery` 是 Responses 协议的字段（Codex 以为在跟
/// 内置 openai 说话时才发，聚合模式就是这样），Chat Completions 没有它；严格校验的
/// 上游（Mistral）收到会整条 422（#9507）。只删这一项，删完为空时整个键删掉，流式时
/// 下面照常补 `include_usage`。
fn drop_responses_only_stream_options(result: &mut Value) {
    let Some(obj) = result.as_object_mut() else {
        return;
    };
    let Some(stream_options) = obj.get_mut("stream_options").and_then(Value::as_object_mut) else {
        return;
    };
    if stream_options
        .remove("reasoning_summary_delivery")
        .is_some()
        && stream_options.is_empty()
    {
        obj.remove("stream_options");
    }
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
    let Some(mapped) = map_reasoning_effort(
        effort,
        config.effort_value_mode.as_deref(),
        config.effort_levels.as_deref(),
    ) else {
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

pub(crate) fn map_reasoning_effort<'a>(
    effort: &str,
    mode: Option<&str>,
    effort_levels: Option<&'a [String]>,
) -> Option<&'a str> {
    let effort = effort.trim().to_ascii_lowercase();
    if matches!(effort.as_str(), "none" | "off" | "disabled") {
        return None;
    }

    // ultra 是 Codex 扩展档位：已知枚举的专用模式（deepseek/openrouter/low_high）
    // 钳到自身最高合法档而非丢弃——丢弃会让"选最深思考"静默退化成"不带 effort"；
    // passthrough 面向枚举未知的通用上游，档位由用户/预设的 reasoningLevels 声明
    // 背书，与 max/xhigh 一样原值透传。
    match mode.unwrap_or("passthrough") {
        // DeepSeek 官方档位 low / high / max，其余档位照官方服务端的换算折
        // （api-docs.deepseek.com/guides/thinking_mode：minimal→low、medium/xhigh→high、ultra→max）。
        "deepseek" => match effort.as_str() {
            "minimal" | "low" => Some("low"),
            "max" | "ultra" => Some("max"),
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
            "max" | "xhigh" | "ultra" => Some("xhigh"),
            "high" => Some("high"),
            "medium" => Some("medium"),
            "low" => Some("low"),
            "minimal" => Some("minimal"),
            _ => None,
        },
        // OpenCode Zen：合法档位逐模型（表数据 = 供应商 modelCatalog 各条目的
        // reasoningLevels，镜像 models.dev——glm-5.2 仅 high|max、deepseek-v4-flash
        // 为 low|high|max、kimi-k3 仅 max），opencode 客户端也严格按模型声明发值，
        // 故不能用统一并集映射。无表（模型未收录目录、或为 toggle/budget 型未声明
        // effort）→ None，完全不发 reasoning_effort；有表 → 钳到「不小于请求的
        // 最近合法档」，请求超出最高档则取最高合法档；请求值本身无法识别 → None
        // （同其他模式的未知值丢弃策略）。
        "zen" => {
            let levels = effort_levels?;
            let requested = zen_effort_rank(&effort)?;
            levels
                .iter()
                .filter_map(|level| zen_effort_rank(level).map(|rank| (rank, level.as_str())))
                .filter(|(rank, _)| *rank >= requested)
                .min_by_key(|(rank, _)| *rank)
                .or_else(|| {
                    levels
                        .iter()
                        .filter_map(|level| {
                            zen_effort_rank(level).map(|rank| (rank, level.as_str()))
                        })
                        .max_by_key(|(rank, _)| *rank)
                })
                .map(|(_, level)| level)
        }
        _ => match effort.as_str() {
            "minimal" => Some("minimal"),
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            "ultra" => Some("ultra"),
            _ => None,
        },
    }
}

/// Codex 规范档位序（minimal < low < medium < high < xhigh < max < ultra），供 zen
/// 逐模型钳制做大小比较；目录里的非法/扩展值（如 "none"）返回 None，查表时被滤掉。
fn zen_effort_rank(effort: &str) -> Option<u8> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" => Some(0),
        "low" => Some(1),
        "medium" => Some(2),
        "high" => Some(3),
        "xhigh" => Some(4),
        "max" => Some(5),
        "ultra" => Some(6),
        _ => None,
    }
}

/// MiniMax 严格要求 messages 中只能首条出现 `role=system`，
/// 否则返回 `invalid params, chat content has invalid message role: system (2013)`。
/// 所以 messages 里最多只留首条 system：
/// - 开头连续的 system（instructions 与首批 developer 指令）合并成首条。
/// - 会话中途、后面（只隔 system）紧接 user 的 system——Codex 在新回合开头插的
///   developer 指令（权限、协作模式、模型切换等）——原位改成 user 消息。拼进首条会
///   改动已经发过的前缀，上游前缀缓存从第一条消息断开，整段对话重算。
/// - 其余中途 system（夹在工具调用循环里，如图片缩放提示）仍拼进首条：原位插 user
///   会把一次工具循环拆成两个回合，影响 DeepSeek / Kimi 等的推理连续性。
fn collapse_system_messages_to_head(messages: Vec<Value>) -> Vec<Value> {
    // 每条消息之后第一条非 system 消息是否为 user。
    let mut followed_by_user = vec![false; messages.len()];
    let mut next_is_user = false;
    for (idx, msg) in messages.iter().enumerate().rev() {
        followed_by_user[idx] = next_is_user;
        if !is_hoistable_system_message(msg) {
            next_is_user = msg.get("role").and_then(Value::as_str) == Some("user");
        }
    }

    let mut system_chunks: Vec<String> = Vec::new();
    let mut rest: Vec<Value> = Vec::with_capacity(messages.len());
    let mut seen_non_system = false;

    for (msg, followed_by_user) in messages.into_iter().zip(followed_by_user) {
        if is_hoistable_system_message(&msg) {
            let text = msg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !text.trim().is_empty() {
                if seen_non_system && followed_by_user {
                    rest.push(json!({"role": "user", "content": text}));
                } else {
                    system_chunks.push(text.to_string());
                }
            }
            continue;
        }
        seen_non_system = true;
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

/// 去掉内容为空的 user 消息，返回去掉的条数。
///
/// Codex 压缩后重建历史时，只有图片、没有文字的用户消息会被换成一段空文本
/// （codex-rs `compact.rs` 的 media-only fallback），转成 Chat 就是 `content: ""`。
/// 严格的 OpenAI 兼容上游直接 400「message content cannot be empty」（#9660）。
/// 空消息不带任何信息，去掉后其余消息逐字节不变；不含空 user 消息的请求不受影响。
fn drop_empty_user_messages(messages: &mut Vec<Value>) -> usize {
    let before = messages.len();
    messages.retain(|message| {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return true;
        }
        match message.get("content") {
            Some(Value::String(text)) => !text.is_empty(),
            Some(Value::Array(parts)) => !parts.is_empty(),
            _ => true,
        }
    });
    before - messages.len()
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
    let mut pending_media = Vec::new();
    let mut pending_reasoning: Option<String> = None;
    let mut last_assistant_index: Option<usize> = None;
    // 已登记的 tool_call id：用于识别「孤儿工具结果」。Codex 历史（例如新建
    // 桌面端会话时注入的 create_thread 回执）可能携带一个没有配对
    // function_call、call_id 为空的 function_call_output。若把它原样序列化成
    // Chat 的 role=tool 消息，严格上游（DeepSeek/OpenCode 等）会判定
    // 「tool message must be a response to a preceding message with tool_calls」
    // 而直接 400。
    let mut known_tool_call_ids: HashSet<String> = HashSet::new();
    // 落在并行工具结果中间的孤儿结果先暂存，等回合边界再降级输出，
    // 避免出现 `tool, user, tool` 这种仍会被严格上游拒绝的序列。
    let mut pending_orphan_results: Vec<Value> = Vec::new();

    match input {
        Value::String(text) => {
            messages.push(json!({
                "role": "user",
                "content": text
            }));
        }
        Value::Array(items) => {
            for item in items {
                // 非工具结果条目即回合边界：先输出暂存的孤儿结果，让 Chat 序列
                // 里 role=tool 消息保持连续；夹在并行工具结果中间直接输出会形成
                // `tool, user, tool`。
                if !is_tool_result_item(item) && hoisted_system_message(item).is_none() {
                    flush_pending_orphan_results(messages, &mut pending_orphan_results);
                }
                append_responses_item_as_chat_message(
                    item,
                    messages,
                    &mut pending_tool_calls,
                    &mut pending_media,
                    &mut pending_reasoning,
                    &mut last_assistant_index,
                    &mut known_tool_call_ids,
                    &mut pending_orphan_results,
                    tool_context,
                )?;
            }
        }
        Value::Object(_) => {
            append_responses_item_as_chat_message(
                input,
                messages,
                &mut pending_tool_calls,
                &mut pending_media,
                &mut pending_reasoning,
                &mut last_assistant_index,
                &mut known_tool_call_ids,
                &mut pending_orphan_results,
                tool_context,
            )?;
        }
        _ => {}
    }

    // 尾部孤儿结果（输入结束时还没等到回合边界）在这里统一降级输出。
    flush_pending_orphan_results(messages, &mut pending_orphan_results);
    // If a later assistant tool-call batch was accumulated after an earlier
    // media-bearing result, the synthetic user media belongs before that next
    // assistant turn.
    flush_pending_chat_tool_media(messages, &mut pending_media);
    flush_pending_tool_calls(
        messages,
        &mut pending_tool_calls,
        &mut pending_media,
        &mut pending_reasoning,
        &mut last_assistant_index,
    );
    // 整个 input 处理完毕后仍剩余的 pending reasoning 属于「真正的尾部」思考
    // （其后已没有任何可前向附挂的 message / function_call），回溯附挂到最后一条
    // assistant；目标已有 reasoning_content 时追加，以保留同一 turn 的 embedded
    // reasoning 与 trailing reasoning。
    attach_pending_reasoning_to_previous_assistant(
        messages,
        last_assistant_index,
        &mut pending_reasoning,
    );
    Ok(())
}

fn is_deepseek_chat_model(model: &str) -> bool {
    let name = model
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name == "deepseek" || name.starts_with("deepseek-")
}

/// Responses can replay assistant text (including an empty tool-only turn's
/// trailing message) before all tool results have arrived. Chat requires those
/// results to follow one assistant tool-call message without another assistant
/// in between. Keep the replayed content on that message until its calls are
/// answered, without crossing user/system boundaries or fabricating results.
fn coalesce_replayed_tool_turn_messages(messages: &mut Vec<Value>) {
    let mut normalized: Vec<Value> = Vec::with_capacity(messages.len());
    let mut open_tool_turn: Option<usize> = None;
    let mut pending_call_ids = HashSet::new();

    for mut message in std::mem::take(messages) {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        register_known_tool_call_id(&mut pending_call_ids, call);
                    }
                }
                if let Some(index) = open_tool_turn {
                    let target = &mut normalized[index];
                    merge_chat_assistant_content(target, message["content"].take());
                    let mut reasoning = message
                        .get("reasoning_content")
                        .and_then(Value::as_str)
                        .map(ToString::to_string);
                    attach_pending_reasoning_to_assistant_unique(target, &mut reasoning);
                    if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut)
                    {
                        target["tool_calls"].as_array_mut().unwrap().append(calls);
                    }
                    continue;
                }
                if !pending_call_ids.is_empty() {
                    open_tool_turn = Some(normalized.len());
                }
            }
            Some("tool") => {
                if let Some(call_id) = message.get("tool_call_id").and_then(Value::as_str) {
                    pending_call_ids.remove(call_id);
                }
                if pending_call_ids.is_empty() {
                    open_tool_turn = None;
                }
            }
            _ => {
                open_tool_turn = None;
                pending_call_ids.clear();
            }
        }
        normalized.push(message);
    }
    *messages = normalized;
}

fn merge_chat_assistant_content(target: &mut Value, content: Value) {
    fn is_empty(content: &Value) -> bool {
        content.is_null()
            || content.as_str().is_some_and(str::is_empty)
            || content.as_array().is_some_and(Vec::is_empty)
    }

    fn into_parts(content: Value) -> Vec<Value> {
        match content {
            Value::Array(parts) => parts,
            Value::String(text) => vec![json!({"type": "text", "text": text})],
            other => vec![other],
        }
    }

    if is_empty(&content) {
        return;
    }
    if is_empty(&target["content"]) {
        target["content"] = content;
        return;
    }
    let previous = target["content"].take();
    target["content"] = match (previous, content) {
        (Value::String(mut previous), Value::String(text)) => {
            previous.push('\n');
            previous.push_str(&text);
            Value::String(previous)
        }
        (previous, content) => {
            let mut parts = into_parts(previous);
            parts.extend(into_parts(content));
            Value::Array(parts)
        }
    };
}

#[allow(clippy::too_many_arguments)]
fn append_responses_item_as_chat_message(
    item: &Value,
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_media: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
    known_tool_call_ids: &mut HashSet<String>,
    pending_orphan_results: &mut Vec<Value>,
    tool_context: &CodexToolContext,
) -> Result<(), ProxyError> {
    let item_type = item.get("type").and_then(|v| v.as_str());
    match item_type {
        Some("function_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            let mut tool_call = responses_function_call_to_chat_tool_call(item, tool_context);
            copy_tool_call_extra_content(&mut tool_call, item);
            register_known_tool_call_id(known_tool_call_ids, &tool_call);
            pending_tool_calls.push(tool_call);
        }
        Some("custom_tool_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            let mut tool_call = responses_custom_tool_call_to_chat_tool_call(item, tool_context);
            copy_tool_call_extra_content(&mut tool_call, item);
            register_known_tool_call_id(known_tool_call_ids, &tool_call);
            pending_tool_calls.push(tool_call);
        }
        Some("tool_search_call") => {
            append_unique_pending_reasoning(pending_reasoning, responses_item_reasoning_text(item));
            let mut tool_call = responses_tool_search_call_to_chat_tool_call(item);
            copy_tool_call_extra_content(&mut tool_call, item);
            register_known_tool_call_id(known_tool_call_ids, &tool_call);
            pending_tool_calls.push(tool_call);
        }
        Some("function_call_output") => {
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            if call_id.is_empty() || !known_tool_call_ids.contains(call_id) {
                // 没有配对 tool_calls 的孤儿结果：先暂存，等回合边界
                // （下一个非工具结果条目 / 输入结束）再降级成 user 消息。
                // 立刻输出会夹在并行工具结果中间形成 `tool, user, tool`，
                // 后一条 tool 前面隔着 user，严格上游会以「tool message
                // must be a response to a preceding message with tool_calls」
                // 直接 400。
                pending_orphan_results.push(item.clone());
                return Ok(());
            }
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
            );
            let media_plan = item
                .get("output")
                .cloned()
                .and_then(plan_chat_tool_output_media);
            let output = if let Some(media_plan) = media_plan {
                queue_chat_tool_output_media(pending_media, call_id, media_plan.media_parts);
                media_plan.tool_content
            } else {
                // Cache-sensitive no-media fallback: keep these expressions
                // byte-for-byte equivalent to the pre-fix conversion.
                match item.get("output") {
                    Some(Value::String(s)) => canonicalize_json_string_if_parseable(s),
                    Some(v) => canonical_json_string(v),
                    None => String::new(),
                }
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("custom_tool_call_output") | Some("tool_search_output") => {
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            if call_id.is_empty() || !known_tool_call_ids.contains(call_id) {
                // 没有配对 tool_calls 的孤儿结果：先暂存，等回合边界
                // （下一个非工具结果条目 / 输入结束）再降级成 user 消息。
                // 立刻输出会夹在并行工具结果中间形成 `tool, user, tool`，
                // 后一条 tool 前面隔着 user，严格上游会以「tool message
                // must be a response to a preceding message with tool_calls」
                // 直接 400。
                pending_orphan_results.push(item.clone());
                return Ok(());
            }
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
            );
            let mut transformed_item = item.clone();
            let replacement_block = json!({
                "type": "text",
                "text": TOOL_RESULT_MEDIA_MOVED_MARKER
            });
            let mut media_parts = Vec::new();
            let replaced = transformed_item
                .get_mut("output")
                .map(|output| {
                    strip_and_clamp_media_from_tool_value(
                        output,
                        &mut media_parts,
                        ToolMediaScope::AllSupported,
                        &replacement_block,
                        TOOL_RESULT_MEDIA_MOVED_MARKER,
                    )
                })
                .unwrap_or(0);
            let output = if replaced > 0 {
                queue_chat_tool_output_media(pending_media, call_id, media_parts);
                canonical_json_string(&transformed_item)
            } else {
                // Preserve the legacy whole-item representation exactly.
                canonical_json_string(item)
            };
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output
            }));
        }
        Some("reasoning") => {
            // reasoning 一律先进入 pending_reasoning，前向附挂到其后的
            // message / function_call（后者经 flush_pending_tool_calls 消费）。
            // 此前这里在 pending_tool_calls 为空时直接回溯附挂到上一条 assistant，
            // 会把新一轮的思考错拼进旧消息，导致紧跟的纯文本 assistant 丢失
            // reasoning_content，思考型模型（kimi 等）多轮对话因此中途"断片"。
            // 真正的尾部剩余由 input 结束时的收尾逻辑、或回合边界消息（user 等）
            // 到达时回溯附挂，见 attach_pending_reasoning_to_previous_assistant。
            append_pending_reasoning(pending_reasoning, responses_reasoning_item_text(item));
        }
        // Codex 远程压缩：触发条目换成压缩提示词，历史里的压缩条目换成摘要正文，
        // 都按一条普通用户消息处理（回合边界、pending reasoning 的附挂规则照旧）。
        Some("compaction_trigger") => {
            append_responses_item_as_chat_message(
                &codex_compaction::compaction_prompt_item(),
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
                known_tool_call_ids,
                pending_orphan_results,
                tool_context,
            )?;
        }
        Some("compaction" | "compaction_summary" | "context_compaction") => {
            if let Some(text) = codex_compaction::compaction_item_replay_text(item) {
                append_responses_item_as_chat_message(
                    &codex_compaction::user_message_item(&text),
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                    known_tool_call_ids,
                    pending_orphan_results,
                    tool_context,
                )?;
            }
        }
        // An `additional_tools` carrier declares tools for this request; its
        // nested tools are lifted via `build_codex_tool_context_from_request`
        // and the carrier itself is not a message. It carries a `role` but no
        // `content`, so letting it fall through the generic message arm used
        // to fabricate a `content: null` system message that strict chat
        // gateways reject with `messages[N]: missing field "content"`.
        Some("additional_tools") => {}
        Some("input_text" | "input_image" | "input_file" | "input_audio") => {
            if let Some(system_message) = hoisted_system_message(item) {
                // Hoisted to messages[0] by collapse_system_messages_to_head; do not
                // flush pending tool media at its mid-stream position.
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                messages.push(system_message);
                return Ok(());
            }
            flush_pending_tool_calls(
                messages,
                pending_tool_calls,
                pending_media,
                pending_reasoning,
                last_assistant_index,
            );
            // `flush_pending_tool_calls` intentionally returns early when
            // there is no new assistant batch. A previous tool result may
            // still have media waiting, so flush it before this new message.
            flush_pending_chat_tool_media(messages, pending_media);
            let role = item
                .get("role")
                .and_then(|v| v.as_str())
                .map(responses_role_to_chat_role)
                .unwrap_or("user");
            let message = json!({
                "role": role,
                "content": responses_content_to_chat_content(role, &Value::Array(vec![item.clone()]))
            });
            if role == "assistant" {
                let mut message = message;
                attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
                return Ok(());
            } else {
                // 非 assistant 的回合边界消息（user 等）：pending reasoning 不再直接
                // 丢弃，优先回溯附挂到上一条 assistant；其已有 reasoning_content 时
                // 追加尾部 reasoning。reasoning 不允许跨 user 回合泄漏到之后的
                // assistant 消息；无上一条 assistant 可附挂时自然丢弃（等同原行为）。
                attach_pending_reasoning_to_previous_assistant(
                    messages,
                    *last_assistant_index,
                    pending_reasoning,
                );
            }
            update_last_assistant_index(messages, &message, last_assistant_index);
            messages.push(message);
        }
        Some("message") | None => {
            if let Some(system_message) = hoisted_system_message(item) {
                // Hoisted to messages[0] by collapse_system_messages_to_head;
                // keep surrounding parallel tool outputs and media contiguous.
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                messages.push(system_message);
            } else if item.get("role").is_some() || item.get("content").is_some() {
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                flush_pending_chat_tool_media(messages, pending_media);
                let message = responses_message_item_to_chat_message(
                    item,
                    pending_reasoning,
                    messages,
                    *last_assistant_index,
                );
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            } else if pending_media.is_empty() {
                // Preserve legacy no-media ordering: inert message-like items
                // used to close a pending tool-call batch.
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
            }
        }
        _ => {
            if let Some(system_message) = hoisted_system_message(item) {
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                messages.push(system_message);
            } else if item.get("role").is_some() || item.get("content").is_some() {
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
                flush_pending_chat_tool_media(messages, pending_media);
                let message = responses_message_item_to_chat_message(
                    item,
                    pending_reasoning,
                    messages,
                    *last_assistant_index,
                );
                update_last_assistant_index(messages, &message, last_assistant_index);
                messages.push(message);
            } else if pending_media.is_empty() {
                // Preserve legacy no-media ordering without letting an inert
                // unknown item flush a media-bearing result batch.
                flush_pending_tool_calls(
                    messages,
                    pending_tool_calls,
                    pending_media,
                    pending_reasoning,
                    last_assistant_index,
                );
            }
        }
    }

    Ok(())
}

/// 登记一个即将进入 Chat 历史的 tool_call id。统一的取值顺序与
/// `responses_*_to_chat_tool_call` 保持一致（call_id 优先，回退 id）。
fn register_known_tool_call_id(known_tool_call_ids: &mut HashSet<String>, tool_call: &Value) {
    if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
        if !id.is_empty() {
            known_tool_call_ids.insert(id.to_string());
        }
    }
}

/// 判断条目是否为工具结果（function_call_output / custom_tool_call_output /
/// tool_search_output）。孤儿暂存逻辑用它识别回合边界：只有连续的工具结果
/// 能保持 role=tool 相邻，其他条目出现即意味着结果段结束。
fn is_tool_result_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("function_call_output" | "custom_tool_call_output" | "tool_search_output")
    )
}

fn is_hoistable_system_message(msg: &Value) -> bool {
    msg.get("role").and_then(Value::as_str) == Some("system")
        && msg.get("content").is_some_and(Value::is_string)
}

fn hoisted_system_message(item: &Value) -> Option<Value> {
    let item_type = item.get("type").and_then(Value::as_str);
    if matches!(
        item_type,
        Some(
            "function_call"
                | "custom_tool_call"
                | "tool_search_call"
                | "function_call_output"
                | "custom_tool_call_output"
                | "tool_search_output"
                | "reasoning"
                | "compaction_trigger"
                | "compaction"
                | "compaction_summary"
                | "context_compaction"
                | "additional_tools"
        )
    ) {
        return None;
    }
    let role = item.get("role").and_then(Value::as_str)?;
    if responses_role_to_chat_role(role) != "system" {
        return None;
    }
    let content = match item_type {
        Some("input_text" | "input_image" | "input_file" | "input_audio") => {
            responses_content_to_chat_content("system", &Value::Array(vec![item.clone()]))
        }
        _ => responses_content_to_chat_content("system", item.get("content")?),
    };
    content.is_string().then(|| {
        json!({
            "role": "system",
            "content": content
        })
    })
}

/// 输出暂存的孤儿工具结果。孤儿可能夹在并行工具结果中间
/// （`tool, 孤儿, tool`）；立即降级成 user 消息会打断 tool 序列，后一条
/// tool 前面隔着 user，仍会被严格上游 400，因此统一等到回合边界
/// （下一个非工具结果条目 / 输入结束）再在这里输出。
///
/// 这里刻意不消费 pending reasoning、也不改写 `last_assistant_index`：
/// 合成 user 消息只是降级输出，不是真实用户回合，不应改变 reasoning 的
/// 归属。紧随其后的边界条目会像没有孤儿一样正常附挂 pending reasoning，
/// 输入结束时也有尾部兜底；若在此处回溯附挂并清空索引，会把本应前向
/// 附挂给后继 function_call 批次的 reasoning 提前消费掉。
fn flush_pending_orphan_results(
    messages: &mut Vec<Value>,
    pending_orphan_results: &mut Vec<Value>,
) {
    for item in std::mem::take(pending_orphan_results) {
        append_orphan_tool_output_as_user_message(messages, &item);
    }
}

/// 孤儿工具结果降级为 user 消息。这类条目没有可配对的 `tool_calls`
/// （典型来源：call_id 为空的 function_call_output），保留内容以避免丢失
/// 上下文，但不能以 role=tool 的形式发给严格上游。
///
/// 输出里的图片/文件媒体块复用配对路径的提取逻辑：文本 body 内联
/// `TOOL_RESULT_MEDIA_ATTACHED_MARKER` 占位，媒体本体作为同一条 user
/// 消息的 content parts 跟随，避免图片被 base64 序列化成纯文本——模型既
/// 看不到图，token 还会被瞬间撑爆。配对路径的 "moved to the following
/// user message" 占位在本路径会指向一条不存在的消息，不能沿用。
fn append_orphan_tool_output_as_user_message(messages: &mut Vec<Value>, item: &Value) {
    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
    let (body, media_parts) = plan_orphan_tool_output_body(item.get("output"));
    let marker = if call_id.is_empty() {
        ORPHAN_TOOL_OUTPUT_MARKER.to_string()
    } else {
        format!("{ORPHAN_TOOL_OUTPUT_MARKER} (call_id: {call_id})")
    };
    let text = format!("{marker}\n{body}");
    let content = if media_parts.is_empty() {
        json!(text)
    } else {
        let mut parts = vec![json!({ "type": "text", "text": text })];
        parts.extend(media_parts);
        Value::Array(parts)
    };
    messages.push(json!({
        "role": "user",
        "content": content
    }));
}

/// 计算孤儿工具结果的文本 body 与媒体 parts。媒体识别复用配对路径的
/// `plan_chat_tool_output_media_with_marker`；没有媒体时保持旧版逐字段的
/// 纯文本序列化，不给无媒体输出多加一层 JSON 包装。
///
/// 孤儿路径的媒体就挂在这条 user 消息自己的后续 parts 里，占位符用
/// `TOOL_RESULT_MEDIA_ATTACHED_MARKER`（与 Anthropic/Gemini/Responses
/// bridge 的"媒体已作为原生媒体挂载"用词一致）。
fn plan_orphan_tool_output_body(output: Option<&Value>) -> (String, Vec<Value>) {
    if let Some(plan) = output.cloned().and_then(|output| {
        plan_chat_tool_output_media_with_marker(output, TOOL_RESULT_MEDIA_ATTACHED_MARKER)
    }) {
        return (plan.tool_content, plan.media_parts);
    }
    let body = match output {
        Some(Value::String(s)) => canonicalize_json_string_if_parseable(s),
        Some(v) => canonical_json_string(v),
        None => String::new(),
    };
    (body, Vec::new())
}

fn flush_pending_tool_calls(
    messages: &mut Vec<Value>,
    pending_tool_calls: &mut Vec<Value>,
    pending_media: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
    last_assistant_index: &mut Option<usize>,
) {
    if pending_tool_calls.is_empty() {
        return;
    }

    // One Responses model turn can contain an assistant commentary message
    // directly followed by function-call items. Keep them on the same Chat
    // assistant message: a standalone text-only assistant turn teaches Chat
    // models to stop after a progress update instead of emitting the expected
    // tool call.
    if let Some(merged_index) = merge_pending_tool_calls_into_adjacent_assistant(
        messages,
        pending_tool_calls,
        pending_reasoning,
    ) {
        *last_assistant_index = Some(merged_index);
        return;
    }

    // Media from the preceding tool-result batch must be presented before a
    // new assistant tool-call turn. Consecutive outputs do not enter here
    // because `pending_tool_calls` is empty after the first output.
    flush_pending_chat_tool_media(messages, pending_media);
    let mut message = json!({
        "role": "assistant",
        "content": null,
        "tool_calls": std::mem::take(pending_tool_calls)
    });
    attach_pending_reasoning_to_assistant(&mut message, pending_reasoning);
    *last_assistant_index = Some(messages.len());
    messages.push(message);
}

/// Merge pending Chat tool calls into a directly adjacent assistant message
/// that does not carry tool calls yet. The Responses input preserves both the
/// commentary text and the calls as separate items of the same model turn;
/// serializing them as two consecutive assistant messages lets Chat models
/// imitate the first text-only message as a complete turn.
fn merge_pending_tool_calls_into_adjacent_assistant(
    messages: &mut [Value],
    pending_tool_calls: &mut Vec<Value>,
    pending_reasoning: &mut Option<String>,
) -> Option<usize> {
    let index = messages
        .iter()
        .rposition(|msg| !is_hoistable_system_message(msg))?;
    let message = messages.get_mut(index)?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let has_tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty());
    if has_tool_calls {
        return None;
    }

    let obj = message.as_object_mut()?;
    obj.insert(
        "tool_calls".to_string(),
        Value::Array(std::mem::take(pending_tool_calls)),
    );
    attach_pending_reasoning_to_assistant_unique(message, pending_reasoning);
    Some(index)
}

fn attach_pending_reasoning_to_assistant_unique(
    message: &mut Value,
    pending_reasoning: &mut Option<String>,
) {
    let Some(reasoning) = pending_reasoning.take() else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }

    let existing_text = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let existing_segments: Vec<&str> = existing_text
        .split("\n\n")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    let missing_segments: Vec<&str> = reasoning
        .split("\n\n")
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && !existing_segments.contains(segment))
        .collect();
    if missing_segments.is_empty() {
        return;
    }

    let merged = if existing_segments.is_empty() {
        missing_segments.join("\n\n")
    } else {
        let existing = existing_segments.join("\n\n");
        let missing = missing_segments.join("\n\n");
        format!("{existing}\n\n{missing}")
    };
    if let Some(obj) = message.as_object_mut() {
        obj.insert("reasoning_content".to_string(), Value::String(merged));
    }
}

fn responses_message_item_to_chat_message(
    item: &Value,
    pending_reasoning: &mut Option<String>,
    messages: &mut [Value],
    last_assistant_index: Option<usize>,
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
    } else {
        // 非 assistant 的回合边界消息（user 等）：pending reasoning 不再直接丢弃，
        // 回溯附挂到上一条 assistant；其已有 reasoning_content 时追加尾部
        // reasoning，同时防止 reasoning 跨 user 回合泄漏到之后的 assistant 消息。
        attach_pending_reasoning_to_previous_assistant(
            messages,
            last_assistant_index,
            pending_reasoning,
        );
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
/// `attach_pending_reasoning_to_previous_assistant` 回填，过早注入占位会被
/// `append_reasoning_content` 追加而污染真实思考。
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

/// 移除 assistant 历史里的 `reasoning_content`，返回被移除的消息条数。
///
/// 该字段是 DeepSeek / Kimi 经第三方网关时的兼容约定，严格校验的
/// OpenAI 兼容上游（Mistral）会以 `extra_forbidden` 整条 422 拒收。所有注入点
/// （真实思考回填、`"tool call"` 占位）都在本模块内，出口统一剥离比逐个给注入点
/// 加门控更不容易漏。与 `claude.rs` 的
/// `should_preserve_reasoning_content_for_openai_chat` 同口径。
fn strip_assistant_reasoning_content(messages: &mut [Value]) -> usize {
    let mut stripped = 0;
    for message in messages.iter_mut() {
        let is_assistant = message.get("role").and_then(Value::as_str) == Some("assistant");
        if !is_assistant {
            continue;
        }
        if let Some(obj) = message.as_object_mut() {
            if obj.remove("reasoning_content").is_some() {
                stripped += 1;
            }
        }
    }
    stripped
}

/// 将仍未消费的 pending reasoning 回溯附挂到上一条 assistant 消息。
///
/// 只允许两种「真正的尾部」场景调用：
/// 1. 整个 input 处理完毕后 pending_reasoning 仍有剩余——其后已没有任何可
///    前向附挂的 message / function_call；
/// 2. user 等回合边界消息到达时 pending_reasoning 非空——reasoning 不允许
///    跨 user 回合泄漏到之后的 assistant 消息，也不能直接丢弃可归属的思考。
///
/// 这里已经处于尾部/边界收尾点，不是普通 reasoning 的前向归属路径；
/// 若目标已有 reasoning_content，追加尾部 reasoning 以保留同一 assistant turn
/// 中同时出现的 embedded reasoning 与尾随 reasoning。无论是否附挂成功，
/// pending 都会被消费（拿走），绝不留到下一条 assistant。
fn attach_pending_reasoning_to_previous_assistant(
    messages: &mut [Value],
    last_assistant_index: Option<usize>,
    pending_reasoning: &mut Option<String>,
) {
    let Some(reasoning) = pending_reasoning.take() else {
        return;
    };
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }
    let Some(message) = last_assistant_index.and_then(|index| messages.get_mut(index)) else {
        return;
    };
    if message.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return;
    }
    if let Some(obj) = message.as_object_mut() {
        append_reasoning_content(obj, reasoning);
    }
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
            "input_file" => {
                if let Some(file) = responses_input_file_to_chat_file(part) {
                    chat_parts.push(json!({
                        "type": "file",
                        "file": file
                    }));
                    has_non_text_part = true;
                }
            }
            "input_audio" => {
                if let Some(input_audio) = part.get("input_audio") {
                    chat_parts.push(json!({
                        "type": "input_audio",
                        "input_audio": input_audio.clone()
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

fn responses_input_file_to_chat_file(part: &Value) -> Option<Value> {
    chat_file_from_input_file(part)
}

/// Collect tools declared inline in the request `input` instead of top-level
/// `tools`: `tool_search_output` items (dynamically loaded tool groups) and
/// `additional_tools` carriers (Codex 0.154+ ships extra tools — the
/// `functions`/`collaboration` exec sandbox, plugins — in this Responses
/// private-extension carrier). The xAI Responses passthrough already promotes
/// the same carriers (`promote_additional_tools`); the Chat and Anthropic
/// converters go through this registry so they keep the carried tools too.
fn collect_input_declared_tools(
    value: &Value,
    seen_identities: &mut HashSet<(Option<String>, String)>,
    out: &mut Vec<PendingCodexTool>,
) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_input_declared_tools(item, seen_identities, out);
            }
        }
        Value::Object(obj) => {
            if matches!(
                obj.get("type").and_then(|v| v.as_str()),
                Some("tool_search_output" | "additional_tools")
            ) {
                if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
                    for tool in tools {
                        collect_response_tool(tool, seen_identities, out);
                    }
                }
            }
            for value in obj.values() {
                collect_input_declared_tools(value, seen_identities, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
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

fn responses_custom_tool_description(tool: &Value) -> String {
    let mut description = String::new();
    description.push_str(CUSTOM_TOOL_PRESERVED_METADATA_HEADING);
    description.push_str("\n```json\n");
    description.push_str(&serialize_tool_definition_for_description(tool));
    description.push_str("\n```");
    description
}

fn serialize_tool_definition_for_description(tool: &Value) -> String {
    // Keep the embedded definition compact to reduce tool-description token
    // overhead for chat-only upstreams, while remaining stable across map
    // storage order.
    canonical_json_string(tool)
}

/// Normalize a function's `parameters` JSON Schema for strict Chat Completions
/// providers while preserving the schema's validation semantics.
///
/// Some Responses tools carry `parameters: null`, `parameters: {"type": null}`
/// or `required: null`. A few strict OpenAI-compatible gateways also require
/// `required` to be an array whenever `additionalProperties` is `false`.
fn normalize_function_parameters(params: Option<&Value>) -> Value {
    let mut params = match params {
        Some(Value::Object(obj)) => Value::Object(obj.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    sanitize_parameter_schema(&mut params, true);
    params
}

fn sanitize_parameter_schema(schema: &mut Value, is_root: bool) {
    let Value::Object(obj) = schema else {
        return;
    };

    if is_root && obj.get("type").and_then(Value::as_str) != Some("object") {
        obj.insert("type".to_string(), json!("object"));
    }

    let invalid_required = obj
        .get("required")
        .is_some_and(|required| !required.is_array());
    let strict_object_without_required = obj.get("required").is_none()
        && obj.get("additionalProperties") == Some(&Value::Bool(false));
    if invalid_required || strict_object_without_required {
        obj.insert("required".to_string(), json!([]));
    }

    // Recurse only through keywords whose values are themselves schemas. This
    // avoids rewriting arbitrary JSON objects in `default` or `examples`.
    for key in [
        "properties",
        "patternProperties",
        "$defs",
        "definitions",
        "dependentSchemas",
        "dependencies",
    ] {
        if let Some(children) = obj.get_mut(key).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                sanitize_parameter_schema_value(child, false);
            }
        }
    }

    for key in [
        "items",
        "additionalItems",
        "contains",
        "propertyNames",
        "additionalProperties",
        "unevaluatedItems",
        "unevaluatedProperties",
        "contentSchema",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(child) = obj.get_mut(key) {
            sanitize_parameter_schema_value(child, false);
        }
    }

    for key in ["prefixItems", "allOf", "anyOf", "oneOf"] {
        if let Some(children) = obj.get_mut(key).and_then(Value::as_array_mut) {
            for child in children {
                sanitize_parameter_schema_value(child, false);
            }
        }
    }
}

fn sanitize_parameter_schema_value(value: &mut Value, is_root: bool) {
    match value {
        Value::Object(_) => sanitize_parameter_schema(value, is_root),
        Value::Array(children) => {
            for child in children {
                sanitize_parameter_schema_value(child, false);
            }
        }
        _ => {}
    }
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
            // Normalize parameters for strict OpenAI-compatible providers.
            let parameters = normalize_function_parameters(obj.get("parameters"));
            obj.insert("parameters".to_string(), parameters);

            obj.insert("name".to_string(), json!(chat_name));
            if let Some(strict) = tool.get("strict").cloned() {
                obj.entry("strict".to_string()).or_insert(strict);
            }
        }
        return Some(chat_tool);
    }

    let mut function = json!({
        "name": chat_name,
    });
    // Omit a missing description instead of serializing null — hosted tools
    // and undescribed custom tools reach this branch without the field, and
    // strict OpenAI-compatible upstreams reject the whole request on a null
    // value (400 "expected string, received null"). Same treatment as the
    // anthropic→chat/responses converters; insertion keeps the original
    // name→description→parameters key order for byte-identical output when
    // the description is present.
    if let Some(description) = tool.get("description").filter(|d| !d.is_null()) {
        function["description"] = description.clone();
    }
    function["parameters"] = normalize_function_parameters(tool.get("parameters"));
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

fn responses_custom_tool_call_to_chat_tool_call(
    item: &Value,
    tool_context: &CodexToolContext,
) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let chat_name = tool_context.chat_name_for_response_function(name, None);
    let input = item.get("input").cloned().unwrap_or_else(|| json!(""));

    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": chat_name,
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
            let chat_name = tool_context.chat_name_for_response_function(name, None);
            json!({
                "type": "function",
                "function": {
                    "name": chat_name
                }
            })
        }
        _ => tool_choice.clone(),
    }
}

/// Convert a non-streaming Chat Completions response into a Responses response,
/// restoring Codex-specific tool names using the original Responses request.
pub(crate) fn chat_completion_to_response_with_context(
    body: Value,
    tool_context: &CodexToolContext,
) -> Result<Value, ProxyError> {
    let body = unwrap_chat_completion_envelope(body);
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
    let tool_calls =
        chat_tool_calls_to_response_output_items(message, reasoning.as_deref(), tool_context);

    // 丢弃过工具调用、且最终一个工具调用都没剩下时，Codex 会收到一个
    // "status=completed 但 output 里没有任何工具调用" 的回合，agent loop 必然静默
    // 收尾（#4341）。此时如实报错，而不是谎报成功。只要还剩下任何一个合法工具
    // 调用，Codex 本来就会继续，判据不成立，行为保持不变。
    //
    // 🔴 与流式分支一致，只对本应 `completed` 的回合生效：`finish_reason=length`
    // 是截断，工具调用缺 name 是截断的后果而非上游发了畸形数据，报成
    // tool_call_dropped 会给出错误的归因。
    if response_status_from_finish_reason(finish_reason) == "completed"
        && tool_calls.dropped > 0
        && tool_calls.items.is_empty()
    {
        return Err(ProxyError::TransformError(format!(
            "Upstream returned {} tool call(s) without a function name, \
             leaving no usable tool call in this turn",
            tool_calls.dropped
        )));
    }
    output.extend(tool_calls.items);

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

/// Chat 思考转成的推理条目 id：`rs_<回复 id>_<输出序号>`（与 Anthropic 转换同一格式）。
///
/// 序号不能省。这条没有密文，Codex 切回官方时原样回放；`store:false` 下官方只要看到
/// `rs_` / `rs_resp_` 后面正好是 32 位十六进制（有的上游回复 id 就是 32 位十六进制），就当成
/// 自己签发的去回查，查不到报 404，直连模式下线程就此续不上（#9054）。末尾带上 `_<序号>`
/// 就不会被回查（11-26 实测）。id 不能干脆去掉：Codex 会另配一个，和进行中事件的 item_id
/// 对不上，官方供应商下思考文字不再实时显示；Grok Build 缺 id 直接解析失败。
pub(crate) fn chat_reasoning_item_id(response_id: &str, output_index: u32) -> String {
    format!("rs_{response_id}_{output_index}")
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
        "id": chat_reasoning_item_id(response_id, 0),
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

    // Mistral 开思考后把思考放在分段 content 的 thinking 段里（#9902）
    if let Some(parts) = message.get("content").and_then(|v| v.as_array()) {
        let reasoning = parts
            .iter()
            .filter(|part| part.get("type").and_then(|v| v.as_str()) == Some("thinking"))
            .filter_map(chat_thinking_part_text)
            .collect::<String>();
        if !reasoning.is_empty() {
            return Some(reasoning);
        }
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
        "id": response_message_item_id(response_id),
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": content
    }))
}

/// 非流式工具调用转换结果。`dropped` 记录因缺少合法函数名而被丢弃的条数，
/// 供调用方判断本回合是否已经不可能让 Codex 继续（见 #4341）。
struct ChatToolCallItems {
    items: Vec<Value>,
    dropped: usize,
}

fn chat_tool_calls_to_response_output_items(
    message: &Value,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> ChatToolCallItems {
    let mut output = Vec::new();
    let mut dropped = 0usize;

    if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            // Skip tool calls with missing function names (defensive: some models
            // may generate tool calls without providing a valid name)
            let function = tool_call.get("function").unwrap_or(&Value::Null);
            let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
            // 纯空白名同样对应不到任何已发布工具，与空名同等对待。
            if name.trim().is_empty() {
                dropped += 1;
                // 只记结构信息，不记 arguments 内容（可能包含用户代码）。
                let call_id_empty = tool_call
                    .get("id")
                    .and_then(|v| v.as_str())
                    .is_none_or(str::is_empty);
                let args_bytes = function
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .map(str::len)
                    .unwrap_or(0);
                log::warn!(
                    "[Codex] dropped tool call: index={index} call_id_empty={call_id_empty} \
                     args_bytes={args_bytes} tools_total={}",
                    tool_calls.len()
                );
                continue;
            }
            output.push(chat_tool_call_to_response_item(
                tool_call,
                index,
                reasoning,
                tool_context,
            ));
        }
    } else if let Some(function_call) = message.get("function_call") {
        match chat_legacy_function_call_to_response_item(function_call, reasoning, tool_context) {
            Some(item) => output.push(item),
            None => dropped += 1,
        }
    }

    ChatToolCallItems {
        items: output,
        dropped,
    }
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
    let mut item = response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        &call_id,
        name,
        &arguments,
        reasoning,
        tool_context,
    );
    copy_tool_call_extra_content(&mut item, tool_call);
    item
}

fn chat_legacy_function_call_to_response_item(
    function_call: &Value,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Option<Value> {
    let call_id = function_call
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("call_0");
    let name = function_call
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Skip legacy function calls with missing names (defensive: some models
    // may generate function_call without providing a valid name)。
    // 纯空白名同样对应不到任何已发布工具，与空名同等对待。
    if name.trim().is_empty() {
        // 只记结构信息，不记 arguments 内容（可能包含用户代码）。
        let args_bytes = function_call
            .get("arguments")
            .and_then(|v| v.as_str())
            .map(str::len)
            .unwrap_or(0);
        log::warn!(
            "[Codex] dropped legacy function_call: call_id={call_id} args_bytes={args_bytes}"
        );
        return None;
    }

    let arguments = canonicalize_tool_arguments(function_call.get("arguments"));

    let item_id = response_tool_call_item_id_from_chat_name(call_id, name, tool_context);
    Some(response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        call_id,
        name,
        &arguments,
        reasoning,
        tool_context,
    ))
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
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens": 0,
            "total_tokens": 0,
            "output_tokens_details": { "reasoning_tokens": 0 }
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

    // 每环要求「存在且非 0」：显式 0 按未上报处理继续向后找（#8041：部分中转把
    // 靠前字段硬编码为桩 0，真值只在低顺位字段）。官方端点同值非 0 不受影响；
    // 真 0 命中时各候选同为 0/缺失，结果不变。
    fn provided_nonzero(value: Option<&Value>) -> Option<u64> {
        value.and_then(Value::as_u64).filter(|tokens| *tokens > 0)
    }
    let direct_cache_read = provided_nonzero(usage.get("cache_read_input_tokens"));
    let cached = direct_cache_read
        .or_else(|| provided_nonzero(usage.pointer("/prompt_tokens_details/cached_tokens")))
        .or_else(|| provided_nonzero(usage.pointer("/input_tokens_details/cached_tokens")))
        // DeepSeek Chat 的文档化缓存命中字段（与 usage/parser.rs 的处理对应），末位兜底。
        // 官方端点目前把同值镜像进未文档化的 prompt_tokens_details.cached_tokens（上面的
        // 标准字段已命中），故仅当上游只发文档字段、不发镜像时此兜底生效（如部分中转），
        // 并防御未文档化镜像将来消失；上游发任一标准字段时行为零变化。
        .or_else(|| provided_nonzero(usage.get("prompt_cache_hit_tokens")))
        .unwrap_or(0);
    let cache_write = usage
        .pointer("/prompt_tokens_details/cache_write_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);
    if cached > 0 || cache_write > 0 {
        result["input_tokens_details"] = json!({
            "cached_tokens": cached,
            "cache_write_tokens": cache_write
        });
    } else {
        result["input_tokens_details"] = json!({ "cached_tokens": 0 });
    }

    if let Some(details) = usage
        .get("completion_tokens_details")
        .filter(|v| v.is_object())
    {
        let mut details = details.clone();
        if details.get("reasoning_tokens").is_none() {
            details["reasoning_tokens"] = json!(0);
        }
        result["output_tokens_details"] = details;
    } else {
        result["output_tokens_details"] = json!({ "reasoning_tokens": 0 });
    }

    if let Some(cache_read) = direct_cache_read {
        result["cache_read_input_tokens"] = json!(cache_read);
    }
    if cache_write > 0 {
        result["cache_creation_input_tokens"] = json!(cache_write);
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

/// Derive the ID of a Responses `message` output item from its response ID.
///
/// A `message` item ID must begin with `msg_`: Codex persists converted
/// output items in the thread history and replays them as input, and strict
/// Responses upstreams reject any message item whose ID does not carry that
/// prefix (`invalid_id_prefix`). Building the item ID as `{response_id}_msg`
/// produces `resp_..._msg`, which poisons the stored thread once the user
/// switches to an official/native Responses provider (see #9054). Rewriting
/// the response ID's own `resp_` prefix to `msg_` keeps the item traceable to
/// its response while satisfying the prefix rule.
pub(crate) fn response_message_item_id(response_id: &str) -> String {
    let suffix = response_id.strip_prefix("resp_").unwrap_or(response_id);
    format!("msg_{suffix}")
}

/// [`response_message_item_id`] for one of several message items belonging to
/// the same response, disambiguated by output index.
pub(crate) fn response_message_item_id_indexed(
    response_id: &str,
    output_index: impl std::fmt::Display,
) -> String {
    format!("{}_{output_index}", response_message_item_id(response_id))
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

    let mut code = source
        .get("code")
        .cloned()
        .or_else(|| source.pointer("/base_resp/status_code").cloned())
        .unwrap_or(serde_json::Value::Null);

    // Gemini uses numeric HTTP codes (or no code at all) for this error. Codex
    // only recognizes context_length_exceeded as a context-budget failure;
    // INVALID_ARGUMENT/upstream_error cannot trigger its compaction recovery.
    if message
        .to_ascii_lowercase()
        .contains("input token count exceeds the maximum number of tokens allowed")
    {
        code = json!("context_length_exceeded");
    }

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
mod tests;
