//! xAI (Grok) native Responses compatibility for Codex Desktop.
//!
//! Codex 0.142+ sends `wire_api="responses"` requests carrying a handful of
//! OpenAI-backend-private fields and tool carriers that xAI's strict
//! `api.x.ai/v1/responses` serde parser rejects (HTTP 400/422). cc-switch's
//! Chat/Anthropic transforms already drop these on the way through, but the
//! *native* Responses passthrough forwards the body verbatim, so we scrub them
//! here.
//!
//! Request-side [`sanitize_xai_responses_request`] is a faithful port of
//! sub2api's `patchGrokResponsesBody`
//! (`backend/internal/service/openai_gateway_grok.go`) plus the #6815 schema
//! collapse for root `oneOf`/`anyOf` function parameters. Field removal and
//! that schema rewrite stay in that function. Collaboration `agent_message`
//! items and return-path whole-float tool argument rewrites are separate
//! entry points so they cannot be mistaken for "just delete a field". Gated on
//! [`super::codex::provider_needs_responses_namespace_flatten`], which covers
//! xAI OAuth *and* API-key cards whose live upstream is `api.x.ai` Responses.
//!
//! Run request sanitizers *after* namespace flattening: by then Codex's
//! `namespace` tools are already lifted to top-level `function` tools, so the
//! tool-type whitelist below keeps them instead of dropping them.
//!
//! Isolation for upstream rebases: keep every xAI-only rewrite in this file.
//! `forwarder` should call [`apply_xai_native_responses_request_compat`] once;
//! `handlers` should only wrap SSE/non-stream restore. The live gate is
//! [`super::codex::provider_needs_responses_namespace_flatten`]. If upstream
//! later lands equivalent sanitization, delete this layer rather than
//! dual-pathing.

use std::collections::{HashMap, HashSet};

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Number, Value};

use super::transform_codex_responses_namespace::{restore_sse_event_namespaces, NamespacedName};
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

/// Codex plugin-private fields removed recursively at any nesting depth.
const RECURSIVE_UNSUPPORTED_FIELDS: &[&str] = &["external_web_access"];

/// Top-level request fields xAI rejects regardless of model.
const TOP_LEVEL_UNSUPPORTED_FIELDS: &[&str] = &["prompt_cache_retention", "safety_identifier"];

/// Top-level sampling fields rejected specifically by grok-4.5.
const GROK_45_UNSUPPORTED_FIELDS: &[&str] = &[
    "presence_penalty",
    "presencePenalty",
    "frequency_penalty",
    "frequencyPenalty",
    "stop",
];

/// Tool `type` values xAI's Responses schema accepts. Sourced from xAI's own
/// serde error enumeration (which is more complete than sub2api's hand-copied
/// list — it includes `image_generation`). Any other `type` is a Codex/OpenAI
/// private carrier (`tool_search`, a stray `namespace`, `custom`, …) that the
/// strict parser would reject, so it is dropped.
const XAI_SUPPORTED_TOOL_TYPES: &[&str] = &[
    "function",
    "web_search",
    "x_search",
    "image_generation",
    "collections_search",
    "file_search",
    "code_execution",
    "code_interpreter",
    "mcp",
    "shell",
];

/// Strip xAI-unsupported fields and tools from a native Codex Responses request
/// body in place. Returns whether anything changed. Deterministic and
/// idempotent: running it twice on the same body changes nothing the second
/// time.
pub(crate) fn sanitize_xai_responses_request(body: &mut Value) -> bool {
    if !body.is_object() {
        return false;
    }

    let mut changed = false;

    // 1. Top-level fields xAI rejects for every model.
    for field in TOP_LEVEL_UNSUPPORTED_FIELDS {
        changed |= remove_top_level_field(body, field);
    }

    // 2. grok-4.5 additionally rejects these sampling knobs.
    if request_targets_grok_45(body) {
        for field in GROK_45_UNSUPPORTED_FIELDS {
            changed |= remove_top_level_field(body, field);
        }
    }

    // 3. Codex plugin-private flags buried at any depth (e.g. inside tools or
    //    tool parameter schemas).
    for field in RECURSIVE_UNSUPPORTED_FIELDS {
        changed |= remove_field_recursive(body, field);
    }

    // 4. Lift the `additional_tools` input carrier (Responses Lite private
    //    shape) up to top-level `tools` so the supported ones survive.
    changed |= promote_additional_tools(body);

    // 5. Drop `content: null` on reasoning input items — xAI's untagged enum
    //    deserializer refuses a present-but-null content field.
    changed |= strip_null_reasoning_content(body);

    // 6. Whitelist the tool types and clean a now-dangling `tool_choice`.
    changed |= filter_unsupported_tools(body);

    // 7. Normalize function tool parameter schemas that xAI rejects before
    //    sampling starts — notably Codex's built-in `automation_update`, which
    //    arrives flattened as `mcp__codex_app__automation_update` with a root
    //    `oneOf`/`anyOf` union containing a `null` branch. The Responses→Chat
    //    bridge already coerces these, but the native Responses passthrough did
    //    not until this step.
    changed |= normalize_xai_function_tool_parameter_schemas(body);

    changed
}

/// Whether the request's (possibly provider-prefixed) model resolves to
/// grok-4.5. Mirrors sub2api's suffix match: `foo/grok-4.5` counts.
fn request_targets_grok_45(body: &Value) -> bool {
    let Some(model) = body.get("model").and_then(Value::as_str) else {
        return false;
    };
    let mut model = model.trim();
    if let Some(idx) = model.rfind('/') {
        model = model[idx + 1..].trim();
    }
    model.eq_ignore_ascii_case("grok-4.5")
}

fn remove_top_level_field(body: &mut Value, field: &str) -> bool {
    body.as_object_mut()
        .and_then(|obj| obj.remove(field))
        .is_some()
}

/// Delete every occurrence of `field` in the tree, at any depth.
fn remove_field_recursive(value: &mut Value, field: &str) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = map.remove(field).is_some();
            for child in map.values_mut() {
                changed |= remove_field_recursive(child, field);
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for child in items.iter_mut() {
                changed |= remove_field_recursive(child, field);
            }
            changed
        }
        _ => false,
    }
}

fn is_additional_tools_item(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str).map(str::trim) == Some("additional_tools")
}

/// Promote any `additional_tools` carrier items from `input` into top-level
/// `tools`, preserving top-level order and appending carrier tools in order,
/// de-duplicated. The carrier items themselves are removed from `input`.
fn promote_additional_tools(body: &mut Value) -> bool {
    // Clone `input` up front so the later mutable write-back to `body` doesn't
    // collide with the read borrow. Only pays the clone on the rare carrier path.
    let input_items: Vec<Value> = match body.get("input").and_then(Value::as_array) {
        Some(arr) if arr.iter().any(is_additional_tools_item) => arr.clone(),
        _ => return false,
    };

    // Seed merged tools + dedup keys from the existing top-level tools.
    let mut merged: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            seen.insert(tool_dedup_key(tool));
            merged.push(tool.clone());
        }
    }

    let mut filtered_input: Vec<Value> = Vec::with_capacity(input_items.len());
    let mut promoted = false;
    for item in input_items {
        if is_additional_tools_item(&item) {
            if let Some(carrier_tools) = item.get("tools").and_then(Value::as_array) {
                for tool in carrier_tools {
                    if seen.insert(tool_dedup_key(tool)) {
                        merged.push(tool.clone());
                        promoted = true;
                    }
                }
            }
            continue; // carrier item dropped regardless of dedup outcome
        }
        filtered_input.push(item);
    }

    if let Some(obj) = body.as_object_mut() {
        obj.insert("input".to_string(), Value::Array(filtered_input));
        if promoted {
            obj.insert("tools".to_string(), Value::Array(merged));
        }
    }
    // We reached here only because a carrier existed, so `input` changed.
    true
}

/// Stable dedup key for a tool: `(type, name)`, `(mcp, server_label)`, or the
/// serialized tool as a last resort. Mirrors sub2api's `grokResponsesToolDedupKey`.
fn tool_dedup_key(tool: &Value) -> String {
    let tool_type = tool
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !tool_type.is_empty() {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            let name = name.trim();
            if !name.is_empty() {
                return format!("type:{tool_type}\u{0}name:{name}");
            }
        }
        if tool_type == "mcp" {
            if let Some(label) = tool.get("server_label").and_then(Value::as_str) {
                let label = label.trim();
                if !label.is_empty() {
                    return format!("type:mcp\u{0}server_label:{label}");
                }
            }
        }
    }
    format!("json:{tool}")
}

fn strip_null_reasoning_content(body: &mut Value) -> bool {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for item in input.iter_mut() {
        if item.get("type").and_then(Value::as_str).map(str::trim) != Some("reasoning") {
            continue;
        }
        if let Some(obj) = item.as_object_mut() {
            if matches!(obj.get("content"), Some(Value::Null)) {
                obj.remove("content");
                changed = true;
            }
        }
    }
    changed
}

/// Keep only whitelisted tool types and drop a `tool_choice` that now points at
/// a removed or unsupported tool.
fn filter_unsupported_tools(body: &mut Value) -> bool {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return false;
    };
    let original_len = tools.len();
    let filtered: Vec<Value> = tools
        .iter()
        .filter(|tool| {
            let t = tool
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            XAI_SUPPORTED_TOOL_TYPES.contains(&t)
        })
        .cloned()
        .collect();

    let mut changed = false;
    if filtered.len() != original_len {
        if let Some(obj) = body.as_object_mut() {
            if filtered.is_empty() {
                obj.remove("tools");
            } else {
                obj.insert("tools".to_string(), Value::Array(filtered.clone()));
            }
        }
        changed = true;
    }

    if body.get("tool_choice").is_some() && should_drop_tool_choice(body, &filtered) {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("tool_choice");
        }
        changed = true;
    }

    changed
}

/// Whether `tool_choice` should be dropped given the surviving `tools`. String
/// choices (`"auto"`, `"none"`, `"required"`) are always kept; object choices
/// are dropped when they reference an unsupported type or a function name that
/// no longer exists.
fn should_drop_tool_choice(body: &Value, tools: &[Value]) -> bool {
    let Some(tool_choice) = body.get("tool_choice") else {
        return false;
    };
    if tools.is_empty() {
        return true;
    }
    let Some(choice) = tool_choice.as_object() else {
        return false; // "auto"/"none"/"required" string choices stay
    };
    let choice_type = choice
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if choice_type.is_empty() {
        return false;
    }
    if !XAI_SUPPORTED_TOOL_TYPES.contains(&choice_type) {
        return true;
    }
    if choice_type == "function" {
        let choice_name = choice
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| {
                choice
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("")
            .trim();
        if choice_name.is_empty() {
            return false;
        }
        let exists = tools.iter().any(|tool| {
            let t = tool
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| {
                    tool.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("")
                .trim();
            t == "function" && name == choice_name
        });
        return !exists;
    }
    false
}

/// Whether a Responses function tool declares parameters that xAI's strict
/// validator rejects before sampling begins.
fn xai_function_parameters_need_simplification(params: &Value) -> bool {
    match params {
        Value::Null => true,
        Value::Object(obj) if obj.is_empty() => true,
        Value::Object(obj) => {
            match obj.get("type") {
                None | Some(Value::Null) => return true,
                Some(Value::String(type_name)) if type_name != "object" => return true,
                _ => {}
            }

            for union_key in ["oneOf", "anyOf"] {
                let Some(branches) = obj.get(union_key).and_then(Value::as_array) else {
                    continue;
                };
                if branches.is_empty() {
                    continue;
                }
                if branches
                    .iter()
                    .any(|branch| branch.get("type").and_then(Value::as_str) != Some("object"))
                {
                    return true;
                }
            }

            false
        }
        _ => true,
    }
}

/// Collapse a root-level `oneOf`/`anyOf` union into a plain object schema.
fn flatten_union_branches_to_object(branches: &[Value]) -> Value {
    let object_branches: Vec<&Value> = branches
        .iter()
        .filter(|branch| branch.get("type").and_then(Value::as_str) == Some("object"))
        .collect();

    if object_branches.len() == 1 {
        let mut result = object_branches[0].clone();
        if let Some(obj) = result.as_object_mut() {
            obj.insert("type".to_string(), json!("object"));
            obj.entry("properties".to_string())
                .or_insert_with(|| json!({}));
        }
        return result;
    }

    if !object_branches.is_empty() {
        let mut merged_properties = Map::new();
        // Intersect `required` across branches: the union means "one of these
        // shapes", so a field only stays mandatory if every branch demands it.
        // A union of the lists would turn the "or" into an "and" and force the
        // model to emit fields the chosen branch does not have.
        let mut merged_required: Option<Vec<Value>> = None;
        for branch in object_branches {
            if let Some(properties) = branch.get("properties").and_then(Value::as_object) {
                for (key, value) in properties {
                    merged_properties
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            let branch_required = branch
                .get("required")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            merged_required = Some(match merged_required {
                None => branch_required,
                Some(existing) => existing
                    .into_iter()
                    .filter(|item| branch_required.contains(item))
                    .collect(),
            });
        }

        let mut result = json!({
            "type": "object",
            "properties": Value::Object(merged_properties),
        });
        let merged_required = merged_required.unwrap_or_default();
        if !merged_required.is_empty() {
            result["required"] = Value::Array(merged_required);
        }
        return result;
    }

    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

/// Rewrite a function tool's JSON Schema parameters into an xAI-compatible root
/// object schema.
fn simplify_xai_function_parameters(params: Option<&Value>) -> Value {
    match params {
        None | Some(Value::Null) => {
            json!({"type": "object", "properties": {}, "additionalProperties": true})
        }
        Some(Value::Object(obj)) if obj.is_empty() => {
            json!({"type": "object", "properties": {}, "additionalProperties": true})
        }
        Some(Value::Object(obj)) => {
            for union_key in ["oneOf", "anyOf"] {
                if let Some(branches) = obj.get(union_key).and_then(Value::as_array) {
                    if branches
                        .iter()
                        .any(|branch| branch.get("type").and_then(Value::as_str) != Some("object"))
                    {
                        return flatten_union_branches_to_object(branches);
                    }
                }
            }

            let mut result = Value::Object(obj.clone());
            if let Some(obj) = result.as_object_mut() {
                match obj.get("type").and_then(Value::as_str) {
                    Some("object") => {}
                    _ => {
                        obj.insert("type".to_string(), json!("object"));
                        obj.entry("properties".to_string())
                            .or_insert_with(|| json!({}));
                    }
                }
            }
            result
        }
        _ => json!({"type": "object", "properties": {}, "additionalProperties": true}),
    }
}

fn function_tool_name(tool: &Value) -> &str {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .trim()
}

fn is_automation_update_tool(name: &str) -> bool {
    name == "codex_app__automation_update"
        || name == "mcp__codex_app__automation_update"
        || name.ends_with("__automation_update")
}

fn rewrite_function_tool_parameters(tool: &mut Value, params: Option<&Value>) -> bool {
    let simplified = simplify_xai_function_parameters(params);
    if params == Some(&simplified) {
        return false;
    }

    if let Some(obj) = tool.as_object_mut() {
        if obj.contains_key("parameters") || params.is_some() {
            obj.insert("parameters".to_string(), simplified);
            return true;
        }
        if let Some(function) = obj.get_mut("function").and_then(Value::as_object_mut) {
            function.insert("parameters".to_string(), simplified);
            return true;
        }
    }

    false
}

fn xai_safe_empty_object_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": true})
}

fn normalize_xai_function_tool_parameters(tool: &mut Value) -> bool {
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return false;
    }

    // Codex Desktop always injects automation_update with a root oneOf/anyOf
    // that includes a non-object (null) branch. xAI rejects the whole turn
    // (farion1231/cc-switch#6815). Keep the tool callable, but force a plain
    // object root the way CLIProxyAPI does.
    if is_automation_update_tool(function_tool_name(tool)) {
        let safe = xai_safe_empty_object_schema();
        let needs_rewrite = {
            let current = tool.get("parameters").or_else(|| {
                tool.get("function")
                    .and_then(|function| function.get("parameters"))
            });
            current != Some(&safe)
        };
        let mut changed = needs_rewrite;
        if let Some(obj) = tool.as_object_mut() {
            if needs_rewrite {
                if obj.contains_key("parameters") || obj.get("function").is_none() {
                    obj.insert("parameters".to_string(), safe);
                } else if let Some(function) =
                    obj.get_mut("function").and_then(Value::as_object_mut)
                {
                    function.insert("parameters".to_string(), safe);
                }
            }
            if obj.get("strict") == Some(&json!(true)) {
                obj.insert("strict".to_string(), json!(false));
                changed = true;
            }
            if let Some(function) = obj.get_mut("function").and_then(Value::as_object_mut) {
                if function.get("strict") == Some(&json!(true)) {
                    function.insert("strict".to_string(), json!(false));
                    changed = true;
                }
            }
        }
        return changed;
    }

    let params = tool
        .get("parameters")
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("parameters"))
        })
        .cloned();

    let changed = match params.as_ref() {
        Some(params) if xai_function_parameters_need_simplification(params) => {
            rewrite_function_tool_parameters(tool, Some(params))
        }
        None => rewrite_function_tool_parameters(tool, None),
        _ => false,
    };

    if changed && is_automation_update_tool(function_tool_name(tool)) {
        if let Some(obj) = tool.as_object_mut() {
            if obj.get("strict") == Some(&json!(true)) {
                obj.insert("strict".to_string(), json!(false));
            }
            if let Some(function) = obj.get_mut("function").and_then(Value::as_object_mut) {
                if function.get("strict") == Some(&json!(true)) {
                    function.insert("strict".to_string(), json!(false));
                }
            }
        }
    }

    changed
}

fn normalize_xai_function_tool_parameter_schemas(body: &mut Value) -> bool {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };

    let mut changed = false;
    for tool in tools.iter_mut() {
        changed |= normalize_xai_function_tool_parameters(tool);
    }
    changed
}

/// One request-side entry point for the xAI native Responses gate.
///
/// `forwarder` stays a thin `if provider_needs_responses_namespace_flatten`
/// call so `git rebase upstream/main` conflicts here or at that single site,
/// not across inlined sanitizers. Logging stays here for the same reason.
pub(crate) fn apply_xai_native_responses_request_compat(
    body: &mut Value,
    provider_id: &str,
    upstream_model: Option<&str>,
    settings: &Value,
) {
    // Remap the model before sanitizing: grok-4.5-specific field stripping
    // keys off `body.model`, so an unknown subagent SKU must land on its final
    // name first or those fields survive the strip.
    if let Some(upstream_model) = upstream_model {
        let allowed = collect_xai_catalog_model_ids(settings);
        if let Some((from, to)) = rewrite_xai_unknown_request_model(body, upstream_model, &allowed)
        {
            log::info!(
                "[Codex] Rewrote xAI-unknown request model {from} -> {to} (provider={provider_id})"
            );
        }
    }
    if sanitize_xai_responses_request(body) {
        log::debug!("[Codex] Sanitized xAI-unsupported Responses fields (provider={provider_id})");
    }
    if rewrite_xai_agent_message_input_items(body) {
        log::info!(
            "[Codex] Rewrote xAI-unsupported agent_message input items (provider={provider_id})"
        );
    }
}

/// Rewrite Codex multi-agent v2 `agent_message` items into ordinary `message`
/// items. xAI's Responses `ModelInput` enum has no `agent_message` variant, so
/// a native passthrough 422s (`input[N]: unknown item type "agent_message"`)
/// before the child agent can run.
///
/// Walk the whole request body, not only the top-level `input` array: Codex
/// may nest the same item under later collaboration turns. Keep this out of
/// [`sanitize_xai_responses_request`]: it is a structural rewrite, not a field
/// deletion. Routed Grok sessions currently put plaintext task bodies in
/// `encrypted_content` parts; flatten those to `input_text`.
pub(crate) fn rewrite_xai_agent_message_input_items(body: &mut Value) -> bool {
    rewrite_agent_message_value(body)
}

fn rewrite_agent_message_value(value: &mut Value) -> bool {
    if rewrite_agent_message_item(value) {
        return true;
    }
    match value {
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= rewrite_agent_message_value(item);
            }
            changed
        }
        Value::Object(obj) => {
            let mut changed = false;
            for child in obj.values_mut() {
                changed |= rewrite_agent_message_value(child);
            }
            changed
        }
        _ => false,
    }
}

/// Remap a request `model` that xAI will not serve onto the provider's
/// configured model (the live main-agent model). Catalog `model`/`slug`/`id`
/// values are preserved so a user who picked `grok-4.5` is not forced onto
/// `grok-4.6`. Unknown OpenAI role SKUs such as `gpt-5.6-sol` are rewritten.
///
/// Returns `Some((from, to))` when the field changed. Missing or empty
/// `model` is filled with `upstream_model`. An empty upstream model is a
/// no-op so we never invent a name.
pub(crate) fn rewrite_xai_unknown_request_model(
    body: &mut Value,
    upstream_model: &str,
    allowed_models: &HashSet<String>,
) -> Option<(String, String)> {
    let upstream = upstream_model.trim();
    if upstream.is_empty() {
        return None;
    }

    let obj = body.as_object_mut()?;
    let request = obj
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    if !request.is_empty() && request_model_is_allowed(&request, upstream, allowed_models) {
        return None;
    }

    obj.insert("model".to_string(), Value::String(upstream.to_string()));
    Some((request, upstream.to_string()))
}

/// Collect catalog model identifiers from a Codex provider's settings.
/// Grok catalogs store the live id on `slug`; other providers may use
/// `model` or `id`. All three are accepted so a catalog hit is not missed.
pub(crate) fn collect_xai_catalog_model_ids(settings: &Value) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
    else {
        return ids;
    };
    for entry in models {
        for key in ["model", "slug", "id"] {
            if let Some(id) = entry
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

fn request_model_is_allowed(
    request: &str,
    upstream: &str,
    allowed_models: &HashSet<String>,
) -> bool {
    request.eq_ignore_ascii_case(upstream)
        || request_is_grok_model(request)
        || allowed_models
            .iter()
            .any(|id| id.eq_ignore_ascii_case(request))
}

/// Whether the request names a Grok-family model, optionally provider-prefixed
/// (`xai/grok-4.6-fast`). Real Grok SKUs the catalog has not caught up with —
/// a brand-new model, or one hand-picked via Codex `/model` on a card without
/// a catalog — must pass through; only alien subagent SKUs are remapped.
fn request_is_grok_model(request: &str) -> bool {
    let mut bare = request.trim();
    if let Some(idx) = bare.rfind('/') {
        bare = bare[idx + 1..].trim();
    }
    // Byte-wise prefix check: a `bare[..4]` str slice would panic when byte 4
    // splits a multi-byte code point (e.g. a CJK model name).
    bare.as_bytes()
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"grok"))
}

fn json_type(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str).map(str::trim)
}

fn rewrite_agent_message_item(item: &mut Value) -> bool {
    if json_type(item) != Some("agent_message") {
        return false;
    }

    let id = item.get("id").cloned();
    let content = flatten_agent_message_content(item.get("content"));
    let mut message = json!({
        "type": "message",
        "role": "user",
        "content": content,
    });
    if let Some(id) = id {
        message["id"] = id;
    }
    *item = message;
    true
}

fn flatten_agent_message_content(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::Array(parts)) => parts.iter().filter_map(part_to_input_text).collect(),
        Some(Value::String(text)) if !text.is_empty() => vec![input_text_part(text)],
        _ => Vec::new(),
    }
}

fn part_to_input_text(part: &Value) -> Option<Value> {
    let text = if json_type(part) == Some("encrypted_content") {
        part.get("encrypted_content")
            .or_else(|| part.get("text"))
            .and_then(Value::as_str)
    } else {
        part.get("text").and_then(Value::as_str)
    }?;
    if text.is_empty() {
        None
    } else {
        Some(input_text_part(text))
    }
}

fn input_text_part(text: &str) -> Value {
    json!({ "type": "input_text", "text": text })
}

/// Rewrite whole-number JSON floats (`92116.0`) to integers (`92116`) on
/// completed function-call argument payloads. Grok emits JSON Number floats for
/// integer tool fields; Codex Desktop then fails local serde (`expected i32` /
/// `expected u64`) and never runs the tool.
///
/// Applies only to `response.function_call_arguments.done` and completed
/// `function_call` items. SSE `*.delta` fragments are left untouched because
/// they are not complete JSON. Parse/rewrite failures pass the original bytes
/// through so Codex still surfaces the error — never replace arguments with
/// `{}` or otherwise swallow the failure.
pub(crate) fn normalize_xai_function_call_integer_arguments(value: &mut Value) -> bool {
    normalize_xai_function_call_integer_arguments_value(value)
}

fn normalize_xai_function_call_integer_arguments_value(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= normalize_xai_function_call_integer_arguments_value(item);
            }
            changed
        }
        Value::Object(obj) => {
            let event_type = obj
                .get("type")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if event_type.as_deref() == Some("response.function_call_arguments.delta") {
                return false;
            }

            let mut changed = false;
            if event_type.as_deref() == Some("response.function_call_arguments.done")
                || event_type.as_deref() == Some("function_call")
            {
                changed |= normalize_function_call_arguments_field(obj);
            }
            for child in obj.values_mut() {
                changed |= normalize_xai_function_call_integer_arguments_value(child);
            }
            changed
        }
        _ => false,
    }
}

fn normalize_function_call_arguments_field(obj: &mut Map<String, Value>) -> bool {
    match obj.get_mut("arguments") {
        Some(Value::String(arguments)) => match rewrite_whole_float_arguments_json(arguments) {
            Ok(Some(rewritten)) => {
                *arguments = rewritten;
                true
            }
            Ok(None) => false,
            Err(error) => {
                log::debug!(
                    "[Codex] xAI function_call arguments were not rewritten; passing through unchanged: {error}"
                );
                false
            }
        },
        Some(other) => rewrite_whole_number_floats(other),
        None => false,
    }
}

fn rewrite_whole_float_arguments_json(
    arguments: &str,
) -> Result<Option<String>, serde_json::Error> {
    let mut value: Value = serde_json::from_str(arguments)?;
    if !rewrite_whole_number_floats(&mut value) {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string(&value)?))
}

fn rewrite_whole_number_floats(value: &mut Value) -> bool {
    match value {
        Value::Number(number) => {
            if let Some(integer) = whole_float_to_json_int(number) {
                *number = integer;
                true
            } else {
                false
            }
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= rewrite_whole_number_floats(item);
            }
            changed
        }
        Value::Object(map) => {
            let mut changed = false;
            for child in map.values_mut() {
                changed |= rewrite_whole_number_floats(child);
            }
            changed
        }
        _ => false,
    }
}

/// Convert a JSON Number that is a finite whole float (`92116.0`) into an
/// integer Number. Non-whole values (`1.5`), actual integers, infinities, and
/// values that cannot round-trip stay unchanged.
fn whole_float_to_json_int(number: &Number) -> Option<Number> {
    if number.is_i64() || number.is_u64() {
        return None;
    }
    let float = number.as_f64()?;
    if !float.is_finite() || float.fract() != 0.0 {
        return None;
    }
    if float >= 0.0 {
        // `u64::MAX as f64` rounds up to 2^64, so `>=` (not `>`): at exactly
        // 2^64 the `as u64` cast below saturates to u64::MAX, which rounds back
        // to 2^64 and slips through the round-trip check — a silent off-by-one.
        // (The negative arm is safe: `i64::MIN as f64` is exactly -2^63.)
        if float >= u64::MAX as f64 {
            return None;
        }
        let integer = float as u64;
        if integer as f64 != float {
            return None;
        }
        Some(Number::from(integer))
    } else {
        if float < i64::MIN as f64 {
            return None;
        }
        let integer = float as i64;
        if integer as f64 != float {
            return None;
        }
        Some(Number::from(integer))
    }
}

/// Wrap a native Responses SSE byte stream: restore flattened namespace names
/// and rewrite completed function-call argument JSON. Delta fragments that are
/// not complete JSON pass through unchanged.
pub(crate) fn create_xai_native_responses_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    restore_map: HashMap<String, NamespacedName>,
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
                        yield Ok(rewrite_xai_native_sse_block(&block, &restore_map));
                    }
                }
                Err(e) => {
                    yield Err(std::io::Error::other(e.to_string()));
                    return;
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            yield Ok(rewrite_xai_native_sse_block(&tail, &restore_map));
        }
    }
}

fn rewrite_xai_native_sse_block(
    block: &str,
    restore_map: &HashMap<String, NamespacedName>,
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
        Err(_) => return Bytes::from(format!("{block}\n\n")),
    };

    let mut changed = restore_sse_event_namespaces(&mut event, restore_map);
    changed |= normalize_xai_function_call_integer_arguments(&mut event);
    if !changed {
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
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn strips_external_web_access_recursively() {
        let mut body = json!({
            "model": "grok-4.5",
            "external_web_access": true,
            "tools": [
                {"type": "function", "name": "f", "external_web_access": true,
                 "parameters": {"type": "object", "q": {"external_web_access": true}}}
            ],
            "metadata": {"external_web_access": false}
        });
        assert!(sanitize_xai_responses_request(&mut body));
        let s = body.to_string();
        assert!(!s.contains("external_web_access"), "left over: {s}");
    }

    #[test]
    fn strips_top_level_unsupported_fields() {
        let mut body = json!({
            "model": "grok-4.5",
            "prompt_cache_retention": "24h",
            "safety_identifier": "abc"
        });
        assert!(sanitize_xai_responses_request(&mut body));
        assert!(body.get("prompt_cache_retention").is_none());
        assert!(body.get("safety_identifier").is_none());
    }

    #[test]
    fn strips_grok_45_only_sampling_fields() {
        let mut body = json!({
            "model": "grok-4.5",
            "presence_penalty": 0.1,
            "frequency_penalty": 0.2,
            "stop": ["x"]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("frequency_penalty").is_none());
        assert!(body.get("stop").is_none());
    }

    #[test]
    fn keeps_sampling_fields_for_non_grok_45() {
        let mut body = json!({
            "model": "grok-4-fast",
            "presence_penalty": 0.1,
            "stop": ["x"]
        });
        // No unsupported fields present, so no change and knobs preserved.
        assert!(!sanitize_xai_responses_request(&mut body));
        assert_eq!(body.get("presence_penalty"), Some(&json!(0.1)));
        assert_eq!(body.get("stop"), Some(&json!(["x"])));
    }

    #[test]
    fn matches_grok_45_with_provider_prefix() {
        let mut body = json!({"model": "xai/grok-4.5", "stop": ["x"]});
        assert!(sanitize_xai_responses_request(&mut body));
        assert!(body.get("stop").is_none());
    }

    #[test]
    fn promotes_additional_tools_dedup() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "function", "name": "kept"}],
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "additional_tools", "tools": [
                    {"type": "function", "name": "kept"},
                    {"type": "function", "name": "extra"}
                ]}
            ]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        // carrier removed from input
        let input = body.get("input").unwrap().as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert!(input.iter().all(|i| !is_additional_tools_item(i)));
        // extra promoted, kept not duplicated
        let tools = body.get("tools").unwrap().as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t.get("name").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(names, vec!["kept", "extra"]);
    }

    #[test]
    fn strips_null_reasoning_content() {
        let mut body = json!({
            "model": "grok-4.5",
            "input": [
                {"type": "reasoning", "content": null, "id": "r1"},
                {"type": "reasoning", "content": [{"text": "keep"}], "id": "r2"}
            ]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        let input = body.get("input").unwrap().as_array().unwrap();
        assert!(input[0].get("content").is_none());
        assert!(input[1].get("content").is_some());
    }

    #[test]
    fn filters_unsupported_tool_types() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [
                {"type": "function", "name": "f"},
                {"type": "tool_search"},
                {"type": "custom", "name": "c"},
                {"type": "mcp", "server_label": "s"}
            ]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        let types: Vec<&str> = body
            .get("tools")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.get("type").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(types, vec!["function", "mcp"]);
    }

    #[test]
    fn drops_dangling_function_tool_choice() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "tool_search"}],
            "tool_choice": {"type": "function", "name": "gone"}
        });
        assert!(sanitize_xai_responses_request(&mut body));
        // tool_search filtered → no tools → tool_choice dropped
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn keeps_valid_function_tool_choice() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "function", "name": "run"}],
            "tool_choice": {"type": "function", "name": "run"}
        });
        assert!(!sanitize_xai_responses_request(&mut body));
        assert_eq!(
            body.get("tool_choice").unwrap(),
            &json!({"type": "function", "name": "run"})
        );
    }

    #[test]
    fn keeps_string_tool_choice() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "function", "name": "run"}],
            "tool_choice": "auto"
        });
        assert!(!sanitize_xai_responses_request(&mut body));
        assert_eq!(body.get("tool_choice").unwrap(), &json!("auto"));
    }

    #[test]
    fn noop_on_clean_request() {
        let mut body = json!({
            "model": "grok-4.5",
            "input": [{"type": "message", "role": "user", "content": "hi"}],
            "tools": [{"type": "function", "name": "f"}]
        });
        assert!(!sanitize_xai_responses_request(&mut body));
    }

    #[test]
    fn idempotent_second_pass() {
        let mut body = json!({
            "model": "grok-4.5",
            "external_web_access": true,
            "prompt_cache_retention": "24h",
            "tools": [{"type": "function", "name": "f"}, {"type": "tool_search"}]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        // second pass finds nothing left to change
        assert!(!sanitize_xai_responses_request(&mut body));
    }

    #[test]
    fn simplifies_flattened_automation_update_one_of_null_root() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "function",
                "name": "mcp__codex_app__automation_update",
                "strict": true,
                "parameters": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {"action": {"type": "string"}},
                            "required": ["action"]
                        },
                        {"type": "null"}
                    ]
                }
            }]
        });

        assert!(sanitize_xai_responses_request(&mut body));

        let tool = &body["tools"][0];
        assert_eq!(tool["strict"], json!(false));
        assert_eq!(
            tool["parameters"],
            json!({"type": "object", "properties": {}, "additionalProperties": true})
        );
    }

    #[test]
    fn simplifies_null_tool_parameters_without_touching_valid_tools() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [
                {
                    "type": "function",
                    "name": "codex_app__automation_update",
                    "parameters": null
                },
                {
                    "type": "function",
                    "name": "echo_tool",
                    "parameters": {
                        "type": "object",
                        "properties": {"message": {"type": "string"}}
                    }
                }
            ]
        });

        assert!(sanitize_xai_responses_request(&mut body));

        assert_eq!(
            body["tools"][0]["parameters"],
            json!({"type": "object", "properties": {}, "additionalProperties": true})
        );
        assert_eq!(
            body["tools"][1]["parameters"],
            json!({
                "type": "object",
                "properties": {"message": {"type": "string"}}
            })
        );
    }

    #[test]
    fn union_flatten_intersects_required_across_object_branches() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "function",
                "name": "mcp__custom__multi_shape",
                "parameters": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {"a": {"type": "string"}, "shared": {"type": "string"}},
                            "required": ["a", "shared"]
                        },
                        {
                            "type": "object",
                            "properties": {"b": {"type": "string"}, "shared": {"type": "string"}},
                            "required": ["b", "shared"]
                        },
                        {"type": "null"}
                    ]
                }
            }]
        });

        assert!(sanitize_xai_responses_request(&mut body));
        let params = &body["tools"][0]["parameters"];
        assert_eq!(params["type"], "object");
        // Properties keep the union of both branches…
        assert!(params["properties"].get("a").is_some());
        assert!(params["properties"].get("b").is_some());
        // …but only a field required by every branch stays required.
        assert_eq!(params["required"], json!(["shared"]));
    }

    #[test]
    fn union_flatten_drops_required_when_a_branch_has_none() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "function",
                "name": "mcp__custom__optional_shape",
                "parameters": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {"a": {"type": "string"}},
                            "required": ["a"]
                        },
                        {"type": "object", "properties": {"b": {"type": "string"}}},
                        {"type": "null"}
                    ]
                }
            }]
        });

        assert!(sanitize_xai_responses_request(&mut body));
        let params = &body["tools"][0]["parameters"];
        assert_eq!(params["type"], "object");
        assert!(params.get("required").is_none());
    }

    #[test]
    fn automation_update_schema_normalization_is_idempotent() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "function",
                "name": "mcp__codex_app__automation_update",
                "parameters": {
                    "oneOf": [
                        {"type": "object", "properties": {"action": {"type": "string"}}},
                        {"type": "null"}
                    ]
                }
            }]
        });

        assert!(sanitize_xai_responses_request(&mut body));
        assert!(!sanitize_xai_responses_request(&mut body));
    }

    #[test]
    fn whole_floats_92116_and_120000_become_integers() {
        let mut value: Value =
            serde_json::from_str(r#"{"session_id":92116.0,"yield_time_ms":120000.0,"wait":1.5}"#)
                .unwrap();
        assert!(rewrite_whole_number_floats(&mut value));
        assert_eq!(value["session_id"].as_i64(), Some(92116));
        assert_eq!(value["yield_time_ms"].as_u64(), Some(120000));
        assert_eq!(value["wait"].as_f64(), Some(1.5));
        assert!(value["wait"].as_i64().is_none());

        let encoded = serde_json::to_string(&value).unwrap();
        assert!(encoded.contains(r#""session_id":92116"#));
        assert!(encoded.contains(r#""yield_time_ms":120000"#));
        assert!(!encoded.contains("92116.0"));
        assert!(!encoded.contains("120000.0"));
        assert!(encoded.contains("1.5"));
    }

    #[test]
    fn two_pow_64_whole_float_is_not_rewritten() {
        // 2^64 slips past a `>` guard (`u64::MAX as f64` rounds up to 2^64) and
        // the saturating cast would silently rewrite it to u64::MAX, off by one.
        let mut value: Value = serde_json::from_str(r#"{"big":18446744073709551616.0}"#).unwrap();
        assert!(!rewrite_whole_number_floats(&mut value));
        assert_eq!(value["big"].as_f64(), Some(18_446_744_073_709_551_616.0));

        // The largest representable whole float below 2^64 still converts.
        let mut value: Value = serde_json::from_str(r#"{"big":18446744073709549568.0}"#).unwrap();
        assert!(rewrite_whole_number_floats(&mut value));
        assert_eq!(value["big"].as_u64(), Some(18_446_744_073_709_549_568));
    }

    #[test]
    fn function_call_arguments_done_rewrites_whole_floats_recursively() {
        let mut event = json!({
            "type": "response.function_call_arguments.done",
            "item_id": "fc_exec",
            "arguments": r#"{"session_id":92116.0,"yield_time_ms":120000.0,"nested":{"n":92116.0},"arr":[120000.0,1.5]}"#
        });

        assert!(normalize_xai_function_call_integer_arguments(&mut event));
        let arguments: Value = serde_json::from_str(event["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["session_id"].as_i64(), Some(92116));
        assert_eq!(arguments["yield_time_ms"].as_u64(), Some(120000));
        assert_eq!(arguments["nested"]["n"].as_i64(), Some(92116));
        assert_eq!(arguments["arr"][0].as_u64(), Some(120000));
        assert_eq!(arguments["arr"][1].as_f64(), Some(1.5));
        assert!(arguments["arr"][1].as_i64().is_none());
    }

    #[test]
    fn completed_function_call_item_rewrites_whole_float_arguments() {
        let mut body = json!({
            "output": [{
                "type": "function_call",
                "name": "write_stdin",
                "arguments": r#"{"session_id":92116.0,"yield_time_ms":120000.0}"#
            }]
        });

        assert!(normalize_xai_function_call_integer_arguments(&mut body));
        let arguments: Value =
            serde_json::from_str(body["output"][0]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["session_id"].as_i64(), Some(92116));
        assert_eq!(arguments["yield_time_ms"].as_u64(), Some(120000));
    }

    #[test]
    fn function_call_argument_deltas_are_not_rewritten() {
        let mut event = json!({
            "type": "response.function_call_arguments.delta",
            "delta": r#"{"session_id":92116.0"#,
            "item": {
                "type": "function_call",
                "arguments": r#"{"session_id":92116.0}"#
            }
        });
        let original = event.clone();
        assert!(!normalize_xai_function_call_integer_arguments(&mut event));
        assert_eq!(event, original);
    }

    #[test]
    fn invalid_function_call_arguments_pass_through() {
        let mut event = json!({
            "type": "response.function_call_arguments.done",
            "arguments": r#"{"session_id":92116.0"#
        });
        assert!(!normalize_xai_function_call_integer_arguments(&mut event));
        assert_eq!(event["arguments"], r#"{"session_id":92116.0"#);
    }

    #[test]
    fn sse_done_event_rewrites_whole_floats_but_delta_bytes_stay_intact() {
        let done = concat!(
            "event: response.function_call_arguments.done\n",
            r#"data: {"type":"response.function_call_arguments.done","arguments":"{\"session_id\":92116.0,\"yield_time_ms\":120000.0}"}"#,
        );
        let rewritten = rewrite_xai_native_sse_block(done, &HashMap::new());
        let rewritten = String::from_utf8(rewritten.to_vec()).unwrap();
        let data = rewritten
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap();
        let event: Value = serde_json::from_str(data).unwrap();
        let arguments: Value = serde_json::from_str(event["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["session_id"].as_i64(), Some(92116));
        assert_eq!(arguments["yield_time_ms"].as_u64(), Some(120000));
        assert!(!rewritten.contains("92116.0"));
        assert!(!rewritten.contains("120000.0"));

        let delta = concat!(
            "event: response.function_call_arguments.delta\n",
            r#"data: {"type":"response.function_call_arguments.delta","delta":"{\"session_id\":92116.0"}"#,
        );
        let passed = rewrite_xai_native_sse_block(delta, &HashMap::new());
        assert_eq!(
            String::from_utf8(passed.to_vec()).unwrap(),
            format!("{delta}\n\n")
        );
    }

    #[test]
    fn rewrites_agent_message_new_task_with_encrypted_content_part() {
        let mut body = json!({
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "review"}]},
                {
                    "type": "agent_message",
                    "id": "amsg_child",
                    "author": "/root",
                    "recipient": "/root/code_review",
                    "content": [
                        {"type": "input_text", "text": "Message Type: NEW_TASK\nPayload:\n"},
                        {"type": "encrypted_content", "encrypted_content": "You are a Senior Code Reviewer."}
                    ]
                }
            ]
        });

        assert!(!sanitize_xai_responses_request(&mut body));
        assert!(rewrite_xai_agent_message_input_items(&mut body));
        assert_eq!(body["input"][0]["type"], "message");
        let item = &body["input"][1];
        assert_eq!(item["type"], "message");
        assert_eq!(item["role"], "user");
        assert_eq!(item["id"], "amsg_child");
        assert_eq!(item["content"][0]["type"], "input_text");
        assert_eq!(
            item["content"][0]["text"],
            "Message Type: NEW_TASK\nPayload:\n"
        );
        assert_eq!(
            item["content"][1]["text"],
            "You are a Senior Code Reviewer."
        );
        assert!(item.get("author").is_none());
        assert!(!rewrite_xai_agent_message_input_items(&mut body));
    }

    #[test]
    fn rewrites_agent_message_final_answer_without_dropping_neighbors() {
        let mut body = json!({
            "input": [
                {
                    "type": "function_call",
                    "name": "wait_agent",
                    "arguments": "{\"timeout_ms\":180000}"
                },
                {
                    "type": "agent_message",
                    "id": "amsg_parent",
                    "author": "/root/code_review",
                    "recipient": "/root",
                    "content": [{
                        "type": "input_text",
                        "text": "Message Type: FINAL_ANSWER\nPayload:\nAgent errored: unexpected status 422"
                    }]
                }
            ]
        });

        assert!(rewrite_xai_agent_message_input_items(&mut body));
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][0]["name"], "wait_agent");
        assert_eq!(body["input"][1]["type"], "message");
        assert_eq!(body["input"][1]["role"], "user");
        assert!(body["input"][1]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("FINAL_ANSWER"));
    }

    #[test]
    fn leaves_ordinary_messages_unchanged() {
        let mut body = json!({
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "hello"}]
            }]
        });
        let original = body.clone();
        assert!(!rewrite_xai_agent_message_input_items(&mut body));
        assert_eq!(body, original);
    }

    #[test]
    fn rewrites_agent_message_at_input_index_matching_xai_422() {
        let mut body = json!({
            "model": "grok-4.6",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "go"}]},
                {"type": "function_call", "name": "spawn_agent", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "ok"},
                {"type": "reasoning", "summary": []},
                {
                    "type": "agent_message",
                    "id": "amsg_child",
                    "author": "/root",
                    "recipient": "/root/review_uncommitted",
                    "content": [
                        {"type": "input_text", "text": "Message Type: NEW_TASK\nPayload:\n"},
                        {"type": "encrypted_content", "encrypted_content": "Review the diff."}
                    ]
                }
            ]
        });

        assert!(rewrite_xai_agent_message_input_items(&mut body));
        assert_eq!(body["input"][4]["type"], "message");
        assert_eq!(body["input"][4]["role"], "user");
        assert_eq!(body["input"][4]["content"][1]["text"], "Review the diff.");
        assert!(body["input"][4].get("author").is_none());
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][1]["type"], "function_call");
    }

    #[test]
    fn rewrites_nested_agent_message_inside_object_wrapper() {
        let mut body = json!({
            "input": {
                "items": [{
                    "type": "agent_message",
                    "content": [{"type": "input_text", "text": "nested"}]
                }]
            }
        });
        assert!(rewrite_xai_agent_message_input_items(&mut body));
        assert_eq!(body["input"]["items"][0]["type"], "message");
        assert_eq!(body["input"]["items"][0]["role"], "user");
    }

    #[test]
    fn request_compat_rewrites_agent_message_and_unknown_model_together() {
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "input": [{
                "type": "agent_message",
                "content": [{"type": "input_text", "text": "hi"}]
            }]
        });
        let settings = json!({
            "modelCatalog": {"models": [{"slug": "grok-4.6"}, {"slug": "grok-4.5"}]}
        });
        apply_xai_native_responses_request_compat(&mut body, "grok", Some("grok-4.6"), &settings);
        assert_eq!(body["model"], "grok-4.6");
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["text"], "hi");
    }

    #[test]
    fn request_compat_strips_grok_45_fields_after_model_remap() {
        // The subagent SKU only resolves to grok-4.5 after the remap, so the
        // remap must run before the sanitizer's grok-4.5 field stripping.
        let mut body = json!({
            "model": "gpt-5.6-sol",
            "presence_penalty": 0.5,
            "stop": ["\n"],
            "input": []
        });
        let settings = json!({
            "modelCatalog": {"models": [{"slug": "grok-4.5"}]}
        });
        apply_xai_native_responses_request_compat(&mut body, "grok", Some("grok-4.5"), &settings);
        assert_eq!(body["model"], "grok-4.5");
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("stop").is_none());
    }

    #[test]
    fn remaps_unknown_openai_role_model_to_upstream() {
        let allowed = collect_xai_catalog_model_ids(&json!({
            "modelCatalog": {
                "models": [
                    {"slug": "grok-4.6"},
                    {"slug": "grok-4.5"}
                ]
            }
        }));
        let mut body = json!({"model": "gpt-5.6-sol", "input": []});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            Some(("gpt-5.6-sol".to_string(), "grok-4.6".to_string()))
        );
        assert_eq!(body["model"], "grok-4.6");
    }

    #[test]
    fn preserves_catalog_slug_instead_of_forcing_upstream() {
        let allowed = collect_xai_catalog_model_ids(&json!({
            "modelCatalog": {
                "models": [
                    {"slug": "grok-4.6"},
                    {"slug": "grok-4.5"}
                ]
            }
        }));
        let mut body = json!({"model": "grok-4.5"});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            None
        );
        assert_eq!(body["model"], "grok-4.5");
    }

    #[test]
    fn preserves_grok_prefixed_model_missing_from_catalog() {
        let allowed = collect_xai_catalog_model_ids(&json!({
            "modelCatalog": {"models": [{"slug": "grok-4.6"}]}
        }));

        // A real Grok SKU the catalog has not caught up with passes through.
        let mut body = json!({"model": "grok-4.7-fast"});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            None
        );
        assert_eq!(body["model"], "grok-4.7-fast");

        // Provider-prefixed spelling passes too.
        let mut body = json!({"model": "xai/Grok-4.7-Fast"});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            None
        );

        // Alien subagent SKUs are still remapped.
        let mut body = json!({"model": "luna"});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            Some(("luna".to_string(), "grok-4.6".to_string()))
        );

        // Non-ASCII names must not panic on the prefix check (byte 4 splits a
        // CJK code point) and fall through to the remap.
        let mut body = json!({"model": "模型"});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            Some(("模型".to_string(), "grok-4.6".to_string()))
        );
    }

    #[test]
    fn fills_missing_model_with_upstream() {
        let allowed = HashSet::new();
        let mut body = json!({"input": []});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "grok-4.6", &allowed),
            Some(("".to_string(), "grok-4.6".to_string()))
        );
        assert_eq!(body["model"], "grok-4.6");
    }

    #[test]
    fn leaves_model_unchanged_when_upstream_is_empty() {
        let allowed = HashSet::new();
        let mut body = json!({"model": "gpt-5.6-sol"});
        assert_eq!(
            rewrite_xai_unknown_request_model(&mut body, "  ", &allowed),
            None
        );
        assert_eq!(body["model"], "gpt-5.6-sol");
    }
}
