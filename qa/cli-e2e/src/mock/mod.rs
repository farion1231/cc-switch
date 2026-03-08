pub mod webdav;

use anyhow::{Context, Result};
use async_stream::stream;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

pub use webdav::WebDavServer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Family {
    Anthropic,
    OpenAi,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum Profile {
    OkJson,
    OkStream,
    AuthError401,
    RateLimit429,
    ServerError500,
    Timeout,
    StreamAbort,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedRequest {
    family: Family,
    method: String,
    path: String,
    query: String,
    profile: Profile,
    headers: HashMap<String, String>,
    body: String,
}

#[derive(Clone)]
struct MockState {
    profiles: Arc<RwLock<HashMap<Family, Profile>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

pub struct MockServer {
    address: SocketAddr,
    state: MockState,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl MockServer {
    pub async fn start() -> Result<Self> {
        let profiles = Arc::new(RwLock::new(HashMap::from([
            (Family::Anthropic, Profile::OkJson),
            (Family::OpenAi, Profile::OkJson),
            (Family::Gemini, Profile::OkJson),
        ])));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = MockState { profiles, requests };

        let app = Router::new()
            .route("/health", get(|| async { Json(json!({ "ok": true })) }))
            .route("/anthropic/*path", any(handle_anthropic))
            .route("/openai/*path", any(handle_openai))
            .route("/gemini/*path", any(handle_gemini))
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind mock server")?;
        let address = listener
            .local_addr()
            .context("failed to inspect mock server address")?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Ok(Self {
            address,
            state,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn anthropic_base(&self) -> String {
        format!("http://{}/anthropic", self.address)
    }

    #[allow(dead_code)]
    pub fn openai_base(&self) -> String {
        format!("http://{}/openai", self.address)
    }

    #[allow(dead_code)]
    pub fn gemini_base(&self) -> String {
        format!("http://{}/gemini", self.address)
    }

    pub async fn set_profile(&self, family: Family, profile: Profile) {
        self.state.profiles.write().await.insert(family, profile);
    }

    #[allow(dead_code)]
    pub async fn reset_requests(&self) {
        self.state.requests.lock().await.clear();
    }

    pub async fn requests_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&*self.state.requests.lock().await).map_err(Into::into)
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle_anthropic(
    state: State<MockState>,
    path: Path<String>,
    query: Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle_family(Family::Anthropic, state, path, query, headers, body).await
}

async fn handle_openai(
    state: State<MockState>,
    path: Path<String>,
    query: Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle_family(Family::OpenAi, state, path, query, headers, body).await
}

async fn handle_gemini(
    state: State<MockState>,
    path: Path<String>,
    query: Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle_family(Family::Gemini, state, path, query, headers, body).await
}

async fn handle_family(
    family: Family,
    State(state): State<MockState>,
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let profile = state
        .profiles
        .read()
        .await
        .get(&family)
        .copied()
        .unwrap_or(Profile::OkJson);

    let headers_map = headers
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|text| (key.as_str().to_string(), text.to_string()))
        })
        .collect::<HashMap<_, _>>();

    state.requests.lock().await.push(RecordedRequest {
        family,
        method: "POST".to_string(),
        path: format!("/{}", path),
        query: serde_json::to_string(&query).unwrap_or_else(|_| "{}".to_string()),
        profile,
        headers: headers_map,
        body: String::from_utf8_lossy(&body).to_string(),
    });

    match profile {
        Profile::OkJson => ok_json_response(family),
        Profile::OkStream => ok_stream_response(family),
        Profile::AuthError401 => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "mock unauthorized" })),
        )
            .into_response(),
        Profile::RateLimit429 => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "mock rate limited" })),
        )
            .into_response(),
        Profile::ServerError500 => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "mock upstream error" })),
        )
            .into_response(),
        Profile::Timeout => {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({ "error": "mock timeout" })),
            )
                .into_response()
        }
        Profile::StreamAbort => stream_abort_response(family),
    }
}

fn ok_json_response(family: Family) -> Response {
    match family {
        Family::Anthropic => Json(json!({
            "id": "msg_mock_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-5",
            "content": [{ "type": "text", "text": "mock anthropic ok" }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 34
            }
        }))
        .into_response(),
        Family::OpenAi => Json(json!({
            "id": "resp_mock_123",
            "model": "gpt-4.1",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": { "role": "assistant", "content": "mock openai ok" }
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 34,
                "total_tokens": 46
            }
        }))
        .into_response(),
        Family::Gemini => Json(json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "mock gemini ok" }],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30
            }
        }))
        .into_response(),
    }
}

fn ok_stream_response(family: Family) -> Response {
    let lines: Vec<String> = match family {
        Family::Anthropic => vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n".to_string(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n".to_string(),
            "event: message_stop\ndata: {\"type\":\"message_stop\",\"usage\":{\"input_tokens\":10,\"output_tokens\":20}}\n\n".to_string(),
        ],
        _ => vec![
            "data: {\"id\":\"stream\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ],
    };
    let stream = stream! {
        for line in lines {
            yield Ok::<_, std::io::Error>(bytes::Bytes::from(line));
        }
    };
    (
        StatusCode::OK,
        [("content-type", "text/event-stream")],
        Body::from_stream(stream),
    )
        .into_response()
}

fn stream_abort_response(family: Family) -> Response {
    let first_chunk = match family {
        Family::Anthropic => {
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_abort\"}}\n\n"
        }
        _ => "data: {\"id\":\"stream-abort\"}\n\n",
    };
    let stream = stream! {
        yield Ok::<_, std::io::Error>(bytes::Bytes::from(first_chunk.to_string()));
    };
    (
        StatusCode::OK,
        [("content-type", "text/event-stream")],
        Body::from_stream(stream),
    )
        .into_response()
}
