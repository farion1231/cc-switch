//! OpenAI Responses API → Chat Completions 请求翻译
//!
//! Codex CLI 使用 Responses API 格式（input/output 结构），但 DeepSeek 等上游
//! 只支持 Chat Completions 格式（messages 结构）。本模块实现双向格式转换。
//!
//! 同时处理 DeepSeek 特殊兼容问题：
//! - 默认 thinking 模式 → 注入 thinking:disabled（防止 reasoning_content echo-back）
//! - 连续的 function_call → 分组为单条 assistant message + 多个 tool_calls

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// 批处理提示：追加到 system message 尾部，引导模型合并同类的独立工具调用
const BATCH_HINT: &str = "\
## 工具调用合并规则（遵循该规则可减少 token 消耗）

- **合并只读查询**：将多个独立的只读命令（如 cat、ls、head、wc、find、rg、git diff、git log）合并到一次 tool call 中，用 `&&` 或 `;` 连接
- **合并文件读取**：一次读取多个相关文件，而不是逐个读取
- **合并状态检查**：将 ps、lsof、git status 等状态检查合并到一次调用
- **避免冗余检查**：如果已确认某个事实，不要重复执行相同或相似的检查命令

注意：只合并*不相互依赖*的命令。如果命令 B 依赖命令 A 的输出，则必须分开调用。";

/// 压缩过长的指令（保留头部 60% + 尾部 30%，中间截断）
fn compress_instructions(instructions: &str) -> String {
    const MAX_LEN: usize = 8000;
    if instructions.len() <= MAX_LEN {
        return instructions.to_string();
    }
    let head_len = MAX_LEN * 60 / 100; // ~4800 chars
    let tail_len = MAX_LEN * 30 / 100; // ~2400 chars
    let mut result = String::with_capacity(MAX_LEN + 100);
    result.push_str(&instructions[..head_len.min(instructions.len())]);
    result.push_str("\n\n... [中间部分已截断以节省 token] ...\n\n");
    let tail_start = instructions.len().saturating_sub(tail_len);
    result.push_str(&instructions[tail_start..]);
    result
}

/// 压缩过长的工具输出（保留头部 50% + 尾部 40%，中间截断）
fn compress_tool_output(output: &str) -> String {
    const MAX_LEN: usize = 4000;
    if output.len() <= MAX_LEN {
        return output.to_string();
    }
    let head_len = MAX_LEN * 50 / 100; // ~2000 chars
    let tail_len = MAX_LEN * 40 / 100; // ~1600 chars
    let mut result = String::with_capacity(MAX_LEN + 100);
    result.push_str(&output[..head_len.min(output.len())]);
    result.push_str("\n\n... [中间输出已截断] ...\n\n");
    let tail_start = output.len().saturating_sub(tail_len);
    result.push_str(&output[tail_start..]);
    result
}

/// Responses API 请求 → Chat Completions 请求
///
/// `reasoning_effort` 控制推理模式：
/// - Some("disabled") → 注入 thinking: {type: "disabled"}
/// - Some("low"|"medium"|"high") → 注入 reasoning_effort
/// - None → 根据请求中的 reasoning 参数自动决定
///
/// `chat_compat_mode` 启用时：
/// - system message 追加批处理提示，减少 tool call 来回次数
/// - 工具输出超过阈值时自动压缩
pub fn responses_to_chat(
    body: &Value,
    reasoning_effort: Option<&str>,
    model_map: Option<&serde_json::Map<String, Value>>,
    chat_compat_mode: bool,
) -> Result<Value, ProxyError> {
    // Debug: log key fields only
    log::info!(
        "[Codex] >>> Request: model={}, stream={}, instructions_len={}, input_items={}, tools_count={}",
        body.get("model").and_then(|v| v.as_str()).unwrap_or("?"),
        body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
        body.get("instructions").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0),
        body.get("input").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        body.get("tools").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
    );

    let mut result = json!({
        "model": body.get("model"),
        "messages": [],
        "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
    });

    // input array → messages (先添加，保证 system message 在最前)
    if let Some(input_arr) = body.get("input").and_then(|v| v.as_array()) {
        let converted = convert_input_to_messages(input_arr, chat_compat_mode)?;
        if let Some(existing) = result["messages"].as_array_mut() {
            for msg in converted {
                existing.push(msg);
            }
        } else {
            result["messages"] = json!(converted);
        }
    } else if let Some(input_str) = body.get("input").and_then(|v| v.as_str()) {
        let msg = json!({"role": "user", "content": input_str});
        result["messages"]
            .as_array_mut()
            .unwrap()
            .push(msg);
    }

    // instructions → system message（插入到最前面）
    if let Some(instructions) = body.get("instructions").and_then(|v| v.as_str()) {
        if !instructions.is_empty() {
            let compressed = compress_instructions(instructions);
            let sys_content = if chat_compat_mode {
                // chat_compat 模式下追加批处理提示，减少 tool call 来回次数
                format!("{}\n\n{}", compressed, BATCH_HINT)
            } else {
                compressed
            };
            let sys_msg = json!({"role": "system", "content": sys_content});
            result["messages"]
                .as_array_mut()
                .unwrap()
                .insert(0, sys_msg);
        }
    }

    // max_output_tokens → max_tokens
    if let Some(v) = body.get("max_output_tokens") {
        result["max_tokens"] = v.clone();
    }

    // 透传参数
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(stop) = body.get("stop") {
        result["stop"] = stop.clone();
    }

    // 模型名映射：从 provider.settings_config.model_map 读取
    // 支持精确匹配和前缀匹配（如 gpt-5.4* → deepseek-v4-flash）
    if let Some(model) = body.get("model").and_then(|v| v.as_str()) {
        let mapped = if let Some(map) = model_map {
            // 先尝试精确匹配
            if let Some(exact) = map.get(model).and_then(|v| v.as_str()) {
                exact.to_string()
            } else {
                // 前缀匹配：如 model_map 的 key 以 * 结尾，匹配前缀
                let mut best = model.to_string();
                for (key, val) in map {
                    if let Some(prefix) = key.strip_suffix('*') {
                        if model.starts_with(prefix) {
                            if let Some(v) = val.as_str() {
                                best = v.to_string();
                                break;
                            }
                        }
                    }
                }
                best
            }
        } else {
            model.to_string()
        };
        if mapped != model {
            log::info!("[Codex] Model mapped: {} → {}", model, mapped);
        }
        result["model"] = json!(mapped);
    }

    // tools: function format → tool format
    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        let chat_tools: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let ttype = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if ttype == "web_search" || ttype == "web_search_preview" {
                    // DeepSeek 等上游不支持 web_search，直接跳过
                    return None;
                }
                if ttype != "function" {
                    return None;
                }
                Some(json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name"),
                        "description": t.get("description"),
                        "parameters": t.get("parameters"),
                    }
                }))
            })
            .collect();
        result["tools"] = json!(chat_tools);
    }

    // Reasoning 处理
    handle_reasoning(body, &mut result, reasoning_effort);

    // tool_choice
    if let Some(tc) = body.get("tool_choice") {
        result["tool_choice"] = tc.clone();
    }

    // metadata 透传
    if let Some(v) = body.get("metadata") {
        result["metadata"] = v.clone();
    }

    Ok(result)
}

/// 处理 reasoning 参数
///
/// 规则：
/// - 用户显式指定 reasoning_effort（如 "high"）→ 注入 reasoning_effort，启用思考
/// - 用户显式指定 "disabled" → 注入 thinking:disabled
/// - 未指定 → 默认 thinking:disabled（防止 DeepSeek 默认思考模式导致 reasoning_content echo-back）
fn handle_reasoning(body: &Value, result: &mut Value, reasoning_effort: Option<&str>) {
    // 只用 provider 显式配置的 reasoning_effort，不从 body 读取
    // Codex CLI 请求中自带 reasoning 参数，如果 fallback 读取会导致始终启用思考
    match reasoning_effort {
        Some(e) if matches!(e, "low" | "medium" | "high" | "xhigh") => {
            result["reasoning_effort"] = json!(e);
        }
        _ => {
            // 默认禁用思考，防止 reasoning_content echo-back 循环
            result["thinking"] = json!({"type": "disabled"});
        }
    }
    let _ = body;
}

/// 转换 input 数组到 messages 数组
///
/// 使用三缓冲机制（参考 CLIProxyAPI e193a007）：
/// - pendingFunctionCalls: 收集连续 function_call
/// - pendingToolOutputs: 收集对应的 function_call_output
/// - bufferedMessages: 收集中间交错的消息
///
/// 在遇到新消息或遍历结束时 flush：
///   1. emit assistant message with all tool_calls
///   2. emit tool messages (来自 pendingToolOutputs)
///   3. emit buffered messages（交错消息，如 developer approval）
fn convert_input_to_messages(input: &[Value], compress: bool) -> Result<Vec<Value>, ProxyError> {
    let mut messages: Vec<Value> = Vec::new();
    let mut pending_calls: Vec<&Value> = Vec::new();
    let mut pending_outputs: Vec<&Value> = Vec::new();
    let mut pending_reasoning: Option<String> = None;

    // 辅助函数：将 function_calls 合并到前一条 assistant message（如果存在），否则创建新的
    let emit_tool_calls = |messages: &mut Vec<Value>,
                            calls: &[&Value],
                            reasoning: &mut Option<String>| {
        let tool_calls: Vec<Value> = calls.iter().enumerate().map(|(_i, fc)| {
            json!({
                "id": fc.get("call_id"),
                "type": "function",
                "function": {
                    "name": fc.get("name"),
                    "arguments": fc.get("arguments"),
                }
            })
        }).collect();

        // 检查上一条消息是否是 assistant（没有 tool_calls），是则合并
        let merged = messages.last_mut().and_then(|last| {
            if last.get("role").and_then(|v| v.as_str()) == Some("assistant")
                && last.get("tool_calls").is_none()
            {
                last["tool_calls"] = json!(tool_calls);
                if last.get("content").is_none() || last["content"] == Value::Null {
                    last["content"] = json!(null);
                }
                if let Some(rt) = reasoning.take() {
                    if !rt.is_empty() && last.get("reasoning_content").is_none() {
                        last["reasoning_content"] = json!(rt);
                    }
                }
                Some(())
            } else {
                None
            }
        });

        if merged.is_none() {
            let mut msg = json!({
                "role": "assistant",
                "content": null,
                "tool_calls": tool_calls,
            });
            if let Some(rt) = reasoning.take() {
                if !rt.is_empty() {
                    msg["reasoning_content"] = json!(rt);
                }
            }
            messages.push(msg);
        }
    };

    // flush pending batch: emit tool_calls message + tool output messages
    let flush = |messages: &mut Vec<Value>,
                 calls: &mut Vec<&Value>,
                 outputs: &mut Vec<&Value>,
                 reasoning: &mut Option<String>| {
        if calls.is_empty() && outputs.is_empty() {
            return;
        }

        if !calls.is_empty() {
            emit_tool_calls(messages, calls, reasoning);
        }

        for output in outputs.iter() {
            let output_content = output.get("output").and_then(|v| v.as_str()).unwrap_or("");
            let content = if compress {
                compress_tool_output(output_content)
            } else {
                output_content.to_string()
            };
            let tool_msg = json!({
                "role": "tool",
                "tool_call_id": output.get("call_id"),
                "content": content,
            });
            messages.push(tool_msg);
        }

        calls.clear();
        outputs.clear();
    };

    for item in input {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("message");
        let effective_type = if item_type == "message" || (item_type.is_empty() && item.get("role").is_some()) {
            "message"
        } else {
            item_type
        };

        match effective_type {
            "message" | "" => {
                // 遇到新消息时，先 flush 之前累积的 function_calls / outputs
                // 防止不同 assistant 回合的 tool calls 被合并到一起
                if !pending_calls.is_empty() || !pending_outputs.is_empty() {
                    flush(&mut messages, &mut pending_calls, &mut pending_outputs, &mut pending_reasoning);
                }
                messages.push(convert_message_item(item, &mut pending_reasoning));
            }
            "function_call" => {
                pending_calls.push(item);
            }
            "function_call_output" => {
                // 先 flush pending calls（同一回合的 tool_calls 合并到前一条 assistant）
                if !pending_calls.is_empty() {
                    emit_tool_calls(&mut messages, &pending_calls, &mut pending_reasoning);
                    pending_calls.clear();
                }
                pending_outputs.push(item);
            }
            "reasoning" => {
                if let Some(summary) = item.get("summary").and_then(|v| v.as_array()) {
                    for s in summary {
                        if s.get("type").and_then(|v| v.as_str()) == Some("summary_text") {
                            if let Some(text) = s.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    pending_reasoning = Some(text.to_string());
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                if !pending_calls.is_empty() || !pending_outputs.is_empty() {
                    flush(&mut messages, &mut pending_calls, &mut pending_outputs, &mut pending_reasoning);
                }
                messages.push(convert_message_item(item, &mut pending_reasoning));
            }
        }
    }

    // 遍历结束，flush 剩余
    flush(
        &mut messages,
        &mut pending_calls,
        &mut pending_outputs,
        &mut pending_reasoning,
    );

    Ok(messages)
}

/// 转换单个 input 消息项为 Chat Completions message
fn convert_message_item(item: &Value, pending_reasoning: &mut Option<String>) -> Value {
    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    // developer → user（Chat Completions 无 developer role）
    let cc_role = if role == "developer" { "user" } else { role };
    let mut msg = json!({"role": cc_role});

    if let Some(content) = item.get("content") {
        if content.is_array() {
            let mut text_parts: Vec<String> = Vec::new();
            let mut reasoning_text: Option<String> = None;

            for part in content.as_array().unwrap() {
                let ptype = part.get("type").and_then(|v| v.as_str()).unwrap_or("input_text");
                match ptype {
                    "input_text" | "output_text" => {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    "reasoning_text" => {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            reasoning_text = Some(t.to_string());
                        }
                    }
                    "input_image" => {
                        // Codex 支持的图片，但 DeepSeek Chat Completions 不支持：
                        // 转为文本占位符，避免 DeepSeek 反序列化报错
                        let placeholder = if let Some(url) = part.get("image_url").and_then(|v| v.as_str()) {
                            format!("[Image: {url}]")
                        } else if let Some(file_id) = part.get("file_id").and_then(|v| v.as_str()) {
                            format!("[Image: file_id={file_id}]")
                        } else if let Some(detail) = part.get("detail").and_then(|v| v.as_str()) {
                            format!("[Image: detail={detail}]")
                        } else {
                            "[Image: format not supported by upstream]".to_string()
                        };
                        text_parts.push(placeholder);
                    }
                    "input_file" | "file" => {
                        // Codex 文件附件，DeepSeek 也不支持
                        let filename = part.get("filename").and_then(|v| v.as_str()).unwrap_or("unknown");
                        text_parts.push(format!("[File: {filename}]"));
                    }
                    _ => {}
                }
            }

            // reasoning_text → 注入 assistant.reasoning_content
            if role == "assistant" {
                if let Some(rt) = reasoning_text {
                    msg["reasoning_content"] = json!(rt);
                } else if let Some(rt) = pending_reasoning.take() {
                    msg["reasoning_content"] = json!(rt);
                }
            }

            msg["content"] = json!(text_parts.join("\n"));
        } else if content.is_string() {
            let text = content.as_str().unwrap_or("");
            msg["content"] = json!(text);
        }
    } else {
        // 没有 content → null
        msg["content"] = Value::Null;
    }

    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_message_conversion() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Hello"}]}
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello");
        // thinking:disabled should be injected
        assert_eq!(result["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_instructions_to_system() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "instructions": "You are a helpful assistant.",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a helpful assistant.");
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn test_function_call_grouping() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{\"city\":\"NYC\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "Sunny"},
                {"role": "user", "content": [{"type": "input_text", "text": "Thanks"}]}
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // assistant with tool_calls
        assert_eq!(messages[0]["role"], "assistant");
        assert!(messages[0]["tool_calls"].is_array());
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "get_weather");

        // tool message
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");

        // user message
        assert_eq!(messages[2]["role"], "user");
    }

    #[test]
    fn test_multiple_function_call_grouping() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{\"city\":\"NYC\"}"},
                {"type": "function_call", "call_id": "call_2", "name": "get_time", "arguments": "{\"tz\":\"EST\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "Sunny"},
                {"type": "function_call_output", "call_id": "call_2", "output": "10:00"},
                {"role": "user", "content": [{"type": "input_text", "text": "Done"}]}
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // assistant with 2 tool_calls
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(messages[0]["tool_calls"][1]["function"]["name"], "get_time");

        // 2 tool messages
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[2]["role"], "tool");

        // user message
        assert_eq!(messages[3]["role"], "user");
    }

    #[test]
    fn test_interleaved_messages_in_tool_group() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [
                {"type": "function_call", "call_id": "call_1", "name": "search", "arguments": "{\"q\":\"test\"}"},
                {"role": "user", "content": [{"type": "input_text", "text": "Please approve this search"}]},
                {"type": "function_call_output", "call_id": "call_1", "output": "Results"},
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();

        // assistant with tool_call
        assert_eq!(messages[0]["role"], "assistant");
        assert!(messages[0]["tool_calls"].is_array());

        // user message comes BEFORE tool results (chronological order preserved)
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Please approve this search");

        // tool result comes after the interleaved user message
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["content"], "Results");
    }

    #[test]
    fn test_reasoning_text_in_message() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [
                {
                    "role": "assistant",
                    "content": [
                        {"type": "reasoning_text", "text": "Let me think..."},
                        {"type": "output_text", "text": "The answer is 42."}
                    ]
                }
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["reasoning_content"], "Let me think...");
        assert_eq!(messages[0]["content"], "The answer is 42.");
    }

    #[test]
    fn test_reasoning_input_item() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Previous thinking..."}]},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Final answer"}]}
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["reasoning_content"], "Previous thinking...");
        assert_eq!(messages[0]["content"], "Final answer");
    }

    #[test]
    fn test_reasoning_effort_override() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "stream": false,
        });

        // reasoning_effort = "disabled" → thinking: disabled
        let result = responses_to_chat(&input, Some("disabled"), None, false).unwrap();
        assert_eq!(result["thinking"]["type"], "disabled");
        assert!(result.get("reasoning_effort").is_none());

        // reasoning_effort = "high" → reasoning_effort: high
        let result = responses_to_chat(&input, Some("high"), None, false).unwrap();
        assert_eq!(result["reasoning_effort"], "high");
        assert!(result.get("thinking").is_none());
    }

    #[test]
    fn test_reasoning_effort_not_from_request() {
        // Body 中的 reasoning 参数不被读取（避免 Codex CLI 自带 reasoning 导致始终启用思考）
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "reasoning": {"effort": "high"},
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        // 应该默认禁用，不从 body 读取
        assert_eq!(result["thinking"]["type"], "disabled");
        assert!(result.get("reasoning_effort").is_none());
    }

    #[test]
    fn test_tool_conversion() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Weather?"}]}],
            "tools": [
                {"type": "function", "name": "get_weather", "description": "Get weather", "parameters": {"type": "object"}}
            ],
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn test_max_output_tokens_mapping() {
        let input = json!({
            "model": "deepseek-v4-flash",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "max_output_tokens": 4096,
            "stream": false,
        });

        let result = responses_to_chat(&input, None, None, false).unwrap();
        assert_eq!(result["max_tokens"], 4096);
    }

    #[test]
    fn test_batch_hint_in_chat_compat_mode() {
        let input = json!({
            "model": "gpt-5.4",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "instructions": "You are a helpful assistant.",
            "stream": false,
        });

        // chat_compat_mode = true → 应包含 BATCH_HINT
        let result = responses_to_chat(&input, None, None, true).unwrap();
        let system_msg = &result["messages"][0];
        assert_eq!(system_msg["role"], "system");
        let content = system_msg["content"].as_str().unwrap();
        assert!(content.contains("工具调用合并规则"), "chat_compat 模式应追加批处理提示");
        assert!(content.contains("You are a helpful assistant."), "应保留原始 instructions");

        // chat_compat_mode = false → 不应包含 BATCH_HINT
        let result2 = responses_to_chat(&input, None, None, false).unwrap();
        let content2 = result2["messages"][0]["content"].as_str().unwrap();
        assert!(!content2.contains("工具调用合并规则"), "非 chat_compat 模式不应有批处理提示");
    }

    #[test]
    fn test_instructions_compression() {
        // 短指令不应被截断
        let short = "You are a helpful assistant.";
        assert_eq!(super::compress_instructions(short), short);

        // 长指令应被截断
        let long = "A".repeat(15000);
        let compressed = super::compress_instructions(&long);
        assert!(compressed.len() < 10000, "压缩后应小于 10000 字符");
        assert!(compressed.contains("[中间部分已截断以节省 token]"), "应包含截断标记");
        assert!(compressed.starts_with("A"), "应保留头部");
        assert!(compressed.ends_with("A"), "应保留尾部");
    }

    #[test]
    fn test_tool_output_compression() {
        // 短输出不应被截断
        let short = "short output";
        assert_eq!(super::compress_tool_output(short), short);

        // 长输出应被截断
        let long_output = "B".repeat(10000);
        let compressed = super::compress_tool_output(&long_output);
        assert!(compressed.len() < 6000, "压缩后应小于 6000 字符");
        assert!(compressed.contains("[中间输出已截断]"), "应包含截断标记");
        assert!(compressed.starts_with("B"), "应保留头部");
        assert!(compressed.ends_with("B"), "应保留尾部");
    }

    #[test]
    fn test_chat_compat_compresses_tool_outputs() {
        // 模拟大工具输出
        let large_output = "X".repeat(10000);
        let input = json!({
            "model": "gpt-5.4",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "Run cmd"}]},
                {"type": "function_call", "call_id": "call_1", "name": "exec", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_1", "output": &large_output},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Done"}]}
            ],
            "stream": false,
        });

        // chat_compat=true → tool output 应被压缩
        let result = responses_to_chat(&input, None, None, true).unwrap();
        let messages = result["messages"].as_array().unwrap();
        // 找到 tool message
        let tool_msg = messages.iter().find(|m| m["role"] == "tool").unwrap();
        let content = tool_msg["content"].as_str().unwrap();
        assert!(content.len() < 6000, "chat_compat 模式 tool 输出应被压缩");
        assert!(content.contains("截断"), "应包含截断标记");

        // chat_compat=false → tool output 不应被压缩
        let result2 = responses_to_chat(&input, None, None, false).unwrap();
        let tool_msg2 = result2["messages"].as_array().unwrap()
            .iter().find(|m| m["role"] == "tool").unwrap();
        let content2 = tool_msg2["content"].as_str().unwrap();
        assert_eq!(content2.len(), 10000, "非 chat_compat 模式不应压缩");
    }
}
