//! Shared Codex tool-catalog bridge for upstream protocols that only expose
//! flat function tools.
//!
//! The Codex Responses protocol has richer tool identities (`namespace`,
//! `custom`, and the private deferred `tool_search` flow). Chat Completions,
//! Anthropic Messages, and several OAuth-backed gateways expose a flat function
//! namespace instead. This module owns the lossy boundary in one place:
//!
//! - assign stable upstream names and preserve their original Codex identity;
//! - reserve one private name for the deferred-search shim;
//! - omit unloaded deferred catalog entries while search is available;
//! - promote tools returned by `tool_search_output` and `additional_tools`;
//! - reject ambiguous cross-identity name collisions instead of silently
//!   selecting whichever declaration happened to appear first.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::codex_chat_common::{
    attach_optional_reasoning_content_field, response_function_call_item,
    response_function_call_item_with_namespace,
};
use crate::proxy::{
    error::ProxyError,
    json_canonical::{canonical_json_string, short_sha256_hex},
};

pub(crate) const TOOL_SEARCH_NATIVE_TYPE: &str = "tool_search";
pub(crate) const TOOL_SEARCH_PROXY_NAME: &str = "ccswitch_tool_search";
pub(crate) const CUSTOM_TOOL_INPUT_FIELD: &str = "input";

const CHAT_TOOL_NAME_MAX_LEN: usize = 64;
const CUSTOM_TOOL_INPUT_DESCRIPTION: &str = "Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description.";
const CUSTOM_TOOL_PRESERVED_METADATA_HEADING: &str = "Original tool definition:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexToolKind {
    Function,
    Namespace,
    Custom,
    ToolSearch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexToolSpec {
    pub(crate) kind: CodexToolKind,
    pub(crate) name: String,
    pub(crate) namespace: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexToolContext {
    enable_codex_private_tools: bool,
    function_tools: Vec<Value>,
    upstream_name_to_spec: HashMap<String, CodexToolSpec>,
    upstream_name_to_index: HashMap<String, usize>,
    namespace_name_to_upstream_name: HashMap<(String, String), String>,
}

impl CodexToolContext {
    pub(crate) fn uses_codex_private_tools(&self) -> bool {
        self.enable_codex_private_tools
    }

    /// Function-shaped tools ready for Chat Completions or another flat-tool
    /// adapter. Anthropic renders these into its native tool schema.
    pub(crate) fn function_tools(&self) -> &[Value] {
        &self.function_tools
    }

    pub(crate) fn lookup_upstream_name(&self, upstream_name: &str) -> Option<&CodexToolSpec> {
        self.upstream_name_to_spec.get(upstream_name)
    }

    pub(crate) fn is_custom_tool_upstream_name(&self, upstream_name: &str) -> bool {
        self.lookup_upstream_name(upstream_name)
            .is_some_and(|spec| matches!(&spec.kind, CodexToolKind::Custom))
    }

    pub(crate) fn upstream_name_for_response_function(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> String {
        if let Some(namespace) = namespace
            .filter(|_| self.enable_codex_private_tools)
            .filter(|value| !value.is_empty())
        {
            if let Some(upstream_name) = self
                .namespace_name_to_upstream_name
                .get(&(namespace.to_string(), name.to_string()))
            {
                return upstream_name.clone();
            }
            return flatten_namespace_tool_name(namespace, name);
        }

        name.to_string()
    }

    /// Resolve a forced function choice only when that exact function was
    /// published to the flat-tool upstream. Historical calls use the looser
    /// resolver above because they may legitimately reference an older catalog;
    /// a current tool choice must never point at an omitted deferred tool.
    pub(crate) fn require_published_response_function(
        &self,
        name: &str,
        namespace: Option<&str>,
    ) -> Result<String, ProxyError> {
        let namespace = namespace.filter(|value| !value.is_empty());
        let upstream_name = self.upstream_name_for_response_function(name, namespace);
        let matches_identity = self
            .lookup_upstream_name(&upstream_name)
            .is_some_and(|spec| {
                spec.name == name
                    && spec.namespace.as_deref() == namespace
                    && matches!(
                        (&spec.kind, namespace),
                        (CodexToolKind::Function, None) | (CodexToolKind::Namespace, Some(_))
                    )
            });
        if matches_identity {
            return Ok(upstream_name);
        }

        let display_name = namespace
            .map(|namespace| format!("{namespace}/{name}"))
            .unwrap_or_else(|| name.to_string());
        Err(ProxyError::InvalidRequest(format!(
            "tool_choice selects unavailable function tool {display_name:?}; the tool may still be deferred"
        )))
    }

    pub(crate) fn require_published_custom_tool(&self, name: &str) -> Result<String, ProxyError> {
        if self.lookup_upstream_name(name).is_some_and(|spec| {
            spec.kind == CodexToolKind::Custom && spec.name == name && spec.namespace.is_none()
        }) {
            return Ok(name.to_string());
        }
        Err(ProxyError::InvalidRequest(format!(
            "tool_choice selects unavailable custom tool {name:?}; the tool may still be deferred"
        )))
    }

    pub(crate) fn require_published_tool_search(&self) -> Result<String, ProxyError> {
        if self
            .lookup_upstream_name(TOOL_SEARCH_PROXY_NAME)
            .is_some_and(|spec| spec.kind == CodexToolKind::ToolSearch)
        {
            return Ok(TOOL_SEARCH_PROXY_NAME.to_string());
        }
        Err(ProxyError::InvalidRequest(
            "tool_choice selects tool_search, but the request does not publish tool_search"
                .to_string(),
        ))
    }

    fn add_function_proxy(
        &mut self,
        upstream_name: String,
        spec: CodexToolSpec,
        function_tool: Value,
    ) -> Result<(), ProxyError> {
        if upstream_name.trim().is_empty() {
            return Ok(());
        }

        if let Some(existing) = self.upstream_name_to_spec.get(&upstream_name) {
            if existing != &spec {
                return Err(ProxyError::TransformError(format!(
                    "Codex tools {:?} and {:?} both map to upstream function name {upstream_name:?}",
                    existing, spec
                )));
            }

            // A tool discovered later in the request is authoritative over an
            // earlier catalog copy with the same identity. This refreshes stale
            // schemas without changing the stable upstream name or response map.
            if let Some(index) = self.upstream_name_to_index.get(&upstream_name).copied() {
                self.function_tools[index] = function_tool;
            }
            return Ok(());
        }

        let index = self.function_tools.len();
        if let Some(namespace) = spec.namespace.as_ref() {
            self.namespace_name_to_upstream_name.insert(
                (namespace.clone(), spec.name.clone()),
                upstream_name.clone(),
            );
        }
        self.upstream_name_to_index
            .insert(upstream_name.clone(), index);
        self.upstream_name_to_spec.insert(upstream_name, spec);
        self.function_tools.push(function_tool);
        Ok(())
    }

    fn add_function_tool(
        &mut self,
        tool: &Value,
        namespace: Option<&str>,
        loaded: bool,
        reserve_private_proxy_name: bool,
    ) -> Result<(), ProxyError> {
        if !loaded && is_deferred_tool(tool) {
            return Ok(());
        }
        let Some(original_name) = responses_tool_name(tool) else {
            return Ok(());
        };
        if reserve_private_proxy_name
            && namespace.is_none()
            && original_name == TOOL_SEARCH_PROXY_NAME
        {
            return Err(reserved_proxy_name_error());
        }
        let upstream_name = namespace
            .map(|namespace| flatten_namespace_tool_name(namespace, &original_name))
            .unwrap_or_else(|| original_name.clone());

        let Some(function_tool) = responses_function_tool_to_chat_tool(tool, &upstream_name) else {
            return Ok(());
        };
        let spec = CodexToolSpec {
            kind: if namespace.is_some() {
                CodexToolKind::Namespace
            } else {
                CodexToolKind::Function
            },
            name: original_name,
            namespace: namespace.map(ToString::to_string),
        };
        self.add_function_proxy(upstream_name, spec, function_tool)
    }

    fn add_custom_tool(
        &mut self,
        tool: &Value,
        loaded: bool,
        reserve_private_proxy_name: bool,
    ) -> Result<(), ProxyError> {
        if !loaded && is_deferred_tool(tool) {
            return Ok(());
        }
        let Some(name) = responses_tool_name(tool) else {
            return Ok(());
        };
        if reserve_private_proxy_name && name == TOOL_SEARCH_PROXY_NAME {
            return Err(reserved_proxy_name_error());
        }
        let description = json!(responses_custom_tool_description(tool));
        let function_tool = json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        CUSTOM_TOOL_INPUT_FIELD: {
                            "type": "string",
                            "description": CUSTOM_TOOL_INPUT_DESCRIPTION
                        }
                    },
                    "required": [CUSTOM_TOOL_INPUT_FIELD]
                }
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::Custom,
            name: name.clone(),
            namespace: None,
        };
        self.add_function_proxy(name, spec, function_tool)
    }

    fn add_tool_search_tool(&mut self) -> Result<(), ProxyError> {
        let function_tool = json!({
            "type": "function",
            "function": {
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
            }
        });
        let spec = CodexToolSpec {
            kind: CodexToolKind::ToolSearch,
            name: TOOL_SEARCH_PROXY_NAME.to_string(),
            namespace: None,
        };
        self.add_function_proxy(TOOL_SEARCH_PROXY_NAME.to_string(), spec, function_tool)
    }

    fn add_namespace_tool(&mut self, tool: &Value, loaded: bool) -> Result<(), ProxyError> {
        if !loaded && is_deferred_tool(tool) {
            return Ok(());
        }
        let Some(namespace) = tool
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(());
        };
        let Some(children) = tool
            .get("tools")
            .or_else(|| tool.get("children"))
            .and_then(Value::as_array)
        else {
            return Ok(());
        };

        for child in children {
            if child.get("type").and_then(Value::as_str) == Some("function") {
                self.add_function_tool(child, Some(namespace), loaded, true)?;
            }
        }
        Ok(())
    }

    fn add_response_tool(
        &mut self,
        tool: &Value,
        loaded: bool,
        enable_codex_private_tools: bool,
        reserve_private_proxy_name: bool,
    ) -> Result<(), ProxyError> {
        match tool {
            Value::String(name) => self.add_custom_tool(
                &json!({
                    "type": "custom",
                    "name": name
                }),
                loaded,
                reserve_private_proxy_name,
            ),
            Value::Object(_) => match tool.get("type").and_then(Value::as_str) {
                Some("function") => {
                    self.add_function_tool(tool, None, loaded, reserve_private_proxy_name)
                }
                Some("custom") => self.add_custom_tool(tool, loaded, reserve_private_proxy_name),
                Some(TOOL_SEARCH_NATIVE_TYPE) if enable_codex_private_tools => {
                    self.add_tool_search_tool()
                }
                Some("namespace") if enable_codex_private_tools => {
                    self.add_namespace_tool(tool, loaded)
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
}

pub(crate) fn build_codex_tool_context_from_request(
    body: &Value,
) -> Result<CodexToolContext, ProxyError> {
    build_tool_context_from_request(body, true)
}

/// Build a flat tool catalog for a standard Responses client without enabling
/// Codex's private `tool_search`, `namespace`, deferred-loading, or
/// `additional_tools` semantics.
pub(crate) fn build_standard_responses_tool_context_from_request(
    body: &Value,
) -> Result<CodexToolContext, ProxyError> {
    build_tool_context_from_request(body, false)
}

fn build_tool_context_from_request(
    body: &Value,
    enable_codex_private_tools: bool,
) -> Result<CodexToolContext, ProxyError> {
    let offers_tool_search = enable_codex_private_tools && request_offers_tool_search(body);
    let mut context = CodexToolContext {
        enable_codex_private_tools,
        ..Default::default()
    };

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            // Without the private search shim there is no loading phase, so
            // preserve legacy behavior and expose the complete catalog.
            context.add_response_tool(
                tool,
                !offers_tool_search,
                enable_codex_private_tools,
                offers_tool_search,
            )?;
        }
    }

    if enable_codex_private_tools {
        match body.get("input") {
            Some(Value::Array(items)) => {
                for item in items {
                    add_direct_tool_carrier(item, &mut context, offers_tool_search)?;
                }
            }
            Some(Value::Object(_)) => {
                add_direct_tool_carrier(&body["input"], &mut context, offers_tool_search)?;
            }
            _ => {}
        }
    }

    Ok(context)
}

fn add_direct_tool_carrier(
    item: &Value,
    context: &mut CodexToolContext,
    reserve_private_proxy_name: bool,
) -> Result<(), ProxyError> {
    if !matches!(
        item.get("type").and_then(Value::as_str),
        Some("tool_search_output" | "additional_tools")
    ) {
        return Ok(());
    }
    if let Some(tools) = item.get("tools").and_then(Value::as_array) {
        for tool in tools {
            context.add_response_tool(tool, true, true, reserve_private_proxy_name)?;
        }
    }
    Ok(())
}

pub(crate) fn request_offers_tool_search(body: &Value) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools.iter().any(|tool| {
                tool.get("type").and_then(Value::as_str) == Some(TOOL_SEARCH_NATIVE_TYPE)
            })
        })
}

pub(crate) fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full_name = format!("{namespace}__{name}");
    if full_name.len() <= CHAT_TOOL_NAME_MAX_LEN {
        return full_name;
    }

    let hash = short_sha256_hex(full_name.as_bytes());
    let suffix = format!("__{hash}");
    let prefix_len = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(suffix.len());
    let mut prefix = String::new();
    for ch in full_name.chars() {
        if prefix.len() + ch.len_utf8() > prefix_len {
            break;
        }
        prefix.push(ch);
    }
    format!("{prefix}{suffix}")
}

fn reserved_proxy_name_error() -> ProxyError {
    ProxyError::TransformError(format!(
        "function name {TOOL_SEARCH_PROXY_NAME} is reserved for the Codex tool_search proxy shim"
    ))
}

fn is_deferred_tool(tool: &Value) -> bool {
    tool.get("defer_loading")
        .or_else(|| tool.get("deferLoading"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn responses_tool_name(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn responses_custom_tool_description(tool: &Value) -> String {
    let mut description = String::new();
    description.push_str(CUSTOM_TOOL_PRESERVED_METADATA_HEADING);
    description.push_str("\n```json\n");
    description.push_str(&canonical_json_string(tool));
    description.push_str("\n```");
    description
}

fn normalize_function_parameters(params: Option<&Value>) -> Value {
    let mut params = match params {
        Some(Value::Object(obj)) => Value::Object(obj.clone()),
        _ => json!({"type": "object", "properties": {}}),
    };
    if let Some(obj) = params.as_object_mut() {
        if obj.get("type").and_then(Value::as_str) != Some("object") {
            obj.insert("type".to_string(), json!("object"));
        }
    }
    params
}

fn responses_function_tool_to_chat_tool(tool: &Value, upstream_name: &str) -> Option<Value> {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }

    if let Some(function) = tool.get("function") {
        let mut function_tool = json!({
            "type": "function",
            "function": function.clone()
        });
        if let Some(obj) = function_tool
            .get_mut("function")
            .and_then(Value::as_object_mut)
        {
            let parameters = normalize_function_parameters(obj.get("parameters"));
            obj.insert("parameters".to_string(), parameters);
            obj.insert("name".to_string(), json!(upstream_name));
            if let Some(strict) = tool.get("strict").cloned() {
                obj.entry("strict".to_string()).or_insert(strict);
            }
        }
        return Some(function_tool);
    }

    let mut function = json!({
        "name": upstream_name,
        "description": tool.get("description").cloned().unwrap_or(Value::Null),
        "parameters": normalize_function_parameters(tool.get("parameters"))
    });
    if let Some(strict) = tool.get("strict") {
        function["strict"] = strict.clone();
    }

    Some(json!({
        "type": "function",
        "function": function
    }))
}

pub(crate) fn response_tool_call_item_id_from_upstream_name(
    call_id: &str,
    upstream_name: &str,
    context: &CodexToolContext,
) -> String {
    if context.is_custom_tool_upstream_name(upstream_name) {
        format!("ctc_{call_id}")
    } else {
        format!("fc_{call_id}")
    }
}

pub(crate) fn response_tool_call_item_from_upstream_name(
    item_id: &str,
    status: &str,
    call_id: &str,
    upstream_name: &str,
    arguments: &str,
    reasoning: Option<&str>,
    context: &CodexToolContext,
) -> Value {
    match context.lookup_upstream_name(upstream_name) {
        Some(spec) if spec.kind == CodexToolKind::ToolSearch => {
            response_tool_search_call_item(call_id, status, arguments, reasoning)
        }
        Some(spec) if spec.kind == CodexToolKind::Custom => response_custom_tool_call_item(
            item_id, status, call_id, &spec.name, arguments, reasoning,
        ),
        Some(spec) => response_function_call_item_with_namespace(
            item_id,
            status,
            call_id,
            &spec.name,
            spec.namespace.as_deref(),
            arguments,
            reasoning,
        ),
        None => response_function_call_item(
            item_id,
            status,
            call_id,
            upstream_name,
            arguments,
            reasoning,
        ),
    }
}

fn response_tool_search_call_item(
    call_id: &str,
    status: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let parsed_arguments = parse_tool_arguments_object(arguments);
    let mut item = json!({
        "type": "tool_search_call",
        "call_id": call_id,
        "status": status,
        "execution": "client",
        "arguments": parsed_arguments
    });
    attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

fn response_custom_tool_call_item(
    item_id: &str,
    status: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
    reasoning: Option<&str>,
) -> Value {
    let input = custom_tool_input_from_upstream_arguments(arguments);
    let mut item = json!({
        "id": item_id,
        "type": "custom_tool_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "input": input
    });
    attach_optional_reasoning_content_field(&mut item, reasoning);
    item
}

fn parse_tool_arguments_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "query": arguments }))
}

pub(crate) fn custom_tool_input_from_upstream_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(obj)) => obj
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(Value::as_str)
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream_names(context: &CodexToolContext) -> Vec<&str> {
        context
            .function_tools()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect()
    }

    #[test]
    fn ordinary_tool_search_coexists_with_private_search_shim() {
        let body = json!({
            "tools": [
                {"type": "tool_search"},
                {"type": "function", "name": "tool_search", "parameters": {}}
            ]
        });

        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(
            upstream_names(&context),
            vec![TOOL_SEARCH_PROXY_NAME, TOOL_SEARCH_NATIVE_TYPE]
        );
        assert_eq!(
            context
                .lookup_upstream_name(TOOL_SEARCH_PROXY_NAME)
                .unwrap()
                .kind,
            CodexToolKind::ToolSearch
        );
        assert_eq!(
            context
                .lookup_upstream_name(TOOL_SEARCH_NATIVE_TYPE)
                .unwrap()
                .kind,
            CodexToolKind::Function
        );
    }

    #[test]
    fn proxy_name_is_reserved_when_private_search_shim_is_published() {
        let body = json!({
            "tools": [
                {"type": "tool_search"},
                {"type": "function", "name": TOOL_SEARCH_PROXY_NAME, "parameters": {}}
            ]
        });
        assert!(build_codex_tool_context_from_request(&body).is_err());
    }

    #[test]
    fn proxy_name_is_available_without_private_search_shim() {
        let body = json!({
            "tools": [{"type": "function", "name": TOOL_SEARCH_PROXY_NAME, "parameters": {}}]
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(upstream_names(&context), vec![TOOL_SEARCH_PROXY_NAME]);
        assert_eq!(
            context
                .lookup_upstream_name(TOOL_SEARCH_PROXY_NAME)
                .unwrap()
                .kind,
            CodexToolKind::Function
        );
    }

    #[test]
    fn forced_choice_requires_the_exact_tool_to_be_published() {
        let context = build_codex_tool_context_from_request(&json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "function",
                    "name": "deferred_function",
                    "defer_loading": true,
                    "parameters": {}
                },
                {"type": "custom", "name": "deferred_custom", "defer_loading": true},
                {"type": "custom", "name": "ready_custom"}
            ]
        }))
        .unwrap();

        assert!(context
            .require_published_response_function("deferred_function", None)
            .is_err());
        assert!(context
            .require_published_custom_tool("deferred_custom")
            .is_err());
        assert_eq!(
            context
                .require_published_custom_tool("ready_custom")
                .unwrap(),
            "ready_custom"
        );
        assert_eq!(
            context.require_published_tool_search().unwrap(),
            TOOL_SEARCH_PROXY_NAME
        );
    }

    #[test]
    fn function_and_custom_name_collision_is_rejected_in_both_orders() {
        for tools in [
            json!([
                {"type": "function", "name": "same", "parameters": {}},
                {"type": "custom", "name": "same"}
            ]),
            json!([
                {"type": "custom", "name": "same"},
                {"type": "function", "name": "same", "parameters": {}}
            ]),
        ] {
            assert!(build_codex_tool_context_from_request(&json!({"tools": tools})).is_err());
        }
    }

    #[test]
    fn namespace_and_top_level_flat_name_collision_is_rejected() {
        let body = json!({
            "tools": [
                {"type": "function", "name": "mcp__mail____search", "parameters": {}},
                {
                    "type": "namespace",
                    "name": "mcp__mail__",
                    "tools": [{"type": "function", "name": "search", "parameters": {}}]
                }
            ]
        });
        assert!(build_codex_tool_context_from_request(&body).is_err());
    }

    #[test]
    fn deferred_catalog_is_omitted_until_loaded() {
        let body = json!({
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
                        "parameters": {"type": "object", "properties": {"stale": {"type": "string"}}}
                    }]
                }
            ],
            "input": [{
                "type": "tool_search_output",
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

        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(
            upstream_names(&context),
            vec![TOOL_SEARCH_PROXY_NAME, "mcp__mail____search"]
        );
        assert_eq!(
            context.function_tools()[1]["function"]["parameters"]["properties"]["query"]["type"],
            "string"
        );
        assert!(
            context.function_tools()[1]["function"]["parameters"]["properties"]
                .get("stale")
                .is_none()
        );
    }

    #[test]
    fn additional_tools_are_loaded() {
        let body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "additional_tools",
                "tools": [{"type": "function", "name": "loaded", "parameters": {}}]
            }]
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(
            upstream_names(&context),
            vec![TOOL_SEARCH_PROXY_NAME, "loaded"]
        );
    }

    #[test]
    fn single_input_tool_carrier_is_loaded() {
        let body = json!({
            "tools": [{"type": "tool_search"}],
            "input": {
                "type": "tool_search_output",
                "tools": [{"type": "function", "name": "loaded", "parameters": {}}]
            }
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(
            upstream_names(&context),
            vec![TOOL_SEARCH_PROXY_NAME, "loaded"]
        );
    }

    #[test]
    fn discovered_copy_replaces_stale_catalog_schema() {
        let body = json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "function",
                    "name": "lookup",
                    "parameters": {"type": "object", "properties": {"stale": {"type": "string"}}}
                }
            ],
            "input": [{
                "type": "additional_tools",
                "tools": [{
                    "type": "function",
                    "name": "lookup",
                    "parameters": {"type": "object", "properties": {"query": {"type": "string"}}}
                }]
            }]
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();
        let lookup = &context.function_tools()[1]["function"]["parameters"]["properties"];
        assert_eq!(lookup["query"]["type"], "string");
        assert!(lookup.get("stale").is_none());
    }

    #[test]
    fn nested_business_json_is_not_scanned_for_tool_carriers() {
        let body = json!({
            "tools": [{"type": "tool_search"}],
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "metadata": {
                        "type": "additional_tools",
                        "tools": [{"type": "function", "name": "not_a_real_tool"}]
                    }
                }]
            }]
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(upstream_names(&context), vec![TOOL_SEARCH_PROXY_NAME]);
    }

    #[test]
    fn response_identity_restores_only_the_proxy_search_name() {
        let body = json!({
            "tools": [
                {"type": "tool_search"},
                {"type": "function", "name": "tool_search", "parameters": {}}
            ]
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();

        let proxy = response_tool_call_item_from_upstream_name(
            "fc_proxy",
            "completed",
            "call_proxy",
            TOOL_SEARCH_PROXY_NAME,
            "{\"query\":\"mail\"}",
            None,
            &context,
        );
        assert_eq!(proxy["type"], "tool_search_call");
        assert_eq!(proxy["arguments"]["query"], "mail");

        let native = response_tool_call_item_from_upstream_name(
            "fc_native",
            "completed",
            "call_native",
            TOOL_SEARCH_NATIVE_TYPE,
            "{}",
            None,
            &context,
        );
        assert_eq!(native["type"], "function_call");
        assert_eq!(native["name"], TOOL_SEARCH_NATIVE_TYPE);
    }

    #[test]
    fn namespaced_child_may_use_proxy_bare_name() {
        let body = json!({
            "tools": [{
                "type": "namespace",
                "name": "mcp__tools__",
                "tools": [{
                    "type": "function",
                    "name": TOOL_SEARCH_PROXY_NAME,
                    "parameters": {}
                }]
            }]
        });
        let context = build_codex_tool_context_from_request(&body).unwrap();
        assert_eq!(
            upstream_names(&context),
            vec!["mcp__tools____ccswitch_tool_search"]
        );
    }

    #[test]
    fn standard_responses_context_does_not_enable_codex_private_tools() {
        let body = json!({
            "tools": [
                {"type": "tool_search"},
                {
                    "type": "namespace",
                    "name": "mcp__private__",
                    "tools": [{"type": "function", "name": "hidden", "parameters": {}}]
                },
                {
                    "type": "function",
                    "name": "deferred_but_standard",
                    "defer_loading": true,
                    "parameters": {}
                },
                {
                    "type": "function",
                    "name": TOOL_SEARCH_PROXY_NAME,
                    "parameters": {}
                }
            ],
            "input": [{
                "type": "additional_tools",
                "tools": [{"type": "function", "name": "private_loaded", "parameters": {}}]
            }]
        });

        let context = build_standard_responses_tool_context_from_request(&body).unwrap();
        assert!(!context.uses_codex_private_tools());
        assert_eq!(
            upstream_names(&context),
            vec!["deferred_but_standard", TOOL_SEARCH_PROXY_NAME]
        );

        let restored = response_tool_call_item_from_upstream_name(
            "fc_standard",
            "completed",
            "call_standard",
            TOOL_SEARCH_PROXY_NAME,
            "{}",
            None,
            &context,
        );
        assert_eq!(restored["type"], "function_call");
        assert_eq!(restored["name"], TOOL_SEARCH_PROXY_NAME);
    }
}
