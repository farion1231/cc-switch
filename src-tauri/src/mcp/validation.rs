//! MCP 服务器配置验证模块

use serde_json::Value;

use crate::error::AppError;

const TYPE_REQUIRED_FOR_URL: &str =
    "包含 url 字段的 MCP 服务器必须显式指定 type 为 'http' 或 'sse'";
const INVALID_TYPE: &str = "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'";

fn require_string_field(spec: &Value, field: &str, error: &str) -> Result<(), AppError> {
    let value = spec.get(field).and_then(|x| x.as_str()).unwrap_or("");
    if value.trim().is_empty() {
        return Err(AppError::McpValidation(error.into()));
    }

    Ok(())
}

/// 规范化并校验服务器配置。
///
/// 统一规则：
/// - `http_headers` 统一转为 `headers`
/// - 缺少 `type` 且有 `command` 时补齐为 `stdio`
/// - 缺少 `type` 且有 `url` 时直接报错，要求显式指定 `http` 或 `sse`
pub fn normalize_server_spec(spec: &Value) -> Result<Value, AppError> {
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器连接定义必须为 JSON 对象".into(),
        ));
    }

    let mut normalized = spec
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::McpValidation("MCP 服务器连接定义必须为 JSON 对象".into()))?;

    if let Some(http_headers) = normalized.remove("http_headers") {
        if let Some(headers) = normalized.get("headers") {
            if headers != &http_headers {
                return Err(AppError::McpValidation(
                    "MCP 服务器不能同时包含不同的 headers 与 http_headers".into(),
                ));
            }
        } else {
            normalized.insert("headers".into(), http_headers);
        }
    }

    let has_command = normalized.contains_key("command");
    let has_url = normalized.contains_key("url");

    let normalized_type = match normalized.get("type") {
        Some(Value::String(t)) => t.clone(),
        Some(_) => {
            return Err(AppError::McpValidation(INVALID_TYPE.into()));
        }
        None if has_command => "stdio".to_string(),
        None if has_url => {
            return Err(AppError::McpValidation(TYPE_REQUIRED_FOR_URL.into()));
        }
        None => "stdio".to_string(),
    };

    match normalized_type.as_str() {
        "stdio" => require_string_field(
            &Value::Object(normalized.clone()),
            "command",
            "stdio 类型的 MCP 服务器缺少 command 字段",
        )?,
        "http" => require_string_field(
            &Value::Object(normalized.clone()),
            "url",
            "http 类型的 MCP 服务器缺少 url 字段",
        )?,
        "sse" => require_string_field(
            &Value::Object(normalized.clone()),
            "url",
            "sse 类型的 MCP 服务器缺少 url 字段",
        )?,
        _ => return Err(AppError::McpValidation(INVALID_TYPE.into())),
    }

    normalized.insert("type".into(), Value::String(normalized_type));

    Ok(Value::Object(normalized))
}

/// 严格校验：数据库中的 MCP spec 必须是 canonical 形式。
pub fn validate_server_spec(spec: &Value) -> Result<(), AppError> {
    normalize_server_spec(spec)?;
    Ok(())
}

/// 从 MCP 条目中提取服务器规范
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
