//! HTTP代理服务器
//!
//! 基于Axum的HTTP服务器，处理代理请求

use super::{handlers, types::*, ProxyError};
use crate::database::Database;
use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use tower_http::cors::{Any, CorsLayer};

/// 代理服务器状态（共享）
#[derive(Clone)]
pub struct ProxyState {
    pub db: Arc<Database>,
    pub config: Arc<RwLock<ProxyConfig>>,
    pub status: Arc<RwLock<ProxyStatus>>,
    pub start_time: Arc<RwLock<Option<std::time::Instant>>>,
}

/// 代理HTTP服务器
pub struct ProxyServer {
    config: ProxyConfig,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
}

impl ProxyServer {
    pub fn new(config: ProxyConfig, db: Arc<Database>) -> Self {
        let state = ProxyState {
            db,
            config: Arc::new(RwLock::new(config.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
        };

        Self {
            config,
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start(&self) -> Result<ProxyServerInfo, ProxyError> {
        // 检查是否已在运行
        if self.shutdown_tx.read().await.is_some() {
            return Err(ProxyError::AlreadyRunning);
        }

        let addr: SocketAddr =
            format!("{}:{}", self.config.listen_address, self.config.listen_port)
                .parse()
                .map_err(|e| ProxyError::BindFailed(format!("无效的地址: {e}")))?;

        // 创建关闭通道
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // 构建路由
        let app = self.build_router();

        // 绑定监听器
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::BindFailed(e.to_string()))?;

        log::info!("代理服务器启动于 {addr}");

        // 保存关闭句柄
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // 更新状态
        let mut status = self.state.status.write().await;
        status.running = true;
        status.address = self.config.listen_address.clone();
        status.port = self.config.listen_port;
        drop(status);

        // 记录启动时间
        *self.state.start_time.write().await = Some(std::time::Instant::now());

        // 启动服务器
        let state = self.state.clone();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .ok();

            // 服务器停止后更新状态
            state.status.write().await.running = false;
            *state.start_time.write().await = None;
        });

        Ok(ProxyServerInfo {
            address: self.config.listen_address.clone(),
            port: self.config.listen_port,
            started_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub async fn stop(&self) -> Result<(), ProxyError> {
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
            Ok(())
        } else {
            Err(ProxyError::NotRunning)
        }
    }

    pub async fn get_status(&self) -> ProxyStatus {
        let mut status = self.state.status.read().await.clone();

        // 计算运行时间
        if let Some(start) = *self.state.start_time.read().await {
            status.uptime_seconds = start.elapsed().as_secs();
        }

        // 获取所有活跃的代理目标
        if let Ok(targets) = self.state.db.get_all_proxy_targets() {
            status.active_targets = targets
                .into_iter()
                .map(|(app_type, name, id)| ActiveTarget {
                    app_type,
                    provider_name: name,
                    provider_id: id,
                })
                .collect();
        }

        status
    }

    fn build_router(&self) -> Router {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            // 健康检查
            .route("/health", get(handlers::health_check))
            .route("/status", get(handlers::get_status))
            // Claude API
            .route("/v1/messages", post(handlers::handle_messages))
            // OpenAI Chat Completions API (Codex CLI)
            .route(
                "/v1/chat/completions",
                post(handlers::handle_chat_completions),
            )
            // OpenAI Responses API (Codex CLI)
            .route("/v1/responses", post(handlers::handle_responses))
            // Gemini API
            .route("/v1beta/*path", post(handlers::handle_gemini))
            .layer(cors)
            .with_state(self.state.clone())
    }

    /// 在不重启服务的情况下更新运行时配置
    pub async fn apply_runtime_config(&self, config: &ProxyConfig) {
        *self.state.config.write().await = config.clone();
    }
}
