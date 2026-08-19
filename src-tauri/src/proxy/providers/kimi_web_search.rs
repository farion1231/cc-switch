//! Bridge Codex's hosted `web_search` tool to Kimi/Moonshot's server-side
//! builtin `$web_search` on the Responses → Chat Completions conversion path.
//!
//! Codex sends `{"type": "web_search"}` in Responses requests. Chat
//! Completions upstreams have no equivalent, so the converter used to drop it
//! silently and the model answered "I cannot search the web". Kimi's Chat
//! Completions endpoints (api.kimi.com / api.moonshot.cn / api.moonshot.ai)
//! do have a server-side equivalent: the builtin function `$web_search`.
//! When declared, the upstream executes the search itself and returns a
//! `builtin_function` tool_call whose `function.arguments` already contain
//! the search results; the client only has to echo that tool_call back as a
//! `role: "tool"` message to get the final answer.
//!
//! This module implements that contract so it stays transparent to Codex:
//! the request side injects the builtin declaration (and forces a
//! non-streaming upstream request so the echo rounds can run inside the
//! proxy), and the response side loops the echo rounds until the model
//! produces a final answer. The marker header [`BRIDGED_HEADER`] tells the
//! response handler the body is a *final* Chat completion that still needs
//! the normal Chat → Responses conversion (and possibly SSE synthesis for
//! streaming clients).

use bytes::Bytes;
use serde_json::{json, Value};

use super::codex_responses_sse as sse;
use crate::provider::Provider;

/// Hop-local marker header: set on the buffered upstream response after the
/// builtin web-search echo rounds completed inside the forwarder. Consumed
/// (and stripped) by the Codex chat→responses handler; never forwarded to
/// the client.
pub(crate) const BRIDGED_HEADER: &str = "x-cc-switch-web-search-bridged";

/// Maximum number of builtin echo rounds per client request (the model may
/// chain several searches before answering).
pub(crate) const MAX_ECHO_ROUNDS: usize = 4;

/// Model alias override for the echo rounds on api.kimi.com.
///
/// Verified 2026-08-18 against the live endpoint: the `k3` alias on
/// api.kimi.com/coding accepts the builtin `$web_search` declaration and
/// returns the server-executed search tool_call, but its chat template fails
/// to tokenize the *echoed* conversation (assistant `builtin_function`
/// tool_call + `role: "tool"` message) with HTTP 400 "tokenization failed",
/// while the `kimi-for-coding` alias handles the identical echo correctly.
/// Rewriting the tool_call shape (e.g. `type: "function"`) avoids the crash
/// but then the search results are no longer resolved server-side, so the
/// only working contract on this endpoint is: run the echo rounds under the
/// `kimi-for-coding` alias. The first (search) round keeps the client's
/// model; only the echo rounds switch.
const KIMI_CODING_ECHO_MODEL: &str = "kimi-for-coding";

/// Apply the echo-round model override when the upstream is api.kimi.com
/// (see [`KIMI_CODING_ECHO_MODEL`]). Other Moonshot hosts keep the client's
/// model, whose chat template handles the builtin echo natively.
pub(crate) fn apply_echo_model_override(url: &str, request_body: &mut Value) {
    let is_kimi_coding = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .map(|host| host.eq_ignore_ascii_case("api.kimi.com"))
        .unwrap_or(false);
    if is_kimi_coding {
        request_body["model"] = json!(KIMI_CODING_ECHO_MODEL);
    }
}

/// Extract the provider's upstream base_url (mirrors the extraction chain
/// used elsewhere on the Codex path: explicit field, then config.toml).
fn provider_base_url(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|value| value.as_str())
                .and_then(super::codex::extract_codex_base_url_from_toml)
        })
}

/// Whether the provider's upstream is a Kimi/Moonshot Chat Completions
/// endpoint known to support the builtin `$web_search` tool.
pub(crate) fn provider_supports_builtin_web_search(provider: &Provider) -> bool {
    let Some(base_url) = provider_base_url(provider) else {
        return false;
    };
    base_url_supports_builtin_web_search(&base_url)
}

/// URL-level check, kept separate from the provider plumbing so it can be
/// unit-tested without constructing a full `Provider`.
fn base_url_supports_builtin_web_search(base_url: &str) -> bool {
    // Match against the parsed hostname (exact or subdomain), not the raw URL
    // string: a substring match would also fire for URLs that merely mention
    // the domains in a path/query or for lookalike hosts such as
    // `api.kimi.com.example`, injecting a nonstandard builtin tool into
    // upstreams that do not support it.
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    ["kimi.com", "moonshot.cn", "moonshot.ai"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// Accumulate Chat Completions `usage` objects across bridge rounds. The
/// bridge issues one upstream request per round (initial search + each echo),
/// each separately billable, so the final response must carry the sum,
/// otherwise downstream conversion/usage logging only accounts for the last
/// echo round.
pub(crate) fn accumulate_chat_usage(total: &mut Value, usage: &Value) {
    const NUMERIC_FIELDS: &[&str] = &["prompt_tokens", "completion_tokens", "total_tokens"];
    let Some(total_obj) = total.as_object_mut() else {
        return;
    };
    for field in NUMERIC_FIELDS {
        let delta = usage.get(*field).and_then(|v| v.as_u64()).unwrap_or(0);
        let current = total_obj.get(*field).and_then(|v| v.as_u64()).unwrap_or(0);
        total_obj.insert((*field).to_string(), json!(current + delta));
    }
    // Preserve nested details (cached/reasoning tokens) by summing them too
    // when both sides carry the same sub-object shape.
    for (details_field, sub_keys) in [
        ("prompt_tokens_details", ["cached_tokens"].as_slice()),
        ("completion_tokens_details", ["reasoning_tokens"].as_slice()),
    ] {
        let Some(usage_details) = usage.get(details_field).and_then(|v| v.as_object()) else {
            continue;
        };
        let total_details = total_obj
            .entry(details_field.to_string())
            .or_insert_with(|| json!({}));
        let Some(total_details) = total_details.as_object_mut() else {
            continue;
        };
        for sub_key in sub_keys {
            let delta = usage_details
                .get(*sub_key)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let current = total_details
                .get(*sub_key)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            total_details.insert(sub_key.to_string(), json!(current + delta));
        }
    }
}

/// Whether the original Codex Responses request declares the hosted
/// `web_search` tool (which the chat converter would otherwise drop).
pub(crate) fn request_has_web_search_tool(body: &Value) -> bool {
    body.get("tools")
        .and_then(|tools| tools.as_array())
        .map(|tools| {
            tools.iter().any(|tool| {
                matches!(
                    tool.get("type").and_then(|t| t.as_str()),
                    Some("web_search") | Some("web_search_preview")
                )
            })
        })
        .unwrap_or(false)
}

/// Inject the `$web_search` builtin declaration into the converted Chat
/// Completions request and force a non-streaming upstream call so the echo
/// rounds can run inside the proxy before anything reaches the client.
pub(crate) fn inject_builtin_web_search(chat_body: &mut Value) {
    let Some(obj) = chat_body.as_object_mut() else {
        return;
    };
    let tools = obj.entry("tools".to_string()).or_insert_with(|| json!([]));
    if let Some(tools) = tools.as_array_mut() {
        let already = tools.iter().any(|tool| {
            tool.get("type").and_then(|t| t.as_str()) == Some("builtin_function")
                && tool
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    == Some("$web_search")
        });
        if !already {
            tools.push(json!({
                "type": "builtin_function",
                "function": { "name": "$web_search" }
            }));
        }
    }
    obj.insert("stream".to_string(), json!(false));
    obj.remove("stream_options");
}

/// Builtin tool calls from an upstream Chat completion (Kimi marks them
/// `type: "builtin_function"`; also accept a `$`-prefixed function name as a
/// fallback signal).
fn builtin_tool_calls(chat_response: &Value) -> Vec<Value> {
    chat_response
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(|tool_calls| tool_calls.as_array())
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter(|call| {
                    call.get("type").and_then(|t| t.as_str()) == Some("builtin_function")
                        || call
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|name| name.starts_with('$'))
                            .unwrap_or(false)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Whether this upstream response still expects builtin echo rounds.
pub(crate) fn has_builtin_tool_calls(chat_response: &Value) -> bool {
    !builtin_tool_calls(chat_response).is_empty()
}

/// Append the assistant message (with its builtin tool_calls) and the
/// `role: "tool"` echoes to the request messages, per Kimi's builtin-tool
/// contract. Returns false when the request/response shape was unexpected
/// (caller should then stop looping and pass the response through).
pub(crate) fn append_echo_messages(request_body: &mut Value, chat_response: &Value) -> bool {
    let calls = builtin_tool_calls(chat_response);
    if calls.is_empty() {
        return false;
    }
    let Some(message) = chat_response
        .get("choices")
        .and_then(|choices| choices.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
    else {
        return false;
    };
    let Some(messages) = request_body
        .get_mut("messages")
        .and_then(|messages| messages.as_array_mut())
    else {
        return false;
    };

    let mut assistant = json!({ "role": "assistant" });
    if let Some(content) = message.get("content") {
        assistant["content"] = content.clone();
    }
    if let Some(tool_calls) = message.get("tool_calls") {
        assistant["tool_calls"] = tool_calls.clone();
    }
    messages.push(assistant);

    for call in calls {
        let Some(id) = call.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        let name = call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("$web_search");
        let arguments = call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
            .unwrap_or("");
        messages.push(json!({
            "role": "tool",
            "tool_call_id": id,
            "name": name,
            "content": arguments,
        }));
    }
    true
}

/// Synthesize a complete Responses SSE stream from a finished Responses JSON
/// value, reusing the shared event builders so the wire format matches the
/// streaming converters exactly. Used when the builtin web-search echo
/// rounds forced the upstream call to be non-streaming while the Codex
/// client still expects an SSE stream.
pub(crate) fn responses_value_to_sse_bytes(responses: &Value) -> Vec<Bytes> {
    let mut events = Vec::new();

    let mut started = responses.clone();
    started["status"] = json!("in_progress");
    started["output"] = json!([]);
    if let Some(obj) = started.as_object_mut() {
        obj.remove("usage");
    }
    events.push(sse::response_created(&started));
    events.push(sse::response_in_progress(&started));

    let output = responses
        .get("output")
        .and_then(|output| output.as_array())
        .cloned()
        .unwrap_or_default();

    for (index, item) in output.iter().enumerate() {
        let index = index as u32;
        let fallback_id = format!("item_{index}");
        let item_id = item
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or(&fallback_id)
            .to_string();
        match item.get("type").and_then(|t| t.as_str()) {
            Some("message") => {
                events.push(sse::message_item_added(index, &item_id));
                let text = item
                    .get("content")
                    .and_then(|content| content.as_array())
                    .map(|parts| {
                        parts
                            .iter()
                            .filter(|part| {
                                part.get("type").and_then(|t| t.as_str()) == Some("output_text")
                            })
                            .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();
                events.push(sse::message_content_part_added(index, &item_id));
                if !text.is_empty() {
                    events.push(sse::output_text_delta(index, &item_id, &text));
                }
                let (close_events, _item) = sse::message_close(index, &item_id, &text);
                events.extend(close_events);
            }
            Some("reasoning") => {
                events.push(sse::reasoning_item_added(index, &item_id));
                let text = item
                    .get("summary")
                    .and_then(|summary| summary.as_array())
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();
                events.push(sse::reasoning_summary_part_added(index, &item_id));
                if !text.is_empty() {
                    events.push(sse::reasoning_summary_text_delta(index, &item_id, &text));
                }
                let (close_events, _item) = sse::reasoning_close(index, &item_id, &text);
                events.extend(close_events);
            }
            _ => {
                // function_call / custom_tool_call / …: emit the finished item
                // directly; the client consumes it from output_item.done.
                events.push(sse::output_item_added(index, item));
                events.push(sse::output_item_done(index, item));
            }
        }
    }

    let mut completed = responses.clone();
    // Preserve the converted terminal status: a truncated answer
    // (finish_reason "length") converts to "incomplete" and must stay so;
    // only default to "completed" when the value carries no status.
    if completed.get("status").and_then(|s| s.as_str()).is_none() {
        completed["status"] = json!("completed");
    }
    events.push(sse::response_completed(&completed));

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_model_override_only_on_kimi_coding_host() {
        let mut body = json!({"model": "k3"});
        apply_echo_model_override("https://api.kimi.com/coding/v1/chat/completions", &mut body);
        assert_eq!(body["model"], json!("kimi-for-coding"));

        let mut body = json!({"model": "kimi-k3"});
        apply_echo_model_override("https://api.moonshot.cn/v1/chat/completions", &mut body);
        assert_eq!(body["model"], json!("kimi-k3"));

        let mut body = json!({"model": "k3"});
        apply_echo_model_override("not a url", &mut body);
        assert_eq!(body["model"], json!("k3"));
    }

    #[test]
    fn support_check_matches_parsed_hostname_only() {
        assert!(base_url_supports_builtin_web_search(
            "https://api.kimi.com/coding/v1"
        ));
        assert!(base_url_supports_builtin_web_search(
            "https://api.moonshot.cn/v1"
        ));
        assert!(base_url_supports_builtin_web_search(
            "https://api.moonshot.ai/v1"
        ));
        // Lookalike hosts and mere mentions in path/query must NOT match.
        assert!(!base_url_supports_builtin_web_search(
            "https://api.kimi.com.example.com/v1"
        ));
        assert!(!base_url_supports_builtin_web_search(
            "https://relay.example.com/kimi.com/v1"
        ));
        assert!(!base_url_supports_builtin_web_search(
            "https://api.openai.com/v1?q=moonshot.cn"
        ));
        assert!(!base_url_supports_builtin_web_search("not a url"));
    }

    #[test]
    fn usage_accumulates_across_rounds() {
        let mut total = json!({});
        accumulate_chat_usage(
            &mut total,
            &json!({
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "total_tokens": 110,
                "prompt_tokens_details": {"cached_tokens": 40},
                "completion_tokens_details": {"reasoning_tokens": 5}
            }),
        );
        accumulate_chat_usage(
            &mut total,
            &json!({
                "prompt_tokens": 50,
                "completion_tokens": 20,
                "total_tokens": 70,
                "prompt_tokens_details": {"cached_tokens": 10},
                "completion_tokens_details": {"reasoning_tokens": 7}
            }),
        );
        assert_eq!(total["prompt_tokens"], json!(150));
        assert_eq!(total["completion_tokens"], json!(30));
        assert_eq!(total["total_tokens"], json!(180));
        assert_eq!(total["prompt_tokens_details"]["cached_tokens"], json!(50));
        assert_eq!(
            total["completion_tokens_details"]["reasoning_tokens"],
            json!(12)
        );
    }

    #[test]
    fn detects_web_search_tool_in_request() {
        assert!(request_has_web_search_tool(&json!({
            "tools": [{"type": "web_search"}]
        })));
        assert!(request_has_web_search_tool(&json!({
            "tools": [{"type": "function", "function": {"name": "x"}}, {"type": "web_search_preview"}]
        })));
        assert!(!request_has_web_search_tool(&json!({
            "tools": [{"type": "function", "function": {"name": "web_search"}}]
        })));
        assert!(!request_has_web_search_tool(&json!({})));
    }

    #[test]
    fn injects_builtin_and_forces_non_stream() {
        let mut body = json!({
            "model": "k3",
            "stream": true,
            "stream_options": {"include_usage": true},
            "tools": [{"type": "function", "function": {"name": "exec", "parameters": {}}}]
        });
        inject_builtin_web_search(&mut body);
        assert_eq!(body["stream"], json!(false));
        assert!(body.get("stream_options").is_none());
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[1]["type"], json!("builtin_function"));
        assert_eq!(tools[1]["function"]["name"], json!("$web_search"));

        // Idempotent: no duplicate builtin on re-injection.
        inject_builtin_web_search(&mut body);
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn echo_round_appends_assistant_and_tool_messages() {
        let mut request = json!({
            "messages": [{"role": "user", "content": "news?"}]
        });
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "t-1",
                        "type": "builtin_function",
                        "function": {"name": "$web_search", "arguments": "{\"search_result\":{}}"}
                    }]
                }
            }]
        });
        assert!(has_builtin_tool_calls(&response));
        assert!(append_echo_messages(&mut request, &response));
        let messages = request["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], json!("assistant"));
        assert_eq!(messages[2]["role"], json!("tool"));
        assert_eq!(messages[2]["tool_call_id"], json!("t-1"));
        assert_eq!(messages[2]["name"], json!("$web_search"));

        let final_response = json!({
            "choices": [{"message": {"role": "assistant", "content": "done"}}]
        });
        assert!(!has_builtin_tool_calls(&final_response));
        assert!(!append_echo_messages(&mut request, &final_response));
    }

    #[test]
    fn synthesizes_sse_for_message_and_function_call() {
        let responses = json!({
            "id": "resp_1",
            "object": "response",
            "created_at": 1,
            "status": "completed",
            "model": "k3",
            "output": [
                {
                    "id": "msg_1",
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "hello", "annotations": []}]
                },
                {
                    "id": "fc_1",
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec",
                    "arguments": "{}",
                    "status": "completed"
                }
            ],
            "usage": {"input_tokens": 1, "output_tokens": 2, "total_tokens": 3}
        });
        let bytes = responses_value_to_sse_bytes(&responses);
        let text = bytes
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("event: response.created"));
        assert!(text.contains("event: response.output_item.added"));
        assert!(text.contains("event: response.output_text.delta"));
        assert!(text.contains("event: response.output_item.done"));
        assert!(text.contains("event: response.completed"));
        assert!(text.contains("\"hello\""));
        assert!(text.contains("\"function_call\""));
    }

    #[test]
    fn synthesized_sse_preserves_incomplete_status() {
        let responses = json!({
            "id": "resp_1",
            "object": "response",
            "created_at": 1,
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "model": "k3",
            "output": [{
                "id": "msg_1",
                "type": "message",
                "status": "incomplete",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "truncated…", "annotations": []}]
            }]
        });
        let bytes = responses_value_to_sse_bytes(&responses);
        let text = bytes
            .iter()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect::<Vec<_>>()
            .join("");
        let completed = text
            .split("\n\n")
            .find(|chunk| chunk.starts_with("event: response.completed"))
            .expect("completed event present");
        assert!(completed.contains("\"status\":\"incomplete\""));
        assert!(!completed.contains("\"status\":\"completed\""));
    }
}
