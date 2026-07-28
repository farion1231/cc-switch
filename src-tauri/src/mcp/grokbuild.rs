//! Grok Build MCP synchronization and import.
//!
//! Grok Build uses the same top-level `[mcp_servers]` TOML layout as Codex,
//! stored alongside its model configuration in `~/.grok/config.toml`.

use serde_json::{json, Value};

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::codex::json_server_to_toml_table;
use super::validation::validate_server_spec;

fn should_sync_grokbuild_mcp() -> bool {
    crate::grok_config::get_grok_config_dir().exists()
}

fn read_config_text() -> Result<String, AppError> {
    let path = crate::grok_config::get_grok_config_path();
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
}

fn json_server_to_grokbuild_toml_table(server_spec: &Value) -> Result<toml_edit::Table, AppError> {
    let mut table = json_server_to_toml_table(server_spec)?;
    // Grok infers transport from `command` or `url` and uses `headers`, while
    // Codex writes an explicit `type` plus `http_headers`.
    table.remove("type");
    if let Some(headers) = table.remove("http_headers") {
        table.insert("headers", headers);
    }
    Ok(table)
}

fn toml_server_to_json(entry: &toml::value::Table) -> Value {
    fn convert(value: &toml::Value) -> Option<Value> {
        match value {
            toml::Value::String(value) => Some(json!(value)),
            toml::Value::Integer(value) => Some(json!(value)),
            toml::Value::Float(value) => Some(json!(value)),
            toml::Value::Boolean(value) => Some(json!(value)),
            toml::Value::Datetime(value) => Some(json!(value.to_string())),
            toml::Value::Array(values) => Some(Value::Array(
                values.iter().filter_map(convert).collect::<Vec<_>>(),
            )),
            toml::Value::Table(values) => Some(Value::Object(
                values
                    .iter()
                    .filter_map(|(key, value)| convert(value).map(|value| (key.clone(), value)))
                    .collect(),
            )),
        }
    }

    let mut spec = serde_json::Map::new();
    for (key, value) in entry {
        let output_key = if key == "http_headers" {
            "headers"
        } else {
            key
        };
        if let Some(value) = convert(value) {
            spec.insert(output_key.to_string(), value);
        }
    }
    let default_type = if spec.contains_key("url") {
        "http"
    } else {
        "stdio"
    };
    spec.entry("type".to_string())
        .or_insert_with(|| json!(default_type));
    Value::Object(spec)
}

pub fn import_from_grokbuild(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let text = read_config_text()?;
    if text.trim().is_empty() {
        return Ok(0);
    }
    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.grok/config.toml 失败: {e}")))?;
    let Some(entries) = root.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Ok(0);
    };

    let servers = config
        .mcp
        .servers
        .get_or_insert_with(std::collections::HashMap::new);
    let mut changed = 0;
    for (id, entry) in entries {
        let Some(entry) = entry.as_table() else {
            continue;
        };
        let spec = toml_server_to_json(entry);
        if let Err(error) = validate_server_spec(&spec) {
            log::warn!("跳过无效 Grok Build MCP 项 '{id}': {error}");
            continue;
        }

        if let Some(existing) = servers.get_mut(id) {
            if !existing.apps.grokbuild {
                existing.apps.grokbuild = true;
                changed += 1;
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec,
                    apps: McpApps {
                        grokbuild: true,
                        ..Default::default()
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
        }
    }
    Ok(changed)
}

pub fn sync_single_server_to_grokbuild(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_grokbuild_mcp() {
        return Ok(());
    }
    use toml_edit::Item;

    let path = crate::grok_config::get_grok_config_path();
    let text = read_config_text()?;
    let mut doc = if text.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        text.parse::<toml_edit::DocumentMut>().map_err(|e| {
            AppError::McpValidation(format!("解析 Grok Build config.toml 失败: {e}"))
        })?
    };
    // 若 mcp_servers 缺失或存在但不是 table（如 `mcp_servers = "x"` / `[]`），
    // 用空 table 归一化，避免后续 `doc["mcp_servers"][id] = …` 对非 table 索引
    // 触发 toml_edit 的 IndexMut panic。用户手写的 config.toml 不可信。
    // 判定走不可变的 `as_table_like`：借可变引用只为判空，会逼着下面再 get_mut 一次。
    if doc
        .get("mcp_servers")
        .is_none_or(|item| item.as_table_like().is_none())
    {
        // 归一化会丢掉用户手写的那个非表值，必须留痕。
        if doc.get("mcp_servers").is_some_and(|item| !item.is_none()) {
            log::warn!("Grok Build config.toml 的 mcp_servers 不是表，已重置为空表");
        }
        doc["mcp_servers"] = toml_edit::table();
    }
    let servers = doc
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            AppError::McpValidation("Grok Build config.toml 的 mcp_servers 不是表".to_string())
        })?;
    servers.insert(
        id,
        Item::Table(json_server_to_grokbuild_toml_table(server_spec)?),
    );
    crate::config::write_text_file(&path, &doc.to_string())
}

pub fn remove_server_from_grokbuild(id: &str) -> Result<(), AppError> {
    if !should_sync_grokbuild_mcp() {
        return Ok(());
    }
    let path = crate::grok_config::get_grok_config_path();
    if !path.exists() {
        return Ok(());
    }
    let text = read_config_text()?;
    let mut doc = match text.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            log::warn!("解析 Grok Build config.toml 失败: {error}，跳过删除操作");
            return Ok(());
        }
    };
    // 与写入侧对称使用 as_table_like_mut：inline table 形态下 as_table_mut 返回
    // None，删除会静默失效——界面显示已移除，Grok Build 下次启动仍会加载它。
    if let Some(item) = doc.get_mut("mcp_servers") {
        // `Item::None` 是 toml_edit 的占位形态，不是用户写下的值——对它告警是噪音。
        // 必须在取可变借用之前算出来。
        let user_authored = !item.is_none();
        match item.as_table_like_mut() {
            Some(servers) => {
                servers.remove(id);
            }
            None if user_authored => {
                log::warn!("Grok Build config.toml 的 mcp_servers 不是表，无法删除服务器 '{id}'");
            }
            None => {}
        }
    }
    crate::config::write_text_file(&path, &doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_http_headers_to_unified_headers() {
        let entry: toml::value::Table = toml::from_str(
            r#"type = "http"
url = "https://example.com/mcp"
http_headers = { Authorization = "Bearer token" }
"#,
        )
        .expect("parse table");
        let spec = toml_server_to_json(&entry);
        assert_eq!(spec["type"], "http");
        assert_eq!(spec["headers"]["Authorization"], "Bearer token");
    }

    #[test]
    fn writes_grokbuild_remote_server_without_codex_only_fields() {
        let table = json_server_to_grokbuild_toml_table(&json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer token" }
        }))
        .expect("convert server");

        assert!(!table.contains_key("type"));
        assert!(!table.contains_key("http_headers"));
        assert_eq!(
            table
                .get("headers")
                .and_then(toml_edit::Item::as_table)
                .and_then(|headers| headers.get("Authorization"))
                .and_then(toml_edit::Item::as_str),
            Some("Bearer token")
        );
    }
}
