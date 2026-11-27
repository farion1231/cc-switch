//! Antigravity 官方账号的状态与专用命令（加号、切号走授权中心的通用 `auth_*` 命令）。

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::services::agy_account::AgyAccountManager;
use crate::services::subscription::SubscriptionQuota;

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

/// 授权中心逐个账号、以及绑了账号的官方卡显示的剩余额度。agy 正登着这个号时问 agy 的
/// `/usage`；别的号返回切走前最后一次查到的（没查到过时为「没有凭据」，界面不显示）。
#[tauri::command(rename_all = "camelCase")]
pub async fn agy_account_quota(
    app: tauri::AppHandle,
    app_state: tauri::State<'_, crate::store::AppState>,
    account_id: String,
    state: tauri::State<'_, AgyAccountState>,
) -> Result<SubscriptionQuota, String> {
    let quota = agy_quota(&state.0, Some(&account_id), QUOTA_REUSE_INTERACTIVE).await?;
    remember_account_quota(&app, &app_state, account_id, &quota);
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

/// Antigravity 官方登录的额度，只问 agy 自己的 `/usage`（不用 agy 的身份发请求）。
/// 不指定账号就是 agy 现在登着的号；指定的号不是 agy 当前登录时，返回它最后一次查到的。
/// `reuse` 内查过同一个号就不再起 agy。查成功的结果按账号记下。
pub(crate) async fn agy_quota(
    manager: &AgyAccountManager,
    account_id: Option<&str>,
    reuse: Duration,
) -> Result<SubscriptionQuota, String> {
    if !crate::services::agy_account::keychain::supported() {
        return crate::services::subscription_agy::get_agy_subscription_quota().await;
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

    let mut last = LAST_LIVE_QUERY.lock().await;
    if let Some(query) = last.as_ref() {
        if query.account_id == current && query.at.elapsed() < reuse {
            return Ok(query.quota.clone());
        }
    }
    let quota = crate::services::subscription_agy::get_agy_subscription_quota().await?;
    // 查询要七八秒：期间在别处换了号，这份结果就不知道是谁的，不记到任何账号上
    let still_current = manager.current_account_id().await == current;
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
