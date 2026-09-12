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
//!   `{name, namespace}` and convert the synthetic `function_call` named
//!   `tool_search` into the native client-executed `tool_search_call` expected
//!   by Codex (streaming and non-streaming).
//!
//! Flatten and restore both derive their name map from the *same* request tools
//! via [`flatten_namespace_tool_name`], so the forwarder (flatten) and the
//! response handler (restore) stay consistent without threading state between
//! them.

use std::collections::HashMap;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use super::transform_codex_chat::{flatten_namespace_tool_name, tool_search_arguments_from_value};
use crate::proxy::error::ProxyError;
use crate::proxy::json_canonical::canonical_json_string;
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
    for tool in request_declared_tools(request_body) {
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
        for child in namespace_children(&tool) {
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

fn request_declared_tools(request_body: &Value) -> Vec<Value> {
    let mut tools = request_body
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(input) = request_body.get("input") {
        collect_tool_search_output_tools(input, &mut tools);
    }
    tools
}

fn collect_tool_search_output_tools(value: &Value, tools: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_search_output_tools(item, tools);
            }
        }
        Value::Object(obj)
            if obj.get("type").and_then(Value::as_str) == Some("tool_search_output") =>
        {
            if let Some(discovered) = obj.get("tools").and_then(Value::as_array) {
                tools.extend(discovered.iter().cloned());
            }
        }
        Value::Object(_) => {}
        _ => {}
    }
}

fn normalize_tool_search_history(body: &mut Value) -> Result<bool, ProxyError> {
    let mut discovered = Vec::new();
    let mut changed = false;
    if let Some(input) = body.get_mut("input") {
        changed |= rewrite_tool_search_history_items(input, &mut discovered)?;
    }
    if discovered.is_empty() {
        return Ok(changed);
    }

    let Some(obj) = body.as_object_mut() else {
        return Err(ProxyError::TransformError(
            "native Responses request body must be an object".to_string(),
        ));
    };
    let tools = obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if tools.is_null() {
        *tools = Value::Array(Vec::new());
    }
    let Some(tools) = tools.as_array_mut() else {
        return Err(ProxyError::TransformError(
            "native Responses tools must be an array".to_string(),
        ));
    };

    let mut seen = std::collections::HashSet::new();
    for tool in tools.iter() {
        if let Some(identity) = response_tool_identity(tool) {
            seen.insert(identity);
        }
    }
    for mut tool in discovered {
        normalize_response_function_tool(&mut tool);
        if tool.get("type").and_then(Value::as_str) == Some("namespace") {
            if let Some(namespace) = tool.get("name").and_then(Value::as_str) {
                if let Some(existing) = tools.iter_mut().find(|candidate| {
                    candidate.get("type").and_then(Value::as_str) == Some("namespace")
                        && candidate.get("name").and_then(Value::as_str) == Some(namespace)
                }) {
                    changed |= merge_namespace_children(existing, &tool);
                    continue;
                }
            }
        }
        if let Some(identity) = response_tool_identity(&tool) {
            if !seen.insert(identity) {
                continue;
            }
        }
        tools.push(tool);
        changed = true;
    }

    // Codex may send `tool_choice: "none"` on the client follow-up because the
    // deferred definitions were absent from its original request tool surface.
    // Once a completed tool_search_output has supplied concrete callable tools,
    // keeping that stale choice would make the promoted definitions impossible
    // to invoke. Scope the override to this discovered-tool replay only; explicit
    // no-tool requests without tool_search_output remain untouched.
    if obj.get("tool_choice").and_then(Value::as_str) == Some("none") {
        obj.insert("tool_choice".to_string(), json!("auto"));
        changed = true;
    }

    Ok(changed)
}

fn rewrite_tool_search_history_items(
    value: &mut Value,
    discovered: &mut Vec<Value>,
) -> Result<bool, ProxyError> {
    match value {
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= rewrite_tool_search_history_items(item, discovered)?;
            }
            Ok(changed)
        }
        Value::Object(obj) => match obj.get("type").and_then(Value::as_str) {
            Some("tool_search_call") => {
                let call_id = obj
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ProxyError::TransformError(
                            "tool_search_call is missing call_id".to_string(),
                        )
                    })?
                    .to_string();
                let arguments = match obj.get("arguments") {
                    Some(Value::String(value)) => value.clone(),
                    Some(value) => canonical_json_string(value),
                    None => "{}".to_string(),
                };
                *value = json!({
                    "type": "function_call",
                    "name": "tool_search",
                    "call_id": call_id,
                    "arguments": arguments
                });
                Ok(true)
            }
            Some("tool_search_output") => {
                let call_id = obj
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ProxyError::TransformError(
                            "tool_search_output is missing call_id".to_string(),
                        )
                    })?
                    .to_string();
                if let Some(tools) = obj.get("tools").and_then(Value::as_array) {
                    discovered.extend(tools.iter().cloned());
                }
                let output = canonical_json_string(value);
                *value = json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                });
                Ok(true)
            }
            _ => Ok(false),
        },
        _ => Ok(false),
    }
}

pub(crate) fn normalize_response_function_tool(tool: &mut Value) {
    let Some(obj) = tool.as_object_mut() else {
        return;
    };
    if obj.get("type").and_then(Value::as_str) == Some("namespace") {
        for key in ["tools", "children"] {
            if let Some(children) = obj.get_mut(key).and_then(Value::as_array_mut) {
                for child in children {
                    normalize_response_function_tool(child);
                }
            }
        }
        return;
    }
    if obj.get("type").and_then(Value::as_str) != Some("function") {
        return;
    }
    if !obj.contains_key("parameters") {
        if let Some(schema) = obj
            .remove("inputSchema")
            .or_else(|| obj.remove("input_schema"))
        {
            obj.insert("parameters".to_string(), schema);
        }
    } else {
        obj.remove("inputSchema");
        obj.remove("input_schema");
    }
    obj.remove("deferLoading");
    obj.remove("defer_loading");
}

fn merge_namespace_children(existing: &mut Value, discovered: &Value) -> bool {
    let mut discovered_children = namespace_children(discovered);
    if discovered_children.is_empty() {
        return false;
    }
    for child in &mut discovered_children {
        normalize_response_function_tool(child);
    }

    let Some(existing_obj) = existing.as_object_mut() else {
        return false;
    };
    let child_key = if existing_obj
        .get("tools")
        .and_then(Value::as_array)
        .is_some()
    {
        "tools"
    } else if existing_obj
        .get("children")
        .and_then(Value::as_array)
        .is_some()
    {
        "children"
    } else {
        existing_obj.insert("tools".to_string(), Value::Array(Vec::new()));
        "tools"
    };
    let Some(existing_children) = existing_obj
        .get_mut(child_key)
        .and_then(Value::as_array_mut)
    else {
        return false;
    };

    let mut seen = std::collections::HashSet::new();
    for child in existing_children.iter() {
        if let Some(identity) = response_tool_identity(child) {
            seen.insert(identity);
        }
    }

    let mut changed = false;
    for child in discovered_children {
        if let Some(identity) = response_tool_identity(&child) {
            if !seen.insert(identity) {
                continue;
            }
        } else if existing_children.contains(&child) {
            continue;
        }
        existing_children.push(child);
        changed = true;
    }
    changed
}

fn response_tool_identity(tool: &Value) -> Option<String> {
    let kind = tool.get("type").and_then(Value::as_str)?;
    let name = tool
        .get("name")
        .or_else(|| tool.get("function").and_then(|value| value.get("name")))
        .and_then(Value::as_str)?;
    Some(format!("{kind}:{name}"))
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
    let changed = normalize_tool_search_history(body)?;
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return Ok(changed);
    };
    if !tools
        .iter()
        .any(|tool| tool.get("type").and_then(Value::as_str) == Some("namespace"))
    {
        return Ok(changed);
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
    for mut tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("namespace") {
            normalize_response_function_tool(&mut tool);
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
            normalize_response_function_tool(&mut lifted);
            flattened.push(lifted);
        }
    }
    body["tools"] = json!(flattened);

    // Rewrite namespace-qualified function_call items in the replayed history.
    if let Some(input) = body.get_mut("input") {
        rewrite_namespace_qualified_input_items(input, &owners);
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

/// Restore native Responses function-call identities. Namespace restoration
/// uses the request-derived map; tool-search restoration is enabled only on the
/// provider-gated third-party compatibility path.
pub(crate) fn restore_response_tool_calls(
    value: &mut Value,
    map: &HashMap<String, NamespacedName>,
    restore_tool_search: bool,
) -> bool {
    if map.is_empty() && !restore_tool_search {
        return false;
    }
    restore_response_output_items(value, map, restore_tool_search, &mut HashMap::new())
}

/// Restore a single parsed SSE event (e.g. `response.output_item.added` /
/// `.done` carrying a `function_call`). Returns whether anything changed.
pub(crate) fn restore_sse_event_tool_calls(
    event: &mut Value,
    map: &HashMap<String, NamespacedName>,
    restore_tool_search: bool,
    tool_search_item_ids: &mut HashMap<String, String>,
) -> bool {
    if map.is_empty() && !restore_tool_search {
        return false;
    }
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
    let mut changed = false;

    if matches!(
        event_type,
        "response.output_item.added" | "response.output_item.done"
    ) {
        if let Some(item) = event.get_mut("item") {
            changed |= restore_output_item(item, map, restore_tool_search, tool_search_item_ids);
        }
    } else if matches!(
        event_type,
        "response.created"
            | "response.queued"
            | "response.in_progress"
            | "response.completed"
            | "response.incomplete"
            | "response.failed"
    ) {
        if let Some(response) = event.get_mut("response") {
            changed |= restore_response_output_items(
                response,
                map,
                restore_tool_search,
                tool_search_item_ids,
            );
        }
    }

    let restored_item_id = event
        .get("item_id")
        .and_then(Value::as_str)
        .and_then(|item_id| tool_search_item_ids.get(item_id))
        .cloned();
    if let Some(restored_item_id) = restored_item_id {
        event["item_id"] = json!(restored_item_id);
        changed = true;
    }

    changed
}

fn namespace_children(tool: &Value) -> Vec<Value> {
    tool.get("tools")
        .or_else(|| tool.get("children"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn rewrite_namespace_qualified_input_items(
    input: &mut Value,
    owners: &HashMap<String, NamespacedName>,
) {
    let Some(items) = input.as_array_mut() else {
        return;
    };
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            rewrite_namespace_qualified_call(item, owners);
        }
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

fn typed_tool_search_item_id(obj: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(item_id) = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|item_id| item_id.starts_with("tsc_") && item_id.len() > 4)
    {
        return Some(item_id.to_string());
    }

    obj.get("call_id")
        .and_then(Value::as_str)
        .filter(|call_id| !call_id.is_empty())
        .map(|call_id| format!("tsc_{call_id}"))
}

fn restore_response_output_items(
    response: &mut Value,
    map: &HashMap<String, NamespacedName>,
    restore_tool_search: bool,
    tool_search_item_ids: &mut HashMap<String, String>,
) -> bool {
    let Some(output) = response.get_mut("output").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for item in output {
        changed |= restore_output_item(item, map, restore_tool_search, tool_search_item_ids);
    }
    changed
}

fn restore_output_item(
    item: &mut Value,
    map: &HashMap<String, NamespacedName>,
    restore_tool_search: bool,
    tool_search_item_ids: &mut HashMap<String, String>,
) -> bool {
    let Some(obj) = item.as_object_mut() else {
        return false;
    };
    if obj.get("type").and_then(Value::as_str) != Some("function_call") {
        return false;
    }

    let is_tool_search = restore_tool_search
        && obj.get("namespace").is_none()
        && obj.get("name").and_then(Value::as_str) == Some("tool_search");
    if is_tool_search {
        let previous_item_id = obj.get("id").and_then(Value::as_str).map(str::to_string);
        let item_id = typed_tool_search_item_id(obj);
        let arguments = tool_search_arguments_from_value(obj.get("arguments"));
        obj.insert("type".to_string(), json!("tool_search_call"));
        obj.insert("execution".to_string(), json!("client"));
        obj.insert("arguments".to_string(), arguments);
        obj.remove("name");
        if let Some(item_id) = item_id {
            if previous_item_id.as_deref() != Some(item_id.as_str()) {
                if let Some(previous_item_id) = previous_item_id {
                    tool_search_item_ids.insert(previous_item_id, item_id.clone());
                }
                obj.insert("id".to_string(), json!(item_id));
            }
        }
        true
    } else if let Some(flat) = obj.get("name").and_then(Value::as_str) {
        if let Some(entry) = map.get(flat) {
            obj.insert("name".to_string(), json!(entry.name));
            obj.insert("namespace".to_string(), json!(entry.namespace));
            true
        } else {
            false
        }
    } else {
        false
    }
}

/// Wrap a native Responses SSE byte stream, restoring flattened namespace
/// calls and synthetic tool-search calls. Unaffected events pass through with
/// their inner content preserved verbatim.
pub(crate) fn create_tool_call_restore_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    map: HashMap<String, NamespacedName>,
    restore_tool_search: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut tool_search_item_ids = HashMap::new();

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        yield Ok(restore_sse_block(
                            &block,
                            &map,
                            restore_tool_search,
                            &mut tool_search_item_ids,
                        ));
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
            yield Ok(restore_sse_block(
                &tail,
                &map,
                restore_tool_search,
                &mut tool_search_item_ids,
            ));
        }
    }
}

/// Restore one SSE block. When the block's `data:` JSON carries an affected
/// function call, re-serialize just that line; otherwise the original block text
/// is preserved and only the `\n\n` delimiter re-appended.
fn restore_sse_block(
    block: &str,
    map: &HashMap<String, NamespacedName>,
    restore_tool_search: bool,
    tool_search_item_ids: &mut HashMap<String, String>,
) -> Bytes {
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

    if !restore_sse_event_tool_calls(&mut event, map, restore_tool_search, tool_search_item_ids) {
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
    fn native_followup_rewrites_tool_search_output_as_function_output() {
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "tools": [{
                "type": "function",
                "name": "tool_search",
                "parameters": {"type": "object"}
            }],
            "input": [
                {
                    "type": "tool_search_call",
                    "call_id": "search-1",
                    "execution": "client",
                    "status": "completed",
                    "arguments": {"query": "automation_update", "limit": 8}
                },
                {
                    "type": "tool_search_output",
                    "call_id": "search-1",
                    "execution": "client",
                    "status": "completed",
                    "tools": []
                }
            ]
        });

        assert!(flatten_request_namespaces(&mut body).unwrap());
        let call = &body["input"][0];
        assert_eq!(call["type"], "function_call");
        assert_eq!(call["name"], "tool_search");
        assert_eq!(call["call_id"], "search-1");
        let arguments: Value = serde_json::from_str(call["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["query"], "automation_update");
        assert_eq!(arguments["limit"], 8);

        let output = &body["input"][1];
        assert_eq!(output["type"], "function_call_output");
        assert_eq!(output["call_id"], "search-1");
        let payload: Value = serde_json::from_str(output["output"].as_str().unwrap()).unwrap();
        assert_eq!(payload["type"], "tool_search_output");
        assert_eq!(payload["tools"], json!([]));
    }

    #[test]
    fn native_followup_promotes_discovered_namespace_tools_and_restores_identity() {
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "tools": [{
                "type": "function",
                "name": "tool_search",
                "parameters": {"type": "object"}
            }],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search-1",
                "execution": "client",
                "status": "completed",
                "tools": [{
                    "type": "namespace",
                    "name": "codex_app",
                    "tools": [{
                        "type": "function",
                        "name": "automation_update",
                        "description": "Manage an automation",
                        "inputSchema": {"type": "object"},
                        "deferLoading": true
                    }]
                }]
            }]
        });

        let restore_map = namespace_restore_map(&body);
        let entry = restore_map.get("codex_app__automation_update").unwrap();
        assert_eq!(entry.namespace, "codex_app");
        assert_eq!(entry.name, "automation_update");

        assert!(flatten_request_namespaces(&mut body).unwrap());
        let names: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"tool_search"));
        assert!(names.contains(&"codex_app__automation_update"));
        let automation = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "codex_app__automation_update")
            .unwrap();
        assert_eq!(automation["parameters"], json!({"type": "object"}));
        assert!(automation.get("inputSchema").is_none());
        assert!(automation.get("deferLoading").is_none());
        assert_eq!(body["input"][0]["type"], "function_call_output");

        let mut response = json!({
            "output": [{
                "type": "function_call",
                "name": "codex_app__automation_update",
                "call_id": "automation-1",
                "arguments": "{}"
            }]
        });
        assert!(restore_response_tool_calls(
            &mut response,
            &restore_map,
            true
        ));
        assert_eq!(response["output"][0]["name"], "automation_update");
        assert_eq!(response["output"][0]["namespace"], "codex_app");
    }

    #[test]
    fn native_followup_merges_discovered_children_into_existing_namespace() {
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "tools": [
                {
                    "type": "function",
                    "name": "tool_search",
                    "parameters": {"type": "object"}
                },
                {
                    "type": "namespace",
                    "name": "codex_app",
                    "description": "Tools provided by the Codex app.",
                    "tools": [{
                        "type": "function",
                        "name": "read_thread",
                        "parameters": {"type": "object"}
                    }]
                }
            ],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search-1",
                "execution": "client",
                "status": "completed",
                "tools": [{
                    "type": "namespace",
                    "name": "codex_app",
                    "description": "Tools provided by the Codex app.",
                    "tools": [{
                        "type": "function",
                        "name": "list_projects",
                        "parameters": {
                            "type": "object",
                            "properties": {},
                            "additionalProperties": false
                        },
                        "defer_loading": true
                    }]
                }]
            }]
        });

        assert!(flatten_request_namespaces(&mut body).unwrap());
        let names: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"codex_app__read_thread"));
        assert!(
            names.contains(&"codex_app__list_projects"),
            "discovered children from an existing namespace must be merged: {names:?}"
        );
    }

    #[test]
    fn native_followup_activates_desktop_snake_case_tools_without_top_level_tools() {
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "tool_choice": "none",
            "input": [{
                "type": "tool_search_output",
                "call_id": "search-1",
                "execution": "client",
                "status": "completed",
                "tools": [{
                    "type": "namespace",
                    "name": "codex_app",
                    "tools": [
                        {
                            "type": "function",
                            "name": "list_projects",
                            "description": "List projects",
                            "strict": false,
                            "defer_loading": true,
                            "parameters": {
                                "type": "object",
                                "properties": {},
                                "additionalProperties": false
                            }
                        },
                        {
                            "type": "function",
                            "name": "create_thread",
                            "description": "Create task",
                            "strict": false,
                            "defer_loading": true,
                            "parameters": {"type": "object"}
                        }
                    ]
                }]
            }]
        });

        assert!(flatten_request_namespaces(&mut body).unwrap());
        let tools = body["tools"].as_array().unwrap();
        for name in ["codex_app__list_projects", "codex_app__create_thread"] {
            let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
            assert_eq!(tool["type"], "function");
            assert!(tool.get("defer_loading").is_none());
            assert!(tool.get("deferLoading").is_none());
        }
        assert_eq!(body["input"][0]["type"], "function_call_output");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn flatten_rewrites_only_direct_protocol_input_items() {
        let nested_call = json!({
            "type": "function_call",
            "namespace": "mcp__files__",
            "name": "read",
            "call_id": "nested-call",
            "arguments": "{}"
        });
        let mut body = json!({
            "tools": [{
                "type": "namespace",
                "name": "mcp__files__",
                "tools": [{"type": "function", "name": "read", "parameters": {}}]
            }],
            "input": [
                {
                    "type": "function_call",
                    "namespace": "mcp__files__",
                    "name": "read",
                    "call_id": "direct-call",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "outer-call",
                    "output": {"result": nested_call.clone()}
                }
            ]
        });

        assert!(flatten_request_namespaces(&mut body).unwrap());
        assert_eq!(body["input"][0]["name"], "mcp__files____read");
        assert!(body["input"][0].get("namespace").is_none());
        assert_eq!(body["input"][1]["output"]["result"], nested_call);
    }

    #[test]
    fn nested_tool_search_output_in_function_payload_is_not_rewritten() {
        let nested = json!({
            "type": "tool_search_output",
            "tools": [{"type": "function", "name": "loaded", "parameters": {}}]
        });
        let mut body = json!({
            "tools": [{"type": "function", "name": "shell", "parameters": {}}],
            "input": [{"type": "function_call_output", "call_id": "call-1", "output": nested.clone()}]
        });

        assert!(!flatten_request_namespaces(&mut body).unwrap());
        assert_eq!(body["input"][0]["output"], nested);
        assert!(body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["name"] != "loaded"));
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
        assert!(restore_response_tool_calls(&mut response, &map, false));
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
        assert!(!restore_response_tool_calls(&mut response, &map, false));
        assert_eq!(response["output"][0]["name"], "plain_tool");
        assert!(response["output"][0].get("namespace").is_none());
    }

    #[test]
    fn restore_converts_tool_search_function_call_for_native_responses() {
        let mut response = json!({
            "output": [
                {
                    "id": "fc_search-1",
                    "type": "function_call",
                    "name": "tool_search",
                    "call_id": "search-1",
                    "status": "completed",
                    "arguments": "{\"query\":\"automation\",\"limit\":\"8\"}"
                },
                {
                    "id": "fc_regular-1",
                    "type": "function_call",
                    "name": "plain_tool",
                    "call_id": "regular-1",
                    "status": "completed",
                    "arguments": "{}"
                }
            ]
        });

        assert!(restore_response_tool_calls(
            &mut response,
            &HashMap::new(),
            true
        ));
        let call = &response["output"][0];
        assert_eq!(call["id"], "tsc_search-1");
        assert_eq!(call["type"], "tool_search_call");
        assert_eq!(call["execution"], "client");
        assert_eq!(call["call_id"], "search-1");
        assert_eq!(call["status"], "completed");
        assert_eq!(call["arguments"]["query"], "automation");
        assert_eq!(call["arguments"]["limit"], 8);
        assert!(call.get("name").is_none());
        assert_eq!(response["output"][1]["id"], "fc_regular-1");
        assert_eq!(response["output"][1]["type"], "function_call");
    }

    #[test]
    fn restore_ignores_function_call_shaped_metadata() {
        let metadata_call = json!({
            "id": "fc_metadata",
            "type": "function_call",
            "name": "tool_search",
            "call_id": "metadata-call",
            "arguments": "{}"
        });
        let mut response = json!({
            "metadata": {"echo": metadata_call.clone()},
            "output": [{
                "id": "fc_output",
                "type": "function_call",
                "name": "tool_search",
                "call_id": "output-call",
                "arguments": "{}"
            }]
        });

        assert!(restore_response_tool_calls(
            &mut response,
            &HashMap::new(),
            true
        ));
        assert_eq!(response["metadata"]["echo"], metadata_call);
        assert_eq!(response["output"][0]["type"], "tool_search_call");

        let mut event = json!({
            "type": "response.output_item.added",
            "metadata": {"echo": metadata_call.clone()},
            "item": {
                "id": "fc_event",
                "type": "function_call",
                "name": "tool_search",
                "call_id": "event-call",
                "arguments": "{}"
            }
        });
        let mut item_ids = HashMap::new();

        assert!(restore_sse_event_tool_calls(
            &mut event,
            &HashMap::new(),
            true,
            &mut item_ids
        ));
        assert_eq!(event["metadata"]["echo"], metadata_call);
        assert_eq!(event["item"]["type"], "tool_search_call");
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
        let out = create_tool_call_restore_sse_stream(input, map, false);
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

    #[tokio::test]
    async fn sse_stream_restores_native_tool_search_function_call() {
        let added = "event: response.output_item.added\n\
                     data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_search-1\",\"type\":\"function_call\",\"name\":\"tool_search\",\"call_id\":\"search-1\",\"status\":\"in_progress\",\"arguments\":\"\"}}\n\n";
        let arguments_delta = "event: response.function_call_arguments.delta\n\
                               data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_search-1\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\\\"thread tools\\\"}\"}\n\n";
        let arguments_done = "event: response.function_call_arguments.done\n\
                              data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_search-1\",\"output_index\":0,\"arguments\":\"{\\\"query\\\":\\\"thread tools\\\",\\\"limit\\\":8}\"}\n\n";
        let done = "event: response.output_item.done\n\
                    data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_search-1\",\"type\":\"function_call\",\"name\":\"tool_search\",\"call_id\":\"search-1\",\"status\":\"completed\",\"arguments\":\"{\\\"query\\\":\\\"thread tools\\\",\\\"limit\\\":8}\"}}\n\n";
        let completed = "event: response.completed\n\
                         data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"id\":\"fc_search-1\",\"type\":\"function_call\",\"name\":\"tool_search\",\"call_id\":\"search-1\",\"status\":\"completed\",\"arguments\":\"{\\\"query\\\":\\\"thread tools\\\",\\\"limit\\\":8}\"}]}}\n\n";
        let input = stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(added)),
            Ok(Bytes::from(arguments_delta)),
            Ok(Bytes::from(arguments_done)),
            Ok(Bytes::from(done)),
            Ok(Bytes::from(completed)),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ]);
        let out = create_tool_call_restore_sse_stream(input, HashMap::new(), true);
        futures::pin_mut!(out);

        let mut collected = String::new();
        while let Some(chunk) = out.next().await {
            collected.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        }

        assert_eq!(
            collected.matches("\"type\":\"tool_search_call\"").count(),
            3
        );
        assert_eq!(collected.matches("\"id\":\"tsc_search-1\"").count(), 3);
        assert_eq!(collected.matches("\"item_id\":\"tsc_search-1\"").count(), 2);
        assert!(!collected.contains("fc_search-1"));
        assert!(collected.contains("\"execution\":\"client\""));
        assert!(collected.contains("\"query\":\"thread tools\""));
        assert!(collected.contains("\"limit\":8"));
        assert!(!collected.contains("\"name\":\"tool_search\""));
    }
}
