//! Account-scoped HTTP transport for Codex quota-window activation.
//!
//! This module deliberately bypasses the live provider router and the native
//! Codex CLI. It sends one isolated request with the managed OAuth token for
//! the claimed account, then drains the complete Responses stream before
//! deciding whether the activation outcome is known.

use crate::proxy::providers::codex_oauth_auth::{
    CodexOAuthManager, CODEX_OAUTH_CLIENT_VERSION, CODEX_OAUTH_ORIGINATOR,
};
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use futures::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

pub(crate) const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(60);
const ACTIVATION_PROMPT: &str = "hello";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivationModel {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivationRunResult {
    pub(crate) status: &'static str,
    pub(crate) exit_code: Option<i32>,
    pub(crate) error: Option<String>,
    pub(crate) actual_model: Option<String>,
}

pub(crate) async fn run_activation_http_for_account(
    manager: &CodexOAuthManager,
    account_id: &str,
    model: &ActivationModel,
) -> ActivationRunResult {
    let token = match manager.get_valid_token_for_account(account_id).await {
        Ok(token) => token,
        Err(error) => {
            return failed_result(
                "authentication",
                &error.to_string(),
                &[account_id.to_string()],
                None,
            );
        }
    };
    let workspace = match manager.chatgpt_account_id_for_account(account_id).await {
        Ok(workspace) => workspace,
        Err(error) => {
            return failed_result(
                "authentication",
                &error.to_string(),
                &[account_id.to_string()],
                None,
            );
        }
    };

    let client = crate::proxy::http_client::get();
    run_activation_http(&client, CODEX_RESPONSES_URL, &token, &workspace, model).await
}

async fn run_activation_http(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    workspace: &str,
    model: &ActivationModel,
) -> ActivationRunResult {
    match tokio::time::timeout(
        ACTIVATION_TIMEOUT,
        run_activation_http_inner(client, endpoint, token, workspace, model),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => unknown_result(
            "timeout: activation outcome is unknown",
            &[token.to_string(), workspace.to_string()],
        ),
    }
}

async fn run_activation_http_inner(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    workspace: &str,
    model: &ActivationModel,
) -> ActivationRunResult {
    let request = build_activation_request_to_endpoint(client, endpoint, token, workspace, model);
    let response = match request.send().await {
        Err(error) => {
            return unknown_result(
                &format!("transport: {error}"),
                &[token.to_string(), workspace.to_string()],
            );
        }
        Ok(response) => response,
    };

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let body = read_error_body(response).await;
        return failed_result(
            "authentication",
            &format!("HTTP {status}: {body}"),
            &[token.to_string(), workspace.to_string()],
            None,
        );
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let body = read_error_body(response).await;
        return failed_result(
            "quota",
            &format!("HTTP {status}: {body}"),
            &[token.to_string(), workspace.to_string()],
            None,
        );
    }
    if !status.is_success() {
        let body = read_error_body(response).await;
        let message = format!("HTTP {status}: {body}");
        if status.is_server_error() {
            return unknown_result(&message, &[token.to_string(), workspace.to_string()]);
        }
        return failed_result(
            "request",
            &message,
            &[token.to_string(), workspace.to_string()],
            None,
        );
    }

    let body = match read_response_body(response).await {
        Ok(body) => body,
        Err(error) => {
            return unknown_result(
                &format!("response stream: {error}"),
                &[token.to_string(), workspace.to_string()],
            );
        }
    };
    match classify_response_body(&body, &model.model) {
        ParsedResponse::Succeeded { actual_model } => ActivationRunResult {
            status: "succeeded",
            exit_code: None,
            error: None,
            actual_model,
        },
        ParsedResponse::Failed { error } => failed_result(
            "response",
            &error,
            &[token.to_string(), workspace.to_string()],
            Some(model.model.clone()),
        ),
        ParsedResponse::Unknown { error } => {
            unknown_result(&error, &[token.to_string(), workspace.to_string()])
        }
    }
}

#[cfg(test)]
pub(crate) fn build_activation_request(
    client: &reqwest::Client,
    token: &str,
    workspace: &str,
    model: &ActivationModel,
) -> reqwest::RequestBuilder {
    build_activation_request_to_endpoint(client, CODEX_RESPONSES_URL, token, workspace, model)
}

fn build_activation_request_to_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    workspace: &str,
    model: &ActivationModel,
) -> reqwest::RequestBuilder {
    let mut body = json!({
        "model": model.model,
        "input": [{
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": ACTIVATION_PROMPT
            }]
        }],
        // Keep the activation request aligned with the existing Codex OAuth
        // Responses transform. The ChatGPT consumer backend expects these
        // stateless/default fields even when the request has no tools or
        // instructions of its own.
        "instructions": "",
        "tools": [],
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "store": false,
        "stream": true
    });
    if model.reasoning_effort.as_deref() == Some("low") {
        body["reasoning"] = json!({ "effort": "low" });
    }

    client
        .post(endpoint)
        .query(&[("client_version", CODEX_OAUTH_CLIENT_VERSION)])
        .header("Authorization", format!("Bearer {token}"))
        .header("ChatGPT-Account-Id", workspace)
        .header("originator", CODEX_OAUTH_ORIGINATOR)
        .header("version", CODEX_OAUTH_CLIENT_VERSION)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .header("Accept-Encoding", "identity")
        .json(&body)
}

async fn read_error_body(response: reqwest::Response) -> String {
    match tokio::time::timeout(ACTIVATION_TIMEOUT, response.text()).await {
        Ok(Ok(body)) => truncate_text(&body),
        Ok(Err(error)) => format!("failed to read error body: {error}"),
        Err(_) => "error body read timeout".to_string(),
    }
}

async fn read_response_body(response: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let mut decoded = String::new();
    let mut utf8_remainder = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(format!("response exceeded {MAX_RESPONSE_BYTES} bytes"));
        }
        body.extend_from_slice(&chunk);
        append_utf8_safe(&mut decoded, &mut utf8_remainder, &chunk);
    }
    if !utf8_remainder.is_empty() {
        return Err("response ended with an incomplete UTF-8 sequence".to_string());
    }
    Ok(decoded.into_bytes())
}

#[derive(Debug, PartialEq, Eq)]
enum ParsedResponse {
    Succeeded { actual_model: Option<String> },
    Failed { error: String },
    Unknown { error: String },
}

fn classify_response_body(body: &[u8], fallback_model: &str) -> ParsedResponse {
    let Ok(text) = std::str::from_utf8(body) else {
        return ParsedResponse::Unknown {
            error: "response body was not valid UTF-8".to_string(),
        };
    };

    let mut buffer = text.to_string();
    let mut completed = false;
    let mut actual_model = None;
    let mut failure = None;
    let mut saw_json = false;

    while let Some(block) = take_sse_block(&mut buffer) {
        inspect_sse_block(
            &block,
            &mut completed,
            &mut actual_model,
            &mut failure,
            &mut saw_json,
        );
    }
    if !buffer.trim().is_empty() {
        inspect_sse_block(
            &buffer,
            &mut completed,
            &mut actual_model,
            &mut failure,
            &mut saw_json,
        );
    }

    if let Some(error) = failure {
        return ParsedResponse::Failed { error };
    }
    if completed {
        return ParsedResponse::Succeeded {
            actual_model: actual_model.or_else(|| Some(fallback_model.to_string())),
        };
    }
    if saw_json {
        return ParsedResponse::Unknown {
            error: "response ended without response.completed".to_string(),
        };
    }
    ParsedResponse::Unknown {
        error: "response contained no parseable completion event".to_string(),
    }
}

fn inspect_sse_block(
    block: &str,
    completed: &mut bool,
    actual_model: &mut Option<String>,
    failure: &mut Option<String>,
    saw_json: &mut bool,
) {
    let mut event_name = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = strip_sse_field(line, "event") {
            event_name = Some(event.trim());
        } else if let Some(data) = strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }
    let raw = if data_lines.is_empty() {
        block.trim()
    } else {
        &data_lines.join("\n")
    };
    if raw.is_empty() || raw == "[DONE]" {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return;
    };
    *saw_json = true;

    let event = event_name
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or_default();
    let response = value.get("response").unwrap_or(&value);
    if let Some(model) = response
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        *actual_model = Some(model.to_string());
    }

    if event == "response.completed"
        || response.get("status").and_then(Value::as_str) == Some("completed")
    {
        *completed = true;
    }
    if event == "response.failed"
        || event == "response.cancelled"
        || matches!(
            response.get("status").and_then(Value::as_str),
            Some("failed" | "cancelled")
        )
        || response.get("error").is_some_and(|error| !error.is_null())
    {
        let error = response.get("error").unwrap_or(response);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("Codex response failed");
        *failure = Some(message.to_string());
    }
}

fn failed_result(
    category: &str,
    error: &str,
    secrets: &[String],
    actual_model: Option<String>,
) -> ActivationRunResult {
    ActivationRunResult {
        status: "failed",
        exit_code: None,
        error: Some(sanitize_http_error(
            &format!("{category}: {error}"),
            secrets,
        )),
        actual_model,
    }
}

fn unknown_result(error: &str, secrets: &[String]) -> ActivationRunResult {
    ActivationRunResult {
        status: "unknown",
        exit_code: None,
        error: Some(sanitize_http_error(error, secrets)),
        actual_model: None,
    }
}

fn sanitize_http_error(error: &str, secrets: &[String]) -> String {
    crate::redact_known_secrets_strict(error, secrets)
        .chars()
        .take(512)
        .collect()
}

fn truncate_text(text: &str) -> String {
    text.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::Json,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
        Router,
    };

    async fn fake_success(headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
        let valid = headers
            .get("chatgpt-account-id")
            .and_then(|value| value.to_str().ok())
            == Some("workspace-id")
            && body["model"] == "gpt-test"
            && body["input"][0].get("type").is_none()
            && body["input"][0]["content"][0]["text"] == ACTIVATION_PROMPT;
        let protocol_fields_valid = body["store"] == false
            && body["instructions"] == ""
            && body["tools"] == json!([])
            && body["parallel_tool_calls"] == true
            && body["include"] == json!(["reasoning.encrypted_content"]);
        if !valid || !protocol_fields_valid {
            return (
                StatusCode::BAD_REQUEST,
                [("content-type", "application/json")],
                "{\"error\":\"invalid activation request\"}",
            );
        }
        (
            StatusCode::OK,
            [("content-type", "text/event-stream")],
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-test\"}}\n\n",
        )
    }

    async fn fake_auth_failure() -> impl IntoResponse {
        (
            StatusCode::UNAUTHORIZED,
            [("content-type", "application/json")],
            "{\"error\":\"unauthorized\"}",
        )
    }

    #[test]
    fn request_builder_uses_explicit_model_and_low_only_when_requested() {
        let client = reqwest::Client::new();
        let request = build_activation_request(
            &client,
            "access-token",
            "workspace-id",
            &ActivationModel {
                model: "gpt-test".to_string(),
                reasoning_effort: Some("low".to_string()),
            },
        )
        .build()
        .unwrap();
        assert_eq!(request.url().path(), "/backend-api/codex/responses");
        assert_eq!(request.headers()["chatgpt-account-id"], "workspace-id");
        assert_eq!(request.headers()["originator"], CODEX_OAUTH_ORIGINATOR);
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["input"][0]["content"][0]["text"], ACTIVATION_PROMPT);
        assert!(body["input"][0].get("type").is_none());
        assert_eq!(body["instructions"], "");
        assert_eq!(body["tools"], json!([]));
        assert_eq!(body["parallel_tool_calls"], true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "low");
    }

    #[test]
    fn response_parser_requires_completed_event() {
        let body = b"event: response.in_progress\ndata: {\"type\":\"response.in_progress\"}\n\n";
        assert!(matches!(
            classify_response_body(body, "gpt-test"),
            ParsedResponse::Unknown { .. }
        ));
    }

    #[test]
    fn response_parser_accepts_completed_event_and_model() {
        let body = br#"event: response.completed
data: {"type":"response.completed","response":{"status":"completed","model":"gpt-test"}}

"#;
        assert_eq!(
            classify_response_body(body, "fallback"),
            ParsedResponse::Succeeded {
                actual_model: Some("gpt-test".to_string())
            }
        );
    }

    #[test]
    fn response_parser_classifies_failed_event() {
        let body = br#"event: response.failed
data: {"type":"response.failed","response":{"status":"failed","error":{"message":"quota"}}}

"#;
        assert_eq!(
            classify_response_body(body, "gpt-test"),
            ParsedResponse::Failed {
                error: "quota".to_string()
            }
        );
    }

    #[tokio::test]
    async fn fake_server_receives_one_successful_generation_request() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/responses", post(fake_success));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let result = run_activation_http(
            &reqwest::Client::new(),
            &format!("http://{address}/responses"),
            "access-token",
            "workspace-id",
            &ActivationModel {
                model: "gpt-test".to_string(),
                reasoning_effort: None,
            },
        )
        .await;
        server.abort();

        assert_eq!(result.status, "succeeded");
        assert_eq!(result.actual_model.as_deref(), Some("gpt-test"));
    }

    #[tokio::test]
    async fn fake_server_auth_failure_does_not_retry() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/responses", post(fake_auth_failure));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let result = run_activation_http(
            &reqwest::Client::new(),
            &format!("http://{address}/responses"),
            "access-token",
            "workspace-id",
            &ActivationModel {
                model: "gpt-test".to_string(),
                reasoning_effort: None,
            },
        )
        .await;
        server.abort();

        assert_eq!(result.status, "failed");
        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("authentication")));
    }
}
