//! Isolated, single dispatch transport for the automatic activation request.
//!
//! The caller supplies a client and endpoint so tests can use a loopback fake
//! server. Production wiring can reuse the application's proxy configured
//! client, while this module itself never touches OAuth files or providers.

use crate::proxy::providers::codex_oauth_auth::{
    CODEX_OAUTH_CLIENT_VERSION, CODEX_OAUTH_ORIGINATOR,
};
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;

pub(crate) const ACTIVATION_MODEL: &str = "gpt-6-luna";
pub(crate) const ACTIVATION_REASONING_EFFORT: &str = "low";
pub(crate) const ACTIVATION_INPUT: &str = "hello";
pub(crate) const ACTIVATION_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationHttpStatus {
    Completed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivationHttpResult {
    pub status: ActivationHttpStatus,
    pub actual_model: Option<String>,
    pub error: Option<String>,
}

pub(crate) fn build_activation_request(
    client: &Client,
    endpoint: &str,
    token: &str,
    workspace: &str,
) -> RequestBuilder {
    let body = json!({
        "model": ACTIVATION_MODEL,
        "input": [{
            "role": "user",
            "content": [{"type": "input_text", "text": ACTIVATION_INPUT}]
        }],
        "instructions": "",
        "tools": [],
        "parallel_tool_calls": true,
        "include": ["reasoning.encrypted_content"],
        "store": false,
        "stream": true,
        "reasoning": {"effort": ACTIVATION_REASONING_EFFORT}
    });
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

/// Send exactly one POST. The supplied client should have redirects and retry
/// policies disabled by its factory; this function has no application retry.
pub(crate) async fn send_activation_once(
    client: &Client,
    endpoint: &str,
    token: &str,
    workspace: &str,
) -> ActivationHttpResult {
    let result = tokio::time::timeout(
        ACTIVATION_TIMEOUT,
        send_activation_inner(client, endpoint, token, workspace),
    )
    .await;
    match result {
        Ok(result) => result,
        Err(_) => unknown("activation response timed out", &[token, workspace]),
    }
}

async fn send_activation_inner(
    client: &Client,
    endpoint: &str,
    token: &str,
    workspace: &str,
) -> ActivationHttpResult {
    let response = match build_activation_request(client, endpoint, token, workspace)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return unknown(&format!("transport error: {error}"), &[token, workspace]),
    };
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return failed(&format!("HTTP {status}"), &[token, workspace]);
    }
    if !status.is_success() {
        let body = read_bounded(response).await.unwrap_or_default();
        let message = format!("HTTP {status}: {body}");
        return if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            unknown(&message, &[token, workspace])
        } else {
            failed(&message, &[token, workspace])
        };
    }
    match read_sse_until_terminal(response, &[token, workspace]).await {
        Ok(result) => result,
        Err(error) => unknown(&error, &[token, workspace]),
    }
}

async fn read_bounded(response: reqwest::Response) -> Result<String, String> {
    read_bounded_stream(response).await
}

async fn read_bounded_stream(mut response: reqwest::Response) -> Result<String, String> {
    let mut output = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if output.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("response exceeded size limit".to_string());
        }
        output.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}

async fn read_sse_until_terminal(
    mut response: reqwest::Response,
    secrets: &[&str],
) -> Result<ActivationHttpResult, String> {
    let mut buffer = String::new();
    let mut utf8_remainder = Vec::new();
    let mut total_bytes = 0usize;
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        total_bytes = total_bytes.saturating_add(chunk.len());
        if total_bytes > MAX_RESPONSE_BYTES {
            return Err("response exceeded size limit".to_string());
        }
        append_utf8_safe(&mut buffer, &mut utf8_remainder, &chunk);
        let mut terminal = None;
        while let Some(block) = take_sse_block(&mut buffer) {
            if let Some(result) = classify_sse_block(&block, secrets) {
                terminal = Some(match terminal.take() {
                    Some(previous) => merge_terminal_results(previous, result, secrets),
                    None => result,
                });
            }
        }
        if let Some(result) = terminal {
            return Ok(result);
        }
    }
    Ok(unknown("response ended without a terminal event", secrets))
}

pub(crate) fn classify_sse(body: &str) -> ActivationHttpResult {
    classify_sse_with_secrets(body, &[])
}

fn classify_sse_with_secrets(body: &str, secrets: &[&str]) -> ActivationHttpResult {
    let mut buffer = body.to_string();
    let mut terminal = None;
    while let Some(block) = take_sse_block(&mut buffer) {
        if let Some(result) = classify_sse_block(&block, secrets) {
            terminal = Some(match terminal.take() {
                Some(previous) => merge_terminal_results(previous, result, secrets),
                None => result,
            });
        }
    }
    terminal.unwrap_or_else(|| unknown("response ended without a terminal event", secrets))
}

fn merge_terminal_results(
    previous: ActivationHttpResult,
    next: ActivationHttpResult,
    secrets: &[&str],
) -> ActivationHttpResult {
    if previous.status != next.status {
        return failed(
            &format!(
                "conflicting terminal events: {}",
                next.error
                    .unwrap_or_else(|| "conflicting terminal event".to_string())
            ),
            secrets,
        );
    }
    previous
}

fn classify_sse_block(block: &str, secrets: &[&str]) -> Option<ActivationHttpResult> {
    let mut event = None;
    let mut data = String::new();
    for line in block.lines() {
        if let Some(value) = strip_sse_field(line, "event") {
            event = Some(value.trim());
        } else if let Some(value) = strip_sse_field(line, "data") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value);
        }
    }
    let parsed = serde_json::from_str::<Value>(&data).ok();
    let event_name = event.or_else(|| {
        parsed
            .as_ref()
            .and_then(|value| value.get("type").and_then(Value::as_str))
    });
    match event_name {
        Some("response.completed") => {
            let payload_type = parsed
                .as_ref()
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str);
            let response_status = parsed
                .as_ref()
                .and_then(|value| value.pointer("/response/status"))
                .and_then(Value::as_str);
            let response_error = parsed
                .as_ref()
                .and_then(|value| value.pointer("/response/error"));
            if payload_type.is_some_and(|value| value != "response.completed")
                || response_status.is_some_and(|value| value != "completed")
                || response_error.is_some_and(|value| !value.is_null())
            {
                return Some(failed(
                    &parsed.as_ref().and_then(error_message).unwrap_or_else(|| {
                        "activation completion payload was not successful".to_string()
                    }),
                    secrets,
                ));
            }
            let actual_model = parsed.as_ref().and_then(|value| {
                value
                    .pointer("/response/model")
                    .or_else(|| value.get("model"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })?;
            if actual_model != ACTIVATION_MODEL {
                return Some(failed("server returned an unexpected model", secrets));
            }
            Some(ActivationHttpResult {
                status: ActivationHttpStatus::Completed,
                actual_model: Some(actual_model),
                error: None,
            })
        }
        Some("response.failed" | "response.cancelled" | "response.incomplete" | "error") => {
            Some(failed(
                &parsed
                    .as_ref()
                    .and_then(error_message)
                    .unwrap_or_else(|| "activation failed".to_string()),
                secrets,
            ))
        }
        _ => None,
    }
}

fn error_message(value: &Value) -> Option<String> {
    value
        .pointer("/response/error/message")
        .or_else(|| value.pointer("/error/message"))
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .pointer("/response/error")
                .or_else(|| value.get("error"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn failed(error: &str, secrets: &[&str]) -> ActivationHttpResult {
    ActivationHttpResult {
        status: ActivationHttpStatus::Failed,
        actual_model: None,
        error: Some(sanitize(error, secrets)),
    }
}

fn unknown(error: &str, secrets: &[&str]) -> ActivationHttpResult {
    ActivationHttpResult {
        status: ActivationHttpStatus::Unknown,
        actual_model: None,
        error: Some(sanitize(error, secrets)),
    }
}

fn sanitize(error: &str, secrets: &[&str]) -> String {
    let mut output = error.to_string();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        output = output.replace(secret, "[REDACTED]");
    }
    output.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::State,
        http::header,
        response::{IntoResponse, Response},
        routing::post,
        Router,
    };
    use bytes::Bytes;
    use futures::StreamExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn request_is_fixed_to_luna_low_hello_and_stateless() {
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let request = build_activation_request(
            &client,
            "http://127.0.0.1:1",
            "fake-token",
            "fake-workspace",
        )
        .build()
        .unwrap();
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["model"], ACTIVATION_MODEL);
        assert_eq!(body["reasoning"]["effort"], ACTIVATION_REASONING_EFFORT);
        assert_eq!(body["input"][0]["content"][0]["text"], ACTIVATION_INPUT);
        assert_eq!(body["tools"], json!([]));
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(
            request.headers().get("ChatGPT-Account-Id").unwrap(),
            "fake-workspace"
        );
    }

    #[test]
    fn only_completed_terminal_event_is_success() {
        let completed = classify_sse("event: response.output_text.done\ndata: {}\n\nevent: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\"}}\n\n");
        assert_eq!(completed.status, ActivationHttpStatus::Completed);
        assert_eq!(
            classify_sse(
                "event: response.completed\ndata: {\"response\":{\"model\":\"other\"}}\n\n"
            )
            .status,
            ActivationHttpStatus::Failed
        );
        assert_eq!(
            classify_sse("data: [DONE]\n\n").status,
            ActivationHttpStatus::Unknown
        );
        assert_eq!(
            classify_sse("event: response.failed\ndata: {\"error\":{}}\n\n").status,
            ActivationHttpStatus::Failed
        );
    }

    #[test]
    fn errors_redact_tokens_and_are_bounded() {
        let result = failed("token=secret-token", &["secret-token"]);
        assert!(!result.error.unwrap().contains("secret-token"));
        assert_eq!(sanitize(&"x".repeat(600), &[]).len(), 512);
    }

    #[tokio::test]
    async fn fake_server_proves_redirect_and_server_error_never_replay_post() {
        async fn redirect(count: State<Arc<AtomicUsize>>) -> axum::response::Response {
            count.fetch_add(1, Ordering::SeqCst);
            (
                StatusCode::TEMPORARY_REDIRECT,
                [(header::LOCATION, "/target")],
            )
                .into_response()
        }
        async fn target(count: State<Arc<AtomicUsize>>) -> axum::response::Response {
            count.fetch_add(100, Ordering::SeqCst);
            StatusCode::OK.into_response()
        }
        async fn server_error(count: State<Arc<AtomicUsize>>) -> axum::response::Response {
            count.fetch_add(1, Ordering::SeqCst);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }

        let count = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/redirect", post(redirect))
            .route("/target", post(target))
            .route("/server-error", post(server_error))
            .with_state(count.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();

        let redirect_result = send_activation_once(
            &client,
            &format!("http://{address}/redirect"),
            "fake-token",
            "fake-workspace",
        )
        .await;
        assert_eq!(redirect_result.status, ActivationHttpStatus::Failed);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "redirect target received no replay"
        );

        let failure_result = send_activation_once(
            &client,
            &format!("http://{address}/server-error"),
            "fake-token",
            "fake-workspace",
        )
        .await;
        assert_eq!(failure_result.status, ActivationHttpStatus::Unknown);
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "5xx produced one POST and no retry"
        );
        server.abort();
    }

    #[tokio::test]
    async fn production_factory_rejects_invalid_proxy_without_a_fallback_client() {
        assert!(crate::proxy::http_client::build_client_with_redirect_policy(
            Some("not a proxy"),
            true,
        )
        .is_err());
    }

    #[test]
    fn completion_payload_with_failed_status_is_not_success() {
        let result = classify_sse(
            "event: response.completed\ndata: {\"type\":\"response.failed\",\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"failed\",\"error\":{\"message\":\"synthetic failure\"}}}\n\n",
        );
        assert_eq!(result.status, ActivationHttpStatus::Failed);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("synthetic failure"));
    }

    #[tokio::test]
    async fn successful_http_sse_error_redacts_token_and_workspace() {
        async fn error_stream() -> axum::response::Response {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                "event: error\ndata: {\"error\":\"rejected FAKE_TOKEN in FAKE_WORKSPACE\"}\n\n",
            )
                .into_response()
        }
        let app = Router::new().route("/error", post(error_stream));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let result = send_activation_once(
            &client,
            &format!("http://{address}/error"),
            "FAKE_TOKEN",
            "FAKE_WORKSPACE",
        )
        .await;
        let error = result.error.unwrap_or_default();
        assert_eq!(result.status, ActivationHttpStatus::Failed);
        assert!(!error.contains("FAKE_TOKEN"));
        assert!(!error.contains("FAKE_WORKSPACE"));
        server.abort();
    }

    #[tokio::test]
    async fn complete_frame_finishes_before_http_body_eof() {
        async fn completed_pending() -> Response {
            let stream = futures::stream::once(async {
                Ok::<_, std::convert::Infallible>(Bytes::from_static(
                    b"event: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"completed\"}}\n\n",
                ))
            })
            .chain(futures::stream::pending());
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }
        let app = Router::new().route("/completed-pending", post(completed_pending));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            send_activation_once(
                &client,
                &format!("http://{address}/completed-pending"),
                "fake-token",
                "fake-workspace",
            ),
        )
        .await
        .expect("complete frame should not wait for body EOF");
        assert_eq!(result.status, ActivationHttpStatus::Completed);
        server.abort();
    }

    #[tokio::test]
    async fn failed_frame_finishes_before_http_body_eof() {
        async fn failed_pending() -> Response {
            let stream = futures::stream::once(async {
                Ok::<_, std::convert::Infallible>(Bytes::from_static(
                    b"event: response.failed\ndata: {\"error\":{\"message\":\"bad FAKE_TOKEN\"}}\n\n",
                ))
            })
            .chain(futures::stream::pending());
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }
        let app = Router::new().route("/failed-pending", post(failed_pending));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            send_activation_once(
                &client,
                &format!("http://{address}/failed-pending"),
                "FAKE_TOKEN",
                "fake-workspace",
            ),
        )
        .await
        .expect("failed frame should not wait for body EOF");
        assert_eq!(result.status, ActivationHttpStatus::Failed);
        assert!(!result.error.unwrap_or_default().contains("FAKE_TOKEN"));
        server.abort();
    }

    #[tokio::test]
    async fn production_stream_parser_handles_utf8_split_and_mixed_delimiters() {
        async fn split_stream() -> Response {
            let first = "event: response.output_text.delta\r\ndata: {\"text\":\"你";
            let second = "好\"}\n\r\nevent: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"completed\"}}\n\n";
            let first_bytes = first.as_bytes();
            let split = first_bytes.len() - "你".len();
            let stream = futures::stream::iter(vec![
                Ok::<_, std::convert::Infallible>(Bytes::copy_from_slice(
                    &first_bytes[..split + 1],
                )),
                Ok::<_, std::convert::Infallible>(Bytes::from(
                    [&first_bytes[split + 1..], second.as_bytes()].concat(),
                )),
            ]);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }
        let app = Router::new().route("/split", post(split_stream));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let result = send_activation_once(
            &client,
            &format!("http://{address}/split"),
            "fake-token",
            "fake-workspace",
        )
        .await;
        assert_eq!(result.status, ActivationHttpStatus::Completed);
        server.abort();
    }

    #[tokio::test]
    async fn fake_server_eof_after_an_unterminated_json_frame_is_unknown() {
        async fn incomplete() -> impl IntoResponse {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                "event: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"completed\"}}\n",
            )
        }
        let app = Router::new().route("/incomplete", post(incomplete));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            crate::proxy::http_client::build_client_with_redirect_policy(None, true).unwrap();
        let result = send_activation_once(
            &client,
            &format!("http://{address}/incomplete"),
            "fake-token",
            "fake-workspace",
        )
        .await;
        assert_eq!(result.status, ActivationHttpStatus::Unknown);
        server.abort();
    }

    #[test]
    fn eof_without_a_complete_sse_frame_is_unknown() {
        let result = classify_sse(
            "event: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"completed\"}}\n",
        );
        assert_eq!(result.status, ActivationHttpStatus::Unknown);
    }

    #[test]
    fn complete_and_failed_frames_in_one_batch_are_conservative() {
        let result = classify_sse(
            "event: response.completed\ndata: {\"response\":{\"model\":\"gpt-6-luna\",\"status\":\"completed\"}}\n\nevent: response.failed\ndata: {\"error\":{\"message\":\"later failure\"}}\n\n",
        );
        assert_eq!(result.status, ActivationHttpStatus::Failed);
    }
}
