//! Codex `tool_search` compatibility for xAI's native Responses endpoint.
//!
//! Codex uses private Responses items (`tool_search`, `tool_search_call`, and
//! `tool_search_output`) to defer large dynamic-tool catalogs. xAI's strict
//! Responses parser does not accept those item types, but it does accept normal
//! function tools and function-call history. This module translates only that
//! protocol surface; Codex remains responsible for searching and executing MCP
//! tools locally.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use super::codex_tool_bridge::{
    request_offers_tool_search as shared_request_offers_tool_search, TOOL_SEARCH_NATIVE_TYPE,
    TOOL_SEARCH_PROXY_NAME,
};
use crate::proxy::error::ProxyError;

const XAI_MAX_TOOL_COUNT: usize = 350;

/// Whether the original Codex request offered the private `tool_search` tool.
///
/// Response restoration uses this request-scoped bit so an unrelated user
/// function is never reclassified accidentally.
pub(crate) fn request_offers_tool_search(body: &Value) -> bool {
    shared_request_offers_tool_search(body)
}

/// Translate Codex's private deferred-tool protocol into xAI-compatible native
/// Responses shapes.
///
/// This runs before namespace flattening and the existing xAI sanitizer:
///
/// - `type: tool_search` becomes a normal function declaration.
/// - prior `tool_search_call` items become normal `function_call` history.
/// - `tool_search_output` tools are promoted to top-level tools, while the
///   output item becomes a normal `function_call_output`.
/// - loaded tools have their deferred marker removed so namespace flattening
///   exposes exactly the tools Codex selected.
pub(crate) fn prepare_xai_tool_search_request(body: &mut Value) -> Result<bool, ProxyError> {
    let offers_tool_search = request_offers_tool_search(body);
    if has_tool_search_proxy_function(body) {
        return Err(ProxyError::TransformError(
            "function name ccswitch_tool_search is reserved for the xAI tool_search proxy shim"
                .to_string(),
        ));
    }
    if !offers_tool_search {
        validate_no_search_tool_catalog(body)?;
    }

    let mut changed = replace_tool_search_declaration(body);
    changed |= replace_tool_search_choice(body);
    let mut loaded_tools = take_additional_tools(body, &mut changed);
    loaded_tools.extend(rewrite_tool_search_history(body, &mut changed));
    if !loaded_tools.is_empty() {
        changed |= append_loaded_tools(body, loaded_tools)?;
    }
    if offers_tool_search {
        changed |= omit_unloaded_top_level_tools(body);
    }
    validate_xai_visible_tool_limit(body)?;
    Ok(changed)
}

/// Normalize every top-level function after namespace flattening.
///
/// Codex may replay a tool discovered in an earlier turn directly in the next
/// request's top-level catalog, without another `tool_search_output` carrier.
/// Those replayed tools must receive the same xAI schema normalization as newly
/// discovered tools, especially when their root schema is `object | null`.
pub(crate) fn normalize_xai_top_level_function_schemas(body: &mut Value) -> bool {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let before = tool.clone();
        normalize_loaded_tool(tool);
        changed |= *tool != before;
    }
    changed
}

fn omit_unloaded_top_level_tools(body: &mut Value) -> bool {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };
    let original_len = tools.len();
    let mut changed = false;
    tools.retain_mut(|tool| {
        if is_tool_deferred(tool) {
            return false;
        }
        if let Some(obj) = tool.as_object_mut() {
            changed |= obj.remove("defer_loading").is_some();
            changed |= obj.remove("deferLoading").is_some();
        }
        true
    });
    changed || tools.len() != original_len
}

fn has_tool_search_proxy_function(body: &Value) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools.iter().any(|tool| {
                tool.get("type").and_then(Value::as_str) == Some("function")
                    && tool
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(is_tool_search_proxy_name)
            })
        })
}

fn is_tool_search_proxy_name(name: &str) -> bool {
    name == TOOL_SEARCH_PROXY_NAME
}

fn validate_no_search_tool_catalog(body: &Value) -> Result<(), ProxyError> {
    if body_contains_deferred_tools(body) {
        return Err(ProxyError::TransformError(
            "xAI native Responses received deferred Codex tools without tool_search support"
                .to_string(),
        ));
    }

    validate_xai_visible_tool_limit(body)
}

fn validate_xai_visible_tool_limit(body: &Value) -> Result<(), ProxyError> {
    let visible_tools = count_xai_visible_tools(body);
    if visible_tools > XAI_MAX_TOOL_COUNT {
        return Err(ProxyError::TransformError(format!(
            "xAI native Responses received {visible_tools} visible tools; limit is {XAI_MAX_TOOL_COUNT}"
        )));
    }
    Ok(())
}

fn body_contains_deferred_tools(body: &Value) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools_contain_deferred(tools))
        || body
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    matches!(
                        item.get("type").and_then(Value::as_str),
                        Some("additional_tools" | "tool_search_output")
                    ) && item
                        .get("tools")
                        .and_then(Value::as_array)
                        .is_some_and(|tools| tools_contain_deferred(tools))
                })
            })
}

fn tools_contain_deferred(tools: &[Value]) -> bool {
    tools.iter().any(tool_contains_deferred)
}

fn tool_contains_deferred(tool: &Value) -> bool {
    is_tool_deferred(tool)
        || tool
            .get("tools")
            .or_else(|| tool.get("children"))
            .and_then(Value::as_array)
            .is_some_and(|tools| tools_contain_deferred(tools))
}

fn is_tool_deferred(tool: &Value) -> bool {
    tool.get("defer_loading")
        .or_else(|| tool.get("deferLoading"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn count_xai_visible_tools(body: &Value) -> usize {
    // Promotion (`append_loaded_tools`) merges carrier tools into the top-level
    // catalog by the same dedup keys, so a repeated `additional_tools` copy must
    // not inflate the no-search 350-tool check.
    let mut seen = HashSet::new();
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            collect_xai_visible_tool_keys(tool, None, &mut seen);
        }
    }
    if let Some(items) = body.get("input").and_then(Value::as_array) {
        for item in items {
            if !matches!(
                item.get("type").and_then(Value::as_str),
                Some("additional_tools" | "tool_search_output")
            ) {
                continue;
            }
            let Some(tools) = item.get("tools").and_then(Value::as_array) else {
                continue;
            };
            for tool in tools {
                collect_xai_visible_tool_keys(tool, None, &mut seen);
            }
        }
    }
    seen.len()
}

fn collect_xai_visible_tool_keys(
    tool: &Value,
    namespace_key: Option<&str>,
    seen: &mut HashSet<String>,
) {
    // Initial deferred catalog entries are omitted later by the request
    // transforms. Loaded copies have their marker removed before reaching this
    // count, so only tools that will actually be sent upstream consume xAI's
    // catalog limit.
    if is_tool_deferred(tool) {
        return;
    }
    match tool.get("type").and_then(Value::as_str) {
        Some("namespace") => {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let nested_key = match namespace_key {
                Some(parent) => format!("{parent}\0namespace\0{name}"),
                None => format!("namespace\0{name}"),
            };
            if let Some(children) = tool
                .get("tools")
                .or_else(|| tool.get("children"))
                .and_then(Value::as_array)
            {
                for child in children {
                    collect_xai_visible_tool_keys(child, Some(&nested_key), seen);
                }
            }
        }
        Some(
            "function" | "web_search" | "x_search" | "image_generation" | "collections_search"
            | "file_search" | "code_execution" | "code_interpreter" | "mcp" | "shell",
        ) => {
            let tool_key = tool_dedup_key(tool);
            let key = match namespace_key {
                Some(namespace_key) => format!("{namespace_key}\0{tool_key}"),
                None => tool_key,
            };
            seen.insert(key);
        }
        _ => {}
    }
}

/// Convert xAI's proxy function call back into a client-executed Codex
/// `tool_search_call`. Recurses through both full Responses payloads and parsed
/// SSE events.
pub(crate) fn restore_tool_search_calls(value: &mut Value) -> bool {
    if restore_direct_output_item(value) {
        return true;
    }
    let Some(obj) = value.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    let is_output_item_event = matches!(
        obj.get("type").and_then(Value::as_str),
        Some("response.output_item.added" | "response.output_item.done")
    );
    if is_output_item_event {
        if let Some(item) = obj.get_mut("item") {
            changed |= restore_direct_output_item(item);
        }
    }
    if let Some(output) = obj.get_mut("output").and_then(Value::as_array_mut) {
        for item in output {
            changed |= restore_direct_output_item(item);
        }
    }
    if let Some(response) = obj.get_mut("response") {
        changed |= restore_tool_search_calls(response);
    }
    changed
}

fn restore_direct_output_item(value: &mut Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    if !is_tool_search_function_call(obj) {
        return false;
    }
    *value = tool_search_call_item(obj);
    true
}

fn replace_tool_search_declaration(body: &mut Value) -> bool {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) == Some(TOOL_SEARCH_NATIVE_TYPE) {
            *tool = tool_search_function();
            changed = true;
        }
    }
    changed
}

fn replace_tool_search_choice(body: &mut Value) -> bool {
    let Some(choice) = body.get_mut("tool_choice") else {
        return false;
    };
    if choice.get("type").and_then(Value::as_str) != Some(TOOL_SEARCH_NATIVE_TYPE) {
        return false;
    }
    *choice = json!({"type": "function", "name": TOOL_SEARCH_PROXY_NAME});
    true
}

fn tool_search_function() -> Value {
    json!({
        "type": "function",
        "name": TOOL_SEARCH_PROXY_NAME,
        "description": "Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task.",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for tools or connectors to load."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tool groups to return."
                }
            },
            "required": ["query"]
        }
    })
}

fn take_additional_tools(body: &mut Value, changed: &mut bool) -> Vec<Value> {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return Vec::new();
    };
    if !input
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("additional_tools"))
    {
        return Vec::new();
    }

    let mut loaded_tools = Vec::new();
    let mut retained = Vec::with_capacity(input.len());
    for item in std::mem::take(input) {
        if item.get("type").and_then(Value::as_str) == Some("additional_tools") {
            if let Some(tools) = item.get("tools").and_then(Value::as_array) {
                for tool in tools {
                    let mut loaded = tool.clone();
                    normalize_loaded_tool(&mut loaded);
                    loaded_tools.push(loaded);
                }
            }
            *changed = true;
        } else {
            retained.push(item);
        }
    }
    *input = retained;
    loaded_tools
}

fn rewrite_tool_search_history(body: &mut Value, changed: &mut bool) -> Vec<Value> {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return Vec::new();
    };

    let mut loaded_tools = Vec::new();
    for item in input {
        match item.get("type").and_then(Value::as_str) {
            Some("tool_search_call") => {
                *item = tool_search_call_as_function_call(item);
                *changed = true;
            }
            Some("tool_search_output") => {
                if let Some(tools) = item.get("tools").and_then(Value::as_array) {
                    for tool in tools {
                        let mut loaded = tool.clone();
                        normalize_loaded_tool(&mut loaded);
                        loaded_tools.push(loaded);
                    }
                }
                *item = tool_search_output_as_function_output(item);
                *changed = true;
            }
            _ => {}
        }
    }
    loaded_tools
}

fn tool_search_call_as_function_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = canonical_arguments(item.get("arguments"));
    let mut output = json!({
        "type": "function_call",
        "name": TOOL_SEARCH_PROXY_NAME,
        "call_id": call_id,
        "arguments": arguments
    });
    if let Some(status) = item.get("status").cloned() {
        output["status"] = status;
    }
    output
}

fn tool_search_output_as_function_output(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string());
    json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output
    })
}

fn canonical_arguments(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(value)) => value.clone(),
        Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

fn append_loaded_tools(body: &mut Value, loaded_tools: Vec<Value>) -> Result<bool, ProxyError> {
    let Some(obj) = body.as_object_mut() else {
        return Ok(false);
    };
    let tools = obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(tools) = tools.as_array_mut() else {
        return Ok(false);
    };

    let mut changed = false;
    for tool in loaded_tools {
        if tool.get("type").and_then(Value::as_str) == Some("function")
            && tool
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(is_tool_search_proxy_name)
        {
            return Err(ProxyError::TransformError(
                "discovered function collides with the xAI tool_search proxy shim".to_string(),
            ));
        }

        let key = tool_dedup_key(&tool);
        if tool.get("type").and_then(Value::as_str) == Some("namespace") {
            if let Some(existing) = tools.iter_mut().find(|candidate| {
                candidate.get("type").and_then(Value::as_str) == Some("namespace")
                    && candidate.get("name") == tool.get("name")
            }) {
                changed |= merge_namespace_children(existing, &tool);
                continue;
            }
        }
        if let Some(index) = tools
            .iter()
            .position(|existing| tool_dedup_key(existing) == key)
        {
            if tools[index] != tool {
                // A discovered top-level function supersedes its stale deferred
                // declaration and schema aliases.
                tools[index] = tool;
                changed = true;
            }
        } else {
            tools.push(tool);
            changed = true;
        }
    }
    Ok(changed)
}

fn merge_namespace_children(existing: &mut Value, loaded: &Value) -> bool {
    // An outer deferred marker means none of the original namespace children
    // were published. A search result is the authoritative selected subset, so
    // retaining unmatched children from the stale catalog would expose tools
    // the client never loaded.
    if is_tool_deferred(existing) {
        if existing != loaded {
            *existing = loaded.clone();
            return true;
        }
        return false;
    }

    let Some(loaded_children) = loaded
        .get("tools")
        .or_else(|| loaded.get("children"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    let Some(existing_obj) = existing.as_object_mut() else {
        return false;
    };
    let mut changed = existing_obj.remove("defer_loading").is_some();
    changed |= existing_obj.remove("deferLoading").is_some();
    if !existing_obj.contains_key("tools") {
        if let Some(children) = existing_obj.remove("children") {
            existing_obj.insert("tools".to_string(), children);
            changed = true;
        }
    }
    let existing_children = existing_obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(existing_children) = existing_children.as_array_mut() else {
        return changed;
    };

    for loaded_child in loaded_children {
        let key = tool_dedup_key(loaded_child);
        if let Some(index) = existing_children
            .iter()
            .position(|child| tool_dedup_key(child) == key)
        {
            if existing_children[index] != *loaded_child {
                // Prefer the discovered copy: its deferred marker and schema
                // aliases have already been normalized.
                existing_children[index] = loaded_child.clone();
                changed = true;
            }
        } else {
            existing_children.push(loaded_child.clone());
            changed = true;
        }
    }
    changed
}

fn tool_dedup_key(tool: &Value) -> String {
    let tool_type = tool
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !tool_type.is_empty() && !name.is_empty() {
        return format!("{tool_type}\0{name}");
    }
    if tool_type == "mcp" {
        if let Some(label) = tool.get("server_label").and_then(Value::as_str) {
            let label = label.trim();
            if !label.is_empty() {
                return format!("mcp\0{label}");
            }
        }
    }
    tool.to_string()
}

fn normalize_loaded_tool(tool: &mut Value) {
    let Some(obj) = tool.as_object_mut() else {
        return;
    };
    obj.remove("defer_loading");
    obj.remove("deferLoading");

    match obj.get("type").and_then(Value::as_str) {
        Some("function") => {
            let mut parameters = ["parameters", "inputSchema", "input_schema"]
                .iter()
                .filter_map(|key| obj.get(*key).cloned())
                .find(|value| {
                    value
                        .as_object()
                        .is_some_and(schema_alias_can_be_root_object)
                })
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            obj.remove("inputSchema");
            obj.remove("input_schema");

            if let Some(schema) = parameters.as_object_mut() {
                normalize_root_object_schema(schema);
            }
            obj.insert("parameters".to_string(), parameters);
        }
        Some("namespace") => {
            let child_key = if obj.contains_key("tools") {
                "tools"
            } else {
                "children"
            };
            if let Some(children) = obj.get_mut(child_key).and_then(Value::as_array_mut) {
                for child in children {
                    normalize_loaded_tool(child);
                }
            }
        }
        _ => {}
    }
}

fn normalize_root_object_schema(schema: &mut Map<String, Value>) {
    // xAI does not treat a ref-only branch as an object branch, even when the
    // referenced `$defs` entry ultimately resolves to an object. Codex
    // Desktop's automation tool uses several layers of ref-only unions at the
    // parameter root, so recursively expand only that root candidate graph
    // into direct object branches. Nested `properties`/`items` schemas are not
    // traversed, preserving legitimate nullable fields.
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let original_union_key = ["oneOf", "anyOf"]
        .into_iter()
        .find(|key| schema.get(*key).is_some());
    let had_root_ref = schema.get("$ref").is_some();
    let mut root_candidate = schema.clone();
    root_candidate.remove("$defs");
    let mut variants = expand_root_object_variants(&root_candidate, &defs, &mut HashSet::new());

    schema.remove("$ref");
    schema.remove("anyOf");
    schema.remove("oneOf");
    if let Some(union_key) = original_union_key {
        if !variants.is_empty() {
            schema.insert(
                union_key.to_string(),
                Value::Array(variants.into_iter().map(Value::Object).collect()),
            );
        }
    } else if had_root_ref && variants.len() == 1 {
        for (key, value) in variants.pop().unwrap() {
            schema.insert(key, value);
        }
    } else if had_root_ref && !variants.is_empty() {
        schema.insert(
            "oneOf".to_string(),
            Value::Array(variants.into_iter().map(Value::Object).collect()),
        );
    }
    schema.insert("type".to_string(), json!("object"));
    schema
        .entry("properties".to_string())
        .or_insert_with(|| json!({}));
}

fn schema_alias_can_be_root_object(schema: &Map<String, Value>) -> bool {
    !schema.is_empty() && schema_type_can_be_object(schema.get("type"))
}

fn expand_root_object_variants(
    schema: &Map<String, Value>,
    defs: &Map<String, Value>,
    visiting: &mut HashSet<String>,
) -> Vec<Map<String, Value>> {
    if !schema_type_can_be_object(schema.get("type")) {
        return Vec::new();
    }

    let mut base = schema.clone();
    base.remove("$ref");
    base.remove("anyOf");
    base.remove("oneOf");
    base.remove("$defs");
    base.insert("type".to_string(), json!("object"));

    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let Some(name) = local_def_name(reference) else {
            return Vec::new();
        };
        if !visiting.insert(name.clone()) {
            return Vec::new();
        }
        let mut variants = defs
            .get(&name)
            .and_then(Value::as_object)
            .map(|target| expand_root_object_variants(target, defs, visiting))
            .unwrap_or_default();
        visiting.remove(&name);
        merge_root_candidate_base(&base, &mut variants);
        return variants;
    }

    if let Some(branches) = ["oneOf", "anyOf"]
        .into_iter()
        .find_map(|key| schema.get(key).and_then(Value::as_array))
    {
        let mut variants = Vec::new();
        for branch in branches {
            if let Some(branch) = branch.as_object() {
                variants.extend(expand_root_object_variants(branch, defs, visiting));
            }
        }
        merge_root_candidate_base(&base, &mut variants);
        return variants;
    }

    vec![base]
}

fn merge_root_candidate_base(base: &Map<String, Value>, variants: &mut [Map<String, Value>]) {
    for variant in variants {
        for (key, value) in base {
            variant.entry(key.clone()).or_insert_with(|| value.clone());
        }
        variant.insert("type".to_string(), json!("object"));
    }
}

fn local_def_name(reference: &str) -> Option<String> {
    reference
        .strip_prefix("#/$defs/")
        .filter(|name| !name.is_empty() && !name.contains('/'))
        .map(|name| name.replace("~1", "/").replace("~0", "~"))
}

fn schema_type_can_be_object(schema_type: Option<&Value>) -> bool {
    match schema_type {
        None => true,
        Some(Value::String(kind)) => kind == "object",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind.as_str() == Some("object")),
        Some(_) => false,
    }
}

fn is_tool_search_function_call(obj: &Map<String, Value>) -> bool {
    obj.get("type").and_then(Value::as_str) == Some("function_call")
        && obj.get("name").and_then(Value::as_str) == Some(TOOL_SEARCH_PROXY_NAME)
        && !obj.contains_key("namespace")
}

fn tool_search_call_item(item: &Map<String, Value>) -> Value {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let mut arguments = item
        .get("arguments")
        .and_then(|value| match value {
            Value::Object(_) => Some(value.clone()),
            Value::String(raw) => serde_json::from_str::<Value>(raw).ok(),
            _ => None,
        })
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(limit) = arguments
        .get("limit")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
    {
        arguments["limit"] = json!(limit);
    }

    let mut output = json!({
        "type": "tool_search_call",
        "call_id": call_id,
        "status": status,
        "execution": "client",
        "arguments": arguments
    });
    if let Some(reasoning) = item.get("reasoning_content").cloned() {
        output["reasoning_content"] = reasoning;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxies_tool_search_and_loaded_namespace_tools() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "namespace",
                    "name": "mcp__mail__",
                    "tools": [{
                        "type": "function",
                        "name": "search",
                        "defer_loading": true,
                        "parameters": {"type": "object"}
                    }]
                }
            ],
            "tool_choice": {"type": "tool_search"},
            "input": [
                {
                    "type": "tool_search_call",
                    "call_id": "search_1",
                    "status": "completed",
                    "execution": "client",
                    "arguments": {"query": "search mail", "limit": 5}
                },
                {
                    "type": "tool_search_output",
                    "call_id": "search_1",
                    "tools": [{
                        "type": "namespace",
                        "name": "mcp__mail__",
                        "tools": [{
                            "type": "function",
                            "name": "search",
                            "defer_loading": true,
                            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}}
                        }]
                    }]
                }
            ]
        });

        assert!(request_offers_tool_search(&body));
        assert!(prepare_xai_tool_search_request(&mut body).unwrap());

        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], TOOL_SEARCH_PROXY_NAME);
        assert_eq!(
            body["tool_choice"],
            json!({"type": "function", "name": TOOL_SEARCH_PROXY_NAME})
        );
        assert_eq!(body["tools"][1]["type"], "namespace");
        assert!(body["tools"][1]["tools"][0].get("defer_loading").is_none());
        assert!(body["tools"][1]["tools"][0].get("inputSchema").is_none());
        assert_eq!(body["tools"][1]["tools"][0]["parameters"]["type"], "object");
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][0]["name"], TOOL_SEARCH_PROXY_NAME);
        assert_eq!(body["input"][1]["type"], "function_call_output");

        super::super::transform_codex_responses_namespace::flatten_request_namespaces(&mut body)
            .unwrap();
        let names = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(names, vec![TOOL_SEARCH_PROXY_NAME, "mcp__mail____search"]);
    }

    #[test]
    fn discovered_namespace_clears_outer_deferred_marker() {
        let mut body = json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "namespace",
                    "name": "mcp__mail__",
                    "defer_loading": true,
                    "tools": [{
                        "type": "function",
                        "name": "search",
                        "defer_loading": true,
                        "parameters": {"type": "object"}
                    }]
                }
            ],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search_1",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__mail__",
                    "deferLoading": true,
                    "tools": [{
                        "type": "function",
                        "name": "search",
                        "deferLoading": true,
                        "parameters": {"type": "object"}
                    }]
                }]
            }]
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        assert!(body["tools"][1].get("defer_loading").is_none());
        super::super::transform_codex_responses_namespace::flatten_request_namespaces(&mut body)
            .unwrap();
        assert_eq!(body["tools"][1]["name"], "mcp__mail____search");
    }

    #[test]
    fn discovered_subset_replaces_outer_deferred_namespace_catalog() {
        let mut body = json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "namespace",
                    "name": "mcp__mail__",
                    "defer_loading": true,
                    "tools": [
                        {
                            "type": "function",
                            "name": "search",
                            "parameters": {"type": "object", "properties": {"stale": {"type": "string"}}}
                        },
                        {
                            "type": "function",
                            "name": "delete",
                            "parameters": {"type": "object"}
                        }
                    ]
                }
            ],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search_1",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__mail__",
                    "defer_loading": true,
                    "tools": [{
                        "type": "function",
                        "name": "search",
                        "defer_loading": true,
                        "parameters": {"type": "object", "properties": {"query": {"type": "string"}}}
                    }]
                }]
            }]
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        let namespace = &body["tools"][1];
        assert!(namespace.get("defer_loading").is_none());
        assert_eq!(namespace["tools"].as_array().unwrap().len(), 1);
        assert_eq!(namespace["tools"][0]["name"], "search");
        assert!(namespace["tools"][0]["parameters"]["properties"]
            .get("stale")
            .is_none());

        super::super::transform_codex_responses_namespace::flatten_request_namespaces(&mut body)
            .unwrap();
        let names = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(names, vec![TOOL_SEARCH_PROXY_NAME, "mcp__mail____search"]);
    }

    #[test]
    fn discovered_function_replaces_stale_top_level_copy_without_corrupting_schema() {
        let mut body = json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "function",
                    "name": "lookup",
                    "defer_loading": true,
                    "parameters": {"type": "object"}
                }
            ],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search_1",
                "tools": [{
                    "type": "function",
                    "name": "lookup",
                    "deferLoading": true,
                    "parameters": {"oneOf": [{"required": ["id"]}]},
                    "inputSchema": {"type": "string"}
                }]
            }]
        });
        body["input"][0]["tools"][0]["parameters"]["properties"] = json!({
            "defer_loading": {"type": "boolean"}
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        let lookup = &body["tools"][1];
        assert!(lookup.get("defer_loading").is_none());
        assert!(lookup.get("deferLoading").is_none());
        assert!(lookup.get("inputSchema").is_none());
        assert_eq!(lookup["parameters"]["type"], "object");
        assert_eq!(
            lookup["parameters"]["properties"]["defer_loading"]["type"],
            "boolean"
        );
    }

    #[test]
    fn invalid_parameters_fall_back_to_valid_schema_alias() {
        let mut tool = json!({
            "type": "function",
            "name": "lookup",
            "parameters": {"type": "string"},
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "string"}}
            }
        });
        normalize_loaded_tool(&mut tool);
        assert_eq!(tool["parameters"]["properties"]["id"]["type"], "string");
        assert!(tool.get("inputSchema").is_none());
    }

    #[test]
    fn ordinary_object_schema_is_not_wrapped_in_synthetic_union() {
        let mut tool = json!({
            "type": "function",
            "name": "lookup",
            "parameters": {
                "type": "object",
                "properties": {"id": {"type": "string"}}
            }
        });

        normalize_loaded_tool(&mut tool);
        let parameters = &tool["parameters"];
        assert_eq!(parameters["type"], "object");
        assert_eq!(parameters["properties"]["id"]["type"], "string");
        assert!(parameters.get("oneOf").is_none());
        assert!(parameters.get("anyOf").is_none());
    }

    #[test]
    fn sole_root_ref_schema_is_inlined_without_synthetic_union() {
        let mut tool = json!({
            "type": "function",
            "name": "lookup",
            "parameters": {
                "$ref": "#/$defs/query",
                "$defs": {
                    "query": {
                        "type": "object",
                        "properties": {"q": {"type": "string"}}
                    }
                }
            }
        });

        normalize_loaded_tool(&mut tool);
        let parameters = &tool["parameters"];
        assert_eq!(parameters["type"], "object");
        assert_eq!(parameters["properties"]["q"]["type"], "string");
        assert!(parameters.get("oneOf").is_none());
        assert!(parameters.get("anyOf").is_none());
        assert!(parameters.get("$ref").is_none());
    }

    #[test]
    fn loaded_root_union_drops_non_object_branches_for_xai() {
        let mut tool = json!({
            "type": "function",
            "name": "automation_update",
            "parameters": {
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {"id": {"type": "string"}}
                    },
                    {"type": "null"}
                ]
            }
        });

        normalize_loaded_tool(&mut tool);
        assert_eq!(tool["parameters"]["type"], "object");
        let branches = tool["parameters"]["anyOf"].as_array().unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0]["type"], "object");
        assert_eq!(branches[0]["properties"]["id"]["type"], "string");
    }

    #[test]
    fn replayed_top_level_function_union_is_normalized_after_flatten() {
        let mut body = json!({
            "tools": [
                {
                    "type": "function",
                    "name": "codex_app__automation_update",
                    "parameters": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "value": {"anyOf": [{"type": "string"}, {"type": "null"}]}
                                }
                            },
                            {"type": "null"}
                        ]
                    }
                },
                {"type": "web_search"}
            ]
        });

        assert!(normalize_xai_top_level_function_schemas(&mut body));
        let parameters = &body["tools"][0]["parameters"];
        assert_eq!(parameters["type"], "object");
        assert_eq!(parameters["oneOf"].as_array().unwrap().len(), 1);
        assert_eq!(
            parameters["oneOf"][0]["properties"]["value"]["anyOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(body["tools"][1]["type"], "web_search");
        assert!(!normalize_xai_top_level_function_schemas(&mut body));
    }

    #[test]
    fn automation_root_ref_union_graph_is_normalized_for_xai() {
        let mut tool = json!({
            "type": "function",
            "name": "automation_update",
            "parameters": {
                "oneOf": [
                    {"$ref": "#/$defs/view"},
                    {"$ref": "#/$defs/create"},
                    {"$ref": "#/$defs/update"}
                ],
                "$defs": {
                    "view": {
                        "type": "object",
                        "properties": {"mode": {"type": "string"}}
                    },
                    "create": {
                        "oneOf": [
                            {"$ref": "#/$defs/create_cron"},
                            {"$ref": "#/$defs/create_heartbeat"}
                        ]
                    },
                    "create_cron": {
                        "type": "object",
                        "properties": {
                            "notificationPolicy": {
                                "anyOf": [
                                    {"type": "string"},
                                    {"type": "null"}
                                ]
                            }
                        }
                    },
                    "create_heartbeat": {"type": "object", "properties": {}},
                    "update": {
                        "oneOf": [
                            {"type": "object", "properties": {"id": {"type": "string"}}},
                            {"type": "object", "properties": {"name": {"type": "string"}}}
                        ]
                    }
                }
            }
        });

        normalize_loaded_tool(&mut tool);
        let parameters = &tool["parameters"];
        assert_eq!(parameters["type"], "object");
        let branches = parameters["oneOf"].as_array().unwrap();
        assert_eq!(branches.len(), 5);
        assert!(branches.iter().all(|branch| branch["type"] == "object"));
        assert!(branches.iter().all(|branch| branch.get("$ref").is_none()));
        assert!(branches
            .iter()
            .all(|branch| branch.get("oneOf").is_none() && branch.get("anyOf").is_none()));
        assert_eq!(
            branches[1]["properties"]["notificationPolicy"]["anyOf"][1]["type"],
            "null"
        );
    }

    #[test]
    fn additional_tools_are_promoted_before_namespace_flatten() {
        let mut body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "additional_tools",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__calendar__",
                    "defer_loading": true,
                    "tools": [{
                        "type": "function",
                        "name": "list_events",
                        "defer_loading": true,
                        "input_schema": {}
                    }]
                }]
            }]
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        assert!(body["input"].as_array().unwrap().is_empty());
        super::super::transform_codex_responses_namespace::flatten_request_namespaces(&mut body)
            .unwrap();
        assert_eq!(body["tools"][1]["name"], "mcp__calendar____list_events");
        assert_eq!(body["tools"][1]["parameters"]["type"], "object");
    }

    #[test]
    fn accepts_native_tool_search_function_alongside_search_shim() {
        let mut body = json!({
            "tools": [
                {"type": "tool_search"},
                {"type": "function", "name": "tool_search", "parameters": {}}
            ]
        });
        assert!(prepare_xai_tool_search_request(&mut body).unwrap());
        assert_eq!(body["tools"][0]["name"], TOOL_SEARCH_PROXY_NAME);
        assert_eq!(body["tools"][1]["name"], TOOL_SEARCH_NATIVE_TYPE);
    }

    #[test]
    fn accepts_native_tool_search_function_without_search_shim() {
        let mut body = json!({
            "tools": [{"type": "function", "name": "tool_search", "parameters": {}}]
        });
        assert!(!prepare_xai_tool_search_request(&mut body).unwrap());
        assert_eq!(body["tools"][0]["name"], TOOL_SEARCH_NATIVE_TYPE);
    }

    #[test]
    fn rejects_proxy_function_name_collision() {
        let mut body = json!({
            "tools": [
                {"type": "tool_search"},
                {"type": "function", "name": TOOL_SEARCH_PROXY_NAME, "parameters": {}}
            ]
        });
        assert!(prepare_xai_tool_search_request(&mut body).is_err());
        assert_eq!(body["tools"][0]["type"], "tool_search");
    }

    #[test]
    fn rejects_proxy_function_name_even_without_search_shim() {
        let mut body = json!({
            "tools": [{
                "type": "function",
                "name": TOOL_SEARCH_PROXY_NAME,
                "parameters": {}
            }]
        });
        assert!(prepare_xai_tool_search_request(&mut body).is_err());
    }

    #[test]
    fn accepts_discovered_native_tool_search_function() {
        let mut body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search_1",
                "tools": [{
                    "type": "function",
                    "name": TOOL_SEARCH_NATIVE_TYPE,
                    "parameters": {"type": "object"}
                }]
            }]
        });

        assert!(prepare_xai_tool_search_request(&mut body).unwrap());
        assert_eq!(body["tools"][0]["name"], TOOL_SEARCH_PROXY_NAME);
        assert_eq!(body["tools"][1]["name"], TOOL_SEARCH_NATIVE_TYPE);
    }

    #[test]
    fn rejects_deferred_catalog_without_tool_search_shim() {
        let mut body = json!({
            "tools": [{
                "type": "namespace",
                "name": "mcp__large__",
                "tools": [{"type": "function", "name": "cold", "defer_loading": true}]
            }]
        });
        assert!(prepare_xai_tool_search_request(&mut body).is_err());
    }

    #[test]
    fn rejects_oversized_catalog_without_tool_search_shim() {
        let tools = (0..351)
            .map(|index| json!({"type": "function", "name": format!("tool_{index}")}))
            .collect::<Vec<_>>();
        let mut body = json!({"tools": tools});
        assert!(prepare_xai_tool_search_request(&mut body).is_err());
    }

    #[test]
    fn accepts_catalog_at_limit_when_additional_tools_repeat_top_level() {
        let tools = (0..XAI_MAX_TOOL_COUNT)
            .map(|index| json!({"type": "function", "name": format!("tool_{index}")}))
            .collect::<Vec<_>>();
        let mut body = json!({
            "tools": tools.clone(),
            "input": [{
                "type": "additional_tools",
                "tools": tools
            }]
        });
        assert!(prepare_xai_tool_search_request(&mut body).is_ok());
        assert_eq!(body["tools"].as_array().unwrap().len(), XAI_MAX_TOOL_COUNT);
        assert!(body["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("type").and_then(Value::as_str) != Some("additional_tools")));
    }

    #[test]
    fn rejects_oversized_unique_catalog_with_additional_tools_without_tool_search_shim() {
        let tools = (0..XAI_MAX_TOOL_COUNT)
            .map(|index| json!({"type": "function", "name": format!("tool_{index}")}))
            .collect::<Vec<_>>();
        let mut body = json!({
            "tools": tools,
            "input": [{
                "type": "additional_tools",
                "tools": [{"type": "function", "name": "tool_extra"}]
            }]
        });
        assert!(prepare_xai_tool_search_request(&mut body).is_err());
    }

    #[test]
    fn loaded_mcp_tools_dedup_by_server_label() {
        let mut body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search_1",
                "tools": [
                    {"type": "mcp", "server_label": "alpha"},
                    {"type": "mcp", "server_label": "beta"}
                ]
            }]
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|tool| tool["server_label"] == "alpha"));
        assert!(tools.iter().any(|tool| tool["server_label"] == "beta"));
    }

    #[test]
    fn initial_top_level_deferred_functions_are_omitted() {
        let deferred = (0..400)
            .map(|index| {
                json!({
                    "type": "function",
                    "name": format!("deferred_{index}"),
                    "defer_loading": true,
                    "parameters": {"type": "object"}
                })
            })
            .collect::<Vec<_>>();
        let mut tools = vec![json!({"type": "tool_search"})];
        tools.extend(deferred);
        tools.push(json!({
            "type": "function",
            "name": "hot",
            "deferLoading": false,
            "parameters": {"type": "object"}
        }));
        let mut body = json!({"tools": tools});

        prepare_xai_tool_search_request(&mut body).unwrap();
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
        assert_eq!(body["tools"][0]["name"], TOOL_SEARCH_PROXY_NAME);
        assert_eq!(body["tools"][1]["name"], "hot");
        assert!(body["tools"][1].get("deferLoading").is_none());
    }

    #[test]
    fn initial_top_level_deferred_non_function_tools_are_omitted() {
        let mut body = json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "mcp",
                    "server_label": "cold",
                    "defer_loading": true
                },
                {
                    "type": "mcp",
                    "server_label": "hot",
                    "deferLoading": false
                }
            ]
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], TOOL_SEARCH_PROXY_NAME);
        assert_eq!(tools[1]["server_label"], "hot");
        assert!(tools[1].get("deferLoading").is_none());
    }

    #[test]
    fn rejects_oversized_catalog_after_search_result_promotion() {
        let children = (0..XAI_MAX_TOOL_COUNT)
            .map(|index| {
                json!({
                    "type": "function",
                    "name": format!("loaded_{index}"),
                    "defer_loading": true,
                    "parameters": {"type": "object"}
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "tool_search_output",
                "call_id": "search_1",
                "tools": [{
                    "type": "namespace",
                    "name": "mcp__loaded__",
                    "defer_loading": true,
                    "tools": children
                }]
            }]
        });

        let error = prepare_xai_tool_search_request(&mut body).unwrap_err();
        assert!(error.to_string().contains("351 visible tools"));
        assert!(error.to_string().contains("limit is 350"));
    }

    #[test]
    fn accepts_search_shim_plus_loaded_catalog_at_limit() {
        let loaded = (0..XAI_MAX_TOOL_COUNT - 1)
            .map(|index| {
                json!({
                    "type": "function",
                    "name": format!("loaded_{index}"),
                    "parameters": {"type": "object"}
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "additional_tools",
                "tools": loaded
            }]
        });

        prepare_xai_tool_search_request(&mut body).unwrap();
        assert_eq!(body["tools"].as_array().unwrap().len(), XAI_MAX_TOOL_COUNT);
    }

    #[test]
    fn full_xai_pipeline_keeps_large_deferred_catalog_under_limit() {
        let children = (0..400)
            .map(|index| {
                json!({
                    "type": "function",
                    "name": format!("tool_{index}"),
                    "defer_loading": true,
                    "parameters": {"type": "object"}
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [
                {"type": "tool_search"},
                {"type": "namespace", "name": "mcp__large__", "tools": children}
            ],
            "input": "Find the right tool."
        });

        assert!(prepare_xai_tool_search_request(&mut body).unwrap());
        assert!(
            super::super::transform_codex_responses_namespace::flatten_request_namespaces(
                &mut body
            )
            .unwrap()
        );
        super::super::transform_codex_responses_xai_sanitize::sanitize_xai_responses_request(
            &mut body,
        );

        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], TOOL_SEARCH_PROXY_NAME);
    }

    #[test]
    fn restores_only_proxy_tool_search_calls_when_enabled_by_caller() {
        let mut response = json!({
            "type": "response.completed",
            "response": {
                "output": [
                    {
                        "type": "function_call",
                        "name": TOOL_SEARCH_PROXY_NAME,
                        "call_id": "search_1",
                        "status": "completed",
                        "arguments": "{\"query\":\"mail\",\"limit\":\"3\"}"
                    },
                    {
                        "type": "function_call",
                        "name": TOOL_SEARCH_NATIVE_TYPE,
                        "call_id": "native_1",
                        "status": "completed",
                        "arguments": "{}"
                    }
                ]
            }
        });

        assert!(restore_tool_search_calls(&mut response));
        let call = &response["response"]["output"][0];
        assert_eq!(call["type"], "tool_search_call");
        assert_eq!(call["execution"], "client");
        assert_eq!(call["arguments"]["query"], "mail");
        assert!(call.get("name").is_none());
        let native_call = &response["response"]["output"][1];
        assert_eq!(native_call["type"], "function_call");
        assert_eq!(native_call["name"], TOOL_SEARCH_NATIVE_TYPE);
    }

    #[test]
    fn malformed_tool_search_arguments_fail_closed_to_empty_object() {
        let mut call = json!({
            "type": "function_call",
            "name": TOOL_SEARCH_PROXY_NAME,
            "call_id": "search_1",
            "arguments": "not json"
        });
        assert!(restore_tool_search_calls(&mut call));
        assert_eq!(call["arguments"], json!({}));
    }
}
