use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;
use crate::error::AppError;
use crate::gemini_config::get_gemini_settings_path;

/// 获取 Gemini MCP 配置文件路径（~/.gemini/settings.json）
fn user_config_path() -> PathBuf {
    get_gemini_settings_path()
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

/// 读取 Gemini settings.json 中的 mcpServers 映射
///
/// 执行反向格式转换以保持与统一 MCP 结构的兼容性：
/// - httpUrl → url + type: "http"
/// - 仅有 url 字段 → 补齐 type: "sse"（Gemini 以字段名推断传输类型）
/// - 仅有 command 字段 → 补齐 type: "stdio"
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

    // 反向格式转换：Gemini 特有格式 → 统一 MCP 格式
    for (_, spec) in servers.iter_mut() {
        if let Some(obj) = spec.as_object_mut() {
            // httpUrl → url + type: "http"
            if let Some(http_url) = obj.remove("httpUrl") {
                obj.insert("url".to_string(), http_url);
                obj.insert("type".to_string(), Value::String("http".to_string()));
            }

            // Gemini CLI 不使用 type 字段：这里补齐成统一结构，便于校验与导入
            if obj.get("type").is_none() {
                if obj.contains_key("command") {
                    obj.insert("type".to_string(), Value::String("stdio".to_string()));
                } else if obj.contains_key("url") {
                    obj.insert("type".to_string(), Value::String("sse".to_string()));
                }
            }
        }
    }

    Ok(servers)
}

/// 统一规范 → Gemini CLI 条目
fn to_gemini_entry(mut obj: Map<String, Value>) -> Map<String, Value> {
    // Gemini CLI 格式转换：
    // - Gemini 不使用 "type" 字段（从字段名推断传输类型）
    // - HTTP 使用 "httpUrl" 字段，SSE 使用 "url" 字段
    // 没写 type 的 url 型规范按 http 处理（与其他应用的投影一致），否则写成
    // `url` 会被 Gemini 当作 SSE 连接 Streamable HTTP 端点。
    let spec_value = Value::Object(obj.clone());
    let transport_type = crate::mcp::validation::spec_transport_type(&spec_value);
    if transport_type == "http" {
        // HTTP streaming: 将 "url" 重命名为 "httpUrl"
        if let Some(url_value) = obj.remove("url") {
            obj.insert("httpUrl".to_string(), url_value);
        }
    }
    // SSE 保持 "url" 字段不变

    // 移除 UI 辅助字段和 type 字段（Gemini 不需要）
    obj.remove("type");
    obj.remove("enabled");
    obj.remove("source");
    obj.remove("id");
    obj.remove("name");
    obj.remove("description");
    obj.remove("tags");
    obj.remove("homepage");
    obj.remove("docs");

    // Timeout 转换：Claude/Codex 使用 startup_timeout_sec/tool_timeout_sec
    // Gemini CLI 只支持 timeout（单位 ms）
    // 默认值：startup=10s, tool=60s
    const DEFAULT_STARTUP_MS: u64 = 10_000;
    const DEFAULT_TOOL_MS: u64 = 60_000;

    let extract_timeout =
        |obj: &mut Map<String, Value>, key: &str, multiplier: u64| -> Option<u64> {
            obj.remove(key).and_then(|val| {
                val.as_u64()
                    .map(|n| n * multiplier)
                    .or_else(|| val.as_f64().map(|f| (f * multiplier as f64) as u64))
            })
        };

    // 分别收集 startup 和 tool timeout，未设置时使用默认值
    let startup_ms = extract_timeout(&mut obj, "startup_timeout_sec", 1000)
        .or_else(|| extract_timeout(&mut obj, "startup_timeout_ms", 1));
    let tool_ms = extract_timeout(&mut obj, "tool_timeout_sec", 1000)
        .or_else(|| extract_timeout(&mut obj, "tool_timeout_ms", 1));

    // 规范里已有 Gemini 原生的 `timeout`（毫秒，比如从 Gemini 导入的条目）时以它为准，
    // 不能被默认值覆盖成 60 秒。
    if obj.get("timeout").is_some_and(Value::is_number) {
        return obj;
    }

    // 取最大值作为 Gemini timeout
    let final_timeout = startup_ms
        .unwrap_or(DEFAULT_STARTUP_MS)
        .max(tool_ms.unwrap_or(DEFAULT_TOOL_MS));
    obj.insert("timeout".to_string(), Value::Number(final_timeout.into()));
    obj
}

/// 将给定的启用 MCP 服务器映射写入到 Gemini settings.json 的 mcpServers 字段
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

        out.insert(id.clone(), Value::Object(to_gemini_entry(obj)));
    }

    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| AppError::Config("~/.gemini/settings.json 根必须是对象".into()))?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(spec: Value) -> Value {
        Value::Object(to_gemini_entry(spec.as_object().unwrap().clone()))
    }

    #[test]
    fn url_only_spec_is_written_as_streamable_http() {
        // 不写 type 的 url 规范在其他应用里都按 http 投影；写成 `url` 会被 Gemini 当成 SSE。
        let out = entry(json!({"url": "https://example.com/mcp"}));
        assert_eq!(out["httpUrl"], "https://example.com/mcp");
        assert!(out.get("url").is_none());
        assert!(out.get("type").is_none());

        let sse = entry(json!({"type": "sse", "url": "https://example.com/sse"}));
        assert_eq!(sse["url"], "https://example.com/sse");
        assert!(sse.get("httpUrl").is_none());
    }

    #[test]
    fn native_timeout_is_kept_and_codex_timeouts_are_converted() {
        let native = entry(json!({"command": "node", "timeout": 600000}));
        assert_eq!(native["timeout"], 600000);

        let converted = entry(json!({"command": "node", "tool_timeout_sec": 120}));
        assert_eq!(converted["timeout"], 120000);
        assert!(converted.get("tool_timeout_sec").is_none());

        // 两种都没有：沿用原来的默认值
        assert_eq!(entry(json!({"command": "node"}))["timeout"], 60000);
    }
}
