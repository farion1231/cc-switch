use crate::{
    app_config::{McpApps, McpServer},
    error::AppError,
    liveagent_config,
    store::AppState,
};
use serde_json::{json, Value};

#[derive(Clone, Copy)]
pub(crate) enum Change<'a> {
    Enable(&'a Value),
    Disable,
    Remove,
}
impl<'a> Change<'a> {
    pub(crate) fn from_enabled(spec: Option<&'a Value>) -> Self {
        spec.map(Self::Enable).unwrap_or(Self::Disable)
    }
}

const TRANSPORT: &[&str] = &["command", "args", "env", "cwd", "url", "headers", "type"];

pub(crate) fn unified(native: &Value) -> Result<Value, AppError> {
    let mut spec = native.clone();
    let obj = spec
        .as_object_mut()
        .ok_or_else(|| AppError::McpValidation("Invalid LiveAgent MCP object".into()))?;
    if let Some(command) = obj.remove("command") {
        let argv = command
            .as_array()
            .filter(|a| !a.is_empty() && a.iter().all(Value::is_string))
            .ok_or_else(|| {
                AppError::McpValidation(
                    "LiveAgent MCP command must be a nonempty string array".into(),
                )
            })?;
        obj.insert("command".into(), argv[0].clone());
        obj.insert("args".into(), json!(&argv[1..]));
        obj.insert("type".into(), json!("stdio"));
    } else if obj.contains_key("url") {
        obj.insert("type".into(), json!("http"));
    }
    obj.remove("enabled");
    super::validation::validate_server_spec(&spec)?;
    Ok(spec)
}

fn native_spec(spec: &Value) -> Result<Value, AppError> {
    super::validation::validate_server_spec(spec)?;
    for key in ["env", "headers"] {
        if spec.get(key).is_some_and(|v| {
            v.as_object()
                .is_none_or(|m| m.values().any(|v| !v.is_string()))
        }) {
            return Err(AppError::McpValidation(format!(
                "LiveAgent MCP {key} must contain string values"
            )));
        }
    }
    for key in ["note", "cwd"] {
        if spec.get(key).is_some_and(|v| !v.is_string()) {
            return Err(AppError::McpValidation(format!(
                "LiveAgent MCP {key} must be a string"
            )));
        }
    }
    for key in ["startupTimeout", "toolTimeout"] {
        if spec.get(key).is_some_and(|v| v.as_u64().is_none()) {
            return Err(AppError::McpValidation(format!(
                "LiveAgent MCP {key} must be a non-negative integer"
            )));
        }
    }
    let mut out = serde_json::Map::new();
    match spec.get("type").and_then(Value::as_str).unwrap_or("stdio") {
        "stdio" => {
            let command = spec
                .get("command")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::McpValidation("MCP command is required".into()))?;
            let mut argv = vec![json!(command)];
            if let Some(args) = spec.get("args") {
                let args = args
                    .as_array()
                    .filter(|a| a.iter().all(Value::is_string))
                    .ok_or_else(|| AppError::McpValidation("MCP args must be strings".into()))?;
                argv.extend(args.clone());
            }
            out.insert("command".into(), json!(argv));
            for key in ["env", "cwd"] {
                if let Some(v) = spec.get(key) {
                    out.insert(key.into(), v.clone());
                }
            }
        }
        "http" => {
            for key in ["url", "headers"] {
                if let Some(v) = spec.get(key) {
                    out.insert(key.into(), v.clone());
                }
            }
        }
        _ => {
            return Err(AppError::McpValidation(
                "LiveAgent supports stdio and Streamable HTTP MCP; SSE is not supported".into(),
            ))
        }
    }
    for key in ["note", "startupTimeout", "toolTimeout"] {
        if let Some(v) = spec.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    out.insert("enabled".into(), json!(true));
    Ok(Value::Object(out))
}

fn edit(
    servers: &mut serde_json::Map<String, Value>,
    id: &str,
    change: Change<'_>,
) -> Result<(), AppError> {
    match change {
        Change::Enable(spec) => {
            let next = native_spec(spec)?;
            let mut current = servers.get(id).cloned().unwrap_or_else(|| json!({}));
            let obj = current
                .as_object_mut()
                .ok_or_else(|| AppError::Config("Invalid LiveAgent MCP entry".into()))?;
            for key in TRANSPORT {
                obj.remove(*key);
            }
            obj.extend(next.as_object().unwrap().clone());
            servers.insert(id.into(), current);
        }
        Change::Disable => {
            if let Some(v) = servers.get_mut(id) {
                if !v.is_object() {
                    return Err(AppError::Config("Invalid LiveAgent MCP entry".into()));
                }
                v["enabled"] = json!(false);
            }
        }
        Change::Remove => {
            servers.remove(id);
        }
    }
    Ok(())
}

pub(crate) fn sync(id: &str, change: Change<'_>) -> Result<(), AppError> {
    sync_and_commit(id, change, || Ok(()))
}
pub(crate) fn sync_and_commit<T>(
    id: &str,
    change: Change<'_>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    liveagent_config::update_mcp_and_commit(|servers| edit(servers, id, change), commit)
}

pub(crate) fn remove_if_managed_and_commit<T>(
    id: &str,
    spec: &Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    liveagent_config::update_mcp_and_commit(
        |servers| {
            if let Some(native) = servers.get(id) {
                // A name collision is not ownership. Keep user-maintained entries.
                if unified(native).is_ok_and(|value| transport(&value) == transport(spec)) {
                    edit(servers, id, Change::Remove)?;
                }
            }
            Ok(())
        },
        commit,
    )
}

/// Connection identity used to tell our entries from user-maintained ones. Both
/// sides are normalized the same way: [`unified`] always yields an explicit `type`
/// and an `args` array, while saved CC Switch specs may omit either.
fn transport(spec: &Value) -> Value {
    let mut s = spec.clone();
    if let Some(obj) = s.as_object_mut() {
        obj.retain(|k, _| TRANSPORT.contains(&k.as_str()));
        if !obj.get("type").is_some_and(Value::is_string) {
            let kind = if obj.contains_key("command") {
                "stdio"
            } else if obj.contains_key("url") {
                "http"
            } else {
                "stdio"
            };
            obj.insert("type".into(), json!(kind));
        }
        if obj.get("type") == Some(&json!("stdio")) && obj.get("args").is_none_or(Value::is_null) {
            obj.insert("args".into(), json!([]));
        }
    }
    s
}

pub(crate) fn import(state: &AppState) -> Result<usize, AppError> {
    let servers = liveagent_config::read_mcp()?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = 0;
    for (id, native) in &servers {
        // LiveAgent manages its own computer-use server.
        if id.eq_ignore_ascii_case("cua-driver") {
            continue;
        }
        let Ok(spec) = unified(native) else {
            skipped += 1;
            continue;
        };
        let enabled = native
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let server = if let Some(mut old) = existing.shift_remove(id) {
            if transport(&old.server) != transport(&spec) {
                skipped += 1;
                continue;
            }
            old.apps.liveagent = enabled;
            old
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    liveagent: enabled,
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
    if skipped > 0 {
        return Err(AppError::InvalidInput(format!("Imported {count} LiveAgent MCP servers; skipped {skipped} invalid or conflicting entries; native configuration preserved")));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_arrays_roundtrip_and_transport_changes_preserve_timeouts() {
        let mut doc = json!({"test":{"command":["node","old.js"],"startupTimeout":10000,"note":"keep"},"other":{"url":"https://other.example/mcp"}});
        let doc = doc.as_object_mut().unwrap();
        edit(
            doc,
            "test",
            Change::Enable(
                &json!({"type":"stdio","command":"npx","args":["-y","server"],"env":{"A":"B"}}),
            ),
        )
        .unwrap();
        assert_eq!(doc["test"]["command"], json!(["npx", "-y", "server"]));
        assert_eq!(
            unified(&doc["test"]).unwrap()["args"],
            json!(["-y", "server"])
        );
        edit(
            doc,
            "test",
            Change::Enable(&json!({"type":"http","url":"https://example.com/mcp"})),
        )
        .unwrap();
        assert!(doc["test"].get("command").is_none());
        assert_eq!(doc["test"]["startupTimeout"], 10000);
        edit(doc, "test", Change::Disable).unwrap();
        assert_eq!(doc["test"]["enabled"], false);
        edit(doc, "test", Change::Remove).unwrap();
        assert!(doc.get("test").is_none());
        assert!(doc["other"].is_object());
    }
    #[test]
    fn ownership_ignores_implicit_type_and_empty_args() {
        // Saved CC Switch specs may omit `type` and `args`; the native round trip
        // always produces them, and must still be recognized as ours.
        for saved in [
            json!({"command":"my-mcp"}),
            json!({"type":"stdio","command":"my-mcp"}),
            json!({"type":"stdio","command":"my-mcp","args":[]}),
        ] {
            let native = native_spec(&saved).unwrap();
            assert_eq!(transport(&unified(&native).unwrap()), transport(&saved));
        }
        let http = json!({"url":"https://example.com/mcp"});
        let native = native_spec(&json!({"type":"http","url":"https://example.com/mcp"})).unwrap();
        assert_eq!(transport(&unified(&native).unwrap()), transport(&http));
        assert_ne!(
            transport(&json!({"command":"my-mcp","args":["x"]})),
            transport(&json!({"command":"my-mcp"}))
        );
    }

    #[test]
    #[serial_test::serial]
    fn deleting_disabled_argless_server_removes_our_native_entry() {
        use crate::{app_config::AppType, database::Database, services::mcp::McpService};
        let root = liveagent_config::test_support::TestRoot::new();
        std::fs::write(root.dir.path().join("config.json"), "{}").unwrap();
        let state = AppState::new(std::sync::Arc::new(Database::memory().unwrap()));
        let server = McpServer {
            id: "argless".into(),
            name: "argless".into(),
            server: json!({"command":"my-mcp"}),
            apps: McpApps {
                liveagent: true,
                ..Default::default()
            },
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        };
        McpService::upsert_server(&state, server).unwrap();
        // Re-importing our own entry is not a conflict.
        assert_eq!(import(&state).unwrap(), 0);
        McpService::toggle_app(&state, "argless", AppType::LiveAgent, false).unwrap();
        McpService::delete_server(&state, "argless").unwrap();
        assert!(liveagent_config::read_mcp()
            .unwrap()
            .get("argless")
            .is_none());
    }

    #[test]
    #[serial_test::serial]
    fn liveagent_saved_mcp_settings_file_takes_precedence() {
        use crate::{app_config::AppType, database::Database, services::mcp::McpService};
        let root = liveagent_config::test_support::TestRoot::new();
        let config = root.dir.path().join("config.json");
        let sidecar = root.dir.path().join("config.json.live-mcp.json");
        std::fs::write(&config, r#"{"mcp":{"stale":{"command":["old"]}}}"#).unwrap();
        let config_before = std::fs::read(&config).unwrap();
        let kept = json!({"id":"kept","enabled":true,"transport":"http","url":"https://kept.example/mcp","description":"LiveAgent's","policy":{"t":"ask"}});
        std::fs::write(
            &sidecar,
            json!({"servers":[
                {"id":"argless","enabled":true,"transport":"stdio","command":"my-mcp","args":["kb"],"timeoutMs":5000,"workspaceRoots":["/w"]},
                kept,
                {"id":"cua-driver","enabled":true,"transport":"stdio","command":"/x/cua-driver"},
                {"id":"off","transport":"stdio","command":"off-mcp"}
            ],"selected":["argless","kept"]})
            .to_string(),
        )
        .unwrap();
        let state = AppState::new(std::sync::Arc::new(Database::memory().unwrap()));

        // Import reflects what K-brain uses: the sidecar, not config.json.mcp.
        assert_eq!(import(&state).unwrap(), 3);
        let saved = state.db.get_all_mcp_servers().unwrap();
        assert!(!saved.contains_key("stale") && !saved.contains_key("cua-driver"));
        assert_eq!(saved["argless"].server["args"], json!(["kb"]));
        assert!(!saved["off"].apps.liveagent);

        // Our edits land in the sidecar and keep LiveAgent-only fields.
        let mut argless = saved["argless"].clone();
        argless.server["args"] = json!(["cc"]);
        McpService::upsert_server(&state, argless).unwrap();
        McpService::upsert_server(
            &state,
            McpServer {
                id: "added".into(),
                name: "added".into(),
                server: json!({"command":"new-mcp","startupTimeout":7}),
                apps: McpApps {
                    liveagent: true,
                    ..Default::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            },
        )
        .unwrap();
        let doc: Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
        let by_id = |id: &str| {
            doc["servers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .cloned()
        };
        let argless = by_id("argless").unwrap();
        assert_eq!(argless["command"], "my-mcp");
        assert_eq!(argless["args"], json!(["cc"]));
        assert_eq!(argless["transport"], "stdio");
        assert_eq!(argless["timeoutMs"], 5000);
        assert_eq!(argless["workspaceRoots"], json!(["/w"]));
        assert_eq!(by_id("kept").unwrap(), kept);
        assert!(by_id("cua-driver").is_some());
        let added = by_id("added").unwrap();
        assert_eq!(added["command"], "new-mcp");
        assert_eq!(added["enabled"], true);
        assert_eq!(added["timeoutMs"], 7000);
        assert!(added.get("startupTimeout").is_none());
        assert_eq!(std::fs::read(&config).unwrap(), config_before);

        McpService::delete_server(&state, "argless").unwrap();
        let doc: Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
        assert!(!doc["servers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "argless"));
        assert_eq!(doc["selected"], json!(["kept"]));

        // K-brain caches the sidecar while running, so the guard applies here too.
        let before = std::fs::read(&sidecar).unwrap();
        liveagent_config::running::FORCE_RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);
        let blocked = McpService::toggle_app(&state, "added", AppType::LiveAgent, false);
        liveagent_config::running::FORCE_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(blocked.is_err());
        assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    }

    #[test]
    fn rejects_sse_and_invalid_commands() {
        assert!(native_spec(&json!({"type":"sse","url":"https://example.com"})).is_err());
        assert!(unified(&json!({"command":[]})).is_err());
        assert!(native_spec(&json!({"command":"node","args":[1]})).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn liveagent_mcp_import_toggle_and_delete_roundtrip() {
        use crate::{app_config::AppType, database::Database, services::mcp::McpService};
        let root = liveagent_config::test_support::TestRoot::new();
        std::fs::write(root.dir.path().join("config.json"),json!({"providers":{},"mcp":{"test":{"command":["node","server.js"],"enabled":true,"toolTimeout":1000}}}).to_string()).unwrap();
        let state = AppState::new(std::sync::Arc::new(Database::memory().unwrap()));
        assert_eq!(import(&state).unwrap(), 1);
        assert!(
            state.db.get_all_mcp_servers().unwrap()["test"]
                .apps
                .liveagent
        );
        McpService::toggle_app(&state, "test", AppType::LiveAgent, false).unwrap();
        assert_eq!(
            liveagent_config::read_mcp().unwrap()["test"]["enabled"],
            false
        );
        McpService::toggle_app(&state, "test", AppType::LiveAgent, true).unwrap();
        assert_eq!(
            liveagent_config::read_mcp().unwrap()["test"]["toolTimeout"],
            1000
        );
        McpService::delete_server(&state, "test").unwrap();
        assert!(liveagent_config::read_mcp().unwrap().get("test").is_none());
    }
}
