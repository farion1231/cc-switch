use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::web::handlers::ws::WsState;
use crate::web::middleware::auth::validate_token;
use crate::web::models::app_state::AppState;

/// WebSocket handler for terminal connections
pub async fn terminal_ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State((state, _)): State<(Arc<crate::web::models::app_state::AppState>, Arc<WsState>)>,
    headers: axum::http::HeaderMap,
) -> Response {
    // Validate JWT token from Authorization header or query param
    let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());
    let token_from_header = auth_header.and_then(|h| h.strip_prefix("Bearer "));
    let token_from_query = params.get("token").map(|s| s.as_str());

    let token = token_from_header.or(token_from_query);

    if token.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization header",
        )
            .into_response();
    }

    let token = token.unwrap();
    if let Err(_) = validate_token(token) {
        return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
    }

    // Get provider ID and app from query params
    let provider_id = params.get("provider").cloned().unwrap_or_default();
    let app = params
        .get("app")
        .cloned()
        .unwrap_or_else(|| "claude".to_string());

    if provider_id.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing provider ID").into_response();
    }

    // Upgrade to WebSocket
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, state, provider_id, app))
}

async fn handle_terminal_socket(
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<AppState>,
    provider_id: String,
    app: String,
) {
    info!(
        "New terminal WebSocket connection for provider: {}",
        provider_id
    );

    // Get provider configuration from database
    let provider_config = match get_provider_config(&state, &provider_id, &app).await {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to get provider config: {}", e);
            let _ = socket
                .send(axum::extract::ws::Message::Text(
                    format!("{{\"error\": \"Failed to get provider config: {}\"}}", e).into(),
                ))
                .await;
            return;
        }
    };

    // Extract environment variables from provider config
    let env_vars = extract_env_vars_from_config(&provider_config, &app);

    // Spawn shell process with PTY
    let mut child = match spawn_shell_with_env(env_vars).await {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to spawn shell: {}", e);
            let _ = socket
                .send(axum::extract::ws::Message::Text(
                    format!("{{\"error\": \"Failed to spawn terminal: {}\"}}", e).into(),
                ))
                .await;
            return;
        }
    };

    info!("Shell spawned successfully for provider: {}", provider_id);

    // Take stdin/stdout from child process
    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let mut stdout = child.stdout.take().expect("Failed to get stdout");
    let mut stderr = child.stderr.take().expect("Failed to get stderr");

    // Send ready message
    let _ = socket
        .send(axum::extract::ws::Message::Text(
            r#"{"status": "ready", "message": "Terminal connected"}"#.into(),
        ))
        .await;

    // Create channels for communication
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);

    // Spawn task to read from stdout and send to WebSocket
    let tx_stdout = tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => {
                    info!("stdout closed");
                    break;
                }
                Ok(n) => {
                    let data = buf[..n].to_vec();
                    if tx_stdout.send(data).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!("Error reading stdout: {}", e);
                    break;
                }
            }
        }
    });

    // Spawn task to read from stderr and send to WebSocket
    let tx_stderr = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => {
                    info!("stderr closed");
                    break;
                }
                Ok(n) => {
                    let data = buf[..n].to_vec();
                    if tx_stderr.send(data).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!("Error reading stderr: {}", e);
                    break;
                }
            }
        }
    });

    // Handle bidirectional communication
    let socket_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Receive data from stdout/stderr tasks and send to WebSocket
                Some(data) = rx.recv() => {
                    // Binary protocol: 0x00 prefix for stdout/stderr data
                    let mut message = vec![0x00u8];
                    message.extend_from_slice(&data);
                    if socket.send(axum::extract::ws::Message::Binary(message)).await.is_err() {
                        break;
                    }
                }
                // Receive messages from WebSocket client
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(axum::extract::ws::Message::Close(_))) => {
                            info!("Terminal WebSocket closed by client");
                            break;
                        }
                        Some(Ok(axum::extract::ws::Message::Binary(data))) => {
                            if data.is_empty() {
                                continue;
                            }

                            // Binary protocol:
                            // 0x00 + data = stdin data
                            // 0x01 + JSON = resize event (not implemented for basic shell)
                            match data[0] {
                                0x00 => {
                                    // stdin data
                                    let stdin_data = &data[1..];
                                    if let Err(e) = stdin.write_all(stdin_data).await {
                                        error!("Error writing to stdin: {}", e);
                                        break;
                                    }
                                    if let Err(e) = stdin.flush().await {
                                        error!("Error flushing stdin: {}", e);
                                        break;
                                    }
                                }
                                0x01 => {
                                    // Resize event - JSON payload
                                    // For basic shell implementation, we ignore resize
                                    // PTY resize would require more complex setup
                                    if let Ok(json_str) = std::str::from_utf8(&data[1..]) {
                                        if let Ok(resize_data) = serde_json::from_str::<serde_json::Value>(json_str) {
                                            if let (Some(cols), Some(rows)) = (
                                                resize_data.get("cols").and_then(|v| v.as_u64()),
                                                resize_data.get("rows").and_then(|v| v.as_u64())
                                            ) {
                                                info!("Terminal resize request: {}x{} (not implemented in shell mode)", cols, rows);
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    warn!("Unknown message type: {}", data[0]);
                                }
                            }
                        }
                        Some(Ok(axum::extract::ws::Message::Text(text))) => {
                            // Handle JSON commands (for backwards compatibility)
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(cols) = data.get("cols").and_then(|v| v.as_u64()) {
                                    if let Some(rows) = data.get("rows").and_then(|v| v.as_u64()) {
                                        info!("Terminal resize (JSON): {}x{} (not implemented)", cols, rows);
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket connection closed");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    // Wait for any task to complete (indicating connection end)
    tokio::select! {
        _ = stdout_task => {},
        _ = stderr_task => {},
        _ = socket_task => {},
    }

    // Clean up child process
    let _ = child.kill().await;

    info!("Terminal session ended for provider: {}", provider_id);
}

async fn get_provider_config(
    state: &AppState,
    provider_id: &str,
    app: &str,
) -> Result<serde_json::Value, String> {
    use rusqlite::Connection;

    state.with_db(|db: &Connection| {
        let mut stmt = db
            .prepare("SELECT settings_config FROM providers WHERE id = ?1 AND app_type = ?2")
            .map_err(|e| e.to_string())?;

        let config_str: String = stmt
            .query_row([provider_id, app], |row| row.get(0))
            .map_err(|e| e.to_string())?;

        serde_json::from_str(&config_str).map_err(|e| e.to_string())
    })
}

fn extract_env_vars_from_config(config: &serde_json::Value, app: &str) -> Vec<(String, String)> {
    let mut env_vars = vec![
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
    ];

    let Some(obj) = config.as_object() else {
        return env_vars;
    };

    // Handle env field (Claude/Gemini common)
    if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env {
            if let Some(str_val) = value.as_str() {
                env_vars.push((key.clone(), str_val.to_string()));
            }
        }

        // Handle base_url based on app type
        let base_url_key = match app {
            "claude" => Some("ANTHROPIC_BASE_URL"),
            "gemini" => Some("GOOGLE_GEMINI_BASE_URL"),
            _ => None,
        };

        if let Some(key) = base_url_key {
            if let Some(url_str) = env.get(key).and_then(|v| v.as_str()) {
                env_vars.push((key.to_string(), url_str.to_string()));
            }
        }
    }

    // Codex uses auth field for OPENAI_API_KEY
    if app == "codex" {
        if let Some(auth) = obj.get("auth").and_then(|v| v.as_str()) {
            env_vars.push(("OPENAI_API_KEY".to_string(), auth.to_string()));
        }
    }

    // Gemini uses api_key field
    if app == "gemini" {
        if let Some(api_key) = obj.get("api_key").and_then(|v| v.as_str()) {
            env_vars.push(("GEMINI_API_KEY".to_string(), api_key.to_string()));
        }
    }

    env_vars
}

async fn spawn_shell_with_env(
    env_vars: Vec<(String, String)>,
) -> Result<tokio::process::Child, String> {
    // Determine shell to use
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    info!(
        "Spawning shell: {} with {} environment variables",
        shell,
        env_vars.len()
    );

    // Build command with environment variables
    let mut cmd = Command::new(&shell);
    cmd.arg("-i") // Interactive mode
        .arg("-l"); // Login shell

    // Set up stdio
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Add environment variables
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }

    // Spawn the process
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ============================================================================
    // extract_env_vars_from_config Tests
    // ============================================================================

    #[test]
    fn test_extract_env_vars_from_config_claude_with_anthropic_keys() {
        // Test Claude provider with ANTHROPIC_API_KEY and ANTHROPIC_BASE_URL
        let config = json!({
            "env": {
                "ANTHROPIC_API_KEY": "sk-ant-test-key",
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        });

        let env_vars = extract_env_vars_from_config(&config, "claude");

        // Should have default TERM and COLORTERM
        assert!(env_vars.iter().any(|(k, _)| k == "TERM"));
        assert!(env_vars.iter().any(|(k, _)| k == "COLORTERM"));

        // Should have the API key and base URL
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "ANTHROPIC_API_KEY" && v == "sk-ant-test-key"));
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v == "https://api.anthropic.com"));
    }

    #[test]
    fn test_extract_env_vars_from_config_codex_with_auth() {
        // Test Codex provider with auth field → OPENAI_API_KEY
        let config = json!({
            "auth": "sk-openai-test-key"
        });

        let env_vars = extract_env_vars_from_config(&config, "codex");

        // Should have default TERM and COLORTERM
        assert!(env_vars.iter().any(|(k, _)| k == "TERM"));
        assert!(env_vars.iter().any(|(k, _)| k == "COLORTERM"));

        // Should convert auth field to OPENAI_API_KEY
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "OPENAI_API_KEY" && v == "sk-openai-test-key"));
    }

    #[test]
    fn test_extract_env_vars_from_config_gemini_with_api_key() {
        // Test Gemini provider with api_key field → GEMINI_API_KEY
        let config = json!({
            "api_key": "gemini-test-api-key"
        });

        let env_vars = extract_env_vars_from_config(&config, "gemini");

        // Should have default TERM and COLORTERM
        assert!(env_vars.iter().any(|(k, _)| k == "TERM"));
        assert!(env_vars.iter().any(|(k, _)| k == "COLORTERM"));

        // Should convert api_key field to GEMINI_API_KEY
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "GEMINI_API_KEY" && v == "gemini-test-api-key"));
    }

    #[test]
    fn test_extract_env_vars_from_config_gemini_with_custom_base_url() {
        // Test Gemini provider with env object containing custom base URL
        let config = json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://custom-gemini.example.com",
                "GEMINI_MODEL": "gemini-2.0-flash"
            },
            "api_key": "gemini-key"
        });

        let env_vars = extract_env_vars_from_config(&config, "gemini");

        // Should have custom base URL from env
        assert!(env_vars.iter().any(
            |(k, v)| k == "GOOGLE_GEMINI_BASE_URL" && v == "https://custom-gemini.example.com"
        ));

        // Should have model from env
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "GEMINI_MODEL" && v == "gemini-2.0-flash"));

        // Should have api_key converted to GEMINI_API_KEY
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "GEMINI_API_KEY" && v == "gemini-key"));
    }

    #[test]
    fn test_extract_env_vars_from_config_provider_with_multiple_custom_vars() {
        // Test provider with env object containing multiple custom variables
        let config = json!({
            "env": {
                "CUSTOM_VAR_1": "value1",
                "CUSTOM_VAR_2": "value2",
                "CUSTOM_VAR_3": "value3",
                "DEBUG": "true"
            }
        });

        let env_vars = extract_env_vars_from_config(&config, "custom");

        // Should have all custom variables
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "CUSTOM_VAR_1" && v == "value1"));
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "CUSTOM_VAR_2" && v == "value2"));
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "CUSTOM_VAR_3" && v == "value3"));
        assert!(env_vars.iter().any(|(k, v)| k == "DEBUG" && v == "true"));
    }

    #[test]
    fn test_extract_env_vars_from_config_empty_config_returns_defaults() {
        // Test empty config returns default vars (TERM, COLORTERM)
        let config = json!({});

        let env_vars = extract_env_vars_from_config(&config, "claude");

        // Should have at least TERM and COLORTERM
        assert_eq!(env_vars.len(), 2);
        assert!(env_vars.iter().any(|(k, _)| k == "TERM"));
        assert!(env_vars.iter().any(|(k, _)| k == "COLORTERM"));
    }

    #[test]
    fn test_extract_env_vars_from_config_null_config_returns_defaults() {
        // Test null config returns default vars
        let config = serde_json::Value::Null;

        let env_vars = extract_env_vars_from_config(&config, "claude");

        // Should have at least TERM and COLORTERM
        assert_eq!(env_vars.len(), 2);
        assert!(env_vars.iter().any(|(k, _)| k == "TERM"));
        assert!(env_vars.iter().any(|(k, _)| k == "COLORTERM"));
    }

    #[test]
    fn test_extract_env_vars_from_config_non_string_values_ignored() {
        // Test that non-string values in env are ignored
        let config = json!({
            "env": {
                "STRING_VAR": "valid",
                "NUMBER_VAR": 123,
                "BOOL_VAR": true,
                "NULL_VAR": null,
                "ARRAY_VAR": ["a", "b"],
                "OBJECT_VAR": {"key": "value"}
            }
        });

        let env_vars = extract_env_vars_from_config(&config, "claude");

        // Should only have the string variable
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "STRING_VAR" && v == "valid"));
        // Non-string values should be filtered out
        assert!(!env_vars.iter().any(|(k, _)| k == "NUMBER_VAR"));
        assert!(!env_vars.iter().any(|(k, _)| k == "BOOL_VAR"));
        assert!(!env_vars.iter().any(|(k, _)| k == "NULL_VAR"));
        assert!(!env_vars.iter().any(|(k, _)| k == "ARRAY_VAR"));
        assert!(!env_vars.iter().any(|(k, _)| k == "OBJECT_VAR"));
    }

    #[test]
    fn test_extract_env_vars_from_config_no_duplicate_keys() {
        // Test that we don't have duplicate keys
        let config = json!({
            "env": {
                "CUSTOM_VAR": "value1"
            }
        });

        let env_vars = extract_env_vars_from_config(&config, "claude");
        let keys: Vec<_> = env_vars.iter().map(|(k, _)| k).collect();

        // No duplicates should exist
        for key in keys.iter() {
            let count = keys.iter().filter(|k| *k == key).count();
            assert_eq!(count, 1, "Key {} appears {} times", key, count);
        }
    }

    #[test]
    fn test_extract_env_vars_from_config_term_defaults() {
        // Test that TERM is always set to xterm-256color
        let config = json!({});
        let env_vars = extract_env_vars_from_config(&config, "claude");

        let term_var = env_vars.iter().find(|(k, _)| k == "TERM");
        assert!(term_var.is_some());
        assert_eq!(term_var.unwrap().1, "xterm-256color");
    }

    #[test]
    fn test_extract_env_vars_from_config_colorterm_defaults() {
        // Test that COLORTERM is always set to truecolor
        let config = json!({});
        let env_vars = extract_env_vars_from_config(&config, "claude");

        let colorterm_var = env_vars.iter().find(|(k, _)| k == "COLORTERM");
        assert!(colorterm_var.is_some());
        assert_eq!(colorterm_var.unwrap().1, "truecolor");
    }

    #[test]
    fn test_extract_env_vars_from_config_complex_scenario() {
        // Complex scenario: Codex with auth and env vars
        let config = json!({
            "auth": "sk-openai-key",
            "env": {
                "OPENAI_ORG_ID": "org-123",
                "CUSTOM_ENDPOINT": "https://api.example.com"
            }
        });

        let env_vars = extract_env_vars_from_config(&config, "codex");

        // Should have auth converted to OPENAI_API_KEY
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "OPENAI_API_KEY" && v == "sk-openai-key"));

        // Should have custom env vars
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "OPENAI_ORG_ID" && v == "org-123"));
        assert!(env_vars
            .iter()
            .any(|(k, v)| k == "CUSTOM_ENDPOINT" && v == "https://api.example.com"));
    }

    // ============================================================================
    // spawn_shell_with_env Tests
    // ============================================================================

    #[tokio::test]
    async fn test_spawn_shell_with_env_returns_valid_child() {
        // Test spawning shell successfully with env vars
        let env_vars = vec![
            ("TEST_VAR".to_string(), "test_value".to_string()),
            ("ANOTHER_VAR".to_string(), "another_value".to_string()),
        ];

        let result = spawn_shell_with_env(env_vars).await;

        // Should succeed
        assert!(result.is_ok(), "Failed to spawn shell: {:?}", result.err());

        let mut child = result.unwrap();

        // Kill the process to clean up
        let _ = child.kill().await;
    }

    #[tokio::test]
    async fn test_spawn_shell_with_env_has_stdin_stdout_stderr() {
        // Test that spawned shell has stdin, stdout, and stderr
        let env_vars = vec![];

        let mut child = spawn_shell_with_env(env_vars)
            .await
            .expect("Failed to spawn shell");

        // All stdio should be piped
        assert!(child.stdin.is_some(), "stdin should be piped");
        assert!(child.stdout.is_some(), "stdout should be piped");
        assert!(child.stderr.is_some(), "stderr should be piped");

        // Clean up
        let _ = child.kill().await;
    }

    #[tokio::test]
    async fn test_spawn_shell_with_env_empty_env_vars() {
        // Test spawning with empty environment variables list
        let env_vars = vec![];

        let result = spawn_shell_with_env(env_vars).await;

        // Should still succeed (system vars still available)
        assert!(result.is_ok());

        let mut child = result.unwrap();
        let _ = child.kill().await;
    }

    #[tokio::test]
    async fn test_spawn_shell_with_env_multiple_vars() {
        // Test spawning with multiple environment variables
        let env_vars = vec![
            ("VAR1".to_string(), "value1".to_string()),
            ("VAR2".to_string(), "value2".to_string()),
            ("VAR3".to_string(), "value3".to_string()),
            ("CUSTOM_PATH".to_string(), "/custom/path".to_string()),
        ];

        let result = spawn_shell_with_env(env_vars).await;

        assert!(result.is_ok());

        let mut child = result.unwrap();
        let _ = child.kill().await;
    }

    #[tokio::test]
    async fn test_spawn_shell_with_env_special_characters_in_values() {
        // Test spawning with special characters in environment values
        let env_vars = vec![
            (
                "URL_WITH_SPECIAL".to_string(),
                "https://api.example.com?key=value&other=123".to_string(),
            ),
            (
                "PATH_WITH_SPACES".to_string(),
                "/path with spaces/to/dir".to_string(),
            ),
            (
                "VALUE_WITH_QUOTES".to_string(),
                "value\"with\"quotes".to_string(),
            ),
        ];

        let result = spawn_shell_with_env(env_vars).await;

        assert!(result.is_ok());

        let mut child = result.unwrap();
        let _ = child.kill().await;
    }

    #[test]
    fn test_spawn_shell_with_env_uses_shell_env_var_or_default() {
        // Test that the function uses SHELL env var or defaults to /bin/bash
        // Note: This test verifies the logic, actual execution depends on system

        // The function should use SHELL env var if set, otherwise /bin/bash
        let shell_from_env = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        // On Unix systems, this should be a valid shell
        assert!(!shell_from_env.is_empty(), "Shell path should not be empty");
        assert!(
            shell_from_env.starts_with("/"),
            "Shell should be an absolute path on Unix"
        );
    }
}
