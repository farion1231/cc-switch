//! Copilot Chat rejects composition and value constraints at a function schema's
//! root. Project object branches into properties while keeping nested schemas.
//! Branch-dependent requirements remain the local tool executor's responsibility.

use serde_json::{json, Map, Value};

const ROOT_CONSTRAINTS: &[&str] = &["oneOf", "anyOf", "allOf", "enum", "const", "not"];

fn merge_property(properties: &mut Map<String, Value>, name: &str, schema: &Value, union: bool) {
    match properties.get_mut(name) {
        Some(existing) if existing != schema => {
            let key = if union { "anyOf" } else { "allOf" };
            *existing = json!({key: [existing.clone(), schema.clone()]});
        }
        None => {
            properties.insert(name.to_owned(), schema.clone());
        }
        _ => {}
    }
}

fn local_target<'a>(document: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    if pointer.is_empty() || pointer.starts_with('/') {
        document.pointer(pointer)
    } else {
        None
    }
}

fn visit_local_refs(value: &mut Value, visit: &mut impl FnMut(&mut String)) {
    match value {
        Value::Object(fields) => {
            if let Some(Value::String(reference)) = fields.get_mut("$ref") {
                visit(reference);
            }
            for (key, field) in fields {
                // These contain instance data rather than schemas.
                if matches!(
                    key.as_str(),
                    "properties"
                        | "$defs"
                        | "definitions"
                        | "patternProperties"
                        | "dependentSchemas"
                ) {
                    if let Some(schemas) = field.as_object_mut() {
                        for schema in schemas.values_mut() {
                            visit_local_refs(schema, visit);
                        }
                    }
                } else if !matches!(key.as_str(), "const" | "enum" | "default" | "examples") {
                    visit_local_refs(field, visit);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                visit_local_refs(value, visit);
            }
        }
        _ => {}
    }
}

fn object_projection(schema: &Value) -> Value {
    let mut projected = object_projection_inner(schema, schema, &mut Vec::new());
    let snapshot = projected.clone();
    let mut moved_reference = false;
    visit_local_refs(&mut projected, &mut |reference| {
        if let Some(target) = local_target(schema, reference) {
            moved_reference |= local_target(&snapshot, reference) != Some(target);
        }
    });
    if moved_reference {
        // Flattening removes composition paths. Archive the source only when
        // a surviving reference would lose or change its target, and rebase
        // references into that source (including recursive references).
        let mut name = "__cc_switch_source".to_owned();
        while projected["$defs"].get(&name).is_some() {
            name.push('_');
        }
        let prefix = format!("#/$defs/{name}");
        let mut rebase = |reference: &mut String| {
            if local_target(schema, reference).is_some() {
                *reference = format!("{prefix}{}", &reference[1..]);
            }
        };
        let mut source = schema.clone();
        visit_local_refs(&mut source, &mut rebase);
        visit_local_refs(&mut projected, &mut rebase);
        let mut definitions = projected["$defs"].as_object().cloned().unwrap_or_default();
        definitions.insert(name, source);
        projected["$defs"] = Value::Object(definitions);
    }
    projected
}

fn object_projection_inner(
    schema: &Value,
    document: &Value,
    active_refs: &mut Vec<String>,
) -> Value {
    let mut root = schema.as_object().cloned().unwrap_or_default();
    let mut expanded_ref = false;
    if let Some(reference) = root.get("$ref").and_then(Value::as_str).map(str::to_owned) {
        if let Some(target) = local_target(document, &reference) {
            if !active_refs.contains(&reference) {
                active_refs.push(reference);
                expanded_ref = true;
                let resolved = object_projection_inner(target, document, active_refs);
                root.remove("$ref");
                let mut branches = root
                    .remove("allOf")
                    .and_then(|value| value.as_array().cloned())
                    .unwrap_or_default();
                branches.push(resolved);
                root.insert("allOf".into(), Value::Array(branches));
            }
        }
    }
    let mut properties = root
        .remove("properties")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let mut required = root
        .remove("required")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();

    for key in ["oneOf", "anyOf", "allOf"] {
        let Some(Value::Array(branches)) = root.remove(key) else {
            continue;
        };
        let mut branch_properties = Map::new();
        let mut branch_required: Option<Vec<Value>> = None;
        for branch in branches.iter().filter(|branch| {
            branch.is_object()
                && branch
                    .get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "object")
        }) {
            let projected = object_projection_inner(branch, document, active_refs);
            // Property schemas may refer to definitions declared in a branch.
            // Keep those targets when lifting the properties to the root.
            for key in ["$defs", "definitions"] {
                if let Some(definitions) = projected.get(key).and_then(Value::as_object) {
                    let target = root.entry(key).or_insert_with(|| json!({}));
                    if let Some(target) = target.as_object_mut() {
                        for (name, definition) in definitions {
                            merge_property(target, name, definition, true);
                        }
                    }
                }
            }
            if let Some(fields) = projected.get("properties").and_then(Value::as_object) {
                for (name, field) in fields {
                    merge_property(&mut branch_properties, name, field, key != "allOf");
                }
            }
            let fields = projected["required"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            branch_required = Some(match branch_required {
                None => fields,
                Some(mut existing) if key == "allOf" => {
                    for field in fields {
                        if !existing.contains(&field) {
                            existing.push(field);
                        }
                    }
                    existing
                }
                Some(existing) => existing
                    .into_iter()
                    .filter(|field| fields.contains(field))
                    .collect(),
            });
        }
        for (name, field) in branch_properties {
            merge_property(&mut properties, &name, &field, false);
        }
        for field in branch_required.unwrap_or_default() {
            if !required.contains(&field) {
                required.push(field);
            }
        }
    }
    for key in ROOT_CONSTRAINTS {
        root.remove(*key);
    }
    root.insert("type".into(), json!("object"));
    root.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        root.insert("required".into(), Value::Array(required));
    }
    if expanded_ref {
        active_refs.pop();
    }
    Value::Object(root)
}

/// Apply only to Copilot Chat tools after client format conversion, including
/// Codex namespaces and deferred tools, and Claude input schemas.
pub(crate) fn normalize_copilot_chat_tool_schemas(body: &mut Value) {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let Some(function) = tool.get_mut("function").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(parameters) = function.get("parameters") else {
            continue;
        };
        if ROOT_CONSTRAINTS
            .iter()
            .any(|key| parameters.get(*key).is_some())
            || parameters.get("$ref").is_some()
        {
            let projected = object_projection(parameters);
            function.insert("parameters".into(), projected);
            // Projection relaxes cross-field unions; strict mode would require
            // every branch's optional fields and defeat that representation.
            function.insert("strict".into(), json!(false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_referenced_union_branches_and_root_reference_siblings() {
        let schema = json!({"$defs": {
            "view": {"type": "object", "properties": {
                "mode": {"const": "view"}, "id": {"type": "string"}
            }, "required": ["mode", "id"]},
            "create": {"type": "object", "properties": {
                "mode": {"const": "create"}, "prompt": {"type": "string"}
            }, "required": ["mode", "prompt"]}
        }, "oneOf": [{"$ref": "#/$defs/view"}, {"$ref": "#/$defs/create"}]});
        let projected = object_projection(&schema);
        assert_eq!(projected["required"], json!(["mode"]));
        assert_eq!(projected["properties"]["id"]["type"], "string");
        assert_eq!(projected["properties"]["prompt"]["type"], "string");
        assert!(projected["$defs"].get("__cc_switch_source").is_none());

        let referenced = object_projection(&json!({
            "$defs": {"view": schema["$defs"]["view"]}, "$ref": "#/$defs/view",
            "properties": {"extra": {"type": "boolean"}}, "required": ["extra"]
        }));
        assert!(referenced.get("$ref").is_none());
        assert_eq!(referenced["properties"]["id"]["type"], "string");
        assert_eq!(referenced["properties"]["extra"]["type"], "boolean");
        assert_eq!(referenced["required"], json!(["extra", "mode", "id"]));
    }

    #[test]
    fn preserves_references_to_removed_paths_and_recursive_schemas() {
        let schema = json!({"oneOf": [{"type": "object", "properties": {
            "value": {"type": "string"},
            "alias": {"$ref": "#/oneOf/0/properties/value"},
            "children": {"type": "array", "items": {"$ref": "#"}},
            "const": {"$ref": "#/oneOf/0/properties/value"},
            "literal": {"const": {"$ref": "#/oneOf/0/properties/value"}}
        }}], "$defs": {"__cc_switch_source": {"type": "null"}}});
        let projected = object_projection(&schema);
        let prefix = "#/$defs/__cc_switch_source_";
        assert_eq!(
            projected["properties"]["alias"]["$ref"],
            format!("{prefix}/oneOf/0/properties/value")
        );
        assert_eq!(projected["properties"]["children"]["items"]["$ref"], prefix);
        assert_eq!(
            projected["properties"]["const"]["$ref"],
            projected["properties"]["alias"]["$ref"]
        );
        assert_eq!(
            projected["properties"]["literal"],
            schema["oneOf"][0]["properties"]["literal"]
        );
        assert_eq!(projected["$defs"]["__cc_switch_source"]["type"], "null");
        assert_eq!(
            projected["$defs"]["__cc_switch_source_"]["oneOf"][0]["properties"]["children"]
                ["items"]["$ref"],
            prefix
        );
        let mut verified = projected.clone();
        visit_local_refs(&mut verified, &mut |reference| {
            assert!(local_target(&projected, reference).is_some(), "{reference}");
        });
    }

    #[test]
    fn cyclic_branch_references_terminate() {
        let projected = object_projection(&json!({"$defs": {
            "branch": {"$ref": "#/$defs/branch", "properties": {"id": {"type": "string"}}}
        }, "anyOf": [{"$ref": "#/$defs/branch"}]}));
        assert_eq!(projected["properties"]["id"]["type"], "string");
    }

    #[test]
    fn projects_object_unions_without_losing_modes_or_branch_fields() {
        for key in ["oneOf", "anyOf"] {
            let schema = json!({key: [
                {"type": "object", "properties": {
                    "mode": {"const": "view"}, "id": {"type": "string"}
                }, "required": ["mode", "id"]},
                {"type": "object", "properties": {
                    "mode": {"enum": ["create", "update"]},
                    "prompt": {"type": "string"},
                    "schedule": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                    "options": {"$ref": "#/$defs/options"}
                }, "required": ["mode", "prompt"],
                "$defs": {"options": {"type": "object", "properties": {"enabled": {"type": "boolean"}}}}},
                {"type": "null"}
            ]});
            let projected = object_projection(&schema);
            assert_eq!(projected["type"], "object");
            assert!(projected.get(key).is_none());
            assert_eq!(projected["required"], json!(["mode"]));
            assert_eq!(
                projected["properties"]["mode"]["anyOf"],
                json!([
                    {"const": "view"}, {"enum": ["create", "update"]}
                ])
            );
            assert_eq!(projected["properties"]["id"]["type"], "string");
            assert_eq!(projected["properties"]["prompt"]["type"], "string");
            assert_eq!(
                projected["$defs"]["options"],
                schema[key][1]["$defs"]["options"]
            );
            assert_eq!(
                projected["properties"]["schedule"],
                schema[key][1]["properties"]["schedule"]
            );
        }
    }

    #[test]
    fn all_of_preserves_required_fields_and_root_properties() {
        let projected = object_projection(&json!({
            "description": "lookup", "additionalProperties": false,
            "properties": {"name": {"type": "string"}}, "required": ["name"],
            "allOf": [
                {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]},
                {"type": "object", "properties": {"mode": {"const": "view"}}, "required": ["mode"]}
            ]
        }));
        assert_eq!(projected["required"], json!(["name", "id", "mode"]));
        assert_eq!(projected["additionalProperties"], false);
        assert_eq!(projected["description"], "lookup");
        assert_eq!(projected["properties"].as_object().unwrap().len(), 3);
        assert!(projected.get("allOf").is_none());
    }

    #[test]
    fn changes_only_function_roots_with_forbidden_keywords() {
        let plain = json!({"type": "function", "function": {
            "name": "plain", "strict": true, "parameters": {
                "type": "object", "properties": {"value": {"oneOf": [{"type": "string"}, {"type": "number"}]}}
            }
        }});
        for key in ROOT_CONSTRAINTS {
            let mut body = json!({"tools": [plain.clone(), {
                "type": "function", "function": {
                    "name": "union", "strict": true,
                    "parameters": {"type": "object", key.to_string(): []}
                }
            }]});
            normalize_copilot_chat_tool_schemas(&mut body);
            assert_eq!(body["tools"][0], plain);
            assert!(body["tools"][1]["function"]["parameters"]
                .get(*key)
                .is_none());
            assert_eq!(body["tools"][1]["function"]["strict"], false);
            let normalized = body.clone();
            normalize_copilot_chat_tool_schemas(&mut body);
            assert_eq!(body, normalized);
        }
    }
}
