//! Codex MCP configuration handling.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::error::AppError;

fn should_sync_codex_mcp() -> bool {
    crate::config::get_codex_config_dir().exists()
}

pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let text = crate::codex_config::read_and_validate_codex_config_text()?;
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }

    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 Codex config.toml 失败: {e}")))?;

    let mut result = HashMap::new();
    let mut import_table = |servers_tbl: &toml::value::Table| {
        for (id, entry_val) in servers_tbl {
            let Some(entry_tbl) = entry_val.as_table() else {
                continue;
            };
            if let Ok(spec) = toml_server_to_json(entry_tbl) {
                result.insert(id.clone(), spec);
            }
        }
    };

    if let Some(mcp_val) = root.get("mcp") {
        if let Some(mcp_tbl) = mcp_val.as_table() {
            if let Some(servers_val) = mcp_tbl.get("servers") {
                if let Some(servers_tbl) = servers_val.as_table() {
                    import_table(servers_tbl);
                }
            }
        }
    }

    if let Some(servers_val) = root.get("mcp_servers") {
        if let Some(servers_tbl) = servers_val.as_table() {
            import_table(servers_tbl);
        }
    }

    Ok(result)
}

pub fn sync_single_server_to_codex(id: &str, server_spec: &Value) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }

    use toml_edit::Item;

    let config_path = crate::codex_config::get_codex_config_path();
    let mut doc = if config_path.exists() {
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
        content
            .parse::<toml_edit::DocumentMut>()
            .unwrap_or_else(|_| toml_edit::DocumentMut::new())
    } else {
        toml_edit::DocumentMut::new()
    };

    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(table) = mcp_item.as_table_like_mut() {
            if table.contains_key("servers") {
                table.remove("servers");
            }
        }
    }

    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }

    doc["mcp_servers"][id] = Item::Table(json_server_to_toml_table(server_spec)?);
    crate::config::write_text_file(&config_path, &doc.to_string())
}

pub fn remove_server_from_codex(id: &str) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }

    let config_path = crate::codex_config::get_codex_config_path();
    if !config_path.exists() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(_) => return Ok(()),
    };

    if let Some(mcp_servers) = doc
        .get_mut("mcp_servers")
        .and_then(|item| item.as_table_mut())
    {
        mcp_servers.remove(id);
    }
    if let Some(mcp_table) = doc.get_mut("mcp").and_then(|item| item.as_table_mut()) {
        if let Some(servers) = mcp_table
            .get_mut("servers")
            .and_then(|item| item.as_table_mut())
        {
            servers.remove(id);
        }
    }

    crate::config::write_text_file(&config_path, &doc.to_string())
}

fn toml_server_to_json(entry_tbl: &toml::value::Table) -> Result<Value, AppError> {
    let kind = entry_tbl
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("stdio");

    let mut spec = serde_json::Map::new();
    spec.insert("type".into(), json!(kind));

    match kind {
        "stdio" => {
            if let Some(command) = entry_tbl.get("command").and_then(|value| value.as_str()) {
                spec.insert("command".into(), json!(command));
            }
            if let Some(args) = entry_tbl.get("args").and_then(|value| value.as_array()) {
                spec.insert(
                    "args".into(),
                    Value::Array(
                        args.iter()
                            .filter_map(|item| item.as_str().map(|text| json!(text)))
                            .collect(),
                    ),
                );
            }
            if let Some(cwd) = entry_tbl.get("cwd").and_then(|value| value.as_str()) {
                spec.insert("cwd".into(), json!(cwd));
            }
            if let Some(env_tbl) = entry_tbl.get("env").and_then(|value| value.as_table()) {
                let mut env = serde_json::Map::new();
                for (key, value) in env_tbl {
                    if let Some(text) = value.as_str() {
                        env.insert(key.clone(), json!(text));
                    }
                }
                if !env.is_empty() {
                    spec.insert("env".into(), Value::Object(env));
                }
            }
        }
        "http" | "sse" => {
            if let Some(url) = entry_tbl.get("url").and_then(|value| value.as_str()) {
                spec.insert("url".into(), json!(url));
            }
            let headers_tbl = entry_tbl
                .get("http_headers")
                .and_then(|value| value.as_table())
                .or_else(|| entry_tbl.get("headers").and_then(|value| value.as_table()));
            if let Some(headers_tbl) = headers_tbl {
                let mut headers = serde_json::Map::new();
                for (key, value) in headers_tbl {
                    if let Some(text) = value.as_str() {
                        headers.insert(key.clone(), json!(text));
                    }
                }
                if !headers.is_empty() {
                    spec.insert("headers".into(), Value::Object(headers));
                }
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!(
                "未知的 Codex MCP 类型: {kind}"
            )));
        }
    }

    Ok(Value::Object(spec))
}

fn json_server_to_toml_table(spec: &Value) -> Result<toml_edit::Table, AppError> {
    use toml_edit::{Array, InlineTable, Item, Table};

    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP 服务器配置必须是对象".into()))?;
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("stdio");
    let mut table = Table::new();
    table["type"] = toml_edit::value(kind);

    match kind {
        "stdio" => {
            let command = obj
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::McpValidation("stdio MCP 缺少 command".into()))?;
            table["command"] = toml_edit::value(command);

            if let Some(args) = obj.get("args").and_then(Value::as_array) {
                let mut arr = Array::default();
                for item in args {
                    if let Some(text) = item.as_str() {
                        arr.push(text);
                    }
                }
                if !arr.is_empty() {
                    table["args"] = Item::Value(toml_edit::Value::Array(arr));
                }
            }

            if let Some(cwd) = obj.get("cwd").and_then(Value::as_str) {
                table["cwd"] = toml_edit::value(cwd);
            }

            if let Some(env) = obj.get("env").and_then(Value::as_object) {
                let mut inline = InlineTable::new();
                for (key, value) in env {
                    if let Some(text) = value.as_str() {
                        inline.insert(key, toml_edit::Value::from(text));
                    }
                }
                if !inline.is_empty() {
                    table["env"] = Item::Value(toml_edit::Value::InlineTable(inline));
                }
            }
        }
        "http" | "sse" => {
            let url = obj
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::McpValidation(format!("{kind} MCP 缺少 url")))?;
            table["url"] = toml_edit::value(url);

            if let Some(headers) = obj.get("headers").and_then(Value::as_object) {
                let mut inline = InlineTable::new();
                for (key, value) in headers {
                    if let Some(text) = value.as_str() {
                        inline.insert(key, toml_edit::Value::from(text));
                    }
                }
                if !inline.is_empty() {
                    table["http_headers"] = Item::Value(toml_edit::Value::InlineTable(inline));
                }
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!("未知的 MCP 类型: {kind}")));
        }
    }

    Ok(table)
}
