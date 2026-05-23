//! Codex OpenAI 格式转换模块
//!
//! 实现 OpenAI Responses API ↔ OpenAI Chat Completions API 格式转换。
//! 用于 Codex CLI（仅支持 Responses API）连接到仅支持 Chat Completions 的供应商（如 DeepSeek）。

use bytes::Bytes;
use futures::{stream, Stream};
use serde_json::{json, Value};

/// OpenAI Responses API 请求 → OpenAI Chat Completions 请求
pub fn responses_to_chat_completions(body: Value) -> Value {
    let mut result = json!({});

    if let Some(model) = body.get("model") {
        result["model"] = model.clone();
    }

    let mut messages: Vec<Value> = Vec::new();

    // instructions → system message
    if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
        messages.push(json!({
            "role": "system",
            "content": instructions
        }));
    }

    // input 数组 → messages
    if let Some(input) = body.get("input").and_then(|v| v.as_array()) {
        convert_input_to_messages(input, &mut messages);
    }

    result["messages"] = json!(messages);

    // max_output_tokens → max_tokens
    if let Some(v) = body.get("max_output_tokens") {
        result["max_tokens"] = v.clone();
    }

    for key in &["temperature", "top_p", "stream", "stop"] {
        if let Some(v) = body.get(key) {
            result[key] = v.clone();
        }
    }

    // tools: Responses API → Chat Completions
    // Responses 支持多种 tool type (function, namespace, web_search 等)
    // Chat Completions/DeepSeek 仅支持 function
    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        let mut chat_tools: Vec<Value> = Vec::new();
        for tool in tools {
            let tool_type = tool
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("function");

            match tool_type {
                "function" => {
                    let mut function_obj = json!({});
                    for key in &["name", "description", "parameters", "strict"] {
                        if let Some(v) = tool.get(key) {
                            function_obj[key] = v.clone();
                        }
                    }
                    chat_tools.push(json!({
                        "type": "function",
                        "function": function_obj
                    }));
                }
                // namespace: MCP server tools — 展开内嵌的 functions
                "namespace" => {
                    if let Some(ns_tools) = tool.get("tools").and_then(|v| v.as_array()) {
                        let ns_prefix = tool
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        for ns_tool in ns_tools {
                            if ns_tool.get("type").and_then(|v| v.as_str()) == Some("function")
                            {
                                let mut function_obj = json!({});
                                let orig_name = ns_tool
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                // Prepend namespace: "filesystem/read_file"
                                let qualified_name = if ns_prefix.is_empty() {
                                    orig_name.to_string()
                                } else {
                                    format!("{ns_prefix}/{orig_name}")
                                };
                                function_obj["name"] = json!(qualified_name);
                                for key in &["description", "parameters", "strict"] {
                                    if let Some(v) = ns_tool.get(key) {
                                        function_obj[key] = v.clone();
                                    }
                                }
                                chat_tools.push(json!({
                                    "type": "function",
                                    "function": function_obj
                                }));
                            }
                        }
                    }
                }
                // web_search, code_interpreter 等 — 跳过（DeepSeek 不支持）
                _ => {}
            }
        }
        if !chat_tools.is_empty() {
            result["tools"] = json!(chat_tools);
        }
    }

    // tool_choice: 过滤掉 Chat Completions 不支持的值
    if let Some(v) = body.get("tool_choice") {
        let mapped = match v {
            Value::String(s) => match s.as_str() {
                "auto" | "none" | "required" => v.clone(),
                _ => json!("auto"),
            },
            Value::Object(_) => v.clone(),
            _ => json!("auto"),
        };
        result["tool_choice"] = mapped;
    }

    // reasoning (Responses API) → thinking (DeepSeek)
    if let Some(reasoning) = body.get("reasoning") {
        if let Some(effort) = reasoning.get("effort").and_then(|v| v.as_str()) {
            result["thinking"] = json!({
                "type": "enabled",
                "reasoning_effort": effort
            });
        }
    }

    result
}

fn convert_input_to_messages(input: &[Value], messages: &mut Vec<Value>) {
    for item in input {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match item_type {
            "function_call" => {
                messages.push(json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": item.get("call_id").cloned().unwrap_or_default(),
                        "type": "function",
                        "function": {
                            "name": item.get("name").cloned().unwrap_or_default(),
                            "arguments": item.get("arguments").cloned().unwrap_or(json!("{}"))
                        }
                    }]
                }));
            }

            "function_call_output" => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
                    "content": item.get("output").map(|v| v.as_str().unwrap_or("")).unwrap_or("")
                }));
            }

            // Responses API message items use "role" without an explicit "type" field
            _ => {
                let role = match item.get("role").and_then(|v| v.as_str()) {
                    Some(r) => r,
                    None => continue,
                };
                let resolved_role = match role {
                    "assistant" => "assistant",
                    "developer" => "system",
                    other => other,
                };

                if let Some(content_blocks) = item.get("content").and_then(|v| v.as_array()) {
                    let texts: Vec<&str> = content_blocks
                        .iter()
                        .filter_map(|block| {
                            let bt = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match bt {
                                "input_text" | "output_text" => {
                                    block.get("text").and_then(|v| v.as_str())
                                }
                                "refusal" => block.get("refusal").and_then(|v| v.as_str()),
                                _ => None,
                            }
                        })
                        .collect();

                    if !texts.is_empty() {
                        messages.push(json!({
                            "role": resolved_role,
                            "content": texts.join("")
                        }));
                    } else if content_blocks.iter().any(|b| {
                        b.get("type").and_then(|v| v.as_str()) == Some("input_image")
                    }) {
                        let parts: Vec<Value> = content_blocks
                            .iter()
                            .filter_map(|block| {
                                if block.get("type").and_then(|v| v.as_str()) == Some("input_image")
                                {
                                    Some(json!({
                                        "type": "image_url",
                                        "image_url": block.get("image_url").cloned().unwrap_or(json!({}))
                                    }))
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if !parts.is_empty() {
                            messages.push(json!({
                                "role": resolved_role,
                                "content": parts
                            }));
                        }
                    }
                } else if let Some(text) = item.get("content").and_then(|v| v.as_str()) {
                    messages.push(json!({
                        "role": resolved_role,
                        "content": text
                    }));
                }
            }
        }
    }
}

/// OpenAI Chat Completions 响应 → OpenAI Responses 响应
pub fn chat_completions_to_responses(body: Value) -> Value {
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let choices = body.get("choices").and_then(|v| v.as_array());
    let usage = body.get("usage");

    let mut output: Vec<Value> = Vec::new();

    if let Some(choices) = choices {
        for choice in choices {
            let message = choice.get("message");

            if let Some(message) = message {
                // Text content
                if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        output.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": text}]
                        }));
                    }
                }

                // Tool calls → function_call items
                if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        let function = tc.get("function");
                        output.push(json!({
                            "type": "function_call",
                            "call_id": tc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": function.and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                            "arguments": function.and_then(|f| f.get("arguments").cloned()).unwrap_or(json!("{}"))
                        }));
                    }
                }
            }
        }
    }

    let usage_json = if let Some(usage) = usage {
        json!({
            "input_tokens": usage.get("prompt_tokens").cloned().unwrap_or(json!(0)),
            "output_tokens": usage.get("completion_tokens").cloned().unwrap_or(json!(0)),
            "total_tokens": usage.get("total_tokens").cloned().unwrap_or(json!(0))
        })
    } else {
        json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        })
    };

    json!({
        "id": body.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "object": "response",
        "model": model,
        "output": output,
        "status": "completed",
        "usage": usage_json
    })
}

/// 将完整的 Responses API 响应包装为 SSE 事件流
///
/// 当原始请求为流式但格式转换需降级为非流式时，用此函数生成一次性 SSE 流。
/// Codex CLI 发送 `stream: true` 时依赖 SSE 事件完成解析。
pub fn wrap_responses_as_sse(
    responses_body: Value,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let response_id = responses_body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("resp_chat")
        .to_string();
    let msg_id = format!("msg_{response_id}");

    let model_name = responses_body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let usage_json = responses_body
        .get("usage")
        .cloned()
        .unwrap_or(json!({"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}));
    let initial_usage = json!({
        "input_tokens": usage_json.get("input_tokens").cloned().unwrap_or(json!(0)),
        "output_tokens": 0,
        "total_tokens": usage_json.get("total_tokens").cloned().unwrap_or(json!(0))
    });

    let created = format!(
        "event: response.created\ndata: {}\n\n",
        json!({
            "type": "response.created",
            "response": {
                "id": &response_id,
                "model": model_name,
                "usage": initial_usage
            }
        })
    );

    let completed = format!(
        "event: response.completed\ndata: {}\n\n",
        json!({
            "type": "response.completed",
            "response": &responses_body
        })
    );

    let events: Vec<Result<Bytes, std::io::Error>> = {
        let mut v: Vec<Result<Bytes, std::io::Error>> = Vec::new();

        // response.created MUST be first
        v.push(Ok(Bytes::from(created)));

        // Append per-output-item events for tool calls and text
        if let Some(output) = responses_body.get("output").and_then(|v| v.as_array()) {
            for (i, item) in output.iter().enumerate() {
                let output_index = i;
                let item_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&msg_id)
                    .to_string();

                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match item_type {
                    "message" => {
                        let text = item
                            .get("content")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.first()
                                    .and_then(|c| c.get("text").and_then(|t| t.as_str()))
                            })
                            .unwrap_or("");

                        // output_item.added
                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": output_index,
                                "item": {
                                    "id": &item_id,
                                    "type": "message",
                                    "role": "assistant",
                                    "status": "in_progress",
                                    "content": []
                                }
                            })
                        ))));

                        // content_part.added
                        v.push(Ok(Bytes::from(format!(
                            "event: response.content_part.added\ndata: {}\n\n",
                            json!({
                                "type": "response.content_part.added",
                                "item_id": &item_id,
                                "output_index": output_index,
                                "content_index": 0,
                                "part": {
                                    "type": "output_text",
                                    "text": "",
                                    "annotations": []
                                }
                            })
                        ))));

                        // output_text.delta
                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_text.delta\ndata: {}\n\n",
                            json!({
                                "type": "response.output_text.delta",
                                "item_id": &item_id,
                                "output_index": output_index,
                                "content_index": 0,
                                "delta": text
                            })
                        ))));

                        // content_part.done
                        v.push(Ok(Bytes::from(format!(
                            "event: response.content_part.done\ndata: {}\n\n",
                            json!({
                                "type": "response.content_part.done",
                                "item_id": &item_id,
                                "output_index": output_index,
                                "content_index": 0,
                                "part": {
                                    "type": "output_text",
                                    "text": text,
                                    "annotations": []
                                }
                            })
                        ))));

                        // output_item.done
                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.done\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.done",
                                "output_index": output_index,
                                "item": {
                                    "id": &item_id,
                                    "type": "message",
                                    "role": "assistant",
                                    "status": "completed",
                                    "content": [{
                                        "type": "output_text",
                                        "text": text,
                                        "annotations": []
                                    }]
                                }
                            })
                        ))));
                    }
                    "function_call" => {
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let arguments = item
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");

                        // output_item.added for function_call
                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": output_index,
                                "item": {
                                    "id": &item_id,
                                    "type": "function_call",
                                    "call_id": &item_id,
                                    "name": name,
                                    "arguments": ""
                                }
                            })
                        ))));

                        // function_call_arguments.delta
                        v.push(Ok(Bytes::from(format!(
                            "event: response.function_call_arguments.delta\ndata: {}\n\n",
                            json!({
                                "type": "response.function_call_arguments.delta",
                                "item_id": &item_id,
                                "output_index": output_index,
                                "delta": arguments
                            })
                        ))));

                        // function_call_arguments.done
                        v.push(Ok(Bytes::from(format!(
                            "event: response.function_call_arguments.done\ndata: {}\n\n",
                            json!({
                                "type": "response.function_call_arguments.done",
                                "item_id": &item_id,
                                "output_index": output_index,
                                "arguments": arguments
                            })
                        ))));
                    }
                    _ => {}
                }
            }
        }

        v.push(Ok(Bytes::from(completed)));
        v
    };

    stream::iter(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_responses_to_chat_simple() {
        let input = json!({
            "model": "deepseek-chat",
            "instructions": "You are a helpful assistant.",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]}
            ],
            "max_output_tokens": 1024,
            "stream": false
        });

        let result = responses_to_chat_completions(input);
        assert_eq!(result["model"], "deepseek-chat");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["messages"][0]["role"], "system");
        assert_eq!(
            result["messages"][0]["content"],
            "You are a helpful assistant."
        );
        assert_eq!(result["messages"][1]["role"], "user");
        assert_eq!(result["messages"][1]["content"], "Hello");
    }

    #[test]
    fn test_responses_to_chat_with_tool_calls() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Read file"}]},
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{\"path\": \"/tmp/x\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "content here"}
            ]
        });

        let result = responses_to_chat_completions(input);
        let msgs = result["messages"].as_array().unwrap();

        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Read file");

        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], json!(null));
        let tc = &msgs[1]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "read_file");

        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn test_chat_to_responses_simple() {
        let input = json!({
            "id": "chatcmpl-123",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hi there!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
        });

        let result = chat_completions_to_responses(input);
        assert_eq!(result["id"], "chatcmpl-123");
        assert_eq!(result["object"], "response");
        assert_eq!(result["output"][0]["type"], "message");
        assert_eq!(
            result["output"][0]["content"][0]["text"],
            "Hi there!"
        );
        assert_eq!(result["usage"]["input_tokens"], 5);
        assert_eq!(result["usage"]["output_tokens"], 3);
    }

    #[test]
    fn test_responses_to_chat_no_instructions() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": "ping"}
            ]
        });

        let result = responses_to_chat_completions(input);
        assert_eq!(result["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_responses_to_chat_tools_wrapping() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ],
            "tools": [{
                "type": "function",
                "name": "read_file",
                "description": "Read a file",
                "parameters": {"type": "object", "properties": {}, "required": []},
                "strict": true
            }]
        });

        let result = responses_to_chat_completions(input);
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "read_file");
        assert_eq!(tools[0]["function"]["description"], "Read a file");
        assert_eq!(tools[0]["function"]["strict"], true);
        assert!(tools[0].get("name").is_none());
    }

    #[test]
    fn test_namespace_tools_unwrapped() {
        let input = json!({
            "model": "deepseek-chat",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ],
            "tools": [
                {
                    "type": "namespace",
                    "namespace": "filesystem",
                    "tools": [
                        {
                            "type": "function",
                            "name": "read_file",
                            "description": "Read a file",
                            "parameters": {}
                        },
                        {
                            "type": "function",
                            "name": "write_file",
                            "description": "Write a file",
                            "parameters": {}
                        }
                    ]
                },
                {
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {}
                },
                {
                    "type": "web_search",
                    "name": "search"
                }
            ]
        });

        let result = responses_to_chat_completions(input);
        let tools = result["tools"].as_array().unwrap();
        // 2 namespace functions + 1 plain function = 3; web_search filtered out
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["function"]["name"], "filesystem/read_file");
        assert_eq!(tools[1]["function"]["name"], "filesystem/write_file");
        assert_eq!(tools[2]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_sse_event_ordering() {
        let body = json!({
            "id": "resp_1",
            "object": "response",
            "model": "deepseek-chat",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello"}]
            }],
            "status": "completed",
            "usage": {"input_tokens": 5, "output_tokens": 3, "total_tokens": 8}
        });

        let stream = wrap_responses_as_sse(body);
        let chunks: Vec<Bytes> = futures::executor::block_on(
            futures::StreamExt::collect::<Vec<Result<Bytes, std::io::Error>>>(stream)
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

        let combined = chunks.concat();
        let text = String::from_utf8_lossy(&combined);

        // response.created MUST be the first event
        let created_pos = text.find("event: response.created").unwrap();
        let completed_pos = text.find("event: response.completed").unwrap();
        let first_output = text.find("event: response.output_item.added").unwrap();

        assert!(
            created_pos < first_output,
            "response.created ({created_pos}) must come before first output_item.added ({first_output})\nFull SSE:\n{text}"
        );
        assert!(
            completed_pos > first_output,
            "response.completed ({completed_pos}) must come after output events ({first_output})\nFull SSE:\n{text}"
        );

        // Parse the first event (response.created) and verify required fields
        let created_data_str = text
            .lines()
            .skip_while(|l| !l.starts_with("data: "))
            .next()
            .unwrap()
            .strip_prefix("data: ")
            .unwrap();
        let created_data: Value = serde_json::from_str(created_data_str).unwrap();
        let created_response = &created_data["response"];

        assert_eq!(
            created_response["id"], "resp_1",
            "response.created must include id"
        );
        assert_eq!(
            created_response["model"], "deepseek-chat",
            "response.created must include model\nFull SSE:\n{text}"
        );
        assert!(
            created_response.get("usage").is_some(),
            "response.created must include usage\nFull SSE:\n{text}"
        );
    }
}
