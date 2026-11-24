//! Keep-alive pool for the raw-write upstream path (`hyper_client::send_raw_request`).
//!
//! That path writes the request bytes itself so the client's original header casing and
//! order reach the upstream unchanged, and lets hyper parse only the response. hyper's
//! `Connection` owns the stream while it runs and used to drop it once the response was
//! over, so every raw request rebuilt TCP and TLS. Now the stream is handed back to this
//! pool instead and reused by the next request to the same target.
//!
//! A stream returns to the pool only when it is known to sit exactly at a message
//! boundary: the consumer read the whole response body, the exchange allows keep-alive,
//! no I/O error or EOF was seen and hyper's read buffer is empty (`drive_connection`).
//! Everything else closes the stream.
//!
//! Idle entries expire on either monotonic or wall time so a suspend cannot hide
//! elapsed idle time. An upstream `Keep-Alive` timeout tightens the local limit,
//! with one second of margin before the peer is expected to close.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};

use super::hyper_client::{ProxyResponse, ProxyStream};
use super::ProxyError;
use bytes::Bytes;
use futures::task::AtomicWaker;
use futures::FutureExt;
use hyper::body::{Body, Frame, Incoming, SizeHint};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::Notify;

/// Which upstream connections may be shared: only requests with the same scheme, host,
/// port and proxy URL reuse each other's connections.
///
/// Deliberately without `Debug`: the proxy URL may carry credentials, so a key is never
/// formatted or logged.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct PoolKey {
    scheme: String,
    host: String,
    port: u16,
    proxy: Option<String>,
}

impl PoolKey {
    pub(crate) fn new(scheme: &str, host: &str, port: u16, proxy: Option<&str>) -> Self {
        Self {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            port,
            proxy: proxy.map(str::to_string),
        }
    }
}

/// Bounds of the idle pool.
///
/// The values are deliberately conservative: a pooled connection that the upstream
/// meanwhile closed makes a request fail where a fresh connection would have worked, so
/// it pays to hold fewer and younger connections than to squeeze out every reuse.
#[derive(Clone, Copy)]
pub(crate) struct PoolConfig {
    /// Idle connections kept per key. Four covers a session's requests that are in
    /// flight at the same time; the rest of a burst is closed instead of kept.
    pub(crate) max_idle_per_key: usize,
    /// Idle connections kept across all keys. Bounds the sockets the proxy holds open
    /// to upstreams nobody is currently using.
    pub(crate) max_idle_total: usize,
    /// How long a connection may sit unused. Bridges the gaps of a tool-use loop
    /// (seconds); anything idle longer is closed rather than risk a server-side idle
    /// timeout we cannot see.
    pub(crate) idle_ttl: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_key: 4,
            max_idle_total: 64,
            idle_ttl: Duration::from_secs(30),
        }
    }
}

/// Idle upstream connections, keyed by target. Cheap to clone: the state is shared.
#[derive(Clone)]
pub(crate) struct UpstreamPool {
    config: PoolConfig,
    shared: Arc<Shared>,
}

struct Shared {
    /// Guards the pool state. Never held across an `.await`.
    pool: Mutex<PoolState>,
}

#[derive(Default)]
struct PoolState {
    by_key: HashMap<PoolKey, VecDeque<IdleEntry>>,
    /// Sum of all queue lengths.
    total: usize,
    /// The sweeper task, if one is running. A dead `Weak` means the task is gone and a
    /// new one must be started on the next `put`.
    sweeper: Option<std::sync::Weak<SweeperToken>>,
}

struct IdleEntry {
    stream: PooledStream,
    idle_since: Instant,
    idle_since_wall: SystemTime,
    idle_ttl: Duration,
}

impl IdleEntry {
    fn is_expired(&self, now: Instant, wall_now: SystemTime) -> bool {
        // Instant can stop during system suspend. Wall time catches that gap;
        // a backward clock adjustment still leaves the monotonic limit in force.
        now.duration_since(self.idle_since) >= self.idle_ttl
            || wall_now
                .duration_since(self.idle_since_wall)
                .is_ok_and(|elapsed| elapsed >= self.idle_ttl)
    }
}

/// Marker owned by the running sweeper task; dropping it retires the task.
struct SweeperToken;

impl UpstreamPool {
    pub(crate) fn new(config: PoolConfig) -> Self {
        Self {
            config,
            shared: Arc::new(Shared {
                pool: Mutex::new(PoolState::default()),
            }),
        }
    }

    /// The pool the raw upstream path uses.
    pub(crate) fn global() -> Self {
        static POOL: LazyLock<UpstreamPool> =
            LazyLock::new(|| UpstreamPool::new(PoolConfig::default()));
        POOL.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.shared
            .pool
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Take an idle connection for `key`, if the pool has one that is still usable.
    pub(crate) fn take(&self, key: &PoolKey) -> Taken {
        while let Some(mut entry) = self.pop_newest(key) {
            let reason = if entry.is_expired(Instant::now(), SystemTime::now()) {
                Some("idle longer than the TTL")
            } else {
                unusable_reason(&mut entry.stream)
            };
            match reason {
                None => return Taken::Idle(entry.stream),
                Some(reason) => {
                    log::debug!("[UpstreamPool] idle upstream connection discarded: {reason}");
                    close_stream(entry.stream);
                }
            }
        }
        Taken::None
    }

    /// Pop the connection that went idle last. It is the youngest, so the one most
    /// likely to still be alive.
    fn pop_newest(&self, key: &PoolKey) -> Option<IdleEntry> {
        let mut pool = self.lock();
        let state = &mut *pool;
        let queue = state.by_key.get_mut(key)?;
        let entry = queue.pop_back()?;
        let empty = queue.is_empty();
        if empty {
            state.by_key.remove(key);
        }
        state.total -= 1;
        Some(entry)
    }

    /// Return a connection whose response ended cleanly. Connections made surplus by
    /// the caps, and expired ones, are closed.
    fn put(&self, key: PoolKey, stream: PooledStream, idle_limit: Duration) {
        let mut evicted = Vec::new();
        let token = {
            let mut pool = self.lock();
            let state = &mut *pool;
            let now = Instant::now();
            let wall_now = SystemTime::now();
            state.remove_expired(now, wall_now, &mut evicted);
            let queue = state.by_key.entry(key).or_default();
            queue.push_back(IdleEntry {
                stream,
                idle_since: now,
                idle_since_wall: wall_now,
                idle_ttl: self.config.idle_ttl.min(idle_limit),
            });
            state.total += 1;
            while queue.len() > self.config.max_idle_per_key {
                if let Some(entry) = queue.pop_front() {
                    evicted.push(entry);
                    state.total -= 1;
                }
            }
            while state.total > self.config.max_idle_total {
                if let Some(entry) = state.pop_oldest() {
                    evicted.push(entry);
                }
            }
            let dead_sweeper = state
                .sweeper
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .is_none();
            // The task owns the only strong reference: when it exits, the weak dies with
            // it and the next `put` starts a new sweeper.
            dead_sweeper.then(|| {
                let token = Arc::new(SweeperToken);
                state.sweeper = Some(Arc::downgrade(&token));
                token
            })
        };
        for entry in evicted {
            close_stream(entry.stream);
        }
        if let Some(token) = token {
            self.spawn_sweeper(token);
        }
    }

    /// Close expired idle connections without waiting for the next request. The task
    /// exits once the pool is empty, which also retires its token.
    fn spawn_sweeper(&self, token: Arc<SweeperToken>) {
        let shared = self.shared.clone();
        let ttl = self.config.idle_ttl;
        tokio::spawn(async move {
            let _token = token;
            loop {
                tokio::time::sleep((ttl / 4).max(Duration::from_millis(1))).await;
                let mut expired = Vec::new();
                let stop = {
                    let mut pool = shared.pool.lock().unwrap_or_else(PoisonError::into_inner);
                    pool.remove_expired(Instant::now(), SystemTime::now(), &mut expired);
                    if pool.total == 0 {
                        pool.sweeper = None;
                        true
                    } else {
                        false
                    }
                };
                for entry in expired {
                    close_stream(entry.stream);
                }
                if stop {
                    return;
                }
            }
        });
    }

    /// How many idle connections are kept for `key`.
    #[cfg(test)]
    pub(crate) fn idle_count(&self, key: &PoolKey) -> usize {
        self.lock().by_key.get(key).map_or(0, VecDeque::len)
    }

    /// The default per-key cap, for tests that assert against the global pool.
    #[cfg(test)]
    pub(crate) fn per_key_cap() -> usize {
        PoolConfig::default().max_idle_per_key
    }
}

impl PoolState {
    /// Drain expired entries. Each response can set a different idle limit, so
    /// expired entries need not be at the front of a queue.
    fn remove_expired(&mut self, now: Instant, wall_now: SystemTime, expired: &mut Vec<IdleEntry>) {
        let total = &mut self.total;
        self.by_key.retain(|_, queue| {
            for _ in 0..queue.len() {
                let entry = queue.pop_front().expect("one entry per iteration");
                if entry.is_expired(now, wall_now) {
                    expired.push(entry);
                    *total -= 1;
                } else {
                    queue.push_back(entry);
                }
            }
            !queue.is_empty()
        });
    }

    /// Remove and return the connection that has been idle the longest, across all keys.
    fn pop_oldest(&mut self) -> Option<IdleEntry> {
        let key = self
            .by_key
            .iter()
            .min_by_key(|(_, queue)| queue.front().map(|entry| entry.idle_since))?
            .0
            .clone();
        let entry = self.by_key.get_mut(&key)?.pop_front()?;
        if self.by_key[&key].is_empty() {
            self.by_key.remove(&key);
        }
        self.total -= 1;
        Some(entry)
    }
}

/// What [`UpstreamPool::take`] found.
pub(crate) enum Taken {
    /// A usable idle connection.
    Idle(PooledStream),
    /// No usable idle connection; the caller opens a new one.
    None,
}

/// Observe accepted bytes below both TLS layers. A TLS write can send records
/// and then return an error without reporting that progress.
pub(crate) struct ProxySocket {
    tcp: tokio::net::TcpStream,
    wrote_bytes: bool,
}

impl ProxySocket {
    pub(crate) fn new(tcp: tokio::net::TcpStream) -> Self {
        Self {
            tcp,
            wrote_bytes: false,
        }
    }
}

impl AsyncRead for ProxySocket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().tcp).poll_read(cx, buf)
    }
}

impl AsyncWrite for ProxySocket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let socket = self.get_mut();
        let result = Pin::new(&mut socket.tcp).poll_write(cx, buf);
        if matches!(result, Poll::Ready(Ok(n)) if n > 0) {
            socket.wrote_bytes = true;
        }
        result
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let socket = self.get_mut();
        let result = Pin::new(&mut socket.tcp).poll_write_vectored(cx, bufs);
        if matches!(result, Poll::Ready(Ok(n)) if n > 0) {
            socket.wrote_bytes = true;
        }
        result
    }

    fn is_write_vectored(&self) -> bool {
        self.tcp.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().tcp).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().tcp).poll_shutdown(cx)
    }
}

/// An upstream connection as the pool holds it: plaintext or TLS, direct or through an
/// HTTP CONNECT tunnel.
pub(crate) enum PooledStream {
    Plain(ProxyStream),
    Tls(Box<tokio_rustls::client::TlsStream<ProxyStream>>),
}

impl PooledStream {
    fn socket_mut(&mut self) -> &mut ProxySocket {
        match self {
            Self::Plain(stream) => stream.socket_mut(),
            Self::Tls(stream) => stream.get_mut().0.socket_mut(),
        }
    }
}

impl AsyncRead for PooledStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for PooledStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(cx),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Why an idle connection must not be used.
///
/// While a connection sits in the pool nobody reads from it, so anything the probe
/// finds was unsolicited: EOF means the upstream closed it, data means the connection
/// is not at a message boundary, an error means it is broken. Only "nothing to read"
/// is good.
///
/// The probe never blocks, and it cannot take bytes that a healthy connection still
/// needs: no request is in flight on a pooled connection, so there is no response left
/// to read. Whatever it reads is dropped together with the connection. On the TLS
/// variant it goes through the TLS layer, so a bare handshake record (e.g. a session
/// ticket) is consumed by rustls and still counts as nothing to read.
fn unusable_reason(stream: &mut PooledStream) -> Option<&'static str> {
    let mut scratch = [0u8; 1];
    let mut buf = ReadBuf::new(&mut scratch);
    // Budget exhaustion also returns Pending, but without checking socket readiness.
    let read = tokio::task::coop::unconstrained(std::future::poll_fn(|cx| {
        Pin::new(&mut *stream).poll_read(cx, &mut buf)
    }))
    .now_or_never();
    match read {
        None => None,
        Some(Ok(())) if buf.filled().is_empty() => Some("closed by the upstream"),
        Some(Ok(())) => Some("unexpected pending bytes"),
        Some(Err(e)) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        Some(Err(_)) => Some("read error"),
    }
}

/// Close a connection that will not be reused. `shutdown` sends the TLS `close_notify`
/// alert before the socket closes; a peer that stopped reading must not pin the task,
/// hence the timeout.
fn close_stream(mut stream: PooledStream) {
    tokio::spawn(async move {
        let _ = tokio::time::timeout(Duration::from_secs(5), stream.shutdown()).await;
    });
}

/// The request as the pool module needs it.
pub(crate) struct RawRequest<'a> {
    /// Exactly the bytes that must reach the upstream (`build_raw_request` output).
    pub(crate) bytes: &'a [u8],
    /// The request itself carries `Connection: close`, so the upstream will close the
    /// connection after responding whether or not the response says so.
    pub(crate) connection_close: bool,
}

#[derive(Debug)]
pub(crate) struct RawWriteError {
    pub(crate) error: ProxyError,
    pub(crate) wrote_bytes: bool,
}

/// Write the original request bytes. If writing fails, drop the socket without
/// flushing buffered TLS records or returning the socket to the pool.
pub(crate) async fn write_raw_request(
    pool: &UpstreamPool,
    key: &PoolKey,
    request: &RawRequest<'_>,
    mut stream: PooledStream,
) -> Result<PooledIo, RawWriteError> {
    stream.socket_mut().wrote_bytes = false;
    let failure = match stream.write_all(request.bytes).await {
        Err(error) => Some(("Write", error)),
        Ok(()) => stream.flush().await.err().map(|error| ("Flush", error)),
    };
    if let Some((stage, error)) = failure {
        return Err(RawWriteError {
            error: ProxyError::ForwardFailed(format!("{stage} failed: {error}")),
            wrote_bytes: stream.socket_mut().wrote_bytes,
        });
    }

    Ok(PooledIo {
        io: stream,
        read_enabled: false,
        pool: pool.clone(),
        key: key.clone(),
        state: Arc::new(ConnState {
            idle_limit_secs: AtomicU64::new(if request.connection_close {
                0
            } else {
                u64::MAX
            }),
            unusable: AtomicBool::new(false),
            body: AtomicU8::new(BODY_OPEN),
            body_settled: Notify::new(),
            driver_done: AtomicBool::new(false),
            driver_done_waker: AtomicWaker::new(),
        }),
    })
}

/// A `Connection` header carrying a `close` token forbids reuse, whichever side sent
/// it. On the request: hyper strips hop-by-hop headers when it encodes a request
/// itself, but the raw path sends the client's headers verbatim, so the upstream may
/// close the connection after answering even though the response says nothing about
/// it. On the response: `close` wins even when another `Connection` field advertises
/// keep-alive. Such a connection is never pooled.
pub(crate) fn connection_forbids_reuse(headers: &http::HeaderMap) -> bool {
    headers
        .get_all(http::header::CONNECTION)
        .iter()
        .any(|value| has_connection_token(value, "close"))
}

/// The IO hyper reads the response through.
///
/// hyper's own writes go nowhere (the request was already written raw), shutting hyper
/// down does not touch the connection, and reads pass through while recording whether
/// the connection broke. [`PooledIo::release`] returns the connection to the pool or
/// closes it; a wrapper dropped without a release just drops the connection.
pub(crate) struct PooledIo {
    io: PooledStream,
    read_enabled: bool,
    pool: UpstreamPool,
    key: PoolKey,
    state: Arc<ConnState>,
}

impl PooledIo {
    /// The state the response body reports to and the driver decides from.
    pub(crate) fn connection_state(&self) -> Arc<ConnState> {
        self.state.clone()
    }

    /// Hand the connection back: to the pool when it is reusable, otherwise closed.
    fn release(self, reusable: bool) {
        if reusable {
            let idle_limit =
                Duration::from_secs(self.state.idle_limit_secs.load(Ordering::Acquire));
            self.pool.put(self.key, self.io, idle_limit);
        } else {
            close_stream(self.io);
        }
    }
}

impl AsyncRead for PooledIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        // Read gate. The request bytes went to the wire before hyper started, so a
        // fast upstream's response can already be waiting on the socket while hyper
        // still has no request in flight. hyper polls read before write, and data
        // arriving with nothing in flight makes it reject the exchange as
        // `UnexpectedMessage` (a "Response parse failed" upstream) — a race the raw
        // path has always had. Until hyper has encoded its dummy request, reads return
        // Pending without touching the transport. `do_hyper_response` queues that
        // request synchronously before the driver starts, so hyper's first `poll_loop`
        // pass writes it (which opens the gate and self-wakes) even though the read
        // before it stayed Pending: a healthy exchange cannot stall on the gate.
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if !this.read_enabled {
            return Poll::Pending;
        }
        let before = buf.filled().len();
        let result = Pin::new(&mut this.io).poll_read(cx, buf);
        match result {
            // An empty successful read is EOF: the connection is gone for reuse, whether
            // hyper still needs it to finish a body or not.
            Poll::Ready(Ok(())) if buf.filled().len() == before => {
                this.state.unusable.store(true, Ordering::Release);
            }
            Poll::Ready(Err(_)) => {
                this.state.unusable.store(true, Ordering::Release);
            }
            _ => {}
        }
        result
    }
}

impl AsyncWrite for PooledIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // The dummy request is now in flight in hyper. Wake its previously
        // gated read. No dummy bytes, flushes or shutdowns reach the transport.
        let this = self.get_mut();
        if !this.read_enabled && !buf.is_empty() {
            this.read_enabled = true;
            cx.waker().wake_by_ref();
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // hyper "shutting down" must not close the connection: reuse is decided later.
        Poll::Ready(Ok(()))
    }
}

const BODY_OPEN: u8 = 0;
const BODY_READ_TO_END: u8 = 1;
const BODY_ABANDONED: u8 = 2;

/// What one raw exchange knows about its connection, shared by the IO hyper reads
/// through, the response body the consumer reads, and the driver task that decides
/// between pooling and closing.
pub(crate) struct ConnState {
    /// Zero forbids reuse; `u64::MAX` leaves the idle limit up to the local pool.
    idle_limit_secs: AtomicU64,
    /// The connection broke (read error or EOF seen by hyper's reads).
    unusable: AtomicBool,
    /// How the response body ended. The first verdict wins.
    body: AtomicU8,
    /// Fired with the first body verdict, for the driver that waits on it.
    body_settled: Notify,
    driver_done: AtomicBool,
    driver_done_waker: AtomicWaker,
}

impl ConnState {
    fn body_outcome(&self) -> u8 {
        self.body.load(Ordering::Acquire)
    }

    /// Record how the response body ended. The first verdict wins; the driver is woken.
    fn settle_body(&self, outcome: u8) {
        if self
            .body
            .compare_exchange(BODY_OPEN, outcome, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.body_settled.notify_one();
        }
    }
}

/// `hyper::body::Incoming` that reports how it ended to the exchange's driver.
///
/// The driver cannot tell a response that was read to the end from one whose body was
/// dropped halfway: hyper's `Connection` future finishes with `Ok(())` in both cases
/// (see `drive_connection`). This wrapper records what the consumer actually did.
pub struct TrackedBody {
    inner: Incoming,
    /// `None` for responses that did not come from the raw path.
    state: Option<Arc<ConnState>>,
}

impl TrackedBody {
    /// Wrap a body that is not tied to a pooled connection (hyper-util fallback path).
    pub(crate) fn untracked(inner: Incoming) -> Self {
        Self { inner, state: None }
    }
}

impl Body for TrackedBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, hyper::Error>>> {
        let this = self.get_mut();
        let poll = match &this.state {
            Some(state) if state.body_outcome() == BODY_READ_TO_END => Poll::Ready(None),
            _ => Pin::new(&mut this.inner).poll_frame(cx),
        };
        if let Some(state) = &this.state {
            match &poll {
                Poll::Ready(None) => {
                    state.settle_body(BODY_READ_TO_END);
                    // Returning EOF hands control back to the caller. Wait for the
                    // driver to finish pooling or closing first, so that an
                    // immediate next request can reuse the connection when it stayed
                    // healthy and no concurrent request took it.
                    if !state.driver_done.load(Ordering::Acquire) {
                        state.driver_done_waker.register(cx.waker());
                        if !state.driver_done.load(Ordering::Acquire) {
                            return Poll::Pending;
                        }
                        state.driver_done_waker.take();
                    }
                }
                // hyper closes a connection after trailers instead of going idle.
                Poll::Ready(Some(Ok(frame))) if frame.is_trailers() => {
                    state.settle_body(BODY_ABANDONED)
                }
                Poll::Ready(Some(Err(_))) => state.settle_body(BODY_ABANDONED),
                Poll::Pending | Poll::Ready(Some(Ok(_))) => {}
            }
        }
        poll
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for TrackedBody {
    /// Dropped without the consumer seeing the end of the stream. That is harmless when
    /// nothing was left to read anyway (a response without a body: 204, 304, HEAD, 1xx):
    /// `Incoming::is_end_stream()` is then true and hyper has nothing pending. Anything
    /// else means the consumer stopped reading mid-body, so the connection is not at a
    /// message boundary.
    fn drop(&mut self) {
        if let Some(state) = &self.state {
            let outcome = if self.inner.is_end_stream() {
                BODY_READ_TO_END
            } else {
                BODY_ABANDONED
            };
            state.settle_body(outcome);
        }
    }
}

/// hyper's connection over a pooled stream, driven by [`spawn_connection_driver`].
pub(crate) type RawConnection = hyper::client::conn::http1::Connection<
    hyper_util::rt::TokioIo<PooledIo>,
    http_body_util::Full<Bytes>,
>;

/// Handle on the exchange's driver, held while waiting for the response head.
pub(crate) struct ConnectionHandle {
    state: Option<Arc<ConnState>>,
}

impl ConnectionHandle {
    /// Attach the tracking to a response head that arrived, and record from the head
    /// whether the upstream keeps the connection open.
    pub(crate) fn tracked_response(
        mut self,
        response: hyper::Response<Incoming>,
        method: &http::Method,
    ) -> ProxyResponse {
        let Some(state) = self.state.take() else {
            unreachable!("tracked_response is called once");
        };
        if state.idle_limit_secs.load(Ordering::Relaxed) != 0 {
            let idle_limit = if response_allows_reuse(&response, method) {
                keep_alive_timeout(response.headers()).unwrap_or(u64::MAX)
            } else {
                0
            };
            state.idle_limit_secs.store(idle_limit, Ordering::Release);
        }
        ProxyResponse::Hyper(response.map(|body| TrackedBody {
            inner: body,
            state: Some(state),
        }))
    }
}

impl Drop for ConnectionHandle {
    /// Dropped without a response (parse error, or cancelled while waiting for the
    /// head): nobody will ever read a body, so the driver must not wait for one.
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            state.settle_body(BODY_ABANDONED);
        }
    }
}

/// Start the task that owns hyper's `Connection` and hands out the response handle.
pub(crate) fn spawn_connection_driver(
    conn: RawConnection,
    state: Arc<ConnState>,
) -> ConnectionHandle {
    let completion = DriverCompletion(state.clone());
    tokio::spawn(drive_connection(conn, completion));
    ConnectionHandle { state: Some(state) }
}

/// Wake an EOF waiter on normal release, panic or task cancellation.
struct DriverCompletion(Arc<ConnState>);

impl Drop for DriverCompletion {
    fn drop(&mut self) {
        self.0.driver_done.store(true, Ordering::Release);
        self.0.driver_done_waker.wake();
    }
}

/// Run hyper's connection until the response is over, then pool or close the stream.
///
/// Both ends have to report before the stream can be judged:
/// - hyper's `Connection` finishes with `Ok(())` once the response is over — whether the
///   body was read to the end, dropped halfway, or cut short by an error (its dispatcher
///   closes itself the moment it notices the dropped request channel or the body
///   cancellation; `proto/h1/dispatch.rs`). An `Err` means the connection is broken.
/// - the consumer settles the body through [`TrackedBody`]: read to the end, or given
///   up on.
async fn drive_connection(mut conn: RawConnection, completion: DriverCompletion) {
    let state = &completion.0;
    let mut hyper_result = None;
    let body_outcome = {
        let settled = state.body_settled.notified();
        tokio::pin!(settled);
        std::future::poll_fn(|cx| {
            if hyper_result.is_none() {
                if let Poll::Ready(result) = Pin::new(&mut conn).poll(cx) {
                    hyper_result = Some(result);
                }
            }
            let _ = settled.as_mut().poll(cx);
            match state.body_outcome() {
                BODY_OPEN => Poll::Pending,
                outcome => Poll::Ready(outcome),
            }
        })
        .await
    };

    let hyper_result = if body_outcome == BODY_READ_TO_END {
        Some(match hyper_result {
            Some(result) => Ok(result),
            None => tokio::time::timeout(Duration::from_secs(5), &mut conn).await,
        })
    } else {
        None
    };
    let parts = conn.into_parts();
    let reusable = match hyper_result {
        Some(Ok(Ok(()))) => match reuse_blocker(state, parts.read_buf.is_empty()) {
            Ok(()) => true,
            Err(reason) => {
                log::debug!(
                    "[UpstreamPool] upstream connection closed instead of pooled: {reason}"
                );
                false
            }
        },
        Some(Ok(Err(error))) => {
            log::debug!("[UpstreamPool] hyper connection error after the response: {error}");
            false
        }
        Some(Err(_)) => {
            log::debug!("[UpstreamPool] hyper did not finish after the response body ended");
            false
        }
        None => false,
    };
    parts.io.into_inner().release(reusable);
}

/// Why a connection whose response ended without complaint still cannot be reused.
fn reuse_blocker(state: &ConnState, read_buf_empty: bool) -> Result<(), &'static str> {
    if state.unusable.load(Ordering::Acquire) {
        Err("I/O error or EOF on the connection")
    } else if state.idle_limit_secs.load(Ordering::Acquire) == 0 {
        Err("the exchange does not allow keep-alive")
    } else if !read_buf_empty {
        Err("bytes left over after the response")
    } else {
        Ok(())
    }
}

/// Decide from a response head whether the connection can carry another request.
///
/// Follows hyper's body framing rules, but always honors a `Connection: close`
/// token even when another field advertises keep-alive.
fn response_allows_reuse(response: &hyper::Response<Incoming>, method: &http::Method) -> bool {
    let headers = response.headers();
    let status = response.status();

    // Upgraded connections speak another protocol now.
    if status == http::StatusCode::SWITCHING_PROTOCOLS
        || (*method == http::Method::CONNECT && status.is_success())
    {
        return false;
    }

    if connection_forbids_reuse(headers) {
        return false;
    }
    match response.version() {
        http::Version::HTTP_11 => {}
        http::Version::HTTP_10
            if headers
                .get_all(http::header::CONNECTION)
                .iter()
                .any(|value| has_connection_token(value, "keep-alive")) => {}
        _ => return false,
    }

    // HEAD and statuses with no body ignore framing headers. Otherwise, hyper
    // checks Transfer-Encoding first: if it isn't chunked, EOF delimits the body.
    let no_body = status.is_informational()
        || status == http::StatusCode::NO_CONTENT
        || status == http::StatusCode::NOT_MODIFIED
        || *method == http::Method::HEAD;
    no_body
        || if headers.contains_key(http::header::TRANSFER_ENCODING) {
            is_chunked(headers)
        } else {
            headers.contains_key(http::header::CONTENT_LENGTH)
        }
}

/// Honor the shortest valid upstream idle hint, with one second of margin for
/// the gap between the peer's idle timer and local pooling.
fn keep_alive_timeout(headers: &http::HeaderMap) -> Option<u64> {
    headers
        .get_all("keep-alive")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|param| {
            let (name, value) = param.split_once('=')?;
            if name.trim().eq_ignore_ascii_case("timeout") {
                value.trim().parse::<u64>().ok()
            } else {
                None
            }
        })
        .min()
        .map(|seconds| seconds.saturating_sub(1))
}

/// `value` is one `Connection` header value: does it carry `token`?
/// Same token splitting as hyper's `headers::connection_has`.
fn has_connection_token(value: &http::HeaderValue, token: &str) -> bool {
    value
        .to_str()
        .map(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
        .unwrap_or(false)
}

/// Is the response body chunked? Like hyper, only the last `Transfer-Encoding` header
/// and its last token count: chunked must be the final encoding.
fn is_chunked(headers: &http::HeaderMap) -> bool {
    headers
        .get_all(http::header::TRANSFER_ENCODING)
        .iter()
        .next_back()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .is_some_and(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{PooledStream, ProxyStream};
    use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    pub(crate) async fn put_idle_with_closed_write_half(
        pool: &super::UpstreamPool,
        key: super::PoolKey,
        mut stream: PooledStream,
    ) {
        stream.socket_mut().shutdown().await.unwrap();
        assert_eq!(super::unusable_reason(&mut stream), None);
        pool.put(key, stream, Duration::MAX);
    }

    /// What the test upstream sends for one request.
    #[derive(Clone)]
    pub(crate) struct Reply {
        /// Status line without the trailing CRLF, e.g. `HTTP/1.1 200 OK`.
        pub status_line: &'static str,
        /// Extra header lines without CRLF, e.g. `Connection: close`.
        pub headers: Vec<String>,
        pub body: Body,
        /// What happens once the reply is written.
        pub after: AfterReply,
    }

    #[derive(Clone)]
    pub(crate) enum Body {
        /// Sent with a `Content-Length` header.
        Fixed(Vec<u8>),
        /// Sent as chunks under `Transfer-Encoding: chunked`, waiting `delay` before
        /// each chunk.
        Chunked(Vec<Vec<u8>>, Duration),
        /// Sent without any length information; only closing the connection delimits it.
        UntilClose(Vec<u8>),
        /// Sent under `Content-Length: declared`, but only `sent` bytes follow: the body
        /// never completes, so the consumer sees a body error instead of an end.
        Short(usize, Vec<u8>),
    }

    #[derive(Clone)]
    pub(crate) enum AfterReply {
        /// Stay open for the next request on this connection.
        KeepOpen,
        /// Close the socket.
        Close,
        /// Close the socket after `delay`, once the reply is likely pooled: the
        /// connection then dies while it sits idle in the pool.
        CloseAfter(Duration),
        /// Send unsolicited bytes, then stay open.
        Send(Vec<u8>),
    }

    impl Reply {
        /// `200 OK` with a `Content-Length` body, connection left open.
        pub(crate) fn fixed(body: impl Into<Vec<u8>>) -> Self {
            Self {
                status_line: "HTTP/1.1 200 OK",
                headers: Vec::new(),
                body: Body::Fixed(body.into()),
                after: AfterReply::KeepOpen,
            }
        }
    }

    /// One request the test upstream received.
    pub(crate) struct CapturedRequest {
        /// 1-based order of arrival across all connections.
        pub index: usize,
        /// The exact bytes of the request (head and body).
        pub bytes: Vec<u8>,
    }

    /// A local HTTP/1.1 upstream with one task per accepted connection.
    pub(crate) struct TestServer {
        port: u16,
        accepted: Arc<AtomicUsize>,
        open: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl TestServer {
        /// Start an upstream that answers each request with `reply(request)`.
        pub(crate) async fn start(
            reply: impl Fn(&CapturedRequest) -> Reply + Send + Sync + 'static,
        ) -> Self {
            Self::start_script(move |request| Some(reply(request))).await
        }

        pub(crate) async fn start_tls(
            reply: impl Fn(&CapturedRequest) -> Reply + Send + Sync + 'static,
        ) -> Self {
            let acceptor = tokio_rustls::TlsAcceptor::from(tls_server_config());
            Self::start_with_tls(Some(acceptor), move |request| Some(reply(request))).await
        }

        /// Returning no reply closes the connection after capturing the request.
        pub(crate) async fn start_script(
            reply: impl Fn(&CapturedRequest) -> Option<Reply> + Send + Sync + 'static,
        ) -> Self {
            Self::start_with_tls(None, reply).await
        }

        async fn start_with_tls(
            acceptor: Option<tokio_rustls::TlsAcceptor>,
            reply: impl Fn(&CapturedRequest) -> Option<Reply> + Send + Sync + 'static,
        ) -> Self {
            let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("bind test upstream");
            let port = listener.local_addr().unwrap().port();
            let accepted = Arc::new(AtomicUsize::new(0));
            let open = Arc::new(AtomicUsize::new(0));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let reply = Arc::new(reply);

            let accepted_task = accepted.clone();
            let open_task = open.clone();
            let requests_task = requests.clone();
            tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    accepted_task.fetch_add(1, Ordering::SeqCst);
                    open_task.fetch_add(1, Ordering::SeqCst);
                    let open_socket = open_task.clone();
                    let requests_socket = requests_task.clone();
                    let reply_socket = reply.clone();
                    let acceptor = acceptor.clone();
                    tokio::spawn(async move {
                        if let Some(acceptor) = acceptor {
                            if let Ok(socket) = acceptor.accept(socket).await {
                                serve_requests(socket, &requests_socket, reply_socket.as_ref())
                                    .await;
                            }
                        } else {
                            serve_requests(socket, &requests_socket, reply_socket.as_ref()).await;
                        }
                        open_socket.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            });

            Self {
                port,
                accepted,
                open,
                requests,
            }
        }

        pub(crate) fn port(&self) -> u16 {
            self.port
        }

        pub(crate) fn url(&self) -> String {
            format!("http://127.0.0.1:{}/v1/messages", self.port)
        }

        /// How many connections this upstream accepted so far.
        pub(crate) fn accepted(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }

        /// How many connections are still open from this upstream's point of view.
        pub(crate) fn open_connections(&self) -> usize {
            self.open.load(Ordering::SeqCst)
        }

        /// The raw bytes of every request received so far.
        pub(crate) fn requests(&self) -> Vec<Vec<u8>> {
            self.requests.lock().unwrap().clone()
        }

        /// Wait until `count` requests have arrived, then return them.
        pub(crate) async fn wait_for_requests(&self, count: usize) -> Vec<Vec<u8>> {
            for _ in 0..400 {
                let requests = self.requests();
                if requests.len() >= count {
                    return requests;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!(
                "test upstream received {} requests, expected {count}",
                self.requests().len()
            );
        }
    }

    async fn serve_requests(
        mut socket: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
        requests: &Mutex<Vec<Vec<u8>>>,
        reply: &impl Fn(&CapturedRequest) -> Option<Reply>,
    ) {
        while let Some(bytes) = read_request(&mut socket).await {
            let captured = {
                let mut logged = requests.lock().unwrap();
                let captured = CapturedRequest {
                    index: logged.len() + 1,
                    bytes: bytes.clone(),
                };
                logged.push(bytes);
                captured
            };
            let Some(reply) = reply(&captured) else {
                break;
            };
            write_reply(&mut socket, &reply).await;
            match reply.after {
                AfterReply::KeepOpen | AfterReply::Send(_) => {}
                AfterReply::Close | AfterReply::CloseAfter(_) => break,
            }
        }
    }

    /// Read one HTTP/1.1 request: head plus a `Content-Length` body.
    /// `None` when the peer closed or the request is not parseable.
    async fn read_request(socket: &mut (impl tokio::io::AsyncRead + Unpin)) -> Option<Vec<u8>> {
        let mut buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
                break pos;
            }
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 64 * 1024 {
                return None;
            }
        };
        let content_length = parse_content_length(&buf[..head_end]).unwrap_or(0);
        let total = head_end + 4 + content_length;
        while buf.len() < total {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(buf)
    }

    fn parse_content_length(head: &[u8]) -> Option<usize> {
        let text = String::from_utf8_lossy(head).to_ascii_lowercase();
        for line in text.split("\r\n") {
            if let Some(value) = line.strip_prefix("content-length:") {
                return value.trim().parse().ok();
            }
        }
        None
    }

    async fn write_reply(socket: &mut (impl tokio::io::AsyncWrite + Unpin), reply: &Reply) {
        let mut head = format!("{}\r\n", reply.status_line);
        match &reply.body {
            Body::Fixed(bytes) => {
                head.push_str(&format!("Content-Length: {}\r\n", bytes.len()));
            }
            Body::Chunked(..) => head.push_str("Transfer-Encoding: chunked\r\n"),
            Body::UntilClose(..) => {}
            Body::Short(declared, _) => {
                head.push_str(&format!("Content-Length: {declared}\r\n"));
            }
        }
        for header in &reply.headers {
            head.push_str(header);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        if socket.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        match &reply.body {
            Body::Fixed(bytes) => {
                let _ = socket.write_all(bytes).await;
            }
            Body::Chunked(chunks, delay) => {
                for chunk in chunks {
                    tokio::time::sleep(*delay).await;
                    let frame = format!("{:x}\r\n", chunk.len());
                    if socket.write_all(frame.as_bytes()).await.is_err() {
                        return;
                    }
                    if socket.write_all(chunk).await.is_err() {
                        return;
                    }
                    if socket.write_all(b"\r\n").await.is_err() {
                        return;
                    }
                    if socket.flush().await.is_err() {
                        return;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            }
            Body::UntilClose(bytes) => {
                let _ = socket.write_all(bytes).await;
            }
            Body::Short(_, bytes) => {
                let _ = socket.write_all(bytes).await;
            }
        }
        if socket.flush().await.is_err() {
            return;
        }
        match &reply.after {
            AfterReply::KeepOpen => {}
            AfterReply::Close => {
                let _ = socket.shutdown().await;
            }
            AfterReply::CloseAfter(delay) => {
                tokio::time::sleep(*delay).await;
                let _ = socket.shutdown().await;
            }
            AfterReply::Send(bytes) => {
                let _ = socket.write_all(bytes).await;
            }
        }
    }

    /// A fresh TCP connection between a client socket and its server-side peer.
    pub(crate) async fn tcp_sockets() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind tcp pair listener");
        let addr = listener.local_addr().unwrap();
        let (client, peer) = tokio::join!(tokio::net::TcpStream::connect(addr), listener.accept());
        (client.expect("connect"), peer.expect("accept").0)
    }

    /// A plaintext client stream the pool can hold, paired with its server-side socket.
    pub(crate) async fn tcp_pair() -> (PooledStream, tokio::net::TcpStream) {
        let (client, peer) = tcp_sockets().await;
        (PooledStream::Plain(ProxyStream::tcp(client)), peer)
    }

    /// Throwaway self-signed test certificate for `localhost` / `127.0.0.1`, generated
    /// on 2026-11-23 and valid until 2126. Test-only: the private key is published here
    /// on purpose and never leaves the test binary.
    pub(crate) const TLS_TEST_CERT_PEM: &str = "\
-----BEGIN CERTIFICATE-----
MIIBvzCCAWWgAwIBAgIUScAlgzDyd1Q8TpjcoDf+hOgsRkQwCgYIKoZIzj0EAwIw
FDESMBAGA1UEAwwJbG9jYWxob3N0MCAXDTI2MTEyMzExMTgyM1oYDzIxMjYxMDMw
MTExODIzWjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwWTATBgcqhkjOPQIBBggqhkjO
PQMBBwNCAAS9uHpcvOXPwIgumE9USYEvz6HQnRc3rERy1qBgzxDHJUp8Oh0mQQYV
rzywIgW7iWdgm6KwN4XzvWVIsH2d5TH1o4GSMIGPMB0GA1UdDgQWBBSPRqKgxY8w
JBv9ghbQEb/uoJLw8jAfBgNVHSMEGDAWgBSPRqKgxY8wJBv9ghbQEb/uoJLw8jAa
BgNVHREEEzARgglsb2NhbGhvc3SHBH8AAAEwDAYDVR0TAQH/BAIwADAOBgNVHQ8B
Af8EBAMCB4AwEwYDVR0lBAwwCgYIKwYBBQUHAwEwCgYIKoZIzj0EAwIDSAAwRQIh
AL+/B+YdXMBC5tlFK2Naa9LzGUCESaFjkg0Vp05cOa9ZAiARr8m+BRad5vWVdzeP
DMsv4VrdPxGX6N/cePWS2LOuUQ==
-----END CERTIFICATE-----
";

    /// The private key of `TLS_TEST_CERT_PEM`.
    pub(crate) const TLS_TEST_KEY_PEM: &str = "\
-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgZtiXdel5L7bMs68l
w2JvuR+PAAKj9acwvdsjqAh/mryhRANCAAS9uHpcvOXPwIgumE9USYEvz6HQnRc3
rERy1qBgzxDHJUp8Oh0mQQYVrzywIgW7iWdgm6KwN4XzvWVIsH2d5TH1
-----END PRIVATE KEY-----
";

    fn tls_server_config() -> Arc<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cert = CertificateDer::from_pem_slice(TLS_TEST_CERT_PEM.as_bytes())
            .expect("test certificate parses");
        let key =
            PrivateKeyDer::from_pem_slice(TLS_TEST_KEY_PEM.as_bytes()).expect("test key parses");
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls supports its default protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("test certificate and key match");
        Arc::new(config)
    }

    pub(crate) fn tls_client_config() -> Arc<rustls::ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = rustls::RootCertStore::empty();
        let cert = CertificateDer::from_pem_slice(TLS_TEST_CERT_PEM.as_bytes())
            .expect("test certificate parses");
        roots.add(cert).expect("test certificate is a valid root");
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls supports its default protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(config)
    }

    /// A TLS connection in the pool's exact production type (`TlsStream<ProxyStream>`
    /// inside `PooledStream::Tls`), paired with the server side of it.
    pub(crate) async fn tls_pair() -> (
        PooledStream,
        tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    ) {
        let (client_tcp, server_tcp) = tcp_sockets().await;
        let acceptor = tokio_rustls::TlsAcceptor::from(tls_server_config());
        let connector = tokio_rustls::TlsConnector::from(tls_client_config());
        let server_name = rustls::pki_types::ServerName::try_from("localhost".to_string())
            .expect("localhost is a valid server name");
        let (client, server) = tokio::join!(
            connector.connect(server_name, ProxyStream::tcp(client_tcp)),
            acceptor.accept(server_tcp)
        );
        (
            PooledStream::Tls(Box::new(client.expect("TLS connect"))),
            server.expect("TLS accept"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use tokio::io::AsyncReadExt;

    fn config(max_idle_per_key: usize, max_idle_total: usize, idle_ttl: Duration) -> PoolConfig {
        PoolConfig {
            max_idle_per_key,
            max_idle_total,
            idle_ttl,
        }
    }

    #[tokio::test]
    async fn take_hands_out_the_newest_idle_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let pool = UpstreamPool::new(config(4, 10, Duration::from_secs(60)));
        let key = PoolKey::new("http", "lifo.example", 80, None);
        let (first, _first_peer) = tcp_pair().await;
        let (second, mut second_peer) = tcp_pair().await;
        pool.put(key.clone(), first, Duration::MAX);
        pool.put(key.clone(), second, Duration::MAX);
        assert_eq!(pool.idle_count(&key), 2);

        let Taken::Idle(mut stream) = pool.take(&key) else {
            panic!("expected an idle connection");
        };
        // Only the newest connection's peer writes; whatever arrives on `stream` must be
        // its byte, proving take handed out the connection that went idle last.
        second_peer.write_all(b"2").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
            .await
            .expect("read timed out")
            .expect("read failed");
        assert_eq!(read, 1, "the newest connection must carry its peer's byte");
        assert_eq!(
            byte[0], b'2',
            "take must hand out the connection that went idle last"
        );
        assert_eq!(
            pool.idle_count(&key),
            1,
            "the older connection stays pooled"
        );
    }

    #[tokio::test]
    async fn per_key_cap_closes_the_oldest_idle_connection() {
        let pool = UpstreamPool::new(config(2, 10, Duration::from_secs(60)));
        let key = PoolKey::new("http", "cap.example", 80, None);
        let mut peers = Vec::new();
        for _ in 0..3 {
            let (stream, peer) = tcp_pair().await;
            pool.put(key.clone(), stream, Duration::MAX);
            peers.push(peer);
        }
        assert_eq!(pool.idle_count(&key), 2, "cap keeps two per key");

        let mut byte = [0u8; 1];
        let evicted = tokio::time::timeout(Duration::from_secs(2), peers.remove(0).read(&mut byte))
            .await
            .expect("peer read timed out");
        assert_eq!(
            evicted.unwrap_or(1),
            0,
            "the surplus idle connection must be closed"
        );
    }

    #[tokio::test]
    async fn global_cap_closes_the_oldest_idle_connection() {
        let pool = UpstreamPool::new(config(4, 3, Duration::from_secs(60)));
        let a = PoolKey::new("http", "global-a.example", 80, None);
        let b = PoolKey::new("http", "global-b.example", 80, None);
        let mut peers = Vec::new();
        for _ in 0..2 {
            let (stream, peer) = tcp_pair().await;
            pool.put(a.clone(), stream, Duration::MAX);
            peers.push(peer);
        }
        for _ in 0..2 {
            let (stream, peer) = tcp_pair().await;
            pool.put(b.clone(), stream, Duration::MAX);
            peers.push(peer);
        }
        assert_eq!(
            pool.idle_count(&a) + pool.idle_count(&b),
            3,
            "global cap keeps three overall"
        );

        let mut byte = [0u8; 1];
        let evicted = tokio::time::timeout(Duration::from_secs(2), peers.remove(0).read(&mut byte))
            .await
            .expect("peer read timed out");
        assert_eq!(
            evicted.unwrap_or(1),
            0,
            "the globally oldest idle connection must be closed"
        );
    }

    #[tokio::test]
    async fn expired_connection_is_not_taken() {
        let pool = UpstreamPool::new(config(4, 10, Duration::from_millis(40)));
        let key = PoolKey::new("http", "ttl.example", 80, None);
        let (stream, mut peer) = tcp_pair().await;
        pool.put(key.clone(), stream, Duration::MAX);

        tokio::time::sleep(Duration::from_millis(120)).await;
        let Taken::None = pool.take(&key) else {
            panic!("an expired connection must not be handed out");
        };

        let mut byte = [0u8; 1];
        let closed = tokio::time::timeout(Duration::from_secs(2), peer.read(&mut byte))
            .await
            .expect("peer read timed out");
        assert_eq!(
            closed.unwrap_or(1),
            0,
            "an expired connection must be closed when dropped on take"
        );
    }

    #[tokio::test]
    async fn expired_connections_are_closed_by_the_sweeper() {
        let pool = UpstreamPool::new(config(4, 10, Duration::from_millis(40)));
        let key = PoolKey::new("http", "sweep.example", 80, None);
        let (stream, mut peer) = tcp_pair().await;
        pool.put(key.clone(), stream, Duration::MAX);

        let mut byte = [0u8; 1];
        let closed = tokio::time::timeout(Duration::from_secs(2), peer.read(&mut byte))
            .await
            .expect("peer read timed out");
        assert_eq!(
            closed.unwrap_or(1),
            0,
            "the sweeper must close an expired idle connection without a take"
        );
        assert_eq!(pool.idle_count(&key), 0);
    }

    #[tokio::test]
    async fn sweeper_can_expire_a_newer_entry_before_an_older_one() {
        let pool = UpstreamPool::new(PoolConfig::default());
        let key = PoolKey::new("https", "EXAMPLE.com", 443, None);
        let (older, _older_peer) = tcp_pair().await;
        let (newer, _newer_peer) = tcp_pair().await;
        pool.put(key.clone(), older, Duration::MAX);
        pool.put(key, newer, Duration::from_secs(1));

        let mut expired = Vec::new();
        pool.lock().remove_expired(
            Instant::now() + Duration::from_secs(2),
            SystemTime::now(),
            &mut expired,
        );
        let lower = PoolKey::new("https", "example.com", 443, None);
        assert_eq!(expired.len(), 1);
        assert_eq!(pool.idle_count(&lower), 1);
    }

    #[tokio::test]
    async fn suspended_time_expires_connections_on_take_and_sweep() {
        let pool = UpstreamPool::new(PoolConfig::default());
        let take_key = PoolKey::new("http", "take.example", 80, None);
        let sweep_key = PoolKey::new("http", "sweep.example", 80, None);
        let (take_stream, _take_peer) = tcp_pair().await;
        let (sweep_stream, _sweep_peer) = tcp_pair().await;
        pool.put(take_key.clone(), take_stream, Duration::MAX);
        pool.put(sweep_key.clone(), sweep_stream, Duration::MAX);

        // Simulate a suspend that advanced wall time but not the monotonic clock.
        let wall_now = SystemTime::now();
        for queue in pool.lock().by_key.values_mut() {
            queue[0].idle_since_wall = wall_now - Duration::from_secs(600);
        }
        assert!(matches!(pool.take(&take_key), Taken::None));

        let mut expired = Vec::new();
        pool.lock()
            .remove_expired(Instant::now(), wall_now, &mut expired);
        assert_eq!(expired.len(), 1);
        assert_eq!(pool.idle_count(&sweep_key), 0);
    }

    #[tokio::test]
    async fn take_rejects_unsolicited_bytes_even_when_the_task_budget_is_exhausted() {
        let pool = UpstreamPool::new(PoolConfig::default());
        let key = PoolKey::new("http", "localhost", 80, None);
        let (socket, mut peer) = tcp_sockets().await;
        peer.write_all(b"unsolicited").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), socket.readable())
            .await
            .expect("peer bytes should become readable")
            .unwrap();
        pool.put(
            key.clone(),
            PooledStream::Plain(ProxyStream::tcp(socket)),
            Duration::MAX,
        );

        tokio::spawn(async move {
            use tokio::task::coop::{consume_budget, has_budget_remaining};

            while has_budget_remaining() {
                consume_budget().await;
            }
            assert!(matches!(pool.take(&key), Taken::None));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn probe_rejects_closed_and_noisy_connections() {
        use tokio::io::AsyncWriteExt;

        // Quiet: nothing pending, nothing to read.
        let (mut stream, _peer) = tcp_pair().await;
        assert_eq!(unusable_reason(&mut stream), None);

        // Closed by the upstream: EOF.
        let (mut stream, peer) = tcp_pair().await;
        drop(peer);
        // Let the IO driver observe the FIN before probing.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(unusable_reason(&mut stream), Some("closed by the upstream"));

        // Unexpected pending bytes.
        let (mut stream, mut peer) = tcp_pair().await;
        peer.write_all(b"unsolicited").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            unusable_reason(&mut stream),
            Some("unexpected pending bytes")
        );
    }

    /// R3.c: a real write error while the request goes out fails the exchange, and the
    /// connection never reaches the pool. The write side of the stream is shut down
    /// first, so the very first `write_all` fails.
    #[tokio::test]
    async fn raw_write_error_drops_the_connection() {
        let pool = UpstreamPool::new(PoolConfig::default());
        let key = PoolKey::new("http", "localhost", 80, None);
        let (mut stream, _peer) = tcp_pair().await;
        stream.shutdown().await.unwrap();
        let result = write_raw_request(
            &pool,
            &key,
            &RawRequest {
                bytes: b"GET / HTTP/1.1\r\n\r\n",
                connection_close: false,
            },
            stream,
        )
        .await;
        assert!(
            matches!(result, Err(RawWriteError { error: ProxyError::ForwardFailed(message), wrote_bytes: false }) if message.starts_with("Write failed:"))
        );
        assert!(matches!(pool.take(&key), Taken::None));
    }

    /// R3.c over TLS: after the request went out, the peer closes without
    /// `close_notify`. The IO hyper reads through surfaces `UnexpectedEof` and records
    /// the connection as broken, so `reuse_blocker` refuses it.
    #[tokio::test]
    async fn abrupt_tls_close_marks_the_http_io_unusable() {
        use tokio::io::AsyncReadExt;

        let pool = UpstreamPool::new(PoolConfig::default());
        let key = PoolKey::new("https", "localhost", 443, None);
        let (stream, mut peer) = tls_pair().await;
        let mut io = write_raw_request(
            &pool,
            &key,
            &RawRequest {
                bytes: b"GET / HTTP/1.1\r\n\r\n",
                connection_close: false,
            },
            stream,
        )
        .await
        .expect("write on a healthy TLS connection");
        io.write_all(b"dummy").await.unwrap();
        // Drain the wire request before dropping the peer without close_notify.
        let mut request = [0; b"GET / HTTP/1.1\r\n\r\n".len()];
        peer.read_exact(&mut request).await.unwrap();
        drop(peer);
        let error = io.read_u8().await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
        assert_eq!(
            reuse_blocker(&io.state, true),
            Err("I/O error or EOF on the connection")
        );
    }

    #[tokio::test]
    async fn release_decision_requires_every_condition() {
        let clean = || ConnState {
            idle_limit_secs: AtomicU64::new(u64::MAX),
            unusable: AtomicBool::new(false),
            body: AtomicU8::new(BODY_READ_TO_END),
            body_settled: Notify::new(),
            driver_done: AtomicBool::new(false),
            driver_done_waker: AtomicWaker::new(),
        };
        assert_eq!(reuse_blocker(&clean(), true), Ok(()));

        let unusable = ConnState {
            unusable: AtomicBool::new(true),
            ..clean()
        };
        assert!(reuse_blocker(&unusable, true).is_err(), "I/O error blocks");

        let no_keep_alive = ConnState {
            idle_limit_secs: AtomicU64::new(0),
            ..clean()
        };
        assert!(
            reuse_blocker(&no_keep_alive, true).is_err(),
            "a close-forbidden exchange blocks"
        );

        assert!(
            reuse_blocker(&clean(), false).is_err(),
            "left-over response bytes block"
        );

        // An abandoned body never reaches the reuse check; the driver's settle step
        // already rejected it. Recorded here to pin the value that means "not reusable".
        let abandoned = ConnState {
            body: AtomicU8::new(BODY_ABANDONED),
            ..clean()
        };
        assert_eq!(abandoned.body_outcome(), BODY_ABANDONED);
    }

    #[tokio::test]
    async fn probe_on_a_tls_stream_tells_health_close_notify_abrupt_close_and_data() {
        use tokio::io::AsyncWriteExt;

        // (a) Healthy: nothing pending, nothing to read.
        let (mut client, _server) = tls_pair().await;
        assert_eq!(unusable_reason(&mut client), None);

        // (b) The peer sent close_notify: a clean EOF through the TLS layer.
        let (mut client, mut server) = tls_pair().await;
        server.shutdown().await.expect("server sends close_notify");
        drop(server);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(unusable_reason(&mut client), Some("closed by the upstream"));

        // (c) The peer vanished without close_notify: an error through the TLS layer.
        let (mut client, server) = tls_pair().await;
        drop(server);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            unusable_reason(&mut client).is_some(),
            "an abruptly closed TLS connection must not be handed out"
        );

        // (d) The peer sent application data nobody asked for.
        let (mut client, mut server) = tls_pair().await;
        server
            .write_all(b"unsolicited")
            .await
            .expect("server writes");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            unusable_reason(&mut client),
            Some("unexpected pending bytes")
        );
    }

    #[tokio::test]
    async fn tls_stream_round_trips_the_pool_and_closes_with_close_notify() {
        use tokio::io::AsyncWriteExt;

        let pool = UpstreamPool::new(config(4, 10, Duration::from_secs(60)));
        let key = PoolKey::new("https", "localhost", 443, None);
        let (client, mut server) = tls_pair().await;
        pool.put(key.clone(), client, Duration::MAX);

        let Taken::Idle(mut client) = pool.take(&key) else {
            panic!("a healthy TLS connection must be handed out");
        };
        client
            .write_all(b"ping")
            .await
            .expect("write through the pooled TLS stream");
        let mut ping = [0u8; 4];
        server
            .read_exact(&mut ping)
            .await
            .expect("server reads the ping");
        assert_eq!(&ping, b"ping");

        close_stream(client);
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(Duration::from_secs(2), server.read(&mut byte))
            .await
            .expect("peer read timed out")
            .expect("peer read failed");
        assert_eq!(read, 0, "the peer must see close_notify as a clean EOF");
    }
}
