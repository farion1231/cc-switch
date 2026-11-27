use super::json_file::JsonMcpFile;
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::Value;
use std::{path::Path, sync::Mutex};

static FILE: JsonMcpFile = JsonMcpFile {
    label: "Kimi Code",
    import_label: "Kimi Code",
    app: AppType::Kimi,
    path: || crate::kimi_config::data_dir().join("mcp.json"),
    transport_key: true,
    lock: Mutex::new(()),
};

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    FILE.sync(id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    FILE.sync_and_commit(id, spec, commit)
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    FILE.import(state)
}

/// Refresh only flags for known, matching native entries; never adopt a
/// conflicting transport or rewrite the user's Kimi configuration.
pub fn reconcile_enabled(state: &AppState, path: &Path) -> Result<(), AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&FILE.lock);
    let document = FILE.read(path)?;
    let servers = state.db.get_all_mcp_servers()?;
    for (id, server) in servers {
        let Some(native) = document["mcpServers"].get(&id) else {
            continue;
        };
        let spec = FILE.unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err()
            || FILE.transport_spec(&server.server) != FILE.transport_spec(&spec)
        {
            continue;
        }
        let enabled = match native.get("enabled") {
            None => true,
            Some(Value::Bool(enabled)) => *enabled,
            Some(_) => continue,
        };
        if server.apps.kimi != enabled {
            state.db.update_mcp_server_app_enabled(
                &id,
                &crate::app_config::AppType::Kimi,
                enabled,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::{McpApps, McpServer};
    use serde_json::json;
    use std::fs;

    #[test]
    fn kimi_mcp_converts_transports_and_preserves_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, json!({"custom":"keep","mcpServers":{"server":{"url":"https://old.example.com","enabled":false,"deferred":true,"toolTimeoutMs":1234},"other":{"command":"node","args":["server.js"]}}}).to_string()).unwrap();
        FILE.sync_file(
            &path,
            "server",
            Some(
                &json!({"type":"sse","url":"https://example.com/sse","headers":{"X-Test":"keep"}}),
            ),
        )
        .unwrap();
        let value = FILE.read(&path).unwrap();
        assert_eq!(value["mcpServers"]["server"]["transport"], "sse");
        assert_eq!(value["mcpServers"]["server"]["enabled"], true);
        assert_eq!(value["mcpServers"]["server"]["deferred"], true);
        assert_eq!(value["mcpServers"]["server"]["toolTimeoutMs"], 1234);
        assert_eq!(
            FILE.unified_spec(&value["mcpServers"]["server"])["type"],
            "sse"
        );
        FILE.sync_file(
            &path,
            "server",
            Some(&json!({"type":"stdio","command":"node","args":["new.js"]})),
        )
        .unwrap();
        let value = FILE.read(&path).unwrap();
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
        assert!(FILE
            .sync_file(&path, "test", Some(&json!({"command":"node"})))
            .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
    #[test]
    fn reconcile_updates_only_matching_servers_flags() {
        let cases = [
            ("default enabled", json!({"command":"node"}), false, true),
            (
                "explicit disabled",
                json!({"command":"node","enabled":false}),
                true,
                false,
            ),
            (
                "conflicting command",
                json!({"command":"other"}),
                false,
                false,
            ),
            (
                "disabled conflict",
                json!({"command":"other","enabled":false}),
                true,
                true,
            ),
            (
                "conflicting cwd",
                json!({"command":"node","cwd":"/other"}),
                false,
                false,
            ),
            ("invalid entry", json!({"type":"sse"}), false, false),
            (
                "invalid enabled value",
                json!({"command":"node","enabled":"false"}),
                false,
                false,
            ),
            ("null entry", Value::Null, false, false),
        ];
        for (name, native, initial_enabled, expected_enabled) in cases {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("mcp.json");
            let original =
                json!({"mcpServers":{"shared":native,"unknown":{"command":"node"}}}).to_string();
            fs::write(&path, &original).unwrap();
            let state = AppState::new(std::sync::Arc::new(
                crate::database::Database::memory().unwrap(),
            ));
            let spec = json!({"type":"stdio","command":"node","toolTimeoutMs":1000});
            state
                .db
                .save_mcp_server(&McpServer {
                    id: "shared".into(),
                    name: "Shared".into(),
                    server: spec.clone(),
                    apps: McpApps {
                        kimi: initial_enabled,
                        claude: true,
                        ..Default::default()
                    },
                    description: Some("keep metadata".into()),
                    homepage: None,
                    docs: None,
                    tags: vec!["shared".into()],
                })
                .unwrap();

            reconcile_enabled(&state, &path).unwrap();

            let servers = state.db.get_all_mcp_servers().unwrap();
            let server = &servers["shared"];
            assert_eq!(server.apps.kimi, expected_enabled, "{name}");
            assert!(server.apps.claude, "{name}");
            assert_eq!(server.server, spec, "{name}");
            assert_eq!(
                server.description.as_deref(),
                Some("keep metadata"),
                "{name}"
            );
            assert_eq!(server.tags, vec!["shared"], "{name}");
            assert_eq!(servers.len(), 1, "{name}");
            assert_eq!(fs::read_to_string(&path).unwrap(), original, "{name}");
        }
    }

    #[test]
    fn reconcile_preserves_flags_when_native_file_or_entry_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let state = AppState::new(std::sync::Arc::new(
            crate::database::Database::memory().unwrap(),
        ));
        state
            .db
            .save_mcp_server(&McpServer {
                id: "managed".into(),
                name: "Managed".into(),
                server: json!({"command":"node"}),
                apps: McpApps {
                    kimi: true,
                    ..Default::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            })
            .unwrap();

        reconcile_enabled(&state, &path).unwrap();
        assert!(state.db.get_all_mcp_servers().unwrap()["managed"].apps.kimi);
        assert!(!path.exists());

        fs::write(&path, r#"{"mcpServers":{}}"#).unwrap();
        reconcile_enabled(&state, &path).unwrap();
        assert!(state.db.get_all_mcp_servers().unwrap()["managed"].apps.kimi);

        fs::write(&path, "{invalid").unwrap();
        assert!(reconcile_enabled(&state, &path).is_err());
        assert!(state.db.get_all_mcp_servers().unwrap()["managed"].apps.kimi);
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
