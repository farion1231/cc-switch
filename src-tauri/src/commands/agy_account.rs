//! Antigravity 官方账号的状态与专用命令（加号、切号走授权中心的通用 `auth_*` 命令）。

use std::future::Future;
use std::path::Path;
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

    let (quota, should_record) = query_live_with_cache(
        &LAST_LIVE_QUERY,
        current.as_deref(),
        reuse,
        crate::services::subscription_agy::get_agy_subscription_quota,
        || async { manager.current_account_id().await == current },
    )
    .await?;
    if should_record {
        if let Some(account_id) = current.as_deref() {
            manager.record_quota(account_id, &quota).await;
        }
    }
    Ok(quota)
}

async fn query_live_with_cache<F, Fut, C, CFut>(
    cache: &Mutex<Option<LiveQuery>>,
    current: Option<&str>,
    reuse: Duration,
    fetch: F,
    is_still_current: C,
) -> Result<(SubscriptionQuota, bool), String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<SubscriptionQuota, String>>,
    C: FnOnce() -> CFut,
    CFut: Future<Output = bool>,
{
    let mut last = cache.lock().await;
    if let Some(query) = last.as_ref() {
        if query.account_id.as_deref() == current && query.at.elapsed() < reuse {
            return Ok((query.quota.clone(), false));
        }
    }
    let quota = fetch().await?;
    // 查询要七八秒：期间在别处换了号，这份结果就不知道是谁的，不记到任何账号上
    let still_current = is_still_current().await;
    // 失败或未检测到 agy 的占位结果不缓存，避免短时间手动重试命中失败缓存
    let should_record = still_current && quota.success;
    if should_record {
        *last = Some(LiveQuery {
            account_id: current.map(str::to_string),
            at: Instant::now(),
            quota: quota.clone(),
        });
    }
    Ok((quota, should_record))
}

/// 终端里跑的命令：agy 没登录时会直接进登录界面；已登录时用户在里面 `/logout` 再 `/login`。
/// 去掉 GUI 进程环境里可能残留的 Key，免得 agy 改走 Key 模式。
#[cfg_attr(not(windows), allow(dead_code))]
fn format_windows_login_command(bin: Option<&Path>) -> String {
    let invocation = match bin {
        Some(path) => {
            let escaped = path.to_string_lossy().replace('%', "%%");
            format!("call \"{escaped}\"")
        }
        None => TOOL.to_string(),
    };
    format!("set \"GEMINI_API_KEY=\" & set \"GOOGLE_GEMINI_BASE_URL=\" & {invocation}")
}

#[cfg_attr(windows, allow(dead_code))]
fn format_unix_login_command(bin: Option<&Path>) -> String {
    let invocation = match bin {
        Some(path) => {
            let escaped = path.to_string_lossy().replace('\'', "'\\''");
            format!("'{escaped}'")
        }
        None => TOOL.to_string(),
    };
    format!("env -u GEMINI_API_KEY -u GOOGLE_GEMINI_BASE_URL {invocation}")
}

fn login_command_for(bin: Option<&Path>) -> String {
    if cfg!(target_os = "windows") {
        format_windows_login_command(bin)
    } else {
        format_unix_login_command(bin)
    }
}

/// 打开用户首选终端运行 `agy`，让用户在 agy 自己的界面里登录要添加的账号。
pub(crate) async fn open_agy_login_terminal() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let bin = crate::agy_config::real_agy_bin();
        let command = login_command_for(bin.as_deref());
        crate::commands::terminal_launch::launch_terminal_running(&command, "agy_login")
    })
    .await
    .map_err(|e| format!("launch task join error: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::{CredentialStatus, QuotaTier};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ok_quota(utilization: f64) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: TOOL.to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: vec![QuotaTier {
                name: "agy_gemini_five_hour".to_string(),
                utilization,
                resets_at: None,
                used_value_usd: None,
                max_value_usd: None,
            }],
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            plan_name: None,
            error: None,
            queried_at: Some(1),
        }
    }

    #[tokio::test]
    async fn live_cache_reuses_success_only_and_never_caches_failures_or_switched_results() {
        let cache = Mutex::new(None);
        let calls = AtomicUsize::new(0);

        // 1. not_found / error 不写入缓存，下次查询必须重新触发 fetch
        let (first, record_first) = query_live_with_cache(
            &cache,
            Some("alice"),
            Duration::from_secs(20),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(SubscriptionQuota::not_found(TOOL))
            },
            || async { true },
        )
        .await
        .unwrap();
        assert!(!first.success);
        assert!(!record_first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let (second, record_second) = query_live_with_cache(
            &cache,
            Some("alice"),
            Duration::from_secs(20),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(SubscriptionQuota::error(
                    TOOL,
                    CredentialStatus::Valid,
                    "failed".into(),
                ))
            },
            || async { true },
        )
        .await
        .unwrap();
        assert!(!second.success);
        assert!(!record_second);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // 2. 查询期间切号：成功也不写入缓存或记录
        let (_switched, record_switched) = query_live_with_cache(
            &cache,
            Some("alice"),
            Duration::from_secs(20),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ok_quota(12.0))
            },
            || async { false },
        )
        .await
        .unwrap();
        assert!(!record_switched);
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // 3. 同一账号查询成功写入缓存并在复用窗口内命中
        let (ok, record_ok) = query_live_with_cache(
            &cache,
            Some("alice"),
            Duration::from_secs(20),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ok_quota(35.0))
            },
            || async { true },
        )
        .await
        .unwrap();
        assert!(ok.success);
        assert!(record_ok);
        assert_eq!(calls.load(Ordering::SeqCst), 4);

        let (cached, record_cached) = query_live_with_cache(
            &cache,
            Some("alice"),
            Duration::from_secs(20),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ok_quota(99.0))
            },
            || async { true },
        )
        .await
        .unwrap();
        assert_eq!(cached.tiers[0].utilization, 35.0);
        assert!(!record_cached);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn login_command_resolves_binary_path_and_escapes_special_chars() {
        assert_eq!(
            format_windows_login_command(None),
            "set \"GEMINI_API_KEY=\" & set \"GOOGLE_GEMINI_BASE_URL=\" & agy"
        );
        assert_eq!(
            format_windows_login_command(Some(Path::new(
                r"C:\Users\My %User%\AppData\Local\agy\bin\agy.exe"
            ))),
            r#"set "GEMINI_API_KEY=" & set "GOOGLE_GEMINI_BASE_URL=" & call "C:\Users\My %%User%%\AppData\Local\agy\bin\agy.exe""#
        );

        assert_eq!(
            format_unix_login_command(None),
            "env -u GEMINI_API_KEY -u GOOGLE_GEMINI_BASE_URL agy"
        );
        assert_eq!(
            format_unix_login_command(Some(Path::new("/Users/o'neil/.local/bin/agy"))),
            "env -u GEMINI_API_KEY -u GOOGLE_GEMINI_BASE_URL '/Users/o'\\''neil/.local/bin/agy'"
        );
    }
}
