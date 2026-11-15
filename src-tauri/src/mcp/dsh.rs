use crate::app_config::{McpApps, McpServer};
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};

pub(crate) fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    sync_and_commit(id, spec, || Ok(()))
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    crate::dsh_config::sync_mcp_and_commit(id, spec, commit)
}

pub(crate) fn import(state: &AppState) -> Result<usize, AppError> {
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    for entry in crate::dsh_config::mcp_entries()? {
        let Some(id) = entry["config"]["serverName"].as_str() else {
            continue;
        };
        let Ok(mut spec) = serde_json::to_value(&entry["config"]) else {
            continue;
        };
        let Some(object) = spec.as_object_mut() else {
            continue;
        };
        let transport = object.remove("transport").unwrap_or(Value::Null);
        object.remove("serverName");
        object.insert(
            "type".into(),
            match transport.as_str() {
                Some("stdio") => json!("stdio"),
                Some("streamable-http") => json!("http"),
                _ => continue,
            },
        );
        if super::validation::validate_server_spec(&spec).is_err() {
            continue;
        }
        let mut server = if let Some(server) = existing.shift_remove(id) {
            // Import never overwrites a different server's transport or managed state.
            let fields = ["type", "command", "args", "env", "cwd", "url", "headers"];
            if fields
                .iter()
                .any(|field| server.server.get(*field) != spec.get(*field))
            {
                continue;
            }
            server
        } else {
            count += 1;
            McpServer {
                id: id.into(),
                name: id.into(),
                server: spec,
                apps: McpApps::default(),
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            }
        };
        server.apps.dsh = entry["disabled"].as_bool() != Some(true);
        state.db.save_mcp_server(&server)?;
    }
    Ok(count)
}
