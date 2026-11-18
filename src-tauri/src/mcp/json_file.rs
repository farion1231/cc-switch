//! `{"mcpServers": {...}}` 形式的原生 MCP 文件（Kimi Code、MiniMax Code）。
//!
//! 写入只替换条目的传输字段并标记 `enabled: true`；条目上的其他字段、用户自己加的
//! 服务器和顶层其他字段都原样保留。坏文件报错，不覆盖。

use crate::app_config::{AppType, McpApps, McpServer};
use crate::config::atomic_write_private;
use crate::error::AppError;
use crate::store::AppState;
use serde_json::{json, Value};
use std::{fs, path::Path, path::PathBuf, sync::Mutex};

pub(crate) struct JsonMcpFile {
    /// 错误信息里的应用名
    pub label: &'static str,
    /// 导入结果信息里的应用名
    pub import_label: &'static str,
    pub app: AppType,
    pub path: fn() -> PathBuf,
    /// 原生格式用 `transport` 字段（而不是 `type`）表示 sse 传输
    pub transport_key: bool,
    pub lock: Mutex<()>,
}

impl JsonMcpFile {
    fn transport_fields(&self) -> &'static [&'static str] {
        if self.transport_key {
            &[
                "command",
                "args",
                "env",
                "url",
                "headers",
                "type",
                "transport",
            ]
        } else {
            &["command", "args", "env", "url", "headers", "type"]
        }
    }

    pub(crate) fn read(&self, path: &Path) -> Result<Value, AppError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(json!({"mcpServers": {}}))
            }
            Err(e) => return Err(AppError::io(path, e)),
        };
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| AppError::Config(format!("Invalid {} MCP JSON", self.label)))?;
        if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
            return Err(AppError::Config(format!(
                "Invalid {} mcpServers object",
                self.label
            )));
        }
        Ok(value)
    }

    pub(crate) fn sync(&self, id: &str, spec: Option<&Value>) -> Result<(), AppError> {
        let _guard = crate::sync_utils::lock_file_operation(&self.lock);
        self.sync_file(&(self.path)(), id, spec)
    }

    pub(crate) fn sync_and_commit<T>(
        &self,
        id: &str,
        spec: Option<&Value>,
        commit: impl FnOnce() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let _guard = crate::sync_utils::lock_file_operation(&self.lock);
        let path = (self.path)();
        crate::mcode_config::write_and_commit(&path, || self.sync_file(&path, id, spec), commit)
    }

    pub(crate) fn sync_file(
        &self,
        path: &Path,
        id: &str,
        spec: Option<&Value>,
    ) -> Result<(), AppError> {
        let mut document = self.read(path)?;
        if document.get("mcpServers").is_none() {
            document["mcpServers"] = json!({});
        }
        let servers = document["mcpServers"].as_object_mut().unwrap();
        if let Some(spec) = spec {
            super::validation::validate_server_spec(&self.unified_spec(spec))?;
            let mut merged = servers.get(id).cloned().unwrap_or_else(|| json!({}));
            let object = merged
                .as_object_mut()
                .ok_or_else(|| AppError::Config(format!("Invalid {} MCP entry", self.label)))?;
            for field in self.transport_fields() {
                object.remove(*field);
            }
            object.extend(self.native_spec(spec).as_object().unwrap().clone());
            object.insert("enabled".into(), json!(true));
            servers.insert(id.into(), merged);
        } else {
            servers.remove(id);
        }
        atomic_write_private(
            path,
            serde_json::to_string_pretty(&document)
                .map_err(|e| AppError::Config(e.to_string()))?
                .as_bytes(),
        )
    }

    pub(crate) fn import(&self, state: &AppState) -> Result<usize, AppError> {
        let document = self.read(&(self.path)())?;
        let mut existing = state.db.get_all_mcp_servers()?;
        let mut count = 0;
        let mut skipped = Vec::new();
        for (id, native) in document["mcpServers"].as_object().into_iter().flatten() {
            let mut spec = self.unified_spec(native);
            if super::validation::validate_server_spec(&spec).is_err() {
                skipped.push(format!("'{id}': invalid transport configuration"));
                continue;
            }
            let enabled = native
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            spec.as_object_mut().unwrap().remove("enabled");
            let server = if let Some(mut server) = existing.shift_remove(id) {
                if self.transport_spec(&server.server) != self.transport_spec(&spec) {
                    skipped.push(format!("'{id}': conflicts with an existing server"));
                    continue;
                }
                server.apps.set_enabled_for(&self.app, enabled);
                server
            } else {
                count += 1;
                let mut apps = McpApps::default();
                apps.set_enabled_for(&self.app, enabled);
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec,
                    apps,
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
                "Imported {count} {} MCP servers; skipped {}. Native configurations were preserved.",
                self.import_label,
                skipped.join("; ")
            )))
        }
    }

    fn transport_spec(&self, spec: &Value) -> Value {
        let mut spec = self.unified_spec(spec);
        if let Some(object) = spec.as_object_mut() {
            object.retain(|key, _| self.transport_fields().contains(&key.as_str()));
        }
        spec
    }

    /// 原生条目 → 统一结构
    pub(crate) fn unified_spec(&self, native: &Value) -> Value {
        let mut spec = native.clone();
        if self.transport_key {
            if let Some(object) = spec.as_object_mut() {
                if let Some(transport) = object.remove("transport") {
                    object.insert("type".into(), transport);
                }
            }
        }
        if spec["type"] == "streamable-http"
            || (spec.is_object() && spec.get("type").is_none() && spec.get("url").is_some())
        {
            spec["type"] = json!("http");
        } else if spec.is_object() && spec.get("type").is_none() && spec.get("command").is_some() {
            spec["type"] = json!("stdio");
        }
        spec
    }

    /// 统一结构 → 写入原生条目的传输字段
    fn native_spec(&self, spec: &Value) -> Value {
        if !self.transport_key {
            return spec.clone();
        }
        let mut native = self.unified_spec(spec);
        let kind = native.as_object_mut().unwrap().remove("type");
        if kind.as_ref().and_then(Value::as_str) == Some("sse") {
            native["transport"] = json!("sse");
        }
        native
    }
}
