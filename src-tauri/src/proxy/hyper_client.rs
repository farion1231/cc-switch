//! Hyper-based HTTP client for proxy forwarding
//!
//! Uses raw TCP/TLS writes to preserve exact original header name casing.
//! Supports HTTP CONNECT tunneling through upstream proxies.
//! Falls back to hyper-util Client (title-case headers) when raw write is not feasible.
//! Idle upstream connections are reused through `upstream_pool`.
//! A failed pooled exchange gets one fresh raw attempt when no bytes reached TCP
//! or when replacing the existing direct-path replay via hyper-util.

use super::upstream_pool::{
    self, PoolKey, PooledIo, PooledStream, ProxySocket, RawRequest, Taken, TrackedBody,
    UpstreamPool,
};
use super::ProxyError;
use bytes::Bytes;
use futures::{stream::Stream, StreamExt};
use http_body_util::BodyExt;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::future::Future;
use std::sync::OnceLock;

/// An async action to run immediately before a protocol converter emits a
/// successful terminal event. Errors, truncation, and client cancellation do
/// not run this hook.
pub type SuccessTerminalHook = Box<dyn FnOnce() -> futures::future::BoxFuture<'static, ()> + Send>;

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

/// 响应体读取上限（128 MiB）。正常非流式补全响应只有几十到几百 KiB；超过则视为
/// 上游异常或恶意 payload，直接拒绝，避免代理进程被超大响应体/压缩炸弹耗尽内存。
pub(crate) const MAX_RESPONSE_BODY_BYTES: usize = 128 * 1024 * 1024;

/// Unified response wrapper that can hold either a hyper or reqwest response.
///
/// The hyper variant is used for the main (direct) path with header-case preservation.
/// The reqwest variant is the fallback when an upstream HTTP/SOCKS5 proxy is configured.
pub enum ProxyResponse {
    Hyper(hyper::Response<TrackedBody>),
    Reqwest(reqwest::Response),
    Buffered {
        status: http::StatusCode,
        headers: http::HeaderMap,
        body: Bytes,
    },
    Streamed {
        status: http::StatusCode,
        headers: http::HeaderMap,
        stream: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    },
    WithSuccessTerminalHook {
        response: Box<ProxyResponse>,
        hook: SuccessTerminalHook,
    },
}

impl ProxyResponse {
    pub fn buffered(status: http::StatusCode, headers: http::HeaderMap, body: Bytes) -> Self {
        Self::Buffered {
            status,
            headers,
            body,
        }
    }

    pub fn streamed(
        status: http::StatusCode,
        headers: http::HeaderMap,
        stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    ) -> Self {
        Self::Streamed {
            status,
            headers,
            stream: Box::pin(stream),
        }
    }

    pub fn with_success_terminal_hook(self, hook: SuccessTerminalHook) -> Self {
        Self::WithSuccessTerminalHook {
            response: Box::new(self),
            hook,
        }
    }

    /// Remove the optional semantic completion callback before a response is
    /// consumed by a converter.
    pub fn take_success_terminal_hook(self) -> (Self, Option<SuccessTerminalHook>) {
        match self {
            Self::WithSuccessTerminalHook { response, hook } => (*response, Some(hook)),
            response => (response, None),
        }
    }

    pub fn status(&self) -> http::StatusCode {
        match self {
            Self::Hyper(r) => r.status(),
            Self::Reqwest(r) => r.status(),
            Self::Buffered { status, .. } | Self::Streamed { status, .. } => *status,
            Self::WithSuccessTerminalHook { response, .. } => response.status(),
        }
    }

    pub fn headers(&self) -> &http::HeaderMap {
        match self {
            Self::Hyper(r) => r.headers(),
            Self::Reqwest(r) => r.headers(),
            Self::Buffered { headers, .. } | Self::Streamed { headers, .. } => headers,
            Self::WithSuccessTerminalHook { response, .. } => response.headers(),
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

    /// Check whether the response explicitly declares a JSON media type.
    pub fn is_json(&self) -> bool {
        self.content_type()
            .map(|content_type| {
                let media_type = content_type
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                media_type == "application/json" || media_type.ends_with("+json")
            })
            .unwrap_or(false)
    }

    /// Consume the response and collect the full body into `Bytes`, aborting the
    /// read as soon as the accumulated body exceeds `max_bytes`.
    ///
    /// 所有变体都在累积过程中逐块检查、超限即断开（drop stream 中止上游连接），
    /// 而不是先收满再比较——否则超大明文 body 仍会完整进入内存，限制形同虚设。
    pub async fn bytes_with_limit(self, max_bytes: usize) -> Result<Bytes, ProxyError> {
        match self {
            Self::WithSuccessTerminalHook { response, .. } => {
                Box::pin(response.bytes_with_limit(max_bytes)).await
            }
            Self::Buffered { body, .. } => {
                // 调用方已把 body 完整缓冲，无法中途截停，只能事后比较
                if body.len() > max_bytes {
                    return Err(ProxyError::ResponseBodyTooLarge(body.len()));
                }
                Ok(body)
            }
            response => {
                // Hyper / Reqwest / Streamed 统一走逐块流式累积，超预算立即报错
                let mut stream = response.bytes_stream();
                let mut body = bytes::BytesMut::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| {
                        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
                    })?;
                    if body.len() + chunk.len() > max_bytes {
                        return Err(ProxyError::ResponseBodyTooLarge(body.len() + chunk.len()));
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(body.freeze())
            }
        }
    }

    /// Consume the response and return a byte-chunk stream (for SSE pass-through).
    pub fn bytes_stream(
        self,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> {
        use futures::StreamExt;

        match self {
            Self::WithSuccessTerminalHook { response, .. } => response.bytes_stream(),
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
                        Some(Err(e)) => Some((
                            Err(std::io::Error::other(super::error::describe_error_chain(
                                &e,
                            ))),
                            body,
                        )),
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
                let stream = r.bytes_stream().map(|r| {
                    r.map_err(|e| std::io::Error::other(super::error::describe_reqwest_error(e)))
                });
                Box::pin(stream)
            }
            Self::Buffered { body, .. } => Box::pin(futures::stream::once(async move { Ok(body) }))
                as std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
            Self::Streamed { stream, .. } => stream,
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
///
/// `log_display` is a caller-supplied, already-sanitized string used only for
/// logging; this layer never derives a log value from the raw `uri`.
#[allow(clippy::too_many_arguments)]
pub async fn send_request(
    uri: http::Uri,
    log_display: &str,
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
        "[HyperClient] Sending request: target={}, header_count={}, \
         has_host={}, has_original_cases={has_cases}, proxy={:?}",
        log_display,
        headers.len(),
        headers.contains_key(http::header::HOST),
        proxy_url.map(super::http_client::mask_url),
    );

    if let Some(original_cases) = original_cases
        .as_ref()
        .filter(|cases| !cases.cases.is_empty())
    {
        // Primary path: use raw write + hyper handshake for exact header casing
        let result = tokio::time::timeout(
            timeout,
            send_raw_request(&uri, &method, &headers, original_cases, &body, proxy_url),
        )
        .await
        .map_err(|_| ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs())))?;

        match result {
            Ok(resp) => return Ok(resp),
            Err(RawExchangeError::Final(e)) => return Err(e),
            Err(RawExchangeError::DirectFallback(e)) => {
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
        .map_err(|e| {
            ProxyError::ForwardFailed(format!(
                "上游请求失败: {}",
                super::error::describe_error_chain(&e)
            ))
        })?;

    // Not tied to the pool: this body ends with its own pooled hyper-util connection.
    Ok(ProxyResponse::Hyper(resp.map(TrackedBody::untracked)))
}

/// TCP or TLS stream returned by `connect_via_proxy`.
///
/// When the proxy URL uses `https://`, the connection to the proxy itself is
/// TLS-wrapped before sending the CONNECT request.  The enum lets
/// `send_raw_request` work with either variant generically.
pub(crate) enum ProxyStream {
    Tcp(ProxySocket),
    Tls(Box<tokio_rustls::client::TlsStream<ProxySocket>>),
}

impl ProxyStream {
    pub(crate) fn tcp(stream: tokio::net::TcpStream) -> Self {
        Self::Tcp(ProxySocket::new(stream))
    }

    pub(crate) fn socket_mut(&mut self) -> &mut ProxySocket {
        match self {
            Self::Tcp(socket) => socket,
            Self::Tls(stream) => stream.get_mut().0,
        }
    }
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

#[derive(Debug)]
enum RawExchangeError {
    DirectFallback(ProxyError),
    Final(ProxyError),
}

/// Send request via raw TCP/TLS with exact original header casing.
///
/// When `proxy_url` is provided, establishes an HTTP CONNECT tunnel through
/// the proxy first, then performs TLS + raw write through the tunnel.
/// This preserves header casing even when an upstream proxy is configured.
///
/// The connection comes from the keep-alive pool (`upstream_pool`) when one is idle
/// for this exact target, otherwise it is established here. TLS goes on before any
/// request byte is written, so a pooled connection is always a complete TLS session.
async fn send_raw_request(
    uri: &http::Uri,
    method: &http::Method,
    headers: &http::HeaderMap,
    original_cases: &OriginalHeaderCases,
    body: &[u8],
    proxy_url: Option<&str>,
) -> Result<ProxyResponse, RawExchangeError> {
    let scheme = uri.scheme_str().unwrap_or("https");
    let host = uri
        .host()
        .ok_or_else(|| ProxyError::ForwardFailed("URI has no host".into()))
        .map_err(if proxy_url.is_none() {
            RawExchangeError::DirectFallback
        } else {
            RawExchangeError::Final
        })?;
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    // Build raw HTTP request bytes
    let raw = build_raw_request(method, path_and_query, headers, original_cases, body);

    // Write the bytes over an idle connection to the same target when the pool has a
    // usable one, otherwise over a connection established here — direct, or through an
    // HTTP CONNECT proxy.
    let key = PoolKey::new(scheme, host, port, proxy_url);
    exchange_raw_request(
        &UpstreamPool::global(),
        &key,
        RawRequest {
            bytes: &raw,
            connection_close: upstream_pool::connection_forbids_reuse(headers),
        },
        method,
        proxy_url.is_none(),
        async {
            let stream = if let Some(proxy) = proxy_url {
                connect_via_proxy(proxy, host, port).await?
            } else {
                ProxyStream::tcp(connect_direct(host, port).await?)
            };
            if scheme == "https" {
                let tls_connector = global_tls_connector();
                let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
                    .map_err(|e| ProxyError::ForwardFailed(format!("Invalid server name: {e}")))?;
                let tls_stream = tls_connector
                    .connect(server_name, stream)
                    .await
                    .map_err(|e| ProxyError::ForwardFailed(format!("TLS handshake failed: {e}")))?;
                Ok(PooledStream::Tls(Box::new(tls_stream)))
            } else {
                Ok(PooledStream::Plain(stream))
            }
        },
    )
    .await
}

async fn exchange_raw_request(
    pool: &UpstreamPool,
    key: &PoolKey,
    request: RawRequest<'_>,
    method: &http::Method,
    direct: bool,
    connect: impl Future<Output = Result<PooledStream, ProxyError>>,
) -> Result<ProxyResponse, RawExchangeError> {
    let fresh_error = if let Taken::Idle(stream) = pool.take(key) {
        log::debug!("[UpstreamPool] upstream connection: reused idle connection");
        let error = match upstream_pool::write_raw_request(pool, key, &request, stream).await {
            Ok(io) => match do_hyper_response(io, method.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) if direct => error,
                Err(error) => return Err(RawExchangeError::Final(error)),
            },
            Err(failure) if direct || !failure.wrote_bytes => failure.error,
            Err(failure) => return Err(RawExchangeError::Final(failure.error)),
        };
        log::debug!("[UpstreamPool] retrying on a new raw connection: {error}");
        // For direct requests this replaces the existing hyper-util replay. A
        // failure here must not fall through to a third attempt. Through a proxy
        // we only get here if TCP did not accept any bytes of the failed write.
        RawExchangeError::Final
    } else {
        log::debug!("[UpstreamPool] upstream connection: opened new connection (none idle)");
        if direct {
            RawExchangeError::DirectFallback
        } else {
            RawExchangeError::Final
        }
    };

    let stream = connect.await.map_err(fresh_error)?;
    let io = upstream_pool::write_raw_request(pool, key, &request, stream)
        .await
        .map_err(|failure| fresh_error(failure.error))?;
    do_hyper_response(io, method.clone())
        .await
        .map_err(fresh_error)
}

/// Open a direct TCP connection to `host:port` with Happy Eyeballs.
///
/// `TcpStream::connect` tries every resolved address one after another and only
/// moves on when the OS gives up on the current one (about 75s per address on
/// macOS, about 21s on Windows). Hosts that publish AAAA records unreachable from
/// the user's network (e.g. two dead IPv6 addresses in front of a working IPv4)
/// then stall every request for minutes before the first byte goes out. Reuse
/// hyper-util's connector, which races the other address family after 300ms the
/// same way curl and browsers do.
async fn connect_direct(host: &str, port: u16) -> Result<tokio::net::TcpStream, ProxyError> {
    use tower::Service;

    // `host` comes from `http::Uri::host()`, which keeps IPv6 literals bracketed.
    let dst: http::Uri = format!("http://{host}:{port}/")
        .parse()
        .map_err(|e| ProxyError::ForwardFailed(format!("Invalid upstream address: {e}")))?;
    let mut connector = hyper_util::client::legacy::connect::HttpConnector::new();
    connector.enforce_http(false);
    std::future::poll_fn(|cx| connector.poll_ready(cx))
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("TCP connect failed: {e}")))?;
    let io = connector.call(dst).await.map_err(|e| {
        ProxyError::ForwardFailed(format!(
            "TCP connect failed: {}",
            super::error::describe_error_chain(&e)
        ))
    })?;
    Ok(io.into_inner())
}

/// Maximum number of response header lines accepted from an HTTP CONNECT proxy.
const MAX_CONNECT_HEADER_LINES: usize = 64;
/// Maximum total bytes accepted for an HTTP CONNECT proxy response header block.
const MAX_CONNECT_HEADER_BYTES: usize = 16 * 1024;

/// Percent-decode RFC 3986 userinfo (`username`/`password`) before Basic Base64 encoding.
fn percent_decode_userinfo(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                decoded.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

async fn read_connect_line_bounded<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<String>, ProxyError> {
    use tokio::io::AsyncBufReadExt;

    let mut line = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT read failed: {e}")))?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(ProxyError::ForwardFailed(
                "Proxy closed connection mid-header during CONNECT".into(),
            ));
        }

        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            if line.len().saturating_add(take) > max_bytes {
                return Err(ProxyError::ForwardFailed(
                    "Proxy CONNECT response headers exceeded size limit".into(),
                ));
            }
            line.extend_from_slice(&available[..take]);
            reader.consume(take);
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }

        let len = available.len();
        if line.len().saturating_add(len) > max_bytes {
            return Err(ProxyError::ForwardFailed(
                "Proxy CONNECT response headers exceeded size limit".into(),
            ));
        }
        line.extend_from_slice(available);
        reader.consume(len);
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
    use tokio::io::{AsyncWriteExt, BufReader};

    if target_host.contains('\r') || target_host.contains('\n') {
        return Err(ProxyError::ForwardFailed(
            "Invalid target host for CONNECT".into(),
        ));
    }

    let parsed = url::Url::parse(proxy_url)
        .map_err(|e| ProxyError::ForwardFailed(format!("Invalid proxy URL: {e}")))?;

    let proxy_host = parsed
        .host_str()
        .ok_or_else(|| ProxyError::ForwardFailed("Proxy URL has no host".into()))?;
    let proxy_port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });

    // Build Proxy-Authorization header if credentials are present (RFC 3986 percent-decoded)
    let proxy_auth = if !parsed.username().is_empty() || parsed.password().is_some() {
        let username = percent_decode_userinfo(parsed.username());
        let password = parsed
            .password()
            .map(percent_decode_userinfo)
            .unwrap_or_default();
        let credentials = format!("{username}:{password}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        Some(format!("Proxy-Authorization: Basic {encoded}\r\n"))
    } else {
        None
    };

    // Connect to the proxy
    let tcp = tokio::net::TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Proxy TCP connect failed: {e}")))?;
    let tcp = ProxySocket::new(tcp);

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

    // Read the proxy's response status line with a hard byte bound
    let mut reader = BufReader::new(&mut stream);
    let mut remaining_bytes = MAX_CONNECT_HEADER_BYTES;
    let status_line = read_connect_line_bounded(&mut reader, remaining_bytes)
        .await?
        .ok_or_else(|| {
            ProxyError::ForwardFailed("Proxy closed connection before CONNECT status line".into())
        })?;
    remaining_bytes = remaining_bytes.saturating_sub(status_line.len());

    // Expect "HTTP/1.1 200 ..." or "HTTP/1.0 200" (reason phrase optional per RFC 9110)
    let mut status_parts = status_line.split_whitespace();
    let http_version = status_parts.next().unwrap_or("");
    let status_code = status_parts.next().unwrap_or("");
    if !http_version.starts_with("HTTP/1.") || status_code != "200" {
        if http_version.starts_with("HTTP/1.") && status_code == "407" {
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

    // Drain remaining response headers (until empty line), bounded by line count, total bytes, and EOF
    let mut header_lines = 0usize;
    loop {
        let Some(line) = read_connect_line_bounded(&mut reader, remaining_bytes).await? else {
            return Err(ProxyError::ForwardFailed(
                "Proxy closed connection before end of CONNECT headers".into(),
            ));
        };
        remaining_bytes = remaining_bytes.saturating_sub(line.len());
        if line == "\r\n" || line == "\n" {
            break;
        }
        header_lines += 1;
        if header_lines > MAX_CONNECT_HEADER_LINES {
            return Err(ProxyError::ForwardFailed(
                "Proxy CONNECT response exceeded maximum header line count".into(),
            ));
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
/// The stream is wrapped so that hyper's own writes go nowhere (it would try to send
/// its own request encoding) while reads pass through, and so that the connection
/// returns to the keep-alive pool once the response is over instead of being closed
/// together with hyper's connection.
async fn do_hyper_response(
    stream: PooledIo,
    method: http::Method,
) -> Result<ProxyResponse, ProxyError> {
    let connection_state = stream.connection_state();
    let io = hyper_util::rt::TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
        .preserve_header_case(true)
        .handshake::<_, http_body_util::Full<Bytes>>(io)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Handshake failed: {e}")))?;

    let dummy_req = http::Request::builder()
        .method(method.clone())
        .uri("/")
        .body(http_body_util::Full::new(Bytes::new()))
        .map_err(|e| ProxyError::ForwardFailed(format!("Build dummy request: {e}")))?;

    // Queue the dummy request before the driver starts. `send_request` enqueues it
    // synchronously, so hyper's very first poll finds a request in flight, encodes it
    // and thereby opens `PooledIo`'s read gate before any response byte is parsed. The
    // encoded dummy bytes are discarded. See the gate in `PooledIo::poll_read` for the
    // race this order closes.
    let response = sender.send_request(dummy_req);
    let connection = upstream_pool::spawn_connection_driver(conn, connection_state);
    let resp = response
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Response parse failed: {e}")))?;

    // `sender` is dropped here on purpose: with no way left to send another request,
    // hyper shuts its connection down as soon as this response is over, which is one of
    // the two signals the driver waits for. If this function is cancelled instead (the
    // first-byte timeout), the handle's Drop tells the driver that no body will come.
    Ok(connection.tracked_response(resp, &method))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffered_with_content_type(content_type: Option<&str>) -> ProxyResponse {
        let mut headers = http::HeaderMap::new();
        if let Some(content_type) = content_type {
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_str(content_type).unwrap(),
            );
        }
        ProxyResponse::buffered(http::StatusCode::OK, headers, Bytes::new())
    }

    #[test]
    fn json_content_type_detection_accepts_json_suffixes() {
        assert!(buffered_with_content_type(Some("application/json; charset=utf-8")).is_json());
        assert!(buffered_with_content_type(Some("application/problem+json")).is_json());
        assert!(!buffered_with_content_type(Some("text/event-stream")).is_json());
        assert!(!buffered_with_content_type(None).is_json());
    }

    #[tokio::test]
    async fn bytes_with_limit_rejects_oversized_buffered_response() {
        let oversized = Bytes::from(vec![0u8; MAX_RESPONSE_BODY_BYTES + 1]);
        let response =
            ProxyResponse::buffered(http::StatusCode::OK, http::HeaderMap::new(), oversized);

        let result = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await;
        assert!(matches!(result, Err(ProxyError::ResponseBodyTooLarge(_))));
    }

    #[tokio::test]
    async fn bytes_with_limit_rejects_oversized_streamed_response() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(2);
        let stream = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        let response =
            ProxyResponse::streamed(http::StatusCode::OK, http::HeaderMap::new(), stream);

        tokio::spawn(async move {
            let _ = tx.send(Ok(Bytes::from(vec![0u8; 64 * 1024]))).await;
            let _ = tx
                .send(Ok(Bytes::from(vec![0u8; MAX_RESPONSE_BODY_BYTES])))
                .await;
        });

        let result = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await;
        assert!(matches!(result, Err(ProxyError::ResponseBodyTooLarge(_))));
    }

    /// 启动一个最小 HTTP/1.1 服务器：响应 `Content-Length: body_len` 的全零 body，
    /// 分块写出并统计实际写成功的字节数（客户端断开后写入失败即停）。
    async fn spawn_fixed_body_server(
        body_len: usize,
    ) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let written = Arc::new(AtomicUsize::new(0));
        let written_report = written.clone();

        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            // 读完请求头（内容不重要）
            let mut buf = [0u8; 4096];
            let mut filled = 0;
            loop {
                if buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                let Ok(n) = socket.read(&mut buf[filled..]).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                filled += n;
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {body_len}\r\nconnection: close\r\n\r\n"
            );
            if socket.write_all(header.as_bytes()).await.is_err() {
                return;
            }
            let chunk = [0u8; 16 * 1024];
            let mut remaining = body_len;
            while remaining > 0 {
                let n = remaining.min(chunk.len());
                if socket.write_all(&chunk[..n]).await.is_err() {
                    break;
                }
                written.fetch_add(n, Ordering::SeqCst);
                remaining -= n;
            }
        });

        (port, written_report)
    }

    /// 客户端断开到服务器写入失败之间有时延（loopback 缓冲区会再吞一部分），
    /// 稍等再读计数。只要客户端真的中途截停，服务器绝不可能写出大半个 body。
    async fn assert_server_aborted_early(
        written: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        body_len: usize,
    ) {
        use std::sync::atomic::Ordering;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let written = written.load(Ordering::SeqCst);
        assert!(
            written < body_len / 2,
            "客户端应在预算耗尽后立即断开，服务器不应写出大部分 body（实际已写 {written}/{body_len} 字节）"
        );
    }

    #[tokio::test]
    async fn bytes_with_limit_aborts_hyper_response_before_full_body_arrives() {
        const BODY_LEN: usize = 16 * 1024 * 1024;
        const LIMIT: usize = 64 * 1024;
        let (port, written) = spawn_fixed_body_server(BODY_LEN).await;

        // 构造真实的 hyper::Response<Incoming>：手工建立 http1 客户端连接
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let io = hyper_util::rt::TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let request = http::Request::builder()
            .uri(format!("http://127.0.0.1:{port}/"))
            .body(http_body_util::Empty::<Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        assert_eq!(response.status(), http::StatusCode::OK);

        let result = ProxyResponse::Hyper(response.map(TrackedBody::untracked))
            .bytes_with_limit(LIMIT)
            .await;
        assert!(matches!(result, Err(ProxyError::ResponseBodyTooLarge(_))));

        // 关键断言：若退回"先 collect 收满再比较"，服务器会把 16 MiB 全部写完
        assert_server_aborted_early(written, BODY_LEN).await;
    }

    #[tokio::test]
    async fn bytes_with_limit_aborts_reqwest_response_before_full_body_arrives() {
        const BODY_LEN: usize = 16 * 1024 * 1024;
        const LIMIT: usize = 64 * 1024;
        let (port, written) = spawn_fixed_body_server(BODY_LEN).await;

        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let result = ProxyResponse::Reqwest(response)
            .bytes_with_limit(LIMIT)
            .await;
        assert!(matches!(result, Err(ProxyError::ResponseBodyTooLarge(_))));

        assert_server_aborted_early(written, BODY_LEN).await;
    }

    #[tokio::test]
    async fn connect_direct_reaches_ipv4_and_bracketed_ipv6_hosts() {
        let v4 = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let v4_port = v4.local_addr().unwrap().port();
        let stream = connect_direct("127.0.0.1", v4_port).await.unwrap();
        assert_eq!(stream.peer_addr().unwrap().port(), v4_port);

        // `http::Uri::host()` hands IPv6 literals over with brackets.
        let Ok(v6) = tokio::net::TcpListener::bind(("::1", 0)).await else {
            return; // no IPv6 loopback on this machine
        };
        let v6_port = v6.local_addr().unwrap().port();
        let uri: http::Uri = format!("https://[::1]:{v6_port}/v1/messages")
            .parse()
            .unwrap();
        let stream = connect_direct(uri.host().unwrap(), v6_port).await.unwrap();
        assert!(stream.peer_addr().unwrap().is_ipv6());
    }

    // ===== upstream keep-alive pool (`upstream_pool`) =====

    use crate::proxy::upstream_pool::test_support::{
        put_idle_with_closed_write_half, tls_client_config, tls_pair, AfterReply, Body, Reply,
        TestServer,
    };
    use crate::proxy::upstream_pool::{PoolConfig, PoolKey, UpstreamPool};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// A representative Anthropic-style request whose header casing must survive the
    /// wire. `accept` is deliberately absent from `cases`, so it goes out like a
    /// proxy-added header (lowercase), exercising both emission branches.
    fn pool_test_request() -> (http::Method, http::HeaderMap, OriginalHeaderCases, Vec<u8>) {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::HOST,
            http::HeaderValue::from_static("127.0.0.1"),
        );
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer test-key"),
        );
        headers.insert(http::header::ACCEPT, http::HeaderValue::from_static("*/*"));
        headers.insert(
            http::HeaderName::from_static("x-mixed-case"),
            http::HeaderValue::from_static("keep-me"),
        );
        let cases = OriginalHeaderCases {
            cases: vec![
                ("host".to_string(), b"Host".to_vec()),
                ("content-type".to_string(), b"content-type".to_vec()),
                ("authorization".to_string(), b"AUTHORIZATION".to_vec()),
                ("x-mixed-case".to_string(), b"X-MiXeD-CaSe".to_vec()),
            ],
        };
        (
            http::Method::POST,
            headers,
            cases,
            br#"{"model":"claude-sonnet","stream":true}"#.to_vec(),
        )
    }

    /// The request `send_raw_with` sends, so tests can compute the expected wire bytes.
    fn pool_test_request_with(
        extra: &[(&str, &str)],
    ) -> (http::Method, http::HeaderMap, OriginalHeaderCases, Vec<u8>) {
        let (method, mut headers, mut cases, body) = pool_test_request();
        for (name, value) in extra {
            let header_name =
                http::HeaderName::from_bytes(name.as_bytes()).expect("test header name parses");
            headers.insert(
                header_name,
                http::HeaderValue::from_str(value).expect("test header value parses"),
            );
            cases
                .cases
                .push((name.to_ascii_lowercase(), name.as_bytes().to_vec()));
        }
        (method, headers, cases, body)
    }

    /// One raw-path request through `send_request`, the way the forwarder calls it.
    async fn send_raw_with(
        url: &str,
        proxy_url: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<ProxyResponse, ProxyError> {
        // A raw-path failure falls back to the hyper-util client, which builds a rustls
        // config; without a process-level provider that panics on the ambiguous crate
        // features instead of surfacing the real error.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (method, headers, cases, body) = pool_test_request_with(extra);
        let mut extensions = http::Extensions::new();
        extensions.insert(cases.clone());
        let uri: http::Uri = url.parse().expect("test url parses");
        send_request(
            uri,
            "pool-test",
            method,
            headers,
            extensions,
            body,
            Duration::from_secs(30),
            proxy_url,
        )
        .await
    }

    async fn send_raw(url: &str, proxy_url: Option<&str>) -> Result<ProxyResponse, ProxyError> {
        send_raw_with(url, proxy_url, &[]).await
    }

    /// Send one request and assert the upstream echoed the request bytes back.
    async fn send_and_expect_echo(url: &str, proxy_url: Option<&str>) {
        let response = send_raw(url, proxy_url).await.expect("request failed");
        assert_eq!(response.status(), http::StatusCode::OK);
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        let (method, headers, cases, request_body) = pool_test_request();
        let expected = build_raw_request(&method, "/v1/messages", &headers, &cases, &request_body);
        assert_eq!(
            &body[..],
            &expected[..],
            "response must be the echoed request"
        );
    }

    /// Send one request and assert the upstream answered with `expected`.
    async fn send_and_expect_body(url: &str, proxy_url: Option<&str>, expected: &[u8]) {
        let response = send_raw(url, proxy_url).await.expect("request failed");
        assert_eq!(response.status(), http::StatusCode::OK);
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        assert_eq!(&body[..], expected);
    }

    /// Poll `check` until it holds or 4 seconds elapse: the driver and the close task
    /// run in the background, so pool and socket state settle a few ticks later.
    async fn wait_for(what: &str, check: impl Fn() -> bool) {
        for _ in 0..400 {
            if check() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}");
    }

    fn pool_key_for(server: &TestServer) -> PoolKey {
        PoolKey::new("http", "127.0.0.1", server.port(), None)
    }

    /// The wire bytes of the standard test request as only the raw path writes them:
    /// the original header order and the odd casings (`content-type`, `AUTHORIZATION`,
    /// `X-MiXeD-CaSe`). The hyper-util fallback would title-case the names and lose the
    /// order (see `fallback_client_bytes_differ_from_raw_bytes`).
    fn standard_raw_request_bytes() -> Vec<u8> {
        let (method, headers, cases, body) = pool_test_request();
        build_raw_request(&method, "/v1/messages", &headers, &cases, &body)
    }

    /// Assert that the upstream's `index`-th (0-based) request was served by the raw
    /// path: the captured wire bytes are byte-identical to `build_raw_request`'s output.
    async fn assert_served_by_raw_path(server: &TestServer, index: usize) {
        let requests = server.wait_for_requests(index + 1).await;
        assert_eq!(
            requests[index],
            standard_raw_request_bytes(),
            "request {index} must reach the upstream through the raw path unchanged"
        );
    }

    /// Assert the follow-up request left a connection in our pool: only the raw path
    /// ever puts connections there (the hyper-util fallback has its own pool).
    async fn assert_pooled_again(pool: &UpstreamPool, key: &PoolKey) {
        wait_for("the follow-up request's connection to be pooled", || {
            pool.idle_count(key) == 1
        })
        .await;
    }

    /// One raw-path exchange through a specific pool, built from the same functions
    /// `send_raw_request` uses (`write_raw_request` + `do_hyper_response`). No timeout
    /// wrapper and no fallback exist on this route, so it can only succeed through the
    /// raw path. Returns the response body.
    async fn raw_exchange(pool: &UpstreamPool, server: &TestServer) -> Bytes {
        let (method, headers, cases, body) = pool_test_request();
        let raw = build_raw_request(&method, "/v1/messages", &headers, &cases, &body);
        let key = pool_key_for(server);
        let port = server.port();
        let response = exchange_raw_request(
            pool,
            &key,
            RawRequest {
                bytes: &raw,
                connection_close: false,
            },
            &method,
            true,
            async {
                Ok(PooledStream::Plain(ProxyStream::tcp(
                    connect_direct("127.0.0.1", port).await?,
                )))
            },
        )
        .await
        .expect("raw response");
        response.bytes_with_limit(1 << 20).await.expect("body read")
    }

    /// A minimal HTTP CONNECT proxy: answers 200 and pipes both ways to the target.
    async fn spawn_connect_proxy() -> (u16, Arc<AtomicUsize>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_task = accepted.clone();
        tokio::spawn(async move {
            while let Ok((mut client, _)) = listener.accept().await {
                accepted_task.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut reader = BufReader::new(&mut client);
                    let mut head = String::new();
                    reader.read_line(&mut head).await.expect("CONNECT line");
                    assert!(
                        head.starts_with("CONNECT "),
                        "not a CONNECT request: {head:?}"
                    );
                    let target = head
                        .split_whitespace()
                        .nth(1)
                        .expect("CONNECT target")
                        .to_string();
                    loop {
                        let mut line = String::new();
                        let n = reader.read_line(&mut line).await.expect("CONNECT headers");
                        assert!(n > 0, "CONNECT head ended early");
                        if line == "\r\n" {
                            break;
                        }
                    }
                    drop(reader);
                    let mut upstream = tokio::net::TcpStream::connect(&target)
                        .await
                        .expect("tunnel target");
                    client
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await
                        .unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                });
            }
        });
        (port, accepted)
    }

    #[tokio::test]
    async fn direct_replay_after_a_reused_connection_fails_stays_raw_and_runs_once() {
        for retry_succeeds in [true, false] {
            let server = TestServer::start_script(move |request| match request.index {
                2 => None,
                3 if !retry_succeeds => None,
                _ => Some(Reply::fixed(request.bytes.clone())),
            })
            .await;
            let url = server.url();
            send_and_expect_echo(&url, None).await;
            assert_pooled_again(&UpstreamPool::global(), &pool_key_for(&server)).await;

            let result = send_raw(&url, None).await;
            if retry_succeeds {
                let response = result.expect("a fresh raw connection should recover the request");
                let echoed = response.bytes_with_limit(1 << 20).await.unwrap();
                assert_eq!(echoed.as_ref(), standard_raw_request_bytes().as_slice());
            } else {
                assert!(matches!(result, Err(ProxyError::ForwardFailed(_))));
            }
            assert_eq!(server.requests(), vec![standard_raw_request_bytes(); 3]);
            assert_eq!(server.accepted(), 2);
        }
    }

    #[tokio::test]
    async fn connect_does_not_replay_a_reused_post_after_the_upstream_received_it() {
        let server = TestServer::start_script(|request| {
            (request.index != 2).then(|| Reply::fixed(request.bytes.clone()))
        })
        .await;
        let (proxy_port, tunnels) = spawn_connect_proxy().await;
        let proxy_url = format!("http://127.0.0.1:{proxy_port}");
        let url = server.url();
        send_and_expect_echo(&url, Some(&proxy_url)).await;
        let key = PoolKey::new("http", "127.0.0.1", server.port(), Some(&proxy_url));
        assert_pooled_again(&UpstreamPool::global(), &key).await;

        let result = send_raw(&url, Some(&proxy_url)).await;
        assert!(matches!(result, Err(ProxyError::ForwardFailed(_))));
        assert_eq!(server.requests(), vec![standard_raw_request_bytes(); 2]);
        assert_eq!(tunnels.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn connect_retries_a_pooled_tls_write_that_never_reached_tcp() {
        let server = TestServer::start_tls(|request| Reply::fixed(request.bytes.clone())).await;
        let (proxy_port, tunnels) = spawn_connect_proxy().await;
        let proxy_url = format!("http://127.0.0.1:{proxy_port}");
        let port = server.port();
        let key = PoolKey::new("https", "127.0.0.1", port, Some(&proxy_url));
        let pool = UpstreamPool::new(PoolConfig::default());
        let (stale_tls, _peer) = tls_pair().await;
        put_idle_with_closed_write_half(&pool, key.clone(), stale_tls).await;

        let raw = standard_raw_request_bytes();
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            exchange_raw_request(
                &pool,
                &key,
                RawRequest {
                    bytes: &raw,
                    connection_close: false,
                },
                &http::Method::POST,
                false,
                async {
                    let stream = connect_via_proxy(&proxy_url, "127.0.0.1", port).await?;
                    let connector = tokio_rustls::TlsConnector::from(tls_client_config());
                    let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
                    let tls = connector
                        .connect(name, stream)
                        .await
                        .expect("TLS handshake");
                    Ok(PooledStream::Tls(Box::new(tls)))
                },
            ),
        )
        .await
        .expect("TLS exchange in time")
        .expect("the write failed before TCP accepted any bytes");
        let echoed = response.bytes_with_limit(1 << 20).await.unwrap();
        assert_eq!(echoed.as_ref(), raw.as_slice());
        assert_eq!(server.requests(), vec![raw]);
        assert_eq!(tunnels.load(Ordering::SeqCst), 1);
    }

    /// R1 + R2: the wire bytes are exactly `build_raw_request`'s output, on fresh and
    /// reused connections alike, and repeated requests to one target share a connection.
    #[tokio::test]
    async fn raw_request_bytes_are_unchanged_and_one_connection_serves_repeated_requests() {
        let server = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let url = server.url();

        for _ in 0..3 {
            let response = send_raw(&url, None).await.expect("request failed");
            assert_eq!(response.status(), http::StatusCode::OK);
            let echoed = response.bytes_with_limit(1 << 20).await.expect("body read");
            let (method, headers, cases, request_body) = pool_test_request();
            let expected =
                build_raw_request(&method, "/v1/messages", &headers, &cases, &request_body);
            assert_eq!(
                &echoed[..],
                &expected[..],
                "wire bytes must reach the upstream unchanged"
            );
        }

        let requests = server.wait_for_requests(3).await;
        let (_, headers, cases, body) = pool_test_request();
        let expected =
            build_raw_request(&http::Method::POST, "/v1/messages", &headers, &cases, &body);
        for captured in requests {
            assert_eq!(
                captured, expected,
                "every request must be byte-identical on the wire"
            );
        }
        assert_eq!(
            server.accepted(),
            1,
            "repeated requests to one target must reuse one connection"
        );
    }

    /// R2: streaming (chunked, SSE-style) responses arrive intact over a reused
    /// connection.
    #[tokio::test]
    async fn streaming_responses_are_intact_over_a_reused_connection() {
        let server = TestServer::start(|request| {
            let echo = request.bytes.clone();
            let (first, second) = echo.split_at(echo.len() / 2);
            Reply {
                status_line: "HTTP/1.1 200 OK",
                headers: vec!["Content-Type: text/event-stream".to_string()],
                body: Body::Chunked(
                    vec![first.to_vec(), second.to_vec()],
                    Duration::from_millis(10),
                ),
                after: AfterReply::KeepOpen,
            }
        })
        .await;
        let url = server.url();
        let (method, headers, cases, request_body) = pool_test_request();
        let expected = build_raw_request(&method, "/v1/messages", &headers, &cases, &request_body);

        for _ in 0..3 {
            let response = send_raw(&url, None).await.expect("request failed");
            let mut collected = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                collected.extend_from_slice(&chunk.expect("chunk"));
            }
            assert_eq!(
                &collected[..],
                &expected[..],
                "streamed body must be intact"
            );
        }

        assert_eq!(
            server.accepted(),
            1,
            "streaming requests must reuse one connection"
        );
    }

    /// R3.a: a `Connection: close` response is not pooled, even though the socket stays
    /// open: only the header rules the reuse out here.
    #[tokio::test]
    async fn response_with_connection_close_is_not_pooled() {
        let server = TestServer::start(|request| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: match request.index {
                1 => vec!["Connection: close".into(), "Connection: keep-alive".into()],
                2 => vec!["Connection: keep-alive".into(), "Connection: close".into()],
                _ => vec!["Connection: close".into()],
            },
            body: Body::Fixed(b"closed".to_vec()),
            after: AfterReply::KeepOpen,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        for index in 0..3 {
            send_and_expect_body(&url, None, b"closed").await;
            assert_served_by_raw_path(&server, index).await;
            wait_for("the close response to close the connection", || {
                pool.idle_count(&key) == 0 && server.open_connections() == 0
            })
            .await;
            assert_eq!(server.accepted(), index + 1);
        }
    }

    /// R3.a: an HTTP/1.0 response without `Connection: keep-alive` is not pooled.
    #[tokio::test]
    async fn http10_response_without_keep_alive_is_not_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.0 200 OK",
            headers: Vec::new(),
            body: Body::Fixed(b"http10".to_vec()),
            after: AfterReply::KeepOpen,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw(&url, None).await.expect("request failed");
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        assert_eq!(&body[..], b"http10");
        wait_for("the HTTP/1.0 response to close the connection", || {
            pool.idle_count(&key) == 0 && server.open_connections() == 0
        })
        .await;

        send_and_expect_body(&url, None, b"http10").await;
        assert_served_by_raw_path(&server, 1).await;
        assert_eq!(
            server.accepted(),
            2,
            "an HTTP/1.0 response must not be reused"
        );
    }

    /// R3.a: an HTTP/1.0 response that does advertise keep-alive is pooled, mirroring
    /// hyper's own keep-alive rules.
    #[tokio::test]
    async fn http10_response_with_keep_alive_is_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.0 200 OK",
            headers: vec!["Connection: keep-alive".to_string()],
            body: Body::Fixed(b"http10-ka".to_vec()),
            after: AfterReply::KeepOpen,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        send_and_expect_body(&url, None, b"http10-ka").await;
        assert_served_by_raw_path(&server, 0).await;
        wait_for("the HTTP/1.0 keep-alive response to be pooled", || {
            pool.idle_count(&key) == 1
        })
        .await;
        send_and_expect_body(&url, None, b"http10-ka").await;
        assert_served_by_raw_path(&server, 1).await;
        assert_eq!(
            server.accepted(),
            1,
            "an HTTP/1.0 keep-alive response must be reused"
        );
    }

    /// R3.a: a response without Content-Length and without chunked encoding ends only
    /// when the upstream closes, so it is never pooled.
    #[tokio::test]
    async fn close_delimited_response_is_not_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: Vec::new(),
            body: Body::UntilClose(b"until close".to_vec()),
            after: AfterReply::Close,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw(&url, None).await.expect("request failed");
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        assert_eq!(&body[..], b"until close");
        wait_for(
            "the close-delimited response to close the connection",
            || pool.idle_count(&key) == 0 && server.open_connections() == 0,
        )
        .await;

        send_and_expect_body(&url, None, b"until close").await;
        assert_served_by_raw_path(&server, 1).await;
        assert_eq!(
            server.accepted(),
            2,
            "a close-delimited body must not be reused"
        );
    }

    /// R3.a: a request that itself carries `Connection: close` never pools its
    /// connection, whatever the response says.
    #[tokio::test]
    async fn request_with_connection_close_is_not_pooled() {
        let server = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw_with(&url, None, &[("Connection", "close")])
            .await
            .expect("request failed");
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        let (method, headers, cases, request_body) =
            pool_test_request_with(&[("Connection", "close")]);
        let expected = build_raw_request(&method, "/v1/messages", &headers, &cases, &request_body);
        assert_eq!(&body[..], &expected[..]);
        wait_for("the close request to close the connection", || {
            pool.idle_count(&key) == 0 && server.open_connections() == 0
        })
        .await;

        send_and_expect_echo(&url, None).await;
        assert_served_by_raw_path(&server, 1).await;
        assert_eq!(server.accepted(), 2, "a close request must not be reused");
    }

    /// R3.b: a consumer that stops reading after the first chunk leaves the connection
    /// mid-body; it must be closed, not pooled.
    #[tokio::test]
    async fn body_dropped_mid_stream_is_not_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: vec!["Content-Type: text/event-stream".to_string()],
            body: Body::Chunked(
                vec![b"chunk-one\n\n".to_vec(), b"chunk-two\n\n".to_vec()],
                Duration::from_millis(150),
            ),
            after: AfterReply::KeepOpen,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw(&url, None).await.expect("request failed");
        let mut stream = response.bytes_stream();
        let first = stream.next().await.expect("first chunk").expect("chunk ok");
        assert_eq!(&first[..], b"chunk-one\n\n");
        drop(stream);
        wait_for("the dropped body to close the connection", || {
            pool.idle_count(&key) == 0 && server.open_connections() == 0
        })
        .await;

        let response = send_raw(&url, None).await.expect("second request failed");
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        assert_eq!(&body[..], b"chunk-one\n\nchunk-two\n\n");
        assert_served_by_raw_path(&server, 1).await;
        assert_eq!(
            server.accepted(),
            2,
            "a connection whose body was dropped must not be reused"
        );
    }

    /// R3.b/R3.c: a body that errors mid-stream (upstream closes before the declared
    /// Content-Length is reached) leaves the connection unusable.
    #[tokio::test]
    async fn body_error_mid_stream_is_not_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: Vec::new(),
            body: Body::Short(1000, b"only a part".to_vec()),
            after: AfterReply::Close,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw(&url, None).await.expect("request failed");
        let result = response.bytes_with_limit(1 << 20).await;
        assert!(
            matches!(result, Err(ProxyError::ForwardFailed(_))),
            "a truncated body must surface an error, got {result:?}"
        );
        wait_for("the errored body to close the connection", || {
            pool.idle_count(&key) == 0 && server.open_connections() == 0
        })
        .await;

        // The next request still gets through — on a fresh connection. Its body errors
        // the same way, since this upstream always truncates; only the fresh connection
        // matters here.
        send_raw(&url, None).await.expect("second request failed");
        wait_for("the second connection to be accepted", || {
            server.accepted() == 2
        })
        .await;
        assert_served_by_raw_path(&server, 1).await;
    }

    /// R3.d: the upstream closes the connection while it sits idle (without saying so
    /// in the response). The next request still succeeds, and it is provably the raw
    /// path serving it on a fresh connection: its wire bytes are byte-identical to
    /// `build_raw_request`'s output, and its connection ends up in our pool afterwards.
    /// A silent fallback to the hyper-util client would fail both checks.
    #[tokio::test]
    async fn upstream_closed_while_idle_is_replaced_by_a_fresh_connection() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: Vec::new(),
            body: Body::Fixed(b"bye".to_vec()),
            // The reply itself stays keep-alive; the upstream only closes later, while
            // the connection sits idle in the pool.
            after: AfterReply::CloseAfter(Duration::from_millis(300)),
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        send_and_expect_body(&url, None, b"bye").await;
        wait_for("the response to be pooled", || pool.idle_count(&key) == 1).await;
        // Wait past the upstream's delayed close, so its FIN has arrived while the
        // connection sat idle in the pool.
        tokio::time::sleep(Duration::from_millis(400)).await;

        send_and_expect_body(&url, None, b"bye").await;
        assert_served_by_raw_path(&server, 1).await;
        assert_pooled_again(&pool, &key).await;
        assert_eq!(
            server.accepted(),
            2,
            "a connection the upstream closed must not be reused"
        );
    }

    /// R3.d: unsolicited bytes while idle mean the connection is not at a message
    /// boundary; the next request still succeeds, and it is provably the raw path
    /// serving it on a fresh connection (wire bytes and pool state, as above).
    #[tokio::test]
    async fn unexpected_bytes_while_idle_are_discarded() {
        let server = TestServer::start(|request| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: Vec::new(),
            body: Body::Fixed(b"clean".to_vec()),
            // Only the first response is followed by junk. The second connection is an
            // ordinary keep-alive one, so it must end up pooled again.
            after: if request.index == 1 {
                AfterReply::Send(b"unsolicited".to_vec())
            } else {
                AfterReply::KeepOpen
            },
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        send_and_expect_body(&url, None, b"clean").await;
        // Give hyper time to see the stray bytes or the pool to hold the connection;
        // both outcomes must end in a fresh connection for the next request.
        tokio::time::sleep(Duration::from_millis(100)).await;

        send_and_expect_body(&url, None, b"clean").await;
        assert_served_by_raw_path(&server, 1).await;
        assert_pooled_again(&pool, &key).await;
        assert_eq!(
            server.accepted(),
            2,
            "a connection with unsolicited bytes must not be reused"
        );
    }

    /// R3.e: different targets (here: different ports) never share connections.
    #[tokio::test]
    async fn different_targets_do_not_share_connections() {
        let a = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let b = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let pool = UpstreamPool::global();
        let key_a = pool_key_for(&a);
        let key_b = pool_key_for(&b);

        send_and_expect_echo(&a.url(), None).await;
        send_and_expect_echo(&b.url(), None).await;
        wait_for("both targets to be pooled", || {
            pool.idle_count(&key_a) == 1 && pool.idle_count(&key_b) == 1
        })
        .await;
        assert_eq!(a.accepted(), 1, "target a must see exactly one connection");
        assert_eq!(b.accepted(), 1, "target b must see exactly one connection");
    }

    /// R5: the CONNECT tunnel path still works and is pooled per proxy URL — two
    /// requests through one proxy share a tunnel, a different proxy opens its own.
    #[tokio::test]
    async fn connect_tunnel_is_pooled_per_proxy_url() {
        let upstream = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let (proxy_a_port, proxy_a_accepted) = spawn_connect_proxy().await;
        let (proxy_b_port, proxy_b_accepted) = spawn_connect_proxy().await;
        let url = upstream.url();
        let proxy_a = format!("http://127.0.0.1:{proxy_a_port}");
        let proxy_b = format!("http://127.0.0.1:{proxy_b_port}");
        let key_a = PoolKey::new("http", "127.0.0.1", upstream.port(), Some(&proxy_a));
        let pool = UpstreamPool::global();

        send_and_expect_echo(&url, Some(&proxy_a)).await;
        wait_for("the tunnel to be pooled", || pool.idle_count(&key_a) == 1).await;
        send_and_expect_echo(&url, Some(&proxy_a)).await;
        assert_eq!(
            proxy_a_accepted.load(Ordering::SeqCst),
            1,
            "both requests must share one CONNECT tunnel"
        );
        wait_for("the tunnel to be pooled", || pool.idle_count(&key_a) == 1).await;

        send_and_expect_echo(&url, Some(&proxy_b)).await;
        assert_eq!(
            proxy_b_accepted.load(Ordering::SeqCst),
            1,
            "a different proxy URL must open its own tunnel"
        );
        assert_eq!(
            upstream.accepted(),
            2,
            "the upstream must see one connection per tunnel"
        );
    }

    #[tokio::test]
    async fn tls_streaming_reuses_the_connection_directly_and_through_connect() {
        const FIRST: &[u8] = b"event: message_start\ndata: {}\n\n";
        const LAST: &[u8] = b"event: message_stop\ndata: {}\n\n";
        let connector = tokio_rustls::TlsConnector::from(tls_client_config());
        let raw = standard_raw_request_bytes();

        for through_proxy in [false, true] {
            let upstream = TestServer::start_tls(|_| Reply {
                status_line: "HTTP/1.1 200 OK",
                headers: vec!["Content-Type: text/event-stream".to_string()],
                body: Body::Chunked(
                    vec![FIRST.to_vec(), LAST.to_vec()],
                    Duration::from_millis(1),
                ),
                after: AfterReply::KeepOpen,
            })
            .await;
            let proxy = if through_proxy {
                let (port, accepted) = spawn_connect_proxy().await;
                Some((format!("http://127.0.0.1:{port}"), accepted))
            } else {
                None
            };
            let proxy_url = proxy.as_ref().map(|(url, _)| url.as_str());
            let port = upstream.port();
            let pool = UpstreamPool::new(PoolConfig::default());
            let key = PoolKey::new("https", "127.0.0.1", port, proxy_url);

            for _ in 0..2 {
                let received = tokio::time::timeout(Duration::from_secs(10), async {
                    let response = exchange_raw_request(
                        &pool,
                        &key,
                        RawRequest {
                            bytes: &raw,
                            connection_close: false,
                        },
                        &http::Method::POST,
                        !through_proxy,
                        async {
                            let stream = if let Some(url) = proxy_url {
                                connect_via_proxy(url, "127.0.0.1", port).await?
                            } else {
                                ProxyStream::tcp(connect_direct("127.0.0.1", port).await?)
                            };
                            let name =
                                rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
                            let tls = connector
                                .connect(name, stream)
                                .await
                                .expect("TLS handshake");
                            Ok(PooledStream::Tls(Box::new(tls)))
                        },
                    )
                    .await
                    .expect("raw response");
                    assert_eq!(response.status(), http::StatusCode::OK);
                    let stream = response.bytes_stream();
                    futures::pin_mut!(stream);
                    let mut received = Vec::new();
                    while let Some(chunk) = stream.next().await {
                        received.extend_from_slice(&chunk.expect("SSE chunk"));
                    }
                    received
                })
                .await
                .expect("TLS exchange in time");
                assert_eq!(received, [FIRST, LAST].concat());
                assert_eq!(pool.idle_count(&key), 1);
            }

            assert_eq!(upstream.requests(), vec![raw.clone(), raw.clone()]);
            assert_eq!(
                upstream.accepted(),
                1,
                "both requests must share one TLS connection"
            );
            if let Some((_, accepted)) = proxy {
                assert_eq!(
                    accepted.load(Ordering::SeqCst),
                    1,
                    "both requests must share one tunnel"
                );
            }
        }
    }

    #[tokio::test]
    async fn keep_alive_timeout_within_margin_closes_an_otherwise_reusable_connection() {
        let server = TestServer::start(|_| Reply {
            headers: vec!["Keep-Alive: max=100, TimeOut = 1".to_string()],
            ..Reply::fixed(b"body".to_vec())
        })
        .await;
        let pool = UpstreamPool::new(PoolConfig::default());
        assert_eq!(raw_exchange(&pool, &server).await.as_ref(), b"body");
        assert_eq!(pool.idle_count(&pool_key_for(&server)), 0);
    }

    /// R4: concurrent requests find no idle connection, so each opens its own; all
    /// responses arrive intact, and afterwards only the per-key cap stays pooled.
    #[tokio::test]
    async fn concurrent_requests_open_one_connection_each_and_all_succeed() {
        let server = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();
        const N: usize = 8;
        let in_flight = Arc::new(tokio::sync::Barrier::new(N));

        let tasks = (0..N)
            .map(|_| {
                let url = url.clone();
                let in_flight = in_flight.clone();
                tokio::spawn(async move {
                    let response = send_raw(&url, None).await.expect("request failed");
                    assert_eq!(response.status(), http::StatusCode::OK);
                    // No task can drain its body and release a connection until
                    // all N requests have reached the upstream.
                    in_flight.wait().await;
                    let echoed = response.bytes_with_limit(1 << 20).await.expect("body read");
                    assert_eq!(echoed.as_ref(), standard_raw_request_bytes().as_slice());
                })
            })
            .collect::<Vec<_>>();
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(15), task)
                .await
                .expect("concurrent request in time")
                .expect("concurrent request task");
        }

        assert_eq!(
            server.accepted(),
            N,
            "concurrent requests must each get their own connection"
        );
        wait_for("the pool to keep only the per-key cap", || {
            pool.idle_count(&key) == UpstreamPool::per_key_cap()
        })
        .await;
    }

    /// R3.a: statuses without a body (204) pool fine even without length headers.
    #[tokio::test]
    async fn status_without_body_is_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 204 No Content",
            headers: Vec::new(),
            body: Body::Fixed(Vec::new()),
            after: AfterReply::KeepOpen,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw(&url, None).await.expect("request failed");
        assert_eq!(response.status(), http::StatusCode::NO_CONTENT);
        let body = response.bytes_with_limit(1 << 20).await.expect("body read");
        assert!(body.is_empty());
        wait_for("the 204 response to be pooled", || {
            pool.idle_count(&key) == 1
        })
        .await;

        let reused = send_raw(&url, None).await.expect("second request failed");
        assert_eq!(reused.status(), http::StatusCode::NO_CONTENT);
        let body = reused.bytes_with_limit(1 << 20).await.expect("body read");
        assert!(body.is_empty());
        assert_served_by_raw_path(&server, 0).await;
        assert_served_by_raw_path(&server, 1).await;
        assert_eq!(server.accepted(), 1, "a 204 response must be reused");
    }

    /// Control for the wire-bytes assertions above: the hyper-util fallback does not
    /// reproduce the raw path's bytes (it title-cases the header names and emits them in
    /// its own order), so a request that silently fell back would fail
    /// `assert_served_by_raw_path` instead of passing by accident.
    #[tokio::test]
    async fn fallback_client_bytes_differ_from_raw_bytes() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let server = TestServer::start(|request| Reply::fixed(request.bytes.clone())).await;
        let (method, headers, cases, body) = pool_test_request();
        let mut extensions = http::Extensions::new();
        extensions.insert(cases);

        // Exactly what `send_request` builds for its fallback branch.
        let mut req = http::Request::builder()
            .method(method)
            .uri(format!("http://127.0.0.1:{}/v1/messages", server.port()))
            .body(http_body_util::Full::new(Bytes::from(body)))
            .expect("fallback request builds");
        *req.headers_mut() = headers;
        *req.extensions_mut() = extensions;
        let response = global_hyper_client()
            .request(req)
            .await
            .expect("fallback request succeeds");
        assert_eq!(response.status(), http::StatusCode::OK);

        let requests = server.wait_for_requests(1).await;
        assert_ne!(
            requests[0],
            standard_raw_request_bytes(),
            "the hyper-util fallback must not be byte-identical to the raw path"
        );
    }

    /// R3.d + R4: an idle connection that both expired (sat idle longer than the TTL)
    /// and was closed by the upstream is discarded; the next request is served by the
    /// raw path on a fresh connection. Driven through a pool with a tiny TTL, since the
    /// global pool's TTL is a minute.
    #[tokio::test]
    async fn expired_and_closed_idle_connection_is_replaced_by_a_fresh_raw_connection() {
        let pool = UpstreamPool::new(PoolConfig {
            idle_ttl: Duration::from_millis(60),
            ..PoolConfig::default()
        });
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 200 OK",
            headers: Vec::new(),
            body: Body::Fixed(b"expired".to_vec()),
            after: AfterReply::CloseAfter(Duration::from_millis(200)),
        })
        .await;
        let key = pool_key_for(&server);

        assert_eq!(&raw_exchange(&pool, &server).await[..], b"expired");
        assert_served_by_raw_path(&server, 0).await;
        wait_for("the response to be pooled", || pool.idle_count(&key) == 1).await;
        // Past both the TTL and the upstream's delayed close.
        tokio::time::sleep(Duration::from_millis(450)).await;

        assert_eq!(&raw_exchange(&pool, &server).await[..], b"expired");
        assert_served_by_raw_path(&server, 1).await;
        assert_pooled_again(&pool, &key).await;
        assert_eq!(
            server.accepted(),
            2,
            "an expired, closed connection must not be reused"
        );
    }

    /// A response without a body (204) whose body is dropped unread still pools the
    /// connection: there was nothing left to read, so the connection is at a message
    /// boundary all the same.
    #[tokio::test]
    async fn status_without_body_dropped_unread_is_pooled() {
        let server = TestServer::start(|_| Reply {
            status_line: "HTTP/1.1 204 No Content",
            headers: Vec::new(),
            body: Body::Fixed(Vec::new()),
            after: AfterReply::KeepOpen,
        })
        .await;
        let url = server.url();
        let key = pool_key_for(&server);
        let pool = UpstreamPool::global();

        let response = send_raw(&url, None).await.expect("request failed");
        assert_eq!(response.status(), http::StatusCode::NO_CONTENT);
        drop(response);
        wait_for("the bodyless response to be pooled", || {
            pool.idle_count(&key) == 1
        })
        .await;

        // The second response is dropped unread as well, and still ends up pooled.
        let reused = send_raw(&url, None).await.expect("second request failed");
        assert_eq!(reused.status(), http::StatusCode::NO_CONTENT);
        drop(reused);
        assert_served_by_raw_path(&server, 1).await;
        assert_pooled_again(&pool, &key).await;
        assert_eq!(
            server.accepted(),
            1,
            "a bodyless response dropped unread must be reused"
        );
    }

    #[tokio::test]
    async fn connect_via_proxy_decodes_percent_encoded_credentials() {
        use base64::Engine;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut auth_header = None;
            loop {
                let mut line = String::new();
                let n = reader.read_line(&mut line).await.unwrap();
                if n == 0 || line.trim().is_empty() {
                    break;
                }
                if let Some(rest) = line.strip_prefix("Proxy-Authorization: ") {
                    auth_header = Some(rest.trim().to_string());
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
            auth_header
        });

        let proxy_url = format!("http://user%40corp:p%40ss%3Aword%231@127.0.0.1:{port}");
        let _stream = connect_via_proxy(&proxy_url, "api.example.com", 443)
            .await
            .unwrap();
        let captured = server.await.unwrap().expect("Proxy-Authorization header");
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("user@corp:p@ss:word#1")
        );
        assert_eq!(
            captured, expected,
            "Proxy-Authorization must base64-encode percent-decoded user:pass"
        );
    }

    #[tokio::test]
    async fn connect_via_proxy_rejects_premature_eof_and_excessive_headers() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        async fn drain_connect_request(socket: &mut tokio::net::TcpStream) {
            let mut reader = BufReader::new(socket);
            loop {
                let mut line = String::new();
                let n = reader.read_line(&mut line).await.unwrap_or(0);
                if n == 0 || line.trim().is_empty() {
                    break;
                }
            }
        }

        // 1. Proxy sends 200 status line then closes socket without blank header terminator.
        let eof_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let eof_port = eof_listener.local_addr().unwrap().port();
        let eof_server = tokio::spawn(async move {
            let (mut socket, _) = eof_listener.accept().await.unwrap();
            drain_connect_request(&mut socket).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\nX-Test: 1\r\n")
                .await;
            let _ = socket.shutdown().await;
        });
        let eof_res = connect_via_proxy(
            &format!("http://127.0.0.1:{eof_port}"),
            "api.example.com",
            443,
        )
        .await;
        let _ = eof_server.await;
        assert!(
            eof_res.is_err(),
            "CONNECT response ending at EOF before blank line must be rejected"
        );

        // 2. Proxy floods excessive CONNECT response headers.
        let flood_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let flood_port = flood_listener.local_addr().unwrap().port();
        let flood_server = tokio::spawn(async move {
            let (mut socket, _) = flood_listener.accept().await.unwrap();
            drain_connect_request(&mut socket).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 Connection Established\r\n")
                .await;
            for i in 0..200 {
                if socket
                    .write_all(format!("X-Flood-{i}: value\r\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
            let _ = socket.write_all(b"\r\n").await;
        });
        let flood_res = connect_via_proxy(
            &format!("http://127.0.0.1:{flood_port}"),
            "api.example.com",
            443,
        )
        .await;
        let _ = flood_server.await;
        assert!(
            flood_res.is_err(),
            "CONNECT response with excessive header lines must be rejected"
        );

        // 3. Proxy sends an oversized single header line (> MAX_CONNECT_HEADER_BYTES).
        let huge_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let huge_port = huge_listener.local_addr().unwrap().port();
        let huge_server = tokio::spawn(async move {
            let (mut socket, _) = huge_listener.accept().await.unwrap();
            drain_connect_request(&mut socket).await;
            let mut payload = b"HTTP/1.1 200 OK\r\nX-Huge: ".to_vec();
            payload.resize(MAX_CONNECT_HEADER_BYTES + 1024, b'A');
            payload.extend_from_slice(b"\r\n\r\n");
            let _ = socket.write_all(&payload).await;
        });
        let huge_res = connect_via_proxy(
            &format!("http://127.0.0.1:{huge_port}"),
            "api.example.com",
            443,
        )
        .await;
        let _ = huge_server.await;
        assert!(
            huge_res.is_err(),
            "CONNECT response exceeding total header byte limit must be rejected"
        );
    }
}
