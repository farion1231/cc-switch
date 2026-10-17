//! REST 控制面服务生命周期（镜像 `ProxyService`）。
//!
//! 与数据面 `ProxyServer` 共享 Tauri tokio runtime，但独立 listener、独立 Router。
//! 仅 `rest_api` feature on 时编译。

use std::sync::Arc;

use serde::Serialize;
use tauri::async_runtime;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, RwLock};

use crate::rest::auth::SharedRestConfig;
use crate::rest::config::RestConfig;
use crate::rest::routes;
use crate::settings;
use crate::store::AppState;

/// axum 复合路由状态——持 AppState + 自定义 Tauri-managed state 的 Arc。
///
/// 桌面端 Tauri `State<X>` 与 REST 端 `State<X>` 共享同一 Arc 实例（有态服务
/// 如 CopilotAuthManager 的缓存/锁在两端一致）。
/// 各子状态经 `FromRef<RestState>` 暴露给 axum `State<T>` 提取器。
#[derive(Clone)]
pub struct RestState {
    pub app: AppState,
    pub skill: std::sync::Arc<crate::services::skill::SkillService>,
    pub copilot: std::sync::Arc<
        tokio::sync::RwLock<crate::proxy::providers::copilot_auth::CopilotAuthManager>,
    >,
    pub xai: std::sync::Arc<
        tokio::sync::RwLock<crate::proxy::providers::xai_oauth_auth::XaiOAuthManager>,
    >,
}

impl axum::extract::FromRef<RestState> for AppState {
    fn from_ref(s: &RestState) -> Self {
        s.app.clone()
    }
}

impl axum::extract::FromRef<RestState> for crate::commands::skill::SkillServiceState {
    fn from_ref(s: &RestState) -> Self {
        Self(s.skill.clone())
    }
}

impl axum::extract::FromRef<RestState> for crate::commands::copilot::CopilotAuthState {
    fn from_ref(s: &RestState) -> Self {
        Self(s.copilot.clone())
    }
}

impl axum::extract::FromRef<RestState> for crate::commands::codex_oauth::CodexOAuthState {
    fn from_ref(s: &RestState) -> Self {
        Self(s.app.codex_oauth_manager.clone())
    }
}

impl axum::extract::FromRef<RestState> for crate::commands::xai_oauth::XaiOAuthState {
    fn from_ref(s: &RestState) -> Self {
        Self(s.xai.clone())
    }
}

/// REST 服务器运行信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestInfo {
    pub address: String,
    pub port: u16,
    pub enabled: bool,
}

/// 后台 listener 句柄：JoinHandle + 优雅关闭信号。
struct RestServerHandle {
    join: async_runtime::JoinHandle<()>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

/// REST 控制面服务（单例，作 Tauri managed state）。
#[derive(Clone)]
pub struct RestService {
    state: RestState,
    config: SharedRestConfig,
    server: Arc<RwLock<Option<RestServerHandle>>>,
}

impl RestService {
    pub fn new(state: RestState) -> Self {
        let cfg = settings::get_settings().rest_api.clone();
        Self {
            state,
            config: Arc::new(RwLock::new(cfg)),
            server: Arc::new(RwLock::new(None)),
        }
    }

    /// 当前共享配置（供 `set_rest_config` 命令热更新）。
    pub fn shared_config(&self) -> SharedRestConfig {
        self.config.clone()
    }

    /// 启动 listener。读当前 config；`enabled=false` 时直接返回（不起）。
    pub async fn start(&self) -> Result<Option<RestInfo>, String> {
        let cfg = self.config.read().await.clone();
        if !cfg.enabled {
            return Ok(None);
        }
        // 已在运行：返回当前
        if self.server.read().await.is_some() {
            return Ok(Some(info_from(&cfg)));
        }

        let addr = cfg.bind_addr().map_err(|e| e.to_string())?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("REST 监听 {addr} 失败: {e}"))?;
        let bound_port = listener
            .local_addr()
            .map_err(|e| format!("获取监听端口失败: {e}"))?
            .port();

        let router = routes::router(self.state.clone(), self.config.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });

        let join = async_runtime::spawn(async move {
            if let Err(e) = serve.await {
                log::error!("[REST] axum serve 退出: {e}");
            }
        });

        *self.server.write().await = Some(RestServerHandle {
            join,
            shutdown_tx: Some(shutdown_tx),
        });

        log::info!("[REST] 控制面已启动: {}:{}", cfg.listen_address, bound_port);
        Ok(Some(RestInfo {
            address: cfg.listen_address.clone(),
            port: bound_port,
            enabled: true,
        }))
    }

    /// 停止 listener。
    pub async fn stop(&self) -> Result<(), String> {
        if let Some(mut handle) = self.server.write().await.take() {
            if let Some(tx) = handle.shutdown_tx.take() {
                let _ = tx.send(());
            }
            let _ = handle.join.await;
            log::info!("[REST] 控制面已停止");
        }
        Ok(())
    }

    /// 是否在运行。
    pub async fn is_running(&self) -> bool {
        self.server.read().await.is_some()
    }

    /// 应用新配置：热更新 token（写入共享 RwLock）；地址/端口变更则 restart listener；
    /// `enabled` 由 true→false 则 stop。
    pub async fn apply_config(&self, new: RestConfig) -> Result<(), String> {
        let old = self.config.read().await.clone();
        let addr_changed =
            old.listen_address != new.listen_address || old.listen_port != new.listen_port;
        *self.config.write().await = new.clone();

        let was_running = self.is_running().await;
        match (was_running, new.enabled, addr_changed) {
            (true, false, _) => {
                self.stop().await?;
            }
            (true, true, true) => {
                self.stop().await?;
                self.start().await?;
            }
            (false, true, _) => {
                self.start().await?;
            }
            _ => {
                // 运行中且仅 token/host 名变了（非地址/端口）：热生效，无需 restart。
            }
        }
        Ok(())
    }
}

fn info_from(cfg: &RestConfig) -> RestInfo {
    RestInfo {
        address: cfg.listen_address.clone(),
        port: cfg.listen_port,
        enabled: cfg.enabled,
    }
}
