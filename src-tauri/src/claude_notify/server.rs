use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::settings::{get_settings, update_settings};

use super::dedupe::ClaudeNotifyDedupe;
use super::toast;
use super::types::{ClaudeNotifyEventType, ClaudeNotifyPayload, ClaudeNotifyRuntimeStatus};

#[derive(Clone)]
pub struct ClaudeNotifyServiceState {
    pub runtime: Arc<RwLock<ClaudeNotifyRuntimeStatus>>,
    pub dedupe: Arc<Mutex<ClaudeNotifyDedupe>>,
    pub app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

pub struct ClaudeNotifyService {
    state: ClaudeNotifyServiceState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    lifecycle_lock: Arc<Mutex<()>>,
}

impl ClaudeNotifyService {
    pub fn new() -> Self {
        Self {
            state: ClaudeNotifyServiceState {
                runtime: Arc::new(RwLock::new(ClaudeNotifyRuntimeStatus::default())),
                dedupe: Arc::new(Mutex::new(ClaudeNotifyDedupe::default())),
                app_handle: Arc::new(RwLock::new(None)),
            },
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
            lifecycle_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.state.app_handle.write().await = Some(handle);
    }

    pub async fn ensure_started(&self) -> Result<ClaudeNotifyRuntimeStatus, String> {
        let _guard = self.lifecycle_lock.lock().await;
        if self.is_running().await {
            return Ok(self.get_status().await);
        }

        self.start_inner().await
    }

    pub async fn stop(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.stop_inner().await
    }

    pub async fn sync_with_settings(&self) -> Result<ClaudeNotifyRuntimeStatus, String> {
        let _guard = self.lifecycle_lock.lock().await;
        let settings = get_settings();
        if settings.enable_claude_background_notifications {
            if self.is_running().await {
                return Ok(self.get_status().await);
            }
            self.start_inner().await
        } else {
            self.stop_inner().await?;
            Ok(self.get_status().await)
        }
    }

    pub async fn get_status(&self) -> ClaudeNotifyRuntimeStatus {
        self.state.runtime.read().await.clone()
    }

    async fn is_running(&self) -> bool {
        self.shutdown_tx.read().await.is_some() && self.get_status().await.listening
    }

    async fn start_inner(&self) -> Result<ClaudeNotifyRuntimeStatus, String> {
        let (port, listener) = self.bind_listener().await?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let app = self.build_router();
        let runtime = self.state.runtime.clone();
        let shutdown_tx_ref = self.shutdown_tx.clone();
        let server_handle_ref = self.server_handle.clone();

        {
            let mut status = runtime.write().await;
            status.listening = true;
            status.port = Some(port);
        }

        *self.shutdown_tx.write().await = Some(shutdown_tx);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();

            *shutdown_tx_ref.write().await = None;
            *server_handle_ref.write().await = None;

            let mut status = runtime.write().await;
            status.listening = false;
        });

        *self.server_handle.write().await = Some(handle);
        Ok(self.state.runtime.read().await.clone())
    }

    async fn stop_inner(&self) -> Result<(), String> {
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        } else {
            let mut status = self.state.runtime.write().await;
            status.listening = false;
            return Ok(());
        }

        let handle = { self.server_handle.write().await.take() };
        if let Some(handle) = handle {
            tokio::time::timeout(std::time::Duration::from_secs(3), handle)
                .await
                .map_err(|_| "关闭 Claude 通知监听超时".to_string())
                .map(|_| ())?;
        }

        let mut status = self.state.runtime.write().await;
        status.listening = false;
        Ok(())
    }

    async fn bind_listener(&self) -> Result<(u16, tokio::net::TcpListener), String> {
        let mut settings = get_settings();

        if let Some(port) = settings.claude_notify_port {
            let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
            if let Ok(listener) = tokio::net::TcpListener::bind(addr).await {
                let actual_port = listener
                    .local_addr()
                    .map_err(|e| format!("读取 Claude 通知监听端口失败: {e}"))?
                    .port();

                if settings.claude_notify_port != Some(actual_port) {
                    settings.claude_notify_port = Some(actual_port);
                    update_settings(settings).map_err(|e| e.to_string())?;
                }

                return Ok((actual_port, listener));
            }
        }

        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .map_err(|e| format!("绑定 Claude 通知监听失败: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("读取 Claude 通知监听端口失败: {e}"))?
            .port();

        settings.claude_notify_port = Some(port);
        update_settings(settings).map_err(|e| e.to_string())?;

        Ok((port, listener))
    }

    fn build_router(&self) -> Router {
        Router::new()
            .route("/hooks/claude-notify", post(handle_notify))
            .route("/hooks/claude-notify/health", get(handle_health))
            .with_state(self.state.clone())
    }
}

async fn handle_health(
    State(state): State<ClaudeNotifyServiceState>,
) -> Json<ClaudeNotifyRuntimeStatus> {
    Json(state.runtime.read().await.clone())
}

async fn handle_notify(
    State(state): State<ClaudeNotifyServiceState>,
    Json(payload): Json<ClaudeNotifyPayload>,
) -> StatusCode {
    if payload.source_app != "claude-code" {
        return StatusCode::BAD_REQUEST;
    }

    let Some(event_type) = payload.normalized_event_type() else {
        return StatusCode::BAD_REQUEST;
    };

    let has_session = payload
        .session_id
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !has_session {
        return StatusCode::BAD_REQUEST;
    }

    let valid_combo = match event_type {
        ClaudeNotifyEventType::PermissionPrompt => {
            payload.notification_type.as_deref() == Some("permission_prompt")
        }
        ClaudeNotifyEventType::IdlePrompt => {
            payload.notification_type.as_deref() == Some("idle_prompt")
        }
        ClaudeNotifyEventType::Stop => payload.notification_type.is_none(),
    };

    if !valid_combo {
        return StatusCode::BAD_REQUEST;
    }

    let settings = get_settings();
    if !settings.enable_claude_background_notifications {
        return StatusCode::NO_CONTENT;
    }

    let allowed = match event_type {
        ClaudeNotifyEventType::PermissionPrompt => {
            settings.enable_claude_permission_prompt_notifications
        }
        ClaudeNotifyEventType::IdlePrompt | ClaudeNotifyEventType::Stop => {
            settings.enable_claude_round_complete_notifications
        }
    };

    if !allowed {
        return StatusCode::NO_CONTENT;
    }

    let session_id = payload.session_id.as_deref().unwrap_or_default();
    let should_emit = {
        let mut dedupe = state.dedupe.lock().await;
        dedupe.should_emit(session_id, &event_type)
    };

    if !should_emit {
        return StatusCode::NO_CONTENT;
    }

    if let Some(app) = state.app_handle.read().await.clone() {
        if let Err(err) = toast::show_toast(&app, &event_type) {
            log::warn!("显示 Claude Toast 失败，已忽略：{err}");
        }
    } else {
        log::warn!("Claude 通知事件已收到，但 AppHandle 尚未绑定，已跳过显示 Toast");
    }

    StatusCode::NO_CONTENT
}
