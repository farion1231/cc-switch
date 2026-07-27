//! OpenAI v1 compatible bridge for Codex OAuth upstreams.
//!
//! The public side of CC Switch accepts ordinary OpenAI Chat Completions
//! requests.  When a routed Codex provider uses managed ChatGPT/Codex OAuth,
//! the real upstream only accepts Codex Responses SSE, so this module performs
//! the narrow wire conversion needed to keep external agents OpenAI-compatible.

use crate::proxy::{
    error::ProxyError,
    json_canonical::{
        canonical_json_string, canonicalize_json_string_if_parseable, canonicalize_tool_arguments,
    },
    sse::{append_utf8_safe, strip_sse_field, take_sse_block},
};
use base64::Engine as _;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Value};

/// 将 OpenAI Chat Completions 请求转换为 ChatGPT Codex 后端接受的 Responses 请求。
///
/// 参数:
/// - `body`: 外部 agent 发送的 `/v1/chat/completions` JSON。
///
/// 返回:
/// - 可直接转发到 `/backend-api/codex/responses` 的 JSON。
///
/// 副作用:
/// - 无。该函数只做结构转换，不读取凭据，也不访问外部服务。
pub fn chat_completions_request_to_codex_responses(body: Value) -> Result<Value, ProxyError> {
    let messages = body
        .get("messages")
        .and_then(|value| value.as_array())
        .ok_or_else(|| ProxyError::TransformError("Chat request missing messages".to_string()))?;

    let mut instructions = Vec::new();
    let mut input = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("user");

        match role {
            "system" | "developer" => {
                if let Some(text) = chat_message_text(message.get("content")) {
                    if !text.trim().is_empty() {
                        instructions.push(text);
                    }
                }
            }
            "tool" => input.push(chat_tool_message_to_response_item(message)),
            "assistant" => append_chat_assistant_message(message, &mut input),
            _ => input.push(chat_user_message_to_response_message(role, message)),
        }
    }

    // ChatGPT Codex Responses 后端要求 instructions 非空；很多第三方 OpenAI SDK
    // 只发送 user message，没有 system/developer 消息，所以这里补一个最小默认值。
    let instructions = if instructions.is_empty() {
        "You are a helpful assistant.".to_string()
    } else {
        instructions.join("\n\n")
    };

    let mut result = json!({
        "model": body.get("model").cloned().unwrap_or_else(|| json!("gpt-5.4-mini")),
        "instructions": instructions,
        "input": input,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "tools": chat_tools_to_responses_tools(body.get("tools")),
        "parallel_tool_calls": body
            .get("parallel_tool_calls")
            .cloned()
            .unwrap_or_else(|| json!(false)),
        "stream": true
    });

    if let Some(tool_choice) = body.get("tool_choice") {
        result["tool_choice"] = chat_tool_choice_to_responses(tool_choice);
    }
    if let Some(service_tier) = body.get("service_tier") {
        result["service_tier"] = service_tier.clone();
    }
    if let Some(metadata) = body.get("metadata") {
        result["metadata"] = metadata.clone();
    }

    Ok(result)
}

/// 按 Codex transport 模式归一化 official OAuth Responses 请求。
///
/// 参数:
/// - `request_body`: Codex Desktop/CLI 发出的 Responses 请求体。
/// - `use_responses_lite`: 是否携带 Responses-Lite 内部协商头。
///   返回:
/// - 可转发到 ChatGPT Codex backend 的请求体。
///   副作用:
/// - 无。函数只转换传入 JSON 值。
///   边界:
/// - Lite 模式把 instructions/tools 编码为 input item，不能提升 developer message，
///   也不能补顶层 instructions/tools；标准模式继续执行原有兼容归一化。
pub(crate) fn normalize_codex_oauth_responses_request(
    request_body: Value,
    use_responses_lite: bool,
) -> Value {
    let request_body = normalize_codex_responses_passthrough_request_for_transport(
        request_body,
        use_responses_lite,
    );
    let mut body = match request_body {
        Value::Object(body) => body,
        other => return other,
    };

    normalize_codex_oauth_responses_input(&mut body);
    if !use_responses_lite {
        ensure_codex_oauth_responses_instructions(&mut body);
    }
    ensure_codex_oauth_reasoning_include(&mut body);

    body.insert("store".to_string(), Value::Bool(false));
    body.insert("stream".to_string(), Value::Bool(true));
    if !use_responses_lite {
        if !body.get("tools").is_some_and(Value::is_array) {
            body.insert("tools".to_string(), Value::Array(Vec::new()));
        }
        if body
            .get("parallel_tool_calls")
            .and_then(Value::as_bool)
            .is_none()
        {
            body.insert("parallel_tool_calls".to_string(), Value::Bool(false));
        }
    }

    body.remove("max_output_tokens");
    body.remove("temperature");
    body.remove("top_p");

    Value::Object(body)
}

/// 归一化 Codex Responses 透传请求中的内部控制消息。
///
/// 参数:
/// - `request_body`: Codex Desktop/CLI 发出的 Responses 请求体。
///   返回:
/// - 将 `input` 中的 system/developer message 提升到顶层 `instructions` 后的请求体。
///   副作用:
/// - 无。函数只转换传入 JSON 值。
///   边界:
/// - 用于 Codex 原生 Responses 透传路径；Chat 转换路径已有自己的 role 合并逻辑。
pub(crate) fn normalize_codex_responses_passthrough_request(request_body: Value) -> Value {
    normalize_codex_responses_passthrough_request_for_transport(request_body, false)
}

/// 按 transport 模式归一化 Codex Responses 透传请求。
///
/// 参数:
/// - `request_body`: Codex Desktop/CLI 发出的 Responses 请求体。
/// - `use_responses_lite`: 是否使用 input 编码 instructions/tools 的 Lite 模式。
///   返回:
/// - 标准模式提升控制消息；Lite 模式只处理所有 transport 都安全的 item 字段。
///   副作用:
/// - 无。函数只转换传入 JSON 值。
pub(crate) fn normalize_codex_responses_passthrough_request_for_transport(
    request_body: Value,
    use_responses_lite: bool,
) -> Value {
    let request_body = normalize_codex_responses_passthrough_items(request_body);
    if use_responses_lite {
        return request_body;
    }
    let mut body = match request_body {
        Value::Object(body) => body,
        other => return other,
    };

    normalize_codex_responses_control_messages(&mut body);

    Value::Object(body)
}

/// 归一化所有 Responses transport 都可安全处理的 input item 字段。
///
/// 参数:
/// - `request_body`: Codex Responses 请求体。
///   返回:
/// - function_call arguments 已规整、其余结构保持不变的请求体。
///   副作用:
/// - 无。函数只转换传入 JSON 值。
///   边界:
/// - 该层不能移动 developer/system message；Responses-Lite 依赖这些 input item
///   表示 instructions，只有标准 Responses 路径才允许后续提升到顶层。
fn normalize_codex_responses_passthrough_items(request_body: Value) -> Value {
    let mut body = match request_body {
        Value::Object(body) => body,
        other => return other,
    };

    normalize_codex_responses_function_call_arguments(&mut body);

    Value::Object(body)
}

/// 把 Responses-Lite 请求体降级为标准 Responses 请求体。
///
/// 参数:
/// - `request_body`: 已按 Lite 规则归一化的请求体。
///   返回:
/// - `additional_tools` 已恢复到顶层 tools、developer/system message 已提升到
///   instructions 的标准 Responses 请求体。
///   副作用:
/// - 无。函数只转换传入 JSON 值。
///   边界:
/// - 只用于上游明确拒绝 Lite header 或命中 Lite fallback 负缓存的重试路径；
///   同时适用于 official OAuth 和第三方 native Responses，不能套用 OAuth 专属字段清理。
pub(crate) fn normalize_codex_responses_lite_fallback_request(request_body: Value) -> Value {
    let mut body = match request_body {
        Value::Object(body) => body,
        other => return other,
    };

    restore_codex_responses_lite_tools(&mut body);
    normalize_codex_responses_passthrough_request(Value::Object(body))
}

/// 从 Lite input 中提取 additional_tools 并恢复到标准 Responses 顶层 tools。
///
/// 参数:
/// - `body`: 正在降级的 Lite 请求体。
///   返回:
/// - 无，直接修改 input 和 tools。
///   副作用:
/// - 无外部副作用。
fn restore_codex_responses_lite_tools(body: &mut Map<String, Value>) {
    let Some(Value::Array(items)) = body.remove("input") else {
        return;
    };

    let mut retained_items = Vec::with_capacity(items.len());
    let mut restored_tools = body
        .remove("tools")
        .and_then(|tools| tools.as_array().cloned())
        .unwrap_or_default();

    for item in items {
        let Value::Object(mut object) = item else {
            retained_items.push(item);
            continue;
        };
        if object.get("type").and_then(Value::as_str) == Some("additional_tools") {
            if let Some(Value::Array(tools)) = object.remove("tools") {
                restored_tools.extend(tools);
            }
        } else {
            retained_items.push(Value::Object(object));
        }
    }

    body.insert("input".to_string(), Value::Array(retained_items));
    body.insert("tools".to_string(), Value::Array(restored_tools));
}

/// 归一化 Responses `input` 字段，避免 ChatGPT Codex backend 拒绝字符串输入。
///
/// 参数:
/// - `body`: 正在构建的请求体对象。
///   返回:
/// - 无，直接修改 `body.input`。
///   副作用:
/// - 无外部副作用；只修改内存中的 JSON 对象。
fn normalize_codex_oauth_responses_input(body: &mut Map<String, Value>) {
    let input = match body.remove("input") {
        Some(Value::Array(items)) => Value::Array(items),
        Some(Value::String(text)) => codex_oauth_input_text_message(text),
        Some(Value::Object(item)) => Value::Array(vec![Value::Object(item)]),
        Some(Value::Null) | None => Value::Array(Vec::new()),
        Some(other) => codex_oauth_input_text_message(other.to_string()),
    };

    body.insert(
        "input".to_string(),
        normalize_codex_oauth_input_items(input),
    );
}

/// 将 Codex Responses input 中的 system/developer 控制消息提升到 instructions。
///
/// 参数:
/// - `body`: 正在构建的请求体对象。
///   返回:
/// - 无，直接修改 `body.input` 与 `body.instructions`。
///   副作用:
/// - 无外部副作用；只修改内存中的 JSON 对象。
fn normalize_codex_responses_control_messages(body: &mut Map<String, Value>) {
    let Some(input) = body.remove("input") else {
        return;
    };

    let (input, control_instructions) = lift_codex_responses_control_messages(input);
    append_codex_responses_control_instructions(body, control_instructions);
    body.insert("input".to_string(), input);
}

/// 规整 Codex Responses input 中历史 function_call 的 arguments。
///
/// 参数:
/// - `body`: 正在转发给原生 Responses 上游的请求体对象。
///   返回:
/// - 无，直接修改 `body.input[*].arguments`。
///   副作用:
/// - 无外部副作用；只修改内存中的 JSON 对象。
///   边界:
/// - 只处理 `type=function_call` 的 Responses 历史 item。MiniMax 等严格上游会重新解析
///   历史工具调用，空字符串或被截断的 JSON 片段会直接触发 400；这里把它们规整为合法
///   JSON 字符串，同时把非法原文保存在 `raw_arguments` 中，避免丢失排障信息。
fn normalize_codex_responses_function_call_arguments(body: &mut Map<String, Value>) {
    let Some(Value::Array(items)) = body.get_mut("input") else {
        return;
    };

    for item in items {
        normalize_codex_responses_function_call_item_arguments(item);
    }
}

/// 规整单条 Responses function_call item 的 arguments 字段。
///
/// 参数:
/// - `item`: 一条 Responses input item。
///   返回:
/// - 无，若该 item 是 function_call，则确保 `arguments` 是合法 JSON 字符串。
///   副作用:
/// - 无。
fn normalize_codex_responses_function_call_item_arguments(item: &mut Value) {
    let Value::Object(object) = item else {
        return;
    };
    if object.get("type").and_then(Value::as_str) != Some("function_call") {
        return;
    }

    let arguments = canonicalize_tool_arguments(object.get("arguments"));
    object.insert("arguments".to_string(), Value::String(arguments));
}

/// 提升 Codex Responses input 中的 system/developer 控制消息。
///
/// 参数:
/// - `input`: Responses `input` 字段。
///   返回:
/// - 删除控制消息后的 input 与被提升的 instruction 文本。
///   副作用:
/// - 无。该函数只转换传入 JSON 值。
fn lift_codex_responses_control_messages(input: Value) -> (Value, Vec<String>) {
    let Value::Array(items) = input else {
        return (input, Vec::new());
    };

    let mut control_instructions = Vec::new();
    let mut retained_items = Vec::with_capacity(items.len());

    for item in items {
        let Value::Object(object) = item else {
            retained_items.push(item);
            continue;
        };

        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if codex_responses_input_item_is_control_message(item_type, &object) {
            let text = codex_responses_input_item_text(&object);
            let text = text.trim();
            if !text.is_empty() {
                control_instructions.push(text.to_string());
            }
        } else {
            retained_items.push(Value::Object(object));
        }
    }

    (Value::Array(retained_items), control_instructions)
}

/// 清理 Codex OAuth backend 不接受的 input item 冗余字段。
///
/// 参数:
/// - `input`: 已经规整成 Responses `input` 数组或单条消息的 JSON。
///   返回:
/// - 清理后的 `input` JSON，保留 message content；reasoning item 保留 summary /
///   encrypted_content，并把 raw content 归并为 summary，删除 backend 不接受的 content。
///   副作用:
/// - 无。该函数只修改传入 JSON 的内存副本。
///   边界:
/// - 只服务 ChatGPT Codex OAuth 私有 backend；公开 OpenAI Responses API 兼容路径不调用。
fn normalize_codex_oauth_input_items(input: Value) -> Value {
    let Value::Array(items) = input else {
        return input;
    };

    let mut normalized_items = Vec::with_capacity(items.len());

    for item in items {
        normalized_items.push(normalize_codex_oauth_input_item(item));
    }

    Value::Array(normalized_items)
}

/// 清理单个 Codex Responses input item 的私有 backend 不兼容字段。
///
/// 参数:
/// - `item`: 一条 Responses input item。
///   返回:
/// - message / agent_message 及未知新类型保留 content；reasoning item 会把 raw content
///   归并进 summary；只有已确认不接受 content 的调用/输出类型才删除该字段。
///   副作用:
/// - 无。
fn normalize_codex_oauth_input_item(item: Value) -> Value {
    let Value::Object(mut object) = item else {
        return item;
    };

    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if item_type == "reasoning" {
        normalize_codex_oauth_reasoning_item(&mut object);
    } else if item_type == "agent_message" {
        normalize_codex_oauth_agent_message(&mut object);
    } else if codex_oauth_input_item_forbids_content(item_type) {
        object.remove("content");
    }

    Value::Object(object)
}

/// Codex Multi-Agent V2 can persist a third-party agent's plaintext reply in an
/// `encrypted_content` block. The official backend later treats that block as an
/// opaque ciphertext and permanently rejects the conversation. Preserve plausible
/// ciphertexts, but recover clearly plaintext blocks as ordinary input text.
fn normalize_codex_oauth_agent_message(object: &mut Map<String, Value>) {
    let Some(Value::Array(content)) = object.get_mut("content") else {
        return;
    };

    for part in content {
        let Value::Object(part) = part else {
            continue;
        };
        if part.get("type").and_then(Value::as_str) != Some("encrypted_content") {
            continue;
        }
        let Some(value) = part
            .get("encrypted_content")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        if looks_like_codex_encrypted_content(&value) {
            continue;
        }

        part.clear();
        part.insert("type".to_string(), Value::String("input_text".to_string()));
        part.insert("text".to_string(), Value::String(value));
    }
}

fn looks_like_codex_encrypted_content(value: &str) -> bool {
    if value.len() < 64 || !value.is_ascii() {
        return false;
    }
    [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ]
    .into_iter()
    .any(|engine| {
        engine
            .decode(value)
            .is_ok_and(|decoded| decoded.len() >= 32)
    })
}

/// 判断 input item 是否是 Codex 内部控制消息。
///
/// 参数:
/// - `item_type`: Responses item 的 `type`。
/// - `object`: item 的 JSON object。
///   返回:
/// - `true` 表示该消息应进入顶层 instructions，而不是作为对话 input 发给 backend。
///   副作用:
/// - 无。
fn codex_responses_input_item_is_control_message(
    item_type: &str,
    object: &Map<String, Value>,
) -> bool {
    matches!(item_type, "" | "message")
        && matches!(
            object.get("role").and_then(Value::as_str),
            Some("system" | "developer")
        )
}

/// 提取 Codex input item 中可合并进 instructions 的文本。
///
/// 参数:
/// - `object`: system/developer message item。
///   返回:
/// - 拼接后的纯文本；无法提取时返回空字符串。
///   副作用:
/// - 无。
fn codex_responses_input_item_text(object: &Map<String, Value>) -> String {
    chat_message_text(object.get("content")).unwrap_or_else(|| {
        object
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    })
}

/// 判断 Codex OAuth backend 的 input item 是否明确禁止携带 `content`。
///
/// 参数:
/// - `item_type`: Responses input item 的 `type` 字段。
///   返回:
/// - `true` 表示该调用/输出类型使用自己的结构化字段，冗余 content 应移除。
///   副作用:
/// - 无。
///   边界:
/// - 这里使用“禁止列表”而不是“允许列表”。Codex 会持续增加 Responses item 类型；
///   例如 Multi-Agent V2 的 `agent_message.content` 是必填字段。未知类型必须保守保留，
///   否则代理会把未来协议字段静默删掉并制造缺失参数错误。
fn codex_oauth_input_item_forbids_content(item_type: &str) -> bool {
    matches!(
        item_type,
        "additional_tools"
            | "item_reference"
            | "function_call"
            | "function_call_output"
            | "custom_tool_call"
            | "custom_tool_call_output"
            | "mcp_tool_call"
            | "mcp_tool_call_output"
            | "tool_search_call"
            | "tool_search_output"
            | "local_shell_call"
            | "computer_call"
            | "computer_call_output"
            | "file_search_call"
            | "code_interpreter_call"
            | "web_search_call"
            | "image_generation_call"
            | "compaction"
            | "compaction_trigger"
            | "context_compaction"
    )
}

/// 规整 official OAuth reasoning input item 的 raw content。
///
/// 参数:
/// - `object`: `type=reasoning` 的 Responses input item。
///   返回:
/// - 无，直接修改 item。
///   副作用:
/// - 无外部副作用；只修改内存中的 JSON 对象。
///   边界:
/// - ChatGPT Codex backend 通过 `summary` / `encrypted_content` 回放 reasoning；
///   `content` 字段在该私有 backend 的 input schema 中不可携带。
fn normalize_codex_oauth_reasoning_item(object: &mut Map<String, Value>) {
    let Some(content) = object.remove("content") else {
        return;
    };

    if !codex_oauth_reasoning_summary_has_text(object.get("summary")) {
        if let Some(summary) = codex_oauth_reasoning_content_to_summary(&content) {
            object.insert("summary".to_string(), summary);
        }
    }
}

/// 判断 reasoning summary 是否已经包含可回放的文本。
///
/// 参数:
/// - `summary`: reasoning item 的 `summary` 字段。
///   返回:
/// - `true` 表示已有非空 summary 文本，不需要从 raw content 补齐。
///   副作用:
/// - 无。
fn codex_oauth_reasoning_summary_has_text(summary: Option<&Value>) -> bool {
    summary
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        })
}

/// 把 reasoning raw content 转成 official backend 接受的 summary 片段。
///
/// 参数:
/// - `content`: 被移除的 raw reasoning `content` 字段。
///   返回:
/// - 有可读文本时返回 `summary_text` 数组；否则返回 `None`。
///   副作用:
/// - 无。
fn codex_oauth_reasoning_content_to_summary(content: &Value) -> Option<Value> {
    let mut texts = Vec::new();
    collect_codex_oauth_reasoning_content_text(content, &mut texts);
    if texts.is_empty() {
        return None;
    }

    Some(Value::Array(
        texts
            .into_iter()
            .map(|text| {
                let mut part = Map::new();
                part.insert(
                    "type".to_string(),
                    Value::String("summary_text".to_string()),
                );
                part.insert("text".to_string(), Value::String(text));
                Value::Object(part)
            })
            .collect(),
    ))
}

/// 收集 reasoning raw content 中的文本，兼容字符串、数组和对象几种历史形态。
///
/// 参数:
/// - `value`: raw content 的任意 JSON 值。
/// - `texts`: 输出文本片段。
///   返回:
/// - 无。
///   副作用:
/// - 向 `texts` 追加文本片段。
fn collect_codex_oauth_reasoning_content_text(value: &Value, texts: &mut Vec<String>) {
    match value {
        Value::String(text) => push_non_empty_reasoning_text(texts, text),
        Value::Array(parts) => {
            for part in parts {
                collect_codex_oauth_reasoning_content_text(part, texts);
            }
        }
        Value::Object(part) => {
            for key in ["text", "content", "reasoning_content", "reasoning"] {
                if let Some(text) = part.get(key).and_then(Value::as_str) {
                    push_non_empty_reasoning_text(texts, text);
                    return;
                }
            }
        }
        _ => {}
    }
}

/// 追加非空 reasoning 文本，并避免空白片段污染 summary。
///
/// 参数:
/// - `texts`: 输出文本片段。
/// - `text`: 待追加文本。
///   返回:
/// - 无。
///   副作用:
/// - 向 `texts` 追加裁剪后的非空文本。
fn push_non_empty_reasoning_text(texts: &mut Vec<String>, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        texts.push(text.to_string());
    }
}

/// 把从 input 中提升出来的 Codex 控制消息追加到顶层 instructions。
///
/// 参数:
/// - `body`: 正在构建的请求体对象。
/// - `control_instructions`: 从 system/developer input item 提取的文本片段。
///   返回:
/// - 无，直接修改 `body.instructions`。
///   副作用:
/// - 无外部副作用。
fn append_codex_responses_control_instructions(
    body: &mut Map<String, Value>,
    control_instructions: Vec<String>,
) {
    if control_instructions.is_empty() {
        return;
    }

    let promoted = control_instructions.join("\n\n");
    match body.get("instructions").and_then(Value::as_str) {
        Some(existing) if !existing.trim().is_empty() => {
            body.insert(
                "instructions".to_string(),
                Value::String(format!("{existing}\n\n{promoted}")),
            );
        }
        _ => {
            body.insert("instructions".to_string(), Value::String(promoted));
        }
    }
}

/// 构造 Codex Responses 兼容的单条 user text message。
///
/// 参数:
/// - `text`: 用户输入文本。
///   返回:
/// - `input` 数组值，内部包含一条 `type=message` 的 user 消息。
///   副作用:
/// - 无。
fn codex_oauth_input_text_message(text: String) -> Value {
    let mut content_part = Map::new();
    content_part.insert("type".to_string(), Value::String("input_text".to_string()));
    content_part.insert("text".to_string(), Value::String(text));

    let mut message = Map::new();
    message.insert("type".to_string(), Value::String("message".to_string()));
    message.insert("role".to_string(), Value::String("user".to_string()));
    message.insert(
        "content".to_string(),
        Value::Array(vec![Value::Object(content_part)]),
    );

    Value::Array(vec![Value::Object(message)])
}

/// 补齐 ChatGPT Codex backend 要求的 `instructions` 字段。
///
/// 参数:
/// - `body`: 正在构建的请求体对象。
///   返回:
/// - 无，缺失或空白时写入最小默认 system instructions。
///   副作用:
/// - 无外部副作用；只修改内存中的 JSON 对象。
fn ensure_codex_oauth_responses_instructions(body: &mut Map<String, Value>) {
    let has_non_empty_instructions = body
        .get("instructions")
        .and_then(Value::as_str)
        .is_some_and(|instructions| !instructions.trim().is_empty());

    if !has_non_empty_instructions {
        body.insert(
            "instructions".to_string(),
            Value::String("You are a helpful assistant.".to_string()),
        );
    }
}

/// 确保 reasoning 加密内容被请求回来，避免多轮 Codex reasoning 状态丢失。
///
/// 参数:
/// - `body`: 正在构建的请求体对象。
///   返回:
/// - 无，直接修改或创建 `include` 数组。
///   副作用:
/// - 无外部副作用；只修改内存中的 JSON 对象。
fn ensure_codex_oauth_reasoning_include(body: &mut Map<String, Value>) {
    const REASONING_MARKER: &str = "reasoning.encrypted_content";

    match body.get_mut("include") {
        Some(Value::Array(includes)) => {
            if !includes
                .iter()
                .any(|value| value.as_str() == Some(REASONING_MARKER))
            {
                includes.push(Value::String(REASONING_MARKER.to_string()));
            }
        }
        _ => {
            body.insert(
                "include".to_string(),
                Value::Array(vec![Value::String(REASONING_MARKER.to_string())]),
            );
        }
    }
}

/// 将非流式 Responses 响应转换为标准 OpenAI Chat Completions 响应。
///
/// 参数:
/// - `response`: 已聚合出的 Responses JSON。
/// - `fallback_model`: 上游响应缺少 model 时使用的请求模型名。
///
/// 返回:
/// - OpenAI SDK 可直接解析的 `chat.completion` JSON。
pub fn codex_responses_to_chat_completion(
    response: Value,
    fallback_model: &str,
) -> Result<Value, ProxyError> {
    if response.get("error").is_some_and(|error| !error.is_null()) {
        return Ok(responses_error_to_chat_error(Some(&response)));
    }

    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_parts = Vec::new();

    if let Some(output) = response.get("output").and_then(|value| value.as_array()) {
        for item in output {
            collect_response_output_item(
                item,
                &mut content_parts,
                &mut tool_calls,
                &mut reasoning_parts,
            );
        }
    }

    let mut message = json!({
        "role": "assistant",
        "content": content_parts.join("")
    });

    if content_parts.is_empty() && !tool_calls.is_empty() {
        message["content"] = Value::Null;
    }
    let has_tool_calls = !tool_calls.is_empty();
    if has_tool_calls {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    if !reasoning_parts.is_empty() {
        message["reasoning_content"] = json!(reasoning_parts.join("\n\n"));
    }

    let finish_reason = finish_reason_from_response(&response, has_tool_calls);
    let model = response
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_model);

    Ok(json!({
        "id": chat_id_from_response_id(response.get("id").and_then(|value| value.as_str())),
        "object": "chat.completion",
        "created": response
            .get("created_at")
            .or_else(|| response.get("created"))
            .and_then(|value| value.as_u64())
            .unwrap_or_else(current_unix_timestamp),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": responses_usage_to_chat_usage(response.get("usage"))
    }))
}

/// 将 Responses API 错误体规整为 OpenAI-compatible 错误体。
///
/// 参数:
/// - `body`: 上游错误 JSON；为空时生成代理层兜底错误。
///
/// 返回:
/// - `{"error": {...}}` 形状的 JSON。
pub fn responses_error_to_chat_error(body: Option<&Value>) -> Value {
    let Some(body) = body else {
        return json!({
            "error": {
                "message": "Upstream returned an empty error response",
                "type": "upstream_error",
                "code": null,
                "param": null
            }
        });
    };

    if let Some(error) = body.get("error") {
        return json!({ "error": normalize_error_object(error) });
    }
    if let Some(error) = body.pointer("/response/error") {
        return json!({ "error": normalize_error_object(error) });
    }

    json!({
        "error": {
            "message": body
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("Upstream Codex OAuth request failed"),
            "type": body
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("upstream_error"),
            "code": body.get("code").cloned().unwrap_or(Value::Null),
            "param": body.get("param").cloned().unwrap_or(Value::Null)
        }
    })
}

/// 创建把 Responses SSE 实时转换成 OpenAI Chat Completions SSE 的流。
///
/// 参数:
/// - `stream`: 上游 Codex Responses SSE 字节流。
/// - `fallback_model`: 上游早期事件缺少 model 时使用的请求模型。
///
/// 返回:
/// - 对外兼容 OpenAI SDK streaming parser 的 SSE 字节流。
pub fn create_chat_sse_stream_from_codex_responses<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    fallback_model: String,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
    async_stream::stream! {
        let mut state = ResponsesToChatSseState::new(fallback_model);
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut failed = false;

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, bytes.as_ref());
                    while let Some(block) = take_sse_block(&mut buffer) {
                        match state.handle_sse_block(&block) {
                            Ok(events) => {
                                for event in events {
                                    yield Ok(event);
                                }
                            }
                            Err(err) => {
                                failed = true;
                                yield Ok(chat_sse_data(json!({
                                    "error": {
                                        "message": err.to_string(),
                                        "type": "stream_error",
                                        "code": null,
                                        "param": null
                                    }
                                })));
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    failed = true;
                    yield Ok(chat_sse_data(json!({
                        "error": {
                            "message": err.to_string(),
                            "type": "stream_error",
                            "code": null,
                            "param": null
                        }
                    })));
                    break;
                }
            }
        }

        if !failed && !state.done {
            for event in state.finish("stop") {
                yield Ok(event);
            }
        }
        yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));
    }
}

#[derive(Debug)]
struct ResponsesToChatSseState {
    id: String,
    model: String,
    created: u64,
    role_sent: bool,
    done: bool,
    next_tool_index: usize,
    emitted_tool_call: bool,
    usage: Option<Value>,
}

impl ResponsesToChatSseState {
    /// 创建 Responses SSE 转 Chat SSE 的状态机。
    ///
    /// 参数:
    /// - `fallback_model`: response.created 到达前用于 chunk 的模型名。
    fn new(fallback_model: String) -> Self {
        Self {
            id: "chatcmpl_ccswitch".to_string(),
            model: fallback_model,
            created: current_unix_timestamp(),
            role_sent: false,
            done: false,
            next_tool_index: 0,
            emitted_tool_call: false,
            usage: None,
        }
    }

    /// 处理一个完整 SSE block，并返回需要下发给 OpenAI 客户端的 chunks。
    ///
    /// 参数:
    /// - `block`: 不含空行分隔符的 SSE 文本块。
    fn handle_sse_block(&mut self, block: &str) -> Result<Vec<Bytes>, ProxyError> {
        let (event_name, data) = parse_sse_block(block)?;
        match event_name.as_deref() {
            Some("response.created") => {
                if let Some(response) = data.get("response") {
                    self.apply_response_metadata(response);
                } else {
                    self.apply_response_metadata(&data);
                }
                Ok(self.ensure_role_chunk())
            }
            Some("response.output_text.delta") => {
                let delta = data
                    .get("delta")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let mut events = self.ensure_role_chunk();
                if !delta.is_empty() {
                    events.push(self.delta_chunk(json!({ "content": delta }), None, None));
                }
                Ok(events)
            }
            Some("response.output_item.done") => {
                let Some(item) = data.get("item") else {
                    return Ok(Vec::new());
                };
                let mut events = self.ensure_role_chunk();
                if let Some(tool_call) = self.tool_call_delta_from_item(item) {
                    events.push(self.delta_chunk(json!({ "tool_calls": [tool_call] }), None, None));
                }
                Ok(events)
            }
            Some("response.completed") => {
                if let Some(response) = data.get("response") {
                    self.apply_response_metadata(response);
                    self.usage = Some(responses_usage_to_chat_usage(response.get("usage")));
                }
                let finish_reason = if self.emitted_tool_call {
                    "tool_calls"
                } else {
                    "stop"
                };
                Ok(self.finish(finish_reason))
            }
            Some("response.failed") => {
                self.done = true;
                Ok(vec![chat_sse_data(responses_error_to_chat_error(Some(
                    &data,
                )))])
            }
            _ => Ok(Vec::new()),
        }
    }

    /// 从 response 元数据事件里补齐 chunk 级别的 id/model/created。
    fn apply_response_metadata(&mut self, response: &Value) {
        if let Some(id) = response.get("id").and_then(|value| value.as_str()) {
            self.id = chat_id_from_response_id(Some(id));
        }
        if let Some(model) = response.get("model").and_then(|value| value.as_str()) {
            if !model.is_empty() {
                self.model = model.to_string();
            }
        }
        if let Some(created) = response
            .get("created_at")
            .or_else(|| response.get("created"))
            .and_then(|value| value.as_u64())
        {
            self.created = created;
        }
    }

    /// 确保流式响应先发送一次 assistant role chunk，兼容 OpenAI SDK。
    fn ensure_role_chunk(&mut self) -> Vec<Bytes> {
        if self.role_sent {
            return Vec::new();
        }
        self.role_sent = true;
        vec![self.delta_chunk(json!({ "role": "assistant" }), None, None)]
    }

    /// 从 Responses output item 生成 OpenAI Chat tool_call delta。
    fn tool_call_delta_from_item(&mut self, item: &Value) -> Option<Value> {
        let item_type = item.get("type").and_then(|value| value.as_str())?;
        if !matches!(
            item_type,
            "function_call" | "custom_tool_call" | "tool_search_call"
        ) {
            return None;
        }

        let index = self.next_tool_index;
        self.next_tool_index += 1;
        self.emitted_tool_call = true;

        let id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch");
        let name = item
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("tool_call");
        let arguments = item
            .get("arguments")
            .or_else(|| item.get("input"))
            .map(response_tool_arguments_to_chat)
            .unwrap_or_else(|| "{}".to_string());

        Some(json!({
            "index": index,
            "id": id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments
            }
        }))
    }

    /// 生成一个标准 Chat Completions stream chunk。
    fn delta_chunk(
        &self,
        delta: Value,
        finish_reason: Option<&str>,
        usage: Option<Value>,
    ) -> Bytes {
        chat_sse_data(json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }],
            "usage": usage.unwrap_or(Value::Null)
        }))
    }

    /// 结束当前流，并在有 usage 时追加 OpenAI SDK 可识别的 usage chunk。
    fn finish(&mut self, finish_reason: &str) -> Vec<Bytes> {
        if self.done {
            return Vec::new();
        }
        self.done = true;
        let mut events = self.ensure_role_chunk();
        events.push(self.delta_chunk(json!({}), Some(finish_reason), None));
        if let Some(usage) = self.usage.take() {
            events.push(chat_sse_data(json!({
                "id": self.id,
                "object": "chat.completion.chunk",
                "created": self.created,
                "model": self.model,
                "choices": [],
                "usage": usage
            })));
        }
        events
    }
}

/// 将 Chat message content 提取为纯文本，供 system/developer 合并 instructions 使用。
fn chat_message_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(|value| value.as_str())
                        .or_else(|| part.as_str())
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        other => other.as_str().map(ToString::to_string),
    }
}

/// 将 user/developer 以外的普通 Chat message 转换为 Responses message item。
fn chat_user_message_to_response_message(role: &str, message: &Value) -> Value {
    json!({
        "type": "message",
        "role": match role {
            "assistant" => "assistant",
            _ => "user",
        },
        "content": chat_content_to_responses_content(message.get("content"), "input_text")
    })
}

/// 将 assistant 历史消息拆成 Responses message 和 function_call items。
fn append_chat_assistant_message(message: &Value, input: &mut Vec<Value>) {
    let content = chat_content_to_responses_content(message.get("content"), "output_text");
    if !content.as_array().is_some_and(|parts| parts.is_empty()) {
        input.push(json!({
            "type": "message",
            "role": "assistant",
            "content": content
        }));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|value| value.as_array()) {
        for tool_call in tool_calls {
            input.push(chat_tool_call_to_response_item(tool_call));
        }
    }
}

/// 将 Chat tool result message 转换为 Responses function_call_output item。
fn chat_tool_message_to_response_item(message: &Value) -> Value {
    json!({
        "type": "function_call_output",
        "call_id": message
            .get("tool_call_id")
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "output": message
            .get("content")
            .and_then(|value| value.as_str())
            .map(canonicalize_json_string_if_parseable)
            .unwrap_or_default()
    })
}

/// 将 Chat tool_call 转换为 Responses function_call item。
fn chat_tool_call_to_response_item(tool_call: &Value) -> Value {
    let function = tool_call.get("function").unwrap_or(&Value::Null);
    json!({
        "type": "function_call",
        "id": tool_call
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "call_id": tool_call
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "name": function
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("tool_call"),
        "arguments": function
            .get("arguments")
            .and_then(|value| value.as_str())
            .unwrap_or("{}")
    })
}

/// 将 Chat content parts 转换为 Responses content parts。
fn chat_content_to_responses_content(content: Option<&Value>, text_type: &str) -> Value {
    match content {
        Some(Value::String(text)) => json!([{ "type": text_type, "text": text }]),
        Some(Value::Array(parts)) => Value::Array(
            parts
                .iter()
                .filter_map(|part| chat_content_part_to_response_part(part, text_type))
                .collect(),
        ),
        Some(Value::Null) | None => json!([]),
        Some(other) => json!([{ "type": text_type, "text": other.to_string() }]),
    }
}

/// 将单个 Chat content part 转换为 Responses content part。
fn chat_content_part_to_response_part(part: &Value, text_type: &str) -> Option<Value> {
    match part.get("type").and_then(|value| value.as_str()) {
        Some("text" | "input_text" | "output_text") => Some(json!({
            "type": text_type,
            "text": part.get("text").and_then(|value| value.as_str()).unwrap_or("")
        })),
        Some("image_url") => Some(json!({
            "type": "input_image",
            "image_url": part
                .pointer("/image_url/url")
                .or_else(|| part.get("image_url"))
                .cloned()
                .unwrap_or(Value::Null)
        })),
        Some("input_audio") => Some(json!({
            "type": "input_audio",
            "input_audio": part.get("input_audio").cloned().unwrap_or(Value::Null)
        })),
        Some("file") => {
            let file = part.get("file")?;
            let mut mapped = Map::new();
            for key in ["file_id", "file_data", "filename"] {
                if let Some(value) = file.get(key) {
                    mapped.insert(key.to_string(), value.clone());
                }
            }
            Some(Value::Object({
                let mut object = Map::new();
                object.insert("type".to_string(), json!("input_file"));
                for (key, value) in mapped {
                    object.insert(key, value);
                }
                object
            }))
        }
        _ => None,
    }
}

/// 将 OpenAI Chat tools 转换为 Responses tools。
fn chat_tools_to_responses_tools(tools: Option<&Value>) -> Value {
    let Some(tools) = tools.and_then(|value| value.as_array()) else {
        return json!([]);
    };

    Value::Array(
        tools
            .iter()
            .filter_map(|tool| {
                if tool.get("type").and_then(|value| value.as_str()) != Some("function") {
                    return None;
                }
                let function = tool.get("function")?;
                let mut mapped = Map::new();
                mapped.insert("type".to_string(), json!("function"));
                mapped.insert(
                    "name".to_string(),
                    function
                        .get("name")
                        .cloned()
                        .unwrap_or_else(|| json!("tool_call")),
                );
                if let Some(description) = function.get("description") {
                    mapped.insert("description".to_string(), description.clone());
                }
                mapped.insert(
                    "parameters".to_string(),
                    function
                        .get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                );
                if let Some(strict) = function.get("strict").or_else(|| tool.get("strict")) {
                    mapped.insert("strict".to_string(), strict.clone());
                }
                Some(Value::Object(mapped))
            })
            .collect(),
    )
}

/// 将 Chat tool_choice 转换为 Responses tool_choice。
fn chat_tool_choice_to_responses(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(value) => match value.as_str() {
            "required" => json!("required"),
            "none" => json!("none"),
            _ => json!("auto"),
        },
        Value::Object(object) => {
            if object.get("type").and_then(|value| value.as_str()) == Some("function") {
                json!({
                    "type": "function",
                    "name": object
                        .get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                })
            } else {
                tool_choice.clone()
            }
        }
        _ => json!("auto"),
    }
}

/// 收集 Responses output item 中的文本、推理和工具调用。
fn collect_response_output_item(
    item: &Value,
    content_parts: &mut Vec<String>,
    tool_calls: &mut Vec<Value>,
    reasoning_parts: &mut Vec<String>,
) {
    match item.get("type").and_then(|value| value.as_str()) {
        Some("message") => collect_response_message_content(item, content_parts, reasoning_parts),
        Some("reasoning") => {
            if let Some(text) = response_reasoning_text(item) {
                reasoning_parts.push(text);
            }
        }
        Some("function_call" | "custom_tool_call" | "tool_search_call") => {
            tool_calls.push(response_item_to_chat_tool_call(item, tool_calls.len()));
            if let Some(text) = response_reasoning_text(item) {
                reasoning_parts.push(text);
            }
        }
        _ => {}
    }
}

/// 收集 Responses message item 内的 content 文本。
fn collect_response_message_content(
    item: &Value,
    content_parts: &mut Vec<String>,
    reasoning_parts: &mut Vec<String>,
) {
    if let Some(text) = response_reasoning_text(item) {
        reasoning_parts.push(text);
    }

    let Some(content) = item.get("content") else {
        return;
    };
    match content {
        Value::String(text) => content_parts.push(text.clone()),
        Value::Array(parts) => {
            for part in parts {
                if let Some(text) = part
                    .get("text")
                    .or_else(|| part.get("refusal"))
                    .and_then(|value| value.as_str())
                {
                    content_parts.push(text.to_string());
                }
            }
        }
        _ => {}
    }
}

/// 提取 Responses item 上可能存在的 reasoning 文本。
fn response_reasoning_text(item: &Value) -> Option<String> {
    item.get("summary")
        .and_then(|value| value.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            item.get("reasoning_content")
                .or_else(|| item.get("reasoning"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
}

/// 将 Responses function_call/custom/tool_search item 转成 Chat tool_call。
fn response_item_to_chat_tool_call(item: &Value, index: usize) -> Value {
    json!({
        "index": index,
        "id": item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("call_ccswitch"),
        "type": "function",
        "function": {
            "name": item
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("tool_call"),
            "arguments": item
                .get("arguments")
                .or_else(|| item.get("input"))
                .map(response_tool_arguments_to_chat)
                .unwrap_or_else(|| "{}".to_string())
        }
    })
}

/// 将 Responses 工具参数规整为 Chat Completions 要求的 JSON 字符串。
fn response_tool_arguments_to_chat(value: &Value) -> String {
    match value {
        Value::String(text) => canonicalize_json_string_if_parseable(text),
        other => canonical_json_string(other),
    }
}

/// 从 Responses status/incomplete_details 推断 OpenAI finish_reason。
fn finish_reason_from_response(response: &Value, has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        return "tool_calls";
    }
    match response.get("status").and_then(|value| value.as_str()) {
        Some("incomplete") => "length",
        _ => "stop",
    }
}

/// 将 Responses usage 转成 Chat Completions usage。
fn responses_usage_to_chat_usage(usage: Option<&Value>) -> Value {
    let Some(usage) = usage.filter(|value| value.is_object()) else {
        return json!({
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        });
    };

    let prompt_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let mut mapped = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": usage
            .get("total_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or(prompt_tokens + completion_tokens)
    });

    if let Some(cached) = usage
        .pointer("/input_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cache_read_input_tokens"))
    {
        mapped["prompt_tokens_details"] = json!({ "cached_tokens": cached });
    }
    mapped
}

/// 解析一个 SSE 文本块中的 event 和 data。
fn parse_sse_block(block: &str) -> Result<(Option<String>, Value), ProxyError> {
    let mut event_name = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = strip_sse_field(line, "event") {
            event_name = Some(event.to_string());
        } else if let Some(data) = strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }

    let data_str = data_lines.join("\n");
    if data_str.trim().is_empty() || data_str.trim() == "[DONE]" {
        return Ok((event_name, Value::Null));
    }
    let data = serde_json::from_str(&data_str).map_err(|err| {
        ProxyError::TransformError(format!("Failed to parse Responses SSE data: {err}"))
    })?;
    Ok((event_name, data))
}

/// 生成一条 OpenAI SSE data 事件。
fn chat_sse_data(value: Value) -> Bytes {
    let data = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("data: {data}\n\n"))
}

/// 将 Responses id 映射成 Chat Completions id。
fn chat_id_from_response_id(id: Option<&str>) -> String {
    let id = id.unwrap_or("resp_ccswitch");
    if id.starts_with("chatcmpl_") {
        id.to_string()
    } else {
        format!("chatcmpl_{id}")
    }
}

/// 生成当前 Unix 时间戳，作为缺省 created 值。
fn current_unix_timestamp() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// 规整不同 Responses 错误形状里的 error 对象。
fn normalize_error_object(error: &Value) -> Value {
    json!({
        "message": error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("Upstream Codex OAuth request failed"),
        "type": error
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("upstream_error"),
        "code": error.get("code").cloned().unwrap_or(Value::Null),
        "param": error.get("param").cloned().unwrap_or(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{stream, StreamExt};

    #[test]
    fn chat_request_maps_to_codex_responses_contract() {
        let input = json!({
            "model": "gpt-5.4-mini",
            "messages": [
                {"role": "system", "content": "Be concise."},
                {"role": "user", "content": "ping"}
            ],
            "stream": false,
            "temperature": 0.2,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "Lookup data",
                    "parameters": {"type": "object"}
                }
            }]
        });

        let result = chat_completions_request_to_codex_responses(input).unwrap();

        assert_eq!(result["model"], "gpt-5.4-mini");
        assert_eq!(result["instructions"], "Be concise.");
        assert_eq!(result["input"][0]["role"], "user");
        assert_eq!(result["input"][0]["content"][0]["text"], "ping");
        assert_eq!(result["store"], false);
        assert_eq!(result["stream"], true);
        assert!(result.get("temperature").is_none());
        assert_eq!(result["tools"][0]["name"], "lookup");
        assert_eq!(result["include"][0], "reasoning.encrypted_content");
    }

    #[test]
    fn chat_request_preserves_chinese_through_codex_responses_conversion() {
        // 第三方 OpenAI-compatible API 会先解析 Chat JSON，再转换为 Codex Responses；
        // 这里同时覆盖原始 UTF-8 中文和 ensure_ascii 风格的 Unicode 转义输入。
        let user_text = "当前页是教材与参考资料页，请用两句话说明应该学什么。Bondy-Murty、West";
        let input = json!({
            "model": "gpt-5.5",
            "messages": [
                {"role": "system", "content": "你是一个中文教学助手。"},
                {"role": "user", "content": user_text}
            ],
            "stream": false
        });

        let result = chat_completions_request_to_codex_responses(input).unwrap();
        let outbound_bytes = serde_json::to_vec(&result).expect("serialize outbound body");
        let reparsed: Value = serde_json::from_slice(&outbound_bytes).expect("reparse body");

        assert_eq!(reparsed["instructions"], "你是一个中文教学助手。");
        assert_eq!(reparsed["input"][0]["content"][0]["text"], user_text);
        assert!(!reparsed["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains('?'));

        let escaped_input: Value = serde_json::from_slice(
            br#"{"model":"gpt-5.5","messages":[{"role":"user","content":"\u4f60\u597d\uff0c\u4e16\u754c\uff01"}]}"#,
        )
        .expect("parse escaped unicode body");
        let escaped_result = chat_completions_request_to_codex_responses(escaped_input).unwrap();

        assert_eq!(
            escaped_result["input"][0]["content"][0]["text"],
            "你好，世界！"
        );
    }

    #[test]
    fn chat_request_preserves_responses_style_text_parts() {
        // 部分 Agent SDK 会把 Chat message content 发成 Responses 风格的 input_text；
        // 外部 API 必须保留这些文本，否则上游只能看到剩余的英文引用或空消息。
        let input = json!({
            "model": "gpt-5.5",
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "请总结教材页：Bondy-Murty 与 West 应该怎么学？"
                    }
                ]
            }]
        });

        let result = chat_completions_request_to_codex_responses(input).unwrap();

        assert_eq!(
            result["input"][0]["content"][0]["text"],
            "请总结教材页：Bondy-Murty 与 West 应该怎么学？"
        );
        assert_eq!(result["input"][0]["content"][0]["type"], "input_text");
    }

    #[test]
    fn chat_request_without_system_prompt_still_sets_codex_instructions() {
        let input = json!({
            "model": "gpt-5.4-mini",
            "messages": [
                {"role": "user", "content": "ping"}
            ],
            "stream": false
        });

        let result = chat_completions_request_to_codex_responses(input).unwrap();

        assert_eq!(result["instructions"], "You are a helpful assistant.");
        assert_eq!(result["input"][0]["content"][0]["text"], "ping");
    }

    #[test]
    fn codex_responses_request_normalizer_accepts_minimal_body() {
        // official Codex backend 比公开 OpenAI Responses 更严格：最小 payload
        // 必须补齐 Codex Desktop 请求体里的必填字段后才能透传。
        let body = json!({
            "model": "gpt-5.4-mini",
            "input": "ping",
            "store": true,
            "stream": false,
            "temperature": 0.1,
            "top_p": 0.9,
            "max_output_tokens": 32
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);

        assert_eq!(normalized["instructions"], "You are a helpful assistant.");
        assert_eq!(normalized["store"], false);
        assert_eq!(normalized["stream"], true);
        assert_eq!(normalized["tools"], json!([]));
        assert_eq!(normalized["parallel_tool_calls"], false);
        assert_eq!(
            normalized["include"],
            json!(["reasoning.encrypted_content"])
        );
        assert_eq!(normalized["input"][0]["type"], "message");
        assert_eq!(normalized["input"][0]["role"], "user");
        assert_eq!(normalized["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(normalized["input"][0]["content"][0]["text"], "ping");
        assert!(normalized.get("temperature").is_none());
        assert!(normalized.get("top_p").is_none());
        assert!(normalized.get("max_output_tokens").is_none());
    }

    #[test]
    fn codex_responses_request_normalizer_preserves_desktop_shape() {
        // Desktop 已经发送 Codex Responses 数组结构时，normalizer 只做幂等护栏，
        // 不改模型、reasoning、service_tier、tools 或已有 instructions。
        let body = json!({
            "model": "gpt-5.5",
            "instructions": "existing instructions",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }],
            "tools": [{ "type": "function", "name": "lookup" }],
            "parallel_tool_calls": true,
            "include": ["reasoning.encrypted_content"],
            "reasoning": { "effort": "high" },
            "service_tier": "priority",
            "store": false,
            "stream": true
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);

        assert_eq!(normalized["model"], "gpt-5.5");
        assert_eq!(normalized["instructions"], "existing instructions");
        assert_eq!(normalized["input"][0]["content"][0]["text"], "hello");
        assert_eq!(normalized["tools"][0]["name"], "lookup");
        assert_eq!(normalized["parallel_tool_calls"], true);
        assert_eq!(normalized["reasoning"]["effort"], "high");
        assert_eq!(normalized["service_tier"], "priority");
        assert_eq!(
            normalized["include"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|value| value.as_str() == Some("reasoning.encrypted_content"))
                .count(),
            1
        );
    }

    #[test]
    fn codex_responses_request_normalizer_strips_content_from_tool_output_items() {
        // ChatGPT Codex OAuth backend 的 tool/call output item 不接受 content 数组；
        // Codex Desktop 某些工具回传会额外带 content，必须在直透前删除。
        let body = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "run tool" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_fn",
                    "output": "done",
                    "content": [{ "type": "output_text", "text": "done" }]
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom",
                    "output": { "body": "patched" },
                    "content": [{ "type": "output_text", "text": "patched" }]
                },
                {
                    "type": "tool_search_output",
                    "call_id": "call_search",
                    "status": "completed",
                    "execution": "client",
                    "tools": [],
                    "content": [{ "type": "output_text", "text": "[]" }]
                }
            ]
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);
        let input = normalized["input"].as_array().expect("input array");

        assert!(input[0].get("content").is_some());
        assert!(input[1].get("content").is_none());
        assert!(input[2].get("content").is_none());
        assert!(input[3].get("content").is_none());
        assert_eq!(input[1]["output"], "done");
        assert_eq!(input[2]["output"]["body"], "patched");
        assert_eq!(input[3]["tools"], json!([]));
    }

    #[test]
    fn codex_oauth_responses_normalizer_preserves_agent_message_content() {
        // Codex Multi-Agent V2 会把任务投递和子 Agent 结果作为 agent_message 写入历史，
        // 其 content 是 official backend 的必填字段。旧的“非 message 全删 content”
        // 规则会把第 21 条历史变成 input[20].content 缺失。
        let encrypted = base64::engine::general_purpose::STANDARD.encode([7_u8; 64]);
        let mut input = (0..20)
            .map(|index| {
                json!({
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": format!("history-{index}")
                    }]
                })
            })
            .collect::<Vec<_>>();
        input.push(json!({
            "type": "agent_message",
            "author": "/root/worker",
            "recipient": "/root",
            "content": [
                {
                    "type": "input_text",
                    "text": "Message Type: FINAL_ANSWER\nPayload:\nDone."
                },
                {
                    "type": "encrypted_content",
                    "encrypted_content": encrypted
                }
            ]
        }));

        let normalized = normalize_codex_oauth_responses_request(
            json!({
                "model": "gpt-5.6-sol",
                "input": input
            }),
            false,
        );
        let input = normalized["input"].as_array().expect("input array");

        assert_eq!(input[20]["type"], "agent_message");
        assert_eq!(input[20]["author"], "/root/worker");
        assert_eq!(input[20]["recipient"], "/root");
        assert_eq!(input[20]["content"][0]["type"], "input_text");
        assert_eq!(input[20]["content"][1]["type"], "encrypted_content");
    }

    #[test]
    fn codex_oauth_responses_normalizer_recovers_plaintext_encrypted_agent_message() {
        let encrypted = base64::engine::general_purpose::STANDARD.encode([9_u8; 64]);
        let input = json!({
            "input": [{
                "type": "agent_message",
                "author": "/root/research",
                "recipient": "/root",
                "content": [
                    {"type": "input_text", "text": "status"},
                    {"type": "encrypted_content", "encrypted_content": "父 agent，我收到的 payload 为空。"},
                    {"type": "encrypted_content", "encrypted_content": encrypted}
                ]
            }]
        });

        let normalized = normalize_codex_oauth_responses_request(input, false);
        let content = normalized["input"][0]["content"].as_array().unwrap();
        assert_eq!(
            content[1],
            json!({"type": "input_text", "text": "父 agent，我收到的 payload 为空。"})
        );
        assert_eq!(content[2]["type"], "encrypted_content");
    }

    #[test]
    fn codex_oauth_responses_normalizer_preserves_unknown_item_content() {
        // 新版 Codex 可能先于 CCSwitchMulti 增加 Responses item。未知类型应原样保留，
        // 只有已知禁止 content 的类型才清理，避免再次出现前向兼容性回归。
        let normalized = normalize_codex_oauth_responses_request(
            json!({
                "model": "gpt-5.6-sol",
                "input": [{
                    "type": "future_agent_event",
                    "content": [{ "type": "input_text", "text": "future payload" }]
                }]
            }),
            false,
        );

        assert_eq!(
            normalized["input"][0]["content"],
            json!([{ "type": "input_text", "text": "future payload" }])
        );
    }

    #[test]
    fn codex_oauth_responses_lite_preserves_input_encoded_instructions_and_tools() {
        // Responses-Lite 把 tools 和 instructions 编码进 input。代理必须保持该结构，
        // 不能按标准 Responses 规则提升 developer message 或补顶层空 tools。
        let normalized = normalize_codex_oauth_responses_request(
            json!({
                "model": "gpt-5.6-sol",
                "input": [
                    {
                        "type": "additional_tools",
                        "role": "developer",
                        "tools": [{
                            "type": "namespace",
                            "name": "collaboration",
                            "tools": [{ "type": "function", "name": "spawn_agent" }]
                        }]
                    },
                    {
                        "type": "message",
                        "role": "developer",
                        "content": [{
                            "type": "input_text",
                            "text": "Use the supplied collaboration tools."
                        }]
                    },
                    {
                        "type": "agent_message",
                        "author": "/root/worker",
                        "recipient": "/root",
                        "content": [{
                            "type": "input_text",
                            "text": "Message Type: FINAL_ANSWER\nPayload:\nDone."
                        }]
                    },
                    {
                        "type": "function_call_output",
                        "call_id": "call-1",
                        "output": "done",
                        "content": [{ "type": "output_text", "text": "redundant" }]
                    }
                ]
            }),
            true,
        );
        let input = normalized["input"].as_array().expect("input array");

        assert!(normalized.get("instructions").is_none());
        assert!(normalized.get("tools").is_none());
        assert_eq!(input[0]["type"], "additional_tools");
        assert_eq!(input[0]["tools"][0]["name"], "collaboration");
        assert_eq!(input[1]["role"], "developer");
        assert_eq!(
            input[1]["content"][0]["text"],
            "Use the supplied collaboration tools."
        );
        assert_eq!(input[2]["type"], "agent_message");
        assert!(input[2].get("content").is_some());
        assert!(input[3].get("content").is_none());
    }

    #[test]
    fn codex_oauth_responses_lite_fallback_restores_standard_request_shape() {
        // 上游拒绝 Lite header 后不能只去头重发；additional_tools 和 developer
        // instructions 也必须恢复成标准 Responses 顶层字段。
        let fallback = normalize_codex_responses_lite_fallback_request(json!({
            "model": "gpt-5.6-sol",
            "input": [
                {
                    "type": "additional_tools",
                    "role": "developer",
                    "tools": [
                        { "type": "function", "name": "lookup" },
                        { "type": "function", "name": "update_plan" }
                    ]
                },
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{
                        "type": "input_text",
                        "text": "Use tools when necessary."
                    }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }]
                }
            ]
        }));
        let input = fallback["input"].as_array().expect("input array");

        assert_eq!(fallback["instructions"], "Use tools when necessary.");
        assert_eq!(fallback["tools"].as_array().map(Vec::len), Some(2));
        assert_eq!(fallback["tools"][0]["name"], "lookup");
        assert_eq!(fallback["tools"][1]["name"], "update_plan");
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert!(input
            .iter()
            .all(|item| item.get("type").and_then(Value::as_str) != Some("additional_tools")));
    }

    #[test]
    fn codex_oauth_responses_normalizer_strips_content_from_all_known_non_content_items() {
        // 这组矩阵固定当前 Codex/OpenAI 已知的非 content item；新增协议类型时必须明确
        // 选择加入该列表或走未知类型保守保留，不能再依赖隐式 catch-all。
        let item_types = [
            "additional_tools",
            "item_reference",
            "function_call",
            "function_call_output",
            "custom_tool_call",
            "custom_tool_call_output",
            "mcp_tool_call",
            "mcp_tool_call_output",
            "tool_search_call",
            "tool_search_output",
            "local_shell_call",
            "computer_call",
            "computer_call_output",
            "file_search_call",
            "code_interpreter_call",
            "web_search_call",
            "image_generation_call",
            "compaction",
            "compaction_trigger",
            "context_compaction",
        ];
        let input = item_types
            .iter()
            .map(|item_type| {
                json!({
                    "type": item_type,
                    "call_id": format!("call-{item_type}"),
                    "content": [{ "type": "output_text", "text": "redundant" }]
                })
            })
            .collect::<Vec<_>>();

        let normalized = normalize_codex_oauth_responses_request(
            json!({
                "model": "gpt-5.6-sol",
                "input": input
            }),
            false,
        );
        let input = normalized["input"].as_array().expect("input array");

        for (index, item_type) in item_types.iter().enumerate() {
            assert_eq!(input[index]["type"], *item_type);
            assert!(
                input[index].get("content").is_none(),
                "{item_type} must not keep content"
            );
        }
    }

    #[test]
    fn codex_oauth_responses_normalizer_removes_duplicate_reasoning_content() {
        // official OAuth backend 依赖 encrypted_content/summary 回放 reasoning；
        // 带 encrypted_content 的 reasoning input 不能再携带 raw content，否则上游会按
        // content 最大长度 0 拒绝。
        let body = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }]
                },
                {
                    "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": "Need to inspect files." }],
                    "encrypted_content": "enc_reasoning",
                    "content": [{ "type": "reasoning_text", "text": "raw hidden reasoning" }]
                }
            ]
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);
        let input = normalized["input"].as_array().expect("input array");

        assert!(input[1].get("content").is_none());
        assert_eq!(input[1]["summary"][0]["text"], "Need to inspect files.");
        assert_eq!(input[1]["encrypted_content"], "enc_reasoning");
    }

    #[test]
    fn codex_oauth_responses_normalizer_promotes_raw_reasoning_content_to_summary() {
        // 旧会话或第三方转换可能只有 reasoning.content，没有 summary。
        // 为了不影响续写体验，official OAuth 直透前把可读文本搬到 summary_text。
        let body = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }]
                },
                {
                    "type": "reasoning",
                    "content": [
                        { "type": "reasoning_text", "text": "Need to check current route." },
                        { "type": "text", "text": "Then answer." }
                    ]
                }
            ]
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);
        let input = normalized["input"].as_array().expect("input array");

        assert!(input[1].get("content").is_none());
        assert_eq!(
            input[1]["summary"],
            json!([
                { "type": "summary_text", "text": "Need to check current route." },
                { "type": "summary_text", "text": "Then answer." }
            ])
        );
    }

    #[test]
    fn codex_responses_passthrough_promotes_control_messages_to_instructions() {
        // Codex Desktop 在同一 session 内切换模型时会把 developer/system 控制消息
        // 放入 input；第三方 Responses API 更严格，不能把这些内部角色当对话消息透传。
        let body = json!({
            "model": "gpt-5.4",
            "instructions": "Existing instructions.",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{ "type": "input_text", "text": "Model switch notice." }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }]
                },
                {
                    "type": "message",
                    "role": "system",
                    "content": "Session policy."
                }
            ]
        });

        let normalized = normalize_codex_responses_passthrough_request(body);
        let input = normalized["input"].as_array().expect("input array");

        assert_eq!(
            normalized["instructions"],
            "Existing instructions.\n\nModel switch notice.\n\nSession policy."
        );
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["text"], "continue");
    }

    #[test]
    fn codex_responses_passthrough_normalizes_function_call_arguments() {
        // MiniMax 等严格 Responses 上游会重新解析历史 function_call.arguments。
        // 空字符串或被截断的 JSON 片段必须在透传前变成合法 JSON 字符串。
        let body = json!({
            "model": "MiniMax-M3",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_empty",
                    "name": "update_plan",
                    "arguments": ""
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_empty",
                    "output": "ok"
                },
                {
                    "type": "function_call",
                    "call_id": "call_partial",
                    "name": "read_file",
                    "arguments": "{"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }]
                }
            ]
        });

        let normalized = normalize_codex_responses_passthrough_request(body);
        let input = normalized["input"].as_array().expect("input array");

        assert_eq!(input[0]["arguments"], "{}");
        assert_eq!(input[2]["arguments"], r#"{"raw_arguments":"{"}"#);
        assert_eq!(input[3]["role"], "user");
    }

    #[test]
    fn codex_oauth_responses_normalizer_promotes_control_messages_and_strips_tool_content() {
        // official OAuth passthrough 复用通用控制消息提升，同时仍执行 ChatGPT backend
        // 专属的 tool output content 清理。
        let body = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{ "type": "input_text", "text": "Model switch notice." }]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "run tool" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_fn",
                    "output": "done",
                    "content": [{ "type": "output_text", "text": "done" }]
                }
            ]
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);
        let input = normalized["input"].as_array().expect("input array");

        assert_eq!(normalized["instructions"], "Model switch notice.");
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "user");
        assert!(input[1].get("content").is_none());
        assert_eq!(input[1]["output"], "done");
    }

    #[test]
    fn codex_oauth_responses_normalizer_promotes_multi_part_developer_message() {
        // Codex Desktop 的宿主提示会把多个 instructions 段落放进同一条
        // developer message；official /responses 直透必须整体提升到顶层
        // instructions，不能把这些段落作为 input[n].content 继续发给上游。
        let body = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [
                        { "type": "input_text", "text": "<permissions instructions>" },
                        { "type": "input_text", "text": "<app-context>" },
                        { "type": "input_text", "text": "<collaboration_mode>" },
                        { "type": "input_text", "text": "<apps_instructions>" },
                        { "type": "input_text", "text": "<skills_instructions>" },
                        { "type": "input_text", "text": "<plugins_instructions>" }
                    ]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "continue" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_fn",
                    "output": "done",
                    "content": [{ "type": "output_text", "text": "done" }]
                }
            ]
        });

        let normalized = normalize_codex_oauth_responses_request(body, false);
        let input = normalized["input"].as_array().expect("input array");

        assert_eq!(
            normalized["instructions"],
            [
                "<permissions instructions>",
                "<app-context>",
                "<collaboration_mode>",
                "<apps_instructions>",
                "<skills_instructions>",
                "<plugins_instructions>",
            ]
            .join("\n")
        );
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["text"], "continue");
        assert_eq!(input[1]["type"], "function_call_output");
        assert!(input[1].get("content").is_none());
    }

    #[test]
    fn responses_json_maps_to_chat_completion() {
        let response = json!({
            "id": "resp_123",
            "model": "gpt-5.4-mini",
            "created_at": 123,
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "pong"}]
            }],
            "usage": {"input_tokens": 4, "output_tokens": 2}
        });

        let result = codex_responses_to_chat_completion(response, "fallback").unwrap();

        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["id"], "chatcmpl_resp_123");
        assert_eq!(result["choices"][0]["message"]["content"], "pong");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
        assert_eq!(result["usage"]["prompt_tokens"], 4);
        assert_eq!(result["usage"]["completion_tokens"], 2);
    }

    #[test]
    fn responses_json_with_null_error_maps_to_chat_completion() {
        let response = json!({
            "id": "resp_null_error",
            "model": "gpt-5.4-mini",
            "created_at": 123,
            "status": "completed",
            "error": null,
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "pong"}]
            }]
        });

        let result = codex_responses_to_chat_completion(response, "fallback").unwrap();

        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["choices"][0]["message"]["content"], "pong");
        assert!(result.get("error").is_none());
    }

    #[test]
    fn responses_tool_call_maps_to_chat_tool_call() {
        let response = json!({
            "id": "resp_tools",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{\"q\":\"x\"}"
            }]
        });

        let result = codex_responses_to_chat_completion(response, "gpt-5.4-mini").unwrap();

        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(result["choices"][0]["message"]["content"], Value::Null);
        assert_eq!(
            result["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
    }

    #[tokio::test]
    async fn responses_sse_maps_to_chat_sse() {
        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_stream\",\"model\":\"gpt-5.4-mini\",\"created_at\":123}}\n\n",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            )),
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_stream\",\"model\":\"gpt-5.4-mini\",\"created_at\":123,\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
            )),
        ]);

        let output = create_chat_sse_stream_from_codex_responses(upstream, "fallback".to_string())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
            .collect::<String>();

        assert!(output.contains("\"object\":\"chat.completion.chunk\""));
        assert!(output.contains("\"content\":\"Hel\""));
        assert!(output.contains("\"content\":\"lo\""));
        assert!(output.contains("\"finish_reason\":\"stop\""));
        assert!(output.contains("\"completion_tokens\":2"));
        assert!(output.contains("\"prompt_tokens\":1"));
        assert!(output.contains("data: [DONE]"));
    }
}
