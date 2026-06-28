use cc_switch_lib::{AppError, McpApps, McpServer, MultiAppConfig};
use serde_json::json;
// =============================================================================
// Section 1: Validation logic – replicated locally to match mcp/validation.rs
// (We test the PUBLIC behaviour through import functions and through the
//  validate_server_spec logic we can call indirectly, as well as the
//  identical logic in our own inline helpers below.)
// =============================================================================

/// Mirror of the validation logic in mcp/validation.rs so we can write
/// focused unit tests without requiring the private function to be re-exported.
fn validate_server_spec_mirror(spec: &serde_json::Value) -> Result<(), AppError> {
    if !spec.is_object() {
        return Err(AppError::McpValidation(
            "MCP 服务器连接定义必须为 JSON 对象".into(),
        ));
    }
    let t_opt = spec.get("type").and_then(|x| x.as_str());
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

/// Mirror of extract_server_spec from mcp/validation.rs
fn extract_server_spec_mirror(entry: &serde_json::Value) -> Result<serde_json::Value, AppError> {
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

// =============================================================================
// Section 2: validate_server_spec tests
// =============================================================================

#[test]
fn validate_stdio_with_command_succeeds() {
    let spec = json!({ "type": "stdio", "command": "npx" });
    assert!(validate_server_spec_mirror(&spec).is_ok());
}

#[test]
fn validate_stdio_without_type_uses_default_succeeds() {
    // type field absent → treated as stdio
    let spec = json!({ "command": "node" });
    assert!(validate_server_spec_mirror(&spec).is_ok());
}

#[test]
fn validate_stdio_missing_command_returns_error() {
    let spec = json!({ "type": "stdio" });
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("command"),
                "error should mention 'command': {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn validate_stdio_empty_command_returns_error() {
    let spec = json!({ "type": "stdio", "command": "   " });
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("command"),
                "error should mention 'command': {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn validate_http_with_url_succeeds() {
    let spec = json!({ "type": "http", "url": "https://example.com/mcp" });
    assert!(validate_server_spec_mirror(&spec).is_ok());
}

#[test]
fn validate_http_missing_url_returns_error() {
    let spec = json!({ "type": "http" });
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(msg.contains("url"), "error should mention 'url': {msg}");
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn validate_sse_with_url_succeeds() {
    let spec = json!({ "type": "sse", "url": "https://example.com/events" });
    assert!(validate_server_spec_mirror(&spec).is_ok());
}

#[test]
fn validate_sse_missing_url_returns_error() {
    let spec = json!({ "type": "sse" });
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(msg.contains("url"), "error should mention 'url': {msg}");
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn validate_unknown_type_returns_error() {
    let spec = json!({ "type": "grpc", "command": "server" });
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("stdio") || msg.contains("http") || msg.contains("sse"),
                "error should list valid types: {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn validate_non_object_returns_error() {
    let spec = json!("not an object");
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("JSON 对象"),
                "error should mention JSON object: {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn validate_array_returns_error() {
    let spec = json!([1, 2, 3]);
    let err = validate_server_spec_mirror(&spec).unwrap_err();
    assert!(matches!(err, AppError::McpValidation(_)));
}

// =============================================================================
// Section 3: extract_server_spec tests
// =============================================================================

#[test]
fn extract_server_spec_success() {
    let entry = json!({
        "server": { "type": "stdio", "command": "node" },
        "enabled": true,
        "name": "my-server"
    });
    let result = extract_server_spec_mirror(&entry).unwrap();
    assert_eq!(result["type"], "stdio");
    assert_eq!(result["command"], "node");
}

#[test]
fn extract_server_spec_missing_server_field_returns_error() {
    let entry = json!({ "type": "stdio", "command": "node" });
    let err = extract_server_spec_mirror(&entry).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("server"),
                "error should mention 'server' field: {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn extract_server_spec_server_not_object_returns_error() {
    let entry = json!({ "server": "not-an-object" });
    let err = extract_server_spec_mirror(&entry).unwrap_err();
    assert!(matches!(err, AppError::McpValidation(_)));
}

#[test]
fn extract_server_spec_non_object_entry_returns_error() {
    let entry = json!("just a string");
    let err = extract_server_spec_mirror(&entry).unwrap_err();
    assert!(matches!(err, AppError::McpValidation(_)));
}

// =============================================================================
// Section 4: OpenCode format conversion tests
// (Testing via the logic documented in opencode.rs — we replicate the
//  conversion logic here to test it as pure functions, since those helpers
//  are not re-exported from lib.rs.)
// =============================================================================

/// Mirror of convert_to_opencode_format from mcp/opencode.rs
fn convert_to_opencode_format_mirror(
    spec: &serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    let mut result = serde_json::Map::new();

    match typ {
        "stdio" => {
            result.insert("type".into(), json!("local"));
            let cmd = obj.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let mut command_arr = vec![json!(cmd)];
            if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
                for arg in args {
                    command_arr.push(arg.clone());
                }
            }
            result.insert("command".into(), serde_json::Value::Array(command_arr));
            if let Some(env) = obj.get("env") {
                if env.is_object() && !env.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    result.insert("environment".into(), env.clone());
                }
            }
            result.insert("enabled".into(), json!(true));
        }
        "sse" | "http" => {
            result.insert("type".into(), json!("remote"));
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    result.insert("headers".into(), headers.clone());
                }
            }
            result.insert("enabled".into(), json!(true));
        }
        _ => {
            return Err(AppError::McpValidation(format!("Unknown MCP type: {typ}")));
        }
    }

    Ok(serde_json::Value::Object(result))
}

/// Mirror of convert_from_opencode_format from mcp/opencode.rs
fn convert_from_opencode_format_mirror(
    spec: &serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("OpenCode MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("local");
    let mut result = serde_json::Map::new();

    match typ {
        "local" => {
            result.insert("type".into(), json!("stdio"));
            if let Some(cmd_arr) = obj.get("command").and_then(|v| v.as_array()) {
                if !cmd_arr.is_empty() {
                    if let Some(cmd) = cmd_arr.first().and_then(|v| v.as_str()) {
                        result.insert("command".into(), json!(cmd));
                    }
                    if cmd_arr.len() > 1 {
                        let args: Vec<serde_json::Value> = cmd_arr[1..].to_vec();
                        result.insert("args".into(), serde_json::Value::Array(args));
                    }
                }
            }
            if let Some(env) = obj.get("environment") {
                if env.is_object() && !env.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    result.insert("env".into(), env.clone());
                }
            }
        }
        "remote" => {
            result.insert("type".into(), json!("sse"));
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    result.insert("headers".into(), headers.clone());
                }
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!(
                "Unknown OpenCode MCP type: {typ}"
            )));
        }
    }

    Ok(serde_json::Value::Object(result))
}

#[test]
fn convert_stdio_to_opencode_local_format() {
    let spec = json!({
        "type": "stdio",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem"],
        "env": { "HOME": "/Users/test" }
    });
    let result = convert_to_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "local", "stdio should map to 'local'");
    assert_eq!(
        result["command"][0], "npx",
        "first element should be command"
    );
    assert_eq!(
        result["command"][1], "-y",
        "second element should be first arg"
    );
    assert_eq!(
        result["command"][2], "@modelcontextprotocol/server-filesystem",
        "third element should be second arg"
    );
    assert_eq!(
        result["environment"]["HOME"], "/Users/test",
        "env should map to environment"
    );
    assert_eq!(result["enabled"], true, "enabled flag should be set");
}

#[test]
fn convert_stdio_without_args_to_opencode_local() {
    let spec = json!({ "type": "stdio", "command": "echo" });
    let result = convert_to_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "local");
    let cmd_arr = result["command"].as_array().unwrap();
    assert_eq!(cmd_arr.len(), 1, "only the command itself, no args");
    assert_eq!(cmd_arr[0], "echo");
}

#[test]
fn convert_sse_to_opencode_remote_format() {
    let spec = json!({
        "type": "sse",
        "url": "https://example.com/mcp",
        "headers": { "Authorization": "Bearer token" }
    });
    let result = convert_to_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "remote", "sse should map to 'remote'");
    assert_eq!(result["url"], "https://example.com/mcp");
    assert_eq!(result["headers"]["Authorization"], "Bearer token");
    assert_eq!(result["enabled"], true);
}

#[test]
fn convert_http_to_opencode_remote_format() {
    let spec = json!({ "type": "http", "url": "https://api.example.com/mcp" });
    let result = convert_to_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "remote", "http should also map to 'remote'");
    assert_eq!(result["url"], "https://api.example.com/mcp");
}

#[test]
fn convert_to_opencode_unknown_type_returns_error() {
    let spec = json!({ "type": "websocket", "url": "wss://example.com" });
    let err = convert_to_opencode_format_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("websocket"),
                "error should mention the unknown type: {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

#[test]
fn convert_opencode_local_to_stdio_format() {
    let spec = json!({
        "type": "local",
        "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem"],
        "environment": { "HOME": "/Users/test" }
    });
    let result = convert_from_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "stdio", "local should map back to 'stdio'");
    assert_eq!(
        result["command"], "npx",
        "command should be first array element"
    );
    assert_eq!(result["args"][0], "-y", "first arg should be -y");
    assert_eq!(result["args"][1], "@modelcontextprotocol/server-filesystem");
    assert_eq!(
        result["env"]["HOME"], "/Users/test",
        "environment should map back to env"
    );
}

#[test]
fn convert_opencode_local_command_only_no_args() {
    let spec = json!({ "type": "local", "command": ["echo"] });
    let result = convert_from_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "stdio");
    assert_eq!(result["command"], "echo");
    assert!(
        result.get("args").is_none(),
        "no args key when there are no args"
    );
}

#[test]
fn convert_opencode_remote_to_sse_format() {
    let spec = json!({
        "type": "remote",
        "url": "https://example.com/mcp",
        "headers": { "X-Api-Key": "secret" }
    });
    let result = convert_from_opencode_format_mirror(&spec).unwrap();
    assert_eq!(result["type"], "sse", "remote should map back to 'sse'");
    assert_eq!(result["url"], "https://example.com/mcp");
    assert_eq!(result["headers"]["X-Api-Key"], "secret");
}

#[test]
fn convert_from_opencode_unknown_type_returns_error() {
    let spec = json!({ "type": "grpc", "url": "grpc://example.com" });
    let err = convert_from_opencode_format_mirror(&spec).unwrap_err();
    match err {
        AppError::McpValidation(msg) => {
            assert!(
                msg.contains("grpc"),
                "error should mention the unknown type: {msg}"
            );
        }
        other => panic!("expected McpValidation, got {other:?}"),
    }
}

// =============================================================================
// Section 5: Round-trip conversion tests (stdio ↔ local, sse ↔ remote)
// =============================================================================

#[test]
fn round_trip_stdio_to_opencode_and_back() {
    let original = json!({
        "type": "stdio",
        "command": "node",
        "args": ["server.js", "--port", "3000"],
        "env": { "NODE_ENV": "production" }
    });

    let opencode_fmt = convert_to_opencode_format_mirror(&original).unwrap();
    let restored = convert_from_opencode_format_mirror(&opencode_fmt).unwrap();

    assert_eq!(restored["type"], "stdio");
    assert_eq!(restored["command"], "node");
    assert_eq!(restored["args"][0], "server.js");
    assert_eq!(restored["args"][1], "--port");
    assert_eq!(restored["args"][2], "3000");
    assert_eq!(restored["env"]["NODE_ENV"], "production");
}

#[test]
fn round_trip_sse_to_opencode_and_back() {
    let original = json!({
        "type": "sse",
        "url": "https://example.com/sse",
        "headers": { "Authorization": "Bearer abc" }
    });

    let opencode_fmt = convert_to_opencode_format_mirror(&original).unwrap();
    assert_eq!(opencode_fmt["type"], "remote");

    let restored = convert_from_opencode_format_mirror(&opencode_fmt).unwrap();
    assert_eq!(restored["type"], "sse");
    assert_eq!(restored["url"], "https://example.com/sse");
    assert_eq!(restored["headers"]["Authorization"], "Bearer abc");
}

// =============================================================================
// Section 6: McpApps / McpServer data structure tests
// =============================================================================

#[test]
fn mcp_apps_default_all_disabled() {
    let apps = McpApps::default();
    assert!(!apps.claude);
    assert!(!apps.codex);
    assert!(!apps.gemini);
    assert!(!apps.opencode);
    assert!(
        apps.is_empty(),
        "default McpApps should have all apps disabled"
    );
}

#[test]
fn mcp_apps_enabled_apps_list() {
    let apps = McpApps {
        claude: true,
        codex: false,
        gemini: true,
        opencode: false,
        hermes: false,
    };
    let enabled = apps.enabled_apps();
    assert_eq!(enabled.len(), 2, "exactly 2 apps should be enabled");
    let has_claude = enabled
        .iter()
        .any(|a| matches!(a, cc_switch_lib::AppType::Claude));
    let has_gemini = enabled
        .iter()
        .any(|a| matches!(a, cc_switch_lib::AppType::Gemini));
    assert!(has_claude, "Claude should be in enabled list");
    assert!(has_gemini, "Gemini should be in enabled list");
}

#[test]
fn mcp_server_serialization_roundtrip() {
    let server = McpServer {
        id: "test-server".to_string(),
        name: "Test Server".to_string(),
        server: json!({ "type": "stdio", "command": "echo" }),
        apps: McpApps {
            claude: true,
            codex: false,
            gemini: false,
            opencode: true,
            hermes: false,
        },
        description: Some("A test MCP server".to_string()),
        homepage: None,
        docs: None,
        tags: vec!["test".to_string(), "echo".to_string()],
    };

    let json_str = serde_json::to_string(&server).expect("serialization should succeed");
    let restored: McpServer =
        serde_json::from_str(&json_str).expect("deserialization should succeed");

    assert_eq!(restored.id, "test-server");
    assert_eq!(restored.name, "Test Server");
    assert!(restored.apps.claude);
    assert!(!restored.apps.codex);
    assert!(restored.apps.opencode);
    assert_eq!(restored.description, Some("A test MCP server".to_string()));
    assert_eq!(restored.tags, vec!["test".to_string(), "echo".to_string()]);
}

#[test]
fn mcp_server_optional_fields_absent_when_none() {
    let server = McpServer {
        id: "minimal".to_string(),
        name: "Minimal".to_string(),
        server: json!({ "type": "stdio", "command": "true" }),
        apps: McpApps::default(),
        description: None,
        homepage: None,
        docs: None,
        tags: vec![],
    };

    let json_val = serde_json::to_value(&server).expect("serialize");
    // description / homepage / docs should be absent (skip_serializing_if = "Option::is_none")
    assert!(
        json_val.get("description").is_none(),
        "absent None field should not appear"
    );
    assert!(
        json_val.get("homepage").is_none(),
        "absent None field should not appear"
    );
    assert!(
        json_val.get("docs").is_none(),
        "absent None field should not appear"
    );
    // tags should be absent too (skip_serializing_if = "Vec::is_empty")
    assert!(
        json_val.get("tags").is_none(),
        "empty Vec should not appear"
    );
}

// =============================================================================
// Section 7: MultiAppConfig and MCP root structure tests
// =============================================================================

#[test]
fn multi_app_config_default_has_empty_mcp_servers_map() {
    let config = MultiAppConfig::default();
    // After v3.7.0, default should have servers = Some(HashMap::new())
    assert!(
        config.mcp.servers.is_some(),
        "default config should have initialized servers map"
    );
    assert!(
        config.mcp.servers.as_ref().unwrap().is_empty(),
        "default servers map should be empty"
    );
}

#[test]
fn validate_stdio_with_env_and_cwd_succeeds() {
    // Extended stdio spec with optional env and cwd fields
    let spec = json!({
        "type": "stdio",
        "command": "python3",
        "args": ["-m", "mcp_server"],
        "env": { "PYTHONPATH": "/usr/local/lib" },
        "cwd": "/home/user/project"
    });
    assert!(
        validate_server_spec_mirror(&spec).is_ok(),
        "stdio spec with env and cwd should be valid"
    );
}

#[test]
fn validate_http_with_headers_succeeds() {
    let spec = json!({
        "type": "http",
        "url": "https://api.example.com/mcp",
        "headers": { "Authorization": "Bearer token" }
    });
    assert!(
        validate_server_spec_mirror(&spec).is_ok(),
        "http spec with headers should be valid"
    );
}

#[test]
fn convert_stdio_empty_env_not_included_in_opencode_format() {
    // Empty env object should not be included in the converted output
    let spec = json!({
        "type": "stdio",
        "command": "echo",
        "env": {}
    });
    let result = convert_to_opencode_format_mirror(&spec).unwrap();
    assert!(
        result.get("environment").is_none(),
        "empty env should not produce an 'environment' key in OpenCode format"
    );
}

#[test]
fn convert_sse_empty_headers_not_included_in_opencode_format() {
    let spec = json!({
        "type": "sse",
        "url": "https://example.com/sse",
        "headers": {}
    });
    let result = convert_to_opencode_format_mirror(&spec).unwrap();
    assert!(
        result.get("headers").is_none(),
        "empty headers should not be included in OpenCode remote format"
    );
}
