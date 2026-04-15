use crate::proxy::error::ProxyError;
use serde_json::{json, Map, Value};

pub fn anthropic_to_gemini(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    if let Some(system) = body.get("system") {
        let instruction = system_to_text(system);
        if !instruction.is_empty() {
            result["systemInstruction"] = json!({
                "parts": [{ "text": instruction }]
            });
        }
    }

    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        let contents = messages
            .iter()
            .map(convert_message_to_gemini)
            .collect::<Result<Vec<_>, _>>()?;
        result["contents"] = json!(contents);
    }

    if let Some(v) = body.get("tools").and_then(|v| v.as_array()) {
        let declarations: Vec<Value> = v
            .iter()
            .filter(|tool| tool.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|tool| {
                json!({
                    "functionDeclarations": [{
                        "name": tool.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": tool.get("description").cloned().unwrap_or(Value::Null),
                        "parameters": clean_gemini_schema(
                            tool.get("input_schema").cloned().unwrap_or_else(|| json!({}))
                        )
                    }]
                })
            })
            .collect();
        if !declarations.is_empty() {
            result["tools"] = json!(declarations);
        }
    }

    let mut generation_config = Map::new();
    if let Some(v) = body.get("max_tokens") {
        generation_config.insert("maxOutputTokens".to_string(), v.clone());
    }
    if let Some(v) = body.get("temperature") {
        generation_config.insert("temperature".to_string(), v.clone());
    }
    if let Some(v) = body.get("top_p") {
        generation_config.insert("topP".to_string(), v.clone());
    }
    if let Some(v) = body.get("stop_sequences") {
        generation_config.insert("stopSequences".to_string(), v.clone());
    }
    if !generation_config.is_empty() {
        result["generationConfig"] = Value::Object(generation_config);
    }

    if let Some(v) = body.get("tool_choice") {
        if let Some(config) = map_tool_choice_to_gemini(v) {
            result["toolConfig"] = config;
        }
    }

    Ok(result)
}

fn clean_gemini_schema(schema: Value) -> Value {
    sanitize_gemini_schema(super::transform::clean_schema(schema))
}

fn sanitize_gemini_schema(schema: Value) -> Value {
    match schema {
        Value::Object(mut obj) => {
            let mut cleaned = Map::new();
            let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);

            if let Some(schema_type) = schema_type {
                cleaned.insert("type".to_string(), schema_type);
            }
            if let Some(description) = obj.remove("description").filter(|v| v.is_string()) {
                cleaned.insert("description".to_string(), description);
            }
            if let Some(enum_values) = obj.remove("enum").filter(|v| v.is_array()) {
                cleaned.insert("enum".to_string(), enum_values);
            }
            if let Some(nullable) = nullable {
                cleaned.insert("nullable".to_string(), nullable);
            }
            if let Some(properties) = obj.remove("properties") {
                if let Value::Object(properties) = properties {
                    cleaned.insert(
                        "properties".to_string(),
                        Value::Object(
                            properties
                                .into_iter()
                                .map(|(key, value)| (key, sanitize_gemini_schema(value)))
                                .collect(),
                        ),
                    );
                }
            }
            if let Some(required) = obj.remove("required").filter(|v| v.is_array()) {
                cleaned.insert("required".to_string(), required);
            }
            if let Some(items) = obj.remove("items") {
                cleaned.insert("items".to_string(), sanitize_gemini_schema(items));
            }

            Value::Object(cleaned)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize_gemini_schema).collect()),
        other => other,
    }
}

fn normalize_gemini_type_and_nullable(
    obj: &mut Map<String, Value>,
) -> (Option<Value>, Option<Value>) {
    let mut nullable = obj.remove("nullable").filter(|v| v.is_boolean());
    let schema_type = match obj.remove("type") {
        Some(Value::Array(types)) => {
            let mut non_null_types = Vec::new();
            let mut saw_null = false;

            for value in types {
                match value.as_str() {
                    Some("null") => saw_null = true,
                    Some(_) => non_null_types.push(value),
                    None => {}
                }
            }

            if saw_null {
                nullable = Some(Value::Bool(true));
            }

            if non_null_types.len() == 1 {
                non_null_types.into_iter().next()
            } else {
                None
            }
        }
        Some(value @ Value::String(_)) => Some(value),
        _ => None,
    };

    (schema_type, nullable)
}

pub fn gemini_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    let response = body.get("response").unwrap_or(&body);

    let candidate = response
        .get("candidates")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or_else(|| json!({}));

    let content = candidate
        .get("content")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let parts = content
        .get("parts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let anthropic_content = parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| gemini_part_to_anthropic_block(part, index))
        .collect::<Vec<_>>();

    let stop_reason = map_gemini_finish_reason(
        candidate.get("finishReason").and_then(|v| v.as_str()),
        anthropic_content
            .iter()
            .any(|block| block.get("type").and_then(|v| v.as_str()) == Some("tool_use")),
    );

    let usage = response.get("usageMetadata");

    Ok(json!({
        "id": response
            .get("responseId")
            .or_else(|| body.get("traceId"))
            .and_then(|v| v.as_str())
            .unwrap_or("msg_gemini"),
        "type": "message",
        "role": "assistant",
        "model": response
            .get("modelVersion")
            .and_then(|v| v.as_str())
            .unwrap_or("gemini"),
        "content": anthropic_content,
        "stop_reason": stop_reason,
        "stop_sequence": Value::Null,
        "usage": build_anthropic_usage_from_gemini(usage),
    }))
}

pub fn build_anthropic_usage_from_gemini(usage: Option<&Value>) -> Value {
    let usage = match usage {
        Some(v) if !v.is_null() => v,
        _ => {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0
            })
        }
    };

    let mut result = json!({
        "input_tokens": usage.get("promptTokenCount").and_then(|v| v.as_u64()).unwrap_or(0),
        "output_tokens": usage.get("candidatesTokenCount").and_then(|v| v.as_u64()).unwrap_or(0)
    });

    if let Some(v) = usage
        .get("cachedContentTokenCount")
        .and_then(|v| v.as_u64())
    {
        result["cache_read_input_tokens"] = json!(v);
    }

    result
}

fn system_to_text(system: &Value) -> String {
    if let Some(text) = system.as_str() {
        return text.to_string();
    }

    if let Some(arr) = system.as_array() {
        return arr
            .iter()
            .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n");
    }

    String::new()
}

fn convert_message_to_gemini(message: &Value) -> Result<Value, ProxyError> {
    let role = match message
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("user")
    {
        "assistant" => "model",
        _ => "user",
    };

    let content = message.get("content");
    let parts = convert_content_to_gemini_parts(content)?;

    Ok(json!({
        "role": role,
        "parts": parts,
    }))
}

fn convert_content_to_gemini_parts(content: Option<&Value>) -> Result<Vec<Value>, ProxyError> {
    let Some(content) = content else {
        return Ok(vec![]);
    };

    if let Some(text) = content.as_str() {
        return Ok(vec![json!({ "text": text })]);
    }

    if let Some(blocks) = content.as_array() {
        let mut parts = Vec::new();
        for block in blocks {
            match block.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        parts.push(json!({ "text": text }));
                    }
                }
                "image" => {
                    if let Some(source) = block.get("source") {
                        let media_type = source
                            .get("media_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/png");
                        let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                        parts.push(json!({
                            "inlineData": {
                                "mimeType": media_type,
                                "data": data,
                            }
                        }));
                    }
                }
                "tool_use" => {
                    parts.push(json!({
                        "functionCall": {
                            "name": block.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "args": block.get("input").cloned().unwrap_or_else(|| json!({}))
                        }
                    }));
                }
                "tool_result" => {
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool_result");
                    let response = match block.get("content") {
                        Some(Value::String(text)) => json!({ "content": text }),
                        Some(Value::Array(arr)) => json!({ "content": flatten_text_blocks(arr) }),
                        Some(value) => json!({ "content": value }),
                        None => json!({}),
                    };
                    parts.push(json!({
                        "functionResponse": {
                            "name": name,
                            "response": response
                        }
                    }));
                }
                "thinking" => {}
                _ => {}
            }
        }
        return Ok(parts);
    }

    Ok(vec![json!({ "text": content })])
}

fn flatten_text_blocks(arr: &[Value]) -> String {
    arr.iter()
        .filter_map(|item| {
            if let Some(text) = item.as_str() {
                Some(text.to_string())
            } else {
                item.get("text")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn map_tool_choice_to_gemini(tool_choice: &Value) -> Option<Value> {
    let mode = match tool_choice {
        Value::String(value) => match value.as_str() {
            "auto" => "AUTO",
            "any" => "ANY",
            "none" => "NONE",
            _ => "AUTO",
        },
        Value::Object(obj) => match obj.get("type").and_then(|v| v.as_str()) {
            Some("any") => "ANY",
            Some("none") => "NONE",
            Some("auto") => "AUTO",
            Some("tool") => "ANY",
            _ => "AUTO",
        },
        _ => "AUTO",
    };

    Some(json!({
        "functionCallingConfig": {
            "mode": mode
        }
    }))
}

fn gemini_part_to_anthropic_block(part: &Value, index: usize) -> Option<Value> {
    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            return Some(json!({ "type": "text", "text": text }));
        }
    }

    if let Some(function_call) = part.get("functionCall") {
        let tool_id = function_call
            .get("id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("toolu_gemini_{index}"));
        return Some(json!({
            "type": "tool_use",
            "id": tool_id,
            "name": function_call.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "input": function_call.get("args").cloned().unwrap_or_else(|| json!({}))
        }));
    }

    None
}

pub fn map_gemini_finish_reason(reason: Option<&str>, has_tool_use: bool) -> &'static str {
    match reason {
        Some("MAX_TOKENS") => "max_tokens",
        Some("SAFETY") | Some("RECITATION") | Some("SPII") | Some("PROHIBITED_CONTENT") => {
            "end_turn"
        }
        _ if has_tool_use => "tool_use",
        _ => "end_turn",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn anthropic_request_converts_to_gemini() {
        let body = json!({
            "model": "gemini-3.1-pro-preview",
            "system": "You are helpful",
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }],
            "max_tokens": 128,
            "stream": true
        });

        let converted = anthropic_to_gemini(body).unwrap();
        assert_eq!(
            converted["systemInstruction"]["parts"][0]["text"],
            "You are helpful"
        );
        assert_eq!(converted["contents"][0]["role"], "user");
        assert_eq!(converted["contents"][0]["parts"][0]["text"], "hello");
        assert_eq!(converted["generationConfig"]["maxOutputTokens"], 128);
    }

    #[test]
    fn anthropic_tools_strip_unsupported_gemini_schema_keywords() {
        let body = json!({
            "model": "gemini-3.1-pro-preview",
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }],
            "tools": [{
                "name": "test_tool",
                "description": "Test tool",
                "input_schema": {
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "propertyNames": { "pattern": "^[a-z]+$" },
                    "properties": {
                        "payload": {
                            "type": "object",
                            "propertyNames": { "pattern": "^[a-z]+$" },
                            "properties": {
                                "mode": {
                                    "type": ["string", "null"],
                                    "description": "Execution mode"
                                }
                            },
                            "required": ["mode"]
                        }
                    },
                    "required": ["payload"]
                }
            }]
        });

        let converted = anthropic_to_gemini(body).unwrap();
        let params = &converted["tools"][0]["functionDeclarations"][0]["parameters"];

        assert!(params.get("$schema").is_none());
        assert!(params.get("propertyNames").is_none());
        assert!(params["properties"]["payload"].get("propertyNames").is_none());
        assert_eq!(params["properties"]["payload"]["properties"]["mode"]["type"], "string");
        assert_eq!(
            params["properties"]["payload"]["properties"]["mode"]["nullable"],
            true
        );
    }

    #[test]
    fn clean_gemini_schema_keeps_supported_subset_only() {
        let cleaned = clean_gemini_schema(json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "title": "Ignored",
            "additionalProperties": false,
            "properties": {
                "tags": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "format": "uri"
                    }
                }
            },
            "required": ["tags"]
        }));

        assert_eq!(
            cleaned,
            json!({
                "type": "object",
                "properties": {
                    "tags": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    }
                },
                "required": ["tags"]
            })
        );
    }

    #[test]
    fn clean_gemini_schema_maps_nullable_union_type() {
        let cleaned = clean_gemini_schema(json!({
            "type": ["integer", "null"],
            "description": "Optional count"
        }));

        assert_eq!(
            cleaned,
            json!({
                "type": "integer",
                "description": "Optional count",
                "nullable": true
            })
        );
    }

    #[test]
    fn clean_gemini_schema_drops_unsupported_multi_type_union() {
        let cleaned = clean_gemini_schema(json!({
            "type": ["string", "integer"]
        }));

        assert_eq!(cleaned, json!({}));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_detects_null_union() {
        let mut obj = serde_json::Map::from_iter([(
            "type".to_string(),
            json!(["number", "null"]),
        )]);

        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, Some(json!("number")));
        assert_eq!(nullable, Some(json!(true)));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_keeps_explicit_nullable() {
        let mut obj = serde_json::Map::from_iter([
            ("type".to_string(), json!("boolean")),
            ("nullable".to_string(), json!(true)),
        ]);

        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, Some(json!("boolean")));
        assert_eq!(nullable, Some(json!(true)));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_drops_complex_union() {
        let mut obj = serde_json::Map::from_iter([(
            "type".to_string(),
            json!(["string", "integer", "null"]),
        )]);

        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, None);
        assert_eq!(nullable, Some(json!(true)));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_ignores_non_string_types() {
        let mut obj = serde_json::Map::from_iter([(
            "type".to_string(),
            json!(["string", { "bad": true }]),
        )]);

        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, Some(json!("string")));
        assert_eq!(nullable, None);
    }

    #[test]
    fn normalize_gemini_type_and_nullable_handles_missing_type() {
        let mut obj = serde_json::Map::new();
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, None);
        assert_eq!(nullable, None);
    }

    #[test]
    fn normalize_gemini_type_and_nullable_handles_invalid_type_value() {
        let mut obj = serde_json::Map::from_iter([("type".to_string(), json!(123))]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, None);
        assert_eq!(nullable, None);
    }

    #[test]
    fn normalize_gemini_type_and_nullable_handles_only_null_union() {
        let mut obj = serde_json::Map::from_iter([("type".to_string(), json!(["null"]))]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, None);
        assert_eq!(nullable, Some(json!(true)));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_preserves_false_nullable() {
        let mut obj = serde_json::Map::from_iter([
            ("type".to_string(), json!("string")),
            ("nullable".to_string(), json!(false)),
        ]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, Some(json!("string")));
        assert_eq!(nullable, Some(json!(false)));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_ignores_non_boolean_nullable() {
        let mut obj = serde_json::Map::from_iter([
            ("type".to_string(), json!("string")),
            ("nullable".to_string(), json!("yes")),
        ]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, Some(json!("string")));
        assert_eq!(nullable, None);
    }

    #[test]
    fn normalize_gemini_type_and_nullable_accepts_single_non_null_from_union() {
        let mut obj = serde_json::Map::from_iter([(
            "type".to_string(),
            json!(["object", "null"]),
        )]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, Some(json!("object")));
        assert_eq!(nullable, Some(json!(true)));
    }

    #[test]
    fn normalize_gemini_type_and_nullable_drops_empty_union() {
        let mut obj = serde_json::Map::from_iter([("type".to_string(), json!([]))]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, None);
        assert_eq!(nullable, None);
    }

    #[test]
    fn normalize_gemini_type_and_nullable_drops_multiple_non_null_union() {
        let mut obj = serde_json::Map::from_iter([(
            "type".to_string(),
            json!(["object", "array"]),
        )]);
        let (schema_type, nullable) = normalize_gemini_type_and_nullable(&mut obj);
        assert_eq!(schema_type, None);
        assert_eq!(nullable, None);
    }

    #[test]
    fn gemini_response_converts_to_anthropic() {
        let body = json!({
            "responseId": "resp_1",
            "modelVersion": "gemini-3.1-pro-preview",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": "hi" }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 4
            }
        });

        let converted = gemini_to_anthropic(body).unwrap();
        assert_eq!(converted["role"], "assistant");
        assert_eq!(converted["content"][0]["text"], "hi");
        assert_eq!(converted["usage"]["input_tokens"], 10);
        assert_eq!(converted["usage"]["output_tokens"], 4);
    }

    #[test]
    fn gemini_function_call_converts_to_tool_use() {
        let body = json!({
            "responseId": "resp_2",
            "modelVersion": "gemini-3.1-pro-preview",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "lookup_weather",
                            "args": { "city": "Tokyo" }
                        }
                    }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 12,
                "candidatesTokenCount": 3
            }
        });

        let converted = gemini_to_anthropic(body).unwrap();
        assert_eq!(converted["stop_reason"], "tool_use");
        assert_eq!(converted["content"][0]["type"], "tool_use");
        assert_eq!(converted["content"][0]["name"], "lookup_weather");
        assert_eq!(converted["content"][0]["input"]["city"], "Tokyo");
        assert_eq!(converted["content"][0]["id"], "toolu_gemini_0");
    }

    #[test]
    fn wrapped_code_assist_response_converts_to_anthropic() {
        let body = json!({
            "traceId": "trace_1",
            "response": {
                "modelVersion": "gemini-3.1-pro-preview",
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {
                        "parts": [{ "text": "pong" }]
                    }
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 4
                }
            }
        });

        let converted = gemini_to_anthropic(body).unwrap();
        assert_eq!(converted["id"], "trace_1");
        assert_eq!(converted["content"][0]["text"], "pong");
        assert_eq!(converted["model"], "gemini-3.1-pro-preview");
        assert_eq!(converted["usage"]["input_tokens"], 10);
        assert_eq!(converted["usage"]["output_tokens"], 4);
    }
}
