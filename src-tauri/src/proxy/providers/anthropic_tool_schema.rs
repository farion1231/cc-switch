//! Preserve root composition constraints in an Anthropic-compatible tool object.
//!
//! The wrapper changes only the upstream argument envelope; the Anthropic bridge
//! restores the original arguments on responses and wraps them during replay.

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

pub(crate) const ANTHROPIC_TOOL_ARGS_PROPERTY: &str = "__cc_switch_args";
const WRAPPED_SCHEMA_POINTER: &str = "#/properties/__cc_switch_args";

#[derive(Clone, Copy)]
enum SchemaDialect {
    Unspecified,
    Draft6,
    Draft7,
    Draft201909,
    Draft202012,
}

impl SchemaDialect {
    fn from_declaration(declaration: &Value, tool_name: &str) -> Result<Self, ProxyError> {
        let path = declaration.as_str().and_then(|dialect| {
            let dialect = dialect.trim_end_matches('#');
            dialect
                .strip_prefix("https://json-schema.org/")
                .or_else(|| dialect.strip_prefix("http://json-schema.org/"))
        });
        match path {
            Some("draft-06/schema") => Ok(Self::Draft6),
            Some("draft-07/schema") => Ok(Self::Draft7),
            Some("draft/2019-09/schema") => Ok(Self::Draft201909),
            Some("draft/2020-12/schema") => Ok(Self::Draft202012),
            // Older dialects use `id`; custom vocabularies can introduce
            // schema positions that this walker cannot identify safely.
            _ => Err(unsupported_scope(tool_name, "$schema")),
        }
    }

    fn applies_ref_siblings(self) -> bool {
        matches!(self, Self::Draft201909 | Self::Draft202012)
    }
}

pub(crate) fn needs_argument_wrapper(schema: &Value) -> bool {
    ["oneOf", "anyOf", "allOf"]
        .iter()
        .any(|keyword| schema.get(keyword).is_some())
}

/// Keep the complete schema together so `oneOf` exclusivity and constraints
/// depending on evaluated properties retain their original meaning.
pub(crate) fn wrap_root_composition_schema(
    mut schema: Value,
    tool_name: &str,
) -> Result<Value, ProxyError> {
    if !needs_argument_wrapper(&schema) {
        return Ok(schema);
    }

    let dialect = schema.get("$schema").cloned();
    let has_resource_id = schema.get("$id").is_some();
    let original_schema = schema.clone();
    rebase_local_references(
        &mut schema,
        &original_schema,
        true,
        !has_resource_id,
        SchemaDialect::Unspecified,
        tool_name,
    )?;
    // A dialect declaration belongs at a schema resource root. With no $id,
    // the moved schema is now part of the wrapper's resource (Core §8.1.1).
    if !has_resource_id {
        schema.as_object_mut().unwrap().remove("$schema");
    }
    let mut wrapped = json!({
        "type": "object",
        "properties": { ANTHROPIC_TOOL_ARGS_PROPERTY: schema },
        "required": [ANTHROPIC_TOOL_ARGS_PROPERTY],
        "additionalProperties": false
    });
    if let Some(dialect) = dialect {
        wrapped["$schema"] = dialect;
    }
    Ok(wrapped)
}

fn unsupported_scope(tool_name: &str, keyword: &str) -> ProxyError {
    ProxyError::InvalidRequest(format!(
        "Cannot preserve schema reference scope for tool {tool_name:?} with {keyword} while wrapping a root composition"
    ))
}

/// A URI fragment is decoded before applying JSON Pointer's ~0/~1 escapes.
fn decode_pointer_fragment(fragment: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(fragment.len());
    let mut bytes = fragment.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = (bytes.next()? as char).to_digit(16)?;
            let low = (bytes.next()? as char).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).ok()
}

/// A pointer can target an otherwise ignored extension or literal value and
/// cause a validator to interpret it as a schema. Such locations cannot be
/// rewritten without also changing instance data, so accept only paths that
/// the schema walker below actually visits.
fn pointer_targets_schema(mut schema: &Value, pointer: &str) -> bool {
    if pointer.is_empty() {
        return schema.is_object() || schema.is_boolean();
    }
    let Some(pointer) = pointer.strip_prefix('/') else {
        return false;
    };
    let mut tokens = pointer
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"));
    while let Some(keyword) = tokens.next() {
        let Some(child) = schema.get(&keyword) else {
            return false;
        };
        schema = match keyword.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas"
            | "dependencies" => {
                let Some(name) = tokens.next() else {
                    return false;
                };
                let Some(child) = child.as_object().and_then(|children| children.get(&name)) else {
                    return false;
                };
                child
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" | "items" if child.is_array() => {
                let Some(index) = tokens.next() else {
                    return false;
                };
                if index.starts_with('0') && index.len() > 1 {
                    return false;
                }
                let Some(child) = index
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| child.get(index))
                else {
                    return false;
                };
                child
            }
            "items"
            | "additionalItems"
            | "contains"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "propertyNames"
            | "not"
            | "if"
            | "then"
            | "else"
            | "contentSchema" => child,
            _ => return false,
        };
    }
    schema.is_object() || schema.is_boolean()
}

/// JSON Schema Core §8.2.1 makes a subschema with `$id` a distinct resource:
/// its local pointers stay relative to that resource, while pointers in the
/// original anonymous resource must follow the moved schema. Anchors are
/// location-independent (§8.2.2), so `#anchor` references do not change.
fn rebase_local_references(
    schema: &mut Value,
    original_schema: &Value,
    mut rebase: bool,
    anonymous_root: bool,
    inherited_dialect: SchemaDialect,
    tool_name: &str,
) -> Result<(), ProxyError> {
    let Some(object) = schema.as_object_mut() else {
        return Ok(());
    };
    // Embedded resources inherit their enclosing resource's dialect unless
    // they declare their own (2020-12 Core §9.3.2). Keep this value local so an
    // override in one branch never affects a sibling schema.
    let dialect = match object.get("$schema") {
        Some(declaration) => SchemaDialect::from_declaration(declaration, tool_name)?,
        None => inherited_dialect,
    };
    // These reference forms require dynamic-scope handling. Plain `id` is
    // an annotation in every supported dialect; only an unspecified dialect
    // leaves ambiguity about whether it has the legacy Draft 4 meaning.
    // Fail explicitly instead of forwarding a silently weakened/broken schema.
    for keyword in ["$dynamicRef", "$recursiveRef"] {
        if object.contains_key(keyword) {
            return Err(unsupported_scope(tool_name, keyword));
        }
    }
    if matches!(dialect, SchemaDialect::Unspecified) && object.contains_key("id") {
        return Err(unsupported_scope(tool_name, "id"));
    }
    // Draft 6/7 ignore siblings of $ref, including $id. Treating that $id as
    // a resource boundary would change the meaning of the reference. Since
    // 2019-09, $ref is an applicator and $id still establishes resource scope.
    // Without a declared dialect, retain the conservative scope check.
    if !dialect.applies_ref_siblings() && object.contains_key("$id") && object.contains_key("$ref")
    {
        return Err(unsupported_scope(tool_name, "$id alongside $ref"));
    }
    if let Some(id) = object.get("$id") {
        let valid_id = id.as_str().is_some_and(|id| {
            !id.is_empty()
                && !id.starts_with('#')
                && id
                    .split_once('#')
                    .is_none_or(|(_, fragment)| fragment.is_empty())
        });
        if !valid_id {
            return Err(unsupported_scope(tool_name, "$id"));
        }
        rebase = false;
    }
    if let Some(Value::String(reference)) = object.get_mut("$ref") {
        // Without an explicit original root ID, its retrieval URI is unknown.
        // A non-fragment reference (even inside a named child resource) could
        // point back to that moved root. Do not guess whether it is external.
        if anonymous_root && !reference.is_empty() && !reference.starts_with('#') {
            return Err(unsupported_scope(
                tool_name,
                "non-local $ref without a root $id",
            ));
        }
        if rebase {
            if reference.is_empty() || reference == "#" {
                *reference = WRAPPED_SCHEMA_POINTER.to_string();
            } else if let Some(fragment) = reference.strip_prefix('#') {
                let pointer = decode_pointer_fragment(fragment)
                    .ok_or_else(|| unsupported_scope(tool_name, "invalid $ref URI fragment"))?;
                // JSON Pointer URI fragments may encode the initial slash.
                // Preserve the rest verbatim, including ~ escapes and % escapes.
                if pointer.starts_with('/') {
                    if !pointer_targets_schema(original_schema, &pointer) {
                        return Err(unsupported_scope(
                            tool_name,
                            "$ref outside known schema locations",
                        ));
                    }
                    *reference = format!("{WRAPPED_SCHEMA_POINTER}{fragment}");
                }
            }
        }
    }

    // Walk schema positions only. In particular, a literal `$ref` in defaults,
    // examples, const, enum, a property name, or an extension is instance data.
    for (keyword, child) in object.iter_mut() {
        match keyword.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas"
            | "dependencies" => {
                if let Some(children) = child.as_object_mut() {
                    for child in children.values_mut() {
                        rebase_local_references(
                            child,
                            original_schema,
                            rebase,
                            anonymous_root,
                            dialect,
                            tool_name,
                        )?;
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(children) = child.as_array_mut() {
                    for child in children {
                        rebase_local_references(
                            child,
                            original_schema,
                            rebase,
                            anonymous_root,
                            dialect,
                            tool_name,
                        )?;
                    }
                }
            }
            "items" if child.is_array() => {
                for child in child.as_array_mut().unwrap() {
                    rebase_local_references(
                        child,
                        original_schema,
                        rebase,
                        anonymous_root,
                        dialect,
                        tool_name,
                    )?;
                }
            }
            "items"
            | "additionalItems"
            | "contains"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "propertyNames"
            | "not"
            | "if"
            | "then"
            | "else"
            | "contentSchema" => {
                rebase_local_references(
                    child,
                    original_schema,
                    rebase,
                    anonymous_root,
                    dialect,
                    tool_name,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::proxy::error::ProxyError;
    use serde_json::{json, Value};

    fn convert(schema: Value) -> Result<Value, ProxyError> {
        let converted = super::super::transform_codex_anthropic::responses_request_to_anthropic(
            json!({
                "model": "claude",
                "input": [{ "role": "user", "content": "test" }],
                "tools": [{ "type": "function", "name": "union_tool", "parameters": schema }]
            }),
            4096,
        )?;
        Ok(converted["tools"][0]["input_schema"].clone())
    }

    #[test]
    fn issue_7459_root_compositions_preserve_complete_schema_inside_required_argument() {
        for keyword in ["oneOf", "anyOf", "allOf"] {
            let mut original = json!({
                "type": "object",
                "properties": { "mode": { "type": "string" } },
                "required": ["mode"],
                "unevaluatedProperties": false,
                "not": { "required": ["forbidden"] }
            });
            original[keyword] = json!([
                { "properties": { "mode": { "const": "first" } }, "required": ["mode"] },
                { "properties": { "mode": { "const": "second" } }, "required": ["mode"] }
            ]);
            let converted = convert(original.clone()).unwrap();
            assert_eq!(converted["type"], "object");
            for forbidden in ["oneOf", "anyOf", "allOf"] {
                assert!(converted.get(forbidden).is_none(), "root {forbidden}");
            }
            assert_eq!(converted["required"], json!(["__cc_switch_args"]));
            assert_eq!(converted["additionalProperties"], false);
            assert_eq!(converted["properties"]["__cc_switch_args"], original);
        }
    }

    #[test]
    fn issue_7459_simultaneous_root_compositions_and_one_of_overlap_are_not_flattened() {
        let original = json!({
            "type": "object",
            "oneOf": [
                { "properties": { "x": { "type": "string" } }, "required": ["x"] },
                { "properties": { "y": { "type": "string" } }, "required": ["y"] }
            ],
            "anyOf": [{ "required": ["x"] }, { "required": ["z"] }],
            "allOf": [{ "maxProperties": 3 }, { "not": { "required": ["excluded"] } }]
        });
        // In particular, {x: "x", y: "y"} must still fail oneOf: keeping both
        // full branches preserves exclusivity, whereas merging properties cannot.
        let converted = convert(original.clone()).unwrap();
        assert_eq!(converted["properties"]["__cc_switch_args"], original);
    }

    #[test]
    fn issue_7459_ordinary_tool_schema_and_nested_compositions_are_unchanged() {
        let original = json!({
            "type": "object",
            "properties": {
                "value": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                "__cc_switch_args": { "type": "string" }
            },
            "required": ["value"],
            "additionalProperties": false
        });
        assert_eq!(convert(original.clone()).unwrap(), original);
    }

    #[test]
    fn issue_7459_wrapped_schema_rebases_local_pointer_and_recursive_root_references() {
        let original = json!({
            "type": "object",
            "$defs": { "value": { "type": "string" } },
            "oneOf": [{ "required": ["value"] }, { "required": ["child"] }],
            "properties": {
                "value": { "$ref": "#/$defs/value" },
                "child": { "$ref": "#" },
                "empty": { "$ref": "" },
                "branch": { "$ref": "#/oneOf/0" },
                "escaped": { "$ref": "#%2F$defs%2Fvalue" }
            }
        });
        let converted = convert(original).unwrap();
        let inner = &converted["properties"]["__cc_switch_args"];
        assert_eq!(
            inner["properties"]["value"]["$ref"],
            "#/properties/__cc_switch_args/$defs/value"
        );
        assert_eq!(
            inner["properties"]["child"]["$ref"],
            "#/properties/__cc_switch_args"
        );
        assert_eq!(
            inner["properties"]["empty"]["$ref"],
            "#/properties/__cc_switch_args"
        );
        assert_eq!(
            inner["properties"]["branch"]["$ref"],
            "#/properties/__cc_switch_args/oneOf/0"
        );
        assert_eq!(
            inner["properties"]["escaped"]["$ref"],
            "#/properties/__cc_switch_args%2F$defs%2Fvalue"
        );
    }

    #[test]
    fn issue_7459_wrapped_schema_keeps_anchors_and_nested_id_resource_scopes() {
        let child = json!({
            "$id": "child.json",
            "$defs": { "value": { "type": "string" } },
            "properties": {
                "value": { "$ref": "#/$defs/value" },
                "self": { "$ref": "#" }
            }
        });
        let original = json!({
            "type": "object",
            "anyOf": [{ "required": ["value"] }],
            "$defs": { "value": { "$anchor": "value", "type": "string" }, "child": child },
            "properties": { "value": { "$ref": "#value" } }
        });
        let converted = convert(original).unwrap();
        let inner = &converted["properties"]["__cc_switch_args"];
        assert_eq!(inner["$defs"]["child"], child);
        assert_eq!(inner["properties"]["value"]["$ref"], "#value");
    }

    #[test]
    fn issue_7459_wrapped_schema_with_own_root_id_keeps_its_reference_scope() {
        let original = json!({
            "$id": "https://example.test/tool.json",
            "type": "object",
            "$defs": { "value": { "type": "string" } },
            "properties": {
                "external": { "$ref": "https://example.test/schema.json#/$defs/value" },
                "relative": { "$ref": "other.json" }
            },
            "allOf": [{ "properties": { "value": { "$ref": "#/$defs/value" } } }]
        });
        assert_eq!(
            convert(original.clone()).unwrap()["properties"]["__cc_switch_args"],
            original
        );
    }

    #[test]
    fn issue_7459_reference_lookalikes_in_instance_values_and_extensions_are_unchanged() {
        let literal = json!({ "$ref": "#/$defs/value", "$dynamicRef": "#root" });
        let original = json!({
            "type": "object",
            "oneOf": [{ "required": ["value"] }],
            "$defs": { "value": { "type": "string" } },
            "properties": {
                "$ref": { "type": "string" },
                "value": { "const": literal, "default": literal, "examples": [literal], "enum": [literal] }
            },
            "x-metadata": literal
        });
        assert_eq!(
            convert(original.clone()).unwrap()["properties"]["__cc_switch_args"],
            original
        );
    }

    #[test]
    fn issue_7459_references_are_rebased_through_schema_valued_keywords() {
        let reference = json!({ "$ref": "#/$defs/value" });
        let rewritten = json!({ "$ref": "#/properties/__cc_switch_args/$defs/value" });
        for (keyword, child, expected) in [
            ("allOf", json!([reference]), json!([rewritten])),
            ("anyOf", json!([reference]), json!([rewritten])),
            ("oneOf", json!([reference]), json!([rewritten])),
            ("prefixItems", json!([reference]), json!([rewritten])),
            ("items", reference.clone(), rewritten.clone()),
            ("items", json!([reference]), json!([rewritten])),
            ("additionalItems", reference.clone(), rewritten.clone()),
            ("contains", reference.clone(), rewritten.clone()),
            ("additionalProperties", reference.clone(), rewritten.clone()),
            (
                "unevaluatedProperties",
                reference.clone(),
                rewritten.clone(),
            ),
            ("unevaluatedItems", reference.clone(), rewritten.clone()),
            ("propertyNames", reference.clone(), rewritten.clone()),
            ("not", reference.clone(), rewritten.clone()),
            ("if", reference.clone(), rewritten.clone()),
            ("then", reference.clone(), rewritten.clone()),
            ("else", reference.clone(), rewritten.clone()),
            ("contentSchema", reference.clone(), rewritten.clone()),
            (
                "properties",
                json!({ "value": reference }),
                json!({ "value": rewritten }),
            ),
            (
                "patternProperties",
                json!({ "value": reference }),
                json!({ "value": rewritten }),
            ),
            (
                "definitions",
                json!({ "value": reference }),
                json!({ "value": rewritten }),
            ),
            (
                "dependentSchemas",
                json!({ "value": reference }),
                json!({ "value": rewritten }),
            ),
            (
                "dependencies",
                json!({ "value": reference, "other": ["value"] }),
                json!({ "value": rewritten, "other": ["value"] }),
            ),
        ] {
            let mut original = json!({
                "type": "object",
                "oneOf": [{ "required": ["value"] }],
                "$defs": { "value": { "type": "string" } }
            });
            original[keyword] = child;
            let converted = convert(original).unwrap();
            assert_eq!(
                converted["properties"]["__cc_switch_args"][keyword], expected,
                "{keyword}"
            );
        }
    }

    #[test]
    fn issue_7459_ambiguous_recursive_reference_scopes_fail_with_tool_name() {
        for keyword in ["$dynamicRef", "$recursiveRef"] {
            let mut original = json!({
                "type": "object",
                "oneOf": [{ "required": ["value"] }],
                "properties": { "value": {} }
            });
            original["properties"]["value"][keyword] = json!("#");
            let error = convert(original).unwrap_err().to_string();
            assert!(error.contains("union_tool"));
            assert!(error.contains(keyword));
        }
    }

    #[test]
    fn issue_7459_schema_dialect_remains_at_a_schema_resource_root() {
        let dialect = "https://json-schema.org/draft/2020-12/schema";
        let original = json!({
            "$schema": dialect,
            "type": "object",
            "oneOf": [{ "required": ["value"] }]
        });
        let converted = convert(original.clone()).unwrap();
        assert_eq!(converted["$schema"], dialect);
        assert!(converted["properties"]["__cc_switch_args"]
            .get("$schema")
            .is_none());

        let mut identified = original.clone();
        identified["$id"] = json!("https://example.test/tool.json");
        let converted = convert(identified.clone()).unwrap();
        assert_eq!(converted["$schema"], dialect);
        assert_eq!(converted["properties"]["__cc_switch_args"], identified);
    }

    #[test]
    fn issue_7459_legacy_and_ambiguous_resource_identifiers_fail_with_tool_name() {
        for metadata in [
            json!({ "$schema": "http://json-schema.org/draft-04/schema#", "id": "root.json" }),
            json!({ "$id": "" }),
            json!({ "$id": "#" }),
            json!({ "$id": "#anchor" }),
        ] {
            let mut original = json!({ "type": "object", "oneOf": [{ "required": ["value"] }] });
            original
                .as_object_mut()
                .unwrap()
                .extend(metadata.as_object().unwrap().clone());
            let error = convert(original).unwrap_err().to_string();
            assert!(error.contains("union_tool"));
        }
    }

    #[test]
    fn issue_7459_anonymous_root_rejects_nonlocal_references_with_unknown_base() {
        for reference in ["other.json", "https://example.test/root.json#/$defs/value"] {
            for nested_resource in [false, true] {
                let mut child = json!({ "$ref": reference });
                if nested_resource {
                    child["$id"] = json!("child.json");
                }
                let original = json!({
                    "type": "object",
                    "$defs": { "value": { "type": "string" } },
                    "oneOf": [{ "required": ["value"] }],
                    "properties": { "value": child }
                });
                let error = convert(original).unwrap_err().to_string();
                assert!(error.contains("union_tool"));
                assert!(error.contains("$ref"));
            }
        }
    }

    #[test]
    fn issue_7459_unsupported_dialects_do_not_misinterpret_id_as_resource_scope() {
        for dialect in [
            "http://json-schema.org/draft-04/schema#",
            "http://json-schema.org/draft-03/schema#",
            "https://example.test/custom-schema",
        ] {
            let original = json!({
                "$schema": dialect,
                "$id": "https://example.test/ignored-in-draft04.json",
                "type": "object",
                "definitions": { "value": { "type": "string" } },
                "oneOf": [{ "properties": { "value": { "$ref": "#/definitions/value" } } }]
            });
            let error = convert(original).unwrap_err().to_string();
            assert!(error.contains("union_tool"));
            assert!(error.contains("$schema"));
        }
    }

    #[test]
    fn issue_7459_known_schema_dialects_are_preserved() {
        for dialect in [
            "http://json-schema.org/draft-06/schema#",
            "http://json-schema.org/draft-07/schema#",
            "https://json-schema.org/draft/2019-09/schema",
            "https://json-schema.org/draft/2020-12/schema",
        ] {
            let original = json!({
                "$schema": dialect,
                "type": "object",
                "definitions": { "value": { "type": "string" } },
                "oneOf": [{ "properties": { "value": { "$ref": "#/definitions/value" } } }]
            });
            let converted = convert(original).unwrap();
            assert_eq!(converted["$schema"], dialect);
            assert_eq!(
                converted["properties"]["__cc_switch_args"]["oneOf"][0]["properties"]["value"]
                    ["$ref"],
                "#/properties/__cc_switch_args/definitions/value"
            );
        }
    }

    #[test]
    fn issue_7459_pointers_into_non_schema_locations_fail_without_changing_literals() {
        for reference in ["#/x-schema", "#/%78-schema", "#/default", "#/examples/0"] {
            let original = json!({
                "type": "object",
                "anyOf": [{ "required": ["value"] }],
                "definitions": { "text": { "type": "string" } },
                "properties": { "value": { "$ref": reference } },
                "x-schema": { "$ref": "#/definitions/text" },
                "default": { "$ref": "#/definitions/text" },
                "examples": [{ "$ref": "#/definitions/text" }]
            });
            let error = convert(original).unwrap_err().to_string();
            assert!(error.contains("union_tool"));
            assert!(error.contains("$ref"));
        }
    }

    #[test]
    fn issue_7459_id_siblings_of_ref_fail_instead_of_changing_draft7_scope() {
        for dialect in [
            "http://json-schema.org/draft-06/schema#",
            "http://json-schema.org/draft-07/schema#",
        ] {
            let original = json!({
                "$schema": dialect,
                "type": "object",
                "anyOf": [{ "required": ["value"] }],
                "definitions": { "text": { "type": "string" } },
                "properties": { "value": { "$id": "child.json", "$ref": "#/definitions/text" } }
            });
            let error = convert(original).unwrap_err().to_string();
            assert!(error.contains("union_tool"));
            assert!(error.contains("$id"));
            assert!(error.contains("$ref"));
        }
    }

    #[test]
    fn issue_7459_f2_modern_root_id_and_ref_keep_sibling_constraints() {
        for dialect in [
            "https://json-schema.org/draft/2019-09/schema",
            "https://json-schema.org/draft/2020-12/schema",
        ] {
            let original = json!({
                "$schema": dialect,
                "$id": "https://example.test/tool",
                "$ref": "#/$defs/base",
                "allOf": [{ "maxProperties": 1 }],
                "$defs": {
                    "base": {
                        "type": "object",
                        "properties": { "value": { "type": "string" } },
                        "required": ["value"],
                        "additionalProperties": false
                    }
                }
            });
            let wrapped =
                super::wrap_root_composition_schema(original.clone(), "union_tool").unwrap();
            assert_eq!(wrapped["$schema"], dialect);
            assert_eq!(wrapped["properties"]["__cc_switch_args"], original);

            // Request conversion already normalizes tool parameters to object.
            // Assert that this existing normalization is the only difference.
            let converted = convert(original.clone()).unwrap();
            let mut normalized = original;
            normalized["type"] = json!("object");
            assert_eq!(converted["$schema"], dialect);
            assert_eq!(converted["properties"]["__cc_switch_args"], normalized);
        }
    }

    #[test]
    fn issue_7459_f2_nested_resources_inherit_modern_ref_sibling_semantics() {
        for dialect in [
            "https://json-schema.org/draft/2019-09/schema",
            "https://json-schema.org/draft/2020-12/schema",
        ] {
            for identified_root in [false, true] {
                let child = json!({
                    "$id": "https://example.test/child",
                    "$ref": "#/$defs/text",
                    "$defs": { "text": { "type": "string" } },
                    "maxLength": 3
                });
                let mut original = json!({
                    "$schema": dialect,
                    "allOf": [{ "required": ["value"] }],
                    "properties": { "value": child }
                });
                if identified_root {
                    original["$id"] = json!("https://example.test/root");
                }
                let converted = convert(original).unwrap();
                assert_eq!(
                    converted["properties"]["__cc_switch_args"]["properties"]["value"],
                    child
                );
            }
        }
    }

    #[test]
    fn issue_7459_f2_nested_dialect_override_is_inherited_without_leaking_to_siblings() {
        for modern in [
            "https://json-schema.org/draft/2019-09/schema",
            "https://json-schema.org/draft/2020-12/schema",
        ] {
            for legacy in [
                "http://json-schema.org/draft-06/schema#",
                "http://json-schema.org/draft-07/schema#",
            ] {
                let original = json!({
                    "$schema": legacy,
                    "allOf": [{ "required": ["value"] }],
                    "properties": {
                        "value": {
                            "$schema": modern,
                            "$id": "https://example.test/modern",
                            "properties": {
                                "nested": {
                                    "$id": "nested.json",
                                    "$ref": "#/$defs/text",
                                    "$defs": { "text": { "type": "string" } }
                                }
                            }
                        }
                    }
                });
                assert!(convert(original.clone()).is_ok(), "{modern} in {legacy}");

                let mut with_legacy_sibling = original;
                with_legacy_sibling["properties"]["z_legacy"] = json!({
                    "$id": "ignored.json", "$ref": "#"
                });
                let error = convert(with_legacy_sibling).unwrap_err().to_string();
                assert!(error.contains("$id alongside $ref"));

                let original = json!({
                    "$schema": modern,
                    "allOf": [{ "required": ["value"] }],
                    "properties": {
                        "value": {
                            "$schema": legacy,
                            "$id": "https://example.test/legacy",
                            "properties": {
                                "nested": { "$id": "ignored.json", "$ref": "#" }
                            }
                        }
                    }
                });
                let error = convert(original).unwrap_err().to_string();
                assert!(error.contains("$id alongside $ref"));
            }
        }
    }

    #[test]
    fn issue_7459_f2_known_dialects_preserve_plain_id_as_an_annotation() {
        for dialect in [
            "http://json-schema.org/draft-06/schema#",
            "http://json-schema.org/draft-07/schema#",
            "https://json-schema.org/draft/2019-09/schema",
            "https://json-schema.org/draft/2020-12/schema",
        ] {
            let original = json!({
                "$schema": dialect,
                "id": "annotation-only.json",
                "allOf": [{ "required": ["value"] }],
                "definitions": { "text": { "type": "string" } },
                "properties": {
                    "value": { "id": "also-an-annotation.json", "$ref": "#/definitions/text" }
                }
            });
            let converted = convert(original).unwrap();
            let inner = &converted["properties"]["__cc_switch_args"];
            assert_eq!(inner["id"], "annotation-only.json");
            assert_eq!(
                inner["properties"]["value"]["id"],
                "also-an-annotation.json"
            );
            assert_eq!(
                inner["properties"]["value"]["$ref"],
                "#/properties/__cc_switch_args/definitions/text"
            );
        }
    }
}
