//! Copilot Chat rejects composition and value constraints at a function schema's
//! root. Project object branches into properties while keeping nested schemas.
//! Branch-dependent requirements remain the local tool executor's responsibility.

use std::collections::HashMap;

use serde_json::{json, Map, Value};
use thiserror::Error;

const ROOT_CONSTRAINTS: &[&str] = &["oneOf", "anyOf", "allOf", "enum", "const", "not"];

pub(crate) const DEFAULT_MAX_DEPTH: usize = 64;
pub(crate) const DEFAULT_MAX_STEPS: usize = 2048;
// Cumulative tree-copy work, including memo hits and property merges. Each
// JSON node costs one unit; strings and object keys also cost their byte length.
const DEFAULT_MAX_VALUE_UNITS: usize = 1_000_000;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum CopilotToolSchemaError {
    #[error("Copilot tool schema projection exceeded maximum depth of {limit}")]
    DepthLimit { limit: usize },

    #[error("Copilot tool schema projection exceeded maximum expansion budget of {limit} steps")]
    WorkBudget { limit: usize },

    #[error("Copilot tool schema projection exceeded tree-copy budget of {limit} value units")]
    ValueBudget { limit: usize },
}

struct ValueBudget {
    remaining: usize,
    limit: usize,
}

impl Default for ValueBudget {
    fn default() -> Self {
        Self {
            remaining: DEFAULT_MAX_VALUE_UNITS,
            limit: DEFAULT_MAX_VALUE_UNITS,
        }
    }
}

impl ValueBudget {
    fn spend(&mut self, units: usize) -> Result<(), CopilotToolSchemaError> {
        self.remaining = self
            .remaining
            .checked_sub(units)
            .ok_or(CopilotToolSchemaError::ValueBudget { limit: self.limit })?;
        Ok(())
    }

    // Inspect before allocating the copy. Bound the traversal stack as well as
    // the copied tree, and stop as soon as the cumulative budget is exhausted.
    fn charge(&mut self, value: &Value) -> Result<(), CopilotToolSchemaError> {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            self.spend(1)?;
            match value {
                Value::String(text) => self.spend(text.len())?,
                Value::Array(values) => {
                    if values.len() > self.remaining {
                        return Err(CopilotToolSchemaError::ValueBudget { limit: self.limit });
                    }
                    pending.extend(values);
                }
                Value::Object(fields) => {
                    if fields.len() > self.remaining {
                        return Err(CopilotToolSchemaError::ValueBudget { limit: self.limit });
                    }
                    for (key, value) in fields {
                        self.spend(key.len())?;
                        pending.push(value);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn copy(&mut self, value: &Value) -> Result<Value, CopilotToolSchemaError> {
        self.charge(value)?;
        Ok(value.clone())
    }
}

fn merge_property(
    properties: &mut Map<String, Value>,
    name: &str,
    schema: &Value,
    union: bool,
    budget: &mut ValueBudget,
) -> Result<(), CopilotToolSchemaError> {
    budget.charge(schema)?;
    if let Some(existing) = properties.get(name) {
        // Charge before comparison too: equality can traverse the entire tree.
        budget.charge(existing)?;
    }
    match properties.get_mut(name) {
        Some(existing) if existing != schema => {
            let key = if union { "anyOf" } else { "allOf" };
            budget.spend(2 + key.len())?;
            *existing = json!({key: [existing.clone(), schema.clone()]});
        }
        None => {
            budget.spend(name.len())?;
            properties.insert(name.to_owned(), schema.clone());
        }
        _ => {}
    }
    Ok(())
}

fn local_target<'a>(document: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    if pointer.is_empty() || pointer.starts_with('/') {
        document.pointer(pointer)
    } else {
        None
    }
}

fn visit_local_refs(
    value: &mut Value,
    visit: &mut impl FnMut(&mut String) -> Result<(), CopilotToolSchemaError>,
) -> Result<(), CopilotToolSchemaError> {
    match value {
        Value::Object(fields) => {
            if let Some(Value::String(reference)) = fields.get_mut("$ref") {
                visit(reference)?;
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
                            visit_local_refs(schema, visit)?;
                        }
                    }
                } else if !matches!(key.as_str(), "const" | "enum" | "default" | "examples") {
                    visit_local_refs(field, visit)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                visit_local_refs(value, visit)?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct ProjectionContext<'a, 'b> {
    document: &'a Value,
    active_refs: Vec<String>,
    memo: HashMap<String, Value>,
    cycle_count: usize,
    depth: usize,
    max_depth: usize,
    steps: usize,
    max_steps: usize,
    values: &'b mut ValueBudget,
}

#[cfg(test)]
pub(crate) fn object_projection(schema: &Value) -> Result<Value, CopilotToolSchemaError> {
    object_projection_with_limits(schema, DEFAULT_MAX_DEPTH, DEFAULT_MAX_STEPS)
}

#[cfg(test)]
pub(crate) fn object_projection_with_limits(
    schema: &Value,
    max_depth: usize,
    max_steps: usize,
) -> Result<Value, CopilotToolSchemaError> {
    object_projection_with_budget(schema, max_depth, max_steps, &mut ValueBudget::default())
}

fn object_projection_with_budget(
    schema: &Value,
    max_depth: usize,
    max_steps: usize,
    values: &mut ValueBudget,
) -> Result<Value, CopilotToolSchemaError> {
    let mut ctx = ProjectionContext {
        document: schema,
        active_refs: Vec::new(),
        memo: HashMap::new(),
        cycle_count: 0,
        depth: 0,
        max_depth,
        steps: 0,
        max_steps,
        values,
    };
    let mut projected = object_projection_inner(schema, &mut ctx)?;
    let snapshot = ctx.values.copy(&projected)?;
    let mut moved_reference = false;
    visit_local_refs(&mut projected, &mut |reference| {
        if let Some(target) = local_target(schema, reference) {
            ctx.values.charge(target)?;
            let projected_target = local_target(&snapshot, reference);
            if let Some(projected_target) = projected_target {
                ctx.values.charge(projected_target)?;
            }
            moved_reference |= projected_target != Some(target);
        }
        Ok(())
    })?;
    if moved_reference {
        // Flattening removes composition paths. Archive the source only when
        // a surviving reference would lose or change its target, and rebase
        // references into that source (including recursive references).
        let mut name = "__cc_switch_source".to_owned();
        while projected["$defs"].get(&name).is_some() {
            name.push('_');
        }
        let prefix = format!("#/$defs/{name}");
        let mut source = ctx.values.copy(schema)?;
        let mut rebase = |reference: &mut String| {
            if local_target(schema, reference).is_some() {
                ctx.values.spend(prefix.len() + reference.len() - 1)?;
                *reference = format!("{prefix}{}", &reference[1..]);
            }
            Ok(())
        };
        visit_local_refs(&mut source, &mut rebase)?;
        visit_local_refs(&mut projected, &mut rebase)?;
        let mut definitions = take_object(projected.as_object_mut().unwrap().remove("$defs"));
        definitions.insert(name, source);
        projected["$defs"] = Value::Object(definitions);
    }
    // Rebasing can lengthen references and archiving can enlarge the result.
    ctx.values.charge(&projected)?;
    Ok(projected)
}

fn take_object(value: Option<Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(fields)) => fields,
        _ => Map::new(),
    }
}

fn take_array(value: Option<Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(values)) => values,
        _ => Vec::new(),
    }
}

fn object_projection_inner(
    schema: &Value,
    ctx: &mut ProjectionContext<'_, '_>,
) -> Result<Value, CopilotToolSchemaError> {
    if ctx.depth > ctx.max_depth {
        return Err(CopilotToolSchemaError::DepthLimit {
            limit: ctx.max_depth,
        });
    }
    if ctx.steps >= ctx.max_steps {
        return Err(CopilotToolSchemaError::WorkBudget {
            limit: ctx.max_steps,
        });
    }
    ctx.steps += 1;

    let mut root = take_object(Some(ctx.values.copy(schema)?));
    if let Some(reference) = root.get("$ref").and_then(Value::as_str).map(str::to_owned) {
        if let Some(cached) = ctx.memo.get(&reference) {
            if root.len() == 1 {
                return ctx.values.copy(cached);
            }
            root.remove("$ref");
            let mut branches = take_array(root.remove("allOf"));
            branches.push(ctx.values.copy(cached)?);
            root.insert("allOf".into(), Value::Array(branches));
        } else if let Some(target) = local_target(ctx.document, &reference) {
            if ctx.active_refs.contains(&reference) {
                ctx.cycle_count += 1;
            } else {
                ctx.active_refs.push(reference.clone());
                ctx.depth += 1;
                let prior_cycles = ctx.cycle_count;
                let resolved_res = object_projection_inner(target, ctx);
                ctx.depth -= 1;
                ctx.active_refs.pop();
                let resolved = resolved_res?;
                if ctx.cycle_count == prior_cycles {
                    ctx.memo
                        .insert(reference.clone(), ctx.values.copy(&resolved)?);
                }
                if root.len() == 1 {
                    return Ok(resolved);
                }
                root.remove("$ref");
                let mut branches = take_array(root.remove("allOf"));
                branches.push(resolved);
                root.insert("allOf".into(), Value::Array(branches));
            }
        }
    }
    let mut properties = take_object(root.remove("properties"));
    let mut required = take_array(root.remove("required"));

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
            ctx.depth += 1;
            let projected_res = object_projection_inner(branch, ctx);
            ctx.depth -= 1;
            let projected = projected_res?;

            // Property schemas may refer to definitions declared in a branch.
            // Keep those targets when lifting the properties to the root.
            for key in ["$defs", "definitions"] {
                if let Some(definitions) = projected.get(key).and_then(Value::as_object) {
                    let target = root.entry(key).or_insert_with(|| json!({}));
                    if let Some(target) = target.as_object_mut() {
                        for (name, definition) in definitions {
                            merge_property(target, name, definition, true, ctx.values)?;
                        }
                    }
                }
            }
            if let Some(fields) = projected.get("properties").and_then(Value::as_object) {
                for (name, field) in fields {
                    merge_property(
                        &mut branch_properties,
                        name,
                        field,
                        key != "allOf",
                        ctx.values,
                    )?;
                }
            }
            let fields = take_array(Some(ctx.values.copy(&projected["required"])?));
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
            merge_property(&mut properties, &name, &field, false, ctx.values)?;
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
    Ok(Value::Object(root))
}

/// Apply only to Copilot Chat tools after client format conversion, including
/// Codex namespaces and deferred tools, and Claude input schemas.
pub(crate) fn normalize_copilot_chat_tool_schemas(
    body: &mut Value,
) -> Result<(), CopilotToolSchemaError> {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut values = ValueBudget::default();
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
            let projected = object_projection_with_budget(
                parameters,
                DEFAULT_MAX_DEPTH,
                DEFAULT_MAX_STEPS,
                &mut values,
            )?;
            function.insert("parameters".into(), projected);
            // Projection relaxes cross-field unions; strict mode would require
            // every branch's optional fields and defeat that representation.
            function.insert("strict".into(), json!(false));
        }
    }
    Ok(())
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
        let projected = object_projection(&schema).expect("projection should succeed");
        assert_eq!(projected["required"], json!(["mode"]));
        assert_eq!(projected["properties"]["id"]["type"], "string");
        assert_eq!(projected["properties"]["prompt"]["type"], "string");
        assert!(projected["$defs"].get("__cc_switch_source").is_none());

        let referenced = object_projection(&json!({
            "$defs": {"view": schema["$defs"]["view"]}, "$ref": "#/$defs/view",
            "properties": {"extra": {"type": "boolean"}}, "required": ["extra"]
        }))
        .expect("referenced projection should succeed");
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
        let projected = object_projection(&schema).expect("projection should succeed");
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
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn cyclic_branch_references_terminate() {
        let projected = object_projection(&json!({"$defs": {
            "branch": {"$ref": "#/$defs/branch", "properties": {"id": {"type": "string"}}}
        }, "anyOf": [{"$ref": "#/$defs/branch"}]}))
        .expect("cyclic branch projection should succeed");
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
            let projected = object_projection(&schema).expect("union projection should succeed");
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
        }))
        .expect("allOf projection should succeed");
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
            normalize_copilot_chat_tool_schemas(&mut body).expect("normalization should succeed");
            assert_eq!(body["tools"][0], plain);
            assert!(body["tools"][1]["function"]["parameters"]
                .get(*key)
                .is_none());
            assert_eq!(body["tools"][1]["function"]["strict"], false);
            let normalized = body.clone();
            normalize_copilot_chat_tool_schemas(&mut body).expect("normalization should succeed");
            assert_eq!(body, normalized);
        }
    }

    #[test]
    fn handles_shared_reference_dag_without_exponential_recursion() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "d0".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"}
                }
            }),
        );
        for i in 1..=20 {
            let prev = format!("#/$defs/d{}", i - 1);
            defs.insert(
                format!("d{i}"),
                json!({
                    "allOf": [
                        {"$ref": prev},
                        {"$ref": prev}
                    ]
                }),
            );
        }
        let schema = json!({
            "$defs": defs,
            "allOf": [{"$ref": "#/$defs/d20"}]
        });

        let projected = object_projection(&schema).expect("DAG projection should succeed");

        assert_eq!(projected["properties"]["id"]["type"], "string");
        assert!(projected.to_string().len() < 2_000);
    }

    #[test]
    fn copilot_shared_refs_with_sibling_constraints_hit_copy_budget() {
        let mut defs = Map::new();
        defs.insert(
            "d0".into(),
            json!({"type": "object", "properties": {"id": {"type": "string"}}}),
        );
        for i in 1..=16 {
            let prev = format!("#/$defs/d{}", i - 1);
            defs.insert(
                format!("d{i}"),
                json!({"allOf": [
                    {"$ref": prev},
                    {"$ref": prev, "properties": {"id": {"minLength": i}}}
                ]}),
            );
        }
        let schema = json!({"$defs": defs, "allOf": [{"$ref": "#/$defs/d16"}]});
        assert!(schema.to_string().len() < 2_000);
        assert_eq!(
            object_projection(&schema).unwrap_err(),
            CopilotToolSchemaError::ValueBudget {
                limit: DEFAULT_MAX_VALUE_UNITS
            }
        );
    }

    #[test]
    fn copilot_copy_budget_is_shared_across_tools() {
        let tool = json!({"type": "function", "function": {"name": "large", "parameters": {
            "allOf": [], "description": "x".repeat(DEFAULT_MAX_VALUE_UNITS / 4)
        }}});
        let mut single = json!({"tools": [tool.clone()]});
        normalize_copilot_chat_tool_schemas(&mut single).unwrap();
        let mut body = json!({"tools": [tool.clone(), tool]});
        assert_eq!(
            normalize_copilot_chat_tool_schemas(&mut body).unwrap_err(),
            CopilotToolSchemaError::ValueBudget {
                limit: DEFAULT_MAX_VALUE_UNITS
            }
        );
    }

    #[test]
    fn copilot_copy_budget_counts_annotation_bytes_before_cloning() {
        let schema = json!({"allOf": [], "description": "x".repeat(DEFAULT_MAX_VALUE_UNITS)});
        assert_eq!(
            object_projection(&schema).unwrap_err(),
            CopilotToolSchemaError::ValueBudget {
                limit: DEFAULT_MAX_VALUE_UNITS
            }
        );
    }

    #[test]
    fn returns_diagnostic_error_when_depth_or_budget_limit_exceeded() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "d0".to_string(),
            json!({
                "type": "object",
                "properties": {"id": {"type": "string"}}
            }),
        );
        for i in 1..=10 {
            defs.insert(
                format!("d{i}"),
                json!({"allOf": [{"$ref": format!("#/$defs/d{}", i - 1)}]}),
            );
        }
        let deep_schema = json!({
            "$defs": defs,
            "allOf": [{"$ref": "#/$defs/d10"}]
        });

        let depth_err = object_projection_with_limits(&deep_schema, 5, 1000).unwrap_err();
        assert_eq!(depth_err, CopilotToolSchemaError::DepthLimit { limit: 5 });

        let budget_err = object_projection_with_limits(&deep_schema, 64, 3).unwrap_err();
        assert_eq!(budget_err, CopilotToolSchemaError::WorkBudget { limit: 3 });
    }

    #[test]
    fn normalize_copilot_chat_tool_schemas_propagates_limits_error() {
        let mut defs = serde_json::Map::new();
        defs.insert(
            "d0".to_string(),
            json!({
                "type": "object",
                "properties": {"id": {"type": "string"}}
            }),
        );
        for i in 1..=100 {
            defs.insert(
                format!("d{i}"),
                json!({"allOf": [{"$ref": format!("#/$defs/d{}", i - 1)}]}),
            );
        }
        let mut body = json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "deep",
                    "parameters": {
                        "$defs": defs,
                        "allOf": [{"$ref": "#/$defs/d100"}]
                    }
                }
            }]
        });
        let res = normalize_copilot_chat_tool_schemas(&mut body);
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err(),
            CopilotToolSchemaError::DepthLimit {
                limit: DEFAULT_MAX_DEPTH
            }
        );
    }
}
