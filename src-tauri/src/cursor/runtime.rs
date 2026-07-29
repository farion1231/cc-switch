use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

use crate::config::get_app_config_dir;
use crate::cursor::projector;
use crate::cursor::types::{
    CursorModelTestResult, CursorRuntimeState, CursorUsageEventPage, SidecarConfig,
    SidecarModelAdapter, SidecarRuntimeState,
};
use crate::database::Database;
use crate::error::AppError;

const READY_PREFIX: &str = "CC_SWITCH_SIDECAR_READY ";
const SIDECAR_NAME: &str = "cursor-sidecar";
const START_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct ReadyPayload {
    address: String,
}

struct RuntimeInner {
    phase: String,
    generation: u64,
    state_observed: bool,
    child: Option<CommandChild>,
    base_url: Option<String>,
    auth_token: Option<String>,
    state: CursorRuntimeState,
}

#[derive(Clone)]
pub struct CursorRuntimeService {
    db: Arc<Database>,
    client: reqwest::Client,
    inner: Arc<Mutex<RuntimeInner>>,
    lifecycle: Arc<Mutex<()>>,
    usage_sync_active: Arc<AtomicBool>,
}

impl CursorRuntimeService {
    pub fn new(db: Arc<Database>) -> Self {
        let platform = std::env::consts::OS.to_string();
        Self {
            db,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            inner: Arc::new(Mutex::new(RuntimeInner {
                phase: "stopped".to_string(),
                generation: 0,
                state_observed: false,
                child: None,
                base_url: None,
                auth_token: None,
                state: CursorRuntimeState {
                    phase: "stopped".to_string(),
                    platform,
                    ..CursorRuntimeState::default()
                },
            })),
            lifecycle: Arc::new(Mutex::new(())),
            usage_sync_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn start(&self, app: &AppHandle) -> Result<CursorRuntimeState, AppError> {
        let _lifecycle_guard = self.lifecycle.lock().await;
        if self.is_running().await {
            return Ok(self.inner.lock().await.state.clone());
        }

        let config = projector::project_enabled_models(&self.db)?;
        if config.model_adapters.is_empty() {
            return self.fail("请先启用至少一个 Cursor 模型").await;
        }

        let (generation, events) = self.launch_sidecar(app, "starting").await?;
        if let Err(error) = self.put_config(&config).await {
            self.force_stop().await;
            return self.fail(error.to_string()).await;
        }
        if let Err(error) = self.post_empty("/v1/start").await {
            self.force_stop().await;
            return self.fail(error.to_string()).await;
        }
        let state = match self.refresh_state().await {
            Ok(state) => state,
            Err(error) => {
                self.restore_after_failed_start().await;
                return self.fail(error.to_string()).await;
            }
        };
        self.spawn_event_monitor(app.clone(), generation, events);
        Ok(state)
    }

    async fn launch_sidecar(
        &self,
        app: &AppHandle,
        phase: &str,
    ) -> Result<(u64, tokio::sync::mpsc::Receiver<CommandEvent>), AppError> {
        let generation = {
            let mut inner = self.inner.lock().await;
            if let Some(child) = inner.child.take() {
                let _ = child.kill();
            }
            inner.generation = inner.generation.saturating_add(1);
            inner.base_url = None;
            inner.auth_token = None;
            inner.phase = phase.to_string();
            inner.state.phase = inner.phase.clone();
            inner.state.last_error.clear();
            inner.generation
        };

        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let data_dir = get_app_config_dir().join("cursor-runtime");
        if let Err(error) = std::fs::create_dir_all(&data_dir) {
            return self
                .fail(format!(
                    "创建 Cursor sidecar 数据目录失败 '{}': {error}",
                    data_dir.display()
                ))
                .await;
        }
        let sidecar = match app.shell().sidecar(SIDECAR_NAME) {
            Ok(command) => command.args([
                "--listen".to_string(),
                "127.0.0.1:0".to_string(),
                "--auth-token".to_string(),
                token.clone(),
                "--data-dir".to_string(),
                data_dir.to_string_lossy().to_string(),
                "--parent-pid".to_string(),
                std::process::id().to_string(),
            ]),
            Err(error) => {
                return self
                    .fail(format!("创建 Cursor sidecar 命令失败: {error}"))
                    .await;
            }
        };
        let (mut events, child) = match sidecar.spawn() {
            Ok(process) => process,
            Err(error) => {
                return self
                    .fail(format!("启动 Cursor sidecar 失败: {error}"))
                    .await;
            }
        };

        let ready_result = tokio::time::timeout(START_TIMEOUT, async {
            while let Some(event) = events.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => {
                        let line = String::from_utf8_lossy(&bytes);
                        if let Some(payload) = line.trim().strip_prefix(READY_PREFIX) {
                            let ready: ReadyPayload =
                                serde_json::from_str(payload).map_err(|error| {
                                    AppError::Message(format!(
                                        "解析 Cursor sidecar ready 消息失败: {error}"
                                    ))
                                })?;
                            return Ok(ready);
                        }
                        log::debug!("[cursor-sidecar] {}", line.trim());
                    }
                    CommandEvent::Stderr(bytes) => {
                        log::warn!(
                            "[cursor-sidecar] {}",
                            String::from_utf8_lossy(&bytes).trim()
                        );
                    }
                    CommandEvent::Terminated(payload) => {
                        return Err(AppError::Message(format!(
                            "Cursor sidecar 在就绪前退出: code={:?} signal={:?}",
                            payload.code, payload.signal
                        )));
                    }
                    _ => {}
                }
            }
            Err(AppError::Message("Cursor sidecar 输出流已关闭".to_string()))
        })
        .await;
        let ready = match ready_result {
            Ok(Ok(ready)) => ready,
            Ok(Err(error)) => {
                let _ = child.kill();
                return self.fail(error.to_string()).await;
            }
            Err(_) => {
                let _ = child.kill();
                return self.fail("等待 Cursor sidecar 就绪超时").await;
            }
        };

        let mut inner = self.inner.lock().await;
        inner.child = Some(child);
        inner.base_url = Some(format!("http://{}", ready.address));
        inner.auth_token = Some(token);
        Ok((generation, events))
    }

    pub async fn stop(&self) -> Result<CursorRuntimeState, AppError> {
        let _lifecycle_guard = self.lifecycle.lock().await;
        {
            let mut inner = self.inner.lock().await;
            if inner.base_url.is_none() {
                inner.phase = "stopped".to_string();
                inner.state.phase = "stopped".to_string();
                inner.state.sidecar_running = false;
                inner.state.backend_running = false;
                inner.state.proxy_running = false;
                inner.state.cursor_settings_applied = false;
                inner.state.last_error.clear();
                return Ok(inner.state.clone());
            }
            inner.phase = "restoring".to_string();
            inner.state.phase = inner.phase.clone();
        }

        let stop_result = self.post_empty("/v1/stop").await;
        if let Err(error) = self.post_empty("/v1/shutdown").await {
            log::warn!("请求 Cursor sidecar 退出失败，将终止子进程: {error}");
        }
        self.force_stop().await;
        if let Err(error) = stop_result {
            return self.fail(error.to_string()).await;
        }
        let mut inner = self.inner.lock().await;
        inner.phase = "stopped".to_string();
        inner.state.phase = "stopped".to_string();
        inner.state.sidecar_running = false;
        inner.state.backend_running = false;
        inner.state.proxy_running = false;
        inner.state.cursor_settings_applied = false;
        inner.state.last_error.clear();
        inner.state_observed = true;
        Ok(inner.state.clone())
    }

    pub async fn state(&self) -> Result<CursorRuntimeState, AppError> {
        {
            let inner = self.inner.lock().await;
            if inner.phase == "running" && inner.base_url.is_some() {
                drop(inner);
                return self.refresh_state().await;
            }
            if inner.state_observed {
                return Ok(inner.state.clone());
            }
        }
        Err(AppError::Message("Cursor 运行时状态尚未探测".to_string()))
    }

    pub async fn observe_state(&self, app: &AppHandle) -> Result<CursorRuntimeState, AppError> {
        if let Ok(state) = self.state().await {
            return Ok(state);
        }
        let _lifecycle_guard = self.lifecycle.lock().await;
        if let Ok(state) = self.state().await {
            return Ok(state);
        }
        let (_generation, events) = self.launch_sidecar(app, "maintenance").await?;
        let response = self.fetch_state().await;
        if let Err(error) = self.post_empty("/v1/shutdown").await {
            log::warn!("Cursor 状态探测后请求 sidecar 退出失败: {error}");
        }
        self.force_stop().await;
        drop(events);

        let sidecar_state = response?;
        let state = sidecar_state.into_runtime_state("stopped", false, std::env::consts::OS);
        let mut inner = self.inner.lock().await;
        inner.phase = state.phase.clone();
        inner.state = state.clone();
        inner.state_observed = true;
        Ok(state)
    }

    pub async fn sync_config(&self) -> Result<(), AppError> {
        self.commit_config_change(projector::project_enabled_models, || Ok(()))
            .await
    }

    pub async fn commit_config_change<P, F>(
        &self,
        project_next: P,
        commit: F,
    ) -> Result<(), AppError>
    where
        P: FnOnce(&Database) -> Result<SidecarConfig, AppError>,
        F: FnOnce() -> Result<(), AppError>,
    {
        let _lifecycle_guard = self.lifecycle.lock().await;
        let running = self.inner.lock().await.base_url.is_some();
        let previous_config = if running {
            Some(projector::project_enabled_models(&self.db)?)
        } else {
            None
        };
        let next_config = project_next(&self.db)?;

        apply_config_change(previous_config, next_config, commit, |config| async move {
            self.put_config(&config).await
        })
        .await
    }

    pub async fn is_running(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.phase == "running" && inner.base_url.is_some()
    }

    pub fn try_begin_usage_sync(&self) -> bool {
        self.usage_sync_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn finish_usage_sync(&self) {
        self.usage_sync_active.store(false, Ordering::Release);
    }

    pub async fn install_ca(&self, app: &AppHandle) -> Result<CursorRuntimeState, AppError> {
        self.run_ca_action(app, "/v1/ca/install", false).await
    }

    pub async fn remove_ca(&self, app: &AppHandle) -> Result<CursorRuntimeState, AppError> {
        self.run_ca_action(app, "/v1/ca/remove", true).await
    }

    async fn run_ca_action(
        &self,
        app: &AppHandle,
        path: &str,
        require_stopped: bool,
    ) -> Result<CursorRuntimeState, AppError> {
        let _lifecycle_guard = self.lifecycle.lock().await;
        if require_stopped && self.is_running().await {
            return Err(AppError::Message(
                "请先停止 Cursor 运行时，再移除 CA".to_string(),
            ));
        }
        let temporary_process = if self.inner.lock().await.base_url.is_none() {
            Some(self.launch_sidecar(app, "maintenance").await?)
        } else {
            None
        };
        let response = match self
            .request(reqwest::Method::POST, path)
            .await?
            .send()
            .await
        {
            Ok(response) => decode_response::<SidecarRuntimeState>(response).await,
            Err(error) => Err(http_error(error)),
        };

        if let Some((_generation, events)) = temporary_process {
            if let Err(error) = self.post_empty("/v1/shutdown").await {
                log::warn!("Cursor CA 操作后请求 sidecar 退出失败: {error}");
            }
            self.force_stop().await;
            drop(events);
        }

        match response {
            Ok(sidecar_state) => {
                let sidecar_running = self.is_running().await;
                let phase = if sidecar_running {
                    "running"
                } else {
                    "stopped"
                };
                let state =
                    sidecar_state.into_runtime_state(phase, sidecar_running, std::env::consts::OS);
                let mut inner = self.inner.lock().await;
                inner.phase = state.phase.clone();
                inner.state = state.clone();
                Ok(state)
            }
            Err(error) => self.fail(error.to_string()).await,
        }
    }

    pub async fn test_model(
        &self,
        app: &AppHandle,
        adapter: &SidecarModelAdapter,
    ) -> Result<CursorModelTestResult, AppError> {
        let _lifecycle_guard = self.lifecycle.lock().await;
        let temporary_sidecar = self.inner.lock().await.base_url.is_none();
        let temporary_process = if temporary_sidecar {
            Some(self.launch_sidecar(app, "testing").await?)
        } else {
            None
        };

        let result = match self.request(reqwest::Method::POST, "/v1/test-model").await {
            Ok(request) => match request.json(adapter).send().await {
                Ok(response) => decode_response(response).await,
                Err(error) => Err(http_error(error)),
            },
            Err(error) => Err(error),
        };
        if let Some((generation, events)) = temporary_process {
            if let Err(error) = self.post_empty("/v1/shutdown").await {
                log::warn!("Cursor 模型测速后请求 sidecar 退出失败: {error}");
            }
            self.force_stop().await;
            drop(events);
            let mut inner = self.inner.lock().await;
            if inner.generation >= generation {
                inner.phase = "stopped".to_string();
                inner.state.phase = "stopped".to_string();
                inner.state.sidecar_running = false;
                inner.state.backend_running = false;
                inner.state.proxy_running = false;
                inner.state.cursor_settings_applied = false;
            }
        }
        result
    }

    async fn fetch_state(&self) -> Result<SidecarRuntimeState, AppError> {
        let response = self
            .request(reqwest::Method::GET, "/v1/state")
            .await?
            .send()
            .await
            .map_err(http_error)?;
        decode_response(response).await
    }

    async fn refresh_state(&self) -> Result<CursorRuntimeState, AppError> {
        let sidecar_state = self.fetch_state().await?;
        let state = sidecar_state.into_runtime_state("running", true, std::env::consts::OS);
        let mut inner = self.inner.lock().await;
        inner.phase = state.phase.clone();
        inner.state = state.clone();
        inner.state_observed = true;
        Ok(state)
    }

    async fn put_config(&self, config: &SidecarConfig) -> Result<(), AppError> {
        let response = self
            .request(reqwest::Method::PUT, "/v1/config")
            .await?
            .json(config)
            .send()
            .await
            .map_err(http_error)?;
        let _: SidecarConfig = decode_response(response).await?;
        Ok(())
    }

    async fn post_empty(&self, path: &str) -> Result<serde_json::Value, AppError> {
        let response = self
            .request(reqwest::Method::POST, path)
            .await?
            .send()
            .await
            .map_err(http_error)?;
        decode_response(response).await
    }

    pub async fn usage_events(
        &self,
        cursor: i64,
        limit: usize,
    ) -> Result<CursorUsageEventPage, AppError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/v1/usage-events?cursor={cursor}&limit={limit}"),
            )
            .await?
            .send()
            .await
            .map_err(http_error)?;
        decode_response(response).await
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, AppError> {
        let inner = self.inner.lock().await;
        let base_url = inner
            .base_url
            .clone()
            .ok_or_else(|| AppError::Message("Cursor sidecar 未运行".to_string()))?;
        let token = inner
            .auth_token
            .clone()
            .ok_or_else(|| AppError::Message("Cursor sidecar 鉴权状态不可用".to_string()))?;
        Ok(self
            .client
            .request(method, format!("{base_url}{path}"))
            .bearer_auth(token))
    }

    fn spawn_event_monitor(
        &self,
        app: AppHandle,
        generation: u64,
        mut events: tokio::sync::mpsc::Receiver<CommandEvent>,
    ) {
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => {
                        log::debug!(
                            "[cursor-sidecar] {}",
                            String::from_utf8_lossy(&bytes).trim()
                        );
                    }
                    CommandEvent::Stderr(bytes) => {
                        log::warn!(
                            "[cursor-sidecar] {}",
                            String::from_utf8_lossy(&bytes).trim()
                        );
                    }
                    CommandEvent::Terminated(payload) => {
                        let changed = service
                            .mark_terminated(
                                generation,
                                format!(
                                    "Cursor sidecar 已退出: code={:?} signal={:?}",
                                    payload.code, payload.signal
                                ),
                            )
                            .await;
                        if changed {
                            let _ = app
                                .emit("cursor-runtime-state-changed", service.state().await.ok());
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });
    }

    async fn mark_terminated(&self, generation: u64, message: String) -> bool {
        let mut inner = self.inner.lock().await;
        if inner.generation != generation || inner.base_url.is_none() {
            return false;
        }
        inner.child = None;
        inner.base_url = None;
        inner.auth_token = None;
        inner.phase = "error".to_string();
        inner.state.phase = "error".to_string();
        inner.state.sidecar_running = false;
        inner.state.backend_running = false;
        inner.state.proxy_running = false;
        inner.state.last_error = message;
        true
    }

    async fn restore_after_failed_start(&self) {
        if let Err(error) = self.post_empty("/v1/stop").await {
            log::warn!("Cursor 启动失败后恢复设置失败: {error}");
        }
        if let Err(error) = self.post_empty("/v1/shutdown").await {
            log::warn!("Cursor 启动失败后请求 sidecar 退出失败: {error}");
        }
        self.force_stop().await;
    }

    async fn force_stop(&self) {
        let mut inner = self.inner.lock().await;
        inner.generation = inner.generation.saturating_add(1);
        if let Some(child) = inner.child.take() {
            let _ = child.kill();
        }
        inner.base_url = None;
        inner.auth_token = None;
    }

    async fn fail<T>(&self, error: impl Into<String>) -> Result<T, AppError> {
        let message = error.into();
        let mut inner = self.inner.lock().await;
        inner.phase = "error".to_string();
        inner.state.phase = "error".to_string();
        inner.state.last_error = message.clone();
        Err(AppError::Message(message))
    }
}

async fn apply_config_change<C, W, WF>(
    previous_config: Option<SidecarConfig>,
    next_config: SidecarConfig,
    commit: C,
    mut write_sidecar: W,
) -> Result<(), AppError>
where
    C: FnOnce() -> Result<(), AppError>,
    W: FnMut(SidecarConfig) -> WF,
    WF: Future<Output = Result<(), AppError>>,
{
    if previous_config.is_some() {
        write_sidecar(next_config).await?;
    }

    if let Err(commit_error) = commit() {
        if let Some(previous_config) = previous_config {
            if let Err(rollback_error) = write_sidecar(previous_config).await {
                return Err(AppError::Message(format!(
                    "数据库更新失败: {commit_error}; Cursor sidecar 回滚失败: {rollback_error}"
                )));
            }
        }
        return Err(commit_error);
    }
    Ok(())
}

fn http_error(error: reqwest::Error) -> AppError {
    AppError::Message(format!("Cursor sidecar 请求失败: {error}"))
}

async fn decode_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, AppError> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AppError::Message(format!("读取 Cursor sidecar 响应失败: {error}")))?;
    if !status.is_success() {
        let message = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).to_string());
        return Err(AppError::Message(message));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::Message(format!("解析 Cursor sidecar 响应失败: {error}")))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::apply_config_change;
    use crate::cursor::types::{SidecarConfig, SidecarHomeMetricsConfig, SidecarRoutingConfig};
    use crate::error::AppError;

    fn config(mode: &str) -> SidecarConfig {
        SidecarConfig {
            log: false,
            provider_stream_idle_timeout: 240,
            backend_listen_addr: "127.0.0.1:18090".to_string(),
            proxy_listen_addr: "127.0.0.1:18080".to_string(),
            model_adapters: Vec::new(),
            routing: SidecarRoutingConfig {
                mode: mode.to_string(),
            },
            home_metrics: SidecarHomeMetricsConfig::default(),
            last_agent_model_hash: String::new(),
        }
    }

    #[tokio::test]
    async fn config_change_without_running_sidecar_only_commits_database() {
        let committed = Cell::new(false);
        let writes = Rc::new(RefCell::new(Vec::new()));
        let result = apply_config_change(
            None,
            config("next"),
            || {
                committed.set(true);
                Ok(())
            },
            {
                let writes = Rc::clone(&writes);
                move |config| {
                    writes.borrow_mut().push(config.routing.mode);
                    std::future::ready(Ok(()))
                }
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(committed.get());
        assert!(writes.borrow().is_empty());
    }

    #[tokio::test]
    async fn running_sidecar_applies_next_config_before_database_commit() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let result = apply_config_change(
            Some(config("previous")),
            config("next"),
            {
                let events = Rc::clone(&events);
                move || {
                    events.borrow_mut().push("commit".to_string());
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move |config| {
                    events
                        .borrow_mut()
                        .push(format!("sidecar:{}", config.routing.mode));
                    std::future::ready(Ok(()))
                }
            },
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(events.borrow().as_slice(), ["sidecar:next", "commit"]);
    }

    #[tokio::test]
    async fn sidecar_rejection_prevents_database_commit() {
        let committed = Cell::new(false);
        let result = apply_config_change(
            Some(config("previous")),
            config("next"),
            || {
                committed.set(true);
                Ok(())
            },
            |_| std::future::ready(Err(AppError::Message("sidecar rejected".to_string()))),
        )
        .await;

        assert_eq!(result.unwrap_err().to_string(), "sidecar rejected");
        assert!(!committed.get());
    }

    #[tokio::test]
    async fn database_failure_restores_previous_sidecar_config() {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let result = apply_config_change(
            Some(config("previous")),
            config("next"),
            || Err(AppError::Database("commit failed".to_string())),
            {
                let writes = Rc::clone(&writes);
                move |config| {
                    writes.borrow_mut().push(config.routing.mode);
                    std::future::ready(Ok(()))
                }
            },
        )
        .await;

        assert_eq!(result.unwrap_err().to_string(), "数据库错误: commit failed");
        assert_eq!(writes.borrow().as_slice(), ["next", "previous"]);
    }

    #[tokio::test]
    async fn rollback_failure_reports_both_database_and_sidecar_errors() {
        let write_count = Cell::new(0);
        let result = apply_config_change(
            Some(config("previous")),
            config("next"),
            || Err(AppError::Database("commit failed".to_string())),
            |_| {
                let current = write_count.get() + 1;
                write_count.set(current);
                std::future::ready(if current == 1 {
                    Ok(())
                } else {
                    Err(AppError::Message("rollback rejected".to_string()))
                })
            },
        )
        .await;

        assert_eq!(
            result.unwrap_err().to_string(),
            "数据库更新失败: 数据库错误: commit failed; Cursor sidecar 回滚失败: rollback rejected"
        );
        assert_eq!(write_count.get(), 2);
    }
}
