use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 7] = [
    "command",
    "args",
    "env",
    "url",
    "headers",
    "type",
    "transport",
];

fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"mcpServers": {}})),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Config("Invalid Kimi Code MCP JSON".into()))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config(
            "Invalid Kimi Code mcpServers object".into(),
        ));
    }
    Ok(value)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(&crate::kimi_config::data_dir().join("mcp.json"), id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = crate::kimi_config::data_dir().join("mcp.json");
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
            .ok_or_else(|| AppError::Config("Invalid Kimi Code MCP entry".into()))?;
        for field in TRANSPORT_FIELDS {
            object.remove(field);
        }
        let mut native = unified_spec(spec);
        let kind = native.as_object_mut().unwrap().remove("type");
        if kind.as_ref().and_then(Value::as_str) == Some("sse") {
            native["transport"] = json!("sse");
        }
        object.extend(native.as_object().unwrap().clone());
        object.insert("enabled".into(), json!(true));
        servers.insert(id.into(), merged);
    } else {
        servers.remove(id);
    }
    atomic_write_private(
        path,
        serde_json::to_string_pretty(&document)
            .map_err(|e| AppError::Config(e.to_string()))?
            .as_bytes(),
    )
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&crate::kimi_config::data_dir().join("mcp.json"))?;
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
            server.apps.kimi = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    kimi: enabled,
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
            "Imported {count} Kimi Code MCP servers; skipped {}. Native configurations were preserved.",
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

fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    if let Some(object) = spec.as_object_mut() {
        if let Some(transport) = object.remove("transport") {
            object.insert("type".into(), transport);
        }
    }
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
    fn kimi_mcp_converts_transports_and_preserves_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, json!({"custom":"keep","mcpServers":{"server":{"url":"https://old.example.com","enabled":false,"deferred":true,"toolTimeoutMs":1234},"other":{"command":"node","args":["server.js"]}}}).to_string()).unwrap();
        sync_file(
            &path,
            "server",
            Some(
                &json!({"type":"sse","url":"https://example.com/sse","headers":{"X-Test":"keep"}}),
            ),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert_eq!(value["mcpServers"]["server"]["transport"], "sse");
        assert_eq!(value["mcpServers"]["server"]["enabled"], true);
        assert_eq!(value["mcpServers"]["server"]["deferred"], true);
        assert_eq!(value["mcpServers"]["server"]["toolTimeoutMs"], 1234);
        assert_eq!(unified_spec(&value["mcpServers"]["server"])["type"], "sse");
        sync_file(
            &path,
            "server",
            Some(&json!({"type":"stdio","command":"node","args":["new.js"]})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"]["server"].get("url").is_none());
        assert!(value["mcpServers"]["server"].get("transport").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "node");
        assert_eq!(value["custom"], "keep");
    }

    #[test]
    fn kimi_mcp_preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "test", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
