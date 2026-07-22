//! Codex `namespace` tool flattening for native Responses upstreams.
//!
//! Codex 0.142+ declares its plugin/MCP tools with a private Responses
//! extension shape — `{"type":"namespace","name":"mcp__x__","tools":[…]}` plus
//! `tool_search` — that the OpenAI ChatGPT backend understands but strict
//! third-party gateways (e.g. xAI's `api.x.ai/v1/responses`) reject with
//! `422 unknown variant "namespace"`. cc-switch's Chat/Anthropic transforms
//! already unwrap these, but the *native* Responses passthrough sends them
//! verbatim.
//!
//! This module implements the request-side flatten + response-side restore for
//! that native path, mirroring the proven design of sub2api
//! (`pkg/apicompat/responses_namespace.go`):
//!
//! - **Request**: lift every `namespace` child into a top-level `function` tool
//!   whose name is the deterministic flat name `<namespace>__<child>` (with the
//!   same sha256 truncation used by the Chat path, so both layers agree), then
//!   rewrite namespace-qualified `function_call` items in the replayed `input`
//!   history to the flat name and drop a `namespace`-typed `tool_choice`.
//! - **Response**: restore the flat `function_call` names back to
//!   `{name, namespace}` so the Codex client can match the call against its own
//!   namespaced tool registry (streaming and non-streaming).
//!
//! Flatten and restore both derive their name map from the *same* request tools
//! via [`flatten_namespace_tool_name`], so the forwarder (flatten) and the
//! response handler (restore) stay consistent without threading state between
//! them.

use std::collections::HashMap;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use super::transform_codex_chat::flatten_namespace_tool_name;
use crate::proxy::error::ProxyError;
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

/// Reverse map entry: a flattened tool name resolves back to its original
/// namespace and bare child name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamespacedName {
    pub namespace: String,
    pub name: String,
}

/// Build the flat-name → `{namespace, name}` restore map from a Codex Responses
/// request body. Used by the response handler to invert the request-side
/// flatten; derives names exactly as [`flatten_request_namespaces`] does.
pub(crate) fn namespace_restore_map(request_body: &Value) -> HashMap<String, NamespacedName> {
    let mut map = HashMap::new();
    let Some(tools) = request_body.get("tools").and_then(Value::as_array) else {
        return map;
    };
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("namespace") {
            continue;
        }
        let Some(namespace) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let namespace = namespace.trim();
        if namespace.is_empty() {
            continue;
        }
        for child in namespace_children(tool) {
            if child.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let Some(name) = child.get("name").and_then(Value::as_str) else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let flat = flatten_namespace_tool_name(namespace, name);
            map.entry(flat).or_insert_with(|| NamespacedName {
                namespace: namespace.to_string(),
                name: name.to_string(),
            });
        }
    }
    map
}

/// Flatten Codex `namespace` tool declarations in a native Responses request
/// body into top-level `function` tools, rewrite namespace-qualified calls in
/// the `input` history, and neutralize a `namespace` `tool_choice`.
///
/// Returns `Ok(true)` when the body was rewritten. Returns a `TransformError`
/// when two distinct namespace children (or a child and a top-level tool)
/// collapse to the same flat name — the upstream could not disambiguate them,
/// so failing loudly beats silently dropping a tool (matches sub2api).
pub(crate) fn flatten_request_namespaces(body: &mut Value) -> Result<bool, ProxyError> {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return Ok(false);
    };
    if !tools
        .iter()
        .any(|tool| tool.get("type").and_then(Value::as_str) == Some("namespace"))
    {
        return Ok(false);
    }

    // Names already occupied by top-level function/custom tools; a namespace
    // child flattening onto one of these is an unrecoverable collision.
    let mut top_level = std::collections::HashSet::new();
    for tool in tools {
        let typ = tool.get("type").and_then(Value::as_str).unwrap_or("");
        if typ == "function" || typ == "custom" {
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                let name = name.trim();
                if !name.is_empty() {
                    top_level.insert(name.to_string());
                }
            }
        }
    }

    // Validate flat-name uniqueness before mutating anything.
    let mut owners: HashMap<String, NamespacedName> = HashMap::new();
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("namespace") {
            continue;
        }
        let Some(namespace) = tool.get("name").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if namespace.is_empty() {
            continue;
        }
        for child in namespace_children(tool) {
            if child.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let Some(name) = child.get("name").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let flat = flatten_namespace_tool_name(namespace, name);
            if top_level.contains(&flat) {
                return Err(ProxyError::TransformError(format!(
                    "namespace tool {namespace:?}/{name:?} flattens to {flat:?} which \
                     collides with a top-level tool of the same name; rename one of them"
                )));
            }
            let entry = NamespacedName {
                namespace: namespace.to_string(),
                name: name.to_string(),
            };
            if let Some(prev) = owners.get(&flat) {
                if *prev != entry {
                    return Err(ProxyError::TransformError(format!(
                        "namespace tools {:?}/{:?} and {namespace:?}/{name:?} both flatten to \
                         {flat:?}; rename one of them",
                        prev.namespace, prev.name
                    )));
                }
            } else {
                owners.insert(flat, entry);
            }
        }
    }

    // Rebuild the tools array with namespace children lifted to top level.
    let tools = body
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut flattened: Vec<Value> = Vec::with_capacity(tools.len());
    let mut seen_flat = std::collections::HashSet::new();
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("namespace") {
            flattened.push(tool);
            continue;
        }
        let Some(namespace) = tool.get("name").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        for child in namespace_children(&tool) {
            if child.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let Some(name) = child.get("name").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let flat = flatten_namespace_tool_name(namespace, name);
            if !seen_flat.insert(flat.clone()) {
                continue;
            }
            let mut lifted = child.clone();
            if let Some(obj) = lifted.as_object_mut() {
                obj.insert("name".to_string(), json!(flat));
            }
            flattened.push(lifted);
        }
    }
    body["tools"] = json!(flattened);

    // Rewrite namespace-qualified function_call items in the replayed history.
    if let Some(input) = body.get_mut("input") {
        rewrite_namespace_qualified_calls(input, &owners);
    }

    // A namespace-typed tool_choice cannot survive flattening: degrade to auto.
    if let Some(choice) = body.get_mut("tool_choice") {
        if choice.get("type").and_then(Value::as_str) == Some("namespace") {
            *choice = json!("auto");
        } else {
            rewrite_namespace_qualified_call(choice, &owners);
        }
    }

    Ok(true)
}

/// Restore flattened `function_call` names in a full (non-streaming) Responses
/// payload back to their `{name, namespace}` identity. Returns whether anything
/// changed.
pub(crate) fn restore_response_namespaces(
    value: &mut Value,
    map: &HashMap<String, NamespacedName>,
) -> bool {
    if map.is_empty() {
        return false;
    }
    restore_value(value, map)
}

/// Restore a single parsed SSE event (e.g. `response.output_item.added` /
/// `.done` carrying a `function_call`). Returns whether anything changed.
pub(crate) fn restore_sse_event_namespaces(
    event: &mut Value,
    map: &HashMap<String, NamespacedName>,
) -> bool {
    if map.is_empty() {
        return false;
    }
    restore_value(event, map)
}

fn namespace_children(tool: &Value) -> Vec<Value> {
    tool.get("tools")
        .or_else(|| tool.get("children"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn rewrite_namespace_qualified_calls(value: &mut Value, owners: &HashMap<String, NamespacedName>) {
    match value {
        Value::Array(items) => {
            for item in items {
                rewrite_namespace_qualified_calls(item, owners);
            }
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(Value::as_str) == Some("function_call") {
                rewrite_namespace_qualified_call(value, owners);
                return;
            }
            for child in obj.values_mut() {
                rewrite_namespace_qualified_calls(child, owners);
            }
        }
        _ => {}
    }
}

fn rewrite_namespace_qualified_call(
    item: &mut Value,
    owners: &HashMap<String, NamespacedName>,
) -> bool {
    let Some(obj) = item.as_object_mut() else {
        return false;
    };
    let namespace = obj
        .get("namespace")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if namespace.is_empty() || name.is_empty() {
        return false;
    }
    let flat = flatten_namespace_tool_name(&namespace, &name);
    match owners.get(&flat) {
        Some(entry) if entry.namespace == namespace && entry.name == name => {
            obj.insert("name".to_string(), json!(flat));
            obj.remove("namespace");
            true
        }
        _ => false,
    }
}

fn restore_value(value: &mut Value, map: &HashMap<String, NamespacedName>) -> bool {
    let mut changed = false;
    match value {
        Value::Array(items) => {
            for item in items {
                changed |= restore_value(item, map);
            }
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(Value::as_str) == Some("function_call") {
                if let Some(flat) = obj.get("name").and_then(Value::as_str) {
                    if let Some(entry) = map.get(flat) {
                        obj.insert("name".to_string(), json!(entry.name));
                        obj.insert("namespace".to_string(), json!(entry.namespace));
                        changed = true;
                    }
                }
            }
            for child in obj.values_mut() {
                changed |= restore_value(child, map);
            }
        }
        _ => {}
    }
    changed
}

/// Wrap a native Responses SSE byte stream, restoring flattened `function_call`
/// names in each event back to their namespace identity. Events that carry no
/// affected function call pass through with their inner content preserved
/// verbatim (only the block delimiter is normalized to `\n\n`).
pub(crate) fn create_namespace_restore_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    map: HashMap<String, NamespacedName>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        yield Ok(restore_sse_block(&block, &map));
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    return;
                }
            }
        }

        // Flush any trailing partial block (streams normally end on a delimiter,
        // but be defensive so no bytes are dropped).
        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            yield Ok(restore_sse_block(&tail, &map));
        }
    }
}

/// Restore one SSE block. When the block's `data:` JSON carries an affected
/// function call, re-serialize just that line; otherwise the original block text
/// is preserved and only the `\n\n` delimiter re-appended.
fn restore_sse_block(block: &str, map: &HashMap<String, NamespacedName>) -> Bytes {
    let mut event_name: Option<&str> = None;
    let mut data_parts: Vec<&str> = Vec::new();
    for line in block.lines() {
        if let Some(event) = strip_sse_field(line, "event") {
            event_name = Some(event.trim());
        }
        if let Some(data) = strip_sse_field(line, "data") {
            data_parts.push(data);
        }
    }

    if data_parts.is_empty() {
        return Bytes::from(format!("{block}\n\n"));
    }

    let data = data_parts.join("\n");
    if data.trim() == "[DONE]" {
        return Bytes::from(format!("{block}\n\n"));
    }

    let mut event: Value = match serde_json::from_str(&data) {
        Ok(value) => value,
        // Non-JSON data (shouldn't happen on the Responses wire): pass through.
        Err(_) => return Bytes::from(format!("{block}\n\n")),
    };

    if !restore_sse_event_namespaces(&mut event, map) {
        return Bytes::from(format!("{block}\n\n"));
    }

    let restored = serde_json::to_string(&event).unwrap_or(data);
    let mut out = String::new();
    if let Some(name) = event_name {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(&restored);
    out.push_str("\n\n");
    Bytes::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    fn namespace_request() -> Value {
        json!({
            "model": "grok-4.5",
            "tools": [
                { "type": "function", "name": "plain_tool", "parameters": {} },
                {
                    "type": "namespace",
                    "name": "mcp__files__",
                    "tools": [
                        { "type": "function", "name": "read", "description": "read a file", "parameters": {} },
                        { "type": "function", "name": "write", "parameters": {} }
                    ]
                }
            ],
            "input": [
                {
                    "type": "function_call",
                    "name": "read",
                    "namespace": "mcp__files__",
                    "call_id": "c1",
                    "arguments": "{}"
                }
            ],
            "tool_choice": { "type": "namespace", "name": "mcp__files__" }
        })
    }

    #[test]
    fn flatten_lifts_namespace_children_to_top_level_functions() {
        let mut body = namespace_request();
        assert!(flatten_request_namespaces(&mut body).unwrap());

        let tools = body["tools"].as_array().unwrap();
        // plain + read + write, all top-level function tools; no namespace left.
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().all(|t| t["type"] == "function"));
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"plain_tool"));
        assert!(names.contains(&"mcp__files____read"));
        assert!(names.contains(&"mcp__files____write"));
        // Child metadata is preserved on the lifted tool.
        let read = tools
            .iter()
            .find(|t| t["name"] == "mcp__files____read")
            .unwrap();
        assert_eq!(read["description"], "read a file");
    }

    #[test]
    fn flatten_rewrites_input_history_calls_and_tool_choice() {
        let mut body = namespace_request();
        flatten_request_namespaces(&mut body).unwrap();

        let call = &body["input"][0];
        assert_eq!(call["name"], "mcp__files____read");
        assert!(call.get("namespace").is_none());
        assert_eq!(call["call_id"], "c1");
        // A namespace-typed tool_choice degrades to "auto".
        assert_eq!(body["tool_choice"], json!("auto"));
    }

    #[test]
    fn flatten_is_noop_without_namespace_tools() {
        let mut body = json!({
            "tools": [ { "type": "function", "name": "plain", "parameters": {} } ]
        });
        assert!(!flatten_request_namespaces(&mut body).unwrap());
    }

    #[test]
    fn flatten_errors_on_flat_name_collision_with_top_level() {
        let mut body = json!({
            "tools": [
                { "type": "function", "name": "mcp__files____read", "parameters": {} },
                {
                    "type": "namespace",
                    "name": "mcp__files__",
                    "tools": [ { "type": "function", "name": "read", "parameters": {} } ]
                }
            ]
        });
        assert!(flatten_request_namespaces(&mut body).is_err());
    }

    #[test]
    fn restore_map_inverts_flatten_naming() {
        let body = namespace_request();
        let map = namespace_restore_map(&body);
        let entry = map.get("mcp__files____read").unwrap();
        assert_eq!(entry.namespace, "mcp__files__");
        assert_eq!(entry.name, "read");
        // Plain top-level tools are not in the restore map.
        assert!(!map.contains_key("plain_tool"));
    }

    #[test]
    fn round_trip_flatten_then_restore_recovers_namespace() {
        let request = namespace_request();
        let map = namespace_restore_map(&request);

        // Upstream returns a function_call using the flattened name.
        let mut response = json!({
            "type": "response",
            "output": [
                {
                    "type": "function_call",
                    "name": "mcp__files____read",
                    "call_id": "c1",
                    "arguments": "{}"
                }
            ]
        });
        assert!(restore_response_namespaces(&mut response, &map));
        let call = &response["output"][0];
        assert_eq!(call["name"], "read");
        assert_eq!(call["namespace"], "mcp__files__");
    }

    #[test]
    fn restore_leaves_unmapped_calls_untouched() {
        let map = namespace_restore_map(&namespace_request());
        let mut response = json!({
            "output": [
                { "type": "function_call", "name": "plain_tool", "call_id": "x" }
            ]
        });
        assert!(!restore_response_namespaces(&mut response, &map));
        assert_eq!(response["output"][0]["name"], "plain_tool");
        assert!(response["output"][0].get("namespace").is_none());
    }

    #[test]
    fn long_flat_names_stay_consistent_between_flatten_and_restore() {
        let long_child = "a".repeat(80);
        let body = json!({
            "tools": [{
                "type": "namespace",
                "name": "mcp__srv__",
                "tools": [ { "type": "function", "name": long_child, "parameters": {} } ]
            }]
        });
        let mut flattened = body.clone();
        flatten_request_namespaces(&mut flattened).unwrap();
        let flat_name = flattened["tools"][0]["name"].as_str().unwrap().to_string();
        // Truncation kicks in past the 64-char chat tool-name limit.
        assert!(flat_name.len() <= 64);

        let map = namespace_restore_map(&body);
        // The truncated name from flatten must be a restore-map key.
        let entry = map.get(&flat_name).unwrap();
        assert_eq!(entry.namespace, "mcp__srv__");
        assert_eq!(entry.name, long_child);
    }

    #[tokio::test]
    async fn sse_stream_restores_function_call_events_and_passes_others_through() {
        let map = namespace_restore_map(&namespace_request());

        let added = "event: response.output_item.added\n\
                     data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"name\":\"mcp__files____read\",\"call_id\":\"c1\"}}\n\n";
        let delta = "event: response.output_text.delta\n\
                     data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n";
        let done = "data: [DONE]\n\n";

        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(added)),
            Ok(Bytes::from(delta)),
            Ok(Bytes::from(done)),
        ];
        let input = stream::iter(chunks);
        let out = create_namespace_restore_sse_stream(input, map);
        futures::pin_mut!(out);

        let mut collected = String::new();
        while let Some(chunk) = out.next().await {
            collected.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        }

        // function_call name restored to namespace form.
        assert!(collected.contains("\"name\":\"read\""));
        assert!(collected.contains("\"namespace\":\"mcp__files__\""));
        assert!(!collected.contains("mcp__files____read"));
        // Unrelated events preserved verbatim.
        assert!(collected.contains("\"delta\":\"hi\""));
        assert!(collected.contains("[DONE]"));
    }
}
