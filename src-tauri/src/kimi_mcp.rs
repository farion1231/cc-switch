//! Kimi Code MCP 配置文件读写模块
//!
//! 处理 `~/.kimi-code/mcp.json` 的读写（标准 `{"mcpServers": {...}}` 结构）。
//!
//! Kimi Code 条目三种形态（不使用统一结构的 `type` 字段）：
//! - stdio: `{"command", "args"[], 可选 "env"{}, "cwd"}`
//! - http:  `{"url", 可选 "headers"{}}`
//! - sse:   `{"transport": "sse", "url"}`
//!
//! 与统一 MCP 结构（`type: stdio/http/sse`）的映射规则：
//! - 有 `command` → stdio
//! - 有 `url` 且 `transport == "sse"` → sse
//! - 有 `url` → http

use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;
use crate::error::AppError;
use crate::kimi_config::get_kimi_mcp_path;

/// 获取 Kimi Code MCP 配置文件路径（~/.kimi-code/mcp.json）
fn user_config_path() -> PathBuf {
    get_kimi_mcp_path()
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

/// 将 Kimi mcp.json 条目转换为统一 MCP 结构（补齐 `type` 字段）：
/// - 有 `command` → `type: "stdio"`
/// - 有 `url` 且 `transport == "sse"` → `type: "sse"`（移除 `transport`）
/// - 有 `url` → `type: "http"`
fn kimi_spec_to_unified(obj: &mut Map<String, Value>) {
    if obj.get("type").is_some() {
        return;
    }

    if obj.contains_key("command") {
        obj.insert("type".to_string(), Value::String("stdio".to_string()));
    } else if obj.contains_key("url") {
        let is_sse = obj.get("transport").and_then(|v| v.as_str()) == Some("sse");
        if is_sse {
            obj.remove("transport");
            obj.insert("type".to_string(), Value::String("sse".to_string()));
        } else {
            obj.insert("type".to_string(), Value::String("http".to_string()));
        }
    }
}

/// 将统一 MCP 结构转换为 Kimi mcp.json 条目：
/// - stdio（或缺省 type）：移除 `type` 与 `transport`，保留 command/args/env/cwd
/// - http：移除 `type` 与 `transport`，保留 url/headers
/// - sse：移除 `type`，写入 `transport: "sse"`，保留 url
fn unified_spec_to_kimi(obj: &mut Map<String, Value>) {
    let transport_type = obj.get("type").and_then(|v| v.as_str()).map(str::to_owned);

    obj.remove("type");
    obj.remove("transport");

    if transport_type.as_deref() == Some("sse") {
        obj.insert("transport".to_string(), Value::String("sse".to_string()));
    }
}

/// 读取 ~/.kimi-code/mcp.json 中的 mcpServers 映射
///
/// 执行反向格式转换以保持与统一 MCP 结构的兼容性（见 `kimi_spec_to_unified`）。
pub fn read_mcp_servers_map() -> Result<std::collections::HashMap<String, Value>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let root = read_json_value(&path)?;
    let mut servers: std::collections::HashMap<String, Value> = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    // 反向格式转换：Kimi 特有格式 → 统一 MCP 格式
    for (_, spec) in servers.iter_mut() {
        if let Some(obj) = spec.as_object_mut() {
            kimi_spec_to_unified(obj);
        }
    }

    Ok(servers)
}

/// 将给定的启用 MCP 服务器映射写入到 ~/.kimi-code/mcp.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), AppError> {
    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    // 构建 mcpServers 对象：移除 UI 辅助字段（enabled/source），仅保留实际 MCP 规范
    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers.iter() {
        let mut obj = if let Some(map) = spec.as_object() {
            map.clone()
        } else {
            return Err(AppError::McpValidation(format!(
                "MCP 服务器 '{id}' 不是对象"
            )));
        };

        // 提取 server 字段（如果存在）
        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
            obj = server_obj;
        }

        // Kimi 格式转换：type stdio/http/sse → 字段形态（见 `unified_spec_to_kimi`）
        unified_spec_to_kimi(&mut obj);

        // 移除 UI 辅助字段（Kimi 不需要）
        obj.remove("enabled");
        obj.remove("source");
        obj.remove("id");
        obj.remove("name");
        obj.remove("description");
        obj.remove("tags");
        obj.remove("homepage");
        obj.remove("docs");

        out.insert(id.clone(), Value::Object(obj));
    }

    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.kimi-code/mcp.json 根必须是对象".into()))?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn import_stdio_spec_gets_stdio_type() {
        let mut obj = json!({"command": "npx", "args": ["-y", "foo"], "env": {"A": "1"}})
            .as_object()
            .unwrap()
            .clone();
        kimi_spec_to_unified(&mut obj);
        assert_eq!(obj["type"], "stdio");
        assert_eq!(obj["command"], "npx");
        assert!(!obj.contains_key("transport"));
    }

    #[test]
    fn import_url_spec_gets_http_type() {
        let mut obj = json!({"url": "https://example.com/mcp", "headers": {"X": "y"}})
            .as_object()
            .unwrap()
            .clone();
        kimi_spec_to_unified(&mut obj);
        assert_eq!(obj["type"], "http");
        assert_eq!(obj["url"], "https://example.com/mcp");
    }

    #[test]
    fn import_sse_spec_gets_sse_type_and_drops_transport() {
        let mut obj = json!({"transport": "sse", "url": "https://example.com/sse"})
            .as_object()
            .unwrap()
            .clone();
        kimi_spec_to_unified(&mut obj);
        assert_eq!(obj["type"], "sse");
        assert!(!obj.contains_key("transport"));
    }

    #[test]
    fn import_existing_type_is_respected() {
        let mut obj = json!({"type": "http", "url": "https://example.com/mcp"})
            .as_object()
            .unwrap()
            .clone();
        kimi_spec_to_unified(&mut obj);
        assert_eq!(obj["type"], "http");
    }

    #[test]
    fn export_stdio_spec_drops_type_and_transport() {
        let mut obj = json!({"type": "stdio", "command": "npx", "args": ["-y", "foo"]})
            .as_object()
            .unwrap()
            .clone();
        unified_spec_to_kimi(&mut obj);
        assert!(!obj.contains_key("type"));
        assert!(!obj.contains_key("transport"));
        assert_eq!(obj["command"], "npx");
    }

    #[test]
    fn export_http_spec_drops_type() {
        let mut obj = json!({"type": "http", "url": "https://example.com/mcp"})
            .as_object()
            .unwrap()
            .clone();
        unified_spec_to_kimi(&mut obj);
        assert!(!obj.contains_key("type"));
        assert!(!obj.contains_key("transport"));
        assert_eq!(obj["url"], "https://example.com/mcp");
    }

    #[test]
    fn export_sse_spec_writes_transport() {
        let mut obj = json!({"type": "sse", "url": "https://example.com/sse"})
            .as_object()
            .unwrap()
            .clone();
        unified_spec_to_kimi(&mut obj);
        assert!(!obj.contains_key("type"));
        assert_eq!(obj["transport"], "sse");
        assert_eq!(obj["url"], "https://example.com/sse");
    }

    #[test]
    fn kimi_format_round_trip() {
        // stdio / http / sse 三种形态 import→export 应保持 Kimi 原格式
        for raw in [
            json!({"command": "npx", "args": ["-y", "foo"]}),
            json!({"url": "https://example.com/mcp", "headers": {"X": "y"}}),
            json!({"transport": "sse", "url": "https://example.com/sse"}),
        ] {
            let mut obj = raw.as_object().unwrap().clone();
            kimi_spec_to_unified(&mut obj);
            unified_spec_to_kimi(&mut obj);
            assert_eq!(Value::Object(obj), raw);
        }
    }
}
