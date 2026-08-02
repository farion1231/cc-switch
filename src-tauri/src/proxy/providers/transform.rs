//! 格式转换模块
//!
//! 实现 Anthropic ↔ OpenAI 格式转换，用于 OpenRouter 支持
//! 参考: anthropic-proxy-rs

use crate::proxy::{
    error::ProxyError,
    json_canonical::canonical_json_string,
    tool_media::{
        chat_media_part_from_tool_part, flush_pending_chat_tool_media, plan_chat_tool_output_media,
        queue_chat_tool_output_media, ToolMediaScope,
    },
};
use serde_json::{json, Value};

const ANTHROPIC_BILLING_HEADER_PREFIX: &str = "x-anthropic-billing-header:";

/// Strip only a leading Claude Code attribution line from system text.
///
/// Claude Code can send dynamic `x-anthropic-billing-header` metadata at the
/// start of `system`. If forwarded into OpenAI Chat messages or Responses
/// `instructions`, the rotating `cch=` value changes the prompt prefix on every
/// request and prevents prefix cache reuse (#2350). Later occurrences are kept
/// to avoid deleting user-authored prompt text.
pub(crate) fn strip_leading_anthropic_billing_header(text: &str) -> &str {
    if !text.starts_with(ANTHROPIC_BILLING_HEADER_PREFIX) {
        return text;
    }

    let Some(line_end) = text
        .as_bytes()
        .iter()
        .position(|byte| *byte == b'\n' || *byte == b'\r')
    else {
        return "";
    };

    let bytes = text.as_bytes();
    let mut rest_start = line_end + 1;
    if bytes[line_end] == b'\r' && bytes.get(line_end + 1) == Some(&b'\n') {
        rest_start += 1;
    }

    let rest = &text[rest_start..];
    if let Some(stripped) = rest.strip_prefix("\r\n") {
        stripped
    } else if let Some(stripped) = rest.strip_prefix('\n') {
        stripped
    } else if let Some(stripped) = rest.strip_prefix('\r') {
        stripped
    } else {
        rest
    }
}

/// Detect OpenAI o-series reasoning models (o1, o3, o4-mini, etc.)
/// These models require `max_completion_tokens` instead of `max_tokens`.
pub fn is_openai_o_series(model: &str) -> bool {
    model.len() > 1
        && model.starts_with('o')
        && model.as_bytes().get(1).is_some_and(|b| b.is_ascii_digit())
}

/// Detect Responses-compatible models that support reasoning effort.
///
/// Supported families:
/// - o-series: o1, o3, o4-mini, etc.
/// - GPT-5+: gpt-5, gpt-5.1, gpt-5.4, gpt-5-codex, etc.
/// - xAI Grok Build models. `grok-4.5` is the current documented Grok Build
///   model; retain the previous `grok-build-*` family for saved providers.
pub fn supports_reasoning_effort(model: &str) -> bool {
    let normalized = model.to_lowercase();
    is_openai_o_series(&normalized)
        || normalized
            .strip_prefix("gpt-")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit() && c >= '5')
        || normalized == "grok-4.5"
        || normalized.starts_with("grok-4.5-")
        || normalized.starts_with("grok-build-")
        || is_qwen_reasoning_effort_model(&normalized)
}

/// Resolve the appropriate OpenAI `reasoning_effort` from an Anthropic request body.
///
/// Priority:
/// 1. Explicit `output_config.effort` — preserves the user's intent directly.
///    `low`/`medium`/`high` map 1:1; `max` maps to `xhigh`
///    (supported by mainstream GPT models). Unknown values are ignored.
/// 2. Fallback: `thinking.type` + `budget_tokens`:
///    - `adaptive` → `xhigh` (adaptive = maximum reasoning effort)
///    - `enabled` with budget → `low` (<4 000) / `medium` (4 000–15 999) / `high` (≥16 000)
///    - `enabled` without budget → `high` (conservative default)
///    - `disabled` / absent → `None`
pub fn resolve_reasoning_effort(body: &Value) -> Option<&'static str> {
    // --- Priority 1: explicit output_config.effort ---
    if let Some(effort) = body
        .pointer("/output_config/effort")
        .and_then(|v| v.as_str())
    {
        return match effort {
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "max" => Some("xhigh"), // OpenAI xhigh = maximum reasoning effort
            _ => None,              // unknown value — do not inject
        };
    }

    // --- Priority 2: thinking.type + budget_tokens fallback ---
    let thinking = body.get("thinking")?;
    match thinking.get("type").and_then(|t| t.as_str()) {
        Some("adaptive") => Some("xhigh"),
        Some("enabled") => {
            let budget = thinking.get("budget_tokens").and_then(|b| b.as_u64());
            match budget {
                Some(b) if b < 4_000 => Some("low"),
                Some(b) if b < 16_000 => Some("medium"),
                Some(_) => Some("high"),
                None => Some("high"), // enabled but no budget — assume strong reasoning
            }
        }
        _ => None, // disabled or missing
    }
}

/// Parse the numeric version following `qwen` in a model name → `(major, minor)`.
///
/// `qwen3.8-max-preview` → `(3, 8)`; `qwen/qwen3.10-plus` → `(3, 10)`;
/// `qwen4-max` → `(4, 0)` (missing minor padded with 0). Unversioned names
/// (`qwen-max`, `qwen-plus`, `qwen-vl-max`) and hyphen-joined generations
/// (`qwen3-235b-a22b` → `(3, 0)`, `qwen3-max-2026-01-23` → `(3, 0)`) parse to
/// their leading segment only, keeping them below the 3.8 reasoning_effort
/// threshold. Returns `None` when no digit follows `qwen`.
pub(crate) fn parse_qwen_version(model: &str) -> Option<(u32, u32)> {
    let lower = model.to_ascii_lowercase();
    // Provider-prefixed names (`qwen/qwen3.8-max-preview`) carry several `qwen`
    // occurrences; try each segment that follows one and take the first that
    // starts with a digit.
    for after in lower.split("qwen").skip(1) {
        if !after.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let version_span: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let mut segments = version_span.split('.');
        let Some(Ok(major)) = segments.next().filter(|s| !s.is_empty()).map(|s| s.parse())
        else {
            continue;
        };
        let minor = segments
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        return Some((major, minor));
    }
    None
}

/// qwen models whose API exposes `reasoning_effort` tiers (version ≥ 3.8).
///
/// Numeric comparison, not substring matching: qwen3.8/qwen3.9/qwen3.10/qwen4.0
/// qualify; qwen3.7, qwen3.5, qwen3-235b, qwen-max stay on the legacy
/// `enable_thinking` behavior. Assumes all qwen ≥ 3.8 share the same
/// reasoning_effort parameter domain.
pub fn is_qwen_reasoning_effort_model(model: &str) -> bool {
    match parse_qwen_version(model) {
        Some((major, minor)) => major > 3 || (major == 3 && minor >= 8),
        None => false,
    }
}

/// Map any incoming effort/thinking signal to a qwen ≥3.8 tier.
///
/// qwen3.8+ are thinking-ONLY models: disable signals (`none`/`off`/`disabled`)
/// map to the lowest tier `low` instead of being forwarded — the vendor errors
/// on any "thinking off" parameter. `high`/`max` clamp up to `xhigh` (qwen has
/// no `high` tier); unknown values also land on `xhigh`.
pub fn map_effort_to_qwen_tier(effort: &str) -> &'static str {
    match effort.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" | "minimal" | "low" => "low",
        "medium" => "medium",
        _ => "xhigh", // high, xhigh, max, unknown → strongest tier
    }
}

/// qwen ≥3.8 tier → thinking budget tokens (DashScope documented mapping:
/// low=4096, medium=16384, xhigh=262144; the gateway maps budgets back to
/// tiers at the same boundaries).
pub fn qwen_tier_to_budget(tier: &str) -> u64 {
    match tier {
        "low" => 4_096,
        "medium" => 16_384,
        _ => 262_144,
    }
}

/// Shared headroom clamp for every site that writes an Anthropic
/// `thinking.budget_tokens`: Anthropic 400s unless `budget_tokens < max_tokens`
/// (and requires `budget_tokens >= 1024` when thinking is on), and a budget
/// that consumes nearly all of max_tokens leaves an effectively empty
/// completion. Cap at half of max_tokens, floor at 1024, and never reach
/// max_tokens itself.
///
/// Invariant: for `max_tokens >= 2` the result is always `< max_tokens`
/// (possibly below the 1024 floor when max_tokens itself is tiny — such a
/// request is already degenerate and upstream decides). Never returns 0.
pub fn clamp_thinking_budget(budget: u64, max_tokens: u64) -> u64 {
    budget
        .min(max_tokens / 2)
        .max(1024)
        .min(max_tokens.saturating_sub(1).max(1))
}

/// Resolve the qwen ≥3.8 reasoning tier from an Anthropic-format request body.
///
/// Tri-state: `None` means no thinking signal is present — the caller must not
/// inject anything (vendor default applies). Unlike `resolve_reasoning_effort`,
/// `thinking:{type:"disabled"}` yields `Some("low")` (thinking-only model), and
/// budget boundaries follow the vendor's tier mapping (≤4096 low, ≤16384
/// medium, else xhigh).
pub fn qwen_reasoning_effort_from_anthropic_body(body: &Value) -> Option<&'static str> {
    // --- Priority 1: explicit output_config.effort ---
    if let Some(effort) = body
        .pointer("/output_config/effort")
        .and_then(|v| v.as_str())
    {
        return Some(map_effort_to_qwen_tier(effort));
    }

    // --- Priority 2: thinking.type + budget_tokens ---
    let thinking = body.get("thinking")?;
    match thinking.get("type").and_then(|t| t.as_str()) {
        Some("adaptive") => Some("xhigh"),
        Some("enabled") => {
            let budget = thinking.get("budget_tokens").and_then(|b| b.as_u64());
            Some(match budget {
                Some(b) if b <= 4_096 => "low",
                Some(b) if b <= 16_384 => "medium",
                _ => "xhigh", // >16384 or enabled without budget (vendor default)
            })
        }
        Some("disabled") => Some("low"), // thinking-only model: disable → lowest tier
        _ => None,
    }
}

/// Sanitize an Anthropic-format passthrough body for qwen ≥3.8 upstreams
/// (Bailian Messages gateway): thinking is always on and cannot be disabled,
/// so rewrite an explicit `thinking:{type:"disabled"}` to the low tier.
/// Everything else passes through untouched — the gateway maps standard
/// `thinking` params (budget_tokens) to its internal tiers itself. Never
/// injects a top-level `reasoning_effort` (not documented for Messages API).
/// Non-qwen models are returned byte-identical.
///
/// The rewritten `thinking` object is mutated in place so any sibling fields
/// survive, and its low-tier budget is clamped against the body's `max_tokens`
/// (Anthropic 400s on `budget_tokens >= max_tokens`). An absent `max_tokens`
/// leaves the raw low-tier budget — the body is malformed either way (Claude
/// requires it), so upstream's own rejection is the correct signal.
pub fn sanitize_anthropic_body_for_qwen_reasoning(mut body: Value) -> Value {
    let is_qwen = body
        .get("model")
        .and_then(|m| m.as_str())
        .is_some_and(is_qwen_reasoning_effort_model);
    if !is_qwen {
        return body;
    }
    let disabled = body.pointer("/thinking/type").and_then(|t| t.as_str()) == Some("disabled");
    if disabled {
        let mut budget = qwen_tier_to_budget("low");
        if let Some(max_tokens) = body.get("max_tokens").and_then(|v| v.as_u64()) {
            budget = clamp_thinking_budget(budget, max_tokens);
        }
        // Mutate in place to preserve sibling fields on `thinking`
        // (`disabled == true` implies thinking is an object; the fallback arm
        // is defensive only).
        if let Some(thinking) = body.get_mut("thinking") {
            thinking["type"] = json!("enabled");
            thinking["budget_tokens"] = json!(budget);
        } else {
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
        }
    }
    body
}

/// Clamp a native OpenAI Responses body's `reasoning.effort` for qwen ≥3.8
/// gateways (native passthrough path).
///
/// - `reasoning` absent → untouched (vendor default xhigh)
/// - `reasoning: null` → explicit disable intent on a thinking-only model →
///   `{"effort": "low"}`
/// - `reasoning.effort` present → mapped through `map_effort_to_qwen_tier`
///   (none/minimal→low, high/max→xhigh); only `effort` is mutated, sibling
///   fields (e.g. `summary`, used by Codex to request reasoning-summary
///   events) survive the rewrite
/// - `reasoning` object without `effort` → untouched (vendor default)
pub fn sanitize_responses_reasoning_for_qwen(body: &mut Value) {
    let is_qwen = body
        .get("model")
        .and_then(|m| m.as_str())
        .is_some_and(is_qwen_reasoning_effort_model);
    if !is_qwen {
        return;
    }
    let Some(reasoning) = body.get_mut("reasoning") else {
        return;
    };
    if reasoning.is_null() {
        // null carries no siblings — a wholesale replace is fine.
        *reasoning = json!({ "effort": "low" });
        return;
    }
    // Map into a &'static str first so the immutable borrow ends before the
    // in-place write; mutating only `effort` preserves sibling fields.
    let mapped = reasoning
        .get("effort")
        .and_then(|v| v.as_str())
        .map(map_effort_to_qwen_tier);
    if let Some(effort) = mapped {
        reasoning["effort"] = json!(effort);
    }
}

/// Anthropic 请求 → OpenAI Chat Completions 请求
///
/// 转换工具库 API：当前无生产调用方（连通性检查不再发真实请求，曾是其唯一 crate 内
/// 消费者），但保留其转换逻辑与下方测试套件，供代理转换路径复用 / 未来接线。
#[allow(dead_code)]
pub fn anthropic_to_openai(body: Value) -> Result<Value, ProxyError> {
    anthropic_to_openai_with_reasoning_content(body, false)
}

/// Anthropic 请求 → OpenAI Chat Completions 请求
///
/// `preserve_reasoning_content` 仅用于明确需要 Moonshot/Kimi/DeepSeek
/// `reasoning_content` 兼容字段的 provider。默认转换保持通用 OpenAI-compatible
/// 请求体，避免向严格后端发送未知字段。
pub fn anthropic_to_openai_with_reasoning_content(
    body: Value,
    preserve_reasoning_content: bool,
) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    let mut messages = Vec::new();

    // 处理 system prompt
    if let Some(system) = body.get("system") {
        if let Some(text) = system.as_str() {
            let text = strip_leading_anthropic_billing_header(text);
            if !text.is_empty() {
                messages.push(json!({"role": "system", "content": text}));
            }
        } else if let Some(arr) = system.as_array() {
            for msg in arr {
                if let Some(text) = msg.get("text").and_then(|t| t.as_str()) {
                    let text = strip_leading_anthropic_billing_header(text);
                    if text.is_empty() {
                        continue;
                    }
                    messages.push(json!({"role": "system", "content": text}));
                }
            }
        }
    }

    // 转换 messages
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");
            let converted = convert_message_to_openai(role, content, preserve_reasoning_content)?;
            messages.extend(converted);
        }
    }

    normalize_openai_system_messages(&mut messages);
    result["messages"] = json!(messages);

    // 转换参数 — o-series 模型需要 max_completion_tokens
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");
    if let Some(v) = body.get("max_tokens") {
        if is_openai_o_series(model) {
            result["max_completion_tokens"] = v.clone();
        } else {
            result["max_tokens"] = v.clone();
        }
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stop_sequences") {
        result["stop"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // Map Anthropic thinking → OpenAI reasoning_effort
    if supports_reasoning_effort(model) {
        // qwen ≥3.8 uses its own tier resolver: disable signals map to "low"
        // (thinking-only model) and high/max clamp to xhigh.
        let effort = if is_qwen_reasoning_effort_model(model) {
            qwen_reasoning_effort_from_anthropic_body(&body)
        } else {
            resolve_reasoning_effort(&body)
        };
        if let Some(effort) = effort {
            result["reasoning_effort"] = json!(effort);
        }
    }

    // 转换 tools (过滤 BatchTool)
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let openai_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                        "description": t.get("description"),
                        "parameters": clean_schema(t.get("input_schema").cloned().unwrap_or(json!({})))
                    }
                })
            })
            .collect();

        if !openai_tools.is_empty() {
            result["tools"] = json!(openai_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice_to_chat(v);
    }

    Ok(result)
}

/// 为 OpenAI Chat Completions 流式请求注入 `stream_options.include_usage`。
///
/// OpenAI 兼容上游在流式下默认不在 SSE 里返回 usage，必须显式声明 include_usage
/// 才会在末尾吐 usage chunk。缺这一注入会导致流式请求的 token/成本/缓存全部漏记
/// （input/output/cache 全为 0）。保留客户端可能透传的其它 stream_options 字段，
/// 仅补 include_usage；非流式请求不动。
///
/// 由 Claude→openai_chat（claude.rs）与 Codex Responses→Chat（transform_codex_chat.rs）
/// 两条转换路径共用，确保两个客户端方向行为一致。
pub(crate) fn inject_openai_stream_include_usage(result: &mut Value) {
    let is_stream = result
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_stream {
        return;
    }
    match result.get_mut("stream_options") {
        Some(Value::Object(opts)) => {
            opts.insert("include_usage".to_string(), json!(true));
        }
        _ => {
            result["stream_options"] = json!({ "include_usage": true });
        }
    }
}

/// Translate an Anthropic `tool_choice` into the OpenAI Chat Completions form.
///
/// Anthropic forms:
///   "auto" / "any" / "none"           (string enum)
///   {"type": "auto" | "any" | "none"}
///   {"type": "tool", "name": "<X>"}
///
/// OpenAI Chat forms:
///   "auto" / "none" / "required"      (note: no "any" — use "required")
///   {"type": "function", "function": {"name": "<X>"}}
///
/// The Responses API uses a flatter `{"type":"function","name":"X"}` selector,
/// so it has a sibling `map_tool_choice_to_responses` in `transform_responses.rs`.
/// Keep the two in sync.
fn map_tool_choice_to_chat(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(s) => match s.as_str() {
            "any" => json!("required"),
            _ => json!(s),
        },
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            Some("any") => json!("required"),
            Some("auto") => json!("auto"),
            Some("none") => json!("none"),
            Some("tool") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({
                    "type": "function",
                    "function": { "name": name }
                })
            }
            _ => tool_choice.clone(),
        },
        _ => tool_choice.clone(),
    }
}

fn normalize_openai_system_messages(messages: &mut Vec<Value>) {
    let system_count = messages
        .iter()
        .filter(|message| message.get("role").and_then(|value| value.as_str()) == Some("system"))
        .count();

    if system_count == 0 {
        return;
    }

    if system_count == 1 {
        if let Some(index) = messages.iter().position(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("system")
        }) {
            if index > 0 {
                let message = messages.remove(index);
                messages.insert(0, message);
            }
        }
        return;
    }

    let mut parts = Vec::new();
    messages.retain(|message| {
        if message.get("role").and_then(|value| value.as_str()) != Some("system") {
            return true;
        }

        match message.get("content") {
            Some(Value::String(text)) if !text.is_empty() => parts.push(text.clone()),
            Some(Value::Array(content_parts)) => {
                let text = content_parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            _ => {}
        }

        false
    });

    if !parts.is_empty() {
        messages.insert(0, json!({"role": "system", "content": parts.join("\n")}));
    }
}

/// 转换单条消息到 OpenAI 格式（可能产生多条消息）
fn convert_message_to_openai(
    role: &str,
    content: Option<&Value>,
    preserve_reasoning_content: bool,
) -> Result<Vec<Value>, ProxyError> {
    let mut result = Vec::new();

    let content = match content {
        Some(c) => c,
        None => {
            result.push(json!({"role": role, "content": null}));
            return Ok(result);
        }
    };

    // 字符串内容
    if let Some(text) = content.as_str() {
        result.push(json!({"role": role, "content": text}));
        return Ok(result);
    }

    // 数组内容（多模态/工具调用）
    if let Some(blocks) = content.as_array() {
        let mut content_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut pending_tool_media = Vec::new();
        // reasoning_parts: 仅在兼容 Moonshot/Kimi/DeepSeek thinking tool-call 路径时
        // 生成 reasoning_content，通用 OpenAI-compatible 路径不发送该非标准字段。
        let mut reasoning_parts = Vec::new();

        for block in blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        content_parts.push(json!({"type": "text", "text": text}));
                    }
                }
                "image" => {
                    if let Some(image) =
                        chat_media_part_from_tool_part(block, ToolMediaScope::ImagesOnly)
                    {
                        content_parts.push(image);
                    }
                }
                "tool_use" => {
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": canonical_json_string(&input)
                        }
                    }));
                }
                "tool_result" => {
                    // tool_result 变成单独的 tool role 消息
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    let content_val = block.get("content");
                    let media_plan = content_val.cloned().and_then(plan_chat_tool_output_media);
                    let content_str = if let Some(media_plan) = media_plan {
                        queue_chat_tool_output_media(
                            &mut pending_tool_media,
                            tool_use_id,
                            media_plan.media_parts,
                        );
                        media_plan.tool_content
                    } else {
                        // Keep the no-media representation exactly equal to
                        // the legacy converter for prompt-cache stability.
                        match content_val {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) => canonical_json_string(v),
                            None => String::new(),
                        }
                    };
                    result.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content_str
                    }));
                }
                "thinking" => {
                    // 提取 thinking 内容，后续可作为 reasoning_content 传给需要它的上游。
                    if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                        if !thinking.is_empty() {
                            reasoning_parts.push(thinking.to_string());
                        }
                    }
                }
                "redacted_thinking" if preserve_reasoning_content => {
                    // Claude Code encrypts historical thinking into redacted_thinking blocks.
                    // MiMo/DeepSeek require non-empty reasoning_content on assistant tool-call
                    // messages, so inject a minimal placeholder when the real content is
                    // unavailable. Skip when preserve_reasoning_content is off (generic
                    // OpenAI-compatible path).
                    reasoning_parts.push("[redacted thinking]".to_string());
                }
                _ => {}
            }
        }

        // Chat tool messages cannot carry image parts. Keep parallel tool
        // results adjacent, then present all extracted media in one user turn
        // before any ordinary message content from the same Anthropic turn.
        flush_pending_chat_tool_media(&mut result, &mut pending_tool_media);

        // 添加带内容和/或工具调用的消息
        if !content_parts.is_empty() || !tool_calls.is_empty() {
            let mut msg = json!({"role": role});

            // 内容处理
            if content_parts.is_empty() {
                msg["content"] = Value::Null;
            } else if content_parts.len() == 1 {
                // 单 text block 简化为纯字符串
                if let Some(text) = content_parts[0].get("text") {
                    msg["content"] = text.clone();
                } else {
                    msg["content"] = json!(content_parts);
                }
            } else {
                msg["content"] = json!(content_parts);
            }

            // 工具调用
            if !tool_calls.is_empty() {
                msg["tool_calls"] = json!(tool_calls);
            }

            if preserve_reasoning_content && role == "assistant" && !tool_calls.is_empty() {
                let reasoning_content = if reasoning_parts.is_empty() {
                    "tool call".to_string()
                } else {
                    reasoning_parts.join("\n")
                };
                msg["reasoning_content"] = json!(reasoning_content);
            }

            result.push(msg);
        }

        return Ok(result);
    }

    // 其他情况直接透传
    result.push(json!({"role": role, "content": content}));
    Ok(result)
}

/// 清理工具参数的 JSON schema，并为根 schema 补齐 OpenAI 要求的 object 类型。
pub fn clean_schema(schema: Value) -> Value {
    clean_schema_inner(schema, true)
}

fn clean_schema_inner(mut schema: Value, is_root: bool) -> Value {
    if let Some(obj) = schema.as_object_mut() {
        let missing_type = is_root && !obj.contains_key("type");
        if missing_type {
            obj.insert("type".to_string(), json!("object"));
        }
        if missing_type && !obj.contains_key("properties") {
            obj.insert("properties".to_string(), json!({}));
        }

        // 移除 "format": "uri"
        if obj.get("format").and_then(|v| v.as_str()) == Some("uri") {
            obj.remove("format");
        }

        // 递归清理嵌套 schema
        if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
            for (_, value) in properties.iter_mut() {
                *value = clean_schema_inner(value.clone(), false);
            }
        }

        if let Some(items) = obj.get_mut("items") {
            *items = clean_schema_inner(items.clone(), false);
        }
    }
    schema
}

/// OpenAI 响应 → Anthropic 响应
pub fn openai_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    let choices = body
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| ProxyError::TransformError("No choices in response".to_string()))?;

    let choice = choices
        .first()
        .ok_or_else(|| ProxyError::TransformError("Empty choices array".to_string()))?;

    let message = choice
        .get("message")
        .ok_or_else(|| ProxyError::TransformError("No message in choice".to_string()))?;

    let mut content = Vec::new();
    let mut has_tool_use = false;

    // DeepSeek provider 会把思考内容放在 message.reasoning_content。
    if let Some(reasoning_content) = message.get("reasoning_content").and_then(|r| r.as_str()) {
        if !reasoning_content.is_empty() {
            content.push(json!({"type": "thinking", "thinking": reasoning_content}));
        }
    }

    // 文本/拒绝内容
    if let Some(msg_content) = message.get("content") {
        if let Some(text) = msg_content.as_str() {
            if !text.is_empty() {
                content.push(json!({"type": "text", "text": text}));
            }
        } else if let Some(parts) = msg_content.as_array() {
            for part in parts {
                let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match part_type {
                    "text" | "output_text" => {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                content.push(json!({"type": "text", "text": text}));
                            }
                        }
                    }
                    "refusal" => {
                        if let Some(refusal) = part.get("refusal").and_then(|r| r.as_str()) {
                            if !refusal.is_empty() {
                                content.push(json!({"type": "text", "text": refusal}));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // Some providers put refusal at message-level.
    if let Some(refusal) = message.get("refusal").and_then(|r| r.as_str()) {
        if !refusal.is_empty() {
            content.push(json!({"type": "text", "text": refusal}));
        }
    }

    // 工具调用（tool_calls）
    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        if !tool_calls.is_empty() {
            has_tool_use = true;
        }
        for tc in tool_calls {
            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let empty_obj = json!({});
            let func = tc.get("function").unwrap_or(&empty_obj);
            let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args_str = func
                .get("arguments")
                .and_then(|a| a.as_str())
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

            content.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }
    // 兼容旧格式（function_call）
    if !has_tool_use {
        if let Some(function_call) = message.get("function_call") {
            let id = function_call
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("");
            let name = function_call
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let has_arguments = function_call.get("arguments").is_some();

            let input = match function_call.get("arguments") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
                Some(v @ Value::Object(_)) | Some(v @ Value::Array(_)) => v.clone(),
                _ => json!({}),
            };

            if !name.is_empty() || has_arguments {
                content.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input
                }));
                has_tool_use = true;
            }
        }
    }

    // 映射 finish_reason → stop_reason
    let stop_reason = choice
        .get("finish_reason")
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" | "function_call" => "tool_use",
            "content_filter" => "end_turn",
            other => {
                log::warn!(
                    "[Claude/OpenAI] Unknown finish_reason in non-streaming response: {other}"
                );
                "end_turn"
            }
        })
        .or(if has_tool_use { Some("tool_use") } else { None });

    // usage — map cache tokens from OpenAI format to Anthropic format
    let usage = body.get("usage").cloned().unwrap_or(json!({}));
    // OpenAI prompt_tokens 含缓存命中，Anthropic input_tokens 不含 → 减去 cache_read 与
    // cache_creation，使 input 成为 fresh input。本路径以 app_type="claude" 记账（calculator
    // 不再扣减），若不减则缓存会被计入 input 与各 cache 桶两次。三桶互斥，恒等：
    // input + cache_read + cache_creation == prompt_tokens（inclusive 上游）。
    // 与流式 build_anthropic_usage_json (#2774) 及 transform_gemini 的 saturating_sub 对称。
    // 最终 cache_read/cache_creation：直传字段优先于 OpenAI nested details。
    let cached = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            usage
                .pointer("/prompt_tokens_details/cache_write_tokens")
                .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .saturating_sub(cached)
        .saturating_sub(cache_creation) as u32;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });

    if cached > 0 {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    if cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
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
    fn test_anthropic_to_openai_simple() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["model"], "claude-3-opus");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_to_openai_with_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_strips_leading_billing_header_from_system_string() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nYou are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_strips_billing_header_from_system_array_parts() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n"},
                {"type": "text", "text": "Stable prompt"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(result["messages"][0]["content"], "Stable prompt");
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_preserves_prompt_after_billing_header_in_same_part() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.119.47e; cc_entrypoint=sdk-cli; cch=a7754;\n\nStable prompt part 1"},
                {"type": "text", "text": "Stable prompt part 2"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "Stable prompt part 1\nStable prompt part 2"
        );
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_keeps_non_leading_billing_header_text() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": "Keep this literal:\nx-anthropic-billing-header: example",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "Keep this literal:\nx-anthropic-billing-header: example"
        );
    }

    #[test]
    fn test_anthropic_to_openai_with_tools() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(
            result["tools"][0]["function"]["parameters"]["type"],
            json!("object")
        );
        assert_eq!(
            result["tools"][0]["function"]["parameters"]["properties"]["location"]["type"],
            json!("string")
        );
    }

    #[test]
    fn test_anthropic_to_openai_defaults_missing_tool_schema_type() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let parameters = &result["tools"][0]["function"]["parameters"];
        assert_eq!(parameters["type"], json!("object"));
        assert_eq!(
            parameters["properties"]["location"]["type"],
            json!("string")
        );
    }

    #[test]
    fn test_anthropic_to_openai_defaults_empty_tool_schema() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Do work"}],
            "tools": [{"name": "do_work", "input_schema": {}}]
        });

        let result = anthropic_to_openai(input).unwrap();
        let parameters = &result["tools"][0]["function"]["parameters"];
        assert_eq!(parameters, &json!({"type": "object", "properties": {}}));
    }

    #[test]
    fn test_clean_schema_only_defaults_root_to_object() {
        let schema = json!({
            "properties": {
                "nullable_value": {
                    "anyOf": [{"type": "string"}, {"type": "null"}]
                },
                "list": {
                    "items": {"type": "string"}
                }
            }
        });

        let result = clean_schema(schema);
        assert_eq!(result["type"], json!("object"));
        assert_eq!(
            result["properties"]["nullable_value"],
            json!({"anyOf": [{"type": "string"}, {"type": "null"}]})
        );
        assert_eq!(
            result["properties"]["list"],
            json!({"items": {"type": "string"}})
        );
    }

    #[test]
    fn test_anthropic_to_openai_strips_cache_control_from_merged_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Be concise.", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"].as_array().unwrap().len(), 2);
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are Claude Code.\nBe concise."
        );
        assert!(result["messages"][0].get("cache_control").is_none());
        assert_eq!(result["messages"][1]["role"], "user");
    }

    #[test]
    fn test_anthropic_to_openai_strips_cache_control_from_mixed_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Be concise."}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are Claude Code.\nBe concise."
        );
        assert!(result["messages"][0].get("cache_control").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_strips_cache_control_from_conflicting_system() {
        let input = json!({
            "model": "claude-3-sonnet",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "Be concise.", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are Claude Code.\nBe concise."
        );
        assert!(result["messages"][0].get("cache_control").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_tool_use() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me check"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
        assert!(msg.get("reasoning_content").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_tool_use_preserves_reasoning_content() {
        let input = json!({
            "model": "kimi-k2.6",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should call the tool."},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["reasoning_content"], "I should call the tool.");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_tool_use_injects_placeholder_reasoning_content_when_missing() {
        let input = json!({
            "model": "kimi-k2.6",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["reasoning_content"], "tool call");
        assert!(msg.get("tool_calls").is_some());
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_tool_use_uses_redacted_thinking_placeholder() {
        let input = json!({
            "model": "mimo-v2.5-pro",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "redacted_thinking", "data": "opaque"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai_with_reasoning_content(input, true).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["reasoning_content"], "[redacted thinking]");
        assert_eq!(msg["tool_calls"][0]["id"], "call_123");
    }

    #[test]
    fn test_anthropic_to_openai_does_not_emit_reasoning_content_by_default() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "I should call the tool."},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert!(msg.get("tool_calls").is_some());
        assert!(msg.get("reasoning_content").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_skips_thinking_only_message() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "No visible content yet."}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_anthropic_to_openai_tool_result() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_123", "content": "Sunny, 25°C"}
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let msg = &result["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "call_123");
        assert_eq!(msg["content"], "Sunny, 25°C");
    }

    #[test]
    fn test_anthropic_to_openai_no_media_tool_results_keep_legacy_representation() {
        let raw_json_string = "{ \"status\": \"ok\", \"count\": 2 }";
        let input = json!({
            "model": "claude-3-opus",
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_string",
                        "content": raw_json_string
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_array",
                        "content": [{"type": "text", "text": "plain"}]
                    }
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["content"], raw_json_string);
        assert_eq!(
            messages[1]["content"],
            canonical_json_string(&json!([{"type": "text", "text": "plain"}]))
        );
    }

    #[test]
    fn test_anthropic_to_openai_moves_tool_result_image_to_user_message() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_image",
                    "content": [
                        {"type": "text", "text": "caption"},
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": "CLAUDE_CHAT_IMAGE_SENTINEL"
                            },
                            "cache_control": {"type": "ephemeral"},
                            "prompt_cache_breakpoint": true
                        }
                    ]
                }]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["tool_call_id"], "call_image");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("tool result media moved"));
        assert!(!messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("CLAUDE_CHAT_IMAGE_SENTINEL"));
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(
            messages[1]["content"][0]["text"],
            "[cc-switch: media output of tool call call_image]"
        );
        assert_eq!(messages[1]["content"][1]["type"], "image_url");
        assert!(messages[1]["content"][1].get("cache_control").is_none());
        assert!(messages[1]["content"][1]
            .get("prompt_cache_breakpoint")
            .is_none());
        assert_eq!(
            messages[1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,CLAUDE_CHAT_IMAGE_SENTINEL"
        );
    }

    #[test]
    fn test_anthropic_to_openai_batches_parallel_tool_result_media() {
        let input = json!({
            "model": "claude-3-opus",
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_1",
                        "content": [{
                            "type": "image",
                            "source": {"type": "base64", "media_type": "image/png", "data": "ONE"}
                        }]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_2",
                        "content": [{
                            "type": "image",
                            "source": {"type": "base64", "media_type": "image/jpeg", "data": "TWO"}
                        }]
                    }
                ]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        let messages = result["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn test_anthropic_to_openai_maps_remote_image_source() {
        let input = json!({
            "model": "claude-3-opus",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {
                        "type": "url",
                        "url": "https://example.com/image.png"
                    },
                    "cache_control": {"type": "ephemeral"},
                    "prompt_cache_breakpoint": true
                }]
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(
            result["messages"][0]["content"][0]["image_url"]["url"],
            "https://example.com/image.png"
        );
        assert!(result["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert!(result["messages"][0]["content"][0]
            .get("prompt_cache_breakpoint")
            .is_none());
    }

    #[test]
    fn test_openai_to_anthropic_simple() {
        let input = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["id"], "chatcmpl-123");
        assert_eq!(result["type"], "message");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello!");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_openai_to_anthropic_preserves_id_for_usage_dedup() {
        let input = json!({
            "id": "chatcmpl-claude-compatible",
            "object": "chat.completion",
            "model": "claude-sonnet-4-5",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        let usage = crate::proxy::usage::parser::TokenUsage::from_claude_response(&result)
            .expect("converted Anthropic response should parse usage");

        assert_eq!(
            usage.message_id.as_deref(),
            Some("chatcmpl-claude-compatible")
        );
        assert_eq!(
            usage.dedup_request_id(None),
            "session:chatcmpl-claude-compatible"
        );
    }

    #[test]
    fn test_openai_to_anthropic_with_tool_calls() {
        let input = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"location\": \"Tokyo\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_123");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_deepseek_reasoning_content_round_trips_for_tool_calls() {
        let upstream_response = json!({
            "id": "chatcmpl-deepseek",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "reasoning_content": "Need the current date before calling weather.",
                    "content": "Let me check the date first.",
                    "tool_calls": [{
                        "id": "call_date",
                        "type": "function",
                        "function": {"name": "get_date", "arguments": "{}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let anthropic_response = openai_to_anthropic(upstream_response).unwrap();
        assert_eq!(anthropic_response["content"][0]["type"], "thinking");
        assert_eq!(
            anthropic_response["content"][0]["thinking"],
            "Need the current date before calling weather."
        );
        assert_eq!(anthropic_response["content"][1]["type"], "text");
        assert_eq!(anthropic_response["content"][2]["type"], "tool_use");
        assert_eq!(anthropic_response["content"][2]["id"], "call_date");

        let follow_up_request = json!({
            "model": "deepseek-v4-flash",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": anthropic_response["content"].clone()
            }]
        });
        let replayed = anthropic_to_openai_with_reasoning_content(follow_up_request, true).unwrap();
        let msg = &replayed["messages"][0];

        assert_eq!(
            msg["reasoning_content"],
            "Need the current date before calling weather."
        );
        assert_eq!(msg["tool_calls"][0]["id"], "call_date");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "get_date");
    }

    #[test]
    fn test_model_passthrough() {
        // 格式转换层只做结构转换，模型映射由上游 proxy::model_mapper 处理
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["model"], "gpt-4o");
    }

    #[test]
    fn test_anthropic_to_openai_does_not_inject_prompt_cache_key() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("prompt_cache_key").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_strips_all_cache_control() {
        let input = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "System prompt", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
                ]
            }],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();
        // System message: no cache_control
        assert!(result["messages"][0].get("cache_control").is_none());
        // User message: content simplified to string (no cache_control → flat string)
        assert_eq!(result["messages"][1]["content"], "Hello");
        // Tool: no cache_control
        assert!(result["tools"][0].get("cache_control").is_none());
    }

    /// 精确复现 Issue #3805 报告的 400 错误场景:
    /// GLM/Qwen 等严格校验模型拒绝 cache_control 和 content 数组格式
    #[test]
    fn test_regression_gh3805_no_cache_control_leak_to_openai() {
        let input = json!({
            "model": "glm-5.1",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "You are helpful.", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
                ]}
            ],
            "tools": [{
                "name": "search",
                "description": "Search the web",
                "input_schema": {"type": "object"},
                "cache_control": {"type": "ephemeral"}
            }]
        });

        let result = anthropic_to_openai(input).unwrap();

        // 验证: messages 中不存在 cache_control
        for (i, msg) in result["messages"].as_array().unwrap().iter().enumerate() {
            assert!(
                msg.get("cache_control").is_none(),
                "messages[{i}] must not have cache_control"
            );
        }

        // 验证: content 中没有 cache_control
        for (i, msg) in result["messages"].as_array().unwrap().iter().enumerate() {
            if let Some(content) = msg.get("content") {
                assert!(
                    !content.is_array()
                        || content
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|part| part.get("cache_control").is_none()),
                    "messages[{i}] content parts must not have cache_control"
                );
            }
        }

        // 验证: system content 为纯字符串格式（不是数组）
        let sys_msg = &result["messages"][0];
        assert_eq!(sys_msg["role"], "system");
        assert!(
            sys_msg["content"].is_string(),
            "system content must be string, got: {}",
            sys_msg["content"]
        );

        // 验证: user content 为纯字符串格式（不是数组）
        let user_msg = &result["messages"][1];
        assert_eq!(user_msg["role"], "user");
        assert!(
            user_msg["content"].is_string(),
            "user content must be string, got: {}",
            user_msg["content"]
        );

        // 验证: tools 中不存在 cache_control
        if let Some(tools) = result["tools"].as_array() {
            for (i, tool) in tools.iter().enumerate() {
                assert!(
                    tool.get("cache_control").is_none(),
                    "tools[{i}] must not have cache_control"
                );
            }
        }
    }

    #[test]
    fn test_openai_to_anthropic_with_cache_tokens() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let result = openai_to_anthropic(input).unwrap();
        // prompt_tokens(100) 含 cached(80)，转换后 input 应为 fresh = 100 - 80 = 20
        assert_eq!(result["usage"]["input_tokens"], 20);
        assert_eq!(result["usage"]["output_tokens"], 50);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 80);
    }

    #[test]
    fn test_openai_to_anthropic_with_direct_cache_fields() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello!"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 20
            }
        });

        let result = openai_to_anthropic(input).unwrap();
        // cache_read(60)+cache_creation(20) 均从 prompt(100) 扣除，fresh = 100 - 60 - 20 = 20
        // 守恒：input(20) + cache_read(60) + cache_creation(20) == prompt(100)
        assert_eq!(result["usage"]["input_tokens"], 20);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 20);
    }

    #[test]
    fn test_openai_to_anthropic_clamps_input_when_cache_exceeds_prompt() {
        // prompt(100) < cache_read(60)+cache_creation(50)=110：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let input = json!({
            "id": "chatcmpl-uf",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "x"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 50
            }
        });
        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["usage"]["input_tokens"], 0);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 60);
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 50);
    }

    #[test]
    fn test_openai_to_anthropic_finish_reason_content_filter_maps_end_turn() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Blocked"},
                "finish_reason": "content_filter"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 1}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "end_turn");
    }

    #[test]
    fn test_openai_to_anthropic_with_legacy_function_call() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "function_call": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Tokyo\"}"
                    }
                },
                "finish_reason": "function_call"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn test_openai_to_anthropic_with_content_parts_and_refusal() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "refusal", "refusal": "I can't do that"}
                    ]
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });

        let result = openai_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello");
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "I can't do that");
    }

    #[test]
    fn test_is_openai_o_series() {
        assert!(is_openai_o_series("o1"));
        assert!(is_openai_o_series("o1-preview"));
        assert!(is_openai_o_series("o1-mini"));
        assert!(is_openai_o_series("o3"));
        assert!(is_openai_o_series("o3-mini"));
        assert!(is_openai_o_series("o4-mini"));
        assert!(!is_openai_o_series("gpt-4o"));
        assert!(!is_openai_o_series("openai-gpt"));
        assert!(!is_openai_o_series("o"));
        assert!(!is_openai_o_series(""));
    }

    #[test]
    fn test_supports_reasoning_effort() {
        assert!(supports_reasoning_effort("o1"));
        assert!(supports_reasoning_effort("o3-mini"));
        assert!(supports_reasoning_effort("gpt-5"));
        assert!(supports_reasoning_effort("gpt-5.4"));
        assert!(supports_reasoning_effort("gpt-5-codex"));
        assert!(supports_reasoning_effort("grok-4.5"));
        assert!(supports_reasoning_effort("grok-build-0.1"));
        assert!(!supports_reasoning_effort("gpt-4o"));
        assert!(!supports_reasoning_effort("claude-sonnet-4-6"));
    }

    // ── resolve_reasoning_effort unit tests ──

    #[test]
    fn test_output_config_low_maps_to_reasoning_effort_low() {
        let body = json!({"output_config": {"effort": "low"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("low"));
    }

    #[test]
    fn test_output_config_medium_maps_to_reasoning_effort_medium() {
        let body = json!({"output_config": {"effort": "medium"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("medium"));
    }

    #[test]
    fn test_output_config_high_maps_to_reasoning_effort_high() {
        let body = json!({"output_config": {"effort": "high"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));
    }

    #[test]
    fn test_output_config_max_maps_to_reasoning_effort_xhigh() {
        let body = json!({"output_config": {"effort": "max"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("xhigh"));
    }

    #[test]
    fn test_output_config_takes_priority_over_thinking() {
        // Even with thinking.adaptive present, explicit effort wins
        let body = json!({
            "output_config": {"effort": "low"},
            "thinking": {"type": "adaptive"}
        });
        assert_eq!(resolve_reasoning_effort(&body), Some("low"));
    }

    #[test]
    fn test_output_config_unknown_value_no_reasoning_effort() {
        let body = json!({"output_config": {"effort": "turbo"}});
        assert_eq!(resolve_reasoning_effort(&body), None);
    }

    #[test]
    fn test_thinking_enabled_small_budget_maps_low() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 1024}});
        assert_eq!(resolve_reasoning_effort(&body), Some("low"));
    }

    #[test]
    fn test_thinking_enabled_medium_budget_maps_medium() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 8000}});
        assert_eq!(resolve_reasoning_effort(&body), Some("medium"));
    }

    #[test]
    fn test_thinking_enabled_large_budget_maps_high() {
        let body = json!({"thinking": {"type": "enabled", "budget_tokens": 32000}});
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));
    }

    #[test]
    fn test_thinking_enabled_without_budget_maps_high() {
        let body = json!({"thinking": {"type": "enabled"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("high"));
    }

    #[test]
    fn test_thinking_adaptive_maps_xhigh() {
        let body = json!({"thinking": {"type": "adaptive"}});
        assert_eq!(resolve_reasoning_effort(&body), Some("xhigh"));
    }

    #[test]
    fn test_thinking_disabled_no_reasoning_effort() {
        let body = json!({"thinking": {"type": "disabled"}});
        assert_eq!(resolve_reasoning_effort(&body), None);
    }

    #[test]
    fn test_no_thinking_field_no_reasoning_effort() {
        let body = json!({"messages": [{"role": "user", "content": "Hello"}]});
        assert_eq!(resolve_reasoning_effort(&body), None);
    }

    // ── Integration: anthropic_to_openai with resolve_reasoning_effort ──

    #[test]
    fn test_non_reasoning_model_no_reasoning_effort() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_reasoning_model_with_output_config_effort() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "output_config": {"effort": "medium"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "medium");
    }

    #[test]
    fn test_reasoning_model_with_output_config_max() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "output_config": {"effort": "max"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "xhigh");
    }

    #[test]
    fn test_reasoning_model_thinking_enabled_small_budget() {
        let input = json!({
            "model": "o3",
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "low");
    }

    #[test]
    fn test_reasoning_model_thinking_adaptive() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "thinking": {"type": "adaptive"},
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "xhigh");
    }

    #[test]
    fn test_reasoning_model_no_thinking_no_effort() {
        let input = json!({
            "model": "gpt-5.4",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_o_series_max_completion_tokens() {
        for model in &["o1", "o3-mini", "o4-mini"] {
            let input = json!({
                "model": model,
                "max_tokens": 4096,
                "messages": [{"role": "user", "content": "Hello"}]
            });

            let result = anthropic_to_openai(input).unwrap();
            assert!(
                result.get("max_tokens").is_none(),
                "{model} should not have max_tokens"
            );
            assert_eq!(
                result["max_completion_tokens"], 4096,
                "{model} should use max_completion_tokens"
            );
        }
    }

    #[test]
    fn test_anthropic_to_openai_non_o_series_keeps_max_tokens() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["max_tokens"], 1024);
        assert!(result.get("max_completion_tokens").is_none());
    }

    fn run_tool_choice(value: Value) -> Value {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{
                "name": "search",
                "description": "search the web",
                "input_schema": {"type": "object", "properties": {}}
            }],
            "tool_choice": value,
        });
        anthropic_to_openai(input).unwrap()["tool_choice"].clone()
    }

    #[test]
    fn tool_choice_string_any_maps_to_required() {
        assert_eq!(run_tool_choice(json!("any")), json!("required"));
    }

    #[test]
    fn tool_choice_string_auto_and_none_pass_through() {
        assert_eq!(run_tool_choice(json!("auto")), json!("auto"));
        assert_eq!(run_tool_choice(json!("none")), json!("none"));
    }

    #[test]
    fn tool_choice_object_any_maps_to_required() {
        assert_eq!(run_tool_choice(json!({"type": "any"})), json!("required"));
    }

    #[test]
    fn tool_choice_object_auto_and_none_collapse_to_string() {
        assert_eq!(run_tool_choice(json!({"type": "auto"})), json!("auto"));
        assert_eq!(run_tool_choice(json!({"type": "none"})), json!("none"));
    }

    #[test]
    fn tool_choice_forced_tool_maps_to_nested_function_selector() {
        // Anthropic {"type":"tool","name":"X"} must become OpenAI Chat
        // {"type":"function","function":{"name":"X"}} — the *nested* form, not
        // the flat Responses-API form.
        assert_eq!(
            run_tool_choice(json!({"type": "tool", "name": "search"})),
            json!({"type": "function", "function": {"name": "search"}}),
        );
    }

    // ── qwen ≥3.8 reasoning_effort support ──

    #[test]
    fn test_parse_qwen_version() {
        assert_eq!(parse_qwen_version("qwen3.8-max-preview"), Some((3, 8)));
        assert_eq!(parse_qwen_version("qwen3.10-plus"), Some((3, 10)));
        assert_eq!(parse_qwen_version("qwen/qwen3.8-max-preview"), Some((3, 8)));
        assert_eq!(parse_qwen_version("Qwen3.9-Max"), Some((3, 9)));
        assert_eq!(parse_qwen_version("qwen4.0-max"), Some((4, 0)));
        assert_eq!(parse_qwen_version("qwen4-max"), Some((4, 0)));
        assert_eq!(parse_qwen_version("qwen3.7-max"), Some((3, 7)));
        assert_eq!(parse_qwen_version("qwen3-235b-a22b"), Some((3, 0)));
        assert_eq!(parse_qwen_version("qwen3-max-2026-01-23"), Some((3, 0)));
        assert_eq!(parse_qwen_version("qwen-max"), None);
        assert_eq!(parse_qwen_version("qwen-plus"), None);
        assert_eq!(parse_qwen_version("qwen-vl-max"), None);
        assert_eq!(parse_qwen_version("gpt-5"), None);
    }

    #[test]
    fn test_is_qwen_reasoning_effort_model_version_comparison() {
        // ≥3.8 qualify (numeric comparison, future-proof)
        assert!(is_qwen_reasoning_effort_model("qwen3.8-max-preview"));
        assert!(is_qwen_reasoning_effort_model("Qwen3.8-Max"));
        assert!(is_qwen_reasoning_effort_model("qwen/qwen3.8-max-preview"));
        assert!(is_qwen_reasoning_effort_model("qwen3.9-plus"));
        assert!(is_qwen_reasoning_effort_model("qwen3.10-max")); // numeric: 3.10 > 3.8
        assert!(is_qwen_reasoning_effort_model("qwen4.0-max"));
        assert!(is_qwen_reasoning_effort_model("qwen4-max"));
        // <3.8 or unversioned keep legacy behavior
        assert!(!is_qwen_reasoning_effort_model("qwen3.7-max"));
        assert!(!is_qwen_reasoning_effort_model("qwen3.5-plus"));
        assert!(!is_qwen_reasoning_effort_model("qwen3-235b-a22b"));
        assert!(!is_qwen_reasoning_effort_model("qwen3-max-2026-01-23"));
        assert!(!is_qwen_reasoning_effort_model("qwen-max"));
        assert!(!is_qwen_reasoning_effort_model("qwen-plus"));
        assert!(!is_qwen_reasoning_effort_model("qwen-vl-max"));
        assert!(!is_qwen_reasoning_effort_model("gpt-5"));
    }

    #[test]
    fn test_supports_reasoning_effort_includes_qwen38() {
        assert!(supports_reasoning_effort("qwen3.8-max-preview"));
        assert!(supports_reasoning_effort("qwen4.0-max"));
        assert!(!supports_reasoning_effort("qwen3.7-max"));
        assert!(!supports_reasoning_effort("qwen3-coder-plus"));
    }

    #[test]
    fn test_map_effort_to_qwen_tier() {
        // disable signals → low (thinking-only model, vendor errors on "off")
        assert_eq!(map_effort_to_qwen_tier("none"), "low");
        assert_eq!(map_effort_to_qwen_tier("off"), "low");
        assert_eq!(map_effort_to_qwen_tier("disabled"), "low");
        assert_eq!(map_effort_to_qwen_tier("minimal"), "low");
        assert_eq!(map_effort_to_qwen_tier("low"), "low");
        assert_eq!(map_effort_to_qwen_tier("medium"), "medium");
        // high/max clamp up to xhigh (qwen has no high tier)
        assert_eq!(map_effort_to_qwen_tier("high"), "xhigh");
        assert_eq!(map_effort_to_qwen_tier("xhigh"), "xhigh");
        assert_eq!(map_effort_to_qwen_tier("max"), "xhigh");
        assert_eq!(map_effort_to_qwen_tier("garbage"), "xhigh");
        assert_eq!(map_effort_to_qwen_tier(" HIGH "), "xhigh");
    }

    #[test]
    fn test_qwen_tier_to_budget() {
        assert_eq!(qwen_tier_to_budget("low"), 4_096);
        assert_eq!(qwen_tier_to_budget("medium"), 16_384);
        assert_eq!(qwen_tier_to_budget("xhigh"), 262_144);
    }

    #[test]
    fn test_qwen_reasoning_effort_from_anthropic_body_output_config_priority() {
        let body = json!({
            "output_config": {"effort": "high"},
            "thinking": {"type": "enabled", "budget_tokens": 2048}
        });
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), Some("xhigh"));

        let body = json!({"output_config": {"effort": "low"}});
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), Some("low"));

        let body = json!({"output_config": {"effort": "max"}});
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), Some("xhigh"));
    }

    #[test]
    fn test_qwen_reasoning_effort_from_anthropic_body_thinking_budget_boundaries() {
        // vendor boundaries: ≤4096 low, ≤16384 medium, else xhigh
        for (budget, expected) in [
            (1024, "low"),
            (4096, "low"),
            (4097, "medium"),
            (16384, "medium"),
            (16385, "xhigh"),
            (262144, "xhigh"),
        ] {
            let body = json!({"thinking": {"type": "enabled", "budget_tokens": budget}});
            assert_eq!(
                qwen_reasoning_effort_from_anthropic_body(&body),
                Some(expected),
                "budget={budget}"
            );
        }
    }

    #[test]
    fn test_qwen_reasoning_effort_from_anthropic_body_type_variants() {
        let body = json!({"thinking": {"type": "adaptive"}});
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), Some("xhigh"));

        // enabled without budget → vendor default tier
        let body = json!({"thinking": {"type": "enabled"}});
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), Some("xhigh"));

        // disabled → low (thinking-only model cannot turn off)
        let body = json!({"thinking": {"type": "disabled"}});
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), Some("low"));

        // no thinking signal → None (caller must not inject)
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&json!({})), None);
        let body = json!({"thinking": {"type": "unexpected"}});
        assert_eq!(qwen_reasoning_effort_from_anthropic_body(&body), None);
    }

    #[test]
    fn test_anthropic_to_openai_qwen38_disabled_maps_to_low() {
        let input = json!({
            "model": "qwen3.8-max-preview",
            "max_tokens": 8192,
            "thinking": {"type": "disabled"},
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = anthropic_to_openai(input).unwrap();
        assert_eq!(result["reasoning_effort"], "low");
        // qwen3.8 rejects reasoning_effort alongside thinking_budget, and
        // enable_thinking must never be forwarded for thinking-only models
        assert!(result.get("thinking_budget").is_none());
        assert!(result.get("enable_thinking").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_qwen38_effort_tiers() {
        for (thinking, expected) in [
            (json!({"type": "enabled", "budget_tokens": 2048}), "low"),
            (json!({"type": "enabled", "budget_tokens": 10000}), "medium"),
            (json!({"type": "enabled", "budget_tokens": 30000}), "xhigh"),
            (json!({"type": "adaptive"}), "xhigh"),
        ] {
            let input = json!({
                "model": "qwen3.8-max-preview",
                "max_tokens": 8192,
                "thinking": thinking,
                "messages": [{"role": "user", "content": "hello"}]
            });
            let result = anthropic_to_openai(input).unwrap();
            assert_eq!(result["reasoning_effort"], expected, "thinking={thinking}");
        }

        // output_config.effort high/max clamp to xhigh
        let input = json!({
            "model": "qwen3.8-max-preview",
            "output_config": {"effort": "max"},
            "messages": [{"role": "user", "content": "hello"}]
        });
        assert_eq!(anthropic_to_openai(input).unwrap()["reasoning_effort"], "xhigh");
    }

    #[test]
    fn test_anthropic_to_openai_qwen38_no_signal_no_injection() {
        let input = json!({
            "model": "qwen3.8-max-preview",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_pre_qwen38_unchanged() {
        // qwen3.7 does not match supports_reasoning_effort → no injection
        let input = json!({
            "model": "qwen3.7-max",
            "thinking": {"type": "enabled", "budget_tokens": 10000},
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_anthropic_to_openai_gpt5_disabled_regression() {
        // non-qwen behavior unchanged: disabled thinking → no reasoning_effort
        let input = json!({
            "model": "gpt-5",
            "thinking": {"type": "disabled"},
            "messages": [{"role": "user", "content": "hello"}]
        });
        let result = anthropic_to_openai(input).unwrap();
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_sanitize_anthropic_body_for_qwen_reasoning() {
        // qwen3.8 + disabled → rewritten to enabled + low-tier budget
        let body = json!({
            "model": "qwen3.8-max-preview",
            "thinking": {"type": "disabled"},
            "messages": []
        });
        let result = sanitize_anthropic_body_for_qwen_reasoning(body);
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 4096);
        assert!(result.get("reasoning_effort").is_none());

        // qwen3.8 + enabled → untouched (gateway maps budget to tier)
        let body = json!({
            "model": "qwen3.8-max-preview",
            "thinking": {"type": "enabled", "budget_tokens": 8000}
        });
        let result = sanitize_anthropic_body_for_qwen_reasoning(body.clone());
        assert_eq!(result, body);

        // qwen3.8 + no thinking → untouched (vendor default)
        let body = json!({"model": "qwen3.8-max-preview"});
        let result = sanitize_anthropic_body_for_qwen_reasoning(body.clone());
        assert_eq!(result, body);

        // future qwen4.0 also covered
        let body = json!({"model": "qwen4.0-max", "thinking": {"type": "disabled"}});
        let result = sanitize_anthropic_body_for_qwen_reasoning(body);
        assert_eq!(result["thinking"]["type"], "enabled");

        // non-qwen → byte-identical
        let body = json!({"model": "claude-sonnet-4-6", "thinking": {"type": "disabled"}});
        let result = sanitize_anthropic_body_for_qwen_reasoning(body.clone());
        assert_eq!(result, body);

        // pre-3.8 qwen → byte-identical (legacy enable_thinking behavior)
        let body = json!({"model": "qwen3.7-max", "thinking": {"type": "disabled"}});
        let result = sanitize_anthropic_body_for_qwen_reasoning(body.clone());
        assert_eq!(result, body);
    }

    #[test]
    fn test_sanitize_responses_reasoning_for_qwen() {
        // effort present → mapped
        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": {"effort": "high"}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "xhigh");

        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": {"effort": "none"}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "low");

        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": {"effort": "minimal"}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "low");

        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": {"effort": "medium"}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "medium");

        // reasoning: null = explicit disable intent → low
        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": null});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "low");

        // reasoning object without effort → untouched
        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": {}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"], json!({}));

        // no reasoning field → untouched
        let mut body = json!({"model": "qwen3.8-max-preview"});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert!(body.get("reasoning").is_none());

        // non-qwen → untouched
        let mut body = json!({"model": "gpt-5", "reasoning": {"effort": "none"}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "none");

        // pre-3.8 qwen → untouched
        let mut body = json!({"model": "qwen3.7-max", "reasoning": {"effort": "high"}});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "high");
    }

    #[test]
    fn test_sanitize_responses_reasoning_for_qwen_preserves_siblings() {
        // Only `effort` is rewritten; siblings (e.g. `summary`, used by Codex
        // to request reasoning-summary events) must survive.
        let mut body = json!({
            "model": "qwen3.8-max-preview",
            "reasoning": {"effort": "high", "summary": "auto", "verbosity": "medium"}
        });
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"]["effort"], "xhigh");
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert_eq!(body["reasoning"]["verbosity"], "medium");

        // null reasoning still becomes exactly {"effort":"low"} (no siblings
        // can exist on null).
        let mut body = json!({"model": "qwen3.8-max-preview", "reasoning": null});
        sanitize_responses_reasoning_for_qwen(&mut body);
        assert_eq!(body["reasoning"], json!({"effort": "low"}));
    }

    #[test]
    fn test_sanitize_anthropic_body_for_qwen_reasoning_clamps_to_max_tokens() {
        // Anthropic 400s on budget_tokens >= max_tokens: a disabled→enabled
        // rewrite must clamp the low-tier budget against the body's ceiling.
        let body = json!({
            "model": "qwen3.8-max-preview",
            "max_tokens": 4096,
            "thinking": {"type": "disabled"}
        });
        let result = sanitize_anthropic_body_for_qwen_reasoning(body);
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 2048); // 4096/2 ceiling

        let body = json!({
            "model": "qwen3.8-max-preview",
            "max_tokens": 2048,
            "thinking": {"type": "disabled"}
        });
        let result = sanitize_anthropic_body_for_qwen_reasoning(body);
        assert_eq!(result["thinking"]["budget_tokens"], 1024); // 1024 floor

        // max_tokens absent (malformed — Claude requires it) → raw low-tier
        // budget, upstream's own rejection is the correct signal.
        let body = json!({
            "model": "qwen3.8-max-preview",
            "thinking": {"type": "disabled"}
        });
        let result = sanitize_anthropic_body_for_qwen_reasoning(body);
        assert_eq!(result["thinking"]["budget_tokens"], 4096);

        // Sibling fields on `thinking` survive the in-place rewrite.
        let body = json!({
            "model": "qwen3.8-max-preview",
            "max_tokens": 8192,
            "thinking": {"type": "disabled", "display": "concise"}
        });
        let result = sanitize_anthropic_body_for_qwen_reasoning(body);
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 4096); // capped at 8192/2
        assert_eq!(result["thinking"]["display"], "concise");
    }

    #[test]
    fn test_clamp_thinking_budget() {
        // Value table
        assert_eq!(clamp_thinking_budget(262_144, 20_000), 10_000); // half ceiling
        assert_eq!(clamp_thinking_budget(16_384, 8_192), 4_096);
        assert_eq!(clamp_thinking_budget(4_096, 4_096), 2_048);
        assert_eq!(clamp_thinking_budget(4_096, 2_048), 1_024); // floor
        assert_eq!(clamp_thinking_budget(4_096, 1_000), 999); // strict < max_tokens wins over floor
        assert_eq!(clamp_thinking_budget(4_096, 2), 1); // degenerate, never 0
        assert_eq!(clamp_thinking_budget(4_096, 0), 1);
        assert_eq!(clamp_thinking_budget(0, 0), 1);

        // Invariant: for max_tokens >= 2 the result is always < max_tokens.
        for max_tokens in [2u64, 3, 999, 1_000, 2_048, 4_096, 8_192, 20_000, 1_000_000] {
            for budget in [0u64, 1, 1_024, 4_096, 16_384, 262_144] {
                let out = clamp_thinking_budget(budget, max_tokens);
                assert!(out > 0, "budget={budget} max_tokens={max_tokens} → {out}");
                assert!(
                    out < max_tokens,
                    "budget={budget} max_tokens={max_tokens} → {out}"
                );
            }
        }
    }
}
