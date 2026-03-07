//! OpenCode MCP configuration handling.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::error::AppError;

fn should_sync_opencode_mcp() -> bool {
    crate::opencode_config::get_opencode_dir().exists()
}

pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    Ok(crate::opencode_config::get_mcp_servers()?.into_iter().collect())
}

pub fn convert_to_opencode_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP 配置必须是对象".into()))?;
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("stdio");
    let mut result = serde_json::Map::new();

    match kind {
        "stdio" => {
            result.insert("type".into(), json!("local"));
            let command = obj.get("command").and_then(Value::as_str).unwrap_or("");
            let mut command_arr = vec![json!(command)];
            if let Some(args) = obj.get("args").and_then(Value::as_array) {
                command_arr.extend(args.iter().cloned());
            }
            result.insert("command".into(), Value::Array(command_arr));
            if let Some(env) = obj.get("env") {
                result.insert("environment".into(), env.clone());
            }
            result.insert("enabled".into(), json!(true));
        }
        "sse" | "http" => {
            result.insert("type".into(), json!("remote"));
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                result.insert("headers".into(), headers.clone());
            }
            result.insert("enabled".into(), json!(true));
        }
        _ => {
            return Err(AppError::McpValidation(format!("未知的 OpenCode MCP 类型: {kind}")));
        }
    }

    Ok(Value::Object(result))
}

pub fn convert_from_opencode_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("OpenCode MCP 配置必须是对象".into()))?;
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("local");
    let mut result = serde_json::Map::new();

    match kind {
        "local" => {
            result.insert("type".into(), json!("stdio"));
            if let Some(command_arr) = obj.get("command").and_then(Value::as_array) {
                if let Some(command) = command_arr.first().and_then(Value::as_str) {
                    result.insert("command".into(), json!(command));
                }
                if command_arr.len() > 1 {
                    result.insert("args".into(), Value::Array(command_arr[1..].to_vec()));
                }
            }
            if let Some(env) = obj.get("environment") {
                result.insert("env".into(), env.clone());
            }
        }
        "remote" => {
            result.insert("type".into(), json!("sse"));
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                result.insert("headers".into(), headers.clone());
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!("未知的 OpenCode MCP 类型: {kind}")));
        }
    }

    Ok(Value::Object(result))
}

pub fn sync_single_server_to_opencode(id: &str, spec: &Value) -> Result<(), AppError> {
    if !should_sync_opencode_mcp() {
        return Ok(());
    }

    let opencode_spec = convert_to_opencode_format(spec)?;
    crate::opencode_config::set_mcp_server(id, opencode_spec)
}

pub fn remove_server_from_opencode(id: &str) -> Result<(), AppError> {
    if !should_sync_opencode_mcp() {
        return Ok(());
    }

    crate::opencode_config::remove_mcp_server(id)
}
