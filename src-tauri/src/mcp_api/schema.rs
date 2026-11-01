use serde_json::Value;

// Deliberately bounded subset; unsupported validation keywords fail at import.
pub fn check_schema(s: &Value, depth: usize) -> Result<(), String> {
    if depth > 24 {
        return Err("Schema nesting exceeds 24 levels".into());
    }
    if s.is_boolean() {
        return Ok(());
    }
    let obj = s.as_object().ok_or("Schema must be an object or boolean")?;
    for (key, value) in obj {
        match key.as_str() {
            "properties" => {
                for child in value
                    .as_object()
                    .ok_or("Invalid schema properties")?
                    .values()
                {
                    check_schema(child, depth + 1)?;
                }
            }
            "items" | "additionalProperties" | "not" => check_schema(value, depth + 1)?,
            "allOf" | "anyOf" | "oneOf" => {
                for child in value
                    .as_array()
                    .filter(|v| !v.is_empty())
                    .ok_or("Invalid composite schema")?
                {
                    check_schema(child, depth + 1)?;
                }
            }
            "type" => {
                let types = if let Some(t) = value.as_str() {
                    vec![t]
                } else {
                    value
                        .as_array()
                        .ok_or("Invalid schema type")?
                        .iter()
                        .map(|v| v.as_str().ok_or("Invalid schema type"))
                        .collect::<Result<Vec<_>, _>>()?
                };
                if types.is_empty()
                    || types.iter().any(|t| {
                        !matches!(
                            *t,
                            "string"
                                | "integer"
                                | "number"
                                | "boolean"
                                | "array"
                                | "object"
                                | "null"
                        )
                    })
                {
                    return Err("Unsupported schema type".into());
                }
            }
            "required" => {
                if value
                    .as_array()
                    .is_none_or(|a| a.iter().any(|v| !v.is_string()))
                {
                    return Err("Invalid required property list".into());
                }
            }
            "pattern" => {
                regex::Regex::new(value.as_str().ok_or("Invalid pattern")?)
                    .map_err(|_| "Unsupported regular expression")?;
            }
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                if !value.is_number() {
                    return Err("Schema numeric bounds must be numbers".into());
                }
            }
            "minLength" | "maxLength" | "minItems" | "maxItems" | "minProperties"
            | "maxProperties" => {
                if value.as_u64().is_none() {
                    return Err("Schema length bounds must be nonnegative integers".into());
                }
            }
            "enum" => {
                if value.as_array().is_none_or(|a| a.is_empty()) {
                    return Err("Schema enum must be a nonempty array".into());
                }
            }
            "uniqueItems" => {
                if !value.is_boolean() {
                    return Err("Schema uniqueItems must be boolean".into());
                }
            }
            "title" | "description" | "default" | "examples" | "example" | "format" | "const"
            | "readOnly" | "writeOnly" | "deprecated" | "$schema" => {}
            _ => return Err(format!("Unsupported schema keyword: {key}")),
        }
    }
    Ok(())
}

pub fn validate(s: &Value, v: &Value) -> Result<(), String> {
    if s == &Value::Bool(false) {
        return Err("Schema rejects this value".into());
    }
    if s == &Value::Bool(true) {
        return Ok(());
    }
    let matches_type = |t: &str| match t {
        "string" => v.is_string(),
        "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => false,
    };
    if let Some(t) = s.get("type") {
        let ok = if let Some(t) = t.as_str() {
            matches_type(t)
        } else {
            t.as_array().is_some_and(|ts| {
                ts.iter()
                    .any(|t| matches_type(t.as_str().unwrap_or_default()))
            })
        };
        if !ok {
            return Err("Argument type does not match schema".into());
        }
    }
    if s.get("const").is_some_and(|x| x != v)
        || s.get("enum")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.contains(v))
    {
        return Err("Argument is outside the allowed values".into());
    }
    for (key, kind) in [("allOf", 0), ("anyOf", 1), ("oneOf", 2)] {
        if let Some(a) = s[key].as_array() {
            let count = a.iter().filter(|child| validate(child, v).is_ok()).count();
            if (kind == 0 && count != a.len())
                || (kind == 1 && count == 0)
                || (kind == 2 && count != 1)
            {
                return Err("Argument does not match composite schema".into());
            }
        }
    }
    if s.get("not").is_some_and(|child| validate(child, v).is_ok()) {
        return Err("Argument matches forbidden schema".into());
    }
    if let Some(obj) = v.as_object() {
        if let Some(required) = s["required"].as_array() {
            for key in required {
                if !obj.contains_key(key.as_str().unwrap_or_default()) {
                    return Err("Missing required argument".into());
                }
            }
        }
        for (key, value) in obj {
            if let Some(child) = s["properties"].get(key) {
                validate(child, value)?;
            } else if let Some(child) = s.get("additionalProperties") {
                validate(child, value)?;
            }
        }
        check_length(s, "minProperties", "maxProperties", obj.len())?;
    }
    if let Some(a) = v.as_array() {
        check_length(s, "minItems", "maxItems", a.len())?;
        if let Some(child) = s.get("items") {
            for value in a {
                validate(child, value)?;
            }
        }
        if s["uniqueItems"] == true {
            let mut seen = std::collections::HashSet::new();
            for value in a {
                if !seen.insert(canonical(value).to_string()) {
                    return Err("Array items must be unique".into());
                }
            }
        }
    }
    if let Some(text) = v.as_str() {
        check_length(s, "minLength", "maxLength", text.chars().count())?;
        if let Some(p) = s["pattern"].as_str() {
            if !regex::Regex::new(p)
                .map_err(|_| "Invalid schema pattern")?
                .is_match(text)
            {
                return Err("Argument does not match required pattern".into());
            }
        }
    }
    if let Some(n) = v.as_f64() {
        if s["minimum"].as_f64().is_some_and(|x| n < x)
            || s["maximum"].as_f64().is_some_and(|x| n > x)
            || s["exclusiveMinimum"].as_f64().is_some_and(|x| n <= x)
            || s["exclusiveMaximum"].as_f64().is_some_and(|x| n >= x)
        {
            return Err("Number outside allowed range".into());
        }
    }
    Ok(())
}
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted: std::collections::BTreeMap<_, _> = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(array) => Value::Array(array.iter().map(canonical).collect()),
        _ => value.clone(),
    }
}
fn check_length(s: &Value, min: &str, max: &str, len: usize) -> Result<(), String> {
    if s[min].as_u64().is_some_and(|x| (len as u64) < x)
        || s[max].as_u64().is_some_and(|x| (len as u64) > x)
    {
        Err("Argument length outside allowed range".into())
    } else {
        Ok(())
    }
}
