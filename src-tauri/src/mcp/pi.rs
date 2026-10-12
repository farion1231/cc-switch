//! Pi（1.0+ 内置 MCP）的 MCP 同步：用户级 `<Pi 配置目录>/mcp.json`。
//!
//! 格式与 Claude / MiniMax Code 相同（顶层 `mcpServers`），差别：
//! - 不支持 SSE；`type` 只能是 `stdio` / `http` / `streamable-http` 或省略
//! - 服务器名只能用字母、数字、`_`、`-`，只差 `-` 和 `_` 的两个名字算同一个
//! - `enabled`、`timeout`、`description`、`exposure`、`toolExposure`、`oauth`、`auth`
//!   是 Pi 自己的字段，写入时原样保留，只替换连接字段
//!
//! 项目级 `.pi/mcp.json` 不在这里管。

use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 7] = ["command", "args", "env", "cwd", "url", "headers", "type"];
/// Pi 自己的条目字段：重建条目时从数据库里保存的连接定义（导入时原样存下）带回来
const PI_FIELDS: [&str; 6] = [
    "timeout",
    "description",
    "exposure",
    "toolExposure",
    "oauth",
    "auth",
];

/// 对 Pi 条目的改动
#[derive(Clone, Copy)]
pub(crate) enum PiChange<'a> {
    /// 写入连接字段并启用
    Enable(&'a Value),
    /// 取消勾选：只标 `enabled: false`，保留条目和 Pi 自己的设置，重新勾选时原样恢复
    Disable,
    /// 从 CC Switch 删除服务器：删掉条目
    Remove,
}

impl<'a> PiChange<'a> {
    /// 勾选状态 → 改动：勾选写入，未勾选只禁用
    pub(crate) fn from_enabled(spec: Option<&'a Value>) -> Self {
        spec.map_or(Self::Disable, Self::Enable)
    }
}

pub(crate) fn config_path() -> Result<PathBuf, AppError> {
    Ok(crate::pi_config::get_pi_agent_dir()?.join("mcp.json"))
}

fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"mcpServers": {}})),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let value: Value =
        serde_json::from_str(&text).map_err(|_| AppError::Config("Invalid Pi MCP JSON".into()))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config("Invalid Pi mcpServers object".into()));
    }
    Ok(value)
}

/// Pi 的服务器名规则：只能用字母、数字、`_`、`-`
fn validate_name(id: &str) -> Result<(), AppError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::McpValidation(format!(
            "Pi 的 MCP 服务器名只能包含字母、数字、_ 和 -：{id}"
        )));
    }
    Ok(())
}

/// 只差 `-` 和 `_` 的名字在 Pi 里是同一个服务器
fn normalized_name(id: &str) -> String {
    id.replace('-', "_")
}

/// CC Switch 的统一格式 → 写进 Pi 的连接字段。SSE 直接拒绝
fn native_transport(spec: &Value) -> Result<Value, AppError> {
    super::validation::validate_server_spec(spec)?;
    if spec.get("type").and_then(Value::as_str) == Some("sse") {
        return Err(AppError::McpValidation(
            "Pi 不支持 SSE 连接方式的 MCP 服务器，请改用 HTTP（streamable HTTP）".into(),
        ));
    }
    let mut native = spec.clone();
    native
        .as_object_mut()
        .expect("validated MCP spec is an object")
        .retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    Ok(native)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(&config_path()?, id, PiChange::from_enabled(spec))
}

/// 删除服务器时清掉 Pi 里由 CC Switch 写入、后来被取消勾选的条目：
/// 只删 `enabled: false` 且连接方式和这个服务器一致的同名条目，用户自己的同名条目不动
pub(crate) fn remove_disabled_if_managed(id: &str, spec: &Value) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path()?;
    if !path.exists() {
        return Ok(());
    }
    let managed = read(&path)?["mcpServers"].get(id).is_some_and(|entry| {
        entry.get("enabled") == Some(&json!(false)) && transport_spec(entry) == transport_spec(spec)
    });
    if managed {
        sync_file(&path, id, PiChange::Remove)?;
    }
    Ok(())
}

/// 先写 Pi 的 `mcp.json` 再提交数据库；数据库失败就把文件恢复原样
pub(crate) fn sync_and_commit<T>(
    id: &str,
    change: PiChange<'_>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path()?;
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, change), commit)
}

fn sync_file(path: &Path, id: &str, change: PiChange<'_>) -> Result<(), AppError> {
    let mut document = read(path)?;
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"].as_object_mut().unwrap();
    match change {
        PiChange::Enable(spec) => {
            validate_name(id)?;
            let key = normalized_name(id);
            if let Some(other) = servers
                .keys()
                .find(|name| name.as_str() != id && normalized_name(name) == key)
            {
                return Err(AppError::McpValidation(format!(
                    "Pi 把只差 - 和 _ 的服务器名视为同一个：{id} 与已有的 {other} 冲突"
                )));
            }
            let transport = native_transport(spec)?;
            // 条目还在就沿用它（用户在 Pi 里改过的设置优先）；不在就从保存的 Pi 字段重建
            let mut merged = servers.get(id).cloned().unwrap_or_else(|| pi_fields(spec));
            let object = merged
                .as_object_mut()
                .ok_or_else(|| AppError::Config("Invalid Pi MCP entry".into()))?;
            for field in TRANSPORT_FIELDS {
                object.remove(field);
            }
            object.extend(transport.as_object().unwrap().clone());
            object.insert("enabled".into(), json!(true));
            servers.insert(id.into(), merged);
        }
        PiChange::Disable => match servers.get_mut(id) {
            Some(Value::Object(entry)) => {
                entry.insert("enabled".into(), json!(false));
            }
            // 没有条目就没什么可禁用的，不为此新建
            _ => return Ok(()),
        },
        PiChange::Remove => {
            if servers.remove(id).is_none() {
                return Ok(());
            }
        }
    }
    atomic_write_private(
        path,
        serde_json::to_string_pretty(&document)
            .map_err(|e| AppError::Config(e.to_string()))?
            .as_bytes(),
    )
}

/// 从 Pi 的 `mcp.json` 导入。Pi 自己的字段（`timeout`、`exposure` 等）留在连接定义里原样保存；
/// `enabled: false` 的条目导入后不勾选 Pi。和已有服务器连接方式不同的同名条目跳过并报告。
pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&config_path()?)?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let mut spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = native
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        spec.as_object_mut().unwrap().remove("enabled");
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.pi = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    pi: enabled,
                    ..Default::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            }
        };
        state.db.save_mcp_server(&server)?;
    }
    if skipped.is_empty() {
        Ok(count)
    } else {
        Err(AppError::InvalidInput(format!(
            "Imported {count} Pi MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

fn transport_spec(spec: &Value) -> Value {
    let mut spec = unified_spec(spec);
    if let Some(object) = spec.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    }
    spec
}

/// 连接定义里保存的 Pi 字段（导入时原样存下）→ 重建条目的起点
fn pi_fields(spec: &Value) -> Value {
    let mut fields = serde_json::Map::new();
    if let Some(object) = spec.as_object() {
        for field in PI_FIELDS {
            if let Some(value) = object.get(field) {
                fields.insert(field.into(), value.clone());
            }
        }
    }
    Value::Object(fields)
}

/// Pi 的条目 → CC Switch 的统一格式：`streamable-http` 记作 `http`，省略的 `type` 按字段补上
pub(crate) fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    if spec["type"] == "streamable-http"
        || (spec.is_object() && spec.get("type").is_none() && spec.get("url").is_some())
    {
        spec["type"] = json!("http");
    } else if spec.is_object() && spec.get("type").is_none() && spec.get("command").is_some() {
        spec["type"] = json!("stdio");
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_pi_fields_and_other_servers_when_replacing_the_transport() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let original = json!({
            "autoEnableCodemode": false,
            "mcpServers": {
                "other": { "command": "node" },
                "docs": {
                    "command": "old", "args": ["old"], "cwd": "/old",
                    "enabled": false, "timeout": 30, "description": "Docs",
                    "exposure": "direct", "toolExposure": { "delete_*": "hidden" },
                    "oauth": { "clientName": "Claude Code" }
                }
            }
        });
        fs::write(&path, original.to_string()).unwrap();

        sync_file(
            &path,
            "docs",
            PiChange::Enable(
                &json!({"type": "http", "url": "https://example.com/mcp", "headers": {"X": "1"}}),
            ),
        )
        .unwrap();
        let written = read(&path).unwrap();
        let docs = &written["mcpServers"]["docs"];
        assert_eq!(written["autoEnableCodemode"], false);
        assert_eq!(
            written["mcpServers"]["other"],
            original["mcpServers"]["other"]
        );
        assert_eq!(docs["url"], "https://example.com/mcp");
        assert_eq!(docs["type"], "http");
        assert!(docs.get("command").is_none() && docs.get("cwd").is_none());
        assert_eq!(docs["enabled"], true);
        for field in [
            "timeout",
            "description",
            "exposure",
            "toolExposure",
            "oauth",
        ] {
            assert_eq!(
                docs[field], original["mcpServers"]["docs"][field],
                "{field}"
            );
        }

        sync_file(&path, "docs", PiChange::Remove).unwrap();
        assert!(read(&path).unwrap()["mcpServers"].get("docs").is_none());
    }

    /// 审查 #7862：取消勾选再勾选不能丢掉 Pi 自己的设置
    #[test]
    fn disable_keeps_the_entry_and_enable_restores_it_with_pi_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let spec = json!({"type": "stdio", "command": "node", "args": ["server.js"]});
        let pi_settings = json!({
            "timeout": 30000,
            "exposure": "direct",
            "toolExposure": {"delete_*": "hidden"},
            "oauth": {"clientName": "custom-client"}
        });
        let mut native = pi_settings.clone();
        native["command"] = json!("node");
        native["args"] = json!(["server.js"]);
        fs::write(&path, json!({"mcpServers": {"srv": native}}).to_string()).unwrap();

        sync_file(&path, "srv", PiChange::Disable).unwrap();
        let disabled = &read(&path).unwrap()["mcpServers"]["srv"];
        assert_eq!(disabled["enabled"], false);
        sync_file(&path, "srv", PiChange::Enable(&spec)).unwrap();
        let entry = read(&path).unwrap()["mcpServers"]["srv"].clone();
        assert_eq!(entry["enabled"], true);
        for field in ["timeout", "exposure", "toolExposure", "oauth"] {
            assert_eq!(entry[field], pi_settings[field], "{field}");
        }

        // 条目被删掉了（比如用户在 Pi 里删的）：从保存的连接定义里的 Pi 字段重建
        sync_file(&path, "srv", PiChange::Remove).unwrap();
        let mut saved = spec.clone();
        saved
            .as_object_mut()
            .unwrap()
            .extend(pi_settings.as_object().unwrap().clone());
        saved["tools"] = json!([{"name": "not a pi field"}]);
        sync_file(&path, "srv", PiChange::Enable(&saved)).unwrap();
        let rebuilt = read(&path).unwrap()["mcpServers"]["srv"].clone();
        for field in ["timeout", "exposure", "toolExposure", "oauth"] {
            assert_eq!(rebuilt[field], pi_settings[field], "{field}");
        }
        assert!(rebuilt.get("tools").is_none());

        // 没有条目时禁用什么都不做，不新建
        sync_file(&path, "absent", PiChange::Disable).unwrap();
        assert!(read(&path).unwrap()["mcpServers"].get("absent").is_none());
    }

    #[test]
    fn rejects_what_pi_rejects_without_touching_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let original = r#"{"mcpServers":{"dev-tools":{"command":"node"}}}"#;
        fs::write(&path, original).unwrap();

        let sse = json!({"type": "sse", "url": "https://example.com/sse"});
        assert!(sync_file(&path, "remote", PiChange::Enable(&sse)).is_err());
        let stdio = json!({"command": "node"});
        assert!(sync_file(&path, "bad name", PiChange::Enable(&stdio)).is_err());
        assert!(sync_file(&path, "dev_tools", PiChange::Enable(&stdio)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        fs::write(&path, "broken json").unwrap();
        assert!(sync_file(&path, "new", PiChange::Enable(&stdio)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken json");
    }

    #[test]
    fn creates_the_file_and_maps_types_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        sync_file(
            &path,
            "fs",
            PiChange::Enable(&json!({"command": "npx", "args": ["-y", "server"], "cwd": "."})),
        )
        .unwrap();
        assert_eq!(read(&path).unwrap()["mcpServers"]["fs"]["cwd"], ".");

        assert_eq!(
            unified_spec(&json!({"type": "streamable-http", "url": "u"}))["type"],
            "http"
        );
        assert_eq!(unified_spec(&json!({"url": "u"}))["type"], "http");
        assert_eq!(unified_spec(&json!({"command": "c"}))["type"], "stdio");
    }
}
