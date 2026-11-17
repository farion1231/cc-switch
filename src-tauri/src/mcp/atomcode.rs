use crate::app_config::{McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Mutex};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

// AtomCode 原生字段（官方文档 mcp.json schema）：stdio 用 command/args/env，
// HTTP 用 url/headers/auth。timeout_ms 与 disabled 是两者共有的可选字段，但
// timeout_ms 属原生选项——重写时保留，不随传输字段清掉（同 kimi 的 toolTimeoutMs）。
const TRANSPORT_FIELDS: [&str; 6] = ["command", "args", "env", "url", "headers", "auth"];

fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({"mcpServers": {}})),
        Err(e) => return Err(AppError::io(path, e)),
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Config("Invalid AtomCode MCP JSON".into()))?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(AppError::Config(
            "Invalid AtomCode mcpServers object".into(),
        ));
    }
    Ok(value)
}

pub fn sync(id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    sync_file(
        &crate::atomcode_config::data_dir().join("mcp.json"),
        id,
        spec,
    )
}

pub(crate) fn sync_and_commit<T>(
    id: &str,
    spec: Option<&Value>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = crate::atomcode_config::data_dir().join("mcp.json");
    crate::mcode_config::write_and_commit(&path, || sync_file(&path, id, spec), commit)
}

fn sync_file(path: &Path, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
    let mut document = read(path)?;
    if document.get("mcpServers").is_none() {
        document["mcpServers"] = json!({});
    }
    let servers = document["mcpServers"].as_object_mut().unwrap();
    if let Some(spec) = spec {
        super::validation::validate_server_spec(&unified_spec(spec))?;
        let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
        let object = merged
            .as_object_mut()
            .ok_or_else(|| AppError::Config("Invalid AtomCode MCP entry".into()))?;
        for field in TRANSPORT_FIELDS {
            object.remove(field);
        }
        object.remove("disabled");
        let native = native_spec(spec);
        object.extend(native.as_object().unwrap().clone());
        servers.insert(id.into(), merged);
    } else {
        // 关闭时对齐 AtomCode 原生语义（config::set_mcp_server_disabled_in_json_file）：
        // 写 `disabled: true` 保留整条条目，而不是删除。这样 AtomCode 自管条目携带的
        // managedBy / trust / autoApprove / timeout_ms 等原生元数据不会丢失，且重新
        // 启用（spec=Some 时 remove("disabled")）可无损恢复。仅作用于已存在条目，
        // 从不凭空新增一个空的 disabled 条目。
        if let Some(entry) = servers.get_mut(id).and_then(Value::as_object_mut) {
            entry.insert("disabled".into(), json!(true));
        }
    }
    atomic_write_private(
        path,
        serde_json::to_string_pretty(&document)
            .map_err(|e| AppError::Config(e.to_string()))?
            .as_bytes(),
    )
}

pub fn import(state: &AppState) -> Result<usize, AppError> {
    let document = read(&crate::atomcode_config::data_dir().join("mcp.json"))?;
    let mut existing = state.db.get_all_mcp_servers()?;
    let mut count = 0;
    let mut skipped = Vec::new();
    for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
        let mut spec = unified_spec(native);
        if super::validation::validate_server_spec(&spec).is_err() {
            skipped.push(format!("'{id}': invalid transport configuration"));
            continue;
        }
        let enabled = !native
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        spec.as_object_mut().unwrap().remove("disabled");
        let server = if let Some(mut server) = existing.shift_remove(id) {
            if transport_spec(&server.server) != transport_spec(&spec) {
                skipped.push(format!("'{id}': conflicts with an existing server"));
                continue;
            }
            server.apps.atomcode = enabled;
            server
        } else {
            count += 1;
            McpServer {
                id: id.clone(),
                name: id.clone(),
                server: spec,
                apps: McpApps {
                    atomcode: enabled,
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
            "Imported {count} AtomCode MCP servers; skipped {}. Native configurations were preserved.",
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

/// 统一 spec（`type: stdio|http`）→ AtomCode 原生 spec（无 type 键）。
fn native_spec(unified: &Value) -> Value {
    let mut native = unified.clone();
    if let Some(object) = native.as_object_mut() {
        object.remove("type");
    }
    native
}

/// AtomCode 原生 spec → 统一 spec：url 即 http，command 即 stdio。
fn unified_spec(native: &Value) -> Value {
    let mut spec = native.clone();
    if spec.is_object() && spec.get("type").is_none() {
        if spec.get("url").is_some() {
            spec["type"] = json!("http");
        } else if spec.get("command").is_some() {
            spec["type"] = json!("stdio");
        }
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomcode_mcp_writes_native_fields_and_preserves_native_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            json!({"custom":"keep","mcpServers":{"server":{"url":"https://old.example.com","disabled":true,"timeout_ms":1234},"other":{"command":"node","args":["server.js"]}}}).to_string(),
        )
        .unwrap();
        sync_file(
            &path,
            "server",
            Some(&json!({"type":"http","url":"https://example.com","headers":{"X-Test":"keep"}})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert_eq!(value["mcpServers"]["server"]["url"], "https://example.com");
        assert_eq!(value["mcpServers"]["server"]["timeout_ms"], 1234);
        // 重新启用后原生的 disabled 标记被移除
        assert!(value["mcpServers"]["server"].get("disabled").is_none());
        assert!(value["mcpServers"]["server"].get("type").is_none());
        assert_eq!(value["mcpServers"]["other"]["command"], "node");
        assert_eq!(value["custom"], "keep");

        // 关闭：对齐 AtomCode 原生语义，写 disabled:true 保留整条条目与原生选项
        sync_file(&path, "server", None).unwrap();
        let value = read(&path).unwrap();
        assert_eq!(value["mcpServers"]["server"]["disabled"], true);
        assert_eq!(value["mcpServers"]["server"]["url"], "https://example.com");
        assert_eq!(value["mcpServers"]["server"]["timeout_ms"], 1234);
        assert_eq!(value["mcpServers"]["other"]["command"], "node");
        assert_eq!(value["custom"], "keep");

        // 重新启用后 disabled 被移除，条目其余字段保持不变
        sync_file(
            &path,
            "server",
            Some(&json!({"type":"http","url":"https://example.com"})),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"]["server"].get("disabled").is_none());
        assert_eq!(value["mcpServers"]["server"]["timeout_ms"], 1234);

        // 关闭一个不存在的条目：不凭空新增空条目
        sync_file(&path, "never-defined", None).unwrap();
        let value = read(&path).unwrap();
        assert!(value["mcpServers"].get("never-defined").is_none());
    }

    #[test]
    fn atomcode_mcp_disable_preserves_native_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            json!({"mcpServers":{"huaweicloud":{"command":"node","args":["srv.mjs"],"managedBy":"atomcode-huaweicloud-v1","trust":false,"autoApprove":[],"timeout_ms":300000}}}).to_string(),
        )
        .unwrap();
        sync_file(&path, "huaweicloud", None).unwrap();
        let value = read(&path).unwrap();
        let entry = &value["mcpServers"]["huaweicloud"];
        assert_eq!(entry["disabled"], true);
        assert_eq!(entry["managedBy"], "atomcode-huaweicloud-v1");
        assert_eq!(entry["trust"], false);
        assert_eq!(entry["autoApprove"], json!([]));
        assert_eq!(entry["timeout_ms"], 300000);
        assert_eq!(entry["command"], "node");
    }

    #[test]
    fn atomcode_mcp_preserves_malformed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(&path, "{invalid").unwrap();
        assert!(sync_file(&path, "test", Some(&json!({"command":"node"}))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{invalid");
    }

    #[test]
    fn atomcode_mcp_unified_spec_round_trip() {
        assert_eq!(
            unified_spec(&json!({"command":"node","args":["-y"]})),
            json!({"command":"node","args":["-y"],"type":"stdio"})
        );
        assert_eq!(
            unified_spec(&json!({"url":"https://example.com"})),
            json!({"url":"https://example.com","type":"http"})
        );
        assert_eq!(
            native_spec(&json!({"type":"stdio","command":"node"})),
            json!({"command":"node"})
        );
    }
}
