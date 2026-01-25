//! 流式响应转换模块
//!
//! 实现 OpenAI SSE → Anthropic SSE 格式转换

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// OpenAI 流式响应数据结构
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    id: String,
    model: String,
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>, // OpenRouter 的推理内容
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// OpenAI 流式响应的 usage 信息（完整版）
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

/// 创建 Anthropic SSE 流
pub fn create_anthropic_sse_stream(
    stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut content_index = 0;
        let mut has_sent_message_start = false;
        let mut current_block_type: Option<String> = None;
        // 使用 HashMap 按 index 管理多个工具调用的 ID 和 content_index
        let mut tool_calls_map: HashMap<usize, (String, usize)> = HashMap::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    while let Some(pos) = buffer.find("\n\n") {
                        let line = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        if line.trim().is_empty() {
                            continue;
                        }

                        for l in line.lines() {
                            if let Some(data) = l.strip_prefix("data: ") {
                                if data.trim() == "[DONE]" {
                                    log::debug!("[Transform] <<< OpenAI SSE: [DONE]");
                                    let event = json!({"type": "message_stop"});
                                    let sse_data = format!("event: message_stop\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    log::debug!("[Transform] >>> Anthropic SSE: message_stop");
                                    yield Ok(Bytes::from(sse_data));
                                    continue;
                                }

                                if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data) {
                                    log::debug!("[Transform] <<< SSE chunk received");

                                    if message_id.is_none() {
                                        message_id = Some(chunk.id.clone());
                                    }
                                    if current_model.is_none() {
                                        current_model = Some(chunk.model.clone());
                                    }

                                    if let Some(choice) = chunk.choices.first() {
                                        if !has_sent_message_start {
                                            let event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": {
                                                        "input_tokens": 0,
                                                        "output_tokens": 0
                                                    }
                                                }
                                            });
                                            let sse_data = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                            has_sent_message_start = true;
                                        }

                                        // 处理 reasoning（thinking）
                                        if let Some(reasoning) = &choice.delta.reasoning {
                                            if current_block_type.is_none() {
                                                let event = json!({
                                                    "type": "content_block_start",
                                                    "index": content_index,
                                                    "content_block": {
                                                        "type": "thinking",
                                                        "thinking": ""
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                                current_block_type = Some("thinking".to_string());
                                            }

                                            let event = json!({
                                                "type": "content_block_delta",
                                                "index": content_index,
                                                "delta": {
                                                    "type": "thinking_delta",
                                                    "thinking": reasoning
                                                }
                                            });
                                            let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                        }

                                        // 处理文本内容
                                        if let Some(content) = &choice.delta.content {
                                            if !content.is_empty() {
                                                if current_block_type.as_deref() != Some("text") {
                                                    if current_block_type.is_some() {
                                                        let event = json!({
                                                            "type": "content_block_stop",
                                                            "index": content_index
                                                        });
                                                        let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                        content_index += 1;
                                                    }

                                                    let event = json!({
                                                        "type": "content_block_start",
                                                        "index": content_index,
                                                        "content_block": {
                                                            "type": "text",
                                                            "text": ""
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                    current_block_type = Some("text".to_string());
                                                }

                                                let event = json!({
                                                    "type": "content_block_delta",
                                                    "index": content_index,
                                                    "delta": {
                                                        "type": "text_delta",
                                                        "text": content
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                        }

                                        // 处理工具调用
                                        if let Some(tool_calls) = &choice.delta.tool_calls {
                                            for tool_call in tool_calls {
                                                let tc_index = tool_call.index;

                                                // 检查是否是新的工具调用（有 id 表示开始新的工具调用）
                                                if let Some(id) = &tool_call.id {
                                                    // 关闭当前的 content block（如果有）
                                                    if current_block_type.is_some() {
                                                        let event = json!({
                                                            "type": "content_block_stop",
                                                            "index": content_index
                                                        });
                                                        let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                        content_index += 1;
                                                    }

                                                    // 记录这个工具调用的 ID 和对应的 content_index
                                                    tool_calls_map.insert(tc_index, (id.clone(), content_index));
                                                    current_block_type = Some("tool_use".to_string());
                                                }

                                                // 获取当前工具调用的信息
                                                let (tool_id, tool_content_index) = tool_calls_map
                                                    .get(&tc_index)
                                                    .cloned()
                                                    .unwrap_or_else(|| {
                                                        log::warn!(
                                                            "[Transform] 收到未知 index 的工具调用 delta: {tc_index}"
                                                        );
                                                        (String::new(), content_index)
                                                    });

                                                if let Some(function) = &tool_call.function {
                                                    // 如果有 name，发送 content_block_start
                                                    if let Some(name) = &function.name {
                                                        let event = json!({
                                                            "type": "content_block_start",
                                                            "index": tool_content_index,
                                                            "content_block": {
                                                                "type": "tool_use",
                                                                "id": tool_id,
                                                                "name": name
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }

                                                    // 如果有 arguments，发送 content_block_delta
                                                    if let Some(args) = &function.arguments {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": tool_content_index,
                                                            "delta": {
                                                                "type": "input_json_delta",
                                                                "partial_json": args
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                }
                                            }
                                        }

                                        // 处理 finish_reason
                                        if let Some(finish_reason) = &choice.finish_reason {
                                            if current_block_type.is_some() {
                                                let event = json!({
                                                    "type": "content_block_stop",
                                                    "index": content_index
                                                });
                                                let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                            }

                                            let stop_reason = map_stop_reason(Some(finish_reason));
                                            // 构建 usage 信息，包含 input_tokens 和 output_tokens
                                            let usage_json = chunk.usage.as_ref().map(|u| json!({
                                                "input_tokens": u.prompt_tokens,
                                                "output_tokens": u.completion_tokens
                                            }));
                                            let event = json!({
                                                "type": "message_delta",
                                                "delta": {
                                                    "stop_reason": stop_reason,
                                                    "stop_sequence": null
                                                },
                                                "usage": usage_json
                                            });
                                            let sse_data = format!("event: message_delta\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Stream error: {e}");
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse_data = format!("event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default());
                    yield Ok(Bytes::from(sse_data));
                    break;
                }
            }
        }
    }
}

/// 映射停止原因
fn map_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason.map(|r| {
        match r {
            "tool_calls" => "tool_use",
            "stop" => "end_turn",
            "length" => "max_tokens",
            _ => "end_turn",
        }
        .to_string()
    })
}
