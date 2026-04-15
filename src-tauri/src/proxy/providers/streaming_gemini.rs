use super::transform_gemini::{build_anthropic_usage_from_gemini, map_gemini_finish_reason};
use crate::proxy::sse::strip_sse_field;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashSet;

pub fn create_anthropic_sse_stream_from_gemini<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut message_id: Option<String> = None;
        let mut current_model: Option<String> = None;
        let mut has_sent_message_start = false;
        let mut next_content_index: u32 = 0;
        let mut current_text_index: Option<u32> = None;
        let mut open_indices: HashSet<u32> = HashSet::new();
        let mut open_tool_indices: Vec<u32> = Vec::new();
        let mut last_usage: Option<Value> = None;
        let mut has_tool_use = false;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(pos) = buffer.find("\n\n") {
                        let block = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        if block.trim().is_empty() {
                            continue;
                        }

                        for line in block.lines() {
                            if let Some(data) = strip_sse_field(line, "data") {
                                if data.trim().is_empty() {
                                    continue;
                                }

                                let chunk_json: Value = match serde_json::from_str(data) {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                let response = chunk_json.get("response").unwrap_or(&chunk_json);

                                if message_id.is_none() {
                                    message_id = response
                                        .get("responseId")
                                        .or_else(|| chunk_json.get("traceId"))
                                        .and_then(|v| v.as_str())
                                        .map(ToString::to_string);
                                }
                                if current_model.is_none() {
                                    current_model = response
                                        .get("modelVersion")
                                        .and_then(|v| v.as_str())
                                        .map(ToString::to_string);
                                }
                                if let Some(usage) = response.get("usageMetadata") {
                                    last_usage = Some(build_anthropic_usage_from_gemini(Some(usage)));
                                }

                                let candidate = response
                                    .get("candidates")
                                    .and_then(|v| v.as_array())
                                    .and_then(|arr| arr.first())
                                    .cloned()
                                    .unwrap_or_else(|| json!({}));
                                let parts = candidate
                                    .get("content")
                                    .and_then(|v| v.get("parts"))
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();

                                if !has_sent_message_start {
                                    let event = json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": message_id.clone().unwrap_or_else(|| "msg_gemini".to_string()),
                                            "type": "message",
                                            "role": "assistant",
                                            "model": current_model.clone().unwrap_or_else(|| "gemini".to_string()),
                                            "usage": last_usage.clone().unwrap_or_else(|| json!({
                                                "input_tokens": 0,
                                                "output_tokens": 0
                                            }))
                                        }
                                    });
                                    let sse = format!(
                                        "event: message_start\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default()
                                    );
                                    yield Ok(Bytes::from(sse));
                                    has_sent_message_start = true;
                                }

                                for part in parts {
                                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                        if !text.is_empty() {
                                            let index = if let Some(index) = current_text_index {
                                                index
                                            } else {
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                let start_event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": {
                                                        "type": "text",
                                                        "text": ""
                                                    }
                                                });
                                                let start_sse = format!(
                                                    "event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&start_event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(start_sse));
                                                open_indices.insert(index);
                                                current_text_index = Some(index);
                                                index
                                            };

                                            let delta_event = json!({
                                                "type": "content_block_delta",
                                                "index": index,
                                                "delta": {
                                                    "type": "text_delta",
                                                    "text": text
                                                }
                                            });
                                            let delta_sse = format!(
                                                "event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&delta_event).unwrap_or_default()
                                            );
                                            yield Ok(Bytes::from(delta_sse));
                                        }
                                    }

                                    if let Some(function_call) = part.get("functionCall") {
                                        has_tool_use = true;
                                        if let Some(index) = current_text_index.take() {
                                            if open_indices.remove(&index) {
                                                let stop_event = json!({
                                                    "type": "content_block_stop",
                                                    "index": index
                                                });
                                                let stop_sse = format!(
                                                    "event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&stop_event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(stop_sse));
                                            }
                                        }

                                        let index = next_content_index;
                                        next_content_index += 1;
                                        open_indices.insert(index);
                                        open_tool_indices.push(index);

                                        let start_event = json!({
                                            "type": "content_block_start",
                                            "index": index,
                                            "content_block": {
                                                "type": "tool_use",
                                                "id": function_call
                                                    .get("id")
                                                    .and_then(|v| v.as_str())
                                                    .map(ToString::to_string)
                                                    .unwrap_or_else(|| format!("toolu_gemini_{index}")),
                                                "name": function_call
                                                    .get("name")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                            }
                                        });
                                        let start_sse = format!(
                                            "event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&start_event).unwrap_or_default()
                                        );
                                        yield Ok(Bytes::from(start_sse));

                                        let args = function_call.get("args").cloned().unwrap_or_else(|| json!({}));
                                        let partial_json = match args {
                                            Value::String(text) => text,
                                            other => serde_json::to_string(&other).unwrap_or_else(|_| "{}".to_string()),
                                        };
                                        if !partial_json.is_empty() {
                                            let delta_event = json!({
                                                "type": "content_block_delta",
                                                "index": index,
                                                "delta": {
                                                    "type": "input_json_delta",
                                                    "partial_json": partial_json
                                                }
                                            });
                                            let delta_sse = format!(
                                                "event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&delta_event).unwrap_or_default()
                                            );
                                            yield Ok(Bytes::from(delta_sse));
                                        }
                                    }
                                }

                                if let Some(finish_reason) = candidate.get("finishReason").and_then(|v| v.as_str()) {
                                    if let Some(index) = current_text_index.take() {
                                        if open_indices.remove(&index) {
                                            let stop_event = json!({
                                                "type": "content_block_stop",
                                                "index": index
                                            });
                                            let stop_sse = format!(
                                                "event: content_block_stop\ndata: {}\n\n",
                                                serde_json::to_string(&stop_event).unwrap_or_default()
                                            );
                                            yield Ok(Bytes::from(stop_sse));
                                        }
                                    }

                                    if !open_tool_indices.is_empty() {
                                        for index in open_tool_indices.drain(..) {
                                            if open_indices.remove(&index) {
                                                let stop_event = json!({
                                                    "type": "content_block_stop",
                                                    "index": index
                                                });
                                                let stop_sse = format!(
                                                    "event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&stop_event).unwrap_or_default()
                                                );
                                                yield Ok(Bytes::from(stop_sse));
                                            }
                                        }
                                    }

                                    let delta_event = json!({
                                        "type": "message_delta",
                                        "delta": {
                                            "stop_reason": map_gemini_finish_reason(Some(finish_reason), has_tool_use),
                                            "stop_sequence": null
                                        },
                                        "usage": last_usage.clone()
                                    });
                                    let delta_sse = format!(
                                        "event: message_delta\ndata: {}\n\n",
                                        serde_json::to_string(&delta_event).unwrap_or_default()
                                    );
                                    yield Ok(Bytes::from(delta_sse));

                                    let stop_event = json!({ "type": "message_stop" });
                                    let stop_sse = format!(
                                        "event: message_stop\ndata: {}\n\n",
                                        serde_json::to_string(&stop_event).unwrap_or_default()
                                    );
                                    yield Ok(Bytes::from(stop_sse));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse = format!(
                        "event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default()
                    );
                    yield Ok(Bytes::from(sse));
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;

    #[tokio::test]
    async fn gemini_stream_text_converts_to_anthropic_sse() {
        let input = concat!(
            "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"gemini-3.1-pro-preview\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":1}}\n\n",
            "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"gemini-3.1-pro-preview\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":1}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_gemini(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("\"type\":\"message_start\""));
        assert!(merged.contains("\"type\":\"text_delta\""));
        assert!(merged.contains("\"text\":\"Hello\""));
        assert!(merged.contains("\"stop_reason\":\"end_turn\""));
        assert!(merged.contains("\"type\":\"message_stop\""));
    }

    #[tokio::test]
    async fn gemini_stream_function_call_converts_to_tool_use_sse() {
        let input = concat!(
            "data: {\"responseId\":\"resp_2\",\"modelVersion\":\"gemini-3.1-pro-preview\",\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"lookup_weather\",\"args\":{\"city\":\"Tokyo\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":8,\"candidatesTokenCount\":2}}\n\n",
            "data: {\"responseId\":\"resp_2\",\"modelVersion\":\"gemini-3.1-pro-preview\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[]}}],\"usageMetadata\":{\"promptTokenCount\":8,\"candidatesTokenCount\":2}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_gemini(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("\"type\":\"tool_use\""));
        assert!(merged.contains("\"name\":\"lookup_weather\""));
        assert!(merged.contains("\"type\":\"input_json_delta\""));
        assert!(merged.contains("{\\\"city\\\":\\\"Tokyo\\\"}"));
        assert!(merged.contains("\"stop_reason\":\"tool_use\""));
    }

    #[tokio::test]
    async fn wrapped_code_assist_stream_converts_to_anthropic_sse() {
        let input = concat!(
            "data: {\"traceId\":\"trace_123\",\"response\":{\"modelVersion\":\"gemini-3.1-pro-preview\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"PONG\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":1}}}\n\n",
            "data: {\"traceId\":\"trace_123\",\"response\":{\"modelVersion\":\"gemini-3.1-pro-preview\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":1}}}\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream_from_gemini(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        assert!(merged.contains("trace_123"));
        assert!(merged.contains("\"text\":\"PONG\""));
        assert!(merged.contains("\"type\":\"message_stop\""));
    }
}
