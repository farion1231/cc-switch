//! Codex OpenAI 格式转换模块
//!
//! 实现 OpenAI Responses API ↔ OpenAI Chat Completions API 格式转换。
//! 用于 Codex CLI（仅支持 Responses API）连接到仅支持 Chat Completions 的供应商（如 DeepSeek）。

use bytes::Bytes;
use futures::{stream, Stream};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// DeepSeek 要求 reasoning_content 必须在后续请求中原样回传。
/// 此缓存按 tool_call.id 存储 reasoning_content，跨请求注入。
static REASONING_CACHE: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 缓存 reasoning_content，按 tool_call IDs
pub fn cache_reasoning_for_tool_calls(reasoning: &str, tool_call_ids: &[String]) {
    if reasoning.is_empty() || tool_call_ids.is_empty() {
        return;
    }
    if let Ok(mut cache) = REASONING_CACHE.lock() {
        for id in tool_call_ids {
            cache.insert(id.clone(), reasoning.to_string());
        }
    }
}

/// 按 tool_call ID 查找缓存的 reasoning_content
fn get_cached_reasoning(tool_call_id: &str) -> Option<String> {
    REASONING_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(tool_call_id).cloned())
}

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
    let mut pending_tool_calls: Vec<Value> = Vec::new();

    for item in input {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match item_type {
            "function_call" => {
                let call_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                // Accumulate consecutive function_calls into one assistant message
                pending_tool_calls.push(json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": item.get("name").cloned().unwrap_or_default(),
                        "arguments": item.get("arguments").cloned().unwrap_or(json!("{}"))
                    }
                }));
            }

            "function_call_output" => {
                // Flush pending tool_calls before processing output
                if !pending_tool_calls.is_empty() {
                    let mut msg = json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": pending_tool_calls
                    });
                    // Inject cached reasoning_content (DeepSeek requirement)
                    inject_reasoning_into_message(&mut msg);
                    messages.push(msg);
                    pending_tool_calls = Vec::new();
                }
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

    // Flush any remaining pending tool_calls
    if !pending_tool_calls.is_empty() {
        let mut msg = json!({
            "role": "assistant",
            "content": null,
            "tool_calls": pending_tool_calls
        });
        inject_reasoning_into_message(&mut msg);
        messages.push(msg);
    }
}

/// 为 assistant message 注入缓存的 reasoning_content（DeepSeek 硬性要求）
fn inject_reasoning_into_message(msg: &mut Value) {
    let tool_calls = match msg.get("tool_calls").and_then(|v| v.as_array()) {
        Some(tc) => tc,
        None => return,
    };
    // 检查任意 tool_call 是否有缓存
    for tc in tool_calls {
        if let Some(call_id) = tc.get("id").and_then(|v| v.as_str()) {
            if let Some(reasoning) = get_cached_reasoning(call_id) {
                msg["reasoning_content"] = json!(reasoning);
                return;
            }
        }
    }
    // 兜底：即使缓存为空，DeepSeek 也要求字段存在
    // 设置空字符串以确保 JSON 序列化时输出 "reasoning_content": ""
    msg["reasoning_content"] = json!("");
}

/// OpenAI Chat Completions 响应 → OpenAI Responses 响应
pub fn chat_completions_to_responses(body: Value) -> Value {
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let choices = body.get("choices").and_then(|v| v.as_array());
    let usage = body.get("usage");

    let mut output: Vec<Value> = Vec::new();
    let mut status = "completed";

    if let Some(choices) = choices {
        for choice in choices {
            let finish_reason = choice
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("stop");
            let message = choice.get("message");

            // Map finish_reason to status (last choice wins)
            status = match finish_reason {
                "stop" | "tool_calls" => "completed",
                "length" => "incomplete",
                "content_filter" => "incomplete",
                _ => "completed",
            };

            if let Some(message) = message {
                // Text content — may be null when only tool_calls are present
                let text = message
                    .get("content")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty());

                // Reasoning content (DeepSeek reasoning models)
                let reasoning = message
                    .get("reasoning_content")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty());

                // Tool calls → separate function_call output items
                let tool_calls = message.get("tool_calls").and_then(|v| v.as_array());

                // Message item: only for text content (not for tool_calls)
                // OpenAI Responses API uses separate function_call items, not tool_use within message
                if let Some(t) = text {
                    output.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": t}]
                    }));
                }

                // Reasoning content → reasoning item
                if let Some(reasoning_text) = reasoning {
                    output.push(json!({
                        "type": "reasoning",
                        "summary": [{"type": "text", "text": reasoning_text}],
                        "status": "completed"
                    }));
                }

                // Tool calls → function_call items (separate from message)
                if let Some(tc) = tool_calls {
                    let mut tc_ids: Vec<String> = Vec::new();
                    for call in tc {
                        let function = call.get("function");
                        let call_id = call.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        output.push(json!({
                            "type": "function_call",
                            "id": call_id,
                            "call_id": call_id,
                            "name": function.and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                            "arguments": function.and_then(|f| f.get("arguments").cloned()).unwrap_or(json!("{}")),
                            "status": "completed"
                        }));
                        if !call_id.is_empty() {
                            tc_ids.push(call_id.to_string());
                        }
                    }
                    // Cache reasoning_content for next round (DeepSeek requirement)
                    if let Some(r) = reasoning {
                        cache_reasoning_for_tool_calls(r, &tc_ids);
                    }
                }
            }
        }
    }

    let usage_json = if let Some(usage) = usage {
        let prompt_tokens = usage.get("prompt_tokens").cloned().unwrap_or(json!(0));
        let completion_tokens = usage.get("completion_tokens").cloned().unwrap_or(json!(0));
        let total_tokens = usage.get("total_tokens").cloned().unwrap_or(json!(0));
        // OpenAI Responses API 期望 input_tokens 包含 cache_read (Anthropic) 或 prompt_cache_hit (OpenAI)
        // 对于 DeepSeek，prompt_cache_hit_tokens 也可能存在
        let cache_hit = usage
            .get("prompt_cache_hit_tokens")
            .or_else(|| usage.get("prompt_tokens_details").and_then(|d| d.get("cached_tokens")))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let input_tokens = prompt_tokens.as_u64().unwrap_or(0) + cache_hit;
        json!({
            "input_tokens": input_tokens,
            "output_tokens": completion_tokens,
            "total_tokens": total_tokens
        })
    } else {
        json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        })
    };

    json!({
        "id": id,
        "object": "response",
        "model": model,
        "output": output,
        "status": status,
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

    let in_progress = format!(
        "event: response.in_progress\ndata: {}\n\n",
        json!({
            "type": "response.in_progress",
            "response": {
                "id": &response_id,
                "model": model_name,
                "status": "in_progress"
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

        // response.in_progress MUST follow response.created
        v.push(Ok(Bytes::from(in_progress)));

        // Append per-output-item events for tool calls, text, and reasoning
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
                        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or(&item_id);

                        // output_item.added for function_call
                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": output_index,
                                "item": {
                                    "id": call_id,
                                    "type": "function_call",
                                    "call_id": call_id,
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
                                "item_id": call_id,
                                "output_index": output_index,
                                "delta": arguments
                            })
                        ))));

                        // function_call_arguments.done
                        v.push(Ok(Bytes::from(format!(
                            "event: response.function_call_arguments.done\ndata: {}\n\n",
                            json!({
                                "type": "response.function_call_arguments.done",
                                "item_id": call_id,
                                "output_index": output_index,
                                "arguments": arguments
                            })
                        ))));

                        // output_item.done for function_call
                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.done\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.done",
                                "output_index": output_index,
                                "item": {
                                    "id": call_id,
                                    "type": "function_call",
                                    "call_id": call_id,
                                    "name": name,
                                    "arguments": arguments,
                                    "status": "completed"
                                }
                            })
                        ))));

                    }
                    "reasoning" => {
                        let summary_text = item
                            .get("summary")
                            .or_else(|| item.get("content"))
                            .and_then(|v| v.as_array())
                            .and_then(|arr| {
                                arr.first()
                                    .and_then(|s| s.get("text").and_then(|t| t.as_str()))
                            })
                            .unwrap_or("");

                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.added",
                                "output_index": output_index,
                                "item": {
                                    "id": &item_id,
                                    "type": "reasoning",
                                    "summary": [{"type": "text", "text": summary_text}],
                                    "status": "completed"
                                }
                            })
                        ))));

                        v.push(Ok(Bytes::from(format!(
                            "event: response.output_item.done\ndata: {}\n\n",
                            json!({
                                "type": "response.output_item.done",
                                "output_index": output_index,
                                "item": {
                                    "id": &item_id,
                                    "type": "reasoning",
                                    "summary": [{"type": "text", "text": summary_text}],
                                    "status": "completed"
                                }
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
        let in_progress_pos = text.find("event: response.in_progress").unwrap();
        let completed_pos = text.find("event: response.completed").unwrap();
        let first_output = text.find("event: response.output_item.added").unwrap();

        assert!(
            created_pos < in_progress_pos,
            "response.created ({created_pos}) must come before response.in_progress ({in_progress_pos})"
        );
        assert!(
            in_progress_pos < first_output,
            "response.in_progress ({in_progress_pos}) must come before first output_item.added ({first_output})\nFull SSE:\n{text}"
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

    #[test]
    fn test_chat_to_responses_with_tool_calls_null_content() {
        let input = json!({
            "id": "chatcmpl-456",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{\"path\":\"/tmp/x\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });

        let result = chat_completions_to_responses(input);
        let output = result["output"].as_array().unwrap();

        // Only function_call items (no message item when content is null)
        assert_eq!(output.len(), 1, "should have only function_call item when content is null");
        assert_eq!(output[0]["type"], "function_call");
        assert_eq!(output[0]["call_id"], "call_abc");
        assert_eq!(output[0]["name"], "read_file");
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn test_chat_to_responses_handles_length_finish() {
        let input = json!({
            "id": "chatcmpl-789",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "未完成的"},
                "finish_reason": "length"
            }]
        });

        let result = chat_completions_to_responses(input);
        assert_eq!(result["status"], "incomplete");
    }

    #[test]
    fn test_chat_to_responses_with_reasoning_content() {
        let input = json!({
            "id": "chatcmpl-reason",
            "model": "deepseek-reasoner",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "答案是42",
                    "reasoning_content": "让我思考一下..."
                },
                "finish_reason": "stop"
            }]
        });

        let result = chat_completions_to_responses(input);
        let output = result["output"].as_array().unwrap();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[1]["type"], "reasoning");
        assert_eq!(output[1]["summary"][0]["type"], "text");
        assert_eq!(output[1]["summary"][0]["text"], "让我思考一下...");
    }

    #[test]
    fn test_chat_to_responses_text_plus_tool_calls() {
        // Model says something before calling a tool
        let input = json!({
            "id": "chatcmpl-789",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Let me search for that.",
                    "tool_calls": [{
                        "id": "call_x1",
                        "type": "function",
                        "function": {"name": "search", "arguments": "{\"q\":\"test\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = chat_completions_to_responses(input);
        let output = result["output"].as_array().unwrap();

        // message item (text) + function_call item
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "Let me search for that.");
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(output[1]["name"], "search");
    }

    #[test]
    fn test_chat_to_responses_session_id_preserved() {
        let input = json!({
            "id": "chatcmpl-session-123",
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }]
        });

        let result = chat_completions_to_responses(input);
        assert_eq!(result["id"], "chatcmpl-session-123");
        assert_eq!(result["object"], "response");
    }
}
