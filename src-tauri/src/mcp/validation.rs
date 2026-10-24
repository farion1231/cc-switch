//! MCP 服务器配置验证模块

use serde_json::Value;

use crate::{app_config::AppType, error::AppError};

/// Validate every independent spec before saving or touching any live file.
pub(crate) fn validate_app_overrides(
    server: &crate::app_config::McpServer,
) -> Result<(), AppError> {
    for (app, key) in [
        (AppType::Claude, "mcp.error.invalidOverrideClaude"),
        (AppType::Codex, "mcp.error.invalidOverrideCodex"),
        (AppType::Gemini, "mcp.error.invalidOverrideGemini"),
        (AppType::GrokBuild, "mcp.error.invalidOverrideGrokbuild"),
        (AppType::OpenCode, "mcp.error.invalidOverrideOpencode"),
        (AppType::Hermes, "mcp.error.invalidOverrideHermes"),
        (AppType::Pi, "mcp.error.invalidOverridePi"),
        (AppType::Mcode, "mcp.error.invalidOverrideMcode"),
        (AppType::Kimi, "mcp.error.invalidOverrideKimi"),
        (AppType::Agy, "mcp.error.invalidOverrideAgy"),
        (AppType::Omp, "mcp.error.invalidOverrideOmp"),
    ] {
        let Some(spec) = server.app_overrides.get(&app) else {
            continue;
        };
        let invalid = || AppError::McpValidation(key.to_string());
        if spec.get("type").is_some_and(|v| !v.is_string()) {
            return Err(invalid());
        }
        // Validate the spec that will actually be written. In particular, an
        // explicit stdio transport without a command cannot masquerade as HTTP.
        validate_server_spec(spec).map_err(|_| invalid())?;
        if matches!(app, AppType::Pi | AppType::Agy) && spec["type"] == "sse" {
            return Err(invalid());
        }
        if spec.get("args").is_some_and(|v| {
            !v.as_array()
                .is_some_and(|args| args.iter().all(Value::is_string))
        }) || ["env", "headers"].iter().any(|field| {
            spec.get(field).is_some_and(|v| {
                !v.as_object()
                    .is_some_and(|rows| rows.values().all(Value::is_string))
            })
        }) || spec.get("cwd").is_some_and(|v| !v.is_string())
        {
            return Err(invalid());
        }
    }
    Ok(())
}

/// 基础校验：允许 stdio/http/sse；或省略 type（视为 stdio）。对应必填字段存在
pub fn validate_server_spec(spec: &Value) -> Result<(), AppError> {
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器连接定义必须为 JSON 对象".into(),
        ));
    }
    let t_opt = spec.get("type").and_then(|x| x.as_str());
    // 支持三种：stdio/http/sse；若缺省 type 则按 stdio 处理（与社区常见 .mcp.json 一致）
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true);
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    let is_sse = t_opt.map(|t| t == "sse").unwrap_or(false);

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
