//! MCP server spec validation helpers.

use serde_json::Value;

use crate::error::AppError;

pub fn validate_mcp_config(config: &Value) -> Result<(), AppError> {
    if !config.is_object() {
        return Err(AppError::McpValidation(
            "MCP 配置必须是 JSON 对象".to_string(),
        ));
    }

    for (id, spec) in config.as_object().expect("checked object") {
        validate_server_spec(spec).map_err(|error| {
            AppError::McpValidation(format!("MCP 服务器 '{id}' 配置无效: {error}"))
        })?;
    }

    Ok(())
}

pub fn validate_server_spec(spec: &Value) -> Result<(), AppError> {
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器连接定义必须为 JSON 对象".into(),
        ));
    }

    let type_value = spec.get("type").and_then(Value::as_str);
    let is_stdio = type_value.map(|value| value == "stdio").unwrap_or(true);
    let is_http = type_value.map(|value| value == "http").unwrap_or(false);
    let is_sse = type_value.map(|value| value == "sse").unwrap_or(false);

    if !(is_stdio || is_http || is_sse) {
        return Err(AppError::McpValidation(
            "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）".into(),
        ));
    }

    if is_stdio {
        let command = spec
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if command.trim().is_empty() {
            return Err(AppError::McpValidation(
                "stdio 类型的 MCP 服务器缺少 command 字段".into(),
            ));
        }
    }

    if is_http || is_sse {
        let url = spec.get("url").and_then(Value::as_str).unwrap_or_default();
        if url.trim().is_empty() {
            return Err(AppError::McpValidation(format!(
                "{} 类型的 MCP 服务器缺少 url 字段",
                if is_http { "http" } else { "sse" }
            )));
        }
    }

    Ok(())
}

pub fn extract_server_spec(entry: &Value) -> Result<Value, AppError> {
    let obj = entry
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP 服务器条目必须为 JSON 对象".into()))?;
    let server = obj
        .get("server")
        .ok_or_else(|| AppError::McpValidation("MCP 服务器条目缺少 server 字段".into()))?;

    if !server.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器 server 字段必须为 JSON 对象".into(),
        ));
    }

    Ok(server.clone())
}
