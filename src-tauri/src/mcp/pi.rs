//! Pi MCP sync (`<agent dir>/mcp.json`)
//!
//! Writes are read-modify-write, so only the target id changes. Outbound entries keep only the
//! fields Pi knows and never carry `type`: Pi infers the transport from `command` vs `url` and
//! rejects `type: "sse"`. An entry Pi cannot validate is skipped with a config error while the
//! rest of the file still loads, so a bad entry silently disappears from Pi's server list.
//!
//! Two Pi-side rules shape this module. Pi keys `mcpServers` by name and accepts only
//! `[A-Za-z0-9_-]+`, so an id outside that set is refused instead of written. And `enabled` is a
//! field value Pi writes from its own `/mcp` manager (`enabled: false` keeps the entry), so it is
//! inherited from the file like `exposure` whenever CC Switch has no value for it.

use crate::app_config::McpApps;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// Serializes writes in this process; concurrent read-modify-write loses entries.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Fields Pi accepts for stdio servers.
const STDIO_FIELDS: [&str; 4] = ["command", "args", "env", "cwd"];
/// Fields Pi accepts for HTTP servers (`oauth` holds Pi's credentials).
const HTTP_FIELDS: [&str; 3] = ["url", "headers", "oauth"];
/// Transport-independent Pi fields. Pi can also set every one of them from `/mcp`, which is why
/// `inherit_pi_fields` keeps the value already in the file when CC Switch has none.
const SHARED_FIELDS: [&str; 4] = ["exposure", "toolExposure", "timeout", "enabled"];

/// Checks the id used as Pi's `mcpServers` key.
///
/// Pi accepts only `[A-Za-z0-9_-]+` (`SERVER_NAME` in its `core/mcp-servers.js`); anything else
/// makes Pi skip the entry with a config error, so a name like `my.server` would leave CC Switch
/// showing "enabled for Pi" while Pi never loads it. Refusing up front keeps the two in sync.
pub(crate) fn validate_server_id(id: &str) -> Result<(), AppError> {
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "Pi MCP server id '{id}' must use only letters, digits, '_' and '-'"
        )))
    }
}

/// Validates one entry as Pi would read it: the id (Pi's `mcpServers` key) plus the connection
/// definition, which shares the other clients' validator.
pub(crate) fn validate_server(id: &str, spec: &Value) -> Result<(), AppError> {
    validate_server_id(id)?;
    super::validation::validate_server_spec(&unified_spec(spec))
}

/// False when Pi's agent directory is missing, so nothing is written or created.
fn should_sync() -> Result<bool, AppError> {
    Ok(crate::pi_config::get_pi_agent_dir()?.exists())
}

fn lock_writes() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))
}

/// Reads the whole document, or `None` when the file is absent (so removal never creates it).
fn read(path: &Path) -> Result<Option<Value>, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::Config(format!("Invalid Pi MCP JSON at {}: {e}", path.display())))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config(format!(
            "Invalid Pi mcpServers object at {}",
            path.display()
        )));
    }
    Ok(Some(value))
}

/// Fills in a missing `type` (`url` → http, otherwise stdio) and normalizes `streamable-http`.
fn unified_spec(spec: &Value) -> Value {
    let mut spec = spec.clone();
    if let Some(object) = spec.as_object_mut() {
        if object.get("type").is_none() {
            let inferred = if object.contains_key("url") {
                "http"
            } else {
                "stdio"
            };
            object.insert("type".to_string(), json!(inferred));
        } else if object.get("type") == Some(&json!("streamable-http")) {
            object.insert("type".to_string(), json!("http"));
        }
    }
    spec
}

/// Builds the entry written to Pi: whitelisted fields only, never `type`.
///
/// `type` is dropped because Pi infers the transport itself and rejects `type: "sse"`. An entry
/// with both `command` and `url` is written as HTTP, without disambiguation.
fn outbound_spec(spec: &Value) -> Value {
    let transport_fields: &[&str] = if spec.get("url").is_some() {
        &HTTP_FIELDS
    } else {
        &STDIO_FIELDS
    };
    let mut out = Map::new();
    if let Some(object) = spec.as_object() {
        for key in transport_fields.iter().chain(SHARED_FIELDS.iter()) {
            if let Some(value) = object.get(*key) {
                out.insert((*key).to_string(), value.clone());
            }
        }
    }
    Value::Object(out)
}

/// Carries the field values Pi can also control (`SHARED_FIELDS`, `enabled` included) over from
/// the existing entry when CC Switch has no value for them.
fn inherit_pi_fields(entry: &mut Value, previous: Option<&Value>) {
    let Some(previous) = previous.and_then(Value::as_object) else {
        return;
    };
    let Some(entry) = entry.as_object_mut() else {
        return;
    };
    for key in SHARED_FIELDS {
        if !entry.contains_key(key) {
            if let Some(value) = previous.get(key) {
                entry.insert(key.to_string(), value.clone());
            }
        }
    }
}

/// Adds, replaces or removes one entry; callers hold the lock and gate on Pi's presence.
fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let existing = read(path)?;
    // Nothing to do when the file is absent; do not create it.
    if spec.is_none() && existing.is_none() {
        return Ok(());
    }

    let mut document = existing.unwrap_or_else(|| json!({ "mcpServers": {} }));
    if document.get("mcpServers").is_none() {
        // The document parses but has no `mcpServers` key: add that key and keep the rest.
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"]
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Invalid Pi mcpServers object".into()))?;

    match spec {
        Some(spec) => {
            // Validate before writing; an invalid id or definition never reaches the user's file.
            validate_server(id, spec)?;
            let mut entry = outbound_spec(spec);
            inherit_pi_fields(&mut entry, servers.get(id));
            servers.insert(id.to_string(), entry);
        }
        None => {
            servers.remove(id);
        }
    }

    let text =
        serde_json::to_string_pretty(&document).map_err(|e| AppError::Config(e.to_string()))?;
    crate::config::atomic_write_private(path, text.as_bytes())
}

/// Writes one MCP server into Pi's global config, replacing its own entry.
pub fn sync_single_server_to_pi(id: &str, spec: &Value) -> Result<(), AppError> {
    if !should_sync()? {
        return Ok(());
    }
    let _guard = lock_writes()?;
    sync_file(&crate::pi_config::get_pi_mcp_path()?, id, Some(spec))
}

/// Removes one MCP server from Pi's global config.
pub fn remove_server_from_pi(id: &str) -> Result<(), AppError> {
    if !should_sync()? {
        return Ok(());
    }
    let _guard = lock_writes()?;
    sync_file(&crate::pi_config::get_pi_mcp_path()?, id, None)
}

/// Imports MCP servers from Pi's global config; returns the number of new ones.
///
/// Entries are stored as-is, Pi-only and unknown fields included, except `enabled` — that is a
/// field value Pi controls from `/mcp`, so it stays in the file and is inherited on every write.
/// An id already in CC Switch only gets its Pi flag set. Invalid entries are skipped and reported.
pub fn import(state: &AppState) -> Result<usize, AppError> {
    let Some(document) = read(&crate::pi_config::get_pi_mcp_path()?)? else {
        return Ok(0);
    };

    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();

    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let mut spec = unified_spec(native);
        // `enabled` is a field value Pi owns from `/mcp`, not connection data: keep it out of the
        // shared spec (the clients that pass unknown keys through would otherwise copy it) and let
        // `inherit_pi_fields` carry the file's current value on every write.
        if let Some(object) = spec.as_object_mut() {
            object.remove("enabled");
        }
        if let Err(error) = super::validation::validate_server_spec(&spec) {
            skipped.push(format!("'{id}': {error}"));
            continue;
        }

        // A new entry is counted only after it has been persisted: an entry Pi would skip is
        // validated away below, so counting it here would report it as imported.
        let mut created = false;
        let server = if let Some(mut server) = existing.shift_remove(id) {
            server.apps.pi = true;
            server
        } else {
            created = true;
            crate::app_config::McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    pi: true,
                    ..Default::default()
                },
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            }
        };
        state.db.save_mcp_server(&server)?;
        if created {
            count += 1;
        }
    }

    if skipped.is_empty() {
        Ok(count)
    } else {
        Err(AppError::InvalidInput(format!(
            "Imported {count} Pi MCP servers; skipped {}. Native configurations were preserved.",
            skipped.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Outbound keeps only whitelisted fields; adapter keys and `type` are dropped.
    #[test]
    fn outbound_spec_keeps_pi_fields_and_drops_foreign_metadata() {
        let stdio = json!({
            "type": "stdio",
            "command": "node",
            "args": ["server.js"],
            "env": {"KEY": "value"},
            "cwd": "/tmp",
            "exposure": "direct",
            "timeout": 5000,
            "enabled": true,
            "directTools": ["ping"],
            "lifecycle": {"start": "auto"}
        });
        assert_eq!(
            outbound_spec(&stdio),
            json!({
                "command": "node",
                "args": ["server.js"],
                "env": {"KEY": "value"},
                "cwd": "/tmp",
                "exposure": "direct",
                "timeout": 5000,
                "enabled": true
            })
        );

        let http = json!({
            "type": "sse",
            "url": "https://example.com/mcp",
            "headers": {"Authorization": "Bearer x"},
            "oauth": {"clientId": "id"},
            "toolExposure": {"ping": "direct"},
            "command": "ignored"
        });
        assert_eq!(
            outbound_spec(&http),
            json!({
                "url": "https://example.com/mcp",
                "headers": {"Authorization": "Bearer x"},
                "oauth": {"clientId": "id"},
                "toolExposure": {"ping": "direct"}
            })
        );
    }

    /// Import fills in a missing `type`: `url` means http, otherwise stdio.
    #[test]
    fn unified_spec_infers_missing_type() {
        assert_eq!(
            unified_spec(&json!({"url": "https://example.com/mcp"}))["type"],
            "http"
        );
        assert_eq!(unified_spec(&json!({"command": "node"}))["type"], "stdio");
        assert_eq!(
            unified_spec(&json!({"type": "streamable-http", "url": "https://example.com/mcp"}))
                ["type"],
            "http"
        );
        assert_eq!(
            unified_spec(&json!({"type": "sse", "url": "u"}))["type"],
            "sse"
        );
    }

    /// Read-modify-write: only the target id changes; other entries and top-level keys stay.
    #[test]
    fn sync_file_preserves_unmanaged_entries_and_top_level_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let original = json!({
            "autoEnableCodemode": true,
            "mcpServers": {
                "unmanaged": {"command": "other", "exposure": "codemode"},
                "edit": {"command": "old"}
            }
        });
        fs::write(&path, original.to_string()).unwrap();

        sync_file(
            &path,
            "edit",
            Some(&json!({"type": "http", "url": "https://example.com/mcp", "exposure": "direct"})),
        )
        .unwrap();

        let written = read(&path).unwrap().unwrap();
        assert_eq!(written["autoEnableCodemode"], json!(true));
        assert_eq!(
            written["mcpServers"]["unmanaged"],
            original["mcpServers"]["unmanaged"]
        );
        assert_eq!(
            written["mcpServers"]["edit"],
            json!({"url": "https://example.com/mcp", "exposure": "direct"})
        );

        // Other entries and top-level keys stay after removal.
        sync_file(&path, "edit", None).unwrap();
        let written = read(&path).unwrap().unwrap();
        assert!(written["mcpServers"].get("edit").is_none());
        assert_eq!(
            written["mcpServers"]["unmanaged"],
            original["mcpServers"]["unmanaged"]
        );
        assert_eq!(written["autoEnableCodemode"], json!(true));
    }

    /// Pi-side field values inherit from the file unless CC Switch provides them.
    #[test]
    fn rewriting_inherits_pi_only_fields_unless_cc_switch_provides_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            json!({"mcpServers": {"managed": {
                "command": "node",
                "args": ["old.js"],
                "exposure": "direct",
                "toolExposure": {"ping": "direct"},
                "timeout": 30,
                "enabled": false,
                "directTools": ["ping"]
            }}})
            .to_string(),
        )
        .unwrap();

        // CC Switch has no values for these keys: keep the file's, rewrite the connection params.
        sync_file(
            &path,
            "managed",
            Some(&json!({"command": "node", "args": ["new.js"]})),
        )
        .unwrap();
        let written = read(&path).unwrap().unwrap();
        assert_eq!(
            written["mcpServers"]["managed"],
            json!({
                "command": "node",
                "args": ["new.js"],
                "exposure": "direct",
                "toolExposure": {"ping": "direct"},
                "timeout": 30,
                "enabled": false
            })
        );

        // An exposure value in CC Switch wins over the one Pi wrote.
        sync_file(
            &path,
            "managed",
            Some(&json!({"command": "node", "args": ["new.js"], "exposure": "codemode"})),
        )
        .unwrap();
        let written = read(&path).unwrap().unwrap();
        assert_eq!(
            written["mcpServers"]["managed"]["exposure"],
            json!("codemode")
        );
        assert_eq!(
            written["mcpServers"]["managed"]["toolExposure"],
            json!({"ping": "direct"})
        );

        // A value CC Switch does hold wins, so the Pi checkbox in the panel stays the SSOT's
        // answer once the user has expressed one.
        sync_file(
            &path,
            "managed",
            Some(&json!({"command": "node", "args": ["new.js"], "enabled": true})),
        )
        .unwrap();
        let written = read(&path).unwrap().unwrap();
        assert_eq!(written["mcpServers"]["managed"]["enabled"], json!(true));
    }

    /// A disable made inside Pi survives a rewrite: Pi keeps the entry with `enabled: false` and
    /// only its own `/mcp` manager clears that key, so CC Switch must not turn the server back on.
    #[test]
    fn rewriting_keeps_a_server_disabled_inside_pi() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            json!({"mcpServers": {"off": {"command": "node", "enabled": false}}}).to_string(),
        )
        .unwrap();

        sync_file(
            &path,
            "off",
            Some(&json!({"command": "node", "args": ["new.js"]})),
        )
        .unwrap();

        let written = read(&path).unwrap().unwrap();
        assert_eq!(written["mcpServers"]["off"]["enabled"], json!(false));
        assert_eq!(written["mcpServers"]["off"]["args"], json!(["new.js"]));
    }

    /// Pi skips an entry whose id is not `[A-Za-z0-9_-]+`, so such an id is refused up front
    /// instead of being written and then quietly ignored by Pi.
    #[test]
    fn server_ids_pi_cannot_key_are_refused() {
        assert!(validate_server_id("mcp-fetch_2").is_ok());
        for id in ["", "my.server", "my server", "mcp/fetch", "服务器"] {
            assert!(
                matches!(validate_server_id(id), Err(AppError::InvalidInput(_))),
                "'{id}' must be refused"
            );
        }
        // The connection definition is validated alongside the id.
        assert!(validate_server("ok", &json!({"type": "stdio"})).is_err());
    }

    /// Fail-closed: an unparsable file raises an error and stays byte-identical.
    #[test]
    fn sync_file_fails_closed_on_unparsable_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, "{ \"mcpServers\": {").unwrap();

        assert!(sync_file(&path, "new", Some(&json!({"command": "node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"mcpServers\": {");

        // A `mcpServers` that is not an object is rejected too.
        fs::write(&path, json!({"mcpServers": []}).to_string()).unwrap();
        assert!(sync_file(&path, "new", Some(&json!({"command": "node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"mcpServers\":[]}");
    }

    /// Removing from an absent file does not create it.
    #[test]
    fn removing_from_absent_file_does_not_create_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        sync_file(&path, "gone", None).unwrap();
        assert!(!path.exists());
    }

    /// Invalid transport definitions are never written.
    #[test]
    fn sync_file_rejects_invalid_transport_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        let original = json!({"mcpServers": {"keep": {"command": "node"}}});
        fs::write(&path, original.to_string()).unwrap();

        assert!(sync_file(&path, "bad", Some(&json!({"type": "stdio"}))).is_err());
        let written = read(&path).unwrap().unwrap();
        assert!(written["mcpServers"].get("bad").is_none());
        assert_eq!(written["mcpServers"]["keep"], json!({"command": "node"}));
    }

    /// Writing to an absent file starts from an empty document.
    #[test]
    fn writing_to_absent_file_starts_from_empty_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        sync_file(&path, "fresh", Some(&json!({"command": "node"}))).unwrap();
        let written = read(&path).unwrap().unwrap();
        assert_eq!(written["mcpServers"]["fresh"], json!({"command": "node"}));
    }
}
