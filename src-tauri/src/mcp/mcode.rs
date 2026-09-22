use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 6] = ["command", "args", "env", "url", "headers", "type"];

fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"mcpServers": {}})),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Config("Invalid MCode MCP JSON".into()))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config("Invalid MCode mcpServers object".into()));
    }
    Ok(value)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(&crate::mcode_config::data_dir().join("mcp.json"), id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = crate::mcode_config::data_dir().join("mcp.json");
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
            .ok_or_else(|| AppError::Config("Invalid MCode MCP entry".into()))?;
        for field in TRANSPORT_FIELDS {
            object.remove(field);
        }
        object.extend(spec.as_object().unwrap().clone());
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
    let document = read(&crate::mcode_config::data_dir().join("mcp.json"))?;
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
            server.apps.mcode = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    mcode: enabled,
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
            "Imported {count} MiniMax Code MCP servers; skipped {}. Native configurations were preserved.",
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
    fn database_failures_restore_mcode_mcp_and_allow_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let db = crate::database::Database::memory().unwrap();
        let server: McpServer = serde_json::from_value(json!({
            "id":"managed", "name":"Managed", "server":{"command":"node"},
            "apps":{"mcode":true}
        }))
        .unwrap();
        db.save_mcp_server(&server).unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        let spec = json!({"command":"updated"});
        // A rejected insert must also remove a newly created native file.
        assert!(crate::mcode_config::write_and_commit(
            &path,
            || sync_file(&path, "managed", Some(&spec)),
            || db.save_mcp_server(&server),
        )
        .is_err());
        assert!(!path.exists());
        let original = b"{ \"mcpServers\": {\"managed\": {\"command\": \"node\", \"timeout\": 5000}}, \"version\": 7 }";
        fs::write(&path, original).unwrap();
        assert!(crate::mcode_config::write_and_commit(
            &path,
            || sync_file(&path, "managed", None),
            || db.update_mcp_server_app_enabled(
                "managed",
                &crate::app_config::AppType::Mcode,
                false
            ),
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(db.get_all_mcp_servers().unwrap()["managed"].apps.mcode);
        assert!(crate::mcode_config::write_and_commit(
            &path,
            || sync_file(&path, "managed", None),
            || db.delete_mcp_server("managed"),
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=OFF")
            .unwrap();
        crate::mcode_config::write_and_commit(
            &path,
            || sync_file(&path, "managed", None),
            || db.delete_mcp_server("managed"),
        )
        .unwrap();
        assert!(db.get_all_mcp_servers().unwrap().is_empty());
        assert!(read(&path).unwrap()["mcpServers"].get("managed").is_none());
    }

    #[test]
    fn mcode_mcp_preserves_unmanaged_entries_and_transport_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let original = json!({"version":7,"mcpServers":{"other":{"command":"node"},"edit":{"command":"old","args":["old"],"timeout":5000}}});
        fs::write(&path, original.to_string()).unwrap();
        sync_file(
            &path,
            "edit",
            Some(&json!({"type":"http","url":"https://example.com/mcp"})),
        )
        .unwrap();
        let written = read(&path).unwrap();
        assert_eq!(written["version"], 7);
        assert_eq!(
            written["mcpServers"]["other"],
            original["mcpServers"]["other"]
        );
        assert_eq!(written["mcpServers"]["edit"]["timeout"], 5000);
        assert_eq!(written["mcpServers"]["edit"]["type"], "http");
        assert_eq!(written["mcpServers"]["edit"]["enabled"], true);
        assert!(written["mcpServers"]["edit"].get("command").is_none());
        sync_file(&path, "edit", None).unwrap();
        assert!(read(&path).unwrap()["mcpServers"].get("edit").is_none());
        fs::write(&path, "broken json").unwrap();
        assert!(sync_file(&path, "new", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "broken json");
    }
}
