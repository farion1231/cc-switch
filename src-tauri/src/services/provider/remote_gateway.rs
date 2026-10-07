//! SSH hosts that use the local proxy through `ssh -R`.
//!
//! Each host gets one reverse tunnel (remote `127.0.0.1:<remote_port>` →
//! the local token-checked gateway listener) and a per-app route that either
//! pins a provider or follows the local current one. The remote CLI config
//! only points at the tunnel, so switching a route needs no remote write.

use std::collections::HashMap;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{watch, Mutex};

use super::remote::{
    apply_remote_writes, build_remote_codex_writes, claude_remote_write, ensure_remote_supported,
    gemini_remote_writes, matched_remote_provider, read_remote_snapshot,
    remote_state_from_snapshot, resolve_ssh_target, InFlightGuard, RemoteSnapshot, RemoteWrite,
    ResolvedSshTarget,
};
use super::{codex_direct, gemini_direct, RemoteProviderState, SshConnectionTarget};
use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::project::claude::{
    proxy_projection, ClaudeProjection, ProxyAuth, PROXY_TOKEN_PLACEHOLDER,
};
use crate::live::project::gemini::GeminiProjection;
use crate::provider::Provider;
use crate::proxy::remote_gateway::{provider_usable_remotely, RemoteGatewayListener, RouterSource};
use crate::services::ProxyService;
use crate::store::AppState;

const REMOTE_PORT_RANGE: std::ops::Range<u16> = 20000..40000;
const CONNECTED_AFTER: Duration = Duration::from_secs(3);
const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
struct GatewayRecord {
    host_key: String,
    target: SshConnectionTarget,
    remote_port: u16,
    token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelState {
    Idle,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStatus {
    pub state: TunnelState,
    pub message: Option<String>,
}

impl TunnelStatus {
    fn new(state: TunnelState, message: Option<String>) -> Self {
        Self { state, message }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayState {
    pub host_key: String,
    pub app: String,
    pub enabled: bool,
    /// `None` follows the local current provider.
    pub provider_id: Option<String>,
    pub remote_port: Option<u16>,
    pub tunnel: TunnelStatus,
    pub proxy_running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayApplyResult {
    pub state: RemoteGatewayState,
    /// Set when the remote config was (re)written.
    pub remote_state: Option<RemoteProviderState>,
    pub written_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayRoute {
    pub app: String,
    /// `None` follows the local current provider.
    pub provider_id: Option<String>,
    /// Name of the provider requests go to right now.
    pub provider_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayHost {
    pub host_key: String,
    pub target: SshConnectionTarget,
    pub remote_port: u16,
    pub tunnel: TunnelStatus,
    pub routes: Vec<RemoteGatewayRoute>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayOverview {
    pub proxy_running: bool,
    pub hosts: Vec<RemoteGatewayHost>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Database(e.to_string())
}

fn load_record(db: &Database, host_key: &str) -> Result<Option<GatewayRecord>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    let row = conn
        .query_row(
            "SELECT target_json, remote_port, token FROM remote_gateways WHERE host_key = ?1",
            [host_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(db_err)?;
    let Some((target_json, remote_port, token)) = row else {
        return Ok(None);
    };
    let target = serde_json::from_str(&target_json)
        .map_err(|e| AppError::Message(format!("远端网关记录损坏: {e}")))?;
    Ok(Some(GatewayRecord {
        host_key: host_key.to_string(),
        target,
        remote_port: u16::try_from(remote_port).unwrap_or(REMOTE_PORT_RANGE.start),
        token,
    }))
}

fn random_remote_port() -> u16 {
    let span = u128::from(REMOTE_PORT_RANGE.end - REMOTE_PORT_RANGE.start);
    let offset = uuid::Uuid::new_v4().as_u128() % span;
    REMOTE_PORT_RANGE.start + offset as u16
}

fn random_token() -> String {
    format!(
        "ccsw-{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn ensure_record(
    db: &Database,
    host_key: &str,
    target: &SshConnectionTarget,
    remote_port: Option<u16>,
) -> Result<GatewayRecord, AppError> {
    if matches!(remote_port, Some(port) if port < 1024) {
        return Err(AppError::Message(
            "远端端口需在 1024-65535 之间".to_string(),
        ));
    }
    let mut stored_target = target.clone();
    stored_target.password = None;
    let target_json = serde_json::to_string(&stored_target)
        .map_err(|e| AppError::Message(format!("序列化 SSH 目标失败: {e}")))?;

    let existing = load_record(db, host_key)?;
    let port = remote_port
        .or(existing.as_ref().map(|record| record.remote_port))
        .unwrap_or_else(random_remote_port);
    let token = existing
        .map(|record| record.token)
        .unwrap_or_else(random_token);

    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO remote_gateways (host_key, target_json, remote_port, token, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(host_key) DO UPDATE SET target_json = ?2, remote_port = ?3",
        rusqlite::params![
            host_key,
            target_json,
            i64::from(port),
            token,
            chrono::Utc::now().timestamp()
        ],
    )
    .map_err(db_err)?;

    Ok(GatewayRecord {
        host_key: host_key.to_string(),
        target: stored_target,
        remote_port: port,
        token,
    })
}

fn load_route(
    db: &Database,
    host_key: &str,
    app: &AppType,
) -> Result<(bool, Option<String>), AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    let route = conn
        .query_row(
            "SELECT enabled, provider_id FROM remote_gateway_routes
             WHERE host_key = ?1 AND app_type = ?2",
            rusqlite::params![host_key, app.as_str()],
            |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(db_err)?;
    Ok(route.unwrap_or((false, None)))
}

fn save_route(
    db: &Database,
    host_key: &str,
    app: &AppType,
    enabled: bool,
    provider_id: Option<&str>,
) -> Result<(), AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO remote_gateway_routes (host_key, app_type, enabled, provider_id)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(host_key, app_type) DO UPDATE SET enabled = ?3, provider_id = ?4",
        rusqlite::params![host_key, app.as_str(), i64::from(enabled), provider_id],
    )
    .map_err(db_err)?;
    Ok(())
}

fn host_has_enabled_routes(db: &Database, host_key: &str) -> Result<bool, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM remote_gateway_routes WHERE host_key = ?1 AND enabled = 1)",
        [host_key],
        |row| row.get::<_, bool>(0),
    )
    .map_err(db_err)
}

fn enabled_routes(db: &Database) -> Result<Vec<(String, String, Option<String>)>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    let mut stmt = conn
        .prepare(
            "SELECT host_key, app_type, provider_id FROM remote_gateway_routes
             WHERE enabled = 1 ORDER BY host_key, app_type",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(db_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
}

fn enabled_host_keys(db: &Database) -> Result<Vec<String>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    let mut stmt = conn
        .prepare("SELECT DISTINCT host_key FROM remote_gateway_routes WHERE enabled = 1")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
}

/// True when the remote files point at this host's gateway (they carry its
/// token), so they count as managed even though no provider matches them.
pub(super) fn snapshot_uses_gateway(
    db: &Database,
    host_key: &str,
    snapshot: &RemoteSnapshot,
) -> bool {
    let Ok(Some(record)) = load_record(db, host_key) else {
        return false;
    };
    snapshot
        .files
        .iter()
        .filter_map(|(_, content)| content.as_deref())
        .any(|content| content.contains(&record.token))
}

// ---------------------------------------------------------------------------
// Tunnels
// ---------------------------------------------------------------------------

struct Tunnel {
    remote_port: u16,
    status: watch::Receiver<TunnelStatus>,
    stop: watch::Sender<bool>,
}

#[derive(Default)]
struct Manager {
    listener: Option<RemoteGatewayListener>,
    tunnels: HashMap<String, Tunnel>,
}

fn manager() -> &'static Mutex<Manager> {
    static MANAGER: OnceLock<Mutex<Manager>> = OnceLock::new();
    MANAGER.get_or_init(|| Mutex::new(Manager::default()))
}

fn router_source(proxy: ProxyService) -> RouterSource {
    Arc::new(move || {
        let proxy = proxy.clone();
        Box::pin(async move { proxy.running_router().await })
    })
}

async fn tunnel_status(host_key: &str) -> TunnelStatus {
    manager()
        .lock()
        .await
        .tunnels
        .get(host_key)
        .map(|tunnel| tunnel.status.borrow().clone())
        .unwrap_or_else(|| TunnelStatus::new(TunnelState::Idle, None))
}

async fn ensure_proxy_running(proxy: &ProxyService) -> Result<(), AppError> {
    if !proxy.is_running().await {
        proxy
            .start()
            .await
            .map_err(|e| AppError::Message(format!("启动本机网关失败: {e}")))?;
    }
    Ok(())
}

/// Starts (or keeps) the host's tunnel and returns its status receiver.
async fn ensure_tunnel(
    db: Arc<Database>,
    proxy: ProxyService,
    record: &GatewayRecord,
) -> Result<watch::Receiver<TunnelStatus>, AppError> {
    let target = resolve_tunnel_target(&record.target)?;
    ensure_proxy_running(&proxy).await?;

    let mut manager = manager().lock().await;
    if manager.listener.is_none() {
        manager.listener = Some(RemoteGatewayListener::start(db, router_source(proxy)).await?);
    }
    let local_port = manager
        .listener
        .as_ref()
        .map_or(0, |listener| listener.port);

    if let Some(tunnel) = manager.tunnels.get(&record.host_key) {
        if tunnel.remote_port == record.remote_port {
            return Ok(tunnel.status.clone());
        }
    }
    if let Some(old) = manager.tunnels.remove(&record.host_key) {
        let _ = old.stop.send(true);
    }

    let (status_tx, status_rx) = watch::channel(TunnelStatus::new(TunnelState::Connecting, None));
    let (stop_tx, stop_rx) = watch::channel(false);
    tokio::spawn(supervise_tunnel(
        target,
        record.remote_port,
        local_port,
        status_tx,
        stop_rx,
    ));
    manager.tunnels.insert(
        record.host_key.clone(),
        Tunnel {
            remote_port: record.remote_port,
            status: status_rx.clone(),
            stop: stop_tx,
        },
    );
    Ok(status_rx)
}

async fn stop_tunnel(host_key: &str) {
    if let Some(tunnel) = manager().lock().await.tunnels.remove(host_key) {
        let _ = tunnel.stop.send(true);
    }
}

/// Waits for the first connection attempt to settle so enable can report a
/// port conflict or auth failure right away.
async fn wait_for_first_attempt(mut status: watch::Receiver<TunnelStatus>) -> TunnelStatus {
    let settle = async {
        loop {
            if status.borrow().state != TunnelState::Connecting {
                break;
            }
            if status.changed().await.is_err() {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(CONNECTED_AFTER + Duration::from_secs(20), settle).await;
    let current = status.borrow().clone();
    current
}

fn resolve_tunnel_target(target: &SshConnectionTarget) -> Result<ResolvedSshTarget, AppError> {
    let resolved = resolve_ssh_target(target)?;
    if resolved.password.is_some() {
        return Err(AppError::Message(
            "本地网关模式需要免密 SSH（~/.ssh/config 中的 Host 或密钥登录），隧道无法使用密码登录"
                .to_string(),
        ));
    }
    Ok(resolved)
}

enum TunnelExit {
    Stopped,
    Failed(String),
}

async fn supervise_tunnel(
    target: ResolvedSshTarget,
    remote_port: u16,
    local_port: u16,
    status: watch::Sender<TunnelStatus>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = MIN_BACKOFF;
    loop {
        status.send_replace(TunnelStatus::new(TunnelState::Connecting, None));
        let started = Instant::now();
        match run_tunnel_once(&target, remote_port, local_port, &status, &mut stop).await {
            TunnelExit::Stopped => {
                status.send_replace(TunnelStatus::new(TunnelState::Idle, None));
                return;
            }
            TunnelExit::Failed(message) => {
                log::warn!(
                    "[RemoteGateway] tunnel to {} dropped: {message}",
                    target.label()
                );
                if started.elapsed() > MAX_BACKOFF {
                    backoff = MIN_BACKOFF;
                }
                status.send_replace(TunnelStatus::new(TunnelState::Error, Some(message)));
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = stop.changed() => {
                status.send_replace(TunnelStatus::new(TunnelState::Idle, None));
                return;
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn run_tunnel_once(
    target: &ResolvedSshTarget,
    remote_port: u16,
    local_port: u16,
    status: &watch::Sender<TunnelStatus>,
    stop: &mut watch::Receiver<bool>,
) -> TunnelExit {
    let mut command = tokio::process::Command::new("ssh");
    command.args([
        "-N",
        "-T",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "BatchMode=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
    ]);
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }
    command
        .arg("-R")
        .arg(format!("127.0.0.1:{remote_port}:127.0.0.1:{local_port}"))
        .arg("--")
        .arg(&target.connect_target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return TunnelExit::Failed(format!("无法启动 ssh: {e}")),
    };

    let last_error = Arc::new(StdMutex::new(None::<String>));
    if let Some(stderr) = child.stderr.take() {
        let last_error = last_error.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() || line.starts_with("Warning: Permanently added") {
                    continue;
                }
                *last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(line);
            }
        });
    }

    let connected_timer = tokio::time::sleep(CONNECTED_AFTER);
    tokio::pin!(connected_timer);
    let mut connected = false;
    loop {
        tokio::select! {
            exit = child.wait() => {
                // Give the stderr reader a moment to catch the final line.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let line = last_error.lock().unwrap_or_else(|e| e.into_inner()).take();
                let code = exit.ok().and_then(|status| status.code());
                return TunnelExit::Failed(describe_tunnel_failure(line, code, remote_port));
            }
            _ = &mut connected_timer, if !connected => {
                connected = true;
                status.send_replace(TunnelStatus::new(TunnelState::Connected, None));
            }
            _ = stop.changed() => {
                let _ = child.kill().await;
                return TunnelExit::Stopped;
            }
        }
    }
}

fn describe_tunnel_failure(line: Option<String>, code: Option<i32>, remote_port: u16) -> String {
    match line {
        Some(line) if line.contains("remote port forwarding failed") => format!(
            "远端端口 {remote_port} 转发失败：端口已被占用或服务器禁止 TCP 转发，请换一个端口"
        ),
        Some(line) => line,
        None => match code {
            Some(code) => format!("ssh 已退出（代码 {code}）"),
            None => "ssh 已退出".to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Remote config
// ---------------------------------------------------------------------------

fn gateway_url(remote_port: u16) -> String {
    format!("http://127.0.0.1:{remote_port}")
}

/// The provider a route resolves to right now: the pinned one, else the one
/// the local proxy is sending requests to.
fn routed_provider(
    state: &AppState,
    app: &AppType,
    provider_id: Option<&str>,
) -> Result<Option<Provider>, AppError> {
    match provider_id {
        Some(id) => state.db.get_provider_by_id(id, app.as_str()),
        None => crate::mode::current::provider_in_use(&state.db, app),
    }
}

fn validate_pinned_provider(
    state: &AppState,
    app: &AppType,
    provider_id: Option<&str>,
) -> Result<(), AppError> {
    let Some(provider_id) = provider_id else {
        return Ok(());
    };
    let provider = state
        .db
        .get_all_providers(app.as_str())?
        .shift_remove(provider_id)
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;
    if !provider_usable_remotely(&provider) {
        return Err(AppError::Message(format!(
            "供应商 {} 依赖本机官方登录，不能通过远端网关使用",
            provider.name
        )));
    }
    Ok(())
}

/// Same client contract as local proxy takeover, with the gateway token in
/// place of the placeholder key.
fn build_gateway_writes(
    state: &AppState,
    app: &AppType,
    record: &GatewayRecord,
    prev: Option<&Provider>,
    provider: Option<&Provider>,
    snapshot: &RemoteSnapshot,
) -> Result<Vec<RemoteWrite>, AppError> {
    let url = gateway_url(record.remote_port);
    match app {
        AppType::Claude => {
            let route = provider
                .map(|provider| ClaudeProjection::of(&provider.settings_config))
                .unwrap_or_default();
            let mut projection = proxy_projection(&route, &url, ProxyAuth::FollowRow, None);
            for value in projection.env.values_mut() {
                if value.as_str() == Some(PROXY_TOKEN_PLACEHOLDER) {
                    *value = Value::String(record.token.clone());
                }
            }
            Ok(vec![claude_remote_write(prev, &projection, snapshot)?])
        }
        AppType::Codex => {
            let provider = provider.ok_or_else(|| {
                AppError::Message("本机 Codex 没有当前供应商，请先选择一个供应商".to_string())
            })?;
            let base_url = format!("{url}/v1");
            // An official route would make the remote send its own ChatGPT login;
            // requests for it are refused anyway, so keep the third-party shape
            // and let the next local switch take effect without a rewrite.
            let placeholder;
            let route = if provider_usable_remotely(provider) {
                provider
            } else {
                placeholder = gateway_placeholder_codex_provider(&base_url);
                &placeholder
            };
            build_remote_codex_writes(
                state.db.as_ref(),
                prev,
                codex_direct::Target::Proxy {
                    route,
                    base_url: &base_url,
                    stack: &[],
                },
                Some(&record.token),
                snapshot,
            )
        }
        AppType::Gemini => {
            let route = match provider {
                Some(provider) => gemini_direct::projection(provider)?,
                None => GeminiProjection::empty(),
            };
            let projection = GeminiProjection::proxy_contract(&route, &url, &record.token);
            gemini_remote_writes(&projection, snapshot)
        }
        _ => unreachable!("unsupported app type checked by caller"),
    }
}

fn gateway_placeholder_codex_provider(base_url: &str) -> Provider {
    let config = format!(
        "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"cc-switch\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\n"
    );
    let mut provider = Provider::with_id(
        "remote-gateway".to_string(),
        "CC Switch".to_string(),
        json!({ "auth": {}, "config": config }),
        None,
    );
    provider.category = Some("custom".to_string());
    provider
}

/// Points the remote CLI at the tunnel. Blocking (runs ssh).
fn push_gateway_config(
    state: &AppState,
    app: &AppType,
    target: &ResolvedSshTarget,
    record: &GatewayRecord,
    provider: Option<&Provider>,
) -> Result<(Vec<String>, RemoteProviderState), AppError> {
    let snapshot = read_remote_snapshot(app, target)?;
    let prev = if snapshot_uses_gateway(&state.db, &record.host_key, &snapshot) {
        None
    } else {
        matched_remote_provider(state, app, &snapshot)?
    };
    let writes: Vec<RemoteWrite> =
        build_gateway_writes(state, app, record, prev.as_ref(), provider, &snapshot)?
            .into_iter()
            .filter(|write| write.content.as_deref() != snapshot.get(write.path))
            .filter(|write| write.content.is_some() || snapshot.get(write.path).is_some())
            .collect();
    let (written, _removed) = if writes.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        apply_remote_writes(target, &writes)?
    };

    let mut after = snapshot;
    for write in writes {
        if let Some((_, content)) = after.files.iter_mut().find(|(path, _)| *path == write.path) {
            *content = write.content;
        }
    }
    let remote_state = remote_state_from_snapshot(state, app, target.label(), &after)?;
    Ok((written, remote_state))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct RemoteGatewayService;

impl RemoteGatewayService {
    pub async fn state(
        state: &AppState,
        app: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteGatewayState, AppError> {
        ensure_remote_supported(&app)?;
        let host_key = resolve_ssh_target(target)?.label;
        Self::state_for(state, &app, &host_key).await
    }

    async fn state_for(
        state: &AppState,
        app: &AppType,
        host_key: &str,
    ) -> Result<RemoteGatewayState, AppError> {
        let record = load_record(&state.db, host_key)?;
        let (enabled, provider_id) = load_route(&state.db, host_key, app)?;
        Ok(RemoteGatewayState {
            host_key: host_key.to_string(),
            app: app.as_str().to_string(),
            enabled,
            provider_id,
            remote_port: record.map(|record| record.remote_port),
            tunnel: tunnel_status(host_key).await,
            proxy_running: state.proxy_service.is_running().await,
        })
    }

    /// Turns gateway mode on (or re-applies it, e.g. after a port change):
    /// saves the route, brings the tunnel up and points the remote CLI at it.
    pub async fn enable(
        state: &AppState,
        app: AppType,
        target: &SshConnectionTarget,
        provider_id: Option<String>,
        remote_port: Option<u16>,
    ) -> Result<RemoteGatewayApplyResult, AppError> {
        ensure_remote_supported(&app)?;
        let resolved = resolve_tunnel_target(target)?;
        let host_key = resolved.label.clone();
        let _in_flight = InFlightGuard::acquire(format!("{host_key}\n{}", app.as_str()))
            .ok_or_else(|| AppError::Message(format!("远端 {host_key} 正在切换中，请稍候")))?;
        validate_pinned_provider(state, &app, provider_id.as_deref())?;

        let record = ensure_record(&state.db, &host_key, target, remote_port)?;
        let status = ensure_tunnel(state.db.clone(), state.proxy_service.clone(), &record).await?;
        let status = wait_for_first_attempt(status).await;
        if status.state == TunnelState::Error {
            if !host_has_enabled_routes(&state.db, &host_key)? {
                stop_tunnel(&host_key).await;
            }
            return Err(AppError::Message(format!(
                "SSH 隧道连接失败：{}",
                status.message.unwrap_or_default()
            )));
        }

        let provider = routed_provider(state, &app, provider_id.as_deref())?;
        let (written_files, remote_state) = {
            let state = state.clone();
            let app = app.clone();
            let record = record.clone();
            tokio::task::spawn_blocking(move || {
                push_gateway_config(&state, &app, &resolved, &record, provider.as_ref())
            })
            .await
            .map_err(|e| AppError::Message(format!("推送网关配置失败: {e}")))?
        }
        .inspect_err(|_| {
            if !host_has_enabled_routes(&state.db, &host_key).unwrap_or(true) {
                let host_key = host_key.clone();
                tokio::spawn(async move { stop_tunnel(&host_key).await });
            }
        })?;

        save_route(&state.db, &host_key, &app, true, provider_id.as_deref())?;
        Ok(RemoteGatewayApplyResult {
            state: Self::state_for(state, &app, &host_key).await?,
            remote_state: Some(remote_state),
            written_files,
        })
    }

    /// Switches which provider the host uses. Takes effect on the next
    /// request; only Codex rewrites its remote `model`.
    pub async fn set_provider(
        state: &AppState,
        app: AppType,
        target: &SshConnectionTarget,
        provider_id: Option<String>,
    ) -> Result<RemoteGatewayApplyResult, AppError> {
        ensure_remote_supported(&app)?;
        let resolved = resolve_tunnel_target(target)?;
        let host_key = resolved.label.clone();
        let (enabled, _) = load_route(&state.db, &host_key, &app)?;
        if !enabled {
            return Err(AppError::Message(format!(
                "{host_key} 的 {} 未启用本地网关",
                app.as_str()
            )));
        }
        validate_pinned_provider(state, &app, provider_id.as_deref())?;
        save_route(&state.db, &host_key, &app, true, provider_id.as_deref())?;

        let mut remote_state = None;
        let mut written_files = Vec::new();
        if matches!(app, AppType::Codex) {
            let record = load_record(&state.db, &host_key)?
                .ok_or_else(|| AppError::Message("远端网关记录不存在".to_string()))?;
            let provider = routed_provider(state, &app, provider_id.as_deref())?;
            let state_clone = state.clone();
            let app_clone = app.clone();
            let result = tokio::task::spawn_blocking(move || {
                push_gateway_config(
                    &state_clone,
                    &app_clone,
                    &resolved,
                    &record,
                    provider.as_ref(),
                )
            })
            .await
            .map_err(|e| AppError::Message(format!("推送网关配置失败: {e}")))?;
            match result {
                Ok((written, state_after)) => {
                    written_files = written;
                    remote_state = Some(state_after);
                }
                // The route already switched; the proxy fixes up the model.
                Err(e) => log::warn!("[RemoteGateway] Codex model push to {host_key} failed: {e}"),
            }
        }

        Ok(RemoteGatewayApplyResult {
            state: Self::state_for(state, &app, &host_key).await?,
            remote_state,
            written_files,
        })
    }

    /// Marks the route disabled and drops the tunnel once no app on the host
    /// uses it. The caller writes a direct provider config beforehand.
    pub async fn disable(
        state: &AppState,
        app: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteGatewayState, AppError> {
        ensure_remote_supported(&app)?;
        let host_key = resolve_ssh_target(target)?.label;
        let (_, provider_id) = load_route(&state.db, &host_key, &app)?;
        save_route(&state.db, &host_key, &app, false, provider_id.as_deref())?;
        if !host_has_enabled_routes(&state.db, &host_key)? {
            stop_tunnel(&host_key).await;
        }
        Self::state_for(state, &app, &host_key).await
    }

    /// Restarts the host's tunnel (e.g. after the network changed).
    pub async fn reconnect(
        state: &AppState,
        app: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteGatewayState, AppError> {
        ensure_remote_supported(&app)?;
        let host_key = resolve_tunnel_target(target)?.label;
        let record = load_record(&state.db, &host_key)?
            .ok_or_else(|| AppError::Message("远端网关尚未启用".to_string()))?;
        stop_tunnel(&host_key).await;
        let status = ensure_tunnel(state.db.clone(), state.proxy_service.clone(), &record).await?;
        wait_for_first_attempt(status).await;
        Self::state_for(state, &app, &host_key).await
    }

    /// Every host with gateway mode on, for the main-window indicator.
    pub async fn overview(state: &AppState) -> Result<RemoteGatewayOverview, AppError> {
        let mut hosts: Vec<RemoteGatewayHost> = Vec::new();
        for (host_key, app, provider_id) in enabled_routes(&state.db)? {
            if hosts.last().map(|host| &host.host_key) != Some(&host_key) {
                let Some(record) = load_record(&state.db, &host_key)? else {
                    continue;
                };
                hosts.push(RemoteGatewayHost {
                    tunnel: tunnel_status(&host_key).await,
                    host_key: host_key.clone(),
                    target: record.target,
                    remote_port: record.remote_port,
                    routes: Vec::new(),
                });
            }
            let provider_name = AppType::from_str(&app)
                .ok()
                .and_then(|app_type| {
                    routed_provider(state, &app_type, provider_id.as_deref())
                        .ok()
                        .flatten()
                })
                .map(|provider| provider.name);
            if let Some(host) = hosts.last_mut() {
                host.routes.push(RemoteGatewayRoute {
                    app,
                    provider_id,
                    provider_name,
                });
            }
        }
        Ok(RemoteGatewayOverview {
            proxy_running: state.proxy_service.is_running().await,
            hosts,
        })
    }

    /// Brings up tunnels for every host with an enabled route (app launch).
    pub async fn resume_all(state: &AppState) {
        let host_keys = match enabled_host_keys(&state.db) {
            Ok(keys) => keys,
            Err(e) => {
                log::warn!("[RemoteGateway] failed to list gateways: {e}");
                return;
            }
        };
        for host_key in host_keys {
            let record = match load_record(&state.db, &host_key) {
                Ok(Some(record)) => record,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("[RemoteGateway] failed to load gateway {host_key}: {e}");
                    continue;
                }
            };
            if let Err(e) =
                ensure_tunnel(state.db.clone(), state.proxy_service.clone(), &record).await
            {
                log::warn!("[RemoteGateway] failed to resume tunnel to {host_key}: {e}");
            }
        }
    }

    /// Stops every tunnel; called on app exit.
    pub async fn shutdown_all() {
        let tunnels: Vec<Tunnel> = {
            let mut manager = manager().lock().await;
            manager.listener = None;
            manager.tunnels.drain().map(|(_, tunnel)| tunnel).collect()
        };
        for tunnel in &tunnels {
            let _ = tunnel.stop.send(true);
        }
        for mut tunnel in tunnels {
            let _ = tokio::time::timeout(Duration::from_millis(500), async {
                while tunnel.status.borrow().state != TunnelState::Idle {
                    if tunnel.status.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real tunnel against `CC_SWITCH_GATEWAY_TEST_HOST` (an ssh config alias):
    /// the server curls through the reverse tunnel into a stub router.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a reachable SSH host"]
    async fn reverse_tunnel_reaches_local_listener() {
        let Ok(alias) = std::env::var("CC_SWITCH_GATEWAY_TEST_HOST") else {
            return;
        };
        let db = Arc::new(Database::memory().unwrap());
        let target = SshConnectionTarget {
            target_type: Some("config".to_string()),
            alias: Some(alias),
            host: None,
            user: None,
            port: None,
            password: None,
        };
        let host_key = resolve_ssh_target(&target).unwrap().label;
        let record = ensure_record(&db, &host_key, &target, None).unwrap();

        let router = axum::Router::new().fallback(|| async { "gateway-ok" });
        let source: RouterSource = Arc::new(move || {
            let router = router.clone();
            Box::pin(async move { Some(router) })
        });
        let listener = RemoteGatewayListener::start(db.clone(), source)
            .await
            .unwrap();

        let (status_tx, mut status_rx) =
            watch::channel(TunnelStatus::new(TunnelState::Connecting, None));
        let (stop_tx, stop_rx) = watch::channel(false);
        let resolved = resolve_tunnel_target(&target).unwrap();
        let task = tokio::spawn(supervise_tunnel(
            resolved.clone(),
            record.remote_port,
            listener.port,
            status_tx,
            stop_rx,
        ));
        while status_rx.borrow().state == TunnelState::Connecting {
            status_rx.changed().await.unwrap();
        }
        assert_eq!(
            status_rx.borrow().state,
            TunnelState::Connected,
            "{:?}",
            status_rx.borrow().message
        );

        let curl = |auth: String| {
            let resolved = resolved.clone();
            let url = format!("{}/v1/messages", gateway_url(record.remote_port));
            tokio::task::spawn_blocking(move || {
                super::super::remote::run_ssh_command(
                    &resolved,
                    &format!("curl -s -H '{auth}' {url}"),
                    None,
                )
            })
        };
        let ok = curl(format!("Authorization: Bearer {}", record.token))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ok.trim(), "gateway-ok");
        let denied = curl("Authorization: Bearer wrong".to_string())
            .await
            .unwrap()
            .unwrap();
        assert!(denied.contains("authentication_error"), "{denied}");

        stop_tx.send(true).unwrap();
        task.await.unwrap();
        assert_eq!(status_rx.borrow().state, TunnelState::Idle);
    }

    #[test]
    fn random_remote_port_stays_in_range() {
        for _ in 0..1000 {
            assert!(REMOTE_PORT_RANGE.contains(&random_remote_port()));
        }
    }

    #[test]
    fn tunnel_failure_maps_forwarding_error() {
        let message = describe_tunnel_failure(
            Some("Error: remote port forwarding failed for listen port 23456".to_string()),
            Some(255),
            23456,
        );
        assert!(message.contains("23456"));
        assert!(message.contains("占用"));
        assert_eq!(
            describe_tunnel_failure(None, Some(255), 1),
            "ssh 已退出（代码 255）"
        );
    }

    #[test]
    fn gemini_gateway_writes_replace_key_and_keep_other_env() {
        use super::super::remote::{GEMINI_ENV_PATH, GEMINI_SETTINGS_PATH};

        let snapshot = RemoteSnapshot {
            files: vec![
                (
                    GEMINI_ENV_PATH,
                    Some("GOOGLE_API_KEY=old\nGEMINI_API_KEY=old\nFOO=bar\n".to_string()),
                ),
                (GEMINI_SETTINGS_PATH, None),
            ],
        };
        let projection = GeminiProjection::proxy_contract(
            &GeminiProjection::empty(),
            "http://127.0.0.1:23456",
            "tok",
        );
        let writes = gemini_remote_writes(&projection, &snapshot).unwrap();
        let env = writes[0].content.as_deref().unwrap();
        assert!(env.contains("GEMINI_API_KEY=tok"));
        assert!(env.contains("GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:23456"));
        assert!(env.contains("FOO=bar"));
        assert!(!env.contains("GOOGLE_API_KEY"));
        assert!(writes[1]
            .content
            .as_deref()
            .unwrap()
            .contains("gemini-api-key"));
    }
}
