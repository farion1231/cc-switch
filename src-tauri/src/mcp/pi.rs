//! Pi MCP sync (`<agent dir>/mcp.json`)
//!
//! Each write is a read-modify-write of the whole document, so only the target id changes. An entry
//! already in the file is the base: the connection fields are replaced with what CC Switch holds,
//! and a key CC Switch also writes from its own tooling is overridden only when the spec carries a
//! value. Everything else stays as Pi wrote it, which is how a Pi-side `description`, `auth` or
//! `oauth` block survives a rewrite without this module knowing about it. Pi skips an entry it
//! cannot validate, logging a config error, so a bad entry drops out of Pi's server list.
//!
//! Pi keys `mcpServers` by name and accepts only `[A-Za-z0-9_-]+`; an id outside that set is refused
//! here instead of written. `enabled` is a field value Pi writes from its own `/mcp` manager, and
//! `enabled: false` keeps the entry, so a disable made inside Pi is never turned back on.

use crate::app_config::McpApps;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// Serializes writes in this process; concurrent read-modify-write loses entries.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Connection fields CC Switch owns for a stdio entry.
const STDIO_FIELDS: [&str; 4] = ["command", "args", "env", "cwd"];
/// Connection fields CC Switch owns for an HTTP entry. `oauth` is not one of them: `pi mcp add
/// --oauth-client-*` writes it too, so it is only overridden when CC Switch has a value (see
/// `OVERRIDE_FIELDS`).
const HTTP_FIELDS: [&str; 2] = ["url", "headers"];
/// Keys Pi's own tooling writes as well, so CC Switch overrides them only when its spec carries a
/// value: `oauth` from `pi mcp add --oauth-client-*`, and the settings Pi's `/mcp` manager edits.
/// Every other key is left as the file holds it, so `description`, `auth`, and any field a later Pi
/// release adds survive without a change to this list.
const OVERRIDE_FIELDS: [&str; 5] = ["oauth", "exposure", "toolExposure", "timeout", "enabled"];

/// Fields dropped from the file's entry before the spec's values go on: both transports' connection
/// fields, plus `type`. Dropping both halves on every write is what keeps a transport switch from
/// leaving a stale one behind, so an entry turned HTTP does not keep its `command`. `type` is never
/// written at all: Pi infers the transport from `command` vs `url`, and a `type: "sse"` inherited
/// from the file would make Pi skip the entry. Derived from the two lists above, so the field sets
/// cannot drift apart.
fn stripped_fields() -> impl Iterator<Item = &'static str> {
    STDIO_FIELDS
        .iter()
        .chain(HTTP_FIELDS.iter())
        .copied()
        .chain(["type"])
}

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

/// Builds the entry written to Pi from the file's current entry and CC Switch's spec.
///
/// The file's entry is the base, so a key CC Switch does not write (Pi's `description`, an `auth`
/// block, an `oauth` registration from `pi mcp add`, or a field a later Pi release introduces)
/// survives a rewrite untouched. The connection half is dropped, then the spec's own
/// values go on top: the transport fields for the transport the spec selects, plus
/// `OVERRIDE_FIELDS`. An entry carrying both `command` and `url` is written as HTTP.
fn outbound_entry(spec: &Value, previous: Option<&Value>) -> Value {
    let mut entry = previous
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in stripped_fields() {
        entry.remove(key);
    }
    if let Some(object) = spec.as_object() {
        let transport_fields: &[&str] = if object.contains_key("url") {
            &HTTP_FIELDS
        } else {
            &STDIO_FIELDS
        };
        for key in transport_fields.iter().chain(OVERRIDE_FIELDS.iter()) {
            if let Some(value) = object.get(*key) {
                entry.insert((*key).to_string(), value.clone());
            }
        }
    }
    Value::Object(entry)
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
            let entry = outbound_entry(spec, servers.get(id));
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
/// Entries are stored as-is, Pi-only and unknown fields included, except `enabled`: Pi controls that
/// from `/mcp`, so it stays in the file and is inherited on every write. An id already in CC Switch
/// only gets its Pi flag set. Invalid entries are skipped and reported.
pub fn import(state: &AppState) -> Result<usize, AppError> {
    let Some(document) = read(&crate::pi_config::get_pi_mcp_path()?)? else {
        return Ok(0);
    };

    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();

    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let mut spec = unified_spec(native);
        // `enabled` is a field value Pi owns from `/mcp` rather than connection data, so keep it out
        // of the shared spec: clients that pass unknown keys through would copy it. The file's own
        // value is inherited on every write.
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

    /// A fresh entry takes the transport fields and the settings CC Switch holds; keys only the
    /// spec carries (adapter metadata) and `type` are dropped.
    #[test]
    fn outbound_entry_writes_the_connection_half_of_a_new_entry() {
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
            outbound_entry(&stdio, None),
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
            outbound_entry(&http, None),
            json!({
                "url": "https://example.com/mcp",
                "headers": {"Authorization": "Bearer x"},
                "oauth": {"clientId": "id"},
                "toolExposure": {"ping": "direct"}
            })
        );
    }

    /// A rewrite keeps every Pi-side key, including fields a later Pi release adds, and swaps only
    /// the connection half.
    #[test]
    fn outbound_entry_keeps_pi_side_keys_and_replaces_the_connection_half() {
        let previous = json!({
            "command": "node",
            "args": ["old.js"],
            "description": "Search the product documentation",
            "auth": {"provider": "anthropic"},
            "oauth": {"clientId": "id", "clientName": "Claude Code"},
            "toolExposure": {"ping": "direct"},
            "timeout": 30,
            "enabled": false,
            "type": "sse",
            "directTools": ["ping"]
        });

        assert_eq!(
            outbound_entry(
                &json!({
                    "type": "http",
                    "url": "https://example.com/mcp",
                    "headers": {"Authorization": "Bearer x"},
                    "exposure": "direct"
                }),
                Some(&previous)
            ),
            json!({
                "url": "https://example.com/mcp",
                "headers": {"Authorization": "Bearer x"},
                "exposure": "direct",
                "oauth": {"clientId": "id", "clientName": "Claude Code"},
                "description": "Search the product documentation",
                "auth": {"provider": "anthropic"},
                "toolExposure": {"ping": "direct"},
                "timeout": 30,
                "enabled": false,
                "directTools": ["ping"]
            }),
            "the stale stdio half and `type` go, Pi's own keys stay"
        );
    }

    /// The spec's own values win over the file's, and the reverse transport switch drops the other
    /// half the same way.
    #[test]
    fn outbound_entry_obeys_spec_values_and_replaces_the_other_transport() {
        let previous = json!({
            "url": "https://old.example.com/mcp",
            "headers": {"Authorization": "Bearer old"},
            "oauth": {"clientId": "registered-in-pi"},
            "exposure": "deferred"
        });

        assert_eq!(
            outbound_entry(
                &json!({
                    "type": "stdio",
                    "command": "node",
                    "args": ["server.js"],
                    "oauth": {"clientId": "from-cc-switch"},
                    "exposure": "direct"
                }),
                Some(&previous)
            ),
            json!({
                "command": "node",
                "args": ["server.js"],
                "oauth": {"clientId": "from-cc-switch"},
                "exposure": "direct"
            }),
            "the HTTP half goes and the spec's oauth and exposure win"
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

    /// Every key CC Switch does not own inherits from the file: the Pi settings, and any field Pi
    /// itself added (here `description` and `auth` from Pi 0.99.2, plus an unknown `directTools`).
    #[test]
    fn rewriting_keeps_pi_side_keys_unless_cc_switch_provides_them() {
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
                "description": "Search the docs",
                "auth": {"provider": "anthropic"},
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
                "enabled": false,
                "description": "Search the docs",
                "auth": {"provider": "anthropic"},
                "directTools": ["ping"]
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
