//! Antigravity 官方账号的状态与专用命令（加号、切号走授权中心的通用 `auth_*` 命令）。

use std::future::Future;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::error::AppError;
use crate::services::agy_account::{AgyAccountManager, AgyAuthOp};
use crate::services::subscription::{CredentialStatus, SubscriptionQuota};

const TOOL: &str = "agy";

/// 界面上的额度查询在这么短的时间里重复时，直接用上一次 `/usage` 的结果，免得同时起好几个 agy。
pub(crate) const QUOTA_REUSE_INTERACTIVE: Duration = Duration::from_secs(20);

pub struct AgyAccountState(pub Arc<AgyAccountManager>);

/// 上一次 `/usage` 的结果：查的时候 agy 登着哪个号、什么时候查的。查询期间持锁，
/// 同时来的请求排队等这一次的结果，不另起 agy。
struct LiveQuery {
    account_id: Option<String>,
    at: Instant,
    quota: SubscriptionQuota,
}

static LAST_LIVE_QUERY: LazyLock<Mutex<Option<LiveQuery>>> = LazyLock::new(|| Mutex::new(None));

/// 切号后提醒用：在跑的 agy 还用旧账号，续期时可能把旧登录写回钥匙串。
#[tauri::command]
pub async fn agy_account_is_running() -> Result<bool, String> {
    tokio::task::spawn_blocking(crate::services::agy_account::agy_running)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn agy_account_running_state(
) -> Result<crate::services::agy_account::AgyRunningState, String> {
    tokio::task::spawn_blocking(crate::services::agy_account::agy_running_state)
        .await
        .map_err(|error| error.to_string())
}

/// 授权中心逐个账号、以及绑了账号的官方卡显示的剩余额度。
/// 默认（`manual = false`）只读本地 `last_quota` 缓存，绝不自动起 `agy --print /usage`；
/// 仅当用户手动点刷新（`manual = true`）且 agy 正登着这个号时才执行 `/usage`。
#[tauri::command(rename_all = "camelCase")]
pub async fn agy_account_quota(
    app: tauri::AppHandle,
    app_state: tauri::State<'_, crate::store::AppState>,
    account_id: String,
    manual: Option<bool>,
    state: tauri::State<'_, AgyAccountState>,
) -> Result<SubscriptionQuota, String> {
    let manual = manual.unwrap_or(false);
    let quota = agy_quota(&state.0, Some(&account_id), QUOTA_REUSE_INTERACTIVE, manual).await?;
    if manual || !matches!(quota.credential_status, CredentialStatus::NotFound) {
        remember_account_quota(&app, &app_state, account_id, &quota);
    }
    Ok(quota)
}

/// 「添加账号」等待中「重新打开终端」：只再开一个终端，不重置这次登录。
#[tauri::command]
pub async fn agy_account_open_login_terminal() -> Result<(), String> {
    open_agy_login_terminal().await
}

/// 按账号查到的额度写进托盘的缓存（托盘逐个账号显示）。不能切号的平台查的是 agy 自己的
/// `/usage`，不是这个账号的，不写。
pub(crate) fn remember_account_quota(
    app: &tauri::AppHandle,
    app_state: &crate::store::AppState,
    account_id: String,
    quota: &SubscriptionQuota,
) {
    if !crate::services::agy_account::keychain::supported() {
        return;
    }
    app_state
        .usage_cache
        .put_agy_account(account_id, quota.clone());
    crate::tray::schedule_tray_refresh(app);
}

fn active_auth_op_error(op: AgyAuthOp) -> String {
    match op {
        AgyAuthOp::Switching => AppError::localized(
            "agy.quota.switch_in_progress",
            "正在切换 Antigravity 账号，请稍后再刷新额度",
            "An Antigravity account switch is in progress. Refresh quota again in a moment.",
        )
        .to_string(),
        AgyAuthOp::LoginPending => AppError::localized(
            "agy.quota.login_in_progress",
            "正在进行 Antigravity 登录，请完成或取消后再刷新额度",
            "An Antigravity sign-in is in progress. Finish or cancel it before refreshing quota.",
        )
        .to_string(),
    }
}

/// Antigravity 官方登录的额度，只问 agy 自己的 `/usage`（不用 agy 的身份发请求）。
/// `manual == false` 时只读本地 `last_quota` 缓存，绝不启动 `agy --print /usage`。
/// `manual == true` 时先做登录中 / 切号中互斥与冷却保护（不拦截运行中的 Antigravity Desktop），
/// 仅在 agy 当前登录的就是目标账号时才执行 `/usage` 并把成功结果记入 `last_quota`。
pub(crate) async fn agy_quota(
    manager: &AgyAccountManager,
    account_id: Option<&str>,
    reuse: Duration,
    manual: bool,
) -> Result<SubscriptionQuota, String> {
    agy_quota_with(
        manager,
        account_id,
        reuse,
        manual,
        &LAST_LIVE_QUERY,
        crate::services::subscription_agy::get_agy_subscription_quota,
    )
    .await
}

async fn agy_quota_with<F, Fut>(
    manager: &AgyAccountManager,
    account_id: Option<&str>,
    reuse: Duration,
    manual: bool,
    live_cache: &Mutex<Option<LiveQuery>>,
    runner: F,
) -> Result<SubscriptionQuota, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<SubscriptionQuota, String>>,
{
    if !manual {
        if !crate::services::agy_account::keychain::supported() {
            if account_id.is_none() {
                if let Some(query) = live_cache.lock().await.as_ref() {
                    if query.account_id.is_none() {
                        return Ok(query.quota.clone());
                    }
                }
            }
            return Ok(SubscriptionQuota::not_found(TOOL));
        }
        let target_id = match account_id {
            Some(id) => Some(id.to_string()),
            None => manager.current_account_id().await,
        };
        return Ok(match target_id {
            Some(id) => manager
                .last_quota(&id)
                .await
                .unwrap_or_else(|| SubscriptionQuota::not_found(TOOL)),
            None => SubscriptionQuota::not_found(TOOL),
        });
    }

    if let Some(op) = manager.active_auth_operation().await {
        return Err(active_auth_op_error(op));
    }

    if !crate::services::agy_account::keychain::supported() {
        let mut last = live_cache.lock().await;
        if let Some(op) = manager.active_auth_operation().await {
            return Err(active_auth_op_error(op));
        }
        if let Some(query) = last.as_ref() {
            if query.account_id.is_none() && query.at.elapsed() < reuse {
                return Ok(query.quota.clone());
            }
        }
        let quota = runner().await?;
        *last = Some(LiveQuery {
            account_id: None,
            at: Instant::now(),
            quota: quota.clone(),
        });
        return Ok(quota);
    }

    let current = manager.current_account_id().await;
    if let Some(account_id) = account_id {
        if current.as_deref() != Some(account_id) {
            return Ok(manager
                .last_quota(account_id)
                .await
                .unwrap_or_else(|| SubscriptionQuota::not_found(TOOL)));
        }
    }
    if current.is_none() {
        return Ok(SubscriptionQuota::not_found(TOOL));
    }

    let mut last = live_cache.lock().await;
    if let Some(op) = manager.active_auth_operation().await {
        return Err(active_auth_op_error(op));
    }
    if let Some(query) = last.as_ref() {
        if query.account_id == current && query.at.elapsed() < reuse {
            return Ok(query.quota.clone());
        }
    }
    let quota = runner().await?;
    // 查询要七八秒：期间在别处换了号或发起了切号/登录，这份结果就不知道是谁的，不记到任何账号上
    let still_current = manager.current_account_id().await == current
        && manager.active_auth_operation().await.is_none();
    if still_current {
        *last = Some(LiveQuery {
            account_id: current.clone(),
            at: Instant::now(),
            quota: quota.clone(),
        });
        if let (Some(account_id), true) = (current.as_deref(), quota.success) {
            manager.record_quota(account_id, &quota).await;
        }
    }
    Ok(quota)
}

/// 终端里跑的命令：agy 没登录时会直接进登录界面；已登录时用户在里面 `/logout` 再 `/login`。
/// 去掉 GUI 进程环境里可能残留的 Key，免得 agy 改走 Key 模式。
fn login_command() -> String {
    if cfg!(target_os = "windows") {
        "set \"GEMINI_API_KEY=\" & set \"GOOGLE_GEMINI_BASE_URL=\" & agy".to_string()
    } else {
        "env -u GEMINI_API_KEY -u GOOGLE_GEMINI_BASE_URL agy".to_string()
    }
}

/// 打开用户首选终端运行 `agy`，让用户在 agy 自己的界面里登录要添加的账号。
pub(crate) async fn open_agy_login_terminal() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        crate::commands::terminal_launch::launch_terminal_running(&login_command(), "agy_login")
    })
    .await
    .map_err(|e| format!("launch task join error: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::{json, Value};

    use crate::services::agy_account::keychain::{self, AgyLoginStore};
    use crate::services::agy_account::AgyRunningState;

    #[derive(Default)]
    struct StubStore {
        value: StdMutex<Option<String>>,
    }

    impl AgyLoginStore for StubStore {
        fn read(&self) -> Result<Option<String>, AppError> {
            Ok(self.value.lock().unwrap().clone())
        }
        fn write(&self, value: &str) -> Result<(), AppError> {
            *self.value.lock().unwrap() = Some(value.to_string());
            Ok(())
        }
    }

    fn cred(sub: &str) -> Value {
        let token = format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&json!({"sub": sub, "email": format!("{sub}@example.com")}))
                    .unwrap()
            )
        );
        json!({
            "token": {"access_token": "a1", "refresh_token": format!("r-{sub}")},
            "id_token": token,
        })
    }

    fn ok_quota() -> SubscriptionQuota {
        SubscriptionQuota {
            tool: TOOL.to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: vec![],
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            plan_name: None,
            error: None,
            queried_at: Some(1_700_000_000_000),
        }
    }

    #[tokio::test]
    async fn non_manual_quota_reads_last_quota_cache_without_running_usage() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StubStore::default());
        *store.value.lock().unwrap() = Some(keychain::encode(&cred("alice")).unwrap());
        let manager = AgyAccountManager::with_login_store(dir.path().to_path_buf(), store);
        assert_eq!(manager.current_account_id().await.as_deref(), Some("alice"));

        let live_cache = Mutex::new(None);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_ref = Arc::clone(&calls);

        // No cached last_quota yet -> returns not_found without invoking runner.
        let initial = agy_quota_with(
            &manager,
            Some("alice"),
            QUOTA_REUSE_INTERACTIVE,
            false,
            &live_cache,
            || {
                calls_ref.fetch_add(1, Ordering::SeqCst);
                async { Ok(ok_quota()) }
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            initial.credential_status,
            CredentialStatus::NotFound
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // With cached last_quota -> returns cached snapshot without invoking runner.
        let cached = ok_quota();
        manager.record_quota("alice", &cached).await;
        let got = agy_quota_with(
            &manager,
            None,
            QUOTA_REUSE_INTERACTIVE,
            false,
            &live_cache,
            || {
                calls_ref.fetch_add(1, Ordering::SeqCst);
                async { Ok(ok_quota()) }
            },
        )
        .await
        .unwrap();
        assert!(got.success);
        assert_eq!(got.queried_at, Some(1_700_000_000_000));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn manual_quota_rejects_while_login_pending_or_switch_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StubStore::default());
        *store.value.lock().unwrap() = Some(keychain::encode(&cred("alice")).unwrap());
        let manager = AgyAccountManager::with_login_store(dir.path().to_path_buf(), store);
        let live_cache = Mutex::new(None);
        let calls = Arc::new(AtomicUsize::new(0));

        // Login pending blocks manual /usage.
        let start = manager.start_login().await.unwrap();
        let calls_ref = Arc::clone(&calls);
        let err = agy_quota_with(
            &manager,
            Some("alice"),
            QUOTA_REUSE_INTERACTIVE,
            true,
            &live_cache,
            || {
                calls_ref.fetch_add(1, Ordering::SeqCst);
                async { Ok(ok_quota()) }
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("登录"), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(manager.cancel_login(&start.flow_id).await);

        // Switch in progress blocks manual /usage.
        manager.set_switching_for_test(true);
        let calls_ref = Arc::clone(&calls);
        let err = agy_quota_with(
            &manager,
            Some("alice"),
            QUOTA_REUSE_INTERACTIVE,
            true,
            &live_cache,
            || {
                calls_ref.fetch_add(1, Ordering::SeqCst);
                async { Ok(ok_quota()) }
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("切换"), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        manager.set_switching_for_test(false);
    }

    #[tokio::test]
    async fn manual_quota_respects_cooldown_and_allows_running_desktop() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StubStore::default());
        *store.value.lock().unwrap() = Some(keychain::encode(&cred("alice")).unwrap());
        let manager = AgyAccountManager::with_test_env(
            dir.path().to_path_buf(),
            store,
            Arc::new(|| AgyRunningState {
                cli: true,
                desktop: true,
            }),
            None,
        );
        assert_eq!(manager.current_account_id().await.as_deref(), Some("alice"));
        let live_cache = Mutex::new(None);
        let calls = Arc::new(AtomicUsize::new(0));

        // First manual query runs even while Antigravity Desktop + CLI are running.
        let calls_ref = Arc::clone(&calls);
        let first = agy_quota_with(
            &manager,
            Some("alice"),
            QUOTA_REUSE_INTERACTIVE,
            true,
            &live_cache,
            || {
                calls_ref.fetch_add(1, Ordering::SeqCst);
                async { Ok(ok_quota()) }
            },
        )
        .await
        .unwrap();
        assert!(first.success);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Second manual query within cooldown reuses cached live result without invoking runner.
        let calls_ref = Arc::clone(&calls);
        let second = agy_quota_with(
            &manager,
            Some("alice"),
            QUOTA_REUSE_INTERACTIVE,
            true,
            &live_cache,
            || {
                calls_ref.fetch_add(1, Ordering::SeqCst);
                async { Ok(ok_quota()) }
            },
        )
        .await
        .unwrap();
        assert!(second.success);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
