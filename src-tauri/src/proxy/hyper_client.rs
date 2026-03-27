//! Hyper-based HTTP client for proxy forwarding
//!
//! Uses hyper directly (instead of reqwest) to support:
//! - `preserve_header_case(true)` — keeps original header name casing
//! - Header order preservation via `HeaderCaseMap` extension transfer
//!
//! Falls back to reqwest when an upstream proxy (HTTP/SOCKS5) is configured,
//! since hyper-util's legacy client doesn't natively support proxy tunneling.

use super::ProxyError;
use bytes::Bytes;
use futures::stream::Stream;
use http_body_util::BodyExt;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::sync::OnceLock;

type HyperClient = Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    http_body_util::Full<Bytes>,
>;

/// Lazily-initialized hyper client with header-case preservation enabled.
fn global_hyper_client() -> &'static HyperClient {
    static CLIENT: OnceLock<HyperClient> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let connector = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build();

        Client::builder(TokioExecutor::new())
            .http1_preserve_header_case(true)
            .build(connector)
    })
}

/// Unified response wrapper that can hold either a hyper or reqwest response.
///
/// The hyper variant is used for the main (direct) path with header-case preservation.
/// The reqwest variant is the fallback when an upstream HTTP/SOCKS5 proxy is configured.
pub enum ProxyResponse {
    Hyper(hyper::Response<hyper::body::Incoming>),
    Reqwest(reqwest::Response),
}

impl ProxyResponse {
    pub fn status(&self) -> http::StatusCode {
        match self {
            Self::Hyper(r) => r.status(),
            Self::Reqwest(r) => r.status(),
        }
    }

    pub fn headers(&self) -> &http::HeaderMap {
        match self {
            Self::Hyper(r) => r.headers(),
            Self::Reqwest(r) => r.headers(),
        }
    }

    /// Shortcut: extract `content-type` header value as `&str`.
    pub fn content_type(&self) -> Option<&str> {
        self.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
    }

    /// Check if the response is an SSE stream.
    pub fn is_sse(&self) -> bool {
        self.content_type()
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false)
    }

    /// Consume the response and collect the full body into `Bytes`.
    pub async fn bytes(self) -> Result<Bytes, ProxyError> {
        match self {
            Self::Hyper(r) => {
                let collected = r.into_body().collect().await.map_err(|e| {
                    ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
                })?;
                Ok(collected.to_bytes())
            }
            Self::Reqwest(r) => r.bytes().await.map_err(|e| {
                ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
            }),
        }
    }

    /// Consume the response and return a byte-chunk stream (for SSE pass-through).
    pub fn bytes_stream(self) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
        use futures::StreamExt;

        match self {
            Self::Hyper(r) => {
                let body = r.into_body();
                let stream = futures::stream::unfold(body, |mut body| async {
                    match body.frame().await {
                        Some(Ok(frame)) => {
                            if let Ok(data) = frame.into_data() {
                                if data.is_empty() {
                                    Some((Ok(Bytes::new()), body))
                                } else {
                                    Some((Ok(data), body))
                                }
                            } else {
                                Some((Ok(Bytes::new()), body))
                            }
                        }
                        Some(Err(e)) => Some((Err(std::io::Error::other(e.to_string())), body)),
                        None => None,
                    }
                })
                .filter(|result| {
                    futures::future::ready(!matches!(result, Ok(ref b) if b.is_empty()))
                });
                Box::pin(stream)
                    as std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>
            }
            Self::Reqwest(r) => {
                let stream = r
                    .bytes_stream()
                    .map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));
                Box::pin(stream)
            }
        }
    }
}

/// Send an HTTP request via the global hyper client (with header-case preservation).
///
/// `original_extensions` should carry the `HeaderCaseMap` populated by the
/// server-side hyper parser (via `preserve_header_case(true)`).
/// The hyper client will read it back and serialise headers with the original casing.
pub async fn send_request(
    uri: http::Uri,
    method: http::Method,
    headers: http::HeaderMap,
    original_extensions: http::Extensions,
    body: Vec<u8>,
    timeout: std::time::Duration,
) -> Result<ProxyResponse, ProxyError> {
    let mut req = http::Request::builder()
        .method(method)
        .uri(&uri)
        .body(http_body_util::Full::new(Bytes::from(body)))
        .map_err(|e| ProxyError::ForwardFailed(format!("Failed to build request: {e}")))?;

    // Set headers (order is preserved by http::HeaderMap insertion order)
    *req.headers_mut() = headers;

    // Transfer extensions from the incoming request — this carries the internal
    // `HeaderCaseMap` that tells the hyper client how to case each header name.
    // Debug: check extension count before transfer
    log::debug!("[HyperClient] Transferring extensions to outgoing request (uri={uri})");
    *req.extensions_mut() = original_extensions;

    let client = global_hyper_client();

    let resp = tokio::time::timeout(timeout, client.request(req))
        .await
        .map_err(|_| ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs())))?
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("connect") {
                ProxyError::ForwardFailed(format!("连接失败: {e}"))
            } else {
                ProxyError::ForwardFailed(e.to_string())
            }
        })?;

    Ok(ProxyResponse::Hyper(resp))
}
