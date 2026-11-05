//! MCP 服务器配置验证模块

use serde_json::Value;

use crate::error::AppError;

/// 基础校验：允许 stdio/http/sse；或省略 type（只有 url 视为 http，否则 stdio）。
/// 对应必填字段存在
pub fn validate_server_spec(spec: &Value) -> Result<(), AppError> {
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器连接定义必须为 JSON 对象".into(),
        ));
    }
    // 与写出侧同一套推断：校验和投影必须对同一份规范得出同一种传输，否则只有 url 的
    // 规范会在导入时被当成缺 command 的 stdio 拒绝，写出时却按 http 处理。
    let transport = spec_transport_type(spec);
    let is_stdio = transport == "stdio";
    let is_http = transport == "http";
    let is_sse = transport == "sse";

    if !(is_stdio || is_http || is_sse) {
        return Err(AppError::McpValidation(
            "MCP 服务器 type 必须是 'stdio'、'http' 或 'sse'（或省略表示 stdio）".into(),
        ));
    }

    if is_stdio {
        let cmd = spec.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.trim().is_empty() {
            return Err(AppError::McpValidation(
                "stdio 类型的 MCP 服务器缺少 command 字段".into(),
            ));
        }
    }
    if is_http {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.trim().is_empty() {
            return Err(AppError::McpValidation(
                "http 类型的 MCP 服务器缺少 url 字段".into(),
            ));
        }
    }
    if is_sse {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.trim().is_empty() {
            return Err(AppError::McpValidation(
                "sse 类型的 MCP 服务器缺少 url 字段".into(),
            ));
        }
    }
    Ok(())
}

/// 写出侧的传输类型（各应用把统一规范投影成原生格式时共用）。
///
/// 规范里可能没有 `type`（面板 JSON 模式粘贴 `{"url": …}`、deeplink 都不补），
/// 此时不能一律当 stdio：那样 url 型服务器会被投影成没有 `url` 的 stdio 条目
/// （Codex 报 `url is not supported for stdio` 整份 config.toml 加载失败，
/// Hermes 只剩 `enabled: true` 的空壳）。显式写了 stdio 却只有 `url` 的规范同理。
pub(crate) fn spec_transport_type(spec: &Value) -> &str {
    let has = |key: &str| {
        spec.get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    let explicit = spec.get("type").and_then(|v| v.as_str());
    if matches!(explicit, None | Some("stdio")) && !has("command") && has("url") {
        return "http";
    }
    explicit.unwrap_or("stdio")
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validation_infers_transport_like_the_projections() {
        // 只有 url：各应用都按 http 写出，校验也必须接受
        assert!(validate_server_spec(&json!({"url": "https://example.com/mcp"})).is_ok());
        assert!(validate_server_spec(&json!({"command": "node"})).is_ok());
        assert!(validate_server_spec(&json!({})).is_err());
        assert!(validate_server_spec(&json!({"type": "http"})).is_err());
        assert!(validate_server_spec(&json!({"type": "ws", "url": "wss://x"})).is_err());
    }
}
