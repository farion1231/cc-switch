//! Qwen Code reads MCP servers from `mcpServers` in its JSONC `settings.json`.
//! The transport is implied by the field: `httpUrl` is Streamable HTTP, `url` is
//! SSE and `command` is stdio. Native-only options (`trust`, `timeout`,
//! `includeTools`, ...) are kept when CC Switch rewrites a server.
use crate::app_config::{McpApps, McpServer};
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::path::Path;

const TRANSPORT_FIELDS: [&str; 9] = [
    "command", "args", "env", "cwd", "url", "httpUrl", "headers", "type", "tcp",
];

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    sync_and_commit(id, spec, || Ok(()))
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = crate::qwen_config::lock()?;
    sync_file(&crate::qwen_config::config_path(), id, spec, commit)
}

fn sync_file<T>(
    path: &Path,
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    if let Some(spec) = spec {
        super::validation::validate_server_spec(&unified_spec(spec))?;
    }
    crate::qwen_config::update_and_commit(
        path,
        |before| {
            let mut document = before.clone();
            let present = document
                .get("mcpServers")
                .and_then(|servers| servers.get(id))
                .is_some();
            if spec.is_none() && !present {
                return Ok(document);
            }
            if !document.get("mcpServers").is_some_and(Value::is_object) {
                document["mcpServers"] = json!({});
            }
            let servers = document["mcpServers"].as_object_mut().unwrap();
            match spec {
                Some(spec) => {
                    let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
                    let object = merged
                        .as_object_mut()
                        .ok_or_else(|| AppError::Config("Invalid Qwen Code MCP entry".into()))?;
                    for field in TRANSPORT_FIELDS {
                        object.remove(field);
                    }
                    object.extend(native_spec(spec).as_object().cloned().unwrap_or_default());
                    servers.insert(id.into(), merged);
                }
                None => {
                    servers.remove(id);
                }
            }
            Ok(document)
        },
        |_| commit(),
    )
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = crate::qwen_config::read_settings()?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document
        .get("mcpServers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.qwen = true;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    qwen: true,
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
            "Imported {count} Qwen Code MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

fn transport_spec(spec: &Value) -> Value {
    let mut spec = unified_spec(spec);
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

/// Qwen native entry -> CC Switch unified spec (`type` + `url`).
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    let Some(object) = spec.as_object_mut() else {
        return spec;
    };
    if let Some(http_url) = object.remove("httpUrl") {
        object.insert("url".into(), http_url);
        object.insert("type".into(), json!("http"));
    } else if matches!(
        object.get("type").and_then(Value::as_str),
        Some("http" | "sse" | "stdio")
    ) {
        // Already unified.
    } else if object.contains_key("url") {
        object.insert("type".into(), json!("sse"));
    } else if object.contains_key("command") {
        object.insert("type".into(), json!("stdio"));
    }
    spec
}

/// CC Switch unified spec -> Qwen native fields.
fn native_spec(spec: &Value) -> Value {
    let mut native = unified_spec(spec);
    let Some(object) = native.as_object_mut() else {
        return native;
    };
    let kind = object
        .remove("type")
        .and_then(|kind| kind.as_str().map(str::to_string));
    if kind.as_deref() == Some("http") {
        if let Some(url) = object.remove("url") {
            object.insert("httpUrl".into(), url);
        }
    }
    native
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn qwen_mcp_maps_transports_and_preserves_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            "{\n  // keep me\n  \"mcpServers\": {\n    \"server\": { \"url\": \"https://old/sse\", \"trust\": true, \"timeout\": 1234 },\n    \"other\": { \"command\": \"node\", \"args\": [\"server.js\"] }\n  }\n}\n",
        )
        .unwrap();
        sync_file(
            &path,
            "server",
            Some(&json!({"type":"http","url":"https://example.com/mcp","headers":{"X":"1"}})),
            || Ok(()),
        )
        .unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("// keep me"));
        let value = crate::qwen_config::read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["httpUrl"], "https://example.com/mcp");
        assert!(server.get("url").is_none());
        assert!(server.get("type").is_none());
        assert_eq!(server["trust"], true);
        assert_eq!(server["timeout"], 1234);
        assert_eq!(unified_spec(server)["type"], "http");

        sync_file(
            &path,
            "server",
            Some(&json!({"type":"sse","url":"https://example.com/sse"})),
            || Ok(()),
        )
        .unwrap();
        let value = crate::qwen_config::read(&path).unwrap();
        assert_eq!(
            value["mcpServers"]["server"]["url"],
            "https://example.com/sse"
        );
        assert!(value["mcpServers"]["server"].get("httpUrl").is_none());
        assert_eq!(unified_spec(&value["mcpServers"]["other"])["type"], "stdio");

        sync_file::<()>(&path, "server", None, || Ok(())).unwrap();
        let value = crate::qwen_config::read(&path).unwrap();
        assert!(value["mcpServers"].get("server").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "node");
    }

    #[test]
    fn qwen_mcp_rolls_back_and_keeps_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        let result: Result<(), AppError> =
            sync_file(&path, "s", Some(&json!({"command":"node"})), || {
                Err(AppError::Message("db".into()))
            });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "s", Some(&json!({"command":"node"})), || Ok(())).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
