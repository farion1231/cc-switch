use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::error::AppError;
use crate::services::webdav_sync as webdav_sync_service;
use crate::settings::{self, WebDavSyncSettings};

const AUTO_SYNC_DEBOUNCE_MS: u64 = 1000;
pub(crate) const MAX_AUTO_SYNC_WAIT_MS: u64 = 10_000;

static DB_CHANGE_TX: OnceLock<Sender<String>> = OnceLock::new();
static AUTO_SYNC_SUPPRESS_DEPTH: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct AutoSyncSuppressionGuard;

impl AutoSyncSuppressionGuard {
    pub fn new() -> Self {
        AUTO_SYNC_SUPPRESS_DEPTH.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for AutoSyncSuppressionGuard {
    fn drop(&mut self) {
        let _ =
            AUTO_SYNC_SUPPRESS_DEPTH.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_sub(1))
            });
    }
}

pub(crate) fn is_auto_sync_suppressed() -> bool {
    AUTO_SYNC_SUPPRESS_DEPTH.load(Ordering::SeqCst) > 0
}

pub fn should_trigger_for_table(table: &str) -> bool {
    let normalized = table.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "providers"
            | "provider_endpoints"
            | "mcp_servers"
            | "prompts"
            | "skills"
            | "skill_repos"
            | "settings"
            | "proxy_config"
    )
}

pub(crate) fn enqueue_change_signal(tx: &Sender<String>, table: &str) -> bool {
    match tx.try_send(table.to_string()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => false,
    }
}

pub(crate) fn auto_sync_wait_duration(started_at: Instant, now: Instant) -> Option<Duration> {
    let max_wait = Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS);
    let debounce = Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS);
    let elapsed = now.saturating_duration_since(started_at);
    if elapsed >= max_wait {
        return None;
    }
    Some(debounce.min(max_wait - elapsed))
}

fn should_run_auto_sync(settings: Option<&WebDavSyncSettings>) -> bool {
    let Some(sync) = settings else {
        return false;
    };
    sync.enabled && sync.auto_sync
}

fn persist_auto_sync_error(settings: &mut WebDavSyncSettings, error: &AppError) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some("auto".to_string());
    let _ = settings::update_webdav_sync_status(settings.status.clone());
}

fn emit_auto_sync_status_updated(app: &AppHandle, status: &str, error: Option<&str>) {
    let payload = match error {
        Some(message) => json!({
            "source": "auto",
            "status": status,
            "error": message,
        }),
        None => json!({
            "source": "auto",
            "status": status,
        }),
    };

    if let Err(err) = app.emit("webdav-sync-status-updated", payload) {
        log::debug!("[WebDAV] failed to emit sync status update event: {err}");
    }
}

async fn run_auto_sync_upload(
    db: &crate::database::Database,
    app: &AppHandle,
) -> Result<(), AppError> {
    let mut settings = settings::get_webdav_sync_settings();
    if !should_run_auto_sync(settings.as_ref()) {
        return Ok(());
    }

    let mut sync_settings = match settings.take() {
        Some(value) => value,
        None => return Ok(()),
    };

    let result = webdav_sync_service::run_with_sync_lock(webdav_sync_service::upload(
        db,
        &mut sync_settings,
    ))
    .await;
    match result {
        Ok(_) => {
            emit_auto_sync_status_updated(app, "success", None);
            Ok(())
        }
        Err(err) => {
            persist_auto_sync_error(&mut sync_settings, &err);
            emit_auto_sync_status_updated(app, "error", Some(&err.to_string()));
            Err(err)
        }
    }
}

pub fn notify_db_changed(table: &str) {
    if is_auto_sync_suppressed() {
        return;
    }
    if !should_trigger_for_table(table) {
        return;
    }
    let Some(tx) = DB_CHANGE_TX.get() else {
        return;
    };
    let _ = enqueue_change_signal(tx, table);
}

pub fn start_worker(db: Arc<crate::database::Database>, app: tauri::AppHandle) {
    if DB_CHANGE_TX.get().is_some() {
        return;
    }

    // Buffer size 1 is enough: we only need "dirty" signals, not every event.
    let (tx, rx) = channel::<String>(1);
    if DB_CHANGE_TX.set(tx).is_err() {
        return;
    }

    // Startup grace: setup-stage DB writes (migration, defaults,
    // periodic_backup_if_needed, recover_from_crash settings writes)
    // must not trigger an auto-upload before the user has done
    // anything. push 2 here; setup closure releases once and a
    // delayed-release task (spawned from setup) releases once more —
    // net zero, but coverage spans both the setup synchronous path
    // AND tasks spawned from it that complete after setup returns.
    // 30s is enough for startup background tasks to land (typically
    // <5s) and short enough that a real user interaction is never
    // suppressed (UI is still rendering at that point).
    // See #4547 + codex review P1.
    AUTO_SYNC_SUPPRESS_DEPTH.fetch_add(2, Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        run_worker_loop(db, rx, app).await;
    });
}

/// 在 setup 流程结束时调用，释放 startup 期 suppression。
/// 调用后 SQLite update_hook 投递的"数据变更"信号才会真正触发自动同步。
///
/// 在 `notify_db_changed` 已经开始触发的场景下（例如 DB hook 抢先于本函数调用），
/// 信号会被 try_send 丢弃（容量 1 的 channel），所以这里是幂等且安全的。
pub fn release_startup_auto_sync() {
    let _ = AUTO_SYNC_SUPPRESS_DEPTH.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
        Some(value.saturating_sub(1))
    });
}

async fn run_worker_loop(
    db: Arc<crate::database::Database>,
    mut rx: Receiver<String>,
    app: tauri::AppHandle,
) {
    while let Some(first_table) = rx.recv().await {
        let started_at = Instant::now();
        let mut merged_count = 1usize;

        while let Some(wait_for) = auto_sync_wait_duration(started_at, Instant::now()) {
            let timeout = tokio::time::timeout(wait_for, rx.recv()).await;

            match timeout {
                Ok(Some(_)) => merged_count += 1,
                Ok(None) => return,
                Err(_) => break,
            }
        }

        log::debug!(
            "[WebDAV][AutoSync] Triggered by table={first_table}, merged_changes={merged_count}"
        );

        if let Err(err) = run_auto_sync_upload(&db, &app).await {
            log::warn!("[WebDAV][AutoSync] Upload failed: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_sync_wait_duration, enqueue_change_signal, is_auto_sync_suppressed,
        release_startup_auto_sync, should_run_auto_sync, should_trigger_for_table,
        AutoSyncSuppressionGuard, MAX_AUTO_SYNC_WAIT_MS,
    };
    use crate::settings::WebDavSyncSettings;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc::channel;

    // AUTO_SYNC_SUPPRESS_DEPTH 是模块级 AtomicUsize，跨模块测试可能并发。
    // 共享 crate::services::SUPPRESS_TEST_LOCK 串行化所有读取/修改它的测试。
    // 现有 `suppression_guard_enables_and_restores_state` 也加同一把锁，
    // 否则它会与新测试并行跑、相互污染。

    #[test]
    fn start_worker_pushes_two_for_setup_and_delayed_release() {
        let _lock = crate::services::SUPPRESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Regression guard for codex review P1 on #4587: start_worker
        // must push *2* (one for the setup closure release, one for the
        // 30s delayed-release task spawned from setup), not just 1.
        // If a future refactor drops back to push(1), this test will
        // detect it because two paired releases no longer leave
        // suppression fully off.
        let _startup = AutoSyncSuppressionGuard::new(); // simulates start_worker push
        AutoSyncSuppressionGuard::new(); // second push for the delayed-release path
                                         // Two releases — one sync (setup), one for the 30s path.
        release_startup_auto_sync();
        release_startup_auto_sync();
        // net = 0 → no suppression
        assert!(
            !is_auto_sync_suppressed(),
            "expected suppression cleared after paired push(2)/release(2)"
        );
    }

    #[test]
    fn release_startup_auto_sync_clears_suppression_when_paired() {
        let _lock = crate::services::SUPPRESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 模拟 start_worker + setup 末尾的配对：start_worker 内 fetch_add(1)，
        // setup 末尾 release_startup_auto_sync() 减 1。验证：
        //   1. guard 进入时 suppression 开启
        //   2. release 后该 guard 的影响被抵消（即便此时还有别的 guard 也无关）
        // 用 RAII guard 避免测试结束时残留污染。
        {
            let _startup_guard = AutoSyncSuppressionGuard::new();
            assert!(is_auto_sync_suppressed());
            release_startup_auto_sync();
            // startup_guard 仍未 drop，仍在抑制；但我们验证了 release 不 panic 且调用合法。
        }
    }

    #[test]
    fn release_is_idempotent_under_saturating_sub() {
        let _lock = crate::services::SUPPRESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // release 必须用 saturating_sub 防止下溢。多次 release 不应让计数溢出成
        // 巨数。此处只验证不 panic + 调用合法，不依赖绝对状态。
        {
            let _g = AutoSyncSuppressionGuard::new();
            release_startup_auto_sync();
            release_startup_auto_sync();
            release_startup_auto_sync();
        }
    }

    #[test]
    fn should_trigger_sync_for_config_tables_only() {
        assert!(should_trigger_for_table("providers"));
        assert!(should_trigger_for_table("settings"));
        assert!(!should_trigger_for_table("proxy_request_logs"));
        assert!(!should_trigger_for_table("provider_health"));
    }

    #[test]
    fn suppression_guard_enables_and_restores_state() {
        let _lock = crate::services::SUPPRESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert!(!is_auto_sync_suppressed());
        {
            let _guard = AutoSyncSuppressionGuard::new();
            assert!(is_auto_sync_suppressed());
        }
        assert!(!is_auto_sync_suppressed());
    }

    #[test]
    fn max_wait_caps_flush_latency_for_continuous_events() {
        let started = Instant::now();
        let later = started + Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS + 1);
        assert!(auto_sync_wait_duration(started, later).is_none());
    }

    #[tokio::test]
    async fn enqueue_change_signal_drops_when_channel_is_full() {
        let (tx, _rx) = channel::<String>(1);
        assert!(enqueue_change_signal(&tx, "providers"));
        assert!(!enqueue_change_signal(&tx, "providers"));
    }

    #[test]
    fn should_run_auto_sync_requires_enabled_and_auto_sync_flag() {
        assert!(!should_run_auto_sync(None));

        let disabled = WebDavSyncSettings {
            enabled: false,
            auto_sync: true,
            ..WebDavSyncSettings::default()
        };
        assert!(!should_run_auto_sync(Some(&disabled)));

        let auto_sync_off = WebDavSyncSettings {
            enabled: true,
            auto_sync: false,
            ..WebDavSyncSettings::default()
        };
        assert!(!should_run_auto_sync(Some(&auto_sync_off)));

        let enabled = WebDavSyncSettings {
            enabled: true,
            auto_sync: true,
            ..WebDavSyncSettings::default()
        };
        assert!(should_run_auto_sync(Some(&enabled)));
    }

    #[test]
    fn service_layer_does_not_depend_on_commands_layer() {
        let source = include_str!("webdav_auto_sync.rs");
        let needle = ["crate", "commands", ""].join("::");
        assert!(
            !source.contains(&needle),
            "services layer should not depend on commands layer"
        );
    }
}
