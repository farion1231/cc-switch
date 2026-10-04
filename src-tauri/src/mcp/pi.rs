//! Pi MCP sync (`<agent dir>/mcp.json`)
//!
//! Each write is a read-modify-write of the whole document, so only the target id changes. An entry
//! already in the file is the base: the connection fields are replaced with what CC Switch holds,
//! and a key Pi's own tooling writes too (`OVERRIDE_FIELDS`) is overridden only when the spec
//! carries a value. Everything else stays as Pi wrote it, which is how a field a later Pi release
//! introduces survives a rewrite without a change to this list. Pi skips an entry it
//! cannot validate, logging a config error, so a bad entry drops out of Pi's server list.
//! Pi loads stdio and streamable HTTP only; the legacy `sse` transport is refused here instead of
//! written, because a `url` entry Pi is handed is always spoken as streamable HTTP (see
//! `validate_server`).
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
/// value: `oauth` and `auth` from `pi mcp add` and Pi's `/mcp` auth flow, `description` from
/// `pi mcp add --description`, and the settings Pi's `/mcp` manager edits. Every other key is left
/// as the file holds it, so a field a later Pi release adds survives without a change to this list.
///
/// `auth` has to be listed rather than inherited from the file alone: import keeps it in the spec,
/// so once an entry has been removed and re-added the file no longer holds the original value and
/// the server would silently lose its authentication. Like `oauth`, the stored value then wins over
/// the file — Pi keeps the OAuth tokens themselves in `mcp-auth.json`, which is neither read nor
/// written. Import never overwrites an existing row's spec, so a later Pi-side rewrite of this block
/// is only picked up by editing the row here; that boundary is `oauth`'s as well.
///
/// Only the spec counts here. The `description` CC Switch keeps on the row for its own server list
/// is separate metadata and is never written to any App's config.
const OVERRIDE_FIELDS: [&str; 7] = [
    "oauth",
    "auth",
    "description",
    "exposure",
    "toolExposure",
    "timeout",
    "enabled",
];

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

/// Transports Pi's `mcp.json` reader accepts. Pi selects one from the entry itself — `command`
/// means stdio, `url` means streamable HTTP — and refuses `sse` outright.
const PI_TRANSPORTS: [&str; 2] = ["stdio", "http"];

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

/// Validates one entry as Pi would read it: the id (Pi's `mcpServers` key), the connection
/// definition, which shares the other clients' validator, and the transport.
///
/// The shared validator accepts `sse` because one spec serves every App and the other Apps do
/// support the legacy transport. Pi does not: it reads a `url` entry as streamable HTTP, so writing
/// an SSE spec would hand Pi an address spoken in another protocol, and the `type` it came from is
/// refused by Pi itself. Rejecting it here, at the App boundary, keeps "enabled for Pi" from ever
/// meaning "Pi skipped it" without narrowing what the other Apps may store.
pub(crate) fn validate_server(id: &str, spec: &Value) -> Result<(), AppError> {
    validate_server_id(id)?;
    let unified = unified_spec(spec);
    super::validation::validate_server_spec(&unified)?;
    reject_transport_pi_cannot_load(id, &unified)
}

/// Rejects the transports Pi's `mcp.json` reader does not accept.
///
/// Expects a spec already normalized by `unified_spec`, so a missing `type` has been filled in and
/// `streamable-http` has become `http` — which is what makes `PI_TRANSPORTS` the whole accepted set.
/// A `type` that is not text is read as the shared validator reads it, as stdio, and is named here
/// rather than rendered as an empty transport.
fn reject_transport_pi_cannot_load(id: &str, spec: &Value) -> Result<(), AppError> {
    let transport = spec
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("non-text");
    if PI_TRANSPORTS.contains(&transport) {
        Ok(())
    } else {
        Err(AppError::McpValidation(format!(
            "Pi MCP server '{id}' uses the '{transport}' transport, which Pi does not support; Pi \
             reaches a remote server over streamable HTTP, so use that endpoint (often '/mcp' \
             instead of '/sse') and set type to 'http'"
        )))
    }
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
/// The file's entry is the base, so a key CC Switch does not write (a field a later Pi release
/// introduces, or metadata Pi's own tooling added) survives a rewrite untouched. The connection
/// half is dropped, then the spec's own values go on top: the transport
/// fields for the transport the spec selects, plus `OVERRIDE_FIELDS`. An entry carrying both
/// `command` and `url` is written as HTTP.
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
/// only gets its Pi flag set. An entry Pi cannot load — a bad key, or the legacy SSE transport — is
/// skipped and reported instead of being adopted as "enabled for Pi", which Pi would ignore.
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
        // Validated as Pi would read it, not merely as the shared store does: both an id Pi cannot
        // key and the legacy SSE transport leave the entry out of Pi's own server list, so adopting
        // one would show "enabled for Pi" with nothing in Pi to load. For an id CC Switch already
        // stores, the row's own spec is what gets projected, so that is what is validated.
        if let Err(error) = validate_server(&server.id, &server.server) {
            skipped.push(format!("'{id}': {error}"));
            continue;
        }
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
