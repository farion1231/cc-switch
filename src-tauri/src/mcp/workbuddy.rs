//! WorkBuddy 的用户级 MCP：`<data dir>/mcp.json`（默认 `~/.workbuddy/mcp.json`）。
//!
//! 格式与 CodeBuddy Code 相同：`{"mcpServers": {"<name>": {...}}}`。stdio 用
//! `command` / `args` / `env` / `cwd`，远程用 `url` / `headers`，`type` 取 `stdio` /
//! `http` / `sse`（缺省时有 `command` 视为 stdio，有 `url` 视为 http）。WorkBuddy 桌面端
//! 自己的 MCP 设置页读写的也是这个文件，单条用 `disabled: true` 停用；`managedBy:
//! "connector"` 的条目归连接器管理，这里不导入也不覆盖。
//!
//! 写入时只替换传输字段，条目上的其他字段（`timeout`、`description`、`defer_loading`、
//! WorkBuddy 自己的标记等）、用户自己加的服务器和顶层其他字段都原样保留。
use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, path::PathBuf, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 7] = ["type", "command", "args", "env", "cwd", "url", "headers"];

pub(crate) fn config_path() -> PathBuf {
    crate::workbuddy_config::data_dir().join("mcp.json")
}

fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"mcpServers": {}})),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Ok(json!({"mcpServers": {}}));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|_| AppError::Config("Invalid WorkBuddy MCP JSON".into()))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config(
            "Invalid WorkBuddy mcpServers object".into(),
        ));
    }
    Ok(value)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    sync_and_commit(id, spec, || Ok(()))
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path();
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

fn is_connector(entry: &Value) -> bool {
    entry.get("managedBy").and_then(Value::as_str) == Some("connector")
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let mut document = read(path)?;
    let present = document
        .get("mcpServers")
        .and_then(|servers| servers.get(id))
        .is_some();
    if spec.is_none() && !present {
        return Ok(());
    }
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"].as_object_mut().unwrap();
    if servers.get(id).is_some_and(is_connector) {
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy MCP server '{id}' is managed by a WorkBuddy connector"
        )));
    }
    match spec {
        Some(spec) => {
            let native = unified_spec(spec);
            super::validation::validate_server_spec(&native)?;
            let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
            let object = merged
                .as_object_mut()
                .ok_or_else(|| AppError::Config("Invalid WorkBuddy MCP entry".into()))?;
            for field in TRANSPORT_FIELDS {
                object.remove(field);
            }
            object.remove("disabled");
            object.extend(native.as_object().cloned().unwrap_or_default());
            servers.insert(id.into(), merged);
        }
        None => {
            servers.remove(id);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json = serde_json::to_string_pretty(&document)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_private(path, format!("{json}\n").as_bytes())
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&config_path())?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document
        .get("mcpServers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if is_connector(native) {
            continue;
        }
        let spec = transport_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = native.get("disabled").and_then(Value::as_bool) != Some(true);
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != spec {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.workbuddy = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    workbuddy: enabled,
                    ..Default::default()
                },
                description: native
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_string),
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
            "Imported {count} WorkBuddy MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

/// 只保留传输相关字段的统一格式，用于导入和冲突比较。
fn transport_spec(spec: &Value) -> Value {
    let mut spec = unified_spec(spec);
    if let Some(object) = spec.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    }
    spec
}

/// 补齐 `type`：WorkBuddy 和 CC Switch 都用 `stdio` / `http` / `sse`；`streamable-http`
/// 是同一种传输的别名。
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    let Some(object) = spec.as_object_mut() else {
        return spec;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("streamable-http" | "streamableHttp" | "remote") => {
            object.insert("type".into(), json!("http"));
        }
        Some(_) => {}
        None if object.contains_key("command") => {
            object.insert("type".into(), json!("stdio"));
        }
        None if object.contains_key("url") => {
            object.insert("type".into(), json!("http"));
        }
        None => {}
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbuddy_mcp_writes_transports_and_preserves_native_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            json!({
                "custom": "keep",
                "mcpServers": {
                    "server": {"url": "https://old.example.com/sse", "type": "sse", "disabled": true, "timeout": 1234, "description": "mine"},
                    "user-added": {"command": "node", "args": ["server.js"]},
                    "connector": {"url": "https://connector.example.com", "managedBy": "connector"}
                }
            })
            .to_string(),
        )
        .unwrap();

        sync_file(
            &path,
            "server",
            Some(&json!({"type":"http","url":"https://example.com/mcp","headers":{"X":"1"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "http");
        assert_eq!(server["url"], "https://example.com/mcp");
        assert_eq!(server["headers"]["X"], "1");
        assert!(server.get("disabled").is_none());
        assert_eq!(server["timeout"], 1234);
        assert_eq!(server["description"], "mine");

        sync_file(
            &path,
            "server",
            Some(&json!({"command":"npx","args":["-y","pkg"],"env":{"A":"b"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "stdio");
        assert_eq!(server["command"], "npx");
        assert!(server.get("url").is_none());
        assert!(server.get("headers").is_none());

        sync_file(&path, "server", None).unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"].get("server").is_none());
        assert_eq!(value["mcpServers"]["user-added"]["command"], "node");
        assert_eq!(value["mcpServers"]["connector"]["managedBy"], "connector");
        assert_eq!(value["custom"], "keep");

        // 连接器托管的条目不覆盖。
        assert!(sync_file(&path, "connector", Some(&json!({"command":"node"}))).is_err());
    }

    #[test]
    fn workbuddy_mcp_handles_missing_empty_and_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("mcp.json");
        // 删除一个不存在的服务器不创建文件。
        sync_file(&path, "missing", None).unwrap();
        assert!(!path.exists());
        sync_file(&path, "s", Some(&json!({"command":"node"}))).unwrap();
        assert_eq!(read(&path).unwrap()["mcpServers"]["s"]["type"], "stdio");

        fs::write(&path, "").unwrap();
        sync_file(&path, "s", Some(&json!({"url":"https://example.com"}))).unwrap();
        assert_eq!(read(&path).unwrap()["mcpServers"]["s"]["type"], "http");

        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "s", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }

    #[test]
    fn workbuddy_mcp_unifies_native_types() {
        assert_eq!(
            transport_spec(&json!({"url":"https://x","timeout":5}))["type"],
            "http"
        );
        assert_eq!(
            transport_spec(&json!({"type":"streamable-http","url":"https://x"}))["type"],
            "http"
        );
        assert_eq!(
            transport_spec(&json!({"type":"sse","url":"https://x"}))["type"],
            "sse"
        );
        let stdio = transport_spec(&json!({"command":"node","defer_loading":false}));
        assert_eq!(stdio["type"], "stdio");
        assert!(stdio.get("defer_loading").is_none());
    }
}
