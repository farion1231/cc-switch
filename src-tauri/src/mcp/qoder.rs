use crate::app_config::{AppType, McpApps, McpServer};
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use crate::qoder_config::settings_write_lock as write_lock;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path};

const TRANSPORT_FIELDS: [&str; 8] = [
    "command",
    "args",
    "env",
    "url",
    "headers",
    "type",
    "transport",
    "cwd",
];

fn read(path: &Path) -> Result<JsoncDocument, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".into(),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let document = JsoncDocument::parse(text.trim_start_matches('\u{feff}'))?;
    if document.root_key_count("mcpServers")? > 1
        || document
            .value()
            .get("mcpServers")
            .is_some_and(|v| !v.is_object())
    {
        return Err(AppError::Config(
            "Invalid Qoder CLI mcpServers object".into(),
        ));
    }
    Ok(document)
}

pub fn sync(app: &AppType, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = write_lock(app)
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(
        &crate::qoder_config::data_dir(app).join("settings.json"),
        id,
        spec,
    )
}

pub(crate) fn sync_and_commit<T>(
    app: &AppType,
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = write_lock(app)
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = crate::qoder_config::data_dir(app).join("settings.json");
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

/// Commit changes to both editions in a fixed order, rolling back both if either
/// native write or the database commit fails. `None` leaves an edition untouched;
/// `Some(None)` explicitly removes the managed server from that edition.
pub(crate) fn sync_editions_and_commit<T>(
    id: &str,
    global: Option<Option<&Value>>,
    china: Option<Option<&Value>>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let with_china = || match china {
        Some(spec) => sync_and_commit(&AppType::QoderCn, id, spec, commit),
        None => commit(),
    };
    match global {
        Some(spec) => sync_and_commit(&AppType::Qoder, id, spec, with_china),
        None => with_china(),
    }
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let mut source = read(path)?;
    let mut document = source.value().clone();
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"].as_object_mut().unwrap();
    if let Some(spec) = spec {
        super::validation::validate_server_spec(&unified_spec(spec))?;
        let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
        let object = merged
            .as_object_mut()
            .ok_or_else(|| AppError::Config("Invalid Qoder CLI MCP entry".into()))?;
        let native = unified_spec(spec);
        for field in TRANSPORT_FIELDS {
            if field == "cwd" && native["type"] == "stdio" && native.get("cwd").is_none() {
                continue;
            }
            object.remove(field);
        }
        object.extend(native.as_object().unwrap().clone());
        object.insert("disabled".into(), json!(false));
        servers.insert(id.into(), merged);
    } else {
        servers.remove(id);
    }
    crate::qoder_config::write_settings(path, &mut source, &mut document)
}

pub fn import(state: &AppState, app: &AppType) -> Result<usize, AppError> {
    let source = read(&crate::qoder_config::data_dir(app).join("settings.json"))?;
    let document = source.value();
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
            .get("disabled")
            .and_then(Value::as_bool)
            .map(|disabled| !disabled)
            .unwrap_or(true);
        spec.as_object_mut().unwrap().remove("disabled");
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.set_enabled_for(app, enabled);
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: {
                    let mut apps = McpApps::default();
                    apps.set_enabled_for(app, enabled);
                    apps
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
            "Imported {count} Qoder CLI MCP servers; skipped {}. Native configurations were preserved.",
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
    fn qoder_mcp_preserves_settings_and_native_options_and_converts_transports() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let original = json!({"model":{"name":"auto"},"security":{"folderTrust":{"enabled":true}},"mcpServers":{"managed":{"command":"old","args":["old"],"disabled":true,"timeout":1234,"trust":true},"native":{"type":"ws","tcp":{"host":"localhost","port":1234}}}});
        fs::write(&path, original.to_string()).unwrap();
        sync_file(&path, "managed", Some(&json!({"type":"sse","url":"https://example.test/sse","headers":{"X-Test":"value"}}))).unwrap();
        let source = read(&path).unwrap();
        let settings = source.value();
        assert_eq!(settings["model"], original["model"]);
        assert_eq!(settings["security"], original["security"]);
        assert_eq!(
            settings["mcpServers"]["native"],
            original["mcpServers"]["native"]
        );
        let server = &settings["mcpServers"]["managed"];
        assert_eq!(server["type"], "sse");
        assert_eq!(server["disabled"], false);
        assert_eq!(server["timeout"], 1234);
        assert_eq!(server["trust"], true);
        assert!(server.get("command").is_none());
        assert_eq!(settings["privacy"]["usageStatisticsEnabled"], false);
        let mut reenabled = settings.clone();
        reenabled["privacy"]["usageStatisticsEnabled"] = json!(true);
        reenabled["privacy"]["otherOption"] = json!("keep");
        fs::write(&path, reenabled.to_string()).unwrap();
        sync_file(&path, "managed", None).unwrap();
        let removed = read(&path).unwrap();
        assert!(removed.value()["mcpServers"].get("managed").is_none());
        assert_eq!(removed.value()["privacy"]["usageStatisticsEnabled"], false);
        assert_eq!(removed.value()["privacy"]["otherOption"], "keep");
    }

    #[test]
    fn qoder_mcp_does_not_overwrite_invalid_settings_and_rolls_back_failed_commits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, "broken json").unwrap();
        assert!(sync_file(&path, "test", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken json");
        fs::write(&path, "{\"model\":{\"name\":\"auto\"}}").unwrap();
        let before = fs::read(&path).unwrap();
        let result: Result<(), AppError> = crate::mcode_config::write_and_commit(
            &path,
            || sync_file(&path, "test", Some(&json!({"command":"node"}))),
            || Err(AppError::Database("fixture failure".into())),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn qoder_mcp_edits_jsonc_and_preserves_native_fields_and_comments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, "{\n// user's settings\n\"providers\": {},\n\"mcpServers\": {\"managed\": {\"command\": \"old\",\"cwd\":\"/old\",\"timeout\":12000,\"includeTools\":[\"find\"],\"oauth\":{\"enabled\":true}}},\n}").unwrap();
        sync_file(
            &path,
            "managed",
            Some(&json!({"command":"node","args":["server.js"]})),
        )
        .unwrap();
        assert_eq!(
            read(&path).unwrap().value()["mcpServers"]["managed"]["cwd"],
            "/old"
        );
        sync_file(&path, "managed", Some(&json!({"type":"streamable-http", "url":"https://example.test/mcp","headers":{"X-Test":"fixture"}}))).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("// user's settings"));
        let source = read(&path).unwrap();
        let server = &source.value()["mcpServers"]["managed"];
        assert_eq!(server["type"], "http");
        assert_eq!(server["includeTools"], json!(["find"]));
        assert_eq!(server["timeout"], 12000);
        assert_eq!(server["oauth"]["enabled"], true);
        assert!(server.get("command").is_none());
        assert!(server.get("cwd").is_none());
        assert_eq!(source.value()["providers"], json!({}));
    }
}
