//! CodeBuddy Code user-scope MCP servers (`{"mcpServers": {...}}`).
//!
//! CodeBuddy reads exactly one user-scope file: the first that exists of
//! `<config>/.mcp.json`, `<config>/mcp.json` (deprecated) and
//! `<base>/.codebuddy.json` (legacy), where `<config>` is `CODEBUDDY_CONFIG_DIR` or
//! `~/.codebuddy` and `<base>` is `CODEBUDDY_CONFIG_DIR` or `~`. Files are not merged,
//! and `codebuddy mcp add -s user` writes to that same first existing file (creating
//! `.mcp.json` only when none exists). CC Switch follows the same rule, so a server is
//! never written to a file CodeBuddy would shadow or ignore.
//!
//! Entries are close to the unified spec (`type` = stdio | http | sse with
//! `command`/`args`/`env` or `url`/`headers`). Native-only fields (`defer_loading`,
//! `tools`, ...), `disabledMcpServers`, the legacy file's `projects` map and every
//! other key are preserved. CodeBuddy accepts JSONC here, but CC Switch only rewrites
//! strict JSON files so a user's comments are never lost.
//!
//! Docs: <https://www.codebuddy.ai/docs/cli/mcp>
use crate::app_config::{McpApps, McpServer};
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

const TRANSPORT_FIELDS: [&str; 7] = ["type", "command", "args", "env", "cwd", "url", "headers"];

/// User-scope candidates in CodeBuddy's lookup order.
fn candidate_paths(config_dir: &Path, base_dir: &Path) -> [PathBuf; 3] {
    [
        config_dir.join(".mcp.json"),
        config_dir.join("mcp.json"),
        base_dir.join(".codebuddy.json"),
    ]
}

fn base_dir() -> PathBuf {
    std::env::var("CODEBUDDY_CONFIG_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(get_home_dir)
}

/// The first existing candidate, or the recommended `.mcp.json` when none exists.
fn resolve(candidates: &[PathBuf; 3]) -> PathBuf {
    candidates
        .iter()
        .find(|path| path.exists())
        .unwrap_or(&candidates[0])
        .clone()
}

pub(crate) fn config_path() -> PathBuf {
    resolve(&candidate_paths(
        &crate::codebuddy_config::data_dir(),
        &base_dir(),
    ))
}

fn read_text(path: &Path) -> Result<String, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.trim_start_matches('\u{feff}').to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn check_servers(value: &Value, path: &Path) -> Result<(), AppError> {
    if !value.is_object()
        || value
            .get("mcpServers")
            .is_some_and(|servers| !servers.is_null() && !servers.is_object())
    {
        return Err(AppError::Config(format!(
            "Invalid CodeBuddy MCP configuration (mcpServers must be an object): {}",
            path.display()
        )));
    }
    Ok(())
}

/// Reads the file as CodeBuddy does (JSONC; an empty file is `{}`).
fn read(path: &Path) -> Result<Value, AppError> {
    let text = read_text(path)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let document = JsoncDocument::parse(&text)
        .map_err(|_| AppError::Config(format!("Invalid CodeBuddy MCP JSON: {}", path.display())))?;
    check_servers(document.value(), path)?;
    Ok(document.value().clone())
}

/// Opens the file for rewriting; files with comments or trailing commas are refused.
fn read_for_write(path: &Path) -> Result<JsoncDocument, AppError> {
    let text = read_text(path)?;
    if text.trim().is_empty() {
        return JsoncDocument::parse("{}");
    }
    serde_json::from_str::<Value>(&text).map_err(|_| {
        AppError::Config(format!(
            "CodeBuddy MCP file {} is not plain JSON (comments or trailing commas); CC Switch will not rewrite it",
            path.display()
        ))
    })?;
    let document = JsoncDocument::parse(&text)
        .map_err(|_| AppError::Config(format!("Invalid CodeBuddy MCP JSON: {}", path.display())))?;
    check_servers(document.value(), path)?;
    Ok(document)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(&config_path(), id, spec)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path();
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    if let Some(spec) = spec {
        super::validation::validate_server_spec(&unified_spec(spec))?;
    }
    let mut document = read_for_write(path)?;
    let mut desired = document.value().clone();
    let present = desired
        .get("mcpServers")
        .and_then(|servers| servers.get(id))
        .is_some();
    if spec.is_none() && !present {
        return Ok(());
    }
    if !desired.get("mcpServers").is_some_and(Value::is_object) {
        desired["mcpServers"] = json!({});
    }
    let servers = desired["mcpServers"].as_object_mut().unwrap();
    match spec {
        Some(spec) => {
            let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
            let object = merged
                .as_object_mut()
                .ok_or_else(|| AppError::Config("Invalid CodeBuddy MCP entry".into()))?;
            for field in TRANSPORT_FIELDS {
                object.remove(field);
            }
            object.extend(unified_spec(spec).as_object().cloned().unwrap_or_default());
            servers.insert(id.into(), merged);
        }
        None => {
            servers.remove(id);
        }
    }
    if !document.apply(&desired)? && path.exists() {
        return Ok(());
    }
    let source = document.validated_source()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| AppError::io(parent, error))?;
    }
    atomic_write_private(path, source.as_bytes())
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&config_path())?;
    let disabled: Vec<&str> = document
        .get("disabledMcpServers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
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
        let enabled = !disabled.contains(&id.as_str());
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.codebuddy = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    codebuddy: enabled,
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
            "Imported {count} CodeBuddy Code MCP servers; skipped {}. Native configurations were preserved.",
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

/// Native entry -> unified spec: normalize the transport aliases CodeBuddy accepts
/// and make the implied type explicit (`command` -> stdio, `url` -> http).
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    let Some(object) = spec.as_object_mut() else {
        return spec;
    };
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    match kind.as_deref() {
        Some("streamable-http" | "streamablehttp" | "streamable_http") => {
            object.insert("type".into(), json!("http"));
        }
        Some(_) => {}
        None if object.contains_key("command") => {
            object.insert("type".into(), json!("stdio"));
        }
        None if object.contains_key("url") => {
            object.insert("type".into(), json!("http"));
        }
        None => {}
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codebuddy_mcp_uses_the_first_existing_user_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".codebuddy");
        let candidates = candidate_paths(&config, dir.path());
        // Nothing exists yet: the recommended file is created.
        assert_eq!(resolve(&candidates), config.join(".mcp.json"));
        // Only the legacy home file exists: keep writing there.
        fs::write(dir.path().join(".codebuddy.json"), "{}").unwrap();
        assert_eq!(resolve(&candidates), dir.path().join(".codebuddy.json"));
        // The deprecated mcp.json shadows the legacy file.
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("mcp.json"), "{}").unwrap();
        assert_eq!(resolve(&candidates), config.join("mcp.json"));
        // .mcp.json shadows both.
        fs::write(config.join(".mcp.json"), "{}").unwrap();
        assert_eq!(resolve(&candidates), config.join(".mcp.json"));
    }

    #[test]
    fn codebuddy_mcp_writes_typed_entries_and_preserves_native_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            json!({
                "projects": {"/work": {"mcpServers": {}}},
                "disabledMcpServers": ["other"],
                "mcpServers": {
                    "server": {"type":"sse","url":"https://old.example.com/sse","defer_loading":true,"tools":["a"]},
                    "other": {"command":"node","args":["server.js"]}
                }
            })
            .to_string(),
        )
        .unwrap();
        sync_file(
            &path,
            "server",
            Some(&json!({"type":"http","url":"https://example.com/mcp","headers":{"X":"1"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "http");
        assert_eq!(server["url"], "https://example.com/mcp");
        assert_eq!(server["headers"]["X"], "1");
        assert_eq!(server["defer_loading"], true);
        assert_eq!(server["tools"], json!(["a"]));
        assert_eq!(value["disabledMcpServers"], json!(["other"]));
        assert!(value["projects"]["/work"].is_object());

        sync_file(
            &path,
            "server",
            Some(&json!({"type":"stdio","command":"npx","args":["-y","pkg"]})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"]["server"].get("url").is_none());
        assert_eq!(value["mcpServers"]["server"]["type"], "stdio");
        assert_eq!(value["mcpServers"]["other"]["command"], "node");

        sync_file(&path, "server", None).unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"].get("server").is_none());
        assert_eq!(unified_spec(&value["mcpServers"]["other"])["type"], "stdio");
    }

    #[test]
    fn codebuddy_mcp_normalizes_native_transports() {
        assert_eq!(unified_spec(&json!({"url":"https://x"}))["type"], "http");
        assert_eq!(
            unified_spec(&json!({"type":"streamable-http","url":"https://x"}))["type"],
            "http"
        );
        assert_eq!(
            unified_spec(&json!({"type":"sse","url":"https://x"}))["type"],
            "sse"
        );
    }

    #[test]
    fn codebuddy_mcp_refuses_commented_or_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mcp.json");
        let commented = "{\n  // mine\n  \"mcpServers\": {}\n}\n";
        fs::write(&path, commented).unwrap();
        // CodeBuddy reads JSONC, so import still works...
        assert!(read(&path).unwrap()["mcpServers"].is_object());
        // ...but CC Switch never rewrites it.
        assert!(sync_file(&path, "s", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), commented);
        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "s", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }

    #[test]
    fn codebuddy_mcp_removing_a_missing_server_does_not_create_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".mcp.json");
        sync_file(&path, "s", None).unwrap();
        assert!(!path.exists());
        sync_file(&path, "s", Some(&json!({"command":"node"}))).unwrap();
        assert_eq!(read(&path).unwrap()["mcpServers"]["s"]["type"], "stdio");
    }
}
