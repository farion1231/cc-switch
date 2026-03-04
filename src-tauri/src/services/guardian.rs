use crate::database::Database;
use crate::services::codex_usage::CodexUsageService;
use crate::services::legacy_startup_migration;
use crate::services::proxy::ProxyService;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GuardianCheckStatus {
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub repaired: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GuardianChecks {
    pub proxy_health: GuardianCheckStatus,
    pub auth_normalize: GuardianCheckStatus,
    pub breaker_recovery: GuardianCheckStatus,
    pub webkit_contamination: GuardianCheckStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct GuardianErrorStats {
    pub auth_401: u32,
    pub quota_429: u32,
    pub upstream_5xx: u32,
    pub transport_disconnect: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GuardianDisconnectSummary {
    pub app_type: String,
    pub provider_id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianMigrationPayload {
    pub status: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
}

impl Default for GuardianMigrationPayload {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            message: "guardian migration status unavailable".to_string(),
            backup_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianStatus {
    pub enabled: bool,
    pub interval_seconds: u64,
    pub worker_started: bool,
    pub run_in_progress: bool,
    pub run_count: u64,
    pub proxy_healthy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_self_heal_at: Option<String>,
    pub errors: GuardianErrorStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transport_disconnect: Option<GuardianDisconnectSummary>,
    pub migration: GuardianMigrationPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_source: Option<String>,
    pub checks: GuardianChecks,
}

impl Default for GuardianStatus {
    fn default() -> Self {
        Self {
            enabled: crate::settings::guardian_enabled(),
            interval_seconds: crate::settings::guardian_interval_seconds(),
            worker_started: false,
            run_in_progress: false,
            run_count: 0,
            proxy_healthy: false,
            last_self_heal_at: None,
            errors: GuardianErrorStats::default(),
            last_transport_disconnect: None,
            migration: GuardianMigrationPayload::default(),
            last_run_at: None,
            last_success_at: None,
            last_error: None,
            last_duration_ms: None,
            last_run_source: None,
            checks: GuardianChecks::default(),
        }
    }
}

#[derive(Clone)]
pub struct GuardianService {
    db: Arc<Database>,
    proxy_service: ProxyService,
    status: Arc<RwLock<GuardianStatus>>,
    started: Arc<AtomicBool>,
    run_lock: Arc<Mutex<()>>,
    notify_state: Arc<Mutex<GuardianNotifyState>>,
}

#[derive(Debug, Clone, Default)]
struct GuardianNotifyState {
    last_key: Option<String>,
    last_sent_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl GuardianService {
    pub fn new(db: Arc<Database>, proxy_service: ProxyService) -> Self {
        Self {
            db,
            proxy_service,
            status: Arc::new(RwLock::new(GuardianStatus::default())),
            started: Arc::new(AtomicBool::new(false)),
            run_lock: Arc::new(Mutex::new(())),
            notify_state: Arc::new(Mutex::new(GuardianNotifyState::default())),
        }
    }

    pub fn start_worker(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }

        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            {
                let mut status = this.status.write().await;
                status.worker_started = true;
            }

            loop {
                let enabled = crate::settings::guardian_enabled();
                let interval_seconds = crate::settings::guardian_interval_seconds();

                {
                    let mut status = this.status.write().await;
                    status.enabled = enabled;
                    status.interval_seconds = interval_seconds;
                }

                if enabled {
                    if let Err(err) = this.run_once_inner("scheduled", false).await {
                        log::warn!("[Guardian] scheduled run failed: {err}");
                    }
                    tokio::time::sleep(Duration::from_secs(interval_seconds)).await;
                } else {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        });
    }

    pub async fn get_status(&self) -> GuardianStatus {
        let mut status = self.status.read().await.clone();
        status.enabled = crate::settings::guardian_enabled();
        status.interval_seconds = crate::settings::guardian_interval_seconds();
        status.worker_started = self.started.load(Ordering::SeqCst);
        status.migration = self.current_migration_payload();
        status
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<GuardianStatus, String> {
        crate::settings::set_guardian_enabled(enabled).map_err(|e| e.to_string())?;
        if enabled {
            self.start_worker();
        }

        {
            let mut status = self.status.write().await;
            status.enabled = enabled;
            status.interval_seconds = crate::settings::guardian_interval_seconds();
            status.worker_started = self.started.load(Ordering::SeqCst);
            status.migration = self.current_migration_payload();
        }

        Ok(self.get_status().await)
    }

    pub async fn run_once(&self, source: &str) -> Result<GuardianStatus, String> {
        self.run_once_inner(source, true).await
    }

    fn current_migration_payload(&self) -> GuardianMigrationPayload {
        match legacy_startup_migration::get_guardian_migration_status() {
            Ok(v) => GuardianMigrationPayload {
                status: v.status,
                message: v.message,
                backup_id: v.backup_id,
            },
            Err(err) => GuardianMigrationPayload {
                status: "unknown".to_string(),
                message: err.to_string(),
                backup_id: None,
            },
        }
    }

    async fn run_once_inner(&self, source: &str, force: bool) -> Result<GuardianStatus, String> {
        if !force && !crate::settings::guardian_enabled() {
            return Ok(self.get_status().await);
        }

        let _guard = self.run_lock.lock().await;
        let started = Instant::now();
        let now = chrono::Utc::now().to_rfc3339();

        {
            let mut status = self.status.write().await;
            status.run_in_progress = true;
            status.last_run_source = Some(source.to_string());
            status.enabled = crate::settings::guardian_enabled();
            status.interval_seconds = crate::settings::guardian_interval_seconds();
            status.worker_started = self.started.load(Ordering::SeqCst);
            status.migration = self.current_migration_payload();
        }

        let proxy_health = self.check_proxy_health().await;
        let auth_normalize = self.check_auth_normalize().await;
        let breaker_recovery = self.check_breaker_recovery().await;
        let webkit_contamination = self.check_webkit_contamination().await;
        let (errors, last_transport_disconnect) = self.collect_error_stats().await;

        let all_ok =
            proxy_health.ok && auth_normalize.ok && breaker_recovery.ok && webkit_contamination.ok;

        let duration_ms = started.elapsed().as_millis() as u64;
        let (status_snapshot, notify_proxy_error, notify_body) = {
            let mut status = self.status.write().await;
            status.run_in_progress = false;
            status.run_count += 1;
            status.last_run_at = Some(now.clone());
            status.last_duration_ms = Some(duration_ms);
            status.checks = GuardianChecks {
                proxy_health,
                auth_normalize,
                breaker_recovery,
                webkit_contamination,
            };
            status.proxy_healthy = status.checks.proxy_health.ok;
            status.errors = errors;
            status.last_transport_disconnect = last_transport_disconnect;
            status.migration = self.current_migration_payload();

            if status.checks.proxy_health.repaired > 0 {
                status.last_self_heal_at = Some(now.clone());
            }

            if all_ok {
                status.last_success_at = Some(now);
                status.last_error = None;
            } else {
                let mut errs = Vec::new();
                if !status.checks.proxy_health.ok {
                    errs.push(format!(
                        "proxyHealth={}",
                        status.checks.proxy_health.message
                    ));
                }
                if !status.checks.auth_normalize.ok {
                    errs.push(format!(
                        "authNormalize={}",
                        status.checks.auth_normalize.message
                    ));
                }
                if !status.checks.breaker_recovery.ok {
                    errs.push(format!(
                        "breakerRecovery={}",
                        status.checks.breaker_recovery.message
                    ));
                }
                if !status.checks.webkit_contamination.ok {
                    errs.push(format!(
                        "webkitContamination={}",
                        status.checks.webkit_contamination.message
                    ));
                }
                status.last_error = Some(errs.join("; "));
            }

            let notify_proxy_error = !status.checks.proxy_health.ok;
            let notify_body = if notify_proxy_error {
                Some(format!(
                    "代理健康检查失败：{}",
                    status.checks.proxy_health.message
                ))
            } else {
                None
            };

            (status.clone(), notify_proxy_error, notify_body)
        };

        if all_ok {
            self.reset_notify_state().await;
        } else if notify_proxy_error {
            if let Some(body) = notify_body {
                let _ = self
                    .notify_guardian_issue("proxy_health_failed", "CC Switch Guardian", &body)
                    .await;
            }
        }

        Ok(status_snapshot)
    }

    async fn reset_notify_state(&self) {
        let mut state = self.notify_state.lock().await;
        state.last_key = None;
        state.last_sent_at = None;
    }

    async fn notify_guardian_issue(
        &self,
        key: &str,
        title: &str,
        body: &str,
    ) -> Result<bool, String> {
        let now = chrono::Utc::now();
        let min_interval = chrono::Duration::seconds(300);

        {
            let mut state = self.notify_state.lock().await;
            if state.last_key.as_deref() == Some(key) {
                if let Some(last) = state.last_sent_at {
                    if now.signed_duration_since(last) < min_interval {
                        return Ok(false);
                    }
                }
            }

            Self::send_system_notification(title, body)?;
            state.last_key = Some(key.to_string());
            state.last_sent_at = Some(now);
        }

        Ok(true)
    }

    #[cfg(target_os = "macos")]
    fn send_system_notification(title: &str, body: &str) -> Result<(), String> {
        fn esc(input: &str) -> String {
            input
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', " ")
        }

        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            esc(body),
            esc(title)
        );

        let output = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| format!("调用系统通知失败: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "系统通知返回异常状态: {}",
                output.status
            ))
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn send_system_notification(_title: &str, _body: &str) -> Result<(), String> {
        Ok(())
    }

    async fn expected_proxy_active(&self) -> bool {
        let takeover_active = self
            .proxy_service
            .is_takeover_active()
            .await
            .unwrap_or(false);
        let global_proxy_on = self
            .db
            .get_global_proxy_config()
            .await
            .map(|cfg| cfg.proxy_enabled)
            .unwrap_or(false);

        takeover_active || global_proxy_on
    }

    async fn probe_proxy_health(
        &self,
        address: &str,
        port: u16,
    ) -> (bool, Option<u16>, Option<String>) {
        let url = format!("http://{address}:{port}/health");
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(err) => return (false, None, Some(format!("创建 HTTP 客户端失败: {err}"))),
        };

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => (true, Some(resp.status().as_u16()), None),
            Ok(resp) => (
                false,
                Some(resp.status().as_u16()),
                Some(format!("代理健康接口返回异常状态: {}", resp.status())),
            ),
            Err(err) => (false, None, Some(format!("访问代理健康接口失败: {err}"))),
        }
    }

    async fn self_heal_proxy_once(
        &self,
        running_before: bool,
    ) -> Result<serde_json::Value, String> {
        if running_before {
            let _ = self.proxy_service.stop().await;
        }

        let started = self.proxy_service.start().await?;
        let (healthy, status_code, probe_err) = self
            .probe_proxy_health(&started.address, started.port)
            .await;

        Ok(json!({
            "selfHealAttempted": true,
            "runningBefore": running_before,
            "runningAfter": true,
            "restartAddress": started.address,
            "restartPort": started.port,
            "probeHealthy": healthy,
            "probeStatus": status_code,
            "probeError": probe_err,
        }))
    }

    async fn check_proxy_health(&self) -> GuardianCheckStatus {
        let now = chrono::Utc::now().to_rfc3339();
        let expected_active = self.expected_proxy_active().await;

        let status = match self.proxy_service.get_status().await {
            Ok(s) => s,
            Err(err) => {
                return GuardianCheckStatus {
                    ok: false,
                    message: format!("读取代理状态失败: {err}"),
                    checked_at: Some(now),
                    details: Some(json!({ "expectedActive": expected_active })),
                    repaired: 0,
                };
            }
        };

        if !status.running {
            if !expected_active {
                return GuardianCheckStatus {
                    ok: true,
                    message: "代理未运行且非预期激活，跳过健康探测".to_string(),
                    checked_at: Some(now),
                    repaired: 0,
                    details: Some(json!({
                        "running": false,
                        "expectedActive": false,
                    })),
                };
            }

            match self.self_heal_proxy_once(false).await {
                Ok(details) => {
                    let healed = details
                        .get("probeHealthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    GuardianCheckStatus {
                        ok: healed,
                        message: if healed {
                            "代理不可用，已执行自愈重启并恢复健康".to_string()
                        } else {
                            "代理不可用，已尝试自愈重启但健康检查仍失败".to_string()
                        },
                        checked_at: Some(now),
                        repaired: if healed { 1 } else { 0 },
                        details: Some(details),
                    }
                }
                Err(err) => GuardianCheckStatus {
                    ok: false,
                    message: format!("代理不可用且自愈重启失败: {err}"),
                    checked_at: Some(now),
                    repaired: 0,
                    details: Some(json!({
                        "running": false,
                        "expectedActive": true,
                        "selfHealAttempted": true,
                    })),
                },
            }
        } else {
            let (healthy, status_code, probe_err) =
                self.probe_proxy_health(&status.address, status.port).await;
            if healthy {
                return GuardianCheckStatus {
                    ok: true,
                    message: "代理健康检查通过".to_string(),
                    checked_at: Some(now),
                    repaired: 0,
                    details: Some(json!({
                        "running": true,
                        "expectedActive": expected_active,
                        "address": status.address,
                        "port": status.port,
                        "status": status_code,
                    })),
                };
            }

            if !expected_active {
                return GuardianCheckStatus {
                    ok: false,
                    message: probe_err.unwrap_or_else(|| "代理健康检查失败".to_string()),
                    checked_at: Some(now),
                    repaired: 0,
                    details: Some(json!({
                        "running": true,
                        "expectedActive": false,
                        "address": status.address,
                        "port": status.port,
                        "status": status_code,
                    })),
                };
            }

            match self.self_heal_proxy_once(true).await {
                Ok(mut details) => {
                    if let Some(obj) = details.as_object_mut() {
                        obj.insert("originalAddress".to_string(), json!(status.address));
                        obj.insert("originalPort".to_string(), json!(status.port));
                        obj.insert("originalProbeStatus".to_string(), json!(status_code));
                        obj.insert("originalProbeError".to_string(), json!(probe_err));
                    }

                    let healed = details
                        .get("probeHealthy")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    GuardianCheckStatus {
                        ok: healed,
                        message: if healed {
                            "代理不健康，已执行一次自愈重启并恢复健康".to_string()
                        } else {
                            "代理不健康，已执行一次自愈重启但仍异常".to_string()
                        },
                        checked_at: Some(now),
                        repaired: if healed { 1 } else { 0 },
                        details: Some(details),
                    }
                }
                Err(err) => GuardianCheckStatus {
                    ok: false,
                    message: format!("代理不健康且自愈重启失败: {err}"),
                    checked_at: Some(now),
                    repaired: 0,
                    details: Some(json!({
                        "running": true,
                        "expectedActive": true,
                        "address": status.address,
                        "port": status.port,
                        "selfHealAttempted": true,
                        "originalProbeStatus": status_code,
                        "originalProbeError": probe_err,
                    })),
                },
            }
        }
    }

    fn normalize_codex_auth_value(value: &mut serde_json::Value) -> bool {
        let has_openai_key = value
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some();

        if has_openai_key {
            return false;
        }

        let Some(access_token) = value
            .get("tokens")
            .and_then(|v| v.get("access_token"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
        else {
            return false;
        };

        if !value.is_object() {
            *value = json!({});
        }

        if let Some(obj) = value.as_object_mut() {
            obj.insert("OPENAI_API_KEY".to_string(), json!(access_token));
            return true;
        }

        false
    }

    fn normalize_codex_auth_file(&self) -> Result<bool, String> {
        let path = crate::codex_config::get_codex_auth_path();
        if !path.exists() {
            return Ok(false);
        }

        let mut auth: serde_json::Value =
            crate::config::read_json_file(&path).map_err(|e| e.to_string())?;

        let changed = Self::normalize_codex_auth_value(&mut auth);
        if changed {
            crate::config::write_json_file(&path, &auth).map_err(|e| e.to_string())?;
        }

        Ok(changed)
    }

    fn normalize_codex_provider_auth(&self) -> Result<u32, String> {
        let providers = self
            .db
            .get_all_providers("codex")
            .map_err(|e| format!("读取 Codex providers 失败: {e}"))?;

        let mut changed = 0u32;
        for (provider_id, mut provider) in providers {
            let Some(auth_value) = provider.settings_config.get_mut("auth") else {
                continue;
            };

            if Self::normalize_codex_auth_value(auth_value) {
                self.db
                    .update_provider_settings_config(
                        "codex",
                        &provider_id,
                        &provider.settings_config,
                    )
                    .map_err(|e| format!("更新 Codex provider 失败 ({provider_id}): {e}"))?;
                changed += 1;
            }
        }

        Ok(changed)
    }

    async fn check_auth_normalize(&self) -> GuardianCheckStatus {
        let now = chrono::Utc::now().to_rfc3339();

        let import_result = CodexUsageService::import_from_switcher_once(&self.db).ok();
        let file_normalized = match self.normalize_codex_auth_file() {
            Ok(changed) => changed,
            Err(err) => {
                return GuardianCheckStatus {
                    ok: false,
                    message: format!("auth 文件归一化失败: {err}"),
                    repaired: 0,
                    checked_at: Some(now),
                    details: None,
                };
            }
        };

        let provider_updates = match self.normalize_codex_provider_auth() {
            Ok(count) => count,
            Err(err) => {
                return GuardianCheckStatus {
                    ok: false,
                    message: format!("provider auth 归一化失败: {err}"),
                    repaired: 0,
                    checked_at: Some(now),
                    details: None,
                };
            }
        };

        let imported = import_result.as_ref().map(|v| v.imported).unwrap_or(0);
        let repaired = provider_updates + if file_normalized { 1 } else { 0 };

        GuardianCheckStatus {
            ok: true,
            message: if repaired > 0 {
                format!("auth 归一化完成（修复 {repaired} 项）")
            } else {
                "auth 已归一化，无需修复".to_string()
            },
            repaired,
            checked_at: Some(now),
            details: Some(json!({
                "normalizedAuthFile": file_normalized,
                "normalizedProviders": provider_updates,
                "importedAccounts": imported,
            })),
        }
    }

    fn should_recover_breaker(last_failure_at: Option<&str>, timeout_seconds: u32) -> bool {
        let Some(last_failure_at) = last_failure_at else {
            return true;
        };

        let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(last_failure_at) else {
            return true;
        };

        let elapsed =
            chrono::Utc::now().signed_duration_since(last_time.with_timezone(&chrono::Utc));
        elapsed.num_seconds() >= timeout_seconds as i64
    }

    async fn check_breaker_recovery(&self) -> GuardianCheckStatus {
        let now = chrono::Utc::now().to_rfc3339();
        let mut recovered = 0u32;
        let mut blocked = 0u32;

        for app in ["claude", "codex", "gemini"] {
            let app_cfg = match self.db.get_proxy_config_for_app(app).await {
                Ok(cfg) => cfg,
                Err(err) => {
                    return GuardianCheckStatus {
                        ok: false,
                        message: format!("读取代理配置失败 ({app}): {err}"),
                        repaired: recovered,
                        checked_at: Some(now),
                        details: None,
                    };
                }
            };

            let providers = match self.db.get_all_providers(app) {
                Ok(p) => p,
                Err(err) => {
                    return GuardianCheckStatus {
                        ok: false,
                        message: format!("读取 provider 列表失败 ({app}): {err}"),
                        repaired: recovered,
                        checked_at: Some(now),
                        details: None,
                    };
                }
            };

            for provider_id in providers.keys() {
                let health = match self.db.get_provider_health(provider_id, app).await {
                    Ok(h) => h,
                    Err(err) => {
                        return GuardianCheckStatus {
                            ok: false,
                            message: format!("读取健康状态失败 ({app}/{provider_id}): {err}"),
                            repaired: recovered,
                            checked_at: Some(now),
                            details: None,
                        };
                    }
                };

                if health.is_healthy {
                    continue;
                }

                if Self::should_recover_breaker(
                    health.last_failure_at.as_deref(),
                    app_cfg.circuit_timeout_seconds,
                ) {
                    if let Err(err) = self
                        .db
                        .update_provider_health(provider_id, app, true, None)
                        .await
                    {
                        return GuardianCheckStatus {
                            ok: false,
                            message: format!("恢复健康状态失败 ({app}/{provider_id}): {err}"),
                            repaired: recovered,
                            checked_at: Some(now),
                            details: None,
                        };
                    }

                    if let Err(err) = self
                        .proxy_service
                        .reset_provider_circuit_breaker(provider_id, app)
                        .await
                    {
                        return GuardianCheckStatus {
                            ok: false,
                            message: format!("重置熔断器失败 ({app}/{provider_id}): {err}"),
                            repaired: recovered,
                            checked_at: Some(now),
                            details: None,
                        };
                    }

                    recovered += 1;
                } else {
                    blocked += 1;
                }
            }
        }

        GuardianCheckStatus {
            ok: true,
            message: format!("熔断恢复检查完成（恢复 {recovered} 项，等待冷却 {blocked} 项）"),
            repaired: recovered,
            checked_at: Some(now),
            details: Some(json!({
                "recovered": recovered,
                "cooldownPending": blocked,
            })),
        }
    }

    fn classify_error_message(msg: &str, stats: &mut GuardianErrorStats) {
        let m = msg.to_ascii_lowercase();

        if Self::is_transport_disconnect_message(&m) {
            stats.transport_disconnect += 1;
            return;
        }

        if m.contains("401") || m.contains("unauthorized") || m.contains("invalid api key") {
            stats.auth_401 += 1;
        }
        if m.contains("429")
            || m.contains("quota")
            || m.contains("rate limit")
            || m.contains("insufficient_quota")
        {
            stats.quota_429 += 1;
        }

        let has_5xx = m.contains("5xx")
            || m.contains(" 500")
            || m.contains(" 502")
            || m.contains(" 503")
            || m.contains(" 504")
            || m.contains("status:500")
            || m.contains("status:502")
            || m.contains("status:503")
            || m.contains("status:504")
            || m.contains("upstream");
        if has_5xx {
            stats.upstream_5xx += 1;
        }
    }

    fn is_transport_disconnect_message(msg_lowercase: &str) -> bool {
        [
            "transport disconnected",
            "transport disconnect",
            "connection reset",
            "connection closed",
            "connection aborted",
            "broken pipe",
            "socket hang up",
            "peer closed",
            "stream closed",
            "unexpected eof",
            "tls eof",
            "connection lost",
            "channel closed",
            "network is unreachable",
        ]
        .iter()
        .any(|token| msg_lowercase.contains(token))
    }

    async fn collect_error_stats(&self) -> (GuardianErrorStats, Option<GuardianDisconnectSummary>) {
        let mut stats = GuardianErrorStats::default();
        let mut latest_disconnect: Option<(chrono::DateTime<chrono::Utc>, GuardianDisconnectSummary)> =
            None;

        for app in ["claude", "codex", "gemini"] {
            let providers = match self.db.get_all_providers(app) {
                Ok(v) => v,
                Err(err) => {
                    log::debug!("[Guardian] skip error stats for {app}: {err}");
                    continue;
                }
            };

            for provider_id in providers.keys() {
                match self.db.get_provider_health(provider_id, app).await {
                    Ok(health) => {
                        if let Some(msg) = health.last_error {
                            Self::classify_error_message(&msg, &mut stats);
                            let message_lower = msg.to_ascii_lowercase();
                            if Self::is_transport_disconnect_message(&message_lower) {
                                let occurred_at = health
                                    .last_failure_at
                                    .clone()
                                    .filter(|v| !v.trim().is_empty());
                                let ts = occurred_at
                                    .as_deref()
                                    .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                                    .map(|v| v.with_timezone(&chrono::Utc))
                                    .unwrap_or_else(chrono::Utc::now);
                                let summary = GuardianDisconnectSummary {
                                    app_type: app.to_string(),
                                    provider_id: provider_id.to_string(),
                                    message: msg,
                                    occurred_at,
                                };
                                if latest_disconnect
                                    .as_ref()
                                    .map(|(prev, _)| ts > *prev)
                                    .unwrap_or(true)
                                {
                                    latest_disconnect = Some((ts, summary));
                                }
                            }
                        }
                    }
                    Err(err) => {
                        log::debug!("[Guardian] skip provider health {app}/{provider_id}: {err}");
                    }
                }
            }
        }

        (stats, latest_disconnect.map(|(_, summary)| summary))
    }

    #[cfg(target_os = "macos")]
    fn launchctl_env(key: &str) -> Option<String> {
        let output = std::process::Command::new("launchctl")
            .args(["getenv", key])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn launchctl_env(_key: &str) -> Option<String> {
        None
    }

    async fn check_webkit_contamination(&self) -> GuardianCheckStatus {
        let now = chrono::Utc::now().to_rfc3339();
        let keys = [
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_FORCE_COMPOSITING_MODE",
            "WEBKIT_DISABLE_COMPOSITING_MODE",
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
        ];

        let mut contaminated = Vec::<(String, String, String)>::new();
        for key in keys {
            if let Ok(value) = std::env::var(key) {
                let is_expected_linux_default = cfg!(target_os = "linux")
                    && key == "WEBKIT_DISABLE_DMABUF_RENDERER"
                    && value.trim() == "1";
                if !is_expected_linux_default {
                    contaminated.push(("process".to_string(), key.to_string(), value));
                }
            }

            if let Some(value) = Self::launchctl_env(key) {
                contaminated.push(("launchctl".to_string(), key.to_string(), value));
            }
        }

        if contaminated.is_empty() {
            GuardianCheckStatus {
                ok: true,
                message: "未检测到 WebKit 污染环境变量".to_string(),
                repaired: 0,
                checked_at: Some(now),
                details: Some(json!({ "contaminated": false })),
            }
        } else {
            GuardianCheckStatus {
                ok: false,
                message: format!("检测到 {} 个 WebKit 污染项", contaminated.len()),
                repaired: 0,
                checked_at: Some(now),
                details: Some(json!({
                    "contaminated": true,
                    "items": contaminated
                        .iter()
                        .map(|(scope, key, value)| json!({ "scope": scope, "key": key, "value": value }))
                        .collect::<Vec<_>>(),
                })),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_recover_when_timeout_elapsed() {
        let ts = (chrono::Utc::now() - chrono::Duration::seconds(120)).to_rfc3339();
        assert!(GuardianService::should_recover_breaker(
            Some(ts.as_str()),
            60
        ));
        assert!(!GuardianService::should_recover_breaker(
            Some(ts.as_str()),
            180
        ));
    }

    #[test]
    fn normalize_codex_auth_backfills_openai_key() {
        let mut value = json!({
            "tokens": {
                "access_token": "eyJ.test.token"
            }
        });
        assert!(GuardianService::normalize_codex_auth_value(&mut value));
        assert_eq!(
            value.get("OPENAI_API_KEY").and_then(|v| v.as_str()),
            Some("eyJ.test.token")
        );
    }

    #[test]
    fn classify_error_message_counts_known_buckets() {
        let mut stats = GuardianErrorStats::default();
        GuardianService::classify_error_message("HTTP 401 Unauthorized", &mut stats);
        GuardianService::classify_error_message("rate limit 429", &mut stats);
        GuardianService::classify_error_message("upstream status:503", &mut stats);

        assert_eq!(stats.auth_401, 1);
        assert_eq!(stats.quota_429, 1);
        assert_eq!(stats.upstream_5xx, 1);
        assert_eq!(stats.transport_disconnect, 0);
    }

    #[test]
    fn classify_error_message_transport_disconnect_is_exclusive_bucket() {
        let mut stats = GuardianErrorStats::default();
        GuardianService::classify_error_message(
            "transport disconnect after upstream status:503",
            &mut stats,
        );

        assert_eq!(stats.transport_disconnect, 1);
        assert_eq!(stats.auth_401, 0);
        assert_eq!(stats.quota_429, 0);
        assert_eq!(stats.upstream_5xx, 0);
    }
}
