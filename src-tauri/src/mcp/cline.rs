//! Cline CLI MCP servers: `cline_mcp_settings.json` (`$CLINE_MCP_SETTINGS_PATH`).
//!
//! Native entries use `type: "stdio" | "sse" | "streamableHttp"` and
//! `disabled: true` instead of an enabled flag. A `url` entry without `type`
//! is SSE in Cline, so writes always spell the transport out.
use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 9] = [
    "command",
    "args",
    "env",
    "cwd",
    "url",
    "headers",
    "type",
    "transportType",
    "transport",
];

fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"mcpServers": {}})),
        Err(e) => return Err(AppError::io(path, e)),
    };
    if text.trim().is_empty() {
        return Ok(json!({"mcpServers": {}}));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Config("Invalid Cline MCP settings JSON".into()))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config("Invalid Cline mcpServers object".into()));
    }
    Ok(value)
}

fn path() -> std::path::PathBuf {
    crate::cline_config::mcp_path()
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(&path(), id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = path();
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let mut document = read(path)?;
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"].as_object_mut().unwrap();
    if let Some(spec) = spec {
        super::validation::validate_server_spec(&unified_spec(spec))?;
        let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
        let object = merged
            .as_object_mut()
            .ok_or_else(|| AppError::Config("Invalid Cline MCP entry".into()))?;
        for field in TRANSPORT_FIELDS {
            object.remove(field);
        }
        object.extend(native_spec(spec).as_object().unwrap().clone());
        object.remove("disabled");
        servers.insert(id.into(), merged);
    } else {
        servers.remove(id);
    }
    let mut text =
        serde_json::to_string_pretty(&document).map_err(|e| AppError::Config(e.to_string()))?;
    text.push('\n');
    atomic_write_private(path, text.as_bytes())
}

/// CC Switch spec → Cline entry.
fn native_spec(spec: &Value) -> Value {
    let mut native = unified_spec(spec);
    let object = native.as_object_mut().unwrap();
    object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    let kind = match object.get("type").and_then(Value::as_str) {
        Some("http") => "streamableHttp",
        Some("sse") => "sse",
        _ => "stdio",
    };
    object.insert("type".into(), json!(kind));
    native
}

/// Cline entry → CC Switch spec (`type` is `stdio`, `http` or `sse`).
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    let Some(object) = spec.as_object_mut() else {
        return spec;
    };
    let kind = object
        .remove("type")
        .or_else(|| object.remove("transportType"))
        .or_else(|| object.remove("transport"))
        .and_then(|kind| kind.as_str().map(str::to_string));
    object.remove("transportType");
    object.remove("transport");
    let kind = match kind.as_deref() {
        Some("streamableHttp" | "streamable-http" | "http") => "http",
        Some("sse") => "sse",
        Some("stdio") => "stdio",
        // Cline treats a bare `url` as SSE.
        _ if object.contains_key("url") => "sse",
        _ => "stdio",
    };
    object.insert("type".into(), json!(kind));
    spec
}

fn transport_spec(spec: &Value) -> Value {
    let mut spec = unified_spec(spec);
    if let Some(object) = spec.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    }
    spec
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&path())?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let spec = transport_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = !native
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != spec {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.cline = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    cline: enabled,
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
            "Imported {count} Cline MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_explicit_cline_transports_and_preserves_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cline_mcp_settings.json");
        fs::write(
            &path,
            json!({"custom": "keep", "mcpServers": {
                "server": {"url": "https://old.example.com", "disabled": true, "timeout": 30, "autoApprove": ["a"]},
                "other": {"command": "node", "args": ["server.js"]}
            }})
            .to_string(),
        )
        .unwrap();
        sync_file(
            &path,
            "server",
            Some(&json!({"type": "http", "url": "https://example.com/mcp", "headers": {"X": "1"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "streamableHttp");
        assert_eq!(server["url"], "https://example.com/mcp");
        assert!(server.get("disabled").is_none(), "enabling clears disabled");
        assert_eq!(server["timeout"], 30);
        assert_eq!(server["autoApprove"][0], "a");
        assert_eq!(unified_spec(server)["type"], "http");

        sync_file(
            &path,
            "server",
            Some(&json!({"command": "node", "args": ["new.js"]})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "stdio");
        assert!(server.get("url").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "node");
        assert_eq!(value["custom"], "keep");

        sync_file(&path, "server", None).unwrap();
        assert!(read(&path).unwrap()["mcpServers"].get("server").is_none());
    }

    #[test]
    fn bare_url_is_sse_like_cline() {
        assert_eq!(unified_spec(&json!({"url": "https://x"}))["type"], "sse");
        assert_eq!(
            unified_spec(&json!({"url": "https://x", "transportType": "http"}))["type"],
            "http"
        );
        assert_eq!(unified_spec(&json!({"command": "node"}))["type"], "stdio");
        assert_eq!(
            native_spec(&json!({"type": "sse", "url": "https://x"}))["type"],
            "sse"
        );
    }

    #[test]
    fn preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cline_mcp_settings.json");
        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "test", Some(&json!({"command": "node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
