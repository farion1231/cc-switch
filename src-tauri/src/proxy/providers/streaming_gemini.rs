//! Gemini Native streaming conversion module.
//!
//! Converts Gemini `streamGenerateContent?alt=sse` chunks into Anthropic-style
//! SSE events for Claude-compatible clients.

use super::gemini_shadow::{GeminiShadowStore, GeminiToolCallMeta};
use super::transform_gemini::{rectify_tool_call_parts, AnthropicToolSchemaHints};
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

fn anthropic_usage_from_gemini(usage: Option<&Value>) -> Value {
    let Some(usage) = usage else {
        return json!({
            "input_tokens": 0,
            "output_tokens": 0
        });
    };

    let input_tokens = usage
        .get("promptTokenCount")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("totalTokenCount")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let output_tokens = total_tokens.saturating_sub(input_tokens);

    let mut result = json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    });

    if let Some(cached) = usage
        .get("cachedContentTokenCount")
        .and_then(|value| value.as_u64())
    {
        result["cache_read_input_tokens"] = json!(cached);
    }

    result
}

fn map_finish_reason(reason: Option<&str>, has_tool_use: bool, blocked: bool) -> &'static str {
    if blocked {
        return "refusal";
    }

    match reason {
        Some("MAX_TOKENS") => "max_tokens",
        Some("SAFETY")
        | Some("RECITATION")
        | Some("SPII")
        | Some("BLOCKLIST")
        | Some("PROHIBITED_CONTENT") => "refusal",
        _ if has_tool_use => "tool_use",
        _ => "end_turn",
    }
}

fn extract_visible_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("thought").and_then(|value| value.as_bool()) != Some(true))
        .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
        .collect::<String>()
}

fn extract_tool_calls(
    parts: &[Value],
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> Vec<GeminiToolCallMeta> {
    let mut rectified_parts = parts.to_vec();
    rectify_tool_call_parts(&mut rectified_parts, tool_schema_hints);

    rectified_parts
        .iter()
        .filter_map(|part| {
            let function_call = part.get("functionCall")?;
            Some(GeminiToolCallMeta::new(
                function_call.get("id").and_then(|value| value.as_str()),
                function_call
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                function_call
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                part.get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|value| value.as_str()),
            ))
        })
        .collect()
}

fn extract_text_thought_signature(parts: &[Value]) -> Option<String> {
    parts
        .iter()
        .filter(|part| part.get("text").is_some() && part.get("functionCall").is_none())
        .filter_map(|part| {
            part.get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(|value| value.as_str())
        })
        .next_back()
        .map(ToString::to_string)
}

fn merge_tool_call_snapshots(
    tool_call_snapshots: &mut Vec<GeminiToolCallMeta>,
    incoming: Vec<GeminiToolCallMeta>,
) {
    for tool_call in incoming {
        let existing_index =
            tool_call_snapshots
                .iter()
                .position(|existing| match (&existing.id, &tool_call.id) {
                    (Some(existing_id), Some(incoming_id)) => existing_id == incoming_id,
                    _ => existing.name == tool_call.name,
                });

        if let Some(index) = existing_index {
            tool_call_snapshots[index] = tool_call;
        } else {
            tool_call_snapshots.push(tool_call);
        }
    }
}

fn build_shadow_assistant_parts(
    text: Option<&str>,
    text_thought_signature: Option<&str>,
    tool_calls: &[GeminiToolCallMeta],
) -> Vec<Value> {
    let mut parts = Vec::new();

    if text.filter(|text| !text.is_empty()).is_some() || text_thought_signature.is_some() {
        let mut part = json!({
            "text": text.unwrap_or("")
        });
        if let Some(signature) = text_thought_signature {
            part["thoughtSignature"] = json!(signature);
        }
        parts.push(part);
    }

    for tool_call in tool_calls {
        let mut part = json!({
            "functionCall": {
                "id": tool_call.id.clone().unwrap_or_default(),
                "name": tool_call.name,
                "args": tool_call.args
            }
        });

        if let Some(signature) = &tool_call.thought_signature {
            part["thoughtSignature"] = json!(signature);
        }

        parts.push(part);
    }

    parts
}

fn encode_sse(event_name: &str, payload: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

pub fn create_anthropic_sse_stream_from_gemini<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    shadow_store: Option<Arc<GeminiShadowStore>>,
    provider_id: Option<String>,
    session_id: Option<String>,
    tool_schema_hints: Option<AnthropicToolSchemaHints>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut message_id: Option<String> = None;
        let mut current_model: Option<String> = None;
        let mut has_sent_message_start = false;
        let mut accumulated_text = String::new();
        let mut text_block_index: Option<u32> = None;
        let mut next_content_index: u32 = 0;
        let mut open_indices: HashSet<u32> = HashSet::new();
        let mut tool_call_snapshots: Vec<GeminiToolCallMeta> = Vec::new();
        let mut text_thought_signature: Option<String> = None;
        let mut latest_usage: Option<Value> = None;
        let mut latest_finish_reason: Option<String> = None;
        let mut blocked_text: Option<String> = None;
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }

                        let mut data_lines: Vec<String> = Vec::new();
                        for line in block.lines() {
                            if let Some(data) = strip_sse_field(line, "data") {
                                data_lines.push(data.to_string());
                            }
                        }

                        if data_lines.is_empty() {
                            continue;
                        }

                        let data = data_lines.join("\n");
                        if data.trim() == "[DONE]" {
                            break;
                        }

                        let chunk_json: Value = match serde_json::from_str(&data) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };

                        if message_id.is_none() {
                            message_id = chunk_json
                                .get("responseId")
                                .and_then(|value| value.as_str())
                                .map(ToString::to_string);
                        }
                        if current_model.is_none() {
                            current_model = chunk_json
                                .get("modelVersion")
                                .and_then(|value| value.as_str())
                                .map(ToString::to_string);
                        }
                        if latest_usage.is_none() {
                            latest_usage = chunk_json.get("usageMetadata").cloned();
                        }

                        if !has_sent_message_start {
                            let event = json!({
                                "type": "message_start",
                                "message": {
                                    "id": message_id.clone().unwrap_or_default(),
                                    "type": "message",
                                    "role": "assistant",
                                    "model": current_model.clone().unwrap_or_default(),
                                    "usage": anthropic_usage_from_gemini(chunk_json.get("usageMetadata"))
                                }
                            });
                            yield Ok(encode_sse("message_start", &event));
                            has_sent_message_start = true;
                        }

                        if let Some(reason) = chunk_json
                            .get("promptFeedback")
                            .and_then(|value| value.get("blockReason"))
                            .and_then(|value| value.as_str())
                        {
                            blocked_text = Some(format!("Request blocked by Gemini safety filters: {reason}"));
                        }

                        if let Some(candidate) = chunk_json
                            .get("candidates")
                            .and_then(|value| value.as_array())
                            .and_then(|value| value.first())
                        {
                            if let Some(reason) = candidate.get("finishReason").and_then(|value| value.as_str()) {
                                latest_finish_reason = Some(reason.to_string());
                            }
                            if let Some(usage) = chunk_json.get("usageMetadata") {
                                latest_usage = Some(usage.clone());
                            }
                            if let Some(parts) = candidate
                                .get("content")
                                .and_then(|value| value.get("parts"))
                                .and_then(|value| value.as_array())
                            {
                                let mut rectified_parts = parts.clone();
                                rectify_tool_call_parts(&mut rectified_parts, tool_schema_hints.as_ref());
                                if let Some(signature) = extract_text_thought_signature(parts) {
                                    text_thought_signature = Some(signature);
                                }
                                merge_tool_call_snapshots(
                                    &mut tool_call_snapshots,
                                    extract_tool_calls(&rectified_parts, tool_schema_hints.as_ref()),
                                );
                                let visible_text = extract_visible_text(&rectified_parts);
                                if !visible_text.is_empty() {
                                    let is_cumulative = visible_text.starts_with(&accumulated_text);
                                    let delta = if is_cumulative {
                                        visible_text[accumulated_text.len()..].to_string()
                                    } else {
                                        visible_text.clone()
                                    };

                                    if !delta.is_empty() {
                                        let index = *text_block_index.get_or_insert_with(|| {
                                            let assigned = next_content_index;
                                            next_content_index += 1;
                                            assigned
                                        });

                                        if !open_indices.contains(&index) {
                                            let start_event = json!({
                                                "type": "content_block_start",
                                                "index": index,
                                                "content_block": {
                                                    "type": "text",
                                                    "text": ""
                                                }
                                            });
                                            yield Ok(encode_sse("content_block_start", &start_event));
                                            open_indices.insert(index);
                                        }

                                        let delta_event = json!({
                                            "type": "content_block_delta",
                                            "index": index,
                                            "delta": {
                                                "type": "text_delta",
                                                "text": delta
                                            }
                                        });
                                        yield Ok(encode_sse("content_block_delta", &delta_event));
                                        if is_cumulative {
                                            accumulated_text = visible_text;
                                        } else {
                                            accumulated_text.push_str(&delta);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            }
        }

        if !has_sent_message_start {
            let event = json!({
                "type": "message_start",
                "message": {
                    "id": message_id.clone().unwrap_or_default(),
                    "type": "message",
                    "role": "assistant",
                    "model": current_model.clone().unwrap_or_default(),
                    "usage": anthropic_usage_from_gemini(latest_usage.as_ref())
                }
            });
            yield Ok(encode_sse("message_start", &event));
        }

        if accumulated_text.is_empty() {
            if let Some(blocked_text) = blocked_text.clone() {
                let index = *text_block_index.get_or_insert_with(|| {
                    let assigned = next_content_index;
                    next_content_index += 1;
                    assigned
                });

                if !open_indices.contains(&index) {
                    let start_event = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "text",
                            "text": ""
                        }
                    });
                    yield Ok(encode_sse("content_block_start", &start_event));
                    open_indices.insert(index);
                }

                let delta_event = json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {
                        "type": "text_delta",
                        "text": blocked_text
                    }
                });
                yield Ok(encode_sse("content_block_delta", &delta_event));
            }
        }

        if let Some(index) = text_block_index {
            if open_indices.remove(&index) {
                let stop_event = json!({
                    "type": "content_block_stop",
                    "index": index
                });
                yield Ok(encode_sse("content_block_stop", &stop_event));
            }
        }

        let tool_calls = tool_call_snapshots;
        for tool_call in &tool_calls {
            let index = next_content_index;
            next_content_index += 1;

            let start_event = json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "tool_use",
                    "id": tool_call.id.clone().unwrap_or_default(),
                    "name": tool_call.name
                }
            });
            yield Ok(encode_sse("content_block_start", &start_event));

            let delta_event = json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": serde_json::to_string(&tool_call.args).unwrap_or_else(|_| "{}".to_string())
                }
            });
            yield Ok(encode_sse("content_block_delta", &delta_event));

            let stop_event = json!({
                "type": "content_block_stop",
                "index": index
            });
            yield Ok(encode_sse("content_block_stop", &stop_event));
        }

        if let (Some(store), Some(provider_id), Some(session_id)) = (
            shadow_store.as_ref(),
            provider_id.as_deref(),
            session_id.as_deref(),
        ) {
            let shadow_text = if accumulated_text.is_empty() {
                blocked_text.as_deref()
            } else {
                Some(accumulated_text.as_str())
            };
            let shadow_parts = build_shadow_assistant_parts(
                shadow_text,
                text_thought_signature.as_deref(),
                &tool_calls,
            );
            if !shadow_parts.is_empty() {
                store.record_assistant_turn(
                    provider_id,
                    session_id,
                    json!({ "parts": shadow_parts }),
                    tool_calls.clone(),
                );
            }
        }

        let stop_reason = map_finish_reason(
            latest_finish_reason.as_deref(),
            !tool_calls.is_empty(),
            blocked_text.is_some(),
        );
        let usage = anthropic_usage_from_gemini(latest_usage.as_ref());
        let message_delta = json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": Value::Null
            },
            "usage": usage
        });
        yield Ok(encode_sse("message_delta", &message_delta));

        let message_stop = json!({ "type": "message_stop" });
        yield Ok(encode_sse("message_stop", &message_stop));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::gemini_shadow::GeminiShadowStore;
    use crate::proxy::providers::transform_gemini::anthropic_to_gemini_with_shadow;
    use std::sync::Arc;

    fn collect_stream_output(chunks: Vec<&str>) -> String {
        let owned_chunks: Vec<String> = chunks.into_iter().map(ToString::to_string).collect();
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let converted = create_anthropic_sse_stream_from_gemini(stream, None, None, None, None);
        futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        })
    }

    fn collect_stream_output_with_shadow(
        chunks: Vec<&str>,
        store: Arc<GeminiShadowStore>,
        provider_id: &str,
        session_id: &str,
    ) -> String {
        let owned_chunks: Vec<String> = chunks.into_iter().map(ToString::to_string).collect();
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let converted = create_anthropic_sse_stream_from_gemini(
            stream,
            Some(store),
            Some(provider_id.to_string()),
            Some(session_id.to_string()),
            None,
        );
        futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        })
    }

    #[test]
    fn converts_text_stream_to_anthropic_sse() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}],\"usageMetadata\":{\"promptTokenCount\":10,\"totalTokenCount\":13}}\n\n",
            "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}],\"usageMetadata\":{\"promptTokenCount\":10,\"totalTokenCount\":15}}\n\n",
        ]);

        assert!(output.contains("event: message_start"));
        assert!(output.contains("\"type\":\"text_delta\""));
        assert!(output.contains("\"text\":\"Hel\""));
        assert!(output.contains("\"text\":\"lo\""));
        assert!(output.contains("\"stop_reason\":\"end_turn\""));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn converts_function_call_stream_to_tool_use_events() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"resp_2\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}},\"thoughtSignature\":\"sig-1\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n",
        ]);

        assert!(output.contains("\"type\":\"tool_use\""));
        assert!(output.contains("\"name\":\"get_weather\""));
        assert!(output.contains("\"type\":\"input_json_delta\""));
        assert!(output.contains("\"stop_reason\":\"tool_use\""));
    }

    #[test]
    fn converts_crlf_delimited_stream_to_anthropic_sse() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"resp_3\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\r\n\r\n",
            "data: {\"responseId\":\"resp_3\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"Hi there\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":9}}\r\n\r\n",
        ]);

        assert!(output.contains("event: message_start"));
        assert!(output.contains("\"type\":\"text_delta\""));
        assert!(output.contains("\"text\":\"Hi\""));
        assert!(output.contains("\"text\":\" there\""));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn stores_full_text_for_shadow_replay_across_delta_chunks() {
        let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
        let output = collect_stream_output_with_shadow(
            vec![
                "data: {\"responseId\":\"resp_4\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\n\n",
                "data: {\"responseId\":\"resp_4\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"lo\"},{\"text\":\"\",\"thoughtSignature\":\"sig-1\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":8}}\n\n",
            ],
            store.clone(),
            "provider-a",
            "session-1",
        );

        assert!(output.contains("\"text\":\"Hel\""));
        assert!(output.contains("\"text\":\"lo\""));

        let shadow = store
            .latest_assistant_content("provider-a", "session-1")
            .unwrap();
        assert_eq!(shadow["parts"][0]["text"], "Hello");
        assert_eq!(shadow["parts"][0]["thoughtSignature"], "sig-1");

        let second_turn = anthropic_to_gemini_with_shadow(
            json!({
                "messages": [
                    { "role": "user", "content": "Hi" },
                    { "role": "assistant", "content": [{ "type": "text", "text": "Hello" }] },
                    { "role": "user", "content": "Continue" }
                ]
            }),
            Some(store.as_ref()),
            Some("provider-a"),
            Some("session-1"),
        )
        .unwrap();

        assert_eq!(second_turn["contents"][1]["role"], "model");
        assert_eq!(second_turn["contents"][1]["parts"][0]["text"], "Hello");
        assert_eq!(
            second_turn["contents"][1]["parts"][0]["thoughtSignature"],
            "sig-1"
        );
    }

    #[test]
    fn rectifies_streamed_tool_call_args_from_tool_schema_hints() {
        let owned_chunks = vec![
            "data: {\"responseId\":\"resp_5\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"Bash\",\"args\":{\"args\":\"git status\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n".to_string(),
        ];
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let hints = super::super::transform_gemini::extract_anthropic_tool_schema_hints(&json!({
            "tools": [{
                "name": "Bash",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "timeout": { "type": "number" }
                    },
                    "required": ["command"]
                }
            }]
        }));
        let converted =
            create_anthropic_sse_stream_from_gemini(stream, None, None, None, Some(hints));
        let output = futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        });

        assert!(output.contains("\"partial_json\":\"{\\\"command\\\":\\\"git status\\\"}\""));
    }

    #[test]
    fn rectifies_streamed_skill_args_from_nested_parameters() {
        let payload = json!({
            "responseId": "resp_6",
            "modelVersion": "gemini-2.5-pro",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{
                        "functionCall": {
                            "id": "call_1",
                            "name": "Skill",
                            "args": {
                                "name": "git-commit",
                                "parameters": {
                                    "args": ["详细分析内容 编写提交信息 分多次提交代码"]
                                }
                            }
                        }
                    }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "totalTokenCount": 8
            }
        });
        let owned_chunks = vec![format!(
            "data: {}\n\n",
            serde_json::to_string(&payload).unwrap()
        )];
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let hints = super::super::transform_gemini::extract_anthropic_tool_schema_hints(&json!({
            "tools": [{
                "name": "Skill",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "skill": { "type": "string" },
                        "args": { "type": "string" }
                    },
                    "required": ["skill"]
                }
            }]
        }));
        let converted =
            create_anthropic_sse_stream_from_gemini(stream, None, None, None, Some(hints));
        let output = futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        });

        assert!(output.contains("git-commit"));
        assert!(output.contains("详细分析内容 编写提交信息 分多次提交代码"));
        assert!(!output.contains("\\\"parameters\\\""));
    }
}
