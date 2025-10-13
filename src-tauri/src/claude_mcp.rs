use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub user_config_path: String,
    pub user_config_exists: bool,
    pub server_count: usize,
}

fn user_config_path() -> PathBuf {
    // 用户级 MCP 配置文件：~/.claude.json
    dirs::home_dir()
        .expect("无法获取用户主目录")
        .join(".claude.json")
}

fn read_json_value(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content =
        fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}: {}", path.display(), e))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 JSON 失败: {}: {}", path.display(), e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败: {}: {}", parent.display(), e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| format!("序列化 JSON 失败: {}", e))?;
    atomic_write(path, json.as_bytes())
}

pub fn get_mcp_status() -> Result<McpStatus, String> {
    let path = user_config_path();
    let (exists, count) = if path.exists() {
        let v = read_json_value(&path)?;
        let servers = v.get("mcpServers").and_then(|x| x.as_object());
        (true, servers.map(|m| m.len()).unwrap_or(0))
    } else {
        (false, 0)
    };

    Ok(McpStatus {
        user_config_path: path.to_string_lossy().to_string(),
        user_config_exists: exists,
        server_count: count,
    })
}

pub fn read_mcp_json() -> Result<Option<String>, String> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("读取 MCP 配置失败: {}", e))?;
    Ok(Some(content))
}

pub fn upsert_mcp_server(id: &str, spec: Value) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Err("MCP 服务器 ID 不能为空".into());
    }
    // 基础字段校验（尽量宽松）
    if !spec.is_object() {
        return Err("MCP 服务器定义必须为 JSON 对象".into());
    }
    let t_opt = spec.get("type").and_then(|x| x.as_str());
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true); // 兼容缺省（按 stdio 处理）
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    if !(is_stdio || is_http) {
        return Err("MCP 服务器 type 必须是 'stdio' 或 'http'（或省略表示 stdio）".into());
    }

    // stdio 类型必须有 command
    if is_stdio {
        let cmd = spec.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.is_empty() {
            return Err("stdio 类型的 MCP 服务器缺少 command 字段".into());
        }
    }

    // http 类型必须有 url
    if is_http {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.is_empty() {
            return Err("http 类型的 MCP 服务器缺少 url 字段".into());
        }
    }

    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    // 确保 mcpServers 对象存在
    {
        let obj = root
            .as_object_mut()
            .ok_or_else(|| "mcp.json 根必须是对象".to_string())?;
        if !obj.contains_key("mcpServers") {
            obj.insert("mcpServers".into(), serde_json::json!({}));
        }
    }

    let before = root.clone();
    if let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.insert(id.to_string(), spec);
    }

    if before == root && path.exists() {
        return Ok(false);
    }

    write_json_value(&path, &root)?;
    Ok(true)
}

pub fn delete_mcp_server(id: &str) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Err("MCP 服务器 ID 不能为空".into());
    }
    let path = user_config_path();
    if !path.exists() {
        return Ok(false);
    }
    let mut root = read_json_value(&path)?;
    let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
        return Ok(false);
    };
    let existed = servers.remove(id).is_some();
    if !existed {
        return Ok(false);
    }
    write_json_value(&path, &root)?;
    Ok(true)
}

pub fn validate_command_in_path(cmd: &str) -> Result<bool, String> {
    if cmd.trim().is_empty() {
        return Ok(false);
    }
    // 如果包含路径分隔符，直接判断是否存在可执行文件
    if cmd.contains('/') || cmd.contains('\\') {
        return Ok(Path::new(cmd).exists());
    }

    let path_var = env::var_os("PATH").unwrap_or_default();
    let paths = env::split_paths(&path_var);

    #[cfg(windows)]
    let exts: Vec<String> = env::var("PATHEXT")
        .unwrap_or(".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(|s| s.trim().to_uppercase())
        .collect();

    for p in paths {
        let candidate = p.join(cmd);
        if candidate.is_file() {
            return Ok(true);
        }
        #[cfg(windows)]
        {
            for ext in &exts {
                let cand = p.join(format!("{}{}", cmd, ext));
                if cand.is_file() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// 将给定的启用 MCP 服务器映射写入到用户级 ~/.claude.json 的 mcpServers 字段
/// 仅覆盖 mcpServers，其他字段保持不变
pub fn set_mcp_servers_map(
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), String> {
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
            return Err(format!("MCP 服务器 '{}' 不是对象", id));
        };

        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val
                .as_object()
                .cloned()
                .ok_or_else(|| format!("MCP 服务器 '{}' server 字段不是对象", id))?;
            obj = server_obj;
        }

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
            .ok_or_else(|| "~/.claude.json 根必须是对象".to_string())?;
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    write_json_value(&path, &root)?;
    Ok(())
}
