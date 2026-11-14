//! GitHub Copilot CLI 的 MCP：`~/.copilot/mcp-config.json` 的 `mcpServers`。
//!
//! 原生格式：stdio 写作 `"type": "local"`（`command` / `args` / `env`），远程是 `http` 或
//! `sse`（`url` / `headers`）；每项都带 `tools` 过滤，默认 `["*"]`。禁用在 Copilot 里是
//! `settings.json` 的 `disabledMcpServers`，CC Switch 停用时直接移除条目（同 Kimi），
//! 导入时把禁用名单里的条目记为未启用。
//!
//! `tools`、`timeout` 只有 Copilot 认，不进统一格式（Claude 等应用原样写出统一格式）。停用会删掉
//! 整个条目，所以删之前把这两项记到本机的 `copilot-mcp-options.json`，再启用时还回去，免得工具
//! 过滤被放宽成 `["*"]`。
use crate::app_config::{McpApps, McpServer};
use crate::config::{atomic_write, atomic_write_private};
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const TRANSPORT_FIELDS: [&str; 7] = ["command", "args", "env", "url", "headers", "type", "cwd"];
/// 条目里只有 Copilot 认、停用时要记下来的字段。
const NATIVE_OPTION_FIELDS: [&str; 2] = ["tools", "timeout"];

/// 对 `mcp-config.json` 里一个条目的改动。
#[derive(Clone, Copy)]
enum Change<'a> {
    /// 启用或编辑：写入传输字段，其余字段（含停用前记下的 `tools`、`timeout`）保留。
    Write(&'a Value),
    /// 停用：删条目，记下 `tools`、`timeout`。
    Disable,
    /// 从 CC Switch 删除：删条目，也忘掉记下的选项。
    Delete,
}

impl<'a> Change<'a> {
    fn from_enabled(spec: Option<&'a Value>) -> Self {
        spec.map_or(Change::Disable, Change::Write)
    }
}

fn options_path() -> PathBuf {
    crate::config::get_app_config_dir().join("copilot-mcp-options.json")
}

/// 读记下的选项。停用后它是 `tools`、`timeout` 唯一的副本，所以只有文件不存在才当作没有；
/// 读不了或坏了就报错，不在它上面继续改。
fn read_options(path: &Path) -> Result<Map<String, Value>, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(AppError::io(path, error)),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(AppError::Config(format!(
            "Invalid GitHub Copilot CLI MCP options file {}",
            path.display()
        ))),
    }
}

fn write_options(path: &Path, options: Map<String, Value>) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(
        path,
        serde_json::to_string_pretty(&Value::Object(options))
            .map_err(|e| AppError::Config(e.to_string()))?
            .as_bytes(),
    )
}

/// `run` 失败时把 `path` 恢复成调用前的样子（`mcp-config.json` 由 `write_and_commit` 回滚，
/// 记下的选项文件在这里一起回滚）。
fn with_file_rollback<T>(
    path: &Path,
    run: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let previous = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::io(path, error)),
    };
    let rollback = || match previous.as_deref() {
        Some(bytes) => atomic_write(path, bytes),
        None => match fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(AppError::io(path, error))
            }
            _ => Ok(()),
        },
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
    let result = crate::sync_utils::resume_after_rollback(outcome, rollback);
    if result.is_err() {
        if let Err(error) = rollback() {
            log::error!("恢复 {} 失败: {error}", path.display());
        }
    }
    result
}

fn read(path: &Path) -> Result<Value, AppError> {
    let value = crate::copilot_config::read_json_document(path)?
        .unwrap_or_else(|| json!({ "mcpServers": {} }));
    if value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config(
            "Invalid GitHub Copilot CLI mcpServers object".into(),
        ));
    }
    Ok(value)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&WRITE_LOCK);
    let options = options_path();
    with_file_rollback(&options, || {
        sync_file(
            &crate::copilot_config::mcp_config_path(),
            &options,
            id,
            Change::from_enabled(spec),
        )
    })
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    commit_change(id, Change::from_enabled(spec), commit)
}

/// 从 CC Switch 删除这个 MCP：删条目，不记停用选项。
pub(crate) fn delete_and_commit<T>(
    id: &str,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    commit_change(id, Change::Delete, commit)
}

/// 删除一个在 Copilot 里没启用的 MCP：只忘掉停用时记下的选项，不碰 `mcp-config.json`
/// （同名条目可能是用户自己写的）。`commit` 失败时选项原样恢复。
pub(crate) fn forget_options_and_commit<T>(
    id: &str,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&WRITE_LOCK);
    let path = options_path();
    with_file_rollback(&path, || {
        let mut options = read_options(&path)?;
        if options.remove(id).is_some() {
            write_options(&path, options)?;
        }
        commit()
    })
}

fn commit_change<T>(
    id: &str,
    change: Change<'_>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = crate::sync_utils::lock_file_operation(&WRITE_LOCK);
    let path = crate::copilot_config::mcp_config_path();
    let options = options_path();
    with_file_rollback(&options, || {
        crate::mcode_config::write_and_commit(
            &path,
            || sync_file(&path, &options, id, change),
            commit,
        )
    })
}

fn sync_file(
    path: &Path,
    options_path: &Path,
    id: &str,
    change: Change<'_>,
) -> Result<(), AppError> {
    let mut document = read(path)?;
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let mut options = read_options(options_path)?;
    let mut options_changed = false;
    let servers = document["mcpServers"].as_object_mut().unwrap();
    if let Change::Write(spec) = change {
        let unified = unified_spec(spec);
        super::validation::validate_server_spec(&unified)?;
        // 现有条目优先；没有时用停用前记下的 `tools`、`timeout`。
        let saved = options.remove(id);
        options_changed = saved.is_some();
        let mut merged = servers
            .get(id)
            .cloned()
            .or(saved)
            .unwrap_or_else(|| json!({}));
        let object = merged
            .as_object_mut()
            .ok_or_else(|| AppError::Config("Invalid GitHub Copilot CLI MCP entry".into()))?;
        for field in TRANSPORT_FIELDS {
            object.remove(field);
        }
        object.extend(native_spec(&unified).as_object().unwrap().clone());
        // 保留用户在 Copilot 里设的工具过滤；新条目和 `copilot mcp add` 一样默认全部。
        object.entry("tools").or_insert_with(|| json!(["*"]));
        servers.insert(id.into(), merged);
    } else {
        let removed = servers.remove(id);
        if matches!(change, Change::Delete) {
            options_changed = options.remove(id).is_some();
        } else if let Some(Value::Object(entry)) = removed {
            // 条目已经不在（重复停用）时保留之前记下的。
            let kept: Map<String, Value> = entry
                .into_iter()
                .filter(|(key, _)| NATIVE_OPTION_FIELDS.contains(&key.as_str()))
                .collect();
            options_changed = if kept.is_empty() {
                options.remove(id).is_some()
            } else {
                options.insert(id.into(), Value::Object(kept));
                true
            };
        }
    }
    // 先存选项再改 `mcp-config.json`：后者失败时条目还在，选项文件由调用方回滚。
    if options_changed {
        write_options(options_path, options)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write_private(
        path,
        serde_json::to_string_pretty(&document)
            .map_err(|e| AppError::Config(e.to_string()))?
            .as_bytes(),
    )
}

/// `settings.json` 的 `disabledMcpServers`；读不了时当作没有禁用项。
fn disabled_servers() -> Vec<String> {
    crate::copilot_config::read_settings_document()
        .ok()
        .flatten()
        .and_then(|settings| settings.get("disabledMcpServers").cloned())
        .and_then(|list| serde_json::from_value::<Vec<String>>(list).ok())
        .unwrap_or_default()
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&crate::copilot_config::mcp_config_path())?;
    let disabled = disabled_servers();
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = !disabled.iter().any(|name| name == id);
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.copilot = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    copilot: enabled,
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
            "Imported {count} GitHub Copilot CLI MCP servers; skipped {}. Native configurations were preserved.",
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

/// 原生条目 → CC Switch 统一格式：`local` 即 `stdio`，去掉 Copilot 独有的 `tools`、`timeout`。
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    let Some(object) = spec.as_object_mut() else {
        return spec;
    };
    object.remove("tools");
    object.remove("timeout");
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    match kind.as_deref() {
        Some("local") => {
            object.insert("type".into(), json!("stdio"));
        }
        Some("streamable-http") | Some("streamableHttp") => {
            object.insert("type".into(), json!("http"));
        }
        None if object.contains_key("url") => {
            object.insert("type".into(), json!("http"));
        }
        None if object.contains_key("command") => {
            object.insert("type".into(), json!("stdio"));
        }
        _ => {}
    }
    spec
}

/// 统一格式 → 原生条目，只留传输字段。
fn native_spec(unified: &Value) -> Value {
    let mut native = unified.clone();
    if let Some(object) = native.as_object_mut() {
        object.retain(|key, _| TRANSPORT_FIELDS.contains(&key.as_str()));
        if object.get("type").and_then(Value::as_str) == Some("stdio") {
            object.insert("type".into(), json!("local"));
            object.entry("args").or_insert_with(|| json!([]));
        }
    }
    native
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copilot_mcp_writes_native_transports_and_preserves_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        let options = dir.path().join("options.json");
        fs::write(
            &path,
            json!({"mcpServers":{
                "server":{"type":"http","url":"https://old.example.com","tools":["search"],"timeout":5000},
                "other":{"type":"local","command":"node","args":["server.js"],"tools":["*"]}
            }})
            .to_string(),
        )
        .unwrap();
        sync_file(
            &path,
            &options,
            "server",
            Change::Write(
                &json!({"type":"stdio","command":"npx","args":["-y","pkg"],"env":{"A":"1"}}),
            ),
        )
        .unwrap();
        let value = read(&path).unwrap();
        let server = &value["mcpServers"]["server"];
        assert_eq!(server["type"], "local");
        assert_eq!(server["command"], "npx");
        assert_eq!(server["env"]["A"], "1");
        assert!(server.get("url").is_none());
        assert_eq!(server["tools"], json!(["search"]));
        assert_eq!(server["timeout"], 5000);
        assert_eq!(value["mcpServers"]["other"]["command"], "node");

        sync_file(
            &path,
            &options,
            "remote",
            Change::Write(
                &json!({"type":"sse","url":"https://example.com/sse","headers":{"X":"1"}}),
            ),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert_eq!(value["mcpServers"]["remote"]["type"], "sse");
        assert_eq!(value["mcpServers"]["remote"]["tools"], json!(["*"]));

        sync_file(&path, &options, "server", Change::Delete).unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"].get("server").is_none());
        assert!(value["mcpServers"].get("other").is_some());
        assert!(!options.exists());
    }

    #[test]
    fn copilot_mcp_disable_then_enable_restores_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        let options = dir.path().join("options.json");
        fs::write(
            &path,
            json!({"mcpServers":{
                "limited":{"type":"local","command":"node","args":[],"tools":["only_this"],"timeout":12345}
            }})
            .to_string(),
        )
        .unwrap();
        let spec = json!({"type":"stdio","command":"node","args":[]});

        sync_file(&path, &options, "limited", Change::Disable).unwrap();
        assert!(read(&path).unwrap()["mcpServers"].get("limited").is_none());
        // 重复停用（条目已不在）不丢记下的选项。
        sync_file(&path, &options, "limited", Change::Disable).unwrap();

        sync_file(&path, &options, "limited", Change::Write(&spec)).unwrap();
        let entry = &read(&path).unwrap()["mcpServers"]["limited"];
        assert_eq!(entry["tools"], json!(["only_this"]));
        assert_eq!(entry["timeout"], 12345);
        assert_eq!(entry["command"], "node");
        assert!(read_options(&options).unwrap().is_empty());

        // 删除后再建同名条目，不套用旧的工具过滤。
        sync_file(&path, &options, "limited", Change::Disable).unwrap();
        sync_file(&path, &options, "limited", Change::Delete).unwrap();
        sync_file(&path, &options, "limited", Change::Write(&spec)).unwrap();
        let entry = &read(&path).unwrap()["mcpServers"]["limited"];
        assert_eq!(entry["tools"], json!(["*"]));
        assert!(entry.get("timeout").is_none());
    }

    #[test]
    fn copilot_native_entries_map_to_unified_specs() {
        let spec = unified_spec(&json!({"type":"local","command":"node","args":[],"tools":["*"]}));
        assert_eq!(spec, json!({"type":"stdio","command":"node","args":[]}));
        let spec = unified_spec(&json!({"url":"https://example.com/mcp"}));
        assert_eq!(spec["type"], "http");
    }

    #[test]
    fn copilot_mcp_keeps_options_when_the_options_file_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        let options = dir.path().join("options.json");
        fs::write(&path, json!({"mcpServers":{}}).to_string()).unwrap();
        fs::write(&options, "{broken").unwrap();
        let spec = json!({"type":"stdio","command":"node","args":[]});
        assert!(sync_file(&path, &options, "limited", Change::Write(&spec)).is_err());
        assert_eq!(fs::read_to_string(&options).unwrap(), "{broken");
        assert!(read(&path).unwrap()["mcpServers"].get("limited").is_none());
    }

    #[test]
    fn copilot_mcp_options_roll_back_when_the_commit_fails() {
        let dir = tempfile::tempdir().unwrap();
        let options = dir.path().join("options.json");
        fs::write(
            &options,
            json!({"limited":{"tools":["only_this"]}}).to_string(),
        )
        .unwrap();
        let before = fs::read_to_string(&options).unwrap();
        let result: Result<(), AppError> = with_file_rollback(&options, || {
            let mut map = read_options(&options)?;
            map.remove("limited");
            write_options(&options, map)?;
            Err(AppError::Message("commit failed".into()))
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&options).unwrap(), before);

        let fresh = dir.path().join("fresh.json");
        let result: Result<(), AppError> = with_file_rollback(&fresh, || {
            write_options(&fresh, Map::new())?;
            Err(AppError::Message("commit failed".into()))
        });
        assert!(result.is_err());
        assert!(!fresh.exists());
    }

    #[test]
    fn panicking_commit_restores_both_mcp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        let options = dir.path().join("options.json");
        let original =
            json!({"mcpServers":{"limited":{"command":"node","tools":["only_this"]}}}).to_string();
        fs::write(&path, &original).unwrap();
        for saved_options in [None, Some(b"{}".as_slice())] {
            if let Some(bytes) = saved_options {
                fs::write(&options, bytes).unwrap();
            }
            assert!(std::panic::catch_unwind(|| {
                let _: Result<(), AppError> = with_file_rollback(&options, || {
                    crate::mcode_config::write_and_commit(
                        &path,
                        || sync_file(&path, &options, "limited", Change::Disable),
                        || panic!("commit interrupted"),
                    )
                });
            })
            .is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            assert_eq!(fs::read(&options).ok().as_deref(), saved_options);
        }
    }

    #[test]
    fn copilot_mcp_preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-config.json");
        fs::write(&path, "{invalid").unwrap();
        let options = dir.path().join("options.json");
        assert!(sync_file(
            &path,
            &options,
            "test",
            Change::Write(&json!({"command":"node"}))
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }
}
