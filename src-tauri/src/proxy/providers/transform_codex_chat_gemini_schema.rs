//! Gemini schema compatibility for the Codex Responses → Chat bridge.
//!
//! Gateways that serve Gemini behind a Chat Completions endpoint (and, judging
//! by third-party reports, Google's own OpenAI-compatible endpoint) translate
//! `tools[].function.parameters` into Gemini's legacy
//! `functionDeclarations[].parameters` field. That field is an OpenAPI subset
//! whose `enum` only takes strings, so a valid JSON Schema such as
//! `{"type": "boolean", "enum": [true]}` (Codex Desktop's
//! `mcp__codex_app.create_worktree.allowAsync`, loaded through `tool_search`)
//! fails the whole turn with `enum[0] (TYPE_STRING)` (#9168, #10032). Probed
//! 2026-11-26 through a Vertex-backed gateway: the legacy field rejects the
//! shape, `parametersJsonSchema` accepts it, but a Chat gateway decides which
//! field it uses, not CC Switch.
//!
//! The rewrite only drops the constraint the field cannot express: an `enum`
//! with non-string values loses those values. On a `string` node the string
//! values are kept; on any other node the `enum` is removed, the declared
//! `type` stays, and the allowed values are appended to `description` so the
//! model still sees them.
//!
//! Scope is deliberately narrow: only the Responses → Chat path, and only when
//! the upstream model is a Gemini model (matched by name, so relays that serve
//! Gemini under their own host are covered). Every other model keeps
//! byte-identical tool schemas, so prompt-cache prefixes are not disturbed.

use serde_json::Value;

/// Keywords whose value is a map from caller-chosen names to schemas.
const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
    "dependencies",
];

/// Keywords whose value is a single schema (or, for draft-07 `items`, a tuple
/// of schemas).
const SINGLE_SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "additionalItems",
    "contains",
    "propertyNames",
    "additionalProperties",
    "unevaluatedItems",
    "unevaluatedProperties",
    "contentSchema",
    "not",
    "if",
    "then",
    "else",
];

/// Keywords whose value is a list of schemas.
const SCHEMA_LIST_KEYWORDS: &[&str] = &["prefixItems", "allOf", "anyOf", "oneOf"];

/// The upstream model is a Gemini model. Accepts relay prefixes such as
/// `google/gemini-3.8-flash` and is case-insensitive.
pub(crate) fn is_gemini_model(model: &str) -> bool {
    model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase()
        .starts_with("gemini")
}

/// Adapt every `tools[].function.parameters` of a Chat Completions body whose
/// `model` is a Gemini model. Returns how many tools had their schema changed.
pub(crate) fn adapt_chat_tools_for_gemini(chat_body: &mut Value) -> usize {
    if !chat_body
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(is_gemini_model)
    {
        return 0;
    }
    let Some(tools) = chat_body.get_mut("tools").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut changed = 0;
    for tool in tools.iter_mut() {
        let Some(parameters) = tool
            .get_mut("function")
            .and_then(|function| function.get_mut("parameters"))
        else {
            continue;
        };
        if drop_non_string_enums(parameters) > 0 {
            changed += 1;
        }
    }
    changed
}

/// Walk a JSON Schema through schema-valued keywords only and rewrite every
/// `enum` that carries non-string values. Returns the number of nodes
/// rewritten. Data-valued keywords (`default`, `examples`, `const`) are never
/// entered, and the keys of `properties` are parameter names, not keywords.
fn drop_non_string_enums(schema: &mut Value) -> usize {
    let Value::Object(map) = schema else {
        return 0;
    };
    let mut rewritten = 0;

    let non_string_values = map
        .get("enum")
        .and_then(Value::as_array)
        .filter(|values| values.iter().any(|value| !value.is_string()))
        .cloned();
    if let Some(values) = non_string_values {
        rewritten += 1;
        let strings: Vec<Value> = values.iter().filter(|v| v.is_string()).cloned().collect();
        let string_node = map.get("type").and_then(Value::as_str) == Some("string");
        if string_node && !strings.is_empty() {
            // Non-string members could never match a string anyway.
            map.insert("enum".to_string(), Value::Array(strings));
        } else {
            map.remove("enum");
            if !string_node {
                let allowed = values
                    .iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let hint = format!("Allowed values: {allowed}");
                let description = match map.get("description").and_then(Value::as_str) {
                    Some(existing) if !existing.trim().is_empty() => {
                        format!("{existing}\n{hint}")
                    }
                    _ => hint,
                };
                map.insert("description".to_string(), Value::String(description));
            }
        }
    }

    for key in SCHEMA_MAP_KEYWORDS {
        if let Some(children) = map.get_mut(*key).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                rewritten += drop_non_string_enums(child);
            }
        }
    }
    for key in SINGLE_SCHEMA_KEYWORDS {
        match map.get_mut(*key) {
            Some(Value::Array(children)) => {
                for child in children {
                    rewritten += drop_non_string_enums(child);
                }
            }
            Some(child) => rewritten += drop_non_string_enums(child),
            None => {}
        }
    }
    for key in SCHEMA_LIST_KEYWORDS {
        if let Some(children) = map.get_mut(*key).and_then(Value::as_array_mut) {
            for child in children {
                rewritten += drop_non_string_enums(child);
            }
        }
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat_body(model: &str, parameters: Value) -> Value {
        json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {"name": "t", "parameters": parameters}
            }]
        })
    }

    #[test]
    fn drops_boolean_enum_and_keeps_the_boolean_type() {
        let mut body = chat_body(
            "gemini-3.8-flash",
            json!({
                "type": "object",
                "properties": {"allowAsync": {"type": "boolean", "enum": [true]}},
                "required": ["allowAsync"]
            }),
        );
        assert_eq!(adapt_chat_tools_for_gemini(&mut body), 1);
        assert_eq!(
            body["tools"][0]["function"]["parameters"],
            json!({
                "type": "object",
                "properties": {"allowAsync": {
                    "type": "boolean",
                    "description": "Allowed values: true"
                }},
                "required": ["allowAsync"]
            })
        );
    }

    #[test]
    fn appends_hint_to_an_existing_description() {
        let mut body = chat_body(
            "google/Gemini-4-Argon",
            json!({
                "type": "object",
                "properties": {"level": {
                    "type": "integer",
                    "description": "Retry level.",
                    "enum": [1, 2]
                }}
            }),
        );
        assert_eq!(adapt_chat_tools_for_gemini(&mut body), 1);
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["level"],
            json!({"type": "integer", "description": "Retry level.\nAllowed values: 1, 2"})
        );
    }

    #[test]
    fn keeps_string_members_of_a_mixed_string_enum() {
        let mut body = chat_body(
            "gemini-3.8-flash",
            json!({
                "type": "object",
                "properties": {"mode": {"type": "string", "enum": ["a", 1, "b"]}}
            }),
        );
        assert_eq!(adapt_chat_tools_for_gemini(&mut body), 1);
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["mode"],
            json!({"type": "string", "enum": ["a", "b"]})
        );
    }

    #[test]
    fn reaches_nested_schema_positions() {
        let mut body = chat_body(
            "gemini-3.8-flash",
            json!({
                "type": "object",
                "properties": {
                    "list": {"type": "array", "items": {"type": "boolean", "enum": [false]}},
                    "choice": {"anyOf": [{"type": "integer", "enum": [0]}, {"type": "string"}]}
                },
                "$defs": {"flag": {"type": "boolean", "enum": [true]}}
            }),
        );
        assert_eq!(adapt_chat_tools_for_gemini(&mut body), 1);
        let parameters = &body["tools"][0]["function"]["parameters"];
        assert!(parameters["properties"]["list"]["items"]
            .get("enum")
            .is_none());
        assert!(parameters["properties"]["choice"]["anyOf"][0]
            .get("enum")
            .is_none());
        assert!(parameters["$defs"]["flag"].get("enum").is_none());
    }

    #[test]
    fn leaves_string_enums_parameter_names_and_literal_data_alone() {
        let parameters = json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string", "enum": ["c", "f"]},
                "enum": {"type": "boolean", "default": {"enum": [true]}}
            },
            "required": ["enum"]
        });
        let mut body = chat_body("gemini-3.8-flash", parameters.clone());
        assert_eq!(adapt_chat_tools_for_gemini(&mut body), 0);
        assert_eq!(body["tools"][0]["function"]["parameters"], parameters);
    }

    #[test]
    fn other_models_keep_byte_identical_schemas() {
        for model in ["gpt-5.5", "gemma-4-27b", "kimi-k3", "claude-opus-5-5"] {
            let parameters = json!({
                "type": "object",
                "properties": {"allowAsync": {"type": "boolean", "enum": [true]}}
            });
            let mut body = chat_body(model, parameters.clone());
            assert_eq!(adapt_chat_tools_for_gemini(&mut body), 0, "{model}");
            assert_eq!(body["tools"][0]["function"]["parameters"], parameters);
        }
    }
}
