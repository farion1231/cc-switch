//! Gemini Native format conversion module.
//!
//! Converts Anthropic Messages requests to Gemini `generateContent` requests,
//! and Gemini `GenerateContentResponse` payloads back to Anthropic Messages
//! responses for Claude-compatible clients.

use super::gemini_schema::build_gemini_function_declaration;
use super::gemini_shadow::{GeminiAssistantTurn, GeminiShadowStore, GeminiToolCallMeta};
use crate::proxy::error::ProxyError;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnthropicToolSchemaHint {
    expected_keys: Vec<String>,
    required_keys: Vec<String>,
}

pub type AnthropicToolSchemaHints = HashMap<String, AnthropicToolSchemaHint>;

pub fn anthropic_to_gemini(body: Value) -> Result<Value, ProxyError> {
    anthropic_to_gemini_with_shadow(body, None, None, None)
}

pub fn anthropic_to_gemini_with_shadow(
    body: Value,
    shadow_store: Option<&GeminiShadowStore>,
    provider_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<Value, ProxyError> {
    let mut result = json!({});
    let shadow_turns = shadow_store
        .zip(provider_id)
        .zip(session_id)
        .and_then(|((store, provider_id), session_id)| store.get_session(provider_id, session_id))
        .map(|snapshot| snapshot.turns)
        .unwrap_or_default();

    if let Some(system) = build_system_instruction(body.get("system"))? {
        result["systemInstruction"] = system;
    }

    if let Some(messages) = body.get("messages").and_then(|value| value.as_array()) {
        result["contents"] = json!(convert_messages_to_contents(messages, &shadow_turns)?);
    }

    if let Some(generation_config) = build_generation_config(&body) {
        result["generationConfig"] = generation_config;
    }

    if let Some(tools) = body.get("tools").and_then(|value| value.as_array()) {
        let function_declarations: Vec<Value> = tools
            .iter()
            .filter(|tool| tool.get("type").and_then(|value| value.as_str()) != Some("BatchTool"))
            .map(|tool| {
                build_gemini_function_declaration(
                    tool.get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or(""),
                    tool.get("description").and_then(|value| value.as_str()),
                    tool.get("input_schema")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                )
            })
            .collect();

        if !function_declarations.is_empty() {
            result["tools"] = json!([{ "functionDeclarations": function_declarations }]);
        }
    }

    if let Some(tool_config) = map_tool_choice(body.get("tool_choice"))? {
        result["toolConfig"] = tool_config;
    }

    Ok(result)
}

pub fn gemini_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    gemini_to_anthropic_with_shadow(body, None, None, None)
}

pub fn gemini_to_anthropic_with_shadow(
    body: Value,
    shadow_store: Option<&GeminiShadowStore>,
    provider_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<Value, ProxyError> {
    gemini_to_anthropic_with_shadow_and_hints(body, shadow_store, provider_id, session_id, None)
}

pub fn gemini_to_anthropic_with_shadow_and_hints(
    body: Value,
    shadow_store: Option<&GeminiShadowStore>,
    provider_id: Option<&str>,
    session_id: Option<&str>,
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> Result<Value, ProxyError> {
    if let Some(block_reason) = body
        .get("promptFeedback")
        .and_then(|value| value.get("blockReason"))
        .and_then(|value| value.as_str())
    {
        let text = format!("Request blocked by Gemini safety filters: {block_reason}");
        return Ok(json!({
            "id": body.get("responseId").and_then(|value| value.as_str()).unwrap_or(""),
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "text", "text": text }],
            "model": body.get("modelVersion").and_then(|value| value.as_str()).unwrap_or(""),
            "stop_reason": "refusal",
            "stop_sequence": Value::Null,
            "usage": build_anthropic_usage(body.get("usageMetadata"))
        }));
    }

    let candidate = body
        .get("candidates")
        .and_then(|value| value.as_array())
        .and_then(|value| value.first())
        .ok_or_else(|| {
            ProxyError::TransformError("No candidates in Gemini response".to_string())
        })?;

    let parts = candidate
        .get("content")
        .and_then(|value| value.get("parts"))
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut rectified_parts = parts.clone();
    rectify_tool_call_parts(&mut rectified_parts, tool_schema_hints);

    let mut content = Vec::new();
    let mut has_tool_use = false;

    for part in &rectified_parts {
        if part.get("thought").and_then(|value| value.as_bool()) == Some(true) {
            continue;
        }

        if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
            if !text.is_empty() {
                content.push(json!({
                    "type": "text",
                    "text": text
                }));
            }
            continue;
        }

        if let Some(function_call) = part.get("functionCall") {
            has_tool_use = true;
            content.push(json!({
                "type": "tool_use",
                "id": function_call.get("id").and_then(|value| value.as_str()).unwrap_or(""),
                "name": function_call.get("name").and_then(|value| value.as_str()).unwrap_or(""),
                "input": function_call.get("args").cloned().unwrap_or_else(|| json!({}))
            }));
        }
    }

    let stop_reason = map_finish_reason(
        candidate
            .get("finishReason")
            .and_then(|value| value.as_str()),
        has_tool_use,
    );

    let anthropic_response = json!({
        "id": body.get("responseId").and_then(|value| value.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("modelVersion").and_then(|value| value.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": build_anthropic_usage(body.get("usageMetadata"))
    });

    if let (Some(store), Some(provider_id), Some(session_id), Some(content)) = (
        shadow_store,
        provider_id,
        session_id,
        candidate.get("content"),
    ) {
        let mut shadow_content = content.clone();
        if let Some(parts_value) = shadow_content.get_mut("parts") {
            *parts_value = json!(rectified_parts.clone());
        }
        store.record_assistant_turn(
            provider_id,
            session_id,
            shadow_content,
            extract_tool_call_meta(&rectified_parts),
        );
    }

    Ok(anthropic_response)
}

pub fn extract_gemini_model(body: &Value) -> Option<&str> {
    body.get("model").and_then(|value| value.as_str())
}

fn build_system_instruction(system: Option<&Value>) -> Result<Option<Value>, ProxyError> {
    let Some(system) = system else {
        return Ok(None);
    };

    if let Some(text) = system.as_str() {
        if text.is_empty() {
            return Ok(None);
        }
        return Ok(Some(json!({
            "parts": [{ "text": text }]
        })));
    }

    let Some(blocks) = system.as_array() else {
        return Err(ProxyError::TransformError(
            "Anthropic system must be a string or an array".to_string(),
        ));
    };

    let texts: Vec<&str> = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(|value| value.as_str()))
        .filter(|text| !text.is_empty())
        .collect();

    if texts.is_empty() {
        return Ok(None);
    }

    Ok(Some(json!({
        "parts": [{ "text": texts.join("\n\n") }]
    })))
}

fn build_generation_config(body: &Value) -> Option<Value> {
    let mut config = Map::new();

    if let Some(value) = body.get("max_tokens") {
        config.insert("maxOutputTokens".to_string(), value.clone());
    }
    if let Some(value) = body.get("temperature") {
        config.insert("temperature".to_string(), value.clone());
    }
    if let Some(value) = body.get("top_p") {
        config.insert("topP".to_string(), value.clone());
    }
    if let Some(value) = body.get("stop_sequences") {
        config.insert("stopSequences".to_string(), value.clone());
    }

    if config.is_empty() {
        None
    } else {
        Some(Value::Object(config))
    }
}

fn convert_messages_to_contents(
    messages: &[Value],
    shadow_turns: &[GeminiAssistantTurn],
) -> Result<Vec<Value>, ProxyError> {
    let mut contents = Vec::new();
    let total_assistant_messages = messages
        .iter()
        .filter(|message| message.get("role").and_then(|value| value.as_str()) == Some("assistant"))
        .count();
    let effective_shadow_turns = if shadow_turns.len() > total_assistant_messages {
        &shadow_turns[shadow_turns.len() - total_assistant_messages..]
    } else {
        shadow_turns
    };
    let mut tool_name_by_id = build_tool_name_map_from_shadow_turns(shadow_turns);
    let shadow_start_index = total_assistant_messages.saturating_sub(effective_shadow_turns.len());
    let mut assistant_seen_index = 0usize;

    for message in messages {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("user");

        let gemini_role = if role == "assistant" { "model" } else { "user" };

        let parts = if role == "assistant" {
            let shadow_index = assistant_seen_index
                .checked_sub(shadow_start_index)
                .filter(|index| *index < effective_shadow_turns.len());
            assistant_seen_index += 1;

            if let Some(index) = shadow_index {
                let shadow_turn = &effective_shadow_turns[index];
                merge_tool_names_from_shadow(shadow_turn, &mut tool_name_by_id);
                if let Some(parts) = shadow_parts(&shadow_turn.assistant_content) {
                    parts
                } else {
                    convert_message_content_to_parts(
                        message.get("content"),
                        role,
                        &mut tool_name_by_id,
                    )?
                }
            } else {
                convert_message_content_to_parts(
                    message.get("content"),
                    role,
                    &mut tool_name_by_id,
                )?
            }
        } else {
            convert_message_content_to_parts(message.get("content"), role, &mut tool_name_by_id)?
        };

        if role == "assistant" {
            merge_tool_names_from_parts(&parts, &mut tool_name_by_id);
        }

        contents.push(json!({
            "role": gemini_role,
            "parts": parts
        }));
    }

    Ok(contents)
}

fn convert_message_content_to_parts(
    content: Option<&Value>,
    role: &str,
    tool_name_by_id: &mut std::collections::HashMap<String, String>,
) -> Result<Vec<Value>, ProxyError> {
    let Some(content) = content else {
        return Ok(Vec::new());
    };

    if let Some(text) = content.as_str() {
        return Ok(vec![json!({ "text": text })]);
    }

    let Some(blocks) = content.as_array() else {
        return Err(ProxyError::TransformError(
            "Anthropic message content must be a string or array".to_string(),
        ));
    };

    let mut parts = Vec::new();

    for block in blocks {
        let block_type = block
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|value| value.as_str()) {
                    parts.push(json!({ "text": text }));
                }
            }
            "image" => {
                let source = block.get("source").ok_or_else(|| {
                    ProxyError::TransformError("Gemini image block missing source".to_string())
                })?;

                let source_type = source
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");

                if source_type != "base64" {
                    return Err(ProxyError::TransformError(format!(
                        "Gemini Native only supports base64 image sources, got `{source_type}`"
                    )));
                }

                parts.push(json!({
                    "inlineData": {
                        "mimeType": source.get("media_type").and_then(|value| value.as_str()).unwrap_or("image/png"),
                        "data": source.get("data").and_then(|value| value.as_str()).unwrap_or("")
                    }
                }));
            }
            "document" => {
                let source = block.get("source").ok_or_else(|| {
                    ProxyError::TransformError("Gemini document block missing source".to_string())
                })?;

                let source_type = source
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");

                if source_type != "base64" {
                    return Err(ProxyError::TransformError(format!(
                        "Gemini Native only supports base64 document sources, got `{source_type}`"
                    )));
                }

                parts.push(json!({
                    "inlineData": {
                        "mimeType": source.get("media_type").and_then(|value| value.as_str()).unwrap_or("application/pdf"),
                        "data": source.get("data").and_then(|value| value.as_str()).unwrap_or("")
                    }
                }));
            }
            "tool_use" => {
                if role != "assistant" {
                    return Err(ProxyError::TransformError(
                        "tool_use blocks are only valid in assistant messages".to_string(),
                    ));
                }

                let id = block
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let name = block
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if !id.is_empty() && !name.is_empty() {
                    tool_name_by_id.insert(id.to_string(), name.to_string());
                }

                parts.push(json!({
                    "functionCall": {
                        "id": id,
                        "name": name,
                        "args": block.get("input").cloned().unwrap_or_else(|| json!({}))
                    }
                }));
            }
            "tool_result" => {
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let name = tool_name_by_id
                    .get(tool_use_id)
                    .cloned()
                    .ok_or_else(|| {
                        ProxyError::TransformError(format!(
                            "Unable to resolve Gemini functionResponse.name for tool_use_id `{tool_use_id}`"
                        ))
                    })?;

                parts.push(json!({
                    "functionResponse": {
                        "id": tool_use_id,
                        "name": name,
                        "response": normalize_tool_result_response(block.get("content"))
                    }
                }));
            }
            "thinking" | "redacted_thinking" => {}
            _ => {}
        }
    }

    Ok(parts)
}

fn normalize_tool_result_response(content: Option<&Value>) -> Value {
    match content {
        Some(Value::String(text)) => json!({ "content": text }),
        Some(Value::Array(blocks)) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter(|block| block.get("type").and_then(|value| value.as_str()) == Some("text"))
                .filter_map(|block| block.get("text").and_then(|value| value.as_str()))
                .collect();

            if texts.is_empty() {
                json!({ "content": Value::Array(blocks.clone()) })
            } else {
                json!({ "content": texts.join("\n") })
            }
        }
        Some(value) => json!({ "content": value.clone() }),
        None => json!({ "content": "" }),
    }
}

fn shadow_parts(content: &Value) -> Option<Vec<Value>> {
    content
        .get("parts")
        .and_then(|value| value.as_array())
        .cloned()
        .or_else(|| content.as_array().cloned())
}

pub fn extract_anthropic_tool_schema_hints(body: &Value) -> AnthropicToolSchemaHints {
    body.get("tools")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(|value| value.as_str())?;
            let input_schema = tool
                .get("input_schema")
                .and_then(|value| value.as_object())?;
            let properties = input_schema
                .get("properties")
                .and_then(|value| value.as_object())?;
            if properties.is_empty() {
                return None;
            }

            let expected_keys = properties.keys().cloned().collect::<Vec<_>>();
            let required_keys = input_schema
                .get("required")
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            Some((
                name.to_string(),
                AnthropicToolSchemaHint {
                    expected_keys,
                    required_keys,
                },
            ))
        })
        .collect()
}

pub fn rectify_tool_call_parts(
    parts: &mut [Value],
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) {
    for part in parts {
        let Some(function_call) = part
            .get_mut("functionCall")
            .and_then(|value| value.as_object_mut())
        else {
            continue;
        };
        let Some(name) = function_call
            .get("name")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
        else {
            continue;
        };
        let Some(args) = function_call.get_mut("args") else {
            continue;
        };

        if rectify_tool_call_args(&name, args, tool_schema_hints) {
            log::info!("[Claude/Gemini] Rectified tool args for `{name}`");
        }
    }
}

pub fn rectify_tool_call_args(
    tool_name: &str,
    args: &mut Value,
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> bool {
    let Some(tool_schema_hints) = tool_schema_hints else {
        return false;
    };
    let Some(hint) = tool_schema_hints.get(tool_name) else {
        return false;
    };
    let Some(args_object) = args.as_object_mut() else {
        return false;
    };
    if args_object.is_empty() || hint.expected_keys.is_empty() {
        return false;
    }
    let mut changed = false;

    if hint.expected_keys.iter().any(|key| key == "skill") && !args_object.contains_key("skill") {
        if let Some(value) = args_object.remove("name") {
            args_object.insert("skill".to_string(), value);
            changed = true;
        }
    }

    if let Some(parameters_value) = args_object.remove("parameters") {
        if let Some(parameters_object) = parameters_value.as_object() {
            for expected_key in &hint.expected_keys {
                if args_object.contains_key(expected_key) {
                    continue;
                }
                let Some(value) = parameters_object.get(expected_key) else {
                    continue;
                };
                let normalized_value = match value {
                    Value::Array(values) if values.len() == 1 => values[0].clone(),
                    _ => value.clone(),
                };
                args_object.insert(expected_key.clone(), normalized_value);
                changed = true;
            }
        }
    }

    if hint
        .required_keys
        .iter()
        .all(|key| args_object.contains_key(key.as_str()))
    {
        return changed;
    }

    let expected_key_set = hint
        .expected_keys
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let unexpected_keys = args_object
        .keys()
        .filter(|key| !expected_key_set.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if unexpected_keys.len() != 1 {
        return false;
    }

    let target_key = hint
        .required_keys
        .iter()
        .find(|key| !args_object.contains_key(key.as_str()))
        .cloned()
        .or_else(|| {
            if hint.expected_keys.len() == 1 && args_object.len() == 1 {
                hint.expected_keys.first().cloned()
            } else {
                None
            }
        });
    let Some(target_key) = target_key else {
        return false;
    };
    if args_object.contains_key(&target_key) {
        return false;
    }

    let source_key = &unexpected_keys[0];
    let Some(value) = args_object.remove(source_key) else {
        return false;
    };
    args_object.insert(target_key, value);
    true
}

fn merge_tool_names_from_shadow(
    turn: &GeminiAssistantTurn,
    tool_name_by_id: &mut HashMap<String, String>,
) {
    for tool_call in &turn.tool_calls {
        if let Some(id) = &tool_call.id {
            tool_name_by_id.insert(id.clone(), tool_call.name.clone());
        }
    }

    if let Some(parts) = shadow_parts(&turn.assistant_content) {
        merge_tool_names_from_parts(&parts, tool_name_by_id);
    }
}

fn build_tool_name_map_from_shadow_turns(
    shadow_turns: &[GeminiAssistantTurn],
) -> HashMap<String, String> {
    let mut tool_name_by_id = HashMap::new();
    for turn in shadow_turns {
        merge_tool_names_from_shadow(turn, &mut tool_name_by_id);
    }
    tool_name_by_id
}

fn merge_tool_names_from_parts(parts: &[Value], tool_name_by_id: &mut HashMap<String, String>) {
    for part in parts {
        let Some(function_call) = part.get("functionCall") else {
            continue;
        };
        let Some(id) = function_call.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(name) = function_call.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        if !id.is_empty() && !name.is_empty() {
            tool_name_by_id.insert(id.to_string(), name.to_string());
        }
    }
}

fn extract_tool_call_meta(parts: &[Value]) -> Vec<GeminiToolCallMeta> {
    parts
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

fn map_tool_choice(tool_choice: Option<&Value>) -> Result<Option<Value>, ProxyError> {
    let Some(tool_choice) = tool_choice else {
        return Ok(None);
    };

    match tool_choice {
        Value::String(choice) => Ok(match choice.as_str() {
            "auto" => Some(json!({
                "functionCallingConfig": { "mode": "AUTO" }
            })),
            "none" => Some(json!({
                "functionCallingConfig": { "mode": "NONE" }
            })),
            other => {
                return Err(ProxyError::TransformError(format!(
                    "Unsupported Gemini tool_choice string: {other}"
                )));
            }
        }),
        Value::Object(object) => {
            let Some(choice_type) = object.get("type").and_then(|value| value.as_str()) else {
                return Ok(None);
            };

            let config = match choice_type {
                "auto" => json!({ "mode": "AUTO" }),
                "none" => json!({ "mode": "NONE" }),
                "any" => json!({ "mode": "ANY" }),
                "tool" => {
                    let name = object
                        .get("name")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    json!({
                        "mode": "ANY",
                        "allowedFunctionNames": [name]
                    })
                }
                other => {
                    return Err(ProxyError::TransformError(format!(
                        "Unsupported Gemini tool_choice type: {other}"
                    )));
                }
            };

            Ok(Some(json!({ "functionCallingConfig": config })))
        }
        _ => Ok(None),
    }
}

fn build_anthropic_usage(usage: Option<&Value>) -> Value {
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

fn map_finish_reason(reason: Option<&str>, has_tool_use: bool) -> Value {
    let mapped = match reason {
        Some("MAX_TOKENS") => Some("max_tokens"),
        Some("STOP") | Some("FINISH_REASON_UNSPECIFIED") | None => {
            if has_tool_use {
                Some("tool_use")
            } else {
                Some("end_turn")
            }
        }
        Some("SAFETY")
        | Some("RECITATION")
        | Some("SPII")
        | Some("BLOCKLIST")
        | Some("PROHIBITED_CONTENT") => Some("refusal"),
        Some(other) => {
            log::warn!("[Claude/Gemini] Unknown Gemini finishReason `{other}`, using end_turn");
            Some("end_turn")
        }
    };

    match mapped {
        Some(value) => json!(value),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_to_gemini_maps_system_and_messages() {
        let input = json!({
            "model": "gemini-2.5-pro",
            "max_tokens": 128,
            "system": "You are helpful.",
            "messages": [
                { "role": "user", "content": "Hello" }
            ]
        });

        let result = anthropic_to_gemini(input).unwrap();
        assert_eq!(
            result["systemInstruction"]["parts"][0]["text"],
            "You are helpful."
        );
        assert_eq!(result["contents"][0]["role"], "user");
        assert_eq!(result["contents"][0]["parts"][0]["text"], "Hello");
        assert_eq!(result["generationConfig"]["maxOutputTokens"], 128);
    }

    #[test]
    fn anthropic_to_gemini_maps_tools_and_tool_results() {
        let input = json!({
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        { "type": "tool_use", "id": "call_1", "name": "get_weather", "input": { "city": "Tokyo" } }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                    ]
                }
            ],
            "tools": [
                {
                    "name": "get_weather",
                    "description": "Weather lookup",
                    "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } }
                }
            ],
            "tool_choice": { "type": "tool", "name": "get_weather" }
        });

        let result = anthropic_to_gemini(input).unwrap();
        assert_eq!(
            result["tools"][0]["functionDeclarations"][0]["name"],
            "get_weather"
        );
        assert!(result["tools"][0]["functionDeclarations"][0]
            .get("parameters")
            .is_some());
        assert_eq!(
            result["contents"][0]["parts"][0]["functionCall"]["name"],
            "get_weather"
        );
        assert_eq!(
            result["contents"][1]["parts"][0]["functionResponse"]["name"],
            "get_weather"
        );
        assert_eq!(
            result["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            "get_weather"
        );
    }

    #[test]
    fn anthropic_to_gemini_resolves_tool_result_name_from_shadow_content() {
        let store = GeminiShadowStore::with_limits(8, 4);
        store.record_assistant_turn(
            "provider-a",
            "session-1",
            json!({
                "parts": [{
                    "functionCall": {
                        "id": "call_1",
                        "name": "get_weather",
                        "args": { "city": "Tokyo" }
                    }
                }]
            }),
            vec![],
        );

        let input = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                    ]
                }
            ]
        });

        let result = anthropic_to_gemini_with_shadow(
            input,
            Some(&store),
            Some("provider-a"),
            Some("session-1"),
        )
        .unwrap();

        assert_eq!(
            result["contents"][0]["parts"][0]["functionResponse"]["name"],
            "get_weather"
        );
    }

    #[test]
    fn anthropic_to_gemini_rejects_tool_result_without_resolvable_name() {
        let input = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        { "type": "tool_result", "tool_use_id": "call_1", "content": "Sunny" }
                    ]
                }
            ]
        });

        let error = anthropic_to_gemini(input).unwrap_err();
        assert!(error
            .to_string()
            .contains("Unable to resolve Gemini functionResponse.name"));
    }

    #[test]
    fn anthropic_to_gemini_uses_parameters_json_schema_for_rich_tool_schema() {
        let input = json!({
            "tools": [
                {
                    "name": "search",
                    "description": "Search data",
                    "input_schema": {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    }
                }
            ]
        });

        let result = anthropic_to_gemini(input).unwrap();
        let declaration = &result["tools"][0]["functionDeclarations"][0];

        assert!(declaration.get("parameters").is_none());
        assert!(declaration.get("parametersJsonSchema").is_some());
        assert!(declaration["parametersJsonSchema"].get("$schema").is_none());
        assert_eq!(
            declaration["parametersJsonSchema"]["additionalProperties"],
            false
        );
    }

    #[test]
    fn gemini_to_anthropic_maps_text_and_usage() {
        let input = json!({
            "responseId": "resp_1",
            "modelVersion": "gemini-2.5-pro",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": "Hello from Gemini" }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 12,
                "totalTokenCount": 20,
                "cachedContentTokenCount": 3
            }
        });

        let result = gemini_to_anthropic(input).unwrap();
        assert_eq!(result["id"], "resp_1");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello from Gemini");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 12);
        assert_eq!(result["usage"]["output_tokens"], 8);
        assert_eq!(result["usage"]["cache_read_input_tokens"], 3);
    }

    #[test]
    fn gemini_to_anthropic_maps_function_calls_to_tool_use() {
        let input = json!({
            "responseId": "resp_2",
            "modelVersion": "gemini-2.5-pro",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{
                        "functionCall": {
                            "id": "call_1",
                            "name": "get_weather",
                            "args": { "city": "Tokyo" }
                        }
                    }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "totalTokenCount": 15
            }
        });

        let result = gemini_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_1");
        assert_eq!(result["stop_reason"], "tool_use");
    }

    #[test]
    fn gemini_to_anthropic_rectifies_tool_args_from_schema_hints() {
        let input = json!({
            "responseId": "resp_2",
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
            }]
        });
        let hints = extract_anthropic_tool_schema_hints(&json!({
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

        let result =
            gemini_to_anthropic_with_shadow_and_hints(input, None, None, None, Some(&hints))
                .unwrap();

        assert_eq!(result["content"][0]["input"]["skill"], "git-commit");
        assert_eq!(
            result["content"][0]["input"]["args"],
            "详细分析内容 编写提交信息 分多次提交代码"
        );
        assert!(result["content"][0]["input"].get("name").is_none());
        assert!(result["content"][0]["input"].get("parameters").is_none());
    }

    #[test]
    fn gemini_to_anthropic_maps_blocked_prompt_to_refusal() {
        let input = json!({
            "responseId": "resp_3",
            "modelVersion": "gemini-2.5-flash",
            "promptFeedback": { "blockReason": "SAFETY" },
            "usageMetadata": {
                "promptTokenCount": 4,
                "totalTokenCount": 4
            }
        });

        let result = gemini_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "refusal");
        assert_eq!(result["content"][0]["type"], "text");
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("SAFETY"));
    }

    #[test]
    fn shadow_replay_aligns_to_latest_turns_after_client_truncation() {
        let store = GeminiShadowStore::with_limits(8, 4);
        // Record 3 shadow turns (assistant messages 0, 1, 2)
        for i in 0..3 {
            store.record_assistant_turn(
                "prov",
                "sess",
                json!({
                    "parts": [{
                        "functionCall": {
                            "id": format!("call_{i}"),
                            "name": format!("tool_{i}"),
                            "args": {}
                        }
                    }]
                }),
                vec![],
            );
        }

        // Client truncates history: only sends assistant messages 1 and 2
        let input = json!({
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        { "type": "tool_use", "id": "call_1", "name": "tool_1", "input": {} }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "tool_result", "tool_use_id": "call_1", "content": "ok" }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        { "type": "tool_use", "id": "call_2", "name": "tool_2", "input": {} }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "tool_result", "tool_use_id": "call_2", "content": "ok" }
                    ]
                }
            ]
        });

        let result =
            anthropic_to_gemini_with_shadow(input, Some(&store), Some("prov"), Some("sess"))
                .unwrap();

        // Shadow turns[1] (tool_1) should align with first assistant message,
        // shadow turns[2] (tool_2) with the second — not turns[0] and turns[1].
        assert_eq!(
            result["contents"][0]["parts"][0]["functionCall"]["name"],
            "tool_1"
        );
        assert_eq!(
            result["contents"][2]["parts"][0]["functionCall"]["name"],
            "tool_2"
        );
    }
}
