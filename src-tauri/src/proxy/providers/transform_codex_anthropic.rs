//! OpenAI Responses ↔ Anthropic Messages format conversion module (used when the Codex upstream is an Anthropic gateway)
//!
//! Scenario: The Codex CLI only speaks the OpenAI Responses protocol, while the
//! upstream AI gateway only offers the native Anthropic Messages protocol
//! (`/v1/messages`). This module converts the Responses request sent by Codex
//! into an Anthropic request, then converts the Anthropic response back into a
//! Responses response.
//!
//! The direction is exactly the mirror of `transform_responses.rs`:
//! - `transform_responses.rs`: Anthropic request → Responses request, Responses response → Anthropic response
//! - this module:               Responses request → Anthropic request, Anthropic response → Responses response

use super::transform_responses::sanitize_anthropic_tool_use_input;
use crate::proxy::error::ProxyError;
use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// Maps Codex's reasoning.effort to the token budget for Anthropic thinking.
///
/// Returning `None` indicates an unrecognized effort value—in that case extended
/// thinking should not be enabled (to avoid accidentally swallowing
/// temperature/top_p), keeping normal sampling.
pub(crate) fn effort_to_thinking_budget(effort: &str) -> Option<u64> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" | "low" => Some(2048),
        "medium" => Some(8192),
        "high" => Some(16384),
        "xhigh" | "max" => Some(24576),
        _ => None,
    }
}

/// Anthropic's stop_reason → Responses' (status, incomplete_details.reason)
pub(crate) fn map_anthropic_stop_reason_to_status(
    stop_reason: Option<&str>,
) -> (&'static str, Option<&'static str>) {
    match stop_reason {
        Some("max_tokens") => ("incomplete", Some("max_output_tokens")),
        // Safety refusal: report as incomplete to avoid Codex treating it as a normally-completed empty reply.
        Some("refusal") => ("incomplete", Some("content_filter")),
        // pause_turn is unreachable on this path (Codex requests do not declare Anthropic server-side tools);
        // if it does occur, log a warning and treat it as completed.
        Some("pause_turn") => {
            log::warn!("[Codex] Received unexpected Anthropic stop_reason=pause_turn, treating it as completed");
            ("completed", None)
        }
        _ => ("completed", None),
    }
}

/// Builds Responses usage from Anthropic usage.
///
/// Anthropic's `input_tokens` is the "cache-excluded" fresh input; OpenAI/Responses'
/// `input_tokens` includes cache hits. To keep downstream metering correct, this
/// adds them (symmetric to the subtraction done for the Claude side in
/// `transform_responses`):
///   input_tokens = input + cache_read
///   input_tokens_details.cached_tokens = cache_read
///
/// Note: **do not** fold `cache_creation` into `input_tokens`. The Codex billing
/// calculator (usage/calculator.rs) only subtracts `cache_read` for codex
/// (`billable = input - cache_read`), and separately lists cache-creation cost via
/// `cache_creation_input_tokens`; if creation were also added into
/// `input_tokens`, it would be double-charged at both the input price and the
/// cache-creation price.
pub(crate) fn build_responses_usage_from_anthropic(usage: Option<&Value>) -> Value {
    let u = match usage {
        Some(v) if v.is_object() => v,
        _ => {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0,
                "total_tokens": 0,
                "output_tokens_details": { "reasoning_tokens": 0 }
            })
        }
    };

    let fresh_input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read = u
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation = u
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let input_tokens = fresh_input.saturating_add(cache_read);
    let total_tokens = input_tokens
        .saturating_add(cache_creation)
        .saturating_add(output);

    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output,
        "total_tokens": total_tokens,
        "output_tokens_details": { "reasoning_tokens": 0 }
    });
    if cache_read > 0 {
        result["input_tokens_details"] = json!({ "cached_tokens": cache_read });
    }
    // Explicitly pass through cache_creation so the downstream usage parser (from_codex_response) attributes billing correctly.
    if cache_creation > 0 {
        result["cache_creation_input_tokens"] = json!(cache_creation);
    }
    result
}

/// OpenAI Responses request → Anthropic Messages request
///
/// `default_max_tokens`: injected when the Responses body has no
/// `max_output_tokens` (Anthropic's `max_tokens` is required; missing it yields a 400).
pub fn responses_request_to_anthropic(
    body: Value,
    default_max_tokens: u64,
) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // Pass model through (the upstream model has already been applied by the forwarder)
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // instructions → system
    if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
        if !instructions.is_empty() {
            result["system"] = json!(instructions);
        }
    }

    // input → messages
    let mut messages = match body.get("input") {
        Some(Value::Array(items)) => convert_input_to_messages(items)?,
        Some(Value::String(text)) if is_meaningful_text(text) => vec![json!({
            "role": "user",
            "content": [{ "type": "text", "text": text }]
        })],
        _ => Vec::new(),
    };
    // Anthropic /v1/messages requires messages to be non-empty and the first to be user.
    // Normalize the history (compacted/resumed sessions may start with
    // assistant/function_call, or input may be entirely reasoning and thus empty after being dropped).
    // Drop unpaired tool_result blocks first (they would otherwise 400), then guarantee a leading user.
    drop_orphan_tool_results(&mut messages);
    drop_empty_messages(&mut messages);
    ensure_leading_user_message(&mut messages);
    if messages.is_empty() {
        return Err(ProxyError::InvalidRequest(
            "cannot convert Codex request: empty messages".to_string(),
        ));
    }
    // Extended thinking is only safe to enable when the final turn is a *fresh* user
    // message (plain text, no tool_result). In that case the model starts a new
    // assistant answer from scratch and no prior signed thinking block is required.
    //
    // It must stay off otherwise:
    // - Mid tool-cycle (final turn is a user tool_result): Anthropic requires the
    //   assistant's preceding tool_use turn to carry its signed thinking block, which we
    //   dropped along with OpenAI's reasoning items → enabling thinking 400s.
    // - Trailing assistant turn (a resumed/compacted history ending in an unpaired
    //   tool_use): Anthropic requires the final assistant message to start with a
    //   thinking block, which we likewise don't have → also a 400.
    // A *completed* tool round is separated from the next user message by the assistant's
    // answer, so its trailing user turn is text-only and thinking re-enables safely.
    // (A whole-history scan instead stays disabled forever once any tool was ever used.)
    let allow_thinking = trailing_turn_allows_thinking(&messages);
    result["messages"] = json!(messages);

    // reasoning.effort → thinking budget
    //
    // Only enable thinking for recognized effort values; unknown values return None,
    // keeping normal sampling (without accidentally swallowing temperature/top_p).
    let mut thinking_enabled = false;
    let mut thinking_budget = 0u64;
    if allow_thinking {
        if let Some(budget) = body
            .pointer("/reasoning/effort")
            .and_then(|v| v.as_str())
            .and_then(effort_to_thinking_budget)
        {
            thinking_budget = budget;
            thinking_enabled = true;
        }
    }

    // max_output_tokens → max_tokens (required)
    let max_tokens = body
        .get("max_output_tokens")
        .and_then(|v| v.as_u64())
        .filter(|v| *v > 0)
        .unwrap_or(default_max_tokens);
    if thinking_enabled {
        // Anthropic requires max_tokens > budget_tokens and budget >= 1024. Reserve
        // headroom for the visible answer: cap the thinking budget at half of max_tokens
        // so a large derived budget (e.g. 24576 for xhigh) can't consume nearly all of a
        // modest max_tokens and leave ~1 output token (an effectively empty completion).
        // Do not raise the caller's max_tokens (it may exceed the model's output ceiling
        // and 400). If the remaining budget is below Anthropic's 1024 floor, disable
        // thinking and restore normal sampling.
        let ceiling = max_tokens / 2;
        thinking_budget = thinking_budget.min(ceiling);
        if thinking_budget < 1024 {
            thinking_enabled = false;
        }
    }
    result["max_tokens"] = json!(max_tokens);

    if thinking_enabled {
        result["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": thinking_budget
        });
        // When extended thinking is enabled, Anthropic does not accept temperature/top_p, so do not pass them through
    } else {
        if let Some(v) = body.get("temperature") {
            result["temperature"] = v.clone();
        }
        if let Some(v) = body.get("top_p") {
            result["top_p"] = v.clone();
        }
    }

    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // tools: keep only the function type
    let mut has_tools = false;
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let anth_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("function"))
            .map(|t| {
                // Do not emit an explicit null when description is missing (strict
                // gateways will 400); when input_schema is missing or not an object,
                // fall back to a valid empty object schema.
                let mut tool = json!({
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "input_schema": t
                        .get("parameters")
                        .cloned()
                        .filter(|p| p.is_object())
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                });
                if let Some(desc) = t.get("description").and_then(|d| d.as_str()) {
                    tool["description"] = json!(desc);
                }
                tool
            })
            .collect();
        if !anth_tools.is_empty() {
            has_tools = true;
            result["tools"] = json!(anth_tools);
        }
    }

    // Only forward tool_choice when tools survived the filter. Anthropic 400s on a
    // tool_choice with no tools ("tool_choice may only be specified while providing
    // tools"), and that 400 is non-retryable — so a request whose only tools were
    // freeform/hosted (web_search, apply_patch, …) must drop tool_choice too.
    if has_tools {
        if let Some(tc) = body.get("tool_choice") {
            let mapped = map_tool_choice_to_anthropic(tc);
            // When extended thinking is enabled, Anthropic rejects forced tools (any/tool), so downgrade to auto.
            result["tool_choice"] = if thinking_enabled {
                downgrade_forced_tool_choice(mapped)
            } else {
                mapped
            };
        }
    }

    Ok(result)
}

/// When extended thinking is enabled, downgrade a forced tool choice (any/tool) to auto; otherwise return it unchanged.
fn downgrade_forced_tool_choice(tool_choice: Value) -> Value {
    match tool_choice.get("type").and_then(|t| t.as_str()) {
        Some("any") | Some("tool") => json!({ "type": "auto" }),
        _ => tool_choice,
    }
}

/// tool_choice: Responses → Anthropic (the reverse of `map_tool_choice_to_responses`)
fn map_tool_choice_to_anthropic(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(s) => match s.as_str() {
            "required" => json!({ "type": "any" }),
            "auto" => json!({ "type": "auto" }),
            "none" => json!({ "type": "none" }),
            _ => json!({ "type": "auto" }),
        },
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            Some("function") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({ "type": "tool", "name": name })
            }
            // Other object shapes (allowed_tools / hosted-tool selectors, etc.) are
            // not recognized by Anthropic; downgrade to auto to avoid passing OpenAI's
            // raw structure through and causing a 400.
            _ => json!({ "type": "auto" }),
        },
        _ => json!({ "type": "auto" }),
    }
}

/// Re-nests the flat Responses input[] back into Anthropic messages.
///
/// - input_text/output_text → text block of the corresponding role
/// - input_image → image block
/// - function_call → assistant's tool_use block (merged with the preceding assistant text into the same message)
/// - function_call_output → user's tool_result block (consecutive ones merged into the same user message)
/// - reasoning (including encrypted_content) → dropped
fn convert_input_to_messages(items: &[Value]) -> Result<Vec<Value>, ProxyError> {
    let mut messages: Vec<Value> = Vec::new();

    for item in items {
        match item.get("type").and_then(|t| t.as_str()) {
            Some("function_call") => {
                let call_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("id").and_then(|v| v.as_str()))
                    .unwrap_or("");
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args_str = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                let input: Value = if args_str.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(args_str).unwrap_or(json!({}))
                };
                let input = sanitize_anthropic_tool_use_input(name, input);
                push_block(
                    &mut messages,
                    "assistant",
                    json!({
                        "type": "tool_use",
                        "id": call_id,
                        "name": name,
                        "input": input
                    }),
                );
            }
            Some("function_call_output") => {
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let output = match item.get("output") {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => canonical_json_string(v),
                    None => String::new(),
                };
                push_block(
                    &mut messages,
                    "user",
                    json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": output
                    }),
                );
            }
            // OpenAI reasoning item (including encrypted_content) has no Anthropic equivalent, drop it
            Some("reasoning") => {}
            // message item or an item carrying a role
            _ => {
                let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let anth_role = if role == "assistant" {
                    "assistant"
                } else {
                    "user"
                };
                match item.get("content") {
                    Some(Value::String(text)) if is_meaningful_text(text) => {
                        push_block(
                            &mut messages,
                            anth_role,
                            json!({ "type": "text", "text": text }),
                        );
                    }
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            let part_type = part.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match part_type {
                                "input_text" | "output_text" => {
                                    if let Some(text) = part
                                        .get("text")
                                        .and_then(|t| t.as_str())
                                        .filter(|t| is_meaningful_text(t))
                                    {
                                        push_block(
                                            &mut messages,
                                            anth_role,
                                            json!({ "type": "text", "text": text }),
                                        );
                                    }
                                }
                                "refusal" => {
                                    if let Some(text) = part
                                        .get("refusal")
                                        .and_then(|t| t.as_str())
                                        .filter(|t| is_meaningful_text(t))
                                    {
                                        push_block(
                                            &mut messages,
                                            anth_role,
                                            json!({ "type": "text", "text": text }),
                                        );
                                    }
                                }
                                "input_image" => {
                                    if let Some(block) = image_block_from_input_image(part) {
                                        push_block(&mut messages, anth_role, block);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(messages)
}

/// Ensures the first message is a user: compacted/resumed sessions may start with
/// assistant or function_call, but Anthropic requires the first to be user, else 400.
/// An empty array is not handled (the caller decides whether to error).
fn ensure_leading_user_message(messages: &mut Vec<Value>) {
    let leads_with_user = messages
        .first()
        .and_then(|m| m.get("role"))
        .and_then(|r| r.as_str())
        == Some("user");
    if !messages.is_empty() && !leads_with_user {
        messages.insert(
            0,
            json!({
                "role": "user",
                "content": [{ "type": "text", "text": "(continuing the conversation)" }]
            }),
        );
    }
}

/// Drops `tool_result` blocks whose matching `tool_use` is absent from an earlier
/// assistant message. Anthropic 400s on an unpaired tool_result, which can occur
/// when a compacted/resumed history keeps a `function_call_output` but dropped its
/// `function_call` (e.g. history that begins with a tool_result). tool_use ids are
/// introduced only by preceding assistant turns, so a single left-to-right pass is
/// sufficient. If removing orphans empties a message, that message is dropped too.
fn drop_orphan_tool_results(messages: &mut Vec<Value>) {
    let mut seen_tool_use_ids: HashSet<String> = HashSet::new();
    let mut removed_any = false;

    for msg in messages.iter_mut() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "assistant" {
            // Record tool_use ids as we pass; a later tool_result must reference one of these.
            if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                for block in blocks {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                            seen_tool_use_ids.insert(id.to_string());
                        }
                    }
                }
            }
        } else if let Some(blocks) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
            let before = blocks.len();
            blocks.retain(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    block
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .map(|id| seen_tool_use_ids.contains(id))
                        .unwrap_or(false)
                } else {
                    true
                }
            });
            if blocks.len() != before {
                removed_any = true;
            }
        }
    }

    if removed_any {
        messages.retain(|msg| {
            msg.get("content")
                .and_then(|c| c.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(true)
        });
    }
}

/// Whether extended thinking is safe to enable for this request: true only when the
/// final message is a *fresh* user turn that carries no `tool_result` block. A trailing
/// user tool_result (mid tool-cycle) or a trailing assistant turn (a resumed/compacted
/// history ending in an unpaired tool_use) both require a signed thinking block we don't
/// have, so thinking must stay off there. Computed on the normalized message list so a
/// dropped orphan tool_result does not count. Defaults to off in every ambiguous case.
fn trailing_turn_allows_thinking(messages: &[Value]) -> bool {
    let Some(last) = messages.last() else {
        return false;
    };
    // Only a user turn can be a fresh prompt; a trailing assistant turn is never safe.
    if last.get("role").and_then(|r| r.as_str()) != Some("user") {
        return false;
    }
    match last.get("content") {
        // A user turn allows thinking unless it still carries a tool_result block.
        Some(Value::Array(blocks)) => !blocks
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result")),
        // Plain string / absent content = ordinary user text → safe.
        _ => true,
    }
}

/// Anthropic 400s on a text content block whose text is empty or whitespace-only.
/// Such blocks arise when a prior Responses turn recorded an empty
/// input_text/output_text (e.g. an empty assistant text emitted alongside a
/// tool_use); replaying it verbatim would fail the next follow-up request.
fn is_meaningful_text(text: &str) -> bool {
    !text.trim().is_empty()
}

/// Removes messages whose content array ended up empty (e.g. a turn that carried
/// only empty text that was filtered out). Anthropic 400s on empty content.
fn drop_empty_messages(messages: &mut Vec<Value>) {
    messages.retain(|msg| {
        msg.get("content")
            .and_then(|c| c.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(true)
    });
}

/// Appends a content block to messages: merge if the last message has the same role, otherwise create a new message.
fn push_block(messages: &mut Vec<Value>, role: &str, block: Value) {
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(|r| r.as_str()) == Some(role) {
            if let Some(arr) = last.get_mut("content").and_then(|c| c.as_array_mut()) {
                arr.push(block);
                return;
            }
        }
    }
    messages.push(json!({
        "role": role,
        "content": [block]
    }));
}

/// Responses' input_image → Anthropic image block.
fn image_block_from_input_image(part: &Value) -> Option<Value> {
    let url = part.get("image_url").and_then(|v| {
        v.as_str()
            .map(str::to_string)
            .or_else(|| v.get("url").and_then(|u| u.as_str()).map(str::to_string))
    })?;

    if let Some(rest) = url.strip_prefix("data:") {
        // data:<media_type>;base64,<data>
        let (meta, data) = rest.split_once(',')?;
        let media_type = meta.split(';').next().unwrap_or("image/png");
        Some(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data
            }
        }))
    } else if url.starts_with("http://") || url.starts_with("https://") {
        Some(json!({
            "type": "image",
            "source": { "type": "url", "url": url }
        }))
    } else {
        None
    }
}

/// Anthropic Messages response → OpenAI Responses response (non-streaming)
pub fn anthropic_response_to_responses(body: Value) -> Result<Value, ProxyError> {
    let id = body.get("id").and_then(|i| i.as_str()).unwrap_or("");
    let response_id = if id.is_empty() {
        "resp_ccswitch".to_string()
    } else if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    };
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("");

    let mut output: Vec<Value> = Vec::new();
    let mut text_parts: Vec<Value> = Vec::new();

    let flush_text = |output: &mut Vec<Value>, text_parts: &mut Vec<Value>| {
        if !text_parts.is_empty() {
            let idx = output.len();
            output.push(json!({
                "id": format!("{response_id}_msg_{idx}"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": std::mem::take(text_parts)
            }));
        }
    };

    if let Some(blocks) = body.get("content").and_then(|c| c.as_array()) {
        for block in blocks {
            match block.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(json!({
                            "type": "output_text",
                            "text": text,
                            "annotations": []
                        }));
                    }
                }
                "tool_use" => {
                    flush_text(&mut output, &mut text_parts);
                    let call_id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    let input = sanitize_anthropic_tool_use_input(name, input);
                    output.push(json!({
                        "id": format!("fc_{call_id}"),
                        "type": "function_call",
                        "status": "completed",
                        "call_id": call_id,
                        "name": name,
                        "arguments": canonical_json_string(&input)
                    }));
                }
                "thinking" => {
                    if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            flush_text(&mut output, &mut text_parts);
                            let idx = output.len();
                            output.push(json!({
                                "id": format!("rs_{response_id}_{idx}"),
                                "type": "reasoning",
                                "summary": [{
                                    "type": "summary_text",
                                    "text": text
                                }]
                            }));
                        }
                    }
                }
                // Drop other blocks such as redacted_thinking
                _ => {}
            }
        }
    }
    flush_text(&mut output, &mut text_parts);

    let (status, incomplete_reason) =
        map_anthropic_stop_reason_to_status(body.get("stop_reason").and_then(|s| s.as_str()));
    let usage = build_responses_usage_from_anthropic(body.get("usage"));

    let mut result = json!({
        "id": response_id,
        "object": "response",
        "created_at": 0,
        "status": status,
        "model": model,
        "output": output,
        "usage": usage
    });
    if let Some(reason) = incomplete_reason {
        result["incomplete_details"] = json!({ "reason": reason });
    }

    Ok(result)
}

/// Aggregates an Anthropic Messages **SSE stream** (with no Content-Type marker)
/// back into a single Anthropic non-streaming message JSON.
///
/// Used as a fallback: the upstream returned an SSE body for a `stream:false`
/// request but without the `text/event-stream` header (symmetric to the
/// `body_looks_like_sse` fallback on the chat / claude side, see #2234). The
/// aggregated message can be handed directly to [`anthropic_response_to_responses`].
///
/// It also tolerates the last event missing a trailing blank line (truncated
/// stream): after looping over complete event blocks, it processes the residual
/// buffer as the last event.
pub fn anthropic_sse_to_message_value(body: &str) -> Result<Value, ProxyError> {
    let mut message: Option<Value> = None;
    // Collect blocks by content index along with the partial_json accumulator for their tool_use.
    let mut blocks: BTreeMap<u64, Value> = BTreeMap::new();
    let mut json_accum: BTreeMap<u64, String> = BTreeMap::new();
    let mut stop_reason: Option<String> = None;
    let mut delta_output_tokens: Option<u64> = None;

    let mut buffer = body.to_string();
    let process_block = |block: &str,
                         message: &mut Option<Value>,
                         blocks: &mut BTreeMap<u64, Value>,
                         json_accum: &mut BTreeMap<u64, String>,
                         stop_reason: &mut Option<String>,
                         delta_output_tokens: &mut Option<u64>|
     -> Result<(), ProxyError> {
        let mut data = String::new();
        for line in block.lines() {
            if let Some(chunk) = strip_sse_field(line, "data") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(chunk);
            }
        }
        if data.trim().is_empty() || data.trim() == "[DONE]" {
            return Ok(());
        }
        let value: Value = match serde_json::from_str(data.trim()) {
            Ok(v) => v,
            Err(_) => return Ok(()), // Skip events that cannot be parsed (ping, etc.)
        };
        match value.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "message_start" => {
                if let Some(msg) = value.get("message") {
                    *message = Some(msg.clone());
                }
            }
            "content_block_start" => {
                if let Some(index) = value.get("index").and_then(|v| v.as_u64()) {
                    let block = value.get("content_block").cloned().unwrap_or(json!({}));
                    blocks.insert(index, block);
                    json_accum.entry(index).or_default();
                }
            }
            "content_block_delta" => {
                if let Some(index) = value.get("index").and_then(|v| v.as_u64()) {
                    let delta = value.get("delta").cloned().unwrap_or(json!({}));
                    match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "text_delta" => {
                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                append_str_field(
                                    blocks.entry(index).or_insert(json!({})),
                                    "text",
                                    text,
                                );
                            }
                        }
                        "thinking_delta" => {
                            if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                                append_str_field(
                                    blocks.entry(index).or_insert(json!({})),
                                    "thinking",
                                    text,
                                );
                            }
                        }
                        "signature_delta" => {
                            if let Some(sig) = delta.get("signature").and_then(|t| t.as_str()) {
                                blocks.entry(index).or_insert(json!({}))["signature"] = json!(sig);
                            }
                        }
                        "input_json_delta" => {
                            if let Some(partial) =
                                delta.get("partial_json").and_then(|t| t.as_str())
                            {
                                json_accum.entry(index).or_default().push_str(partial);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                if let Some(index) = value.get("index").and_then(|v| v.as_u64()) {
                    if let Some(accum) = json_accum.get(&index) {
                        if !accum.trim().is_empty() {
                            let parsed: Value =
                                serde_json::from_str(accum).unwrap_or_else(|_| json!({}));
                            if let Some(block) = blocks.get_mut(&index) {
                                block["input"] = parsed;
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
                    *stop_reason = Some(reason.to_string());
                }
                if let Some(output) = value
                    .pointer("/usage/output_tokens")
                    .and_then(|v| v.as_u64())
                {
                    *delta_output_tokens = Some(output);
                }
            }
            "message_stop" => {}
            "error" => {
                let msg = value
                    .pointer("/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("upstream anthropic SSE error");
                return Err(ProxyError::TransformError(format!(
                    "anthropic SSE error event: {msg}"
                )));
            }
            _ => {}
        }
        Ok(())
    };

    while let Some(block) = take_sse_block(&mut buffer) {
        process_block(
            &block,
            &mut message,
            &mut blocks,
            &mut json_accum,
            &mut stop_reason,
            &mut delta_output_tokens,
        )?;
    }
    // Tolerate the last event missing a trailing blank line (truncated stream).
    if !buffer.trim().is_empty() {
        process_block(
            &buffer.clone(),
            &mut message,
            &mut blocks,
            &mut json_accum,
            &mut stop_reason,
            &mut delta_output_tokens,
        )?;
    }

    let mut message = message.ok_or_else(|| {
        ProxyError::TransformError(
            "anthropic SSE aggregation: missing message_start event".to_string(),
        )
    })?;

    // Merge in the content blocks (ordered by index), stop_reason, and the cumulative output_tokens.
    let content: Vec<Value> = blocks.into_values().collect();
    message["content"] = json!(content);
    if let Some(reason) = stop_reason {
        message["stop_reason"] = json!(reason);
    }
    if let Some(output) = delta_output_tokens {
        // message_delta's usage.output_tokens is a cumulative value, overriding the 0 from message_start.
        if let Some(usage) = message.get_mut("usage").and_then(|u| u.as_object_mut()) {
            usage.insert("output_tokens".to_string(), json!(output));
        }
    }

    Ok(message)
}

/// Appends content to a string field of a JSON object (creating it if absent).
fn append_str_field(block: &mut Value, field: &str, text: &str) {
    let existing = block
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    block[field] = json!(format!("{existing}{text}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Request: Responses → Anthropic ====================

    #[test]
    fn test_request_simple_text() {
        let input = json!({
            "model": "claude-3-5-sonnet",
            "max_output_tokens": 1024,
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "Hello" }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["model"], "claude-3-5-sonnet");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][0]["content"][0]["type"], "text");
        assert_eq!(result["messages"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_request_missing_max_output_tokens_injects_default() {
        let input = json!({
            "model": "claude",
            "input": [{ "role": "user", "content": "Hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["max_tokens"], 4096);
    }

    #[test]
    fn test_request_instructions_to_system() {
        let input = json!({
            "model": "claude",
            "max_output_tokens": 100,
            "instructions": "You are helpful.",
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["system"], "You are helpful.");
    }

    #[test]
    fn test_request_no_instructions_no_system() {
        let input = json!({
            "model": "claude",
            "max_output_tokens": 100,
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert!(result.get("system").is_none());
    }

    #[test]
    fn test_request_tools_and_filtering() {
        let input = json!({
            "model": "claude",
            "max_output_tokens": 100,
            "input": [{ "role": "user", "content": "hi" }],
            "tools": [
                { "type": "function", "name": "get_weather", "description": "d", "parameters": {"type": "object"} },
                { "type": "web_search" },
                { "type": "custom", "name": "apply_patch" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["input_schema"]["type"], "object");
        assert!(tools[0].get("parameters").is_none());
    }

    #[test]
    fn test_request_tool_choice_mapping() {
        // A function tool must be present, else tool_choice is (correctly) dropped.
        let base = |tc: Value| {
            json!({
                "model": "c", "max_output_tokens": 100,
                "input": [{ "role": "user", "content": "hi" }],
                "tools": [{ "type": "function", "name": "x", "parameters": {"type": "object"} }],
                "tool_choice": tc
            })
        };
        assert_eq!(
            responses_request_to_anthropic(base(json!("required")), 4096).unwrap()["tool_choice"],
            json!({"type": "any"})
        );
        assert_eq!(
            responses_request_to_anthropic(base(json!("auto")), 4096).unwrap()["tool_choice"],
            json!({"type": "auto"})
        );
        assert_eq!(
            responses_request_to_anthropic(base(json!({"type": "function", "name": "x"})), 4096)
                .unwrap()["tool_choice"],
            json!({"type": "tool", "name": "x"})
        );
    }

    #[test]
    fn test_request_function_call_renests_into_assistant_tool_use() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "role": "assistant", "content": [{ "type": "output_text", "text": "Let me check" }] },
                { "type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        // The assistant-first history is normalized: a synthetic user message is
        // prepended, and the assistant (text + tool_use merged) becomes the second one.
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "text");
        assert_eq!(messages[1]["content"][1]["type"], "tool_use");
        assert_eq!(messages[1]["content"][1]["id"], "call_1");
        assert_eq!(messages[1]["content"][1]["input"]["city"], "Tokyo");
    }

    #[test]
    fn test_request_function_call_outputs_merge_into_one_user_message() {
        // Consecutive function_call_output items (each paired with a preceding
        // function_call) merge into a single user message of tool_result blocks.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "t", "arguments": "{}" },
                { "type": "function_call", "call_id": "c2", "name": "t", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "A" },
                { "type": "function_call_output", "call_id": "c2", "output": "B" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        // Leading assistant history is normalized with a synthetic leading user.
        let last = messages.last().unwrap();
        assert_eq!(last["role"], "user");
        let content = last["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "c1");
        assert_eq!(content[0]["content"], "A");
        assert_eq!(content[1]["tool_use_id"], "c2");
    }

    #[test]
    fn test_request_orphan_tool_result_dropped() {
        // A function_call_output whose matching function_call was dropped (e.g. by
        // compaction) becomes an orphan tool_result; it must be removed so Anthropic
        // does not 400, while the rest of the turn is preserved.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "type": "function_call_output", "call_id": "ghost", "output": "X" },
                { "role": "user", "content": [{ "type": "input_text", "text": "hello" }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        let content = messages[0]["content"].as_array().unwrap();
        // Only the text survives; the orphan tool_result is gone.
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "hello");
    }

    #[test]
    fn test_request_empty_text_blocks_dropped() {
        // An empty/whitespace-only assistant text emitted alongside a tool_use must be
        // filtered out (Anthropic 400s on empty text blocks), keeping the tool_use, and
        // a user turn made up solely of empty text must not leave an empty message.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "role": "assistant", "content": [{ "type": "output_text", "text": "" }] },
                { "type": "function_call", "call_id": "c1", "name": "t", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" },
                { "role": "user", "content": [{ "type": "input_text", "text": "   " }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        // Empty assistant text is gone; no message carries an empty text block.
        for msg in messages {
            for block in msg["content"].as_array().unwrap() {
                if block["type"] == "text" {
                    assert!(!block["text"].as_str().unwrap().trim().is_empty());
                }
            }
            assert!(!msg["content"].as_array().unwrap().is_empty());
        }
        // The whitespace-only trailing user turn collapsed into the tool_result user message.
        let last = messages.last().unwrap();
        assert_eq!(last["role"], "user");
        assert_eq!(last["content"][0]["type"], "tool_result");
    }

    #[test]
    fn test_request_all_orphan_tool_results_error() {
        // If the entire input is orphan tool_results, dropping them empties the
        // message list and conversion errors (nothing valid to send to Anthropic).
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "type": "function_call_output", "call_id": "c1", "output": "A" },
                { "type": "function_call_output", "call_id": "c2", "output": "B" }
            ]
        });
        assert!(responses_request_to_anthropic(input, 4096).is_err());
    }

    #[test]
    fn test_request_paired_tool_result_kept() {
        // A tool_result whose function_call is present survives the orphan guard.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "t", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last["role"], "user");
        assert_eq!(last["content"][0]["type"], "tool_result");
        assert_eq!(last["content"][0]["tool_use_id"], "c1");
    }

    #[test]
    fn test_request_empty_arguments_parses_to_empty_object() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "t", "arguments": "" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        // function_call at the head → a synthetic user is prepended, tool_use is in the second assistant message.
        assert_eq!(result["messages"][1]["content"][0]["input"], json!({}));
    }

    #[test]
    fn test_request_image_data_url() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [{
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "what?" },
                    { "type": "input_image", "image_url": "data:image/png;base64,abc123" }
                ]
            }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let content = result["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], "abc123");
    }

    #[test]
    fn test_request_image_http_url() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [{
                "role": "user",
                "content": [{ "type": "input_image", "image_url": "https://x/y.png" }]
            }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let block = &result["messages"][0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "url");
        assert_eq!(block["source"]["url"], "https://x/y.png");
    }

    #[test]
    fn test_request_effort_to_thinking_and_drops_temperature() {
        let input = json!({
            "model": "c",
            // Well above 2× the high-effort budget so the output-headroom cap
            // (max_tokens/2) does not clamp it — this test covers effort mapping.
            "max_output_tokens": 40000,
            "temperature": 0.7,
            "top_p": 0.9,
            "reasoning": { "effort": "high" },
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 16384);
        assert!(result.get("temperature").is_none());
        assert!(result.get("top_p").is_none());
    }

    #[test]
    fn test_request_unknown_effort_keeps_sampling_params() {
        // An unrecognized effort should not enable thinking, nor swallow temperature/top_p.
        let input = json!({
            "model": "c",
            "max_output_tokens": 20000,
            "temperature": 0.7,
            "top_p": 0.9,
            "reasoning": { "effort": "turbo" },
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert!(result.get("thinking").is_none());
        assert_eq!(result["temperature"], 0.7);
        assert_eq!(result["top_p"], 0.9);
    }

    #[test]
    fn test_request_tool_history_disables_thinking() {
        // When tool history is present (function_call_output), do not inject thinking
        // even if effort is set, and fall back to passing temperature/top_p through,
        // to avoid the Anthropic 400 caused by a missing thinking block.
        let input = json!({
            "model": "c",
            "max_output_tokens": 20000,
            "temperature": 0.5,
            "reasoning": { "effort": "high" },
            "input": [
                { "type": "function_call", "call_id": "c1", "name": "t", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert!(result.get("thinking").is_none());
        assert_eq!(result["temperature"], 0.5);
    }

    #[test]
    fn test_request_completed_tool_round_reenables_thinking() {
        // A *completed* tool round (assistant answered after the tool_result) followed
        // by a fresh user question: the trailing turn is text-only, so thinking must be
        // re-enabled — unlike a whole-history scan, which would stay off forever.
        let input = json!({
            "model": "c",
            "max_output_tokens": 20000,
            "reasoning": { "effort": "high" },
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "function_call", "call_id": "c1", "name": "t", "arguments": "{}" },
                { "type": "function_call_output", "call_id": "c1", "output": "ok" },
                { "role": "assistant", "content": [{ "type": "output_text", "text": "done" }] },
                { "role": "user", "content": [{ "type": "input_text", "text": "next question" }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        // The last message is a text-only user turn → not mid tool-cycle → thinking on.
        let messages = result["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        assert_eq!(last["role"], "user");
        assert_eq!(last["content"][0]["type"], "text");
        assert_eq!(result["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_request_tool_choice_dropped_when_no_function_tools() {
        // When every tool is filtered out (web_search / apply_patch), no tools are
        // emitted, so tool_choice must be dropped too — otherwise Anthropic 400s.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [{ "role": "user", "content": "hi" }],
            "tools": [
                { "type": "web_search" },
                { "type": "custom", "name": "apply_patch" }
            ],
            "tool_choice": "required"
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert!(result.get("tools").is_none());
        assert!(result.get("tool_choice").is_none());
    }

    #[test]
    fn test_request_thinking_downgrades_forced_tool_choice() {
        // First round (no tool history) + effort → thinking enabled; forced tool_choice is downgraded to auto.
        let input = json!({
            "model": "c",
            "max_output_tokens": 20000,
            "reasoning": { "effort": "high" },
            "tools": [{ "type": "function", "name": "x", "parameters": {"type": "object"} }],
            "tool_choice": "required",
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["tool_choice"], json!({ "type": "auto" }));
    }

    #[test]
    fn test_request_forced_tool_choice_kept_without_thinking() {
        // With no effort (thinking off), a forced tool_choice is kept as-is.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "tools": [{ "type": "function", "name": "x", "parameters": {"type": "object"} }],
            "tool_choice": "required",
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["tool_choice"], json!({ "type": "any" }));
    }

    #[test]
    fn test_request_small_max_tokens_disables_thinking() {
        // The chosen effort budget is clamped below max_tokens; after clamping it is
        // < 1024 → disable thinking, fall back to sampling, and do not raise the
        // caller's max_tokens (to avoid exceeding the model's output ceiling and 400).
        let input = json!({
            "model": "c",
            "max_output_tokens": 1000,
            "temperature": 0.7,
            "reasoning": { "effort": "high" },
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert!(result.get("thinking").is_none());
        assert_eq!(result["max_tokens"], 1000);
        assert_eq!(result["temperature"], 0.7);
    }

    #[test]
    fn test_request_thinking_budget_clamped_below_max_tokens() {
        // The chosen effort budget exceeds half of max_tokens, so it is capped at
        // max_tokens/2 (reserving the other half for the visible answer) while staying
        // >= 1024, so thinking stays enabled.
        let input = json!({
            "model": "c",
            "max_output_tokens": 5000,
            "reasoning": { "effort": "high" }, // budget 16384
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 2500);
        assert_eq!(result["max_tokens"], 5000);
    }

    #[test]
    fn test_request_default_max_tokens_leaves_output_headroom() {
        // Regression: on the no-max_output_tokens fallback path, a large derived thinking
        // budget must not consume nearly all of the default max_tokens. With default 8192
        // and high effort (16384), the budget is capped at 8192/2 = 4096, leaving 4096 for
        // the visible answer (previously it clamped to 8191, leaving ~1 output token).
        let input = json!({
            "model": "c",
            "reasoning": { "effort": "high" },
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 8192).unwrap();
        assert_eq!(result["thinking"]["type"], "enabled");
        assert_eq!(result["thinking"]["budget_tokens"], 4096);
        assert_eq!(result["max_tokens"], 8192);
        assert!(
            result["max_tokens"].as_u64().unwrap()
                - result["thinking"]["budget_tokens"].as_u64().unwrap()
                >= 4096,
            "at least half of max_tokens must remain for the visible answer"
        );
    }

    #[test]
    fn test_request_thinking_disabled_when_trailing_turn_is_assistant() {
        // A resumed/compacted history ending in an unpaired assistant tool_use must not
        // re-enable thinking — Anthropic would 400 on the missing signed thinking block.
        let input = json!({
            "model": "c",
            "max_output_tokens": 40000,
            "reasoning": { "effort": "high" },
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "run it" }] },
                { "type": "function_call", "call_id": "c1", "name": "sh", "arguments": "{}" }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        // Trailing message is an assistant tool_use turn → thinking stays off.
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.last().unwrap()["role"], "assistant");
        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn test_request_reasoning_item_dropped() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "type": "reasoning", "id": "rs_1", "encrypted_content": "xxx" },
                { "role": "user", "content": [{ "type": "input_text", "text": "hi" }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_request_drops_openai_only_fields() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "store": false,
            "include": ["reasoning.encrypted_content"],
            "service_tier": "priority",
            "parallel_tool_calls": true,
            "input": [{ "role": "user", "content": "hi" }]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert!(result.get("store").is_none());
        assert!(result.get("include").is_none());
        assert!(result.get("service_tier").is_none());
        assert!(result.get("parallel_tool_calls").is_none());
    }

    // ==================== Response: Anthropic → Responses ====================

    #[test]
    fn test_response_text_end_turn() {
        let input = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude",
            "content": [{ "type": "text", "text": "Hello!" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        });
        let result = anthropic_response_to_responses(input).unwrap();
        assert_eq!(result["id"], "resp_msg_1");
        assert_eq!(result["status"], "completed");
        assert_eq!(result["output"][0]["type"], "message");
        assert_eq!(result["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(result["output"][0]["content"][0]["text"], "Hello!");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
        assert_eq!(result["usage"]["total_tokens"], 15);
    }

    #[test]
    fn test_response_tool_use() {
        let input = json!({
            "id": "msg_1",
            "content": [{
                "type": "tool_use",
                "id": "call_1",
                "name": "get_weather",
                "input": { "city": "Tokyo" }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 15 }
        });
        let result = anthropic_response_to_responses(input).unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["output"][0]["type"], "function_call");
        assert_eq!(result["output"][0]["call_id"], "call_1");
        assert_eq!(result["output"][0]["name"], "get_weather");
        assert_eq!(result["output"][0]["arguments"], "{\"city\":\"Tokyo\"}");
    }

    #[test]
    fn test_response_thinking_becomes_reasoning() {
        let input = json!({
            "id": "msg_1",
            "content": [
                { "type": "thinking", "thinking": "Let me think" },
                { "type": "text", "text": "answer" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 2 }
        });
        let result = anthropic_response_to_responses(input).unwrap();
        assert_eq!(result["output"][0]["type"], "reasoning");
        assert_eq!(result["output"][0]["summary"][0]["text"], "Let me think");
        assert_eq!(result["output"][1]["type"], "message");
    }

    #[test]
    fn test_response_max_tokens_incomplete() {
        let input = json!({
            "id": "msg_1",
            "content": [{ "type": "text", "text": "partial" }],
            "stop_reason": "max_tokens",
            "usage": { "input_tokens": 1, "output_tokens": 2 }
        });
        let result = anthropic_response_to_responses(input).unwrap();
        assert_eq!(result["status"], "incomplete");
        assert_eq!(result["incomplete_details"]["reason"], "max_output_tokens");
    }

    #[test]
    fn test_response_usage_cache_no_double_count() {
        let input = json!({
            "id": "msg_1",
            "content": [{ "type": "text", "text": "x" }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 20,
                "output_tokens": 5,
                "cache_read_input_tokens": 60,
                "cache_creation_input_tokens": 20
            }
        });
        let result = anthropic_response_to_responses(input).unwrap();
        // input_tokens = fresh + cache_read = 20 + 60 = 80 (excluding cache_creation).
        // The Codex billing calculator only subtracts cache_read from input (→ billable=fresh=20),
        // and separately lists cache-creation cost via cache_creation_input_tokens; folding creation into input would double-charge.
        assert_eq!(result["usage"]["input_tokens"], 80);
        assert_eq!(result["usage"]["output_tokens"], 5);
        // total still includes everything: 80 + cache_creation 20 + output 5 = 105
        assert_eq!(result["usage"]["total_tokens"], 105);
        assert_eq!(result["usage"]["input_tokens_details"]["cached_tokens"], 60);
        // cache_creation is passed through explicitly for downstream billing attribution (counted only once)
        assert_eq!(result["usage"]["cache_creation_input_tokens"], 20);
    }

    #[test]
    fn test_response_read_tool_drops_empty_pages() {
        let input = json!({
            "id": "msg_1",
            "content": [{
                "type": "tool_use",
                "id": "call_1",
                "name": "Read",
                "input": { "file_path": "/tmp/x", "pages": "" }
            }],
            "stop_reason": "tool_use"
        });
        let result = anthropic_response_to_responses(input).unwrap();
        let args = result["output"][0]["arguments"].as_str().unwrap();
        assert!(args.contains("/tmp/x"));
        assert!(!args.contains("pages"));
    }

    #[test]
    fn test_response_refusal_is_incomplete_content_filter() {
        let input = json!({
            "id": "msg_1",
            "content": [{ "type": "text", "text": "" }],
            "stop_reason": "refusal",
            "usage": { "input_tokens": 1, "output_tokens": 0 }
        });
        let result = anthropic_response_to_responses(input).unwrap();
        assert_eq!(result["status"], "incomplete");
        assert_eq!(result["incomplete_details"]["reason"], "content_filter");
    }

    // ==================== Request normalization: non-empty & first is user ====================

    #[test]
    fn test_request_empty_messages_errors() {
        // input is entirely reasoning (dropped) → messages is empty → error explicitly rather than leaving it to the upstream 400.
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "instructions": "sys",
            "input": [
                { "type": "reasoning", "id": "rs_1", "encrypted_content": "xxx" }
            ]
        });
        assert!(responses_request_to_anthropic(input, 4096).is_err());
    }

    #[test]
    fn test_request_assistant_first_gets_leading_user() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [
                { "role": "assistant", "content": [{ "type": "output_text", "text": "hi" }] }
            ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
    }

    // ==================== tools / tool_choice edge cases ====================

    #[test]
    fn test_request_tool_without_description_or_params() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [{ "role": "user", "content": "hi" }],
            "tools": [ { "type": "function", "name": "noop" } ]
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        let tool = &result["tools"][0];
        // Do not emit an explicit null description; input_schema falls back to a valid object schema.
        assert!(tool.get("description").is_none());
        assert_eq!(tool["input_schema"]["type"], "object");
    }

    #[test]
    fn test_request_unknown_object_tool_choice_degrades_to_auto() {
        let input = json!({
            "model": "c",
            "max_output_tokens": 100,
            "input": [{ "role": "user", "content": "hi" }],
            "tools": [{ "type": "function", "name": "x", "parameters": {"type": "object"} }],
            "tool_choice": { "type": "allowed_tools", "tools": [] }
        });
        let result = responses_request_to_anthropic(input, 4096).unwrap();
        assert_eq!(result["tool_choice"], json!({ "type": "auto" }));
    }

    // ==================== SSE aggregation fallback ====================

    #[test]
    fn test_anthropic_sse_aggregation_text_and_usage() {
        let sse = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["content"][0]["type"], "text");
        assert_eq!(msg["content"][0]["text"], "Hello world");
        assert_eq!(msg["stop_reason"], "end_turn");
        assert_eq!(msg["usage"]["input_tokens"], 10);
        assert_eq!(msg["usage"]["output_tokens"], 7);

        // The aggregated result can be converted directly into Responses.
        let resp = anthropic_response_to_responses(msg).unwrap();
        assert_eq!(resp["status"], "completed");
        assert_eq!(resp["output"][0]["content"][0]["text"], "Hello world");
    }

    #[test]
    fn test_anthropic_sse_aggregation_tool_use_partial_json() {
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"get_weather\",\"input\":{}}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Tokyo\\\"}\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["content"][0]["type"], "tool_use");
        assert_eq!(msg["content"][0]["name"], "get_weather");
        assert_eq!(msg["content"][0]["input"]["city"], "Tokyo");
        assert_eq!(msg["stop_reason"], "tool_use");
    }

    #[test]
    fn test_anthropic_sse_aggregation_tool_use_input_only_in_start() {
        // Parity guard with the live streaming emitter's
        // `test_tool_use_input_only_in_start_event`: a gateway that carries the full tool
        // `input` on content_block_start and emits NO input_json_delta must still resolve
        // the same arguments (the empty-accum fallback keeps the start-carried input).
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"get_weather\",\"input\":{\"city\":\"Tokyo\"}}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["content"][0]["type"], "tool_use");
        assert_eq!(msg["content"][0]["name"], "get_weather");
        // Identical to the deltas-only case above — neither path may drop start input.
        assert_eq!(msg["content"][0]["input"]["city"], "Tokyo");
        assert_eq!(msg["stop_reason"], "tool_use");
    }

    #[test]
    fn test_anthropic_sse_aggregation_missing_message_start_errors() {
        let sse = "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n";
        assert!(anthropic_sse_to_message_value(sse).is_err());
    }

    #[test]
    fn test_anthropic_sse_aggregation_error_event_errors() {
        let sse = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"overloaded\"}}\n\n";
        assert!(anthropic_sse_to_message_value(sse).is_err());
    }

    #[test]
    fn test_anthropic_sse_aggregation_tolerates_missing_trailing_blank_line() {
        // The last event missing a trailing blank line (truncated stream) should still be processed.
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"hi\"}}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["stop_reason"], "end_turn");
        assert_eq!(msg["usage"]["output_tokens"], 2);
    }
}
