//! Reversible tool names for Claude → Chat / Responses / Gemini conversions.

use super::{
    hyper_client::ProxyResponse,
    response_processor::{read_decoded_body, strip_entity_headers_for_rebuilt_body},
    sse::{append_utf8_safe, strip_sse_field, take_sse_block},
    ProxyError,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};

#[derive(Default)]
pub(crate) struct ToolAliases {
    reverse: HashMap<String, String>,
}

impl ToolAliases {
    fn alias(&mut self, value: &mut Value) {
        let Some(name) = value.as_str() else { return };
        let original = name.to_string();
        // Chat function names are ASCII and limited to 64 bytes. A digest keeps
        // long names with the same truncated prefix distinct and stable per turn.
        let alias = if original.len() <= 60 {
            format!("ccs_{original}")
        } else {
            let hash = format!("{:x}", Sha256::digest(original.as_bytes()));
            let prefix: String = original
                .chars()
                .take(43)
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            format!("ccs_{prefix}_{}", &hash[..16])
        };
        self.reverse.insert(alias.clone(), original);
        *value = Value::String(alias);
    }

    fn restore_function(&self, function: &mut Value) {
        if let Some(name) = function.get_mut("name") {
            if let Some(original) = name.as_str().and_then(|name| self.reverse.get(name)) {
                *name = Value::String(original.clone());
            }
        }
    }

    fn restore_message(&self, message: &mut Value) {
        if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for call in calls {
                if let Some(function) = call.get_mut("function") {
                    self.restore_function(function);
                }
            }
        }
        if let Some(function) = message.get_mut("function_call") {
            self.restore_function(function);
        }
    }

    fn restore_json(&self, body: &mut Value) {
        if let Some(choices) = body.get_mut("choices").and_then(Value::as_array_mut) {
            for choice in choices {
                if let Some(message) = choice.get_mut("message") {
                    self.restore_message(message);
                }
            }
        }
        self.restore_response_items(body);
        if let Some(response) = body.get_mut("response") {
            self.restore_response_items(response);
        }
        if let Some(item) = body.get_mut("item") {
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                self.restore_function(item);
            }
        }
        if body
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.starts_with("response.function_call_arguments."))
        {
            self.restore_function(body);
        }
        // Gemini uses the same candidate shape in JSON and each SSE event.
        if let Some(candidates) = body.get_mut("candidates").and_then(Value::as_array_mut) {
            for candidate in candidates {
                if let Some(parts) = candidate
                    .pointer_mut("/content/parts")
                    .and_then(Value::as_array_mut)
                {
                    for part in parts {
                        if let Some(call) = part.get_mut("functionCall") {
                            self.restore_function(call);
                        }
                    }
                }
            }
        }
    }

    fn restore_response_items(&self, body: &mut Value) {
        if let Some(output) = body.get_mut("output").and_then(Value::as_array_mut) {
            for item in output {
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    self.restore_function(item);
                }
            }
        }
    }
}

/// Rewrite only structured function-name fields, including previous turns and
/// forced tool choices. Never replace text inside descriptions or arguments.
pub(crate) fn adapt_request(body: &mut Value) -> ToolAliases {
    let mut aliases = ToolAliases::default();
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            if let Some(name) = tool.pointer_mut("/function/name") {
                aliases.alias(name);
            }
            if tool.get("type").and_then(Value::as_str) == Some("function") {
                if let Some(name) = tool.get_mut("name") {
                    aliases.alias(name);
                }
            }
            if let Some(declarations) = tool
                .get_mut("functionDeclarations")
                .and_then(Value::as_array_mut)
            {
                for declaration in declarations {
                    if let Some(name) = declaration.get_mut("name") {
                        aliases.alias(name);
                    }
                }
            }
        }
    }
    if let Some(name) = body.pointer_mut("/tool_choice/function/name") {
        aliases.alias(name);
    }
    if let Some(name) = body.pointer_mut("/tool_choice/name") {
        aliases.alias(name);
    }
    if let Some(allowed) = body
        .pointer_mut("/tool_choice/tools")
        .and_then(Value::as_array_mut)
    {
        for tool in allowed {
            if let Some(name) = tool.pointer_mut("/function/name") {
                aliases.alias(name);
            }
            if let Some(name) = tool.get_mut("name") {
                aliases.alias(name);
            }
        }
    }
    if let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) {
        for item in input {
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                if let Some(name) = item.get_mut("name") {
                    aliases.alias(name);
                }
            }
        }
    }
    if let Some(allowed) = body
        .pointer_mut("/toolConfig/functionCallingConfig/allowedFunctionNames")
        .and_then(Value::as_array_mut)
    {
        for name in allowed {
            aliases.alias(name);
        }
    }
    if let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) {
        for content in contents {
            if let Some(parts) = content.get_mut("parts").and_then(Value::as_array_mut) {
                for part in parts {
                    for field in ["functionCall", "functionResponse"] {
                        if let Some(name) =
                            part.get_mut(field).and_then(|call| call.get_mut("name"))
                        {
                            aliases.alias(name);
                        }
                    }
                }
            }
        }
    }
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages.iter_mut() {
            if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
                for call in calls {
                    if let Some(name) = call.pointer_mut("/function/name") {
                        aliases.alias(name);
                    }
                }
            }
            if let Some(name) = message.pointer_mut("/function_call/name") {
                aliases.alias(name);
            }
            if message.get("role").and_then(Value::as_str) == Some("function") {
                if let Some(name) = message.get_mut("name") {
                    aliases.alias(name);
                }
            }
        }
        // Strict Chat backends reject environment system messages after a user.
        // Move system messages to the front, preserving their relative order.
        let mut system = Vec::new();
        let mut conversation = Vec::new();
        for message in std::mem::take(messages) {
            if message.get("role").and_then(Value::as_str) == Some("system") {
                system.push(message);
            } else {
                conversation.push(message);
            }
        }
        // Only combine plain text messages; preserve structured content/metadata.
        if system.len() > 1
            && system.iter().all(|m| {
                m.as_object().is_some_and(|o| o.len() == 2)
                    && m.get("content").is_some_and(Value::is_string)
            })
        {
            let content = system
                .iter()
                .filter_map(|m| m["content"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            system[0]["content"] = Value::String(content);
            system.truncate(1);
        }
        system.extend(conversation);
        *messages = system;
    }
    aliases
}

#[derive(Default)]
struct NameFragments {
    pending: String,
    emitted: bool,
}

fn restore_fragment(function: &mut Value, state: &mut NameFragments, aliases: &ToolAliases) {
    if state.emitted {
        if let Some(object) = function.as_object_mut() {
            object.remove("name");
        }
        return;
    }
    if let Some(fragment) = function.get("name").and_then(Value::as_str) {
        state.pending.push_str(fragment);
    }
    if state.pending.is_empty() {
        return;
    }
    // Read and ReadFile can share a name prefix. Wait for either more name
    // bytes or the arguments field, which marks the end of the name.
    let ambiguous = !function.get("arguments").is_some_and(Value::is_string)
        && aliases
            .reverse
            .keys()
            .any(|alias| alias != &state.pending && alias.starts_with(&state.pending));
    if ambiguous {
        if let Some(object) = function.as_object_mut() {
            object.remove("name");
        }
        return;
    }
    if let Some(original) = aliases.reverse.get(&state.pending) {
        function["name"] = Value::String(original.clone());
        state.emitted = true;
    } else if aliases
        .reverse
        .keys()
        .any(|alias| alias.starts_with(&state.pending))
    {
        // Wait for the rest of a fragmented name before the protocol converter
        // starts a tool block. IDs and argument fragments can still flow through.
        if let Some(object) = function.as_object_mut() {
            object.remove("name");
        }
    } else {
        function["name"] = Value::String(std::mem::take(&mut state.pending));
        state.emitted = true;
    }
}

fn restore_event(
    block: &str,
    aliases: &ToolAliases,
    names: &mut HashMap<(u64, u64), NameFragments>,
) -> String {
    let data = block
        .lines()
        .filter_map(|line| strip_sse_field(line, "data"))
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(mut value) = serde_json::from_str::<Value>(&data) else {
        return format!("{block}\n\n");
    };
    aliases.restore_json(&mut value);
    if let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            let choice_index = choice["index"].as_u64().unwrap_or(0);
            if let Some(delta) = choice.get_mut("delta") {
                if let Some(calls) = delta.get_mut("tool_calls").and_then(Value::as_array_mut) {
                    for call in calls {
                        let tool_index = call["index"].as_u64().unwrap_or(0);
                        if let Some(function) = call.get_mut("function") {
                            restore_fragment(
                                function,
                                names.entry((choice_index, tool_index)).or_default(),
                                aliases,
                            );
                        }
                    }
                }
                if let Some(function) = delta.get_mut("function_call") {
                    restore_fragment(
                        function,
                        names.entry((choice_index, u64::MAX)).or_default(),
                        aliases,
                    );
                }
            }
        }
    }
    let mut result = block
        .lines()
        .filter(|line| strip_sse_field(line, "data").is_none())
        .collect::<Vec<_>>()
        .join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result.push_str("data: ");
    result.push_str(&value.to_string());
    result.push_str("\n\n");
    result
}

pub(crate) async fn restore_response(
    response: ProxyResponse,
    aliases: ToolAliases,
    request_is_streaming: bool,
    timeout: Duration,
) -> Result<ProxyResponse, ProxyError> {
    if aliases.reverse.is_empty() {
        return Ok(response);
    }
    if super::response_processor::is_sse_response(&response, request_is_streaming) {
        let status = response.status();
        let mut headers = response.headers().clone();
        if super::content_encoding::get_content_encoding(&headers)
            .is_some_and(|encoding| encoding != "identity")
        {
            return Err(ProxyError::TransformError(
                "Cannot restore tool names in compressed SSE".into(),
            ));
        }
        strip_entity_headers_for_rebuilt_body(&mut headers);
        let mut upstream = response.bytes_stream();
        let stream = async_stream::try_stream! {
            let mut buffer = String::new();
            let mut remainder = Vec::new();
            let mut names = HashMap::new();
            while let Some(chunk) = upstream.next().await {
                let chunk = chunk?;
                append_utf8_safe(&mut buffer, &mut remainder, &chunk);
                while let Some(block) = take_sse_block(&mut buffer) {
                    if block.len() > 8 * 1024 * 1024 { Err(std::io::Error::other("Tool alias SSE event exceeds 8 MiB"))?; }
                    yield Bytes::from(restore_event(&block, &aliases, &mut names));
                }
                if buffer.len() > 8 * 1024 * 1024 { Err(std::io::Error::other("Tool alias SSE event exceeds 8 MiB"))?; }
            }
            if !buffer.is_empty() { yield Bytes::from(restore_event(&buffer, &aliases, &mut names)); }
            if names.values().any(|state| !state.emitted && !state.pending.is_empty()) {
                Err(std::io::Error::other("Incomplete aliased tool name in upstream stream"))?;
            }
        };
        Ok(ProxyResponse::streamed(status, headers, stream))
    } else {
        let (mut headers, status, bytes) =
            read_decoded_body(response, "ToolAliases", timeout).await?;
        let mut body: Value = serde_json::from_slice(&bytes).map_err(|e| {
            ProxyError::TransformError(format!("Invalid response for tool aliases: {e}"))
        })?;
        aliases.restore_json(&mut body);
        strip_entity_headers_for_rebuilt_body(&mut headers);
        Ok(ProxyResponse::buffered(
            status,
            headers,
            Bytes::from(body.to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_preserves_schema_and_maps_history_choice_and_system_messages() {
        let schema = json!({"type":"object","properties":{"command":{"type":"string","description":"Use Bash"}},"required":["command"]});
        let mut body = json!({
            "tools":[{"type":"function","function":{"name":"Bash","description":"Run Bash","parameters":schema}}],
            "tool_choice":{"type":"function","function":{"name":"Bash"}},
            "messages":[{"role":"system","content":"first"},{"role":"user","content":"Bash ccs_Bash"},{"role":"system","content":"last"},
                {"role":"assistant","tool_calls":[{"id":"call1","function":{"name":"Bash","arguments":"{\"command\":\"echo Bash\"}"}}]},
                {"role":"tool","tool_call_id":"call1","content":"Bash"}]
        });
        let aliases = adapt_request(&mut body);
        assert_eq!(body["tools"][0]["function"]["name"], "ccs_Bash");
        assert_eq!(body["tools"][0]["function"]["parameters"], schema);
        assert_eq!(body["tool_choice"]["function"]["name"], "ccs_Bash");
        assert_eq!(body["messages"][0]["content"], "first\n\nlast");
        assert_eq!(body["messages"][1]["content"], "Bash ccs_Bash");
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["name"],
            "ccs_Bash"
        );
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{\"command\":\"echo Bash\"}"
        );
        let mut response = json!({"choices":[{"message":{"content":"ccs_Bash","tool_calls":[{"id":"call1","function":{"name":"ccs_Bash","arguments":"{}"}}]}}]});
        aliases.restore_json(&mut response);
        assert_eq!(
            response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "Bash"
        );
        assert_eq!(response["choices"][0]["message"]["content"], "ccs_Bash");
    }

    #[test]
    fn long_names_are_stable_and_distinct() {
        let name = "a".repeat(64);
        let mut body = json!({"tools":[{"function":{"name":name}},{"function":{"name":format!("{}b", "a".repeat(63))}}]});
        let mut same = body.clone();
        adapt_request(&mut body);
        adapt_request(&mut same);
        assert_eq!(body, same);
        let a = body["tools"][0]["function"]["name"].as_str().unwrap();
        let b = body["tools"][1]["function"]["name"].as_str().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }

    #[test]
    fn responses_and_gemini_requests_keep_schemas_and_map_history_and_choices() {
        let schema = json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]});
        let mut responses = json!({
            "tools":[{"type":"function","name":"Bash","parameters":schema}],
            "tool_choice":{"type":"function","name":"Bash"},
            "input":[{"type":"function_call","call_id":"c1","name":"Bash","arguments":"{\"command\":\"pwd\"}"},
                     {"type":"function_call_output","call_id":"c1","output":"Bash"}]
        });
        adapt_request(&mut responses);
        assert_eq!(responses["tools"][0]["name"], "ccs_Bash");
        assert_eq!(responses["tools"][0]["parameters"], schema);
        assert_eq!(responses["tool_choice"]["name"], "ccs_Bash");
        assert_eq!(responses["input"][0]["name"], "ccs_Bash");
        assert_eq!(responses["input"][1]["output"], "Bash");
        let mut gemini = json!({
            "tools":[{"functionDeclarations":[{"name":"Bash","parameters":schema}]}],
            "toolConfig":{"functionCallingConfig":{"mode":"ANY","allowedFunctionNames":["Bash"]}},
            "contents":[{"role":"model","parts":[{"functionCall":{"name":"Bash","args":{"command":"pwd"}},"thoughtSignature":"signature"}]},
                        {"role":"user","parts":[{"functionResponse":{"name":"Bash","response":{"result":"Bash"}}}]}]
        });
        adapt_request(&mut gemini);
        assert_eq!(
            gemini["tools"][0]["functionDeclarations"][0]["name"],
            "ccs_Bash"
        );
        assert_eq!(
            gemini["tools"][0]["functionDeclarations"][0]["parameters"],
            schema
        );
        assert_eq!(
            gemini["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            "ccs_Bash"
        );
        assert_eq!(
            gemini["contents"][0]["parts"][0]["functionCall"]["name"],
            "ccs_Bash"
        );
        assert_eq!(
            gemini["contents"][0]["parts"][0]["thoughtSignature"],
            "signature"
        );
        assert_eq!(
            gemini["contents"][1]["parts"][0]["functionResponse"]["name"],
            "ccs_Bash"
        );
        assert_eq!(
            gemini["contents"][1]["parts"][0]["functionResponse"]["response"]["result"],
            "Bash"
        );
    }

    #[tokio::test]
    async fn responses_and_gemini_restore_json_and_stream_events() {
        let cases = vec![
            (
                json!({"output":[{"type":"function_call","name":"ccs_Bash","call_id":"c1","arguments":"{}"}]}),
                "/output/0/name",
            ),
            (
                json!({"type":"response.output_item.added","item":{"type":"function_call","name":"ccs_Bash","arguments":""}}),
                "/item/name",
            ),
            (
                json!({"type":"response.completed","response":{"output":[{"type":"function_call","name":"ccs_Bash","arguments":"{}"}]}}),
                "/response/output/0/name",
            ),
            (
                json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"ccs_Bash","args":{"command":"pwd"}},"thoughtSignature":"sig"}]}}],"usageMetadata":{"totalTokenCount":5}}),
                "/candidates/0/content/parts/0/functionCall/name",
            ),
        ];
        for (body, pointer) in cases {
            for streaming in [false, true] {
                let aliases =
                    adapt_request(&mut json!({"tools":[{"type":"function","name":"Bash"}]}));
                let mut expected = body.clone();
                *expected.pointer_mut(pointer).unwrap() = json!("Bash");
                let mut headers = http::HeaderMap::new();
                headers.insert(
                    "content-type",
                    http::HeaderValue::from_static(if streaming {
                        "text/event-stream"
                    } else {
                        "application/json"
                    }),
                );
                let bytes = if streaming {
                    format!("event: test\ndata: {body}\n\n")
                } else {
                    body.to_string()
                };
                let response =
                    ProxyResponse::buffered(http::StatusCode::OK, headers, Bytes::from(bytes));
                let restored = restore_response(response, aliases, streaming, Duration::ZERO)
                    .await
                    .unwrap()
                    .bytes_with_limit(8192)
                    .await
                    .unwrap();
                let restored = String::from_utf8(restored.to_vec()).unwrap();
                let value: Value = if streaming {
                    assert!(restored.contains("event: test"));
                    serde_json::from_str(
                        restored
                            .lines()
                            .find_map(|line| strip_sse_field(line, "data"))
                            .unwrap(),
                    )
                    .unwrap()
                } else {
                    serde_json::from_str(&restored).unwrap()
                };
                assert_eq!(value, expected);
            }
        }
    }

    #[test]
    fn split_names_do_not_select_a_shorter_tool_with_the_same_prefix() {
        let mut request =
            json!({"tools":[{"function":{"name":"Read"}},{"function":{"name":"ReadFile"}}]});
        let aliases = adapt_request(&mut request);
        let mut state = NameFragments::default();
        let mut first = json!({"name":"ccs_Read"});
        restore_fragment(&mut first, &mut state, &aliases);
        assert!(first.get("name").is_none());
        let mut second = json!({"name":"File","arguments":"{}"});
        restore_fragment(&mut second, &mut state, &aliases);
        assert_eq!(second["name"], "ReadFile");
        let mut state = NameFragments::default();
        restore_fragment(&mut json!({"name":"ccs_Read"}), &mut state, &aliases);
        let mut arguments = json!({"arguments":"{}"});
        restore_fragment(&mut arguments, &mut state, &aliases);
        assert_eq!(arguments["name"], "Read");
    }

    #[tokio::test]
    async fn streaming_restores_split_names_and_utf8_across_byte_chunks() {
        let mut request = json!({"tools":[{"function":{"name":"Bash"}}]});
        let aliases = adapt_request(&mut request);
        let text = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"ccs_\"}}]}}]}\r\n\r\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"Bash\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"中文\"}}]}\n\ndata: [DONE]\n\n"
        );
        let chunks: Vec<_> = text
            .as_bytes()
            .chunks(7)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "content-type",
            http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert("content-length", http::HeaderValue::from_static("123"));
        let response =
            ProxyResponse::streamed(http::StatusCode::OK, headers, futures::stream::iter(chunks));
        let restored = restore_response(response, aliases, true, Duration::ZERO)
            .await
            .unwrap();
        assert!(!restored.headers().contains_key("content-length"));
        let output =
            String::from_utf8(restored.bytes_with_limit(8192).await.unwrap().to_vec()).unwrap();
        let events: Vec<_> = output
            .split("\n\n")
            .filter_map(|block| block.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect();
        assert!(
            events[0]["choices"][0]["delta"]["tool_calls"][0]["function"]
                .get("name")
                .is_none()
        );
        assert_eq!(
            events[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"],
            "Bash"
        );
        assert_eq!(
            events[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
            "{}"
        );
        assert!(output.contains("中文"));
        assert!(output.contains("data: [DONE]"));
    }
}
