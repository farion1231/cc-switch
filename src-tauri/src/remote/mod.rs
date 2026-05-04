//! Remote Management HTTP Server
//!
//! 轻量 Web 服务，通过手机/远程浏览器切换 provider。
//! 直接调用 ProviderService::switch()，确保 backfill 和状态一致性。
//! 彻底解决独立 Python remote 绕过桌面端状态管理导致的竞态覆盖问题。

mod handlers;
mod html;

use axum::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};

/// Remote server 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteConfig {
    /// 是否启用远程管理服务
    pub enabled: bool,
    /// 监听端口
    pub port: u16,
    /// 是否同时监听 Tailscale IP（远程访问）
    pub tailscale_enabled: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 4000,
            tailscale_enabled: false,
        }
    }
}

/// Remote server 共享状态
pub struct RemoteState {
    /// Tauri AppHandle，用于访问 managed AppState 和发射事件
    pub app_handle: tauri::AppHandle,
    /// SSE 广播通道发送端
    pub sse_tx: broadcast::Sender<String>,
    /// 运行状态标志：stop() 后设为 false，拒绝新请求
    pub running: std::sync::atomic::AtomicBool,
}

struct ListenerEntry {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

/// Remote 管理服务器
pub struct RemoteServer {
    state: Arc<RemoteState>,
    /// 按地址独立管理的 listener：支持动态增删，Tailscale 切换时不中断 localhost
    listeners: Arc<RwLock<HashMap<SocketAddr, ListenerEntry>>>,
    config: RwLock<RemoteConfig>,
}

/// Tauri managed state wrapper
pub struct ManagedRemoteServer(Arc<RwLock<Option<RemoteServer>>>);

impl ManagedRemoteServer {
    pub fn new(inner: Arc<RwLock<Option<RemoteServer>>>) -> Self {
        Self(inner)
    }

    pub fn lock(&self) -> &Arc<RwLock<Option<RemoteServer>>> {
        &self.0
    }
}

/// 获取 Tailscale IPv4 地址
/// 依赖系统 PATH 解析 tailscale 命令
pub fn get_tailscale_ip() -> Option<String> {
    std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|ip| !ip.is_empty())
}

/// Check if Tailscale CLI is available and returns an IP
pub fn is_tailscale_available() -> bool {
    get_tailscale_ip().is_some()
}

/// Start or update the remote server.
/// If a server already exists, performs an incremental update:
/// - keeps existing listeners (e.g. localhost) that are still needed
/// - only starts/stops the changed addresses (e.g. Tailscale IP)
pub async fn start_remote(
    app: &tauri::AppHandle,
    config: RemoteConfig,
) -> Result<Vec<String>, String> {
    use tauri::Manager;

    let managed = app.state::<ManagedRemoteServer>();
    let mut guard = managed.lock().write().await;

    if let Some(server) = guard.as_ref() {
        // 已有 server：增量更新，不中断保留的地址
        log::info!("[Remote] Updating remote server configuration...");
        let urls = server.update(config).await?;
        log::info!("[Remote] Server updated on: {}", urls.join(", "));
        Ok(urls)
    } else {
        // 首次启动
        let new_server = RemoteServer::new(app.clone(), config);
        let urls = new_server.start().await?;
        *guard = Some(new_server);
        log::info!("[Remote] Server started on: {}", urls.join(", "));
        Ok(urls)
    }
}

/// Stop the remote server
pub async fn stop_remote(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let managed = app.state::<ManagedRemoteServer>();
    let mut guard = managed.lock().write().await;

    if let Some(server) = guard.take() {
        // 用户显式关闭：先广播 shutdown 让浏览器主动销毁页面
        match server
            .state()
            .sse_tx
            .send(r#"{"type":"shutdown"}"#.to_string())
        {
            Ok(n) => log::info!("[Remote] Broadcasted shutdown to {} SSE clients", n),
            Err(e) => log::warn!("[Remote] No SSE clients to broadcast shutdown: {e}"),
        }
        // 给 SSE 消息一点时间到达客户端
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        server.stop().await?;
    }

    log::info!("[Remote] Server stopped via command");
    Ok(())
}

impl RemoteServer {
    fn new(app_handle: tauri::AppHandle, config: RemoteConfig) -> Self {
        let (sse_tx, _) = broadcast::channel(16);

        let state = Arc::new(RemoteState {
            app_handle,
            sse_tx,
            running: std::sync::atomic::AtomicBool::new(true),
        });

        Self {
            state,
            listeners: Arc::new(RwLock::new(HashMap::new())),
            config: RwLock::new(config),
        }
    }

    /// 根据配置计算目标监听地址列表
    fn compute_target_addrs(config: &RemoteConfig) -> Vec<SocketAddr> {
        let mut addrs = Vec::new();

        // 始终包含 localhost
        if let Ok(addr) = format!("127.0.0.1:{}", config.port).parse() {
            addrs.push(addr);
        }

        // 可选：添加 Tailscale IP
        if config.tailscale_enabled {
            if let Some(ts_ip) = get_tailscale_ip() {
                if let Ok(addr) = format!("{}:{}", ts_ip, config.port).parse() {
                    addrs.push(addr);
                }
            }
        }

        addrs
    }

    /// 启动指定地址的 listener（如果已在运行则跳过）
    async fn start_addrs(&self, addrs: Vec<SocketAddr>) -> Vec<String> {
        let port = self.config.read().await.port;
        let app = self.build_router(port);
        let mut started_urls = Vec::new();
        let mut listeners = self.listeners.write().await;

        for addr in addrs {
            if listeners.contains_key(&addr) {
                started_urls.push(format!("http://{addr}"));
                continue;
            }

            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

                    let app_clone = app.clone();
                    let handle = tokio::spawn(async move {
                        let serve = axum::serve(listener, app_clone);
                        let graceful = serve.with_graceful_shutdown(async move {
                            let _ = shutdown_rx.await;
                        });
                        if let Err(e) = graceful.await {
                            log::warn!("[Remote] Server error on {addr}: {e}");
                        }
                    });

                    listeners.insert(
                        addr,
                        ListenerEntry {
                            shutdown_tx,
                            handle,
                        },
                    );
                    started_urls.push(format!("http://{addr}"));
                    log::info!("[Remote] Listening on {addr}");
                }
                Err(e) => {
                    log::warn!("[Remote] Failed to bind {addr}: {e}");
                }
            }
        }

        started_urls
    }

    /// 停止指定地址的 listener
    async fn stop_addrs(&self, addrs: Vec<SocketAddr>) {
        let mut listeners = self.listeners.write().await;

        for addr in addrs {
            if let Some(entry) = listeners.remove(&addr) {
                let _ = entry.shutdown_tx.send(());
                let abort_handle = entry.handle.abort_handle();

                match tokio::time::timeout(std::time::Duration::from_secs(3), entry.handle).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::warn!("[Remote] Listener task on {addr} panicked: {e}"),
                    Err(_) => {
                        log::warn!("[Remote] Listener on {addr} stop timeout, aborting");
                        abort_handle.abort();
                    }
                }

                log::info!("[Remote] Stopped listening on {addr}");
            }
        }
    }

    /// 启动所有配置中的地址（首次启动用）
    async fn start(&self) -> Result<Vec<String>, String> {
        let config = self.config.read().await.clone();
        let addrs = Self::compute_target_addrs(&config);
        let urls = self.start_addrs(addrs).await;

        if urls.is_empty() {
            return Err("Failed to bind any address".to_string());
        }

        Ok(urls)
    }

    /// 增量更新：根据新配置增删 listener，不中断保留的地址
    async fn update(&self, new_config: RemoteConfig) -> Result<Vec<String>, String> {
        let current_addrs: Vec<SocketAddr> = self.listeners.read().await.keys().cloned().collect();
        let target_addrs = Self::compute_target_addrs(&new_config);

        // 停止不再需要的地址
        let to_stop: Vec<SocketAddr> = current_addrs
            .iter()
            .filter(|a| !target_addrs.contains(a))
            .cloned()
            .collect();
        if !to_stop.is_empty() {
            self.stop_addrs(to_stop).await;
        }

        // 启动新地址
        let to_start: Vec<SocketAddr> = target_addrs
            .iter()
            .filter(|a| !current_addrs.contains(a))
            .cloned()
            .collect();
        let urls: Vec<String> = target_addrs.iter().map(|a| format!("http://{a}")).collect();

        if !to_start.is_empty() {
            self.start_addrs(to_start).await;
        }

        // 更新配置
        *self.config.write().await = new_config;

        Ok(urls)
    }

    async fn stop(&self) -> Result<(), String> {
        // 先标记为 stopped，拒绝已建立连接上的新请求
        self.state
            .running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        log::info!("[Remote] Running flag set to false");

        let addrs: Vec<SocketAddr> = self.listeners.read().await.keys().cloned().collect();
        self.stop_addrs(addrs).await;

        log::info!("[Remote] Server stopped");
        Ok(())
    }

    fn state(&self) -> Arc<RemoteState> {
        self.state.clone()
    }

    fn build_router(&self, port: u16) -> Router {
        use axum::http::HeaderValue;

        let mut origins = vec![
            // The page served by this server itself
            format!("http://127.0.0.1:{port}")
                .parse::<HeaderValue>()
                .expect("valid origin"),
            // Tauri webview: production (macOS/Linux)
            "tauri://localhost".parse::<HeaderValue>().expect("valid origin"),
            // Tauri webview: production (Windows)
            "https://tauri.localhost"
                .parse::<HeaderValue>()
                .expect("valid origin"),
        ];

        // Tauri webview: dev server (only in debug builds)
        #[cfg(debug_assertions)]
        origins.push(
            "http://localhost:3000"
                .parse::<HeaderValue>()
                .expect("valid origin"),
        );

        // Allow Tailscale origin when enabled, so the Tauri app's health checks
        // work even when the server is also bound to the Tailscale address.
        if let Ok(config) = self.config.try_read() {
            if config.tailscale_enabled {
                if let Some(ts_ip) = get_tailscale_ip() {
                    if let Ok(hv) = format!("http://{}:{port}", ts_ip).parse::<HeaderValue>() {
                        origins.push(hv);
                    }
                }
            }
        }

        let cors = CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            .route("/", axum::routing::get(handlers::index))
            .route(
                "/api/providers",
                axum::routing::get(handlers::get_providers),
            )
            .route(
                "/api/switch",
                axum::routing::post(handlers::switch_provider),
            )
            .route("/api/current", axum::routing::get(handlers::get_current))
            .route("/api/events", axum::routing::get(handlers::sse_events))
            .route("/api/health", axum::routing::get(handlers::health_check))
            .route("/api/icon", axum::routing::get(handlers::get_icon))
            .route(
                "/api/provider-icons/:name",
                axum::routing::get(handlers::get_provider_icon),
            )
            .layer(cors)
            .with_state(self.state.clone())
    }
}

/// App 内切换 provider 后，直接广播 SSE 给所有已连接的远程浏览器（App → Web 方向）。
/// 在 tray.rs / failover.rs 等切换点调用此函数，替代无效的 app_handle.listen() 方案
/// （app_handle.emit() 只发给 JS 前端，不触发 Rust listen 回调）。
///
/// 函数内部会从 AppState 查询 provider 名称，调用方只需传入 provider_id 和 app_type。
pub async fn broadcast_provider_switch(
    app: &tauri::AppHandle,
    app_type_str: &str,
    provider_id: &str,
) {
    use tauri::Manager;

    let managed = match app.try_state::<ManagedRemoteServer>() {
        Some(m) => m,
        None => {
            log::warn!("[Remote] broadcast_provider_switch: ManagedRemoteServer not found");
            return;
        }
    };

    let guard = managed.lock().read().await;
    let server = match guard.as_ref() {
        Some(s) => s,
        None => {
            log::warn!("[Remote] broadcast_provider_switch: RemoteServer not started");
            return;
        }
    };

    let name: String = app
        .try_state::<crate::store::AppState>()
        .and_then(|s| {
            s.db.get_all_providers(app_type_str)
                .ok()
                .and_then(|map| map.get(provider_id).map(|p| p.name.clone()))
        })
        .unwrap_or_else(|| provider_id.to_string());

    let msg = serde_json::json!({
        "type": "switch",
        "provider_id": provider_id,
        "name": name,
    })
    .to_string();

    match server.state().sse_tx.send(msg) {
        Ok(_) => {
            log::info!(
                "[Remote] Broadcasted provider switch: {} (id={})",
                name,
                provider_id
            );
        }
        Err(e) => {
            log::error!("[Remote] Failed to broadcast provider switch: {}", e);
        }
    }
}
