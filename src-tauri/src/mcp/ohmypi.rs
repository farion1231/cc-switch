//! OMP-native MCP state: preserve per-server options and use the user denylist
//! when disabling so project and foreign discovery cannot reactivate a server.
use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::sync::Mutex;

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 7] = ["command", "args", "env", "cwd", "url", "headers", "type"];

#[derive(Clone, Copy)]
pub(crate) enum OhMyPiChange<'a> {
    Enable(&'a Value, &'a Value),
    Disable(&'a Value, &'a Value),
    Remove(Option<&'a Value>),
}

impl<'a> OhMyPiChange<'a> {
    pub(crate) fn from_enabled(spec: &'a Value, enabled: bool) -> Self {
        Self::from_previous(spec, spec, enabled)
    }
    pub(crate) fn from_previous(spec: &'a Value, previous: &'a Value, enabled: bool) -> Self {
        if enabled {
            Self::Enable(spec, previous)
        } else {
            Self::Disable(spec, previous)
        }
    }
}

pub fn sync_single_server_to_ohmypi(
    _config: &MultiAppConfig,
    id: &str,
    spec: &Value,
) -> Result<(), AppError> {
    sync(id, OhMyPiChange::Enable(spec, spec))
}

pub fn remove_server_from_ohmypi(id: &str) -> Result<(), AppError> {
    sync(id, OhMyPiChange::Remove(None))
}

fn sync(id: &str, change: OhMyPiChange<'_>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_locked(id, change)
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    change: OhMyPiChange<'_>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    crate::mcode_config::write_and_commit(
        &crate::ohmypi_config::get_ohmypi_mcp_path()?,
        || sync_locked(id, change),
        commit,
    )
}

fn update_list(document: &mut Value, field: &str, id: &str, include: bool) -> Result<(), AppError> {
    if document.get(field).is_none() && !include {
        return Ok(());
    }
    let list = document
        .as_object_mut()
        .unwrap()
        .entry(field)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| AppError::Config(format!("Oh My Pi MCP '{field}' must be an array")))?;
    list.retain(|value| value.as_str() != Some(id));
    if include {
        list.push(json!(id));
    }
    Ok(())
}

fn sync_locked(id: &str, change: OhMyPiChange<'_>) -> Result<(), AppError> {
    crate::ohmypi_config::update_ohmypi_mcp_document(|document| {
        let before = document.clone();
        let servers = document
            .as_object_mut()
            .unwrap()
            .entry("mcpServers")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                AppError::Config("Oh My Pi MCP 'mcpServers' must be an object".into())
            })?;
        match change {
            OhMyPiChange::Enable(spec, previous) | OhMyPiChange::Disable(spec, previous) => {
                let unified = unified_spec(spec);
                super::validation::validate_server_spec(&unified)?;
                if servers
                    .get(id)
                    .is_some_and(|native| transport_spec(native) != transport_spec(previous))
                {
                    return Err(AppError::InvalidInput(format!("Oh My Pi MCP '{id}' conflicts with an independently configured server; native configuration preserved")));
                }
                // Native options edited by OMP win; stored options restore a removed entry.
                let mut merged = servers.get(id).cloned().unwrap_or_else(|| unified.clone());
                let entry = merged
                    .as_object_mut()
                    .ok_or_else(|| AppError::Config("Invalid Oh My Pi MCP entry".into()))?;
                for field in TRANSPORT_FIELDS {
                    entry.remove(field);
                }
                entry.extend(
                    unified
                        .as_object()
                        .unwrap()
                        .iter()
                        .filter(|(key, _)| TRANSPORT_FIELDS.contains(&key.as_str()))
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
                entry.insert(
                    "enabled".into(),
                    json!(matches!(change, OhMyPiChange::Enable(_, _))),
                );
                servers.insert(id.into(), merged);
                update_list(
                    document,
                    "disabledServers",
                    id,
                    matches!(change, OhMyPiChange::Disable(_, _)),
                )?;
                update_list(document, "enabledServers", id, false)?;
            }
            OhMyPiChange::Remove(expected) => {
                if expected.is_some_and(|spec| {
                    servers
                        .get(id)
                        .is_some_and(|native| transport_spec(native) != transport_spec(spec))
                }) {
                    return Err(AppError::InvalidInput(format!("Oh My Pi MCP '{id}' changed outside CC Switch; native configuration preserved")));
                }
                servers.remove(id);
                update_list(document, "disabledServers", id, false)?;
                update_list(document, "enabledServers", id, false)?;
            }
        }
        Ok(*document != before)
    })
}

fn read_document() -> Result<Value, AppError> {
    let mut result = json!({});
    crate::ohmypi_config::update_ohmypi_mcp_document(|document| {
        result = document.clone();
        Ok(false)
    })?;
    if result
        .get("mcpServers")
        .is_some_and(|servers| !servers.is_object())
    {
        return Err(AppError::Config(
            "Oh My Pi MCP 'mcpServers' must be an object".into(),
        ));
    }
    Ok(result)
}

pub(crate) fn is_disabled_managed(id: &str, spec: &Value) -> Result<bool, AppError> {
    let document = read_document()?;
    Ok(document["mcpServers"].get(id).is_some_and(|entry| {
        entry.get("enabled") == Some(&json!(false)) && transport_spec(entry) == transport_spec(spec)
    }) && document["disabledServers"]
        .as_array()
        .is_some_and(|items| items.contains(&json!(id))))
}

pub(crate) fn remove_disabled_if_managed(id: &str, spec: &Value) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let document = read_document()?;
    if document["mcpServers"].get(id).is_some_and(|entry| {
        entry.get("enabled") == Some(&json!(false)) && transport_spec(entry) == transport_spec(spec)
    }) {
        sync_locked(id, OhMyPiChange::Remove(Some(spec)))?;
    }
    Ok(())
}

fn native_enabled(document: &Value, id: &str, spec: &Value) -> bool {
    let listed = |key: &str| {
        document[key]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(id)))
    };
    if listed("disabledServers") {
        return false;
    }
    let disabled = spec.get("enabled").is_some_and(|value| {
        value == &json!(false)
            || value
                .as_str()
                .is_some_and(|text| matches!(text.to_lowercase().as_str(), "false" | "0"))
    });
    !disabled || listed("enabledServers")
}

pub(crate) fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read_document()?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    let servers = document.get("mcpServers").and_then(Value::as_object);
    for (id, native) in servers.into_iter().flatten() {
        let mut spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = native_enabled(&document, id, native);
        spec.as_object_mut().unwrap().remove("enabled");
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.ohmypi = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    ohmypi: enabled,
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
        Err(AppError::InvalidInput(format!("Imported {count} Oh My Pi MCP servers; skipped {}. Native configurations were preserved.", skipped.join("; "))))
    }
}

fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    if spec.is_object() && spec.get("type").is_none() {
        if spec.get("command").is_some() {
            spec["type"] = json!("stdio");
        } else if spec.get("url").is_some() {
            spec["type"] = json!("http");
        }
    }
    spec
}

fn transport_spec(spec: &Value) -> Value {
    let mut spec = unified_spec(spec);
    if let Some(object) = spec.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
    }
    spec
}
