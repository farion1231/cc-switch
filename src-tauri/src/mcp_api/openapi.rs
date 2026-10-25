use super::{schema, validate_target, ApiPreview, ApiTool};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub fn compile_document(
    text: &str,
    source: Option<&str>,
    base: Option<&str>,
) -> Result<ApiPreview, String> {
    if text.len() > 5 * 1024 * 1024 {
        return Err("API document exceeds 5 MiB".into());
    }
    let doc: Value = serde_yaml::from_str(text).map_err(|_| "Invalid JSON/YAML API document")?;
    let swagger = doc["swagger"] == "2.0";
    if !swagger
        && !doc["openapi"]
            .as_str()
            .is_some_and(|v| v.starts_with("3.0.") || v.starts_with("3.1."))
    {
        return Err("Expected OpenAPI 3.0/3.1 or Swagger 2.0".into());
    }
    let paths = doc["paths"]
        .as_object()
        .ok_or("API document has no paths object")?;
    let mut preview = ApiPreview {
        title: doc["info"]["title"]
            .as_str()
            .unwrap_or("API")
            .chars()
            .take(128)
            .collect(),
        tools: Vec::new(),
        skipped: Vec::new(),
    };
    let mut names = HashSet::new();
    let mut total_bytes = 0usize;
    for (path, item) in paths {
        if item.get("$ref").is_some() {
            preview
                .skipped
                .push(format!("{path}: referenced path items are unsupported"));
            continue;
        }
        for method in ["get", "head", "post", "put", "patch", "delete", "options"] {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let result =
                compile_operation(&doc, item, operation, path, method, swagger, source, base);
            match result {
                Ok(tool) => {
                    total_bytes += serde_json::to_vec(&tool)
                        .map_err(|_| "Cannot encode tool preview")?
                        .len();
                    if total_bytes > 5 * 1024 * 1024 {
                        return Err(
                            "Expanded tool preview exceeds 5 MiB; use a smaller specification"
                                .into(),
                        );
                    }
                    if names.insert(tool.name.clone()) {
                        preview.tools.push(tool);
                    } else {
                        preview.skipped.push(format!(
                            "{} {path}: duplicate tool name",
                            method.to_uppercase()
                        ));
                    }
                }
                Err(reason) => preview
                    .skipped
                    .push(format!("{} {path}: {reason}", method.to_uppercase())),
            }
            if preview.tools.len() + preview.skipped.len() > 1000 {
                return Err(
                    "API document exceeds 1000 operations; use a smaller specification".into(),
                );
            }
        }
    }
    Ok(preview)
}

#[allow(clippy::too_many_arguments)]
fn compile_operation(
    doc: &Value,
    item: &Value,
    op: &Value,
    path: &str,
    method: &str,
    swagger: bool,
    source: Option<&str>,
    base: Option<&str>,
) -> Result<ApiTool, String> {
    if !op.is_object()
        || !path.starts_with('/')
        || path.contains(['?', '#'])
        || path.split('/').any(|s| s == ".." || s == ".")
    {
        return Err("Invalid API path or operation".into());
    }
    if item.get("$ref").is_some() {
        return Err("Referenced path items are unsupported".into());
    }
    let base = base_url(doc, item, op, swagger, source, base)?;
    let mut groups: Map<String, Value> = Map::new();
    let mut required = Vec::<String>::new();
    let mut params = Vec::<Value>::new();
    for container in [item, op] {
        if let Some(values) = container.get("parameters") {
            for p in values.as_array().ok_or("Parameters must be an array")? {
                let p = resolve(doc, p, &mut Vec::new(), 0, &mut 0)?;
                params.retain(|old| old["in"] != p["in"] || old["name"] != p["name"]);
                params.push(p);
            }
        }
    }
    for p in params {
        let location = p["in"].as_str().ok_or("Parameter location is missing")?;
        if location == "body" && swagger {
            let consumes = op.get("consumes").or_else(|| doc.get("consumes"));
            if consumes.is_some_and(|c| {
                !c.as_array()
                    .is_some_and(|a| a.iter().any(|v| v == "application/json"))
            }) {
                return Err("Only application/json request bodies are supported".into());
            }
            groups.insert("body".into(), input_schema(doc, &p["schema"])?);
            if p["required"] == true {
                required.push("body".into());
            }
            continue;
        }
        let group = match location {
            "path" => "path",
            "query" => "query",
            "header" => "headers",
            _ => {
                return Err(
                    "Only path, query, header and JSON body parameters are supported".into(),
                )
            }
        };
        let name = p["name"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("Parameter name is missing")?;
        if p.get("content").is_some() || p["allowReserved"] == true {
            return Err("Parameter content/allowReserved serialization is unsupported".into());
        }
        let mut s = if swagger {
            let mut s = Map::new();
            for key in [
                "type",
                "items",
                "enum",
                "default",
                "description",
                "minimum",
                "maximum",
                "minLength",
                "maxLength",
                "pattern",
            ] {
                if let Some(v) = p.get(key) {
                    s.insert(key.into(), v.clone());
                }
            }
            input_schema(doc, &Value::Object(s))?
        } else {
            input_schema(doc, p.get("schema").ok_or("Parameter schema is missing")?)?
        };
        if let Some(description) = p["description"].as_str() {
            s["description"] = json!(description);
        }
        let ty = s["type"]
            .as_str()
            .ok_or("Parameter type must be explicit and non-nullable")?;
        if !matches!(ty, "string" | "integer" | "number" | "boolean" | "array") {
            return Err("Object parameters are unsupported; use a JSON body".into());
        }
        if ty == "array" {
            if location != "query"
                || (!swagger
                    && (p["style"].as_str().unwrap_or("form") != "form" || p["explode"] == false))
                || (swagger && p["collectionFormat"] != "multi")
                || !matches!(
                    s["items"]["type"].as_str(),
                    Some("string" | "number" | "integer" | "boolean")
                )
            {
                return Err("Only repeated primitive query arrays (form/explode or collectionFormat=multi) are supported".into());
            }
        } else if p["style"].as_str().is_some_and(|style| {
            style
                != if location == "query" {
                    "form"
                } else {
                    "simple"
                }
        }) {
            return Err("Unsupported parameter serialization style".into());
        }
        let entry = groups.entry(group.to_string()).or_insert_with(
            || json!({"type":"object","properties":{},"additionalProperties":false,"required":[]}),
        );
        entry["properties"][name] = s;
        if p["required"] == true || location == "path" {
            entry["required"].as_array_mut().unwrap().push(json!(name));
            if !required.iter().any(|v| v == group) {
                required.push(group.into());
            }
        }
    }
    if let Some(body) = op.get("requestBody") {
        let body = resolve(doc, body, &mut Vec::new(), 0, &mut 0)?;
        let content = body["content"]
            .as_object()
            .ok_or("Request body content is missing")?;
        let json = content
            .get("application/json")
            .ok_or("Only application/json request bodies are supported")?;
        groups.insert(
            "body".into(),
            input_schema(
                doc,
                json.get("schema").ok_or("JSON body schema is missing")?,
            )?,
        );
        if body["required"] == true {
            required.push("body".into());
        }
    }
    if matches!(method, "get" | "head") && groups.contains_key("body") {
        return Err("GET/HEAD request bodies are unsupported".into());
    }
    let fallback = format!(
        "{method}_{}",
        path.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    let name = op["operationId"].as_str().unwrap_or(&fallback);
    let name = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_.-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>();
    let tool = ApiTool {
        read_only: op["x-mcp-read-only"].as_bool(),
        name,
        description: op["summary"]
            .as_str()
            .or_else(|| op["description"].as_str())
            .unwrap_or(path)
            .chars()
            .take(1000)
            .collect(),
        method: method.to_uppercase(),
        url: format!("{}{path}", base.trim_end_matches('/')),
        input_schema: json!({"type":"object","properties":groups,"required":required,"additionalProperties":false}),
    };
    // Validate one operation without imposing the final selection's 100-tool limit.
    super::validate_profile(&super::ApiProfile {
        version: 1,
        tools: vec![tool.clone()],
    })?;
    Ok(tool)
}

fn base_url(
    doc: &Value,
    item: &Value,
    op: &Value,
    swagger: bool,
    source: Option<&str>,
    override_url: Option<&str>,
) -> Result<String, String> {
    let candidate = if let Some(base) = override_url {
        base.to_string()
    } else if swagger {
        let host = doc["host"]
            .as_str()
            .ok_or("Swagger host is missing; supply a base URL")?;
        let scheme = op["schemes"][0]
            .as_str()
            .or_else(|| doc["schemes"][0].as_str())
            .unwrap_or("https");
        format!(
            "{scheme}://{host}{}",
            doc["basePath"].as_str().unwrap_or("")
        )
    } else {
        op.get("servers")
            .or_else(|| item.get("servers"))
            .or_else(|| doc.get("servers"))
            .and_then(|v| v[0]["url"].as_str())
            .unwrap_or("/")
            .to_string()
    };
    if candidate.contains(['{', '}']) {
        return Err("Server variables are unsupported; supply a concrete base URL".into());
    }
    let url = match url::Url::parse(&candidate) {
        Ok(url) => url,
        Err(_) => validate_target(source.ok_or("Supply a base URL for relative servers")?)?
            .join(&candidate)
            .map_err(|_| "Invalid relative server URL")?,
    };
    validate_target(url.as_str())?;
    if url.query().is_some() {
        return Err("Base URL must not contain a query string".into());
    }
    Ok(url.to_string())
}

fn input_schema(doc: &Value, s: &Value) -> Result<Value, String> {
    let mut s = resolve(doc, s, &mut Vec::new(), 0, &mut 0)?;
    normalize(&mut s);
    schema::check_schema(&s, 0)?;
    Ok(s)
}
fn normalize(s: &mut Value) {
    if let Some(obj) = s.as_object_mut() {
        for (exclusive, inclusive) in [
            ("exclusiveMinimum", "minimum"),
            ("exclusiveMaximum", "maximum"),
        ] {
            if let Some(flag) = obj.get(exclusive).and_then(Value::as_bool) {
                obj.remove(exclusive);
                if flag {
                    if let Some(bound) = obj.remove(inclusive) {
                        obj.insert(exclusive.into(), bound);
                    } else {
                        obj.insert(exclusive.into(), Value::Bool(true));
                    }
                }
            }
        }
        if obj.remove("nullable") == Some(json!(true)) {
            if let Some(t) = obj.get("type").and_then(Value::as_str).map(str::to_string) {
                obj.insert("type".into(), json!([t, "null"]));
            }
        }
        // OpenAPI documentation annotations are not JSON Schema assertions.
        for key in ["xml", "externalDocs", "discriminator"] {
            obj.remove(key);
        }
        obj.retain(|k, _| !k.starts_with("x-"));
        let mut readonly = HashSet::new();
        if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            props.retain(|key, schema| {
                if schema["readOnly"] == true {
                    readonly.insert(key.clone());
                    false
                } else {
                    true
                }
            });
        }
        if let Some(required) = obj.get_mut("required").and_then(Value::as_array_mut) {
            required.retain(|name| !name.as_str().is_some_and(|name| readonly.contains(name)));
        }
        for (key, value) in obj.iter_mut() {
            match key.as_str() {
                "properties" => {
                    if let Some(props) = value.as_object_mut() {
                        for child in props.values_mut() {
                            normalize(child);
                        }
                    }
                }
                "items" | "additionalProperties" | "not" => normalize(value),
                "allOf" | "anyOf" | "oneOf" => {
                    if let Some(items) = value.as_array_mut() {
                        for child in items {
                            normalize(child);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
fn resolve(
    doc: &Value,
    v: &Value,
    stack: &mut Vec<String>,
    depth: usize,
    nodes: &mut usize,
) -> Result<Value, String> {
    // Charge large strings as well as nodes to bound reference expansion memory.
    *nodes += 1 + v.as_str().map_or(0, |text| text.len() / 16);
    if depth > 24 || *nodes > 10000 {
        return Err("Reference/schema expansion exceeds limits".into());
    }
    match v {
        Value::Object(obj) => {
            if let Some(reference) = obj.get("$ref") {
                let reference = reference.as_str().ok_or("Invalid reference")?;
                if !reference.starts_with("#/") || stack.iter().any(|v| v == reference) {
                    return Err("External or circular references are unsupported".into());
                }
                // Reject ambiguous siblings rather than silently dropping constraints.
                if obj
                    .keys()
                    .any(|k| !matches!(k.as_str(), "$ref" | "description" | "summary"))
                {
                    return Err("Reference siblings with constraints are unsupported".into());
                }
                let target = doc
                    .pointer(&reference[1..])
                    .ok_or("Unresolved internal reference")?;
                stack.push(reference.into());
                let result = resolve(doc, target, stack, depth + 1, nodes);
                stack.pop();
                return result;
            }
            let mut out = Map::new();
            for (key, value) in obj {
                out.insert(key.clone(), resolve(doc, value, stack, depth + 1, nodes)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| resolve(doc, item, stack, depth + 1, nodes))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        _ => Ok(v.clone()),
    }
}
