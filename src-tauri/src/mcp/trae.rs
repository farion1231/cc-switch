//! TRAE CLI reads Codex-style `[mcp_servers.<id>]` tables from `traecli.toml`.
use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Map, Value};
use std::{fs, path::Path};

fn read_document(path: &Path) -> Result<toml_edit::DocumentMut, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(AppError::io(path, e)),
    };
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|_| AppError::Config("Invalid TRAE CLI TOML configuration".into()))
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = crate::trae_config::lock()?;
    sync_file(&crate::trae_config::config_path(), id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = crate::trae_config::lock()?;
    let path = crate::trae_config::config_path();
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let mut document = read_document(path)?;
    match spec {
        Some(spec) => {
            super::validation::validate_server_spec(spec)?;
            if super::validation::spec_transport_type(spec) == "sse" {
                return Err(AppError::McpValidation(
                    "TRAE CLI supports stdio and streamable HTTP MCP servers, not SSE".into(),
                ));
            }
            let mut table = super::codex::json_server_to_toml_table(spec)?;
            table.decor_mut().set_prefix("\n");
            if document
                .get("mcp_servers")
                .and_then(toml_edit::Item::as_table_like)
                .is_none()
            {
                if document
                    .get("mcp_servers")
                    .is_some_and(|item| !item.is_none())
                {
                    return Err(AppError::Config(
                        "TRAE CLI mcp_servers must be a table".into(),
                    ));
                }
                let mut servers = toml_edit::Table::new();
                servers.set_implicit(true);
                document["mcp_servers"] = toml_edit::Item::Table(servers);
            }
            document["mcp_servers"]
                .as_table_like_mut()
                .unwrap()
                .insert(id, toml_edit::Item::Table(table));
        }
        None => {
            let Some(servers) = document
                .get_mut("mcp_servers")
                .and_then(toml_edit::Item::as_table_like_mut)
            else {
                return Ok(());
            };
            if servers.remove(id).is_none() {
                return Ok(());
            }
        }
    }
    atomic_write_private(path, document.to_string().as_bytes())
}

/// Convert a native `[mcp_servers.<id>]` entry to the unified JSON spec.
fn unified_spec(native: &Value) -> Option<Value> {
    let entry = native.as_object()?;
    let mut spec = Map::new();
    if entry.contains_key("url") {
        spec.insert("type".into(), json!("http"));
        spec.insert("url".into(), entry["url"].clone());
        if let Some(headers) = entry.get("http_headers").or_else(|| entry.get("headers")) {
            spec.insert("headers".into(), headers.clone());
        }
    } else {
        spec.insert("type".into(), json!("stdio"));
        spec.insert("command".into(), entry.get("command")?.clone());
        for key in ["args", "env", "cwd"] {
            if let Some(value) = entry.get(key) {
                spec.insert(key.into(), value.clone());
            }
        }
    }
    // Keep extension fields (timeouts, enabled_tools, ...) so they round-trip.
    for (key, value) in entry {
        if !matches!(
            key.as_str(),
            "type" | "url" | "http_headers" | "headers" | "command" | "args" | "env" | "cwd"
        ) {
            spec.insert(key.clone(), value.clone());
        }
    }
    Some(Value::Object(spec))
}

fn transport(spec: &Value) -> Value {
    let mut spec = spec.clone();
    if let Some(object) = spec.as_object_mut() {
        object.retain(|key, _| {
            matches!(
                key.as_str(),
                "type" | "command" | "args" | "env" | "cwd" | "url" | "headers"
            )
        });
    }
    spec
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = crate::trae_config::read(&crate::trae_config::config_path())?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document
        .get("mcp_servers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let Some(spec) = unified_spec(native)
            .filter(|spec| super::validation::validate_server_spec(spec).is_ok())
        else {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        };
        let enabled = native.get("enabled").and_then(Value::as_bool) != Some(false);
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport(&server.server) != transport(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.trae = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    trae: enabled,
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
            "Imported {count} TRAE CLI MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trae_mcp_round_trips_stdio_and_http_without_touching_providers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        let initial = "# comment\nmodel_provider = \"custom\"\n[model_providers.custom]\nname = \"x\"\nbase_url = \"https://x.example\"\n";
        fs::write(&path, initial).unwrap();
        sync_file(
            &path,
            "docs",
            Some(&json!({"type":"stdio","command":"node","args":["server.js"],"env":{"A":"1"}})),
        )
        .unwrap();
        sync_file(
            &path,
            "remote",
            Some(&json!({"type":"http","url":"https://mcp.example/mcp","headers":{"Authorization":"Bearer x"}})),
        )
        .unwrap();
        let value = crate::trae_config::read(&path).unwrap();
        assert_eq!(value["mcp_servers"]["docs"]["command"], "node");
        assert!(value["mcp_servers"]["docs"].get("type").is_none());
        assert_eq!(
            value["mcp_servers"]["remote"]["http_headers"]["Authorization"],
            "Bearer x"
        );
        assert_eq!(value["model_provider"], "custom");
        assert_eq!(
            unified_spec(&value["mcp_servers"]["remote"]).unwrap()["headers"]["Authorization"],
            "Bearer x"
        );
        assert_eq!(
            unified_spec(&value["mcp_servers"]["docs"]).unwrap()["type"],
            "stdio"
        );
        assert!(sync_file(&path, "sse", Some(&json!({"type":"sse","url":"https://x"}))).is_err());
        sync_file(&path, "docs", None).unwrap();
        sync_file(&path, "remote", None).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# comment"));
        assert_eq!(
            crate::trae_config::read(&path).unwrap(),
            toml::from_str::<Value>(initial).unwrap()
        );
    }

    #[test]
    fn trae_mcp_preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        fs::write(&path, "[mcp_servers\n").unwrap();
        assert!(sync_file(
            &path,
            "test",
            Some(&json!({"type":"stdio","command":"node"}))
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[mcp_servers\n");
    }
}
