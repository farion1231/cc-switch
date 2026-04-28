//! Hyper-based HTTP client for proxy forwarding
//!
//! Uses raw TCP/TLS writes to preserve exact original header name casing.
//! Supports HTTP CONNECT tunneling through upstream proxies.
//! Falls back to hyper-util Client (title-case headers) when raw write is not feasible.

use super::ProxyError;
use bytes::Bytes;
use futures::stream::Stream;
use http_body_util::BodyExt;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::sync::OnceLock;

/// Our own header case map: maps lowercase header name → original wire-casing bytes.
///
/// This is a backup mechanism independent of hyper's internal `HeaderCaseMap` (which is
/// `pub(crate)` and cannot be directly inspected or constructed from outside hyper).
///
/// Populated in `server.rs` by peeking at raw TCP bytes before hyper parses them.
/// Used in `send_request` to manually write headers with original casing when hyper's
/// own mechanism fails.
#[derive(Clone, Debug, Default)]
pub(crate) struct OriginalHeaderCases {
    /// Ordered list of (lowercase_name, original_wire_bytes) pairs.
    /// Multiple entries with the same name are allowed (for repeated headers).
    pub cases: Vec<(String, Vec<u8>)>,
}

impl OriginalHeaderCases {
    /// Parse raw HTTP request bytes (from TcpStream::peek) to extract original header casings.
    pub fn from_raw_bytes(buf: &[u8]) -> Self {
        let mut headers_buf = [httparse::EMPTY_HEADER; 128];
        let mut req = httparse::Request::new(&mut headers_buf);
        // We don't care if parsing is partial — we just want the header names we can get
        let _ = req.parse(buf);

        let mut cases = Vec::new();
        for header in req.headers.iter() {
            if header.name.is_empty() {
                break;
            }
            cases.push((
                header.name.to_ascii_lowercase(),
                header.name.as_bytes().to_vec(),
            ));
        }

        Self { cases }
    }
}

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
            .http1_title_case_headers(true)
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
    Buffered {
        status: http::StatusCode,
        headers: http::HeaderMap,
        body: Bytes,
    },
}

impl ProxyResponse {
    pub fn status(&self) -> http::StatusCode {
        match self {
            Self::Hyper(r) => r.status(),
            Self::Reqwest(r) => r.status(),
            Self::Buffered { status, .. } => *status,
        }
    }

    pub fn headers(&self) -> &http::HeaderMap {
        match self {
            Self::Hyper(r) => r.headers(),
            Self::Reqwest(r) => r.headers(),
            Self::Buffered { headers, .. } => headers,
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
            Self::Buffered { body, .. } => Ok(body),
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
            Self::Buffered { body, .. } => Box::pin(futures::stream::once(async move { Ok(body) })),
        }
    }
}

/// Send an HTTP request with header-case preservation.
///
/// Uses a two-tier strategy:
/// 1. Primary: raw HTTP/1.1 write via TLS stream with exact original header casing
///    (from `OriginalHeaderCases` captured by peek in server.rs), then hand off to
///    hyper for response parsing.
/// 2. Fallback: hyper-util Client with `title_case_headers(true)` when raw write
///    isn't feasible (e.g., missing original cases).
///
/// The caller is expected to include `Host` in the supplied `headers` at the
/// correct position.
///
/// `proxy_url`: optional upstream HTTP proxy URL (e.g. `http://127.0.0.1:7890`).
/// When set, the raw write path uses HTTP CONNECT tunneling through the proxy,
/// so header-case preservation works even when an upstream proxy is configured.
pub async fn send_request(
    uri: http::Uri,
    method: http::Method,
    headers: http::HeaderMap,
    original_extensions: http::Extensions,
    body: Vec<u8>,
    timeout: std::time::Duration,
    proxy_url: Option<&str>,
) -> Result<ProxyResponse, ProxyError> {
    // Extract our own OriginalHeaderCases if available
    let original_cases = original_extensions.get::<OriginalHeaderCases>().cloned();
    let has_cases = original_cases
        .as_ref()
        .map(|c| !c.cases.is_empty())
        .unwrap_or(false);

    log::debug!(
        "[HyperClient] Sending request: uri={uri}, header_count={}, \
         has_host={}, has_original_cases={has_cases}, proxy={:?}",
        headers.len(),
        headers.contains_key(http::header::HOST),
        proxy_url,
    );

    if has_cases {
        // Primary path: use raw write + hyper handshake for exact header casing
        let result = tokio::time::timeout(
            timeout,
            send_raw_request(
                &uri,
                &method,
                &headers,
                original_cases.as_ref().unwrap(),
                &body,
                proxy_url,
            ),
        )
        .await
        .map_err(|_| ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs())))?;

        match result {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if proxy_url.is_some() {
                    // Don't bypass configured proxy with direct connect fallback
                    return Err(e);
                }
                log::warn!("[HyperClient] Raw write failed, falling back to hyper-util: {e}");
                // Fall through to hyper-util Client
            }
        }
    }

    // Fallback: hyper-util Client (title-case headers, no proxy support)
    let mut req = http::Request::builder()
        .method(method)
        .uri(&uri)
        .body(http_body_util::Full::new(Bytes::from(body)))
        .map_err(|e| ProxyError::ForwardFailed(format!("Failed to build request: {e}")))?;

    *req.headers_mut() = headers;
    *req.extensions_mut() = original_extensions;

    let client = global_hyper_client();
    let resp = tokio::time::timeout(timeout, client.request(req))
        .await
        .map_err(|_| ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs())))?
        .map_err(|e| ProxyError::ForwardFailed(format!("上游请求失败: {e}")))?;

    Ok(ProxyResponse::Hyper(resp))
}

/// TCP or TLS stream returned by `connect_via_proxy`.
///
/// When the proxy URL uses `https://`, the connection to the proxy itself is
/// TLS-wrapped before sending the CONNECT request.  The enum lets
/// `send_raw_request` work with either variant generically.
enum ProxyStream {
    Tcp(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for ProxyStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for ProxyStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Send request via raw TCP/TLS with exact original header casing.
///
/// When `proxy_url` is provided, establishes an HTTP CONNECT tunnel through
/// the proxy first, then performs TLS + raw write through the tunnel.
/// This preserves header casing even when an upstream proxy is configured.
async fn send_raw_request(
    uri: &http::Uri,
    method: &http::Method,
    headers: &http::HeaderMap,
    original_cases: &OriginalHeaderCases,
    body: &[u8],
    proxy_url: Option<&str>,
) -> Result<ProxyResponse, ProxyError> {
    use tokio::io::AsyncWriteExt;

    let scheme = uri.scheme_str().unwrap_or("https");
    let host = uri
        .host()
        .ok_or_else(|| ProxyError::ForwardFailed("URI has no host".into()))?;
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    // Build raw HTTP request bytes
    let raw = build_raw_request(method, path_and_query, headers, original_cases, body);

    // Establish TCP connection — either direct or through HTTP CONNECT proxy
    let stream = if let Some(proxy) = proxy_url {
        connect_via_proxy(proxy, host, port).await?
    } else {
        ProxyStream::Tcp(
            tokio::net::TcpStream::connect((host, port))
                .await
                .map_err(|e| ProxyError::ForwardFailed(format!("TCP connect failed: {e}")))?,
        )
    };

    if scheme == "https" {
        let tls_connector = global_tls_connector();
        let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
            .map_err(|e| ProxyError::ForwardFailed(format!("Invalid server name: {e}")))?;
        let mut tls_stream = tls_connector
            .connect(server_name, stream)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("TLS handshake failed: {e}")))?;

        tls_stream
            .write_all(&raw)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("Write failed: {e}")))?;
        tls_stream
            .flush()
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("Flush failed: {e}")))?;

        let filtered = WriteFilter::new(tls_stream);
        do_hyper_response(filtered, method.clone()).await
    } else {
        let mut stream = stream;
        stream
            .write_all(&raw)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("Write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("Flush failed: {e}")))?;

        let filtered = WriteFilter::new(stream);
        do_hyper_response(filtered, method.clone()).await
    }
}

/// Establish a connection through an HTTP CONNECT proxy tunnel.
///
/// 1. Connect TCP to the proxy server (TLS-wrapped when `https://` proxy)
/// 2. Send `CONNECT host:port HTTP/1.1` with optional `Proxy-Authorization`
/// 3. Read the proxy's 200 response (407 → `AuthError`)
/// 4. Return the tunneled stream (ready for target TLS handshake + raw write)
async fn connect_via_proxy(
    proxy_url: &str,
    target_host: &str,
    target_port: u16,
) -> Result<ProxyStream, ProxyError> {
    use base64::Engine;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let parsed = url::Url::parse(proxy_url)
        .map_err(|e| ProxyError::ForwardFailed(format!("Invalid proxy URL: {e}")))?;

    let proxy_host = parsed
        .host_str()
        .ok_or_else(|| ProxyError::ForwardFailed("Proxy URL has no host".into()))?;
    let proxy_port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });

    // Build Proxy-Authorization header if credentials are present
    let proxy_auth = if !parsed.username().is_empty() {
        let password = parsed.password().unwrap_or("");
        let credentials = format!("{}:{}", parsed.username(), password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        Some(format!("Proxy-Authorization: Basic {encoded}\r\n"))
    } else {
        None
    };

    // Connect to the proxy
    let tcp = tokio::net::TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Proxy TCP connect failed: {e}")))?;

    // Wrap with TLS if the proxy URL uses https://
    let mut stream: ProxyStream = if parsed.scheme() == "https" {
        let tls_connector = global_tls_connector();
        let server_name = rustls::pki_types::ServerName::try_from(proxy_host.to_string())
            .map_err(|e| ProxyError::ForwardFailed(format!("Invalid proxy server name: {e}")))?;
        let tls_stream = tls_connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("Proxy TLS handshake failed: {e}")))?;
        ProxyStream::Tls(Box::new(tls_stream))
    } else {
        ProxyStream::Tcp(tcp)
    };

    // Send CONNECT request
    let mut connect_req = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\n\
         Host: {target_host}:{target_port}\r\n"
    );
    if let Some(auth) = &proxy_auth {
        connect_req.push_str(auth);
    }
    connect_req.push_str("\r\n");

    stream
        .write_all(connect_req.as_bytes())
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT write failed: {e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT flush failed: {e}")))?;

    // Read the proxy's response status line
    let mut reader = BufReader::new(&mut stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT read failed: {e}")))?;

    // Expect "HTTP/1.1 200 ..." or "HTTP/1.0 200 ..."
    if !status_line.contains(" 200 ") {
        if status_line.contains(" 407 ") {
            return Err(ProxyError::AuthError(format!(
                "Proxy authentication required (407): {}",
                status_line.trim()
            )));
        }
        return Err(ProxyError::ForwardFailed(format!(
            "Proxy CONNECT rejected: {}",
            status_line.trim()
        )));
    }

    // Drain remaining response headers (until empty line)
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT header read: {e}")))?;
        if line.trim().is_empty() {
            break;
        }
    }
    // BufReader might have buffered data; drop it to get raw stream back.
    // Since CONNECT response is headers-only (no body), and we read until \r\n\r\n,
    // the BufReader buffer should be empty at this point.
    drop(reader);

    log::debug!(
        "[HyperClient] CONNECT tunnel established via {proxy_host}:{proxy_port} -> {target_host}:{target_port}"
    );

    Ok(stream)
}

/// Lazily-initialized TLS connector for raw connections.
///
/// Loads both webpki roots AND native system certificates so that
/// proxy MITM CAs (e.g. Clash, mitmproxy) installed in the system
/// keychain are trusted through the CONNECT tunnel.
fn global_tls_connector() -> &'static tokio_rustls::TlsConnector {
    static CONNECTOR: OnceLock<tokio_rustls::TlsConnector> = OnceLock::new();
    CONNECTOR.get_or_init(|| {
        let mut root_store = rustls::RootCertStore::empty();
        // Baseline: Mozilla/webpki roots
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // Native system certs (includes user-installed proxy CAs)
        let native = rustls_native_certs::load_native_certs();
        let (added, _errors) = root_store.add_parsable_certificates(native.certs);
        log::debug!("[HyperClient] TLS root store: webpki + {added} native certs");
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
    })
}

/// Build raw HTTP/1.1 request bytes with original header casing.
fn build_raw_request(
    method: &http::Method,
    path_and_query: &str,
    headers: &http::HeaderMap,
    original_cases: &OriginalHeaderCases,
    body: &[u8],
) -> Vec<u8> {
    let mut raw = Vec::with_capacity(4096 + body.len());

    // Request line
    raw.extend_from_slice(method.as_str().as_bytes());
    raw.extend_from_slice(b" ");
    raw.extend_from_slice(path_and_query.as_bytes());
    raw.extend_from_slice(b" HTTP/1.1\r\n");

    // Headers with original casing, emitted in original wire order.
    //
    // Strategy:
    // 1. Walk `original_cases.cases` in order — this preserves the exact
    //    header sequence the client sent.  For each entry, emit the stored
    //    original-casing name plus the current value from `headers` (the
    //    proxy may have rewritten the value, e.g. Authorization).
    //    Repeated headers with the same name are handled by tracking a
    //    per-name value cursor so we step through `get_all()` in order.
    // 2. After the original headers, append any headers that exist in
    //    `headers` but were not present in the original request (i.e. added
    //    by the proxy).  These are emitted in lowercase.
    //
    // This replaces the old `for name in headers.keys()` loop which iterated
    // in hash-map order, destroying the original header sequence.
    let mut emitted: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(original_cases.cases.len());
    // Per-name cursor: how many values we have already emitted for each name.
    let mut value_cursor: std::collections::HashMap<String, usize> =
        std::collections::HashMap::with_capacity(original_cases.cases.len());

    for (lower_name, orig_name_bytes) in &original_cases.cases {
        if let Ok(header_name) = http::header::HeaderName::from_bytes(lower_name.as_bytes()) {
            let all_values: Vec<_> = headers.get_all(&header_name).iter().collect();
            let cursor = value_cursor.entry(lower_name.clone()).or_insert(0);
            if let Some(value) = all_values.get(*cursor) {
                raw.extend_from_slice(orig_name_bytes);
                raw.extend_from_slice(b": ");
                raw.extend_from_slice(value.as_bytes());
                raw.extend_from_slice(b"\r\n");
                *cursor += 1;
                emitted.insert(lower_name.clone());
            }
        }
    }

    // Append proxy-added headers (not present in the original request).
    for name in headers.keys() {
        let lower = name.as_str().to_ascii_lowercase();
        if !emitted.contains(&lower) {
            for value in headers.get_all(name) {
                raw.extend_from_slice(name.as_str().as_bytes());
                raw.extend_from_slice(b": ");
                raw.extend_from_slice(value.as_bytes());
                raw.extend_from_slice(b"\r\n");
            }
            emitted.insert(lower);
        }
    }

    // Add Content-Length if not already present
    if !headers.contains_key(http::header::CONTENT_LENGTH) {
        raw.extend_from_slice(b"Content-Length: ");
        raw.extend_from_slice(body.len().to_string().as_bytes());
        raw.extend_from_slice(b"\r\n");
    }

    // End of headers + body
    raw.extend_from_slice(b"\r\n");
    raw.extend_from_slice(body);

    raw
}

/// Use hyper's low-level client to parse the response on a stream where we've
/// already written the request.
///
/// `WriteFilter` discards any writes from hyper (it would try to send its own
/// request encoding), while passing reads through transparently.
async fn do_hyper_response<S>(
    stream: WriteFilter<S>,
    method: http::Method,
) -> Result<ProxyResponse, ProxyError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let io = hyper_util::rt::TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
        .preserve_header_case(true)
        .handshake::<_, http_body_util::Full<Bytes>>(io)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Handshake failed: {e}")))?;

    // Spawn the connection driver (reads responses from the stream)
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            log::debug!("[HyperClient] raw conn driver error: {e}");
        }
    });

    // Send a dummy request through hyper — hyper will encode this and try to write it,
    // but WriteFilter discards all writes. Hyper will then read the response from the stream.
    let dummy_req = http::Request::builder()
        .method(method)
        .uri("/")
        .body(http_body_util::Full::new(Bytes::new()))
        .map_err(|e| ProxyError::ForwardFailed(format!("Build dummy request: {e}")))?;

    let resp = sender
        .send_request(dummy_req)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Response parse failed: {e}")))?;

    Ok(ProxyResponse::Hyper(resp))
}

/// A stream wrapper that discards all writes but passes reads through.
///
/// This lets hyper's connection driver think it sent a request (its encoded bytes
/// go to /dev/null), while correctly parsing the response that the upstream server
/// sends in reply to our raw-written request.
struct WriteFilter<S> {
    inner: S,
}

impl<S> WriteFilter<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for WriteFilter<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // Pass reads through to the underlying stream
        let inner = std::pin::Pin::new(&mut self.get_mut().inner);
        inner.poll_read(cx, buf)
    }
}

impl<S: Unpin> tokio::io::AsyncWrite for WriteFilter<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        // Discard all writes — pretend they succeeded
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
