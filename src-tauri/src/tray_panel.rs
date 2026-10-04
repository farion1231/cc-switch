//! 托盘左键弹出的用量面板（macOS）：菜单栏图标下方的一块毛玻璃窗口，
//! 写 token 用量和各应用在用那家的额度。右键仍是原生菜单（切换供应商）。
//!
//! 窗口懒创建、之后只隐藏不销毁；失焦即隐藏，像系统的菜单栏弹出面板。
//! 额度不在弹出时查：后台每 5 分钟查一次写进缓存，面板只读缓存（手动刷新除外）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::proxy::providers::copilot_auth::CopilotUsageResponse;
use crate::services::subscription::{CredentialStatus, QuotaTier, SubscriptionQuota};
use crate::store::AppState;
use crate::tray::TrayPanelApp;

#[cfg(target_os = "macos")]
pub use popup::toggle_panel;

pub const PANEL_LABEL: &str = "tray-panel";

const PANEL_WIDTH: f64 = 340.0;
const PANEL_MIN_HEIGHT: f64 = 160.0;
const PANEL_MAX_HEIGHT: f64 = 720.0;

fn panel_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL_LABEL)
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(window) = panel_window(app) {
        let _ = window.hide();
    }
}

/// 只有 macOS 托盘左键会弹出面板（Windows 左键开主界面，Linux 不派发点击事件）。
#[cfg(target_os = "macos")]
mod popup {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::window::{Effect, EffectState, EffectsBuilder};
    use tauri::{
        AppHandle, Emitter, PhysicalPosition, Rect, WebviewUrl, WebviewWindow,
        WebviewWindowBuilder, WindowEvent,
    };

    use super::{panel_window, PANEL_LABEL, PANEL_WIDTH};

    /// 前端每次弹出时据此重新拉数据。
    const PANEL_SHOWN_EVENT: &str = "tray-panel-shown";
    const PANEL_RADIUS: f64 = 14.0;
    /// 面板顶边离菜单栏图标的距离（逻辑像素）。
    const PANEL_GAP: f64 = 6.0;
    /// 面板离屏幕左右边缘至少留这么多（逻辑像素）。
    const SCREEN_MARGIN: f64 = 8.0;

    /// 点托盘图标关面板时，失焦隐藏会先于点击事件到达；这段时间内的点击当作「关」，
    /// 否则面板会被刚隐藏又立刻弹出。
    const BLUR_CLICK_GRACE: Duration = Duration::from_millis(300);
    static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);

    fn create_panel(app: &AppHandle) -> tauri::Result<WebviewWindow> {
        let window =
            WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("tray-panel.html".into()))
                .title("CC Switch")
                .inner_size(PANEL_WIDTH, 480.0)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible_on_all_workspaces(true)
                .visible(false)
                .focused(false)
                .shadow(true)
                .transparent(true)
                .build()?;
        apply_glass(&window);

        let handle = window.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::Focused(false) if handle.is_visible().unwrap_or(false) => {
                *LAST_BLUR_HIDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                let _ = handle.hide();
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = handle.hide();
            }
            _ => {}
        });
        Ok(window)
    }

    /// macOS 26+ 垫一层 Liquid Glass；更早的系统退回系统的 Popover 毛玻璃。
    /// 都要在主线程上动 AppKit 视图。
    fn apply_glass(window: &WebviewWindow) {
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            if !attach_liquid_glass(&target) {
                let _ = target.set_effects(
                    EffectsBuilder::new()
                        .effect(Effect::Popover)
                        // 失焦也保持毛玻璃（虽然失焦就会隐藏，但避免弹出瞬间闪灰）
                        .state(EffectState::Active)
                        .radius(PANEL_RADIUS)
                        .build(),
                );
            }
        });
    }

    /// 在网页（透明）下面插一个铺满窗口的 `NSGlassEffectView`；系统没有这个类时返回 false。
    fn attach_liquid_glass(window: &WebviewWindow) -> bool {
        use objc2_06::{runtime::AnyClass, MainThreadMarker};
        use objc2_app_kit_03::{
            NSAutoresizingMaskOptions, NSGlassEffectView, NSGlassEffectViewStyle, NSWindow,
            NSWindowOrderingMode,
        };

        if AnyClass::get(c"NSGlassEffectView").is_none() {
            return false;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Ok(ns_window) = window.ns_window() else {
            return false;
        };
        // SAFETY: 指针来自 tao，窗口存活期间有效；这里在主线程上只借用不持有。
        let ns_window = unsafe { &*ns_window.cast::<NSWindow>() };
        let Some(content) = ns_window.contentView() else {
            return false;
        };

        let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), content.bounds());
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        glass.setCornerRadius(PANEL_RADIUS);
        glass.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        // 放在所有子视图（WKWebView）下面
        content.addSubview_positioned_relativeTo(&glass, NSWindowOrderingMode::Below, None);
        true
    }

    /// 面板放在图标正下方、水平居中，左右不出屏。`icon` 是托盘点击事件带的图标矩形。
    fn position_panel(app: &AppHandle, window: &WebviewWindow, icon: &Rect) {
        let scale = window.scale_factor().unwrap_or(1.0);
        let icon_pos = icon.position.to_physical::<f64>(scale);
        let icon_size = icon.size.to_physical::<f64>(scale);
        let center_x = icon_pos.x + icon_size.width / 2.0;

        let monitor = app
            .monitor_from_point(center_x, icon_pos.y + icon_size.height / 2.0)
            .ok()
            .flatten();
        let scale = monitor.as_ref().map_or(scale, |m| m.scale_factor());
        let width = PANEL_WIDTH * scale;
        let mut x = center_x - width / 2.0;
        if let Some(m) = &monitor {
            let left = m.position().x as f64 + SCREEN_MARGIN * scale;
            let right = (m.position().x + m.size().width as i32) as f64 - SCREEN_MARGIN * scale;
            x = x.min(right - width).max(left);
        }
        let y = icon_pos.y + icon_size.height + PANEL_GAP * scale;
        let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    }

    /// 托盘左键：开着就关，关着就在图标下方弹出。
    pub fn toggle_panel(app: &AppHandle, icon: &Rect) {
        let window = match panel_window(app) {
            Some(w) => w,
            None => match create_panel(app) {
                Ok(w) => w,
                Err(e) => {
                    log::error!("[TrayPanel] 创建面板窗口失败: {e}");
                    return;
                }
            },
        };

        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
            return;
        }
        let just_blurred = LAST_BLUR_HIDE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .is_some_and(|at| at.elapsed() < BLUR_CLICK_GRACE);
        if just_blurred {
            return;
        }

        position_panel(app, &window, icon);
        // 先发再显示：前端借这一下先同步主题，少闪一下旧配色
        let _ = window.emit_to(PANEL_LABEL, PANEL_SHOWN_EVENT, ());
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// ─── 定时查额度 ────────────────────────────────────────────────────────────────

/// 面板不在弹出时查接口：后台每隔这么久查一次，面板只读缓存。
#[cfg(target_os = "macos")]
const QUOTA_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// 启动后等一会儿再查第一次，避开启动时的一堆初始化。
#[cfg(target_os = "macos")]
const QUOTA_FIRST_REFRESH_DELAY: std::time::Duration = std::time::Duration::from_secs(15);
/// 查完通知面板重新读缓存。
const PANEL_UPDATED_EVENT: &str = "tray-panel-updated";

/// 授权中心的三家，顺序和授权中心一致。
const AUTH_PROVIDERS: [&str; 3] = ["github_copilot", "codex_oauth", "xai_oauth"];

/// 授权中心里一个账号和它上次查到的额度。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayPanelAccount {
    pub provider: &'static str,
    pub account: crate::commands::ManagedAuthAccount,
    /// 需要重新登录的账号不查，为空；Copilot 的高级请求也折成同样的结构
    pub quota: Option<SubscriptionQuota>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayPanelSnapshot {
    pub apps: Vec<TrayPanelApp>,
    pub accounts: Vec<TrayPanelAccount>,
    /// 上次定时 / 手动查询完成的时间（毫秒）
    pub refreshed_at: Option<i64>,
}

#[derive(Default)]
struct AccountsCache {
    accounts: Vec<TrayPanelAccount>,
    refreshed_at: Option<i64>,
}

static ACCOUNTS_CACHE: Lazy<RwLock<AccountsCache>> = Lazy::new(Default::default);
/// 定时和手动刷新撞在一起时只跑一个。
static REFRESHING: AtomicBool = AtomicBool::new(false);

fn needs_reauth(account: &crate::commands::ManagedAuthAccount) -> bool {
    account.reauth_required || account.requires_reauth
}

fn copilot_quota(result: Result<CopilotUsageResponse, String>) -> SubscriptionQuota {
    match result {
        Ok(usage) => {
            let premium = &usage.quota_snapshots.premium_interactions;
            let utilization = if premium.entitlement > 0 {
                (premium.entitlement - premium.remaining) as f64 / premium.entitlement as f64
                    * 100.0
            } else {
                0.0
            };
            SubscriptionQuota {
                tool: "github_copilot".to_string(),
                credential_status: CredentialStatus::Valid,
                credential_message: None,
                success: true,
                tiers: vec![QuotaTier {
                    name: "premium".to_string(),
                    utilization,
                    resets_at: Some(usage.quota_reset_date.clone()),
                    used_value_usd: None,
                    max_value_usd: None,
                }],
                extra_usage: None,
                reset_credits: None,
                error: None,
                queried_at: Some(chrono::Utc::now().timestamp_millis()),
            }
        }
        Err(e) => SubscriptionQuota::error("github_copilot", CredentialStatus::Valid, e),
    }
}

/// 查一个账号的额度。瞬时网络错误时沿用上次查到的，免得面板一闪「没查到」。
async fn query_account(
    app: &AppHandle,
    provider: &'static str,
    account: &crate::commands::ManagedAuthAccount,
    previous: Option<SubscriptionQuota>,
) -> Option<SubscriptionQuota> {
    if needs_reauth(account) {
        return None;
    }
    let id = account.id.clone();
    let result = match provider {
        "codex_oauth" => {
            crate::commands::get_codex_oauth_quota(app.clone(), app.state(), Some(id), app.state())
                .await
        }
        "xai_oauth" => crate::commands::get_xai_oauth_quota(Some(id), app.state()).await,
        _ => Ok(copilot_quota(
            crate::commands::copilot_get_usage_for_account(id, app.state()).await,
        )),
    };
    match result {
        Ok(quota) => Some(quota),
        Err(e) => {
            log::debug!("[TrayPanel] 查 {provider} 账号额度失败: {e}");
            previous.or_else(|| {
                Some(SubscriptionQuota::error(
                    provider,
                    CredentialStatus::Valid,
                    e,
                ))
            })
        }
    }
}

/// 查一遍：各应用在用那家（写进 UsageCache）+ 授权中心每个账号，然后通知面板。
pub async fn refresh_quotas(app: &AppHandle) {
    if REFRESHING.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::tray::refresh_all_usage_in_tray(app).await;

    let previous = ACCOUNTS_CACHE
        .read()
        .map(|cache| cache.accounts.clone())
        .unwrap_or_default();
    let mut accounts = Vec::new();
    for provider in AUTH_PROVIDERS {
        let list = match crate::commands::auth_list_accounts(
            provider.to_string(),
            app.state(),
            app.state(),
            app.state(),
        )
        .await
        {
            Ok(list) => list,
            Err(e) => {
                log::debug!("[TrayPanel] 读 {provider} 账号失败: {e}");
                continue;
            }
        };
        let queries = list.into_iter().map(|account| {
            let previous = previous
                .iter()
                .find(|entry| entry.provider == provider && entry.account.id == account.id)
                .and_then(|entry| entry.quota.clone());
            async move {
                let quota = query_account(app, provider, &account, previous).await;
                TrayPanelAccount {
                    provider,
                    account,
                    quota,
                }
            }
        });
        accounts.extend(futures::future::join_all(queries).await);
    }

    if let Ok(mut cache) = ACCOUNTS_CACHE.write() {
        cache.accounts = accounts;
        cache.refreshed_at = Some(chrono::Utc::now().timestamp_millis());
    }
    REFRESHING.store(false, Ordering::Release);
    let _ = app.emit_to(PANEL_LABEL, PANEL_UPDATED_EVENT, ());
}

/// 后台定时查额度（只有 macOS 有面板，其他平台不跑）。
#[cfg(target_os = "macos")]
pub fn start_quota_worker(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(QUOTA_FIRST_REFRESH_DELAY).await;
        loop {
            refresh_quotas(&app).await;
            tokio::time::sleep(QUOTA_REFRESH_INTERVAL).await;
        }
    });
}

fn snapshot(app: &AppHandle) -> TrayPanelSnapshot {
    let (accounts, refreshed_at) = ACCOUNTS_CACHE
        .read()
        .map(|cache| (cache.accounts.clone(), cache.refreshed_at))
        .unwrap_or_default();
    TrayPanelSnapshot {
        apps: crate::tray::collect_panel_apps(&app.state::<AppState>()),
        accounts,
        refreshed_at,
    }
}

// ─── 命令 ──────────────────────────────────────────────────────────────────────

/// 读缓存，不发请求：额度由后台定时查（`start_quota_worker`）。
#[tauri::command]
pub fn get_tray_panel_snapshot(app: AppHandle) -> TrayPanelSnapshot {
    snapshot(&app)
}

/// 面板上点刷新：立刻查一遍再返回。
#[tauri::command]
pub async fn refresh_tray_panel(app: AppHandle) -> TrayPanelSnapshot {
    refresh_quotas(&app).await;
    snapshot(&app)
}

#[tauri::command]
pub fn tray_panel_hide(app: AppHandle) {
    hide_panel(&app);
}

/// 前端按内容高度回报，面板跟着内容伸缩（顶边不动）。
#[tauri::command]
pub fn tray_panel_set_height(app: AppHandle, height: f64) {
    if let Some(window) = panel_window(&app) {
        let height = height.clamp(PANEL_MIN_HEIGHT, PANEL_MAX_HEIGHT);
        let _ = window.set_size(tauri::LogicalSize::new(PANEL_WIDTH, height));
    }
}

#[tauri::command]
pub fn tray_panel_open_main(app: AppHandle) {
    hide_panel(&app);
    crate::tray::show_main_window(&app);
}
