//! Background auto sync shared by the WebDAV and S3 backends: local DB
//! changes are debounced, then the snapshot is uploaded under the global
//! sync lock. Each backend supplies its settings and upload call through
//! [`AutoSyncBackend`].

use std::future::Future;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::database::Database;
use crate::error::AppError;
use crate::services::sync_protocol::{run_with_sync_lock, should_trigger_auto_sync_for_table};
use crate::settings::WebDavSyncStatus;

const AUTO_SYNC_DEBOUNCE_MS: u64 = 1000;
const MAX_AUTO_SYNC_WAIT_MS: u64 = 10_000;

pub(crate) trait AutoSyncBackend: Send + Sync + 'static {
    type Settings: Send + 'static;
    /// Log prefix, e.g. `WebDAV`.
    const NAME: &'static str;
    /// Event the frontend listens to for auto sync results.
    const STATUS_EVENT: &'static str;

    fn state() -> &'static AutoSyncState;
    fn load() -> Option<Self::Settings>;
    fn wants_auto_sync(settings: &Self::Settings) -> bool;
    fn status_mut(settings: &mut Self::Settings) -> &mut WebDavSyncStatus;
    fn store_status(status: WebDavSyncStatus) -> Result<(), AppError>;
    fn upload<'a>(
        db: &'a Database,
        settings: &'a mut Self::Settings,
    ) -> impl Future<Output = Result<Value, AppError>> + Send + 'a;
}

/// Per-backend worker state.
pub(crate) struct AutoSyncState {
    tx: OnceLock<Sender<String>>,
    suppress_depth: AtomicUsize,
}

impl AutoSyncState {
    pub(crate) const fn new() -> Self {
        Self {
            tx: OnceLock::new(),
            suppress_depth: AtomicUsize::new(0),
        }
    }
}

/// While alive, DB changes don't queue an auto upload for backend `B`.
pub(crate) struct SuppressionGuard<B: AutoSyncBackend>(PhantomData<B>);

impl<B: AutoSyncBackend> SuppressionGuard<B> {
    pub fn new() -> Self {
        B::state().suppress_depth.fetch_add(1, Ordering::SeqCst);
        Self(PhantomData)
    }
}

impl<B: AutoSyncBackend> Drop for SuppressionGuard<B> {
    fn drop(&mut self) {
        let _ =
            B::state()
                .suppress_depth
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    Some(value.saturating_sub(1))
                });
    }
}

pub(crate) fn is_suppressed<B: AutoSyncBackend>() -> bool {
    B::state().suppress_depth.load(Ordering::SeqCst) > 0
}

fn enqueue_change_signal(tx: &Sender<String>, table: &str) -> bool {
    match tx.try_send(table.to_string()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => false,
    }
}

fn auto_sync_wait_duration(started_at: Instant, now: Instant) -> Option<Duration> {
    let max_wait = Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS);
    let debounce = Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS);
    let elapsed = now.saturating_duration_since(started_at);
    if elapsed >= max_wait {
        return None;
    }
    Some(debounce.min(max_wait - elapsed))
}

fn load_if_auto_sync<B: AutoSyncBackend>() -> Option<B::Settings> {
    B::load().filter(B::wants_auto_sync)
}

fn persist_auto_sync_error<B: AutoSyncBackend>(settings: &mut B::Settings, error: &AppError) {
    let status = B::status_mut(settings);
    status.last_error = Some(error.to_string());
    status.last_error_source = Some("auto".to_string());
    let _ = B::store_status(status.clone());
}

fn emit_auto_sync_status_updated<B: AutoSyncBackend, R: Runtime>(
    app: &AppHandle<R>,
    status: &str,
    error: Option<&str>,
) {
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

    if let Err(err) = app.emit(B::STATUS_EVENT, payload) {
        log::debug!(
            "[{}] failed to emit sync status update event: {err}",
            B::NAME
        );
    }
}

async fn run_auto_sync_upload<B: AutoSyncBackend, R: Runtime>(
    db: &Database,
    app: &AppHandle<R>,
) -> Result<(), AppError> {
    if load_if_auto_sync::<B>().is_none() {
        return Ok(());
    }
    // Re-read the settings under the sync lock so a "clear config" or "turn
    // off auto sync" that got the lock first is honoured instead of uploading
    // with the stale credentials captured before waiting.
    let outcome = run_with_sync_lock(async {
        let Some(mut sync_settings) = load_if_auto_sync::<B>() else {
            return Ok(None);
        };
        let result = B::upload(db, &mut sync_settings).await;
        Ok(Some((sync_settings, result)))
    })
    .await?;
    let Some((mut sync_settings, result)) = outcome else {
        return Ok(());
    };
    match result {
        Ok(_) => {
            emit_auto_sync_status_updated::<B, R>(app, "success", None);
            Ok(())
        }
        Err(err) => {
            persist_auto_sync_error::<B>(&mut sync_settings, &err);
            emit_auto_sync_status_updated::<B, R>(app, "error", Some(&err.to_string()));
            Err(err)
        }
    }
}

pub fn notify_db_changed<B: AutoSyncBackend>(table: &str) {
    if is_suppressed::<B>() {
        return;
    }
    if !should_trigger_auto_sync_for_table(table) {
        return;
    }
    let Some(tx) = B::state().tx.get() else {
        return;
    };
    let _ = enqueue_change_signal(tx, table);
}

pub fn start_worker<B: AutoSyncBackend>(db: Arc<Database>, app: AppHandle) {
    if B::state().tx.get().is_some() {
        return;
    }

    // Buffer size 1 is enough: we only need "dirty" signals, not every event.
    let (tx, rx) = channel::<String>(1);
    if B::state().tx.set(tx).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        run_worker_loop::<B, _>(db, rx, app).await;
    });
}

async fn run_worker_loop<B: AutoSyncBackend, R: Runtime>(
    db: Arc<Database>,
    mut rx: Receiver<String>,
    app: AppHandle<R>,
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
            "[{}][AutoSync] Triggered by table={first_table}, merged_changes={merged_count}",
            B::NAME
        );

        if let Err(err) = run_auto_sync_upload::<B, R>(&db, &app).await {
            log::warn!("[{}][AutoSync] Upload failed: {err}", B::NAME);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_sync_wait_duration, enqueue_change_signal, is_suppressed, run_worker_loop,
        AutoSyncBackend, SuppressionGuard, AUTO_SYNC_DEBOUNCE_MS, MAX_AUTO_SYNC_WAIT_MS,
    };
    use crate::database::Database;
    use crate::services::s3_auto_sync::S3AutoSync;
    use crate::services::sync_protocol::sync_mutex;
    use crate::services::webdav_auto_sync::WebDavAutoSync;
    use crate::settings::{self, AppSettings, S3SyncSettings, WebDavSyncSettings};
    use serial_test::serial;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc::channel;

    #[test]
    #[serial]
    fn suppression_guard_enables_and_restores_state() {
        assert!(!is_suppressed::<WebDavAutoSync>());
        {
            let _guard = SuppressionGuard::<WebDavAutoSync>::new();
            assert!(is_suppressed::<WebDavAutoSync>());
        }
        assert!(!is_suppressed::<WebDavAutoSync>());
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
    fn service_layer_does_not_depend_on_commands_layer() {
        let needle = ["crate", "commands", ""].join("::");
        for source in [
            include_str!("auto_sync.rs"),
            include_str!("webdav_auto_sync.rs"),
            include_str!("s3_auto_sync.rs"),
        ] {
            assert!(
                !source.contains(&needle),
                "services layer should not depend on commands layer"
            );
        }
    }

    /// A remote that answers 200 to every request and counts them.
    fn counting_remote() -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_in_thread = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                hits_in_thread.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut body_len = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        body_len = value.trim().parse().unwrap_or(0);
                    }
                }
                let _ = reader.read_exact(&mut vec![0; body_len]);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        (url, hits)
    }

    fn webdav_settings(remote: &str, enabled: bool, auto_sync: bool) -> WebDavSyncSettings {
        WebDavSyncSettings {
            enabled,
            auto_sync,
            base_url: remote.to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }
    }

    fn s3_settings(remote: &str, enabled: bool, auto_sync: bool) -> S3SyncSettings {
        S3SyncSettings {
            enabled,
            auto_sync,
            region: "us-east-1".to_string(),
            bucket: "bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            endpoint: remote.to_string(),
            ..S3SyncSettings::default()
        }
    }

    /// A DB change queues an auto upload while another sync operation holds
    /// the lock. While it waits, the user saves `changed` settings
    /// (`(enabled, auto_sync)`), or nothing. Returns how many requests reached
    /// the remote.
    async fn requests_after_queued_upload<B: AutoSyncBackend>(
        store: fn(Option<B::Settings>) -> Result<(), crate::error::AppError>,
        settings_for: fn(&str, bool, bool) -> B::Settings,
        changed: Option<(bool, bool)>,
    ) -> usize {
        let test_home = std::env::temp_dir().join(format!(
            "cc-switch-{}-auto-sync-queue-test",
            B::NAME.to_lowercase()
        ));
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);
        settings::update_settings(AppSettings::default()).expect("reset settings");

        let (remote, hits) = counting_remote();
        store(Some(settings_for(&remote, true, true))).expect("seed settings");

        let db = Arc::new(Database::memory().expect("memory db"));
        let app = tauri::test::mock_app();
        let (tx, rx) = channel::<String>(1);

        let other_sync = sync_mutex().lock().await;
        let worker = tokio::spawn(run_worker_loop::<B, _>(db, rx, app.handle().clone()));
        tx.send("providers".to_string())
            .await
            .expect("queue change");
        // Past the debounce, so the upload is queued on the sync lock.
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 300)).await;

        if let Some((enabled, auto_sync)) = changed {
            store(Some(settings_for(&remote, enabled, auto_sync))).expect("save settings");
        }
        drop(other_sync);
        drop(tx);
        tokio::time::timeout(Duration::from_secs(10), worker)
            .await
            .expect("worker should finish")
            .expect("worker should not panic");
        hits.load(Ordering::SeqCst)
    }

    macro_rules! queued_upload_tests {
        ($name:ident, $backend:ty, $store:path, $settings:path) => {
            mod $name {
                use super::*;

                #[tokio::test]
                #[serial]
                async fn queued_upload_runs_when_settings_are_unchanged() {
                    let requests =
                        requests_after_queued_upload::<$backend>($store, $settings, None).await;
                    assert!(requests > 0, "the queued upload should reach the remote");
                }

                #[tokio::test]
                #[serial]
                async fn queued_upload_is_dropped_after_auto_sync_is_turned_off() {
                    let requests = requests_after_queued_upload::<$backend>(
                        $store,
                        $settings,
                        Some((true, false)),
                    )
                    .await;
                    assert_eq!(
                        requests, 0,
                        "nothing may be uploaded after auto sync is off"
                    );
                }

                #[tokio::test]
                #[serial]
                async fn queued_upload_is_dropped_after_sync_is_disabled() {
                    let requests = requests_after_queued_upload::<$backend>(
                        $store,
                        $settings,
                        Some((false, true)),
                    )
                    .await;
                    assert_eq!(
                        requests, 0,
                        "nothing may be uploaded after sync is disabled"
                    );
                }
            }
        };
    }

    queued_upload_tests!(
        webdav,
        WebDavAutoSync,
        settings::set_webdav_sync_settings,
        webdav_settings
    );
    queued_upload_tests!(s3, S3AutoSync, settings::set_s3_sync_settings, s3_settings);
}
