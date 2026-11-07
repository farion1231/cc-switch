//! macOS 桌面小组件（`macos-widget/` 里的 WidgetKit 扩展）的数据快照。
//!
//! 小组件跑在沙盒里，读不到数据库，也拿不到登录凭据：这里把展示数据写成
//! `widget-snapshot.json`，放进两边共享的 App Group 容器，小组件按自己的时间线去读。
//! 快照里只有要显示的文字和数字，不含任何令牌。三段：
//! - `apps`：各应用在用的那家和额度（托盘面板算好的同一份）；
//! - `accounts`：授权中心全部账号的额度（`accounts.rs`，按账号分级刷新）；
//! - `usage`：用量统计（`usage.rs`，读本地数据库）。
//!
//! App Group 名字写在扩展的 Info.plist（`CCSwitchAppGroup`，打包脚本按 Team ID 生成）；
//! 没嵌入扩展的构建（`pnpm dev`、Windows / Linux）里找不到它，整个模块什么都不做。
//! 开发时可以用环境变量 `CC_SWITCH_WIDGET_APP_GROUP` 指定。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use once_cell::sync::OnceCell;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::store::AppState;
use crate::tray::{PanelModel, PanelQuota};

mod accounts;
mod reload;
mod usage;
mod writer;

pub use accounts::WidgetAccount;
pub use usage::WidgetUsage;

pub const SNAPSHOT_FILE: &str = "widget-snapshot.json";
/// 2：授权中心账号按渠道写结构化数据、用量带六个范围。小组件见到 1 才提示「请更新 CC Switch」。
const SNAPSHOT_VERSION: u32 = 2;
/// 扩展 Info.plist 里写 App Group 的键（与 `macos-widget/Info.plist` 一致）。
const APP_GROUP_PLIST_KEY: &str = "CCSwitchAppGroup";
const APP_GROUP_ENV: &str = "CC_SWITCH_WIDGET_APP_GROUP";
const WIDGET_APPEX: &str = "CCSwitchWidget.appex";
/// 后台多久替小组件查一次额度。各供应商自己的查询还有各自的节流。
const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 一轮里每一步（在用供应商 / 授权中心账号）最多等这么久，超时就先写已有的。
const REFRESH_STEP_TIMEOUT: Duration = Duration::from_secs(120);
/// 合并同一轮刷新里多次额度写入，只导出一次。
const EXPORT_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSnapshot {
    pub version: u32,
    /// Unix 秒
    pub generated_at: i64,
    pub apps: Vec<WidgetApp>,
    /// 还没列过账号（刚开启、第一轮还没跑完）时不写。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounts: Option<Vec<WidgetAccount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<WidgetUsage>,
    /// 设置里的「额度显示」：`left`（写剩余）/ `used`（写已用），授权中心账号的额度按它写。
    pub quota_display: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetApp {
    pub app: String,
    pub name: String,
    /// 在用那家的名字。
    pub current: Option<String>,
    /// `direct` / `route` / `stack`
    pub mode_kind: String,
    pub attention: Option<String>,
    pub quota_lines: Vec<WidgetQuotaLine>,
    /// 额度没查到时的原因（已按界面语言写好）。
    pub quota_note: Option<String>,
    /// 托盘不管的应用（Antigravity）：额度按档位原样给，小组件照授权中心的规则写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<accounts::TierRow>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetQuotaLine {
    /// 已按界面语言和「额度显示」设置写好的一行，如「5 小时 剩 62%」。
    pub text: String,
    /// 剩余百分比（0–100）；余额这种没有总额的为空，不画进度条。
    pub left: Option<f64>,
    pub warn: bool,
    pub resets_at: Option<String>,
}

fn widget_lines(quota: &PanelQuota) -> Vec<WidgetQuotaLine> {
    quota
        .lines
        .iter()
        .map(|line| WidgetQuotaLine {
            text: line.text.clone(),
            left: line.left,
            warn: line.warn,
            resets_at: line.resets_at.clone(),
        })
        .collect()
}

pub fn snapshot_from_panel(model: &PanelModel, generated_at: i64) -> WidgetSnapshot {
    let apps = model
        .apps
        .iter()
        .map(|app| {
            let (quota_lines, quota_note) = match &app.quota {
                Some(quota) => (widget_lines(quota), quota.note.clone()),
                None => (Vec::new(), None),
            };
            WidgetApp {
                app: app.app.clone(),
                name: app.name.to_string(),
                current: app.current.clone(),
                mode_kind: app.mode_kind.to_string(),
                attention: app.attention.map(str::to_string),
                quota_lines,
                quota_note,
                tiers: None,
            }
        })
        .collect();
    WidgetSnapshot {
        version: SNAPSHOT_VERSION,
        generated_at,
        apps,
        accounts: None,
        usage: None,
        quota_display: "left",
    }
}

/// Antigravity 在用的那家：官方登录时额度取它绑的 agy 账号（没绑就是 agy 现在登着的号）
/// 在授权中心那边查到的档位；第三方供应商没有额度，只写名字。
/// OpenCode、Pi、Kimi 等是多家并存的应用，没有「在用的那家」，只在用量里按应用出现。
fn agy_app_row(state: &AppState) -> Option<WidgetApp> {
    use crate::app_config::AppType;
    let visible = crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default()
        .is_visible(&AppType::Agy);
    if !visible || !cfg!(target_os = "macos") {
        return None;
    }
    let id = crate::mode::current::provider_for(
        &state.db,
        &AppType::Agy,
        crate::mode::current::Purpose::InUse,
    )
    .ok()??;
    let provider = state
        .db
        .get_provider_by_id(&id, AppType::Agy.as_str())
        .ok()??;
    let tiers = crate::services::provider::agy_direct::is_official(&provider)
        .then(|| {
            let bound = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.managed_account_id_for("agy_oauth"));
            accounts::agy_tiers(bound.as_deref())
        })
        .flatten();
    Some(WidgetApp {
        app: AppType::Agy.as_str().to_string(),
        name: crate::tray::app_display_name(&AppType::Agy).to_string(),
        current: Some(provider.name),
        mode_kind: "direct".to_string(),
        attention: None,
        quota_lines: Vec::new(),
        quota_note: None,
        tiers,
    })
}

/// 从扩展 Info.plist（XML）里取 `CCSwitchAppGroup` 的值。
fn app_group_from_plist(xml: &str) -> Option<String> {
    let key = format!("<key>{APP_GROUP_PLIST_KEY}</key>");
    let rest = &xml[xml.find(&key)? + key.len()..];
    let rest = rest.trim_start().strip_prefix("<string>")?;
    let value = rest[..rest.find("</string>")?].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn embedded_app_group() -> Option<String> {
    // …/CC Switch.app/Contents/MacOS/cc-switch → …/Contents/PlugIns/CCSwitchWidget.appex
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    let plist = contents
        .join("PlugIns")
        .join(WIDGET_APPEX)
        .join("Contents/Info.plist");
    app_group_from_plist(&std::fs::read_to_string(plist).ok()?)
}

/// 本次运行能不能给小组件供数：只在 macOS、且嵌入了扩展（或用环境变量指定）时有值。
pub fn app_group() -> Option<&'static str> {
    static GROUP: OnceCell<Option<String>> = OnceCell::new();
    GROUP
        .get_or_init(|| {
            if !cfg!(target_os = "macos") {
                return None;
            }
            std::env::var(APP_GROUP_ENV)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .or_else(embedded_app_group)
        })
        .as_deref()
}

fn snapshot_path(group: &str) -> PathBuf {
    crate::config::get_home_dir()
        .join("Library/Group Containers")
        .join(group)
        .join(SNAPSHOT_FILE)
}

pub fn is_enabled() -> bool {
    app_group().is_some() && crate::settings::get_settings().macos_widget_enabled
}

/// 串行写快照：两次导出并发时，先收集的旧数据不会后落盘覆盖新数据。
static EXPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 立即按当前缓存写一次快照（同步，读库 + 写文件；在 async 里用 `export_blocking`）。
/// 开关关着时什么都不做：关掉时正在跑的刷新、排着的导出都不能把快照写回去。
pub fn export_now(app: &AppHandle) {
    let _guard = EXPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if !is_enabled() {
        return;
    }
    let Some(group) = app_group() else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let started = std::time::Instant::now();
    log::debug!("[Widget] 开始收集快照");
    let model = match crate::tray::collect_panel_model(state.inner()) {
        Ok(model) => model,
        Err(e) => {
            log::warn!("[Widget] 收集小组件数据失败: {e}");
            return;
        }
    };
    let mut snapshot = snapshot_from_panel(&model, chrono::Utc::now().timestamp());
    // 托盘只管 5 个应用；Antigravity 也是「切换在用的那家」，小组件补上
    snapshot.apps.extend(agy_app_row(state.inner()));
    snapshot.accounts = accounts::snapshot();
    let settings = crate::settings::get_settings();
    if settings.quota_display.as_deref() == Some("used") {
        snapshot.quota_display = "used";
    }
    let money = usage::Money::from_settings(settings.usage_display.as_ref());
    snapshot.usage = match usage::collect(&state.db, &money) {
        Ok(usage) => Some(usage),
        Err(e) => {
            log::warn!("[Widget] 读取用量统计失败: {e}");
            None
        }
    };
    let path = snapshot_path(group);
    log::debug!(
        "[Widget] 数据收齐（{} ms），写入 {}",
        started.elapsed().as_millis(),
        path.display()
    );
    let bytes = match serde_json::to_vec(&snapshot) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("[Widget] 序列化小组件快照失败: {e}");
            return;
        }
    };
    // 内容比较不算写入时间，否则每次都算「变了」
    let content = serde_json::to_vec(&WidgetSnapshot {
        generated_at: 0,
        ..snapshot.clone()
    })
    .unwrap_or_default();
    let summary = format!(
        "{} 个应用，{} 个账号，用量{}",
        snapshot.apps.len(),
        snapshot.accounts.as_ref().map_or(0, Vec::len),
        if snapshot.usage.is_some() {
            "有"
        } else {
            "无"
        }
    );
    writer::submit(writer::Job::Write {
        path,
        bytes,
        content,
        summary,
    });
}

/// 额度或托盘状态变了：合并一下再导出。没开小组件时直接返回，热路径上只多一次读设置。
pub fn schedule_export(app: &AppHandle) {
    static SCHEDULED: AtomicBool = AtomicBool::new(false);
    if !is_enabled() || SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(EXPORT_DEBOUNCE);
        SCHEDULED.store(false, Ordering::Release);
        export_now(&app);
    });
}

/// 关掉小组件：删掉快照，小组件随即显示「未开启」。
pub fn clear_snapshot() {
    let Some(group) = app_group() else {
        return;
    };
    writer::submit(writer::Job::Delete {
        path: snapshot_path(group),
    });
}

/// 在阻塞线程池上写快照（读库、写文件不占 async 运行时的线程）。
async fn export_blocking(app: &AppHandle) {
    let app = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || export_now(&app)).await;
}

/// 同一时间只跑一轮刷新：后台循环和开关触发的刷新撞上时，后来的直接返回。
static REFRESHING: AtomicBool = AtomicBool::new(false);

struct RefreshGuard;

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::Release);
    }
}

/// 刷新一轮在用供应商和授权中心账号的额度，再导出。
pub async fn refresh_and_export(app: &AppHandle) {
    if REFRESHING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _guard = RefreshGuard;
    // 先列账号（本地，很快）再写一次：用量、各应用在用的那家、授权中心的账号马上就有，
    // 额度等下面的网络请求查完再补
    if tokio::time::timeout(REFRESH_STEP_TIMEOUT, accounts::list(app))
        .await
        .is_err()
    {
        log::warn!("[Widget] 读取授权中心账号超时");
    }
    export_blocking(app).await;
    let tray = async {
        crate::tray::refresh_all_usage_in_tray(app).await;
        crate::tray::refresh_panel_provider_usage(app).await;
    };
    if tokio::time::timeout(REFRESH_STEP_TIMEOUT, tray)
        .await
        .is_err()
    {
        log::warn!("[Widget] 刷新在用供应商额度超时");
    }
    export_blocking(app).await;
    // 超时只是不再等：各账号的查询在自己的任务里跑完并写回缓存，下次导出就带上
    if tokio::time::timeout(REFRESH_STEP_TIMEOUT, accounts::query_due(app))
        .await
        .is_err()
    {
        log::warn!("[Widget] 等授权中心账号额度超时，查完的下次导出再写");
    }
    export_blocking(app).await;
}

/// 启动流程走完后调用：开着小组件时每隔 `REFRESH_INTERVAL` 替它查一次额度。
/// 主窗口关掉、轻量模式下前端的定时查询都停了，小组件只能靠这里。
pub fn start_background_refresh(app: AppHandle) {
    log::info!(
        "[Widget] App Group: {:?}，开关: {}",
        app_group(),
        crate::settings::get_settings().macos_widget_enabled
    );
    if app_group().is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            if is_enabled() {
                refresh_and_export(&app).await;
            }
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });
}

/// 设置里切换了开关。
pub fn on_setting_changed(app: &AppHandle, enabled: bool) {
    // 没嵌入扩展的构建（pnpm dev 等）：开了也没地方写，别白查一遍额度
    if app_group().is_none() {
        return;
    }
    if enabled {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            refresh_and_export(&app).await;
        });
    } else {
        accounts::clear();
        // 删完会通知小组件重读，换成「未开启」
        clear_snapshot();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::{PanelApp, PanelQuota, PanelQuotaLine};

    fn panel_app(app: &str, quota: Option<PanelQuota>) -> PanelApp {
        PanelApp {
            app: app.to_string(),
            name: "Claude Code",
            current: Some("Official".to_string()),
            mode: "Direct",
            mode_kind: "direct",
            header: None,
            attention: None,
            quota,
            add_id: None,
            add_text: String::new(),
            rows: Vec::new(),
        }
    }

    #[test]
    fn snapshot_keeps_display_fields_only() {
        let model = PanelModel {
            problems: Vec::new(),
            feedback: None,
            apps: vec![
                panel_app(
                    "claude",
                    Some(PanelQuota {
                        lines: vec![PanelQuotaLine {
                            text: "5-hour 62% left".to_string(),
                            left: Some(62.0),
                            warn: false,
                            resets_at: Some("2026-10-28T12:00:00Z".to_string()),
                            emphasis: None,
                        }],
                        note: None,
                        reset_credits: Some(2),
                    }),
                ),
                panel_app("codex", None),
            ],
            lightweight: false,
            quit_hint: None,
        };

        let snapshot = snapshot_from_panel(&model, 42);
        assert_eq!(snapshot.version, SNAPSHOT_VERSION);
        assert_eq!(snapshot.generated_at, 42);
        assert_eq!(snapshot.apps.len(), 2);
        assert_eq!(snapshot.apps[0].quota_lines[0].left, Some(62.0));
        assert!(snapshot.apps[1].quota_lines.is_empty());

        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["generatedAt"], 42);
        assert_eq!(json["apps"][0]["quotaLines"][0]["text"], "5-hour 62% left");
        assert_eq!(json["apps"][0]["modeKind"], "direct");
        // 账号、用量没准备好时不写这两段，小组件按「还没有」处理
        assert!(json.get("accounts").is_none());
        assert!(json.get("usage").is_none());
    }

    #[test]
    fn reads_app_group_from_info_plist() {
        let xml = r#"<?xml version="1.0"?>
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key>
  <string>com.ccswitch.desktop.widget</string>
  <key>CCSwitchAppGroup</key>
  <string>ABCDE12345.com.ccswitch.desktop</string>
</dict></plist>"#;
        assert_eq!(
            app_group_from_plist(xml).as_deref(),
            Some("ABCDE12345.com.ccswitch.desktop")
        );
        assert_eq!(app_group_from_plist("<dict></dict>"), None);
        assert_eq!(
            app_group_from_plist("<key>CCSwitchAppGroup</key><string> </string>"),
            None
        );
    }
}
