//! GitHub Copilot CLI 的 MCP：`~/.copilot/mcp-config.json` 的 `mcpServers`。
//!
//! 原生格式：stdio 写作 `"type": "local"`（`command` / `args` / `env`），远程是 `http` 或
//! `sse`（`url` / `headers`）；每项都带 `tools` 过滤，默认 `["*"]`。禁用在 Copilot 里是
//! `settings.json` 的 `disabledMcpServers`，CC Switch 停用时直接移除条目（同 Kimi），
//! 导入时把禁用名单里的条目记为未启用。
use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 7] = ["command", "args", "env", "url", "headers", "type", "cwd"];

fn read(path: &Path) -> Result<Value, AppError> {
    let value = crate::copilot_config::read_json_document(path)?
        .unwrap_or_else(|| json!({ "mcpServers": {} }));
    if value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config(
            "Invalid GitHub Copilot CLI mcpServers object".into(),
        ));
    }
    Ok(value)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(&crate::copilot_config::mcp_config_path(), id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = crate::copilot_config::mcp_config_path();
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let mut document = read(path)?;
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"].as_object_mut().unwrap();
    if let Some(spec) = spec {
        let unified = unified_spec(spec);
        super::validation::validate_server_spec(&unified)?;
        let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
        let object = merged
            .as_object_mut()
            .ok_or_else(|| AppError::Config("Invalid GitHub Copilot CLI MCP entry".into()))?;
        for field in TRANSPORT_FIELDS {
            object.remove(field);
        }
        object.extend(native_spec(&unified).as_object().unwrap().clone());
        // 保留用户在 Copilot 里设的工具过滤；新条目和 `copilot mcp add` 一样默认全部。
        object.entry("tools").or_insert_with(|| json!(["*"]));
        servers.insert(id.into(), merged);
    } else {
        servers.remove(id);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write_private(
        path,
        serde_json::to_string_pretty(&document)
            .map_err(|e| AppError::Config(e.to_string()))?
            .as_bytes(),
    )
}

/// `settings.json` 的 `disabledMcpServers`；读不了时当作没有禁用项。
fn disabled_servers() -> Vec<String> {
    crate::copilot_config::read_settings_document()
        .ok()
        .flatten()
        .and_then(|settings| settings.get("disabledMcpServers").cloned())
        .and_then(|list| serde_json::from_value::<Vec<String>>(list).ok())
        .unwrap_or_default()
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&crate::copilot_config::mcp_config_path())?;
    let disabled = disabled_servers();
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = !disabled.iter().any(|name| name == id);
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.copilot = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    copilot: enabled,
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
            "Imported {count} GitHub Copilot CLI MCP servers; skipped {}. Native configurations were preserved.",
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

/// 原生条目 → CC Switch 统一格式：`local` 即 `stdio`，去掉 Copilot 独有的 `tools`、`timeout`。
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    let Some(object) = spec.as_object_mut() else {
        return spec;
    };
    object.remove("tools");
    object.remove("timeout");
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    match kind.as_deref() {
        Some("local") => {
            object.insert("type".into(), json!("stdio"));
        }
        Some("streamable-http") | Some("streamableHttp") => {
            object.insert("type".into(), json!("http"));
        }
        None if object.contains_key("url") => {
            object.insert("type".into(), json!("http"));
        }
        None if object.contains_key("command") => {
            object.insert("type".into(), json!("stdio"));
        }
        _ => {}
    }
    spec
}

/// 统一格式 → 原生条目，只留传输字段。
fn native_spec(unified: &Value) -> Value {
    let mut native = unified.clone();
    if let Some(object) = native.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
        if object.get("type").and_then(Value::as_str) == Some("stdio") {
            object.insert("type".into(), json!("local"));
            object.entry("args").or_insert_with(|| json!([]));
        }
    }
    native
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copilot_mcp_writes_native_transports_and_preserves_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        fs::write(
            &path,
            json!({"mcpServers":{
                "server":{"type":"http","url":"https://old.example.com","tools":["search"],"timeout":5000},
                "other":{"type":"local","command":"node","args":["server.js"],"tools":["*"]}
            }})
            .to_string(),
        )
        .unwrap();
        sync_file(
            &path,
            "server",
            Some(&json!({"type":"stdio","command":"npx","args":["-y","pkg"],"env":{"A":"1"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "local");
        assert_eq!(server["command"], "npx");
        assert_eq!(server["env"]["A"], "1");
        assert!(server.get("url").is_none());
        assert_eq!(server["tools"], json!(["search"]));
        assert_eq!(server["timeout"], 5000);
        assert_eq!(value["mcpServers"]["other"]["command"], "node");

        sync_file(
            &path,
            "remote",
            Some(&json!({"type":"sse","url":"https://example.com/sse","headers":{"X":"1"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert_eq!(value["mcpServers"]["remote"]["type"], "sse");
        assert_eq!(value["mcpServers"]["remote"]["tools"], json!(["*"]));

        sync_file(&path, "server", None).unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"].get("server").is_none());
        assert!(value["mcpServers"].get("other").is_some());
    }

    #[test]
    fn copilot_native_entries_map_to_unified_specs() {
        let spec = unified_spec(&json!({"type":"local","command":"node","args":[],"tools":["*"]}));
        assert_eq!(spec, json!({"type":"stdio","command":"node","args":[]}));
        let spec = unified_spec(&json!({"url":"https://example.com/mcp"}));
        assert_eq!(spec["type"], "http");
    }

    #[test]
    fn copilot_mcp_preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "test", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
