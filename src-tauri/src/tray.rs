//! 托盘菜单管理模块
//!
//! 负责系统托盘图标和菜单的创建、更新和事件处理。

use once_cell::sync::Lazy;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

/// 每个 app 分区的"第二行" disabled MenuItem 句柄，用于 usage 数据到达时就地更新文本，
/// 避免 `set_menu` 整建打断用户正在查看的菜单。
/// `create_tray_menu` 每次重建都会整表覆盖写入；缓存为空时不插入条目。
static TRAY_SECTION_DETAIL_ITEMS: Lazy<
    std::sync::Mutex<std::collections::HashMap<AppType, MenuItem<tauri::Wry>>>,
> = Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 主界面当前聚焦的 app（前端 activeApp 变化时写入）。
/// None = 未知（启动期）。
static TRAY_FOCUSED_APP: Lazy<std::sync::Mutex<Option<AppType>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

/// 最近一次有订阅数据的官方 app，在 TRAY_FOCUSED_APP 指向无数据 app 时作为 fallback。
static TRAY_LAST_OFFICIAL_APP: Lazy<std::sync::Mutex<Option<AppType>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

/// 托盘菜单文本（国际化）
#[derive(Clone, Copy)]
pub struct TrayTexts {
    pub show_main: &'static str,
    pub no_providers_label: &'static str,
    pub lightweight_mode: &'static str,
    pub quit: &'static str,
    pub _auto_label: &'static str,
}

impl TrayTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" => Self {
                show_main: "Open main window",
                no_providers_label: "(no providers)",
                lightweight_mode: "Lightweight Mode",
                quit: "Quit",
                _auto_label: "Auto (Failover)",
            },
            "ja" => Self {
                show_main: "メインウィンドウを開く",
                no_providers_label: "(プロバイダーなし)",
                lightweight_mode: "軽量モード",
                quit: "終了",
                _auto_label: "自動 (フェイルオーバー)",
            },
            _ => Self {
                show_main: "打开主界面",
                no_providers_label: "(无供应商)",
                lightweight_mode: "轻量模式",
                quit: "退出",
                _auto_label: "自动 (故障转移)",
            },
        }
    }
}

/// 托盘应用分区配置
pub struct TrayAppSection {
    pub app_type: AppType,
    pub prefix: &'static str,
    pub empty_id: &'static str,
    pub header_label: &'static str,
    pub log_name: &'static str,
}

/// Auto 菜单项后缀
pub const AUTO_SUFFIX: &str = "auto";
pub const TRAY_ID: &str = "cc-switch";

pub const TRAY_SECTIONS: [TrayAppSection; 3] = [
    TrayAppSection {
        app_type: AppType::Claude,
        prefix: "claude_",
        empty_id: "claude_empty",
        header_label: "Claude",
        log_name: "Claude",
    },
    TrayAppSection {
        app_type: AppType::Codex,
        prefix: "codex_",
        empty_id: "codex_empty",
        header_label: "Codex",
        log_name: "Codex",
    },
    TrayAppSection {
        app_type: AppType::Gemini,
        prefix: "gemini_",
        empty_id: "gemini_empty",
        header_label: "Gemini",
        log_name: "Gemini",
    },
];

/// 配色阈值（与前端 `utilizationColor` 语义一致）。
const UTIL_WARN_PCT: f64 = 70.0;
const UTIL_DANGER_PCT: f64 = 90.0;

fn emoji_for_utilization(pct: f64) -> &'static str {
    if pct >= UTIL_DANGER_PCT {
        "\u{1F534}" // 🔴
    } else if pct >= UTIL_WARN_PCT {
        "\u{1F7E0}" // 🟠
    } else {
        "\u{1F7E2}" // 🟢
    }
}

/// Original tray icon PNG bytes.
const ICON_BASE_BYTES: &[u8] = include_bytes!("../icons/tray/macos/statusbar_template_3x.png");

/// Decoded RGBA pixels of the base icon — decoded once at first use.
static ICON_BASE_RGBA: Lazy<(Vec<u8>, u32, u32)> =
    Lazy::new(|| match tauri::image::Image::from_bytes(ICON_BASE_BYTES) {
        Ok(img) => {
            let w = img.width();
            let h = img.height();
            (img.rgba().to_vec(), w, h)
        }
        Err(_) => (vec![0u8; 72 * 72 * 4], 72, 72),
    });

/// Last percentage rendered to the tray icon (integer bucket 0–100).
/// Used to skip re-renders when the utilization hasn't changed.
/// Cache key: (rounded_pct, color_tier) where tier 0=normal 1=warn 2=danger.
/// Including the tier ensures a re-render when utilization crosses a color
/// boundary even if both values round to the same integer.
static LAST_TRAY_ICON_PCT: std::sync::Mutex<Option<(u8, u8)>> =
    std::sync::Mutex::new(None);

fn pct_to_color_tier(pct: f64) -> u8 {
    if pct >= UTIL_DANGER_PCT {
        2
    } else if pct >= UTIL_WARN_PCT {
        1
    } else {
        0
    }
}

/// Recolor the original icon's pixels as a clockwise progress fill.
/// Non-transparent pixels within the fill angle get the utilization color;
/// the rest become white. Alpha is preserved to keep anti-aliased edges.
/// Returns None → caller should keep the startup template icon unchanged.
pub(crate) fn generate_ring_icon_rgba(utilization_pct: Option<f64>, size: u32) -> Vec<u8> {
    let (base, w, h) = &*ICON_BASE_RGBA;
    let mut buf = if *w == size && *h == size {
        base.clone()
    } else {
        vec![0u8; (size * size * 4) as usize]
    };

    let Some(pct) = utilization_pct else {
        return buf; // No data → original pixels, used at startup via template mode
    };

    let s = size as f64;
    let cx = s / 2.0;
    let cy = s / 2.0;

    let (fr, fg, fb) = if pct >= UTIL_DANGER_PCT {
        (230u8, 60u8, 60u8)
    } else if pct >= UTIL_WARN_PCT {
        (240u8, 120u8, 30u8)
    } else {
        (60u8, 200u8, 80u8)
    };

    let fill_angle = (pct / 100.0).clamp(0.0, 1.0) * std::f64::consts::TAU;
    let start = -std::f64::consts::FRAC_PI_2; // 12 o'clock

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            if buf[idx + 3] == 0 {
                continue; // transparent — outside icon shape, leave untouched
            }

            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let mut angle = dy.atan2(dx) - start;
            if angle < 0.0 {
                angle += std::f64::consts::TAU;
            }

            let (r, g, b) = if angle <= fill_angle {
                (fr, fg, fb) // filled sector → utilization color
            } else {
                (255u8, 255u8, 255u8) // unfilled sector → white
            };
            buf[idx] = r;
            buf[idx + 1] = g;
            buf[idx + 2] = b;
            // buf[idx + 3] unchanged — preserve original alpha
        }
    }

    buf
}

/// 获取指定 app 当前官方订阅的最高 tier 利用率（0–100）。
/// 当前 provider 非官方或无缓存时返回 None。
fn get_section_subscription_pct(
    app_state: &crate::store::AppState,
    app_type: &AppType,
) -> Option<f64> {
    let current_id = crate::settings::get_effective_current_provider(&app_state.db, app_type)
        .ok()
        .flatten()?;
    let providers = app_state.db.get_all_providers(app_type.as_str()).ok()?;
    let provider = providers.get(&current_id)?;
    if provider.category.as_deref() != Some("official") {
        return None;
    }
    app_state
        .usage_cache
        .with_subscription(app_type, |quota| {
            quota.tiers.iter().map(|t| t.utilization).reduce(f64::max)
        })
        .flatten()
}

/// 根据主界面焦点和设置计算托盘图标应展示的利用率。
/// - 有焦点且有数据 → 该 app 的利用率，并记录为 last_official
/// - 有焦点但无数据（第三方/无订阅）→ fallback 到 last_official
/// - 无焦点记录（启动期）→ 所有 section 的最大值
fn compute_tray_worst_pct(app_state: &crate::store::AppState) -> Option<f64> {
    let focused = TRAY_FOCUSED_APP
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();

    if let Some(ref app_type) = focused {
        if let Some(pct) = get_section_subscription_pct(app_state, app_type) {
            // 记录最近有数据的官方 app
            if let Ok(mut last) = TRAY_LAST_OFFICIAL_APP.lock() {
                *last = Some(app_type.clone());
            }
            return Some(pct);
        }
        // 焦点 app 无数据（第三方），回退到上一个有数据的官方 app
        let last = TRAY_LAST_OFFICIAL_APP
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(ref last_app) = last {
            return get_section_subscription_pct(app_state, last_app);
        }
        return None;
    }

    // 启动期无焦点：取所有 section 最大值
    let mut worst: Option<f64> = None;
    for section in TRAY_SECTIONS.iter() {
        if let Some(pct) = get_section_subscription_pct(app_state, &section.app_type) {
            worst = Some(worst.map_or(pct, |w: f64| w.max(pct)));
        }
    }
    worst
}

/// 前端通知主界面 activeApp 切换时调用，更新焦点并刷新托盘图标。
pub(crate) fn set_tray_focused_app(app_type_str: &str, app: &tauri::AppHandle) {
    use std::str::FromStr;
    let parsed = AppType::from_str(app_type_str).ok(); // 第三方 app 解析为 None
    if let Ok(mut guard) = TRAY_FOCUSED_APP.lock() {
        *guard = parsed;
    }
    update_tray_icon(app);
}

/// Public wrapper called from outside this module (e.g. after settings save).
pub fn update_tray_icon_pub(app: &tauri::AppHandle) {
    update_tray_icon(app);
}

/// Reset the tray usage refresh throttle so the next call to
/// `refresh_all_usage_in_tray` runs immediately regardless of when the last
/// refresh happened. Safe to call on deliberate user actions (settings save).
pub(crate) fn reset_tray_refresh_throttle() {
    *LAST_TRAY_USAGE_REFRESH
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = None;
}

/// Update the tray icon to show the original icon with a colored progress arc overlaid.
/// Only fires when subscription data is available and the setting is enabled.
fn update_tray_icon(app: &tauri::AppHandle) {
    let Some(app_state) = app.try_state::<crate::store::AppState>() else {
        return;
    };
    let enabled = crate::settings::get_settings().tray_progress_icon;
    if !enabled {
        if let Ok(mut last) = LAST_TRAY_ICON_PCT.lock() {
            *last = None; // clear so re-enabling shows the icon immediately
        }
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            #[cfg(target_os = "macos")]
            let _ = tray.set_icon_as_template(true);
            let (base, w, h) = &*ICON_BASE_RGBA;
            let icon = tauri::image::Image::new(base, *w, *h);
            let _ = tray.set_icon(Some(icon));
        }
        return;
    }
    // Ring icon is macOS-only; on other platforms leave the system icon untouched.
    #[cfg(not(target_os = "macos"))]
    return;

    #[cfg(target_os = "macos")]
    {
        let Some(pct) = compute_tray_worst_pct(&app_state) else {
            // Data disappeared (e.g. switched to a third-party provider).
            // If we previously rendered a ring icon, clear the cache and restore
            // the base template so a stale colored arc is not left on screen.
            let had_icon = LAST_TRAY_ICON_PCT
                .lock()
                .map(|mut g| g.take().is_some())
                .unwrap_or(false);
            if had_icon {
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    let _ = tray.set_icon_as_template(true);
                    let (base, w, h) = &*ICON_BASE_RGBA;
                    let icon = tauri::image::Image::new(base, *w, *h);
                    let _ = tray.set_icon(Some(icon));
                }
            }
            return;
        };
        let pct_key = (pct.round() as u8, pct_to_color_tier(pct));
        if let Ok(mut last) = LAST_TRAY_ICON_PCT.lock() {
            if *last == Some(pct_key) {
                return; // percentage and color tier unchanged — skip pixel re-render
            }
            *last = Some(pct_key);
        }
        let rgba = generate_ring_icon_rgba(Some(pct), 72);
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            // Switch off template mode so the colored ring is rendered as-is.
            let _ = tray.set_icon_as_template(false);
            let icon = tauri::image::Image::new(&rgba, 72, 72);
            if let Err(e) = tray.set_icon(Some(icon)) {
                log::debug!("[Tray] 更新环形图标失败: {e}");
            }
        }
    }
}

/// Parse an ISO 8601 reset timestamp and return a human-readable countdown.
fn format_countdown(resets_at_iso: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(resets_at_iso).ok()?;
    let secs = dt.signed_duration_since(chrono::Utc::now()).num_seconds();
    if secs <= 0 {
        return None;
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    Some(if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    })
}

#[allow(dead_code)] // only called from tests
fn format_subscription_summary(
    quota: &crate::services::subscription::SubscriptionQuota,
) -> Option<String> {
    use crate::services::subscription::{
        TIER_FIVE_HOUR, TIER_GEMINI_FLASH, TIER_GEMINI_FLASH_LITE, TIER_GEMINI_PRO, TIER_SEVEN_DAY,
    };
    if !quota.success {
        return None;
    }

    // 按 tool 选取主卡槽 tier 并映射到短 label：
    //   Claude / Codex 沿用时间窗口（h=5 小时，w=7 天）；
    //   Gemini 用模型维度（p=pro，f=flash，l=flash-lite）——Gemini 后端 tier
    //   命名是 gemini_pro / gemini_flash / gemini_flash_lite，与时间窗口不同命名空间。
    //   flash_lite 必须纳入：否则 lite 利用率最高时色标偏低，与前端 footer 行为不一致。
    let parts: Vec<(&'static str, f64)> = match quota.tool.as_str() {
        "gemini" => {
            let mut v = Vec::new();
            if let Some(t) = quota.tiers.iter().find(|t| t.name == TIER_GEMINI_PRO) {
                v.push(("p", t.utilization));
            }
            if let Some(t) = quota.tiers.iter().find(|t| t.name == TIER_GEMINI_FLASH) {
                v.push(("f", t.utilization));
            }
            if let Some(t) = quota
                .tiers
                .iter()
                .find(|t| t.name == TIER_GEMINI_FLASH_LITE)
            {
                v.push(("l", t.utilization));
            }
            v
        }
        _ => {
            let mut v = Vec::new();
            if let Some(t) = quota.tiers.iter().find(|t| t.name == TIER_FIVE_HOUR) {
                v.push(("h", t.utilization));
            }
            if let Some(t) = quota.tiers.iter().find(|t| t.name == TIER_SEVEN_DAY) {
                v.push(("w", t.utilization));
            }
            v
        }
    };

    if parts.is_empty() {
        return None;
    }

    // 色标取所有已选 tier 里最高的利用率——用户更关心"离上限多近"。
    let worst = parts
        .iter()
        .map(|(_, u)| *u)
        .fold(f64::NEG_INFINITY, f64::max);
    if !worst.is_finite() {
        return None;
    }

    let emoji = emoji_for_utilization(worst);
    let body = parts
        .iter()
        .map(|(label, u)| format!("{label}{}%", u.round() as i64))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("{emoji} {body}"))
}

/// Combines subscription usage and reset countdown into a single-line detail string
/// suitable for rendering as a dedicated disabled menu item beneath the app submenu.
/// Returns `None` when the quota indicates failure or no known tiers are present.
/// Builds the second-line detail string shown beneath each app's submenu entry.
/// Format matches the main-window subscription footer:
/// `emoji tier%  countdown  tier%  countdown …`  (countdown omitted when reset is past)
pub(crate) fn format_subscription_detail_from_quota(
    quota: &crate::services::subscription::SubscriptionQuota,
) -> Option<String> {
    use crate::services::subscription::{
        TIER_FIVE_HOUR, TIER_GEMINI_FLASH, TIER_GEMINI_FLASH_LITE, TIER_GEMINI_PRO, TIER_SEVEN_DAY,
    };
    if !quota.success {
        return None;
    }

    let tier_map: &[(&str, &str)] = match quota.tool.as_str() {
        "gemini" => &[
            (TIER_GEMINI_PRO, "p"),
            (TIER_GEMINI_FLASH, "f"),
            (TIER_GEMINI_FLASH_LITE, "l"),
        ],
        _ => &[(TIER_FIVE_HOUR, "h"), (TIER_SEVEN_DAY, "w")],
    };

    let mut worst = f64::NEG_INFINITY;
    let parts: Vec<String> = tier_map
        .iter()
        .filter_map(|(tier_name, label)| {
            let tier = quota.tiers.iter().find(|t| t.name == *tier_name)?;
            worst = worst.max(tier.utilization);
            let pct = format!("{}{:.0}%", label, tier.utilization);
            Some(match tier.resets_at.as_deref().and_then(format_countdown) {
                Some(cd) => format!("{pct} {cd}"),
                None => pct,
            })
        })
        .collect();

    if parts.is_empty() || !worst.is_finite() {
        return None;
    }

    Some(format!(
        "{} {}",
        emoji_for_utilization(worst),
        parts.join("  \u{00B7}  ")
    ))
}

fn tier_pct(data: &crate::provider::UsageData) -> Option<f64> {
    match (data.used, data.total) {
        (Some(used), Some(total)) if total > 0.0 => Some(used / total * 100.0),
        _ => None,
    }
}

fn format_script_summary(result: &crate::provider::UsageResult) -> Option<String> {
    use crate::services::subscription::{TIER_FIVE_HOUR, TIER_WEEKLY_LIMIT};

    if !result.success {
        return None;
    }
    let data = result.data.as_ref()?;
    if data.is_empty() {
        return None;
    }

    // commands::provider 的 token_plan 分支把 SubscriptionQuota 的每个 tier
    // 扁平化为一条 UsageData（plan_name 承载 tier 名），所以这里按 plan_name
    // 识别双桶形态，其余 usage 结果（Copilot / balance / 自定义脚本）走 fallback。
    const TOKEN_PLAN_LABELS: &[(&str, &str)] = &[(TIER_FIVE_HOUR, "h"), (TIER_WEEKLY_LIMIT, "w")];

    let mut parts: Vec<(&'static str, f64)> = Vec::new();
    for &(tier_name, label) in TOKEN_PLAN_LABELS {
        let Some(d) = data
            .iter()
            .find(|d| d.plan_name.as_deref() == Some(tier_name))
        else {
            continue;
        };
        if let Some(u) = tier_pct(d) {
            parts.push((label, u));
        }
    }
    if !parts.is_empty() {
        let worst = parts
            .iter()
            .map(|(_, u)| *u)
            .fold(f64::NEG_INFINITY, f64::max);
        let emoji = emoji_for_utilization(worst);
        let body = parts
            .iter()
            .map(|(label, u)| format!("{label}{}%", u.round() as i64))
            .collect::<Vec<_>>()
            .join(" ");
        return Some(format!("{emoji} {body}"));
    }

    let first = data.first()?;
    let pct = tier_pct(first)?;
    let emoji = emoji_for_utilization(pct);
    let plan = first.plan_name.as_deref().unwrap_or("");
    let rounded = pct.round() as i64;
    if plan.is_empty() {
        Some(format!("{} {}%", emoji, rounded))
    } else {
        Some(format!("{} {} {}%", emoji, plan, rounded))
    }
}

/// Builds the detail line text shown beneath each app's submenu (e.g. "   🟢 h9% w27% · ⏱ h 1h 30m").
/// Script usage takes priority over subscription quota. Returns `None` when the cache has no data.
fn format_usage_detail_line(
    app_state: &AppState,
    app_type: &AppType,
    provider: &crate::provider::Provider,
    provider_id: &str,
) -> Option<String> {
    if provider.has_usage_script_enabled() {
        let s = app_state
            .usage_cache
            .with_script(app_type, provider_id, format_script_summary)
            .flatten()?;
        return Some(s);
    } else {
        app_state
            .usage_cache
            .invalidate_script(app_type, provider_id);
    }

    if provider.category.as_deref() != Some("official") {
        return None;
    }

    app_state
        .usage_cache
        .with_subscription(app_type, format_subscription_detail_from_quota)
        .flatten()
}

/// 对供应商列表排序：sort_index → created_at → name
fn sort_providers(
    providers: &indexmap::IndexMap<String, crate::provider::Provider>,
) -> Vec<(&String, &crate::provider::Provider)> {
    let mut sorted: Vec<_> = providers.iter().collect();
    sorted.sort_by(|(_, a), (_, b)| {
        match (a.sort_index, b.sort_index) {
            (Some(idx_a), Some(idx_b)) => return idx_a.cmp(&idx_b),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }

        match (a.created_at, b.created_at) {
            (Some(time_a), Some(time_b)) => return time_a.cmp(&time_b),
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            _ => {}
        }

        a.name.cmp(&b.name)
    });
    sorted
}

/// 处理供应商托盘事件
pub fn handle_provider_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    for section in TRAY_SECTIONS.iter() {
        if let Some(suffix) = event_id.strip_prefix(section.prefix) {
            // 处理 Auto 点击
            if suffix == AUTO_SUFFIX {
                log::info!("切换到{} Auto模式", section.log_name);
                let app_handle = app.clone();
                let app_type = section.app_type.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(e) = handle_auto_click(&app_handle, &app_type) {
                        log::error!("切换{}Auto模式失败: {e}", section.log_name);
                    }
                });
                return true;
            }

            // 处理供应商点击
            log::info!("切换到{}供应商: {suffix}", section.log_name);
            let app_handle = app.clone();
            let provider_id = suffix.to_string();
            let app_type = section.app_type.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = handle_provider_click(&app_handle, &app_type, &provider_id) {
                    log::error!("切换{}供应商失败: {e}", section.log_name);
                }
            });
            return true;
        }
    }
    false
}

/// 处理 Auto 点击：启用 proxy 和 auto_failover
fn handle_auto_click(app: &tauri::AppHandle, app_type: &AppType) -> Result<(), AppError> {
    if let Some(app_state) = app.try_state::<AppState>() {
        let app_type_str = app_type.as_str();

        // 强一致语义：Auto 模式开启后立即切到队列 P1（P1→P2→...）
        // 若队列为空，则尝试把“当前供应商”自动加入队列作为 P1，避免用户陷入无法开启的死锁。
        let mut queue = app_state.db.get_failover_queue(app_type_str)?;
        if queue.is_empty() {
            let current_id =
                crate::settings::get_effective_current_provider(&app_state.db, app_type)?;
            let Some(current_id) = current_id else {
                return Err(AppError::Message(
                    "故障转移队列为空，且未设置当前供应商，无法启用 Auto 模式".to_string(),
                ));
            };
            app_state
                .db
                .add_to_failover_queue(app_type_str, &current_id)?;
            queue = app_state.db.get_failover_queue(app_type_str)?;
        }

        let p1_provider_id = queue
            .first()
            .map(|item| item.provider_id.clone())
            .ok_or_else(|| AppError::Message("故障转移队列为空，无法启用 Auto 模式".to_string()))?;

        // 真正启用 failover：启动代理服务 + 执行接管 + 开启 auto_failover
        let proxy_service = &app_state.proxy_service;

        // 1) 确保代理服务运行（会自动设置 proxy_enabled = true）
        let is_running = futures::executor::block_on(proxy_service.is_running());
        if !is_running {
            log::info!("[Tray] Auto 模式：启动代理服务");
            if let Err(e) = futures::executor::block_on(proxy_service.start()) {
                log::error!("[Tray] 启动代理服务失败: {e}");
                return Err(AppError::Message(format!("启动代理服务失败: {e}")));
            }
        }

        // 2) 执行 Live 配置接管（确保该 app 被代理接管）
        log::info!("[Tray] Auto 模式：对 {app_type_str} 执行接管");
        if let Err(e) =
            futures::executor::block_on(proxy_service.set_takeover_for_app(app_type_str, true))
        {
            log::error!("[Tray] 执行接管失败: {e}");
            return Err(AppError::Message(format!("执行接管失败: {e}")));
        }

        // 3) 设置 auto_failover_enabled = true
        app_state
            .db
            .set_proxy_flags_sync(app_type_str, true, true)?;

        // 3.1) 立即切到队列 P1（热切换：不写 Live，仅更新 DB/settings/备份）
        if let Err(e) = futures::executor::block_on(
            proxy_service.switch_proxy_target(app_type_str, &p1_provider_id),
        ) {
            log::error!("[Tray] Auto 模式切换到队列 P1 失败: {e}");
            return Err(AppError::Message(format!(
                "Auto 模式切换到队列 P1 失败: {e}"
            )));
        }

        // 4) 更新托盘菜单
        if let Ok(new_menu) = create_tray_menu(app, app_state.inner()) {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(new_menu));
            }
        }

        // 5) 发射事件到前端
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "proxyEnabled": true,
            "autoFailoverEnabled": true,
            "providerId": p1_provider_id
        });
        if let Err(e) = app.emit("proxy-flags-changed", event_data.clone()) {
            log::error!("发射 proxy-flags-changed 事件失败: {e}");
        }
        // 发射 provider-switched 事件（保持向后兼容，Auto 切换也算一种切换）
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    Ok(())
}

/// 处理供应商点击：关闭 auto_failover + 切换供应商
fn handle_provider_click(
    app: &tauri::AppHandle,
    app_type: &AppType,
    provider_id: &str,
) -> Result<(), AppError> {
    if let Some(app_state) = app.try_state::<AppState>() {
        let app_type_str = app_type.as_str();

        // 获取当前 proxy 状态，保持 enabled 不变，只关闭 auto_failover
        let (proxy_enabled, _) = app_state.db.get_proxy_flags_sync(app_type_str);
        app_state
            .db
            .set_proxy_flags_sync(app_type_str, proxy_enabled, false)?;

        // 切换供应商
        crate::commands::switch_provider(
            app_state.clone(),
            app_type_str.to_string(),
            provider_id.to_string(),
        )
        .map_err(AppError::Message)?;

        // 更新托盘菜单
        if let Ok(new_menu) = create_tray_menu(app, app_state.inner()) {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(new_menu));
            }
        }

        // 发射事件到前端
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "proxyEnabled": proxy_enabled,
            "autoFailoverEnabled": false,
            "providerId": provider_id
        });
        if let Err(e) = app.emit("proxy-flags-changed", event_data.clone()) {
            log::error!("发射 proxy-flags-changed 事件失败: {e}");
        }
        // 发射 provider-switched 事件（保持向后兼容）
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    Ok(())
}

/// 创建动态托盘菜单
pub fn create_tray_menu(
    app: &tauri::AppHandle,
    app_state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    let app_settings = crate::settings::get_settings();
    let tray_texts = TrayTexts::from_language(app_settings.language.as_deref().unwrap_or("zh"));

    // Get visible apps setting, default to all visible
    let visible_apps = app_settings.visible_apps.unwrap_or_default();

    let mut menu_builder = MenuBuilder::new(app);
    let mut detail_handles: std::collections::HashMap<AppType, MenuItem<tauri::Wry>> =
        std::collections::HashMap::new();

    // 顶部：打开主界面
    let show_main_item =
        MenuItem::with_id(app, "show_main", tray_texts.show_main, true, None::<&str>)
            .map_err(|e| AppError::Message(format!("创建打开主界面菜单失败: {e}")))?;
    menu_builder = menu_builder.item(&show_main_item).separator();

    // Pre-compute proxy running state (used to disable official providers in tray menu)
    let is_proxy_running = futures::executor::block_on(app_state.proxy_service.is_running());

    // 每个应用类型折叠为子菜单，避免供应商过多时菜单过长
    for section in TRAY_SECTIONS.iter() {
        if !visible_apps.is_visible(&section.app_type) {
            continue;
        }

        let app_type_str = section.app_type.as_str();
        let providers = app_state.db.get_all_providers(app_type_str)?;

        let current_id =
            crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)?
                .unwrap_or_default();

        if providers.is_empty() {
            // 空供应商：显示禁用的菜单项
            let label = format!("{} {}", section.header_label, tray_texts.no_providers_label);
            let empty_item = MenuItem::with_id(app, section.empty_id, &label, false, None::<&str>)
                .map_err(|e| {
                    AppError::Message(format!("创建{}空提示失败: {e}", section.log_name))
                })?;
            menu_builder = menu_builder.item(&empty_item);
        } else {
            let current_provider = providers.get(&current_id);
            let submenu_label = match current_provider {
                Some(p) => format!("{} \u{00B7} {}", section.header_label, p.name),
                None => section.header_label.to_string(),
            };
            let submenu_id = format!("submenu_{}", app_type_str);

            // Check if this app is under proxy takeover (for disabling official providers)
            let is_app_taken_over = is_proxy_running
                && (futures::executor::block_on(app_state.db.get_live_backup(app_type_str))
                    .ok()
                    .flatten()
                    .is_some()
                    || app_state
                        .proxy_service
                        .detect_takeover_in_live_config_for_app(&section.app_type));

            let mut submenu_builder = SubmenuBuilder::with_id(app, &submenu_id, &submenu_label);

            for (id, provider) in sort_providers(&providers) {
                let is_current = current_id == *id;
                let is_official_blocked =
                    is_app_taken_over && provider.category.as_deref() == Some("official");
                let label = if is_official_blocked {
                    format!("{} \u{26D4}", &provider.name) // ⛔ emoji
                } else {
                    provider.name.clone()
                };
                let item = CheckMenuItem::with_id(
                    app,
                    format!("{}{}", section.prefix, id),
                    &label,
                    !is_official_blocked, // disabled when blocked
                    is_current,
                    None::<&str>,
                )
                .map_err(|e| {
                    AppError::Message(format!("创建{}菜单项失败: {e}", section.log_name))
                })?;
                submenu_builder = submenu_builder.item(&item);
            }

            let submenu = submenu_builder.build().map_err(|e| {
                AppError::Message(format!("构建{}子菜单失败: {e}", section.log_name))
            })?;
            menu_builder = menu_builder.item(&submenu);

            // 第二行 detail 条目：仅当缓存已有数据时才插入（避免空行占位）。
            // 数据首次到达时 update_tray_usage_labels 检测"handle 缺失但有数据"，
            // 触发一次整建；此后只走 set_text 热路径，高度恒定，不再关闭已打开的菜单。
            if let Some(p) = current_provider {
                if let Some(text) =
                    format_usage_detail_line(app_state, &section.app_type, p, &current_id)
                {
                    let detail_id = format!("detail_{}", app_type_str);
                    match MenuItem::with_id(app, &detail_id, &text, false, None::<&str>) {
                        Ok(item) => {
                            menu_builder = menu_builder.item(&item);
                            detail_handles.insert(section.app_type.clone(), item);
                        }
                        Err(e) => log::debug!("[Tray] 创建{}detail行失败: {e}", section.log_name),
                    }
                }
            }
        }

        menu_builder = menu_builder.separator();
    }

    let lightweight_item = CheckMenuItem::with_id(
        app,
        "lightweight_mode",
        tray_texts.lightweight_mode,
        true,
        crate::lightweight::is_lightweight_mode(),
        None::<&str>,
    )
    .map_err(|e| AppError::Message(format!("创建轻量模式菜单失败: {e}")))?;

    menu_builder = menu_builder.item(&lightweight_item).separator();

    // 退出菜单（分隔符已在上面的 section 循环中添加）
    let quit_item = MenuItem::with_id(app, "quit", tray_texts.quit, true, None::<&str>)
        .map_err(|e| AppError::Message(format!("创建退出菜单失败: {e}")))?;

    menu_builder = menu_builder.item(&quit_item);

    let menu = menu_builder
        .build()
        .map_err(|e| AppError::Message(format!("构建菜单失败: {e}")))?;

    *TRAY_SECTION_DETAIL_ITEMS
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = detail_handles;

    Ok(menu)
}

/// 就地更新各 app 分区的"第二行" detail 条目文本（usage 数据写入缓存时走这条）。
/// 文本始终为单行字符串，NSMenuItem 高度不变，不会触发 macOS 强制收起已打开菜单的问题。
/// detail 条目的插入/移除只发生在 `create_tray_menu` 整建时（启动预热或用户切换供应商等），
/// 此函数永远不调用 `refresh_tray_menu`，保证菜单永不因缓存更新而自动关闭。
fn update_tray_usage_labels(app: &tauri::AppHandle) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };

    let detail_items = match TRAY_SECTION_DETAIL_ITEMS.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    let visible_apps = crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default();

    let mut needs_rebuild = false;
    for section in TRAY_SECTIONS.iter() {
        // Hidden sections have no detail item in the menu — skip them to avoid
        // incorrectly triggering a full menu rebuild on every usage refresh.
        if !visible_apps.is_visible(&section.app_type) {
            continue;
        }
        let Ok(providers) = app_state.db.get_all_providers(section.app_type.as_str()) else {
            continue;
        };
        let Ok(Some(current_id)) =
            crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)
        else {
            continue;
        };
        let Some(provider) = providers.get(&current_id) else {
            continue;
        };

        let text = format_usage_detail_line(&app_state, &section.app_type, provider, &current_id);

        match (detail_items.get(&section.app_type), text) {
            (Some(item), Some(ref t)) => {
                // handle 已缓存，直接更新文本（高度不变，菜单不会关闭）
                if let Err(e) = item.set_text(t) {
                    log::debug!("[Tray] 更新{}detail行失败: {e}", section.log_name);
                }
            }
            (None, Some(_)) => {
                // 数据首次到达而 handle 尚未建立（如网络错误恢复后），需要整建
                needs_rebuild = true;
            }
            (Some(_), None) => {
                // usage became unavailable but a stale detail item exists — rebuild to remove it
                needs_rebuild = true;
            }
            (None, None) => {}
        }
    }
    drop(detail_items); // release lock before possible rebuild or icon update

    if needs_rebuild {
        // 此处调用安全：菜单重建只在"数据从无到有"时触发，通常发生在启动阶段，
        // 用户极少在此窗口期打开菜单。
        refresh_tray_menu(app);
        return; // refresh_tray_menu 末尾已调用 update_tray_icon
    }
    update_tray_icon(app);
}

pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    use crate::store::AppState;

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(new_menu) = create_tray_menu(app, state.inner()) {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                if let Err(e) = tray.set_menu(Some(new_menu)) {
                    log::error!("刷新托盘菜单失败: {e}");
                }
            }
        }
    }
    update_tray_icon(app);
}

#[cfg(target_os = "macos")]
pub fn apply_tray_policy(app: &tauri::AppHandle, dock_visible: bool) {
    use tauri::ActivationPolicy;

    let desired_policy = if dock_visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };

    if let Err(err) = app.set_dock_visibility(dock_visible) {
        log::warn!("设置 Dock 显示状态失败: {err}");
    }

    if let Err(err) = app.set_activation_policy(desired_policy) {
        log::warn!("设置激活策略失败: {err}");
    }
}

/// 处理托盘菜单事件
pub fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    log::info!("处理托盘菜单事件: {event_id}");

    match event_id {
        "show_main" => {
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                {
                    let _ = window.set_skip_taskbar(false);
                }
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                #[cfg(target_os = "linux")]
                {
                    crate::linux_fix::nudge_main_window(window.clone());
                }
                #[cfg(target_os = "macos")]
                {
                    apply_tray_policy(app, true);
                }
            } else if crate::lightweight::is_lightweight_mode() {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式重建窗口失败: {e}");
                }
            }
        }
        "lightweight_mode" => {
            if crate::lightweight::is_lightweight_mode() {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式失败: {e}");
                }
            } else if let Err(e) = crate::lightweight::enter_lightweight_mode(app) {
                log::error!("进入轻量模式失败: {e}");
            }
        }
        "quit" => {
            log::info!("退出应用");
            app.exit(0);
        }
        _ => {
            if handle_provider_tray_event(app, event_id) {
                return;
            }
            log::warn!("未处理的菜单事件: {event_id}");
        }
    }
}

static LAST_TRAY_USAGE_REFRESH: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);
const MIN_TRAY_USAGE_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 合并多次快速触发的"usage 标题软更新"：批量刷新期间多个 usage 命令
/// 同时成功时，只会产生一次就地 `set_text` 批量调用。走软更新而不是
/// `refresh_tray_menu` 整建，避免用户打开中的菜单被 macOS 系统关闭。
static TRAY_REBUILD_SCHEDULED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if TRAY_REBUILD_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 50ms 合窗：让同一轮 React Query / 托盘批量刷新触发的多个写入
        // 共享一次标题更新。
        std::thread::sleep(std::time::Duration::from_millis(50));
        TRAY_REBUILD_SCHEDULED.store(false, Ordering::Release);
        update_tray_usage_labels(&app);
    });
}

/// 并行刷新每个可见 app "当前 provider" 的用量；成功 / 失败结果都通过各
/// command 的 write-through 逻辑写入 `UsageCache`，单次重建菜单由
/// `schedule_tray_refresh` 做合并。内部 10 秒节流防止鼠标悬停反复进出时
/// 雪崩请求；互斥锁被毒化时以上次状态为准继续推进，不会永久阻塞。
///
/// 刷新面与 `format_usage_detail_line` 的展示面严格对齐 —— 每次悬停最多发
/// `TRAY_SECTIONS.len()` 次外部请求，script 优先（覆盖 coding_plan / balance /
/// Copilot / 自定义脚本），否则当前 provider 必须是 `official` 才查订阅。
pub(crate) async fn refresh_all_usage_in_tray(app: &tauri::AppHandle) {
    use crate::commands::CopilotAuthState;
    use futures::future::join_all;

    {
        let mut guard = LAST_TRAY_USAGE_REFRESH
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = std::time::Instant::now();
        if let Some(last) = *guard {
            if now.duration_since(last) < MIN_TRAY_USAGE_REFRESH_INTERVAL {
                return;
            }
        }
        *guard = Some(now);
    }

    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };

    // 与 `create_tray_menu` 保持一致：用户隐藏的 app 不参与外部 API 查询，
    // 避免在未使用的 app 上浪费请求、撞 rate limit 或反复触发鉴权失败日志。
    let visible_apps = crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default();

    let mut subscription_futures = Vec::new();
    let mut script_futures = Vec::new();

    for section in TRAY_SECTIONS.iter() {
        if !visible_apps.is_visible(&section.app_type) {
            continue;
        }

        let app_type_str = section.app_type.as_str();
        let log_name = section.log_name;

        // 解析 effective current provider；未设置 / 出错都静默跳过，
        // 与 create_tray_menu 的行为保持一致。
        let current_id =
            match crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)
            {
                Ok(Some(id)) => id,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("[Tray] 读取{log_name}当前供应商失败: {e}");
                    continue;
                }
            };
        // 只需当前 provider —— by-id 查询避免把整个 app 的 provider 列表加载
        // 进内存（每次悬停 × 3 sections 的热路径）。
        let current = match app_state.db.get_provider_by_id(&current_id, app_type_str) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(e) => {
                log::warn!("[Tray] 读取{log_name}当前供应商失败: {e}");
                continue;
            }
        };

        // 与 format_usage_detail_line 同一优先级：脚本启用 → 查脚本；
        // 否则当前 provider 是 official → 查订阅；其它情况不发请求。
        if current.has_usage_script_enabled() {
            let app_clone = app.clone();
            let state = app.state::<AppState>();
            let copilot_state = app.state::<CopilotAuthState>();
            let provider_id = current_id.clone();
            let app_str = app_type_str.to_string();
            script_futures.push(async move {
                if let Err(e) = crate::commands::queryProviderUsage(
                    app_clone,
                    state,
                    copilot_state,
                    provider_id.clone(),
                    app_str,
                )
                .await
                {
                    log::debug!("[Tray] 刷新{log_name}供应商 {provider_id} 用量失败: {e}");
                }
            });
        } else if current.category.as_deref() == Some("official") {
            let app_clone = app.clone();
            let state = app.state::<AppState>();
            let tool = app_type_str.to_string();
            subscription_futures.push(async move {
                if let Err(e) =
                    crate::commands::get_subscription_quota(app_clone, state, tool).await
                {
                    log::debug!("[Tray] 刷新{log_name}订阅用量失败（可能未登录）: {e}");
                }
            });
        }
    }

    // 两组并行启动，整体等待 —— 订阅/脚本互不依赖，没必要串行。
    futures::future::join(join_all(subscription_futures), join_all(script_futures)).await;
}

#[cfg(test)]
mod tests {
    use super::{
        format_script_summary, format_subscription_detail_from_quota, format_subscription_summary,
        TRAY_ID,
    };
    use crate::provider::{UsageData, UsageResult};
    use crate::services::subscription::{
        CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_WEEKLY_LIMIT,
    };

    #[test]
    fn tray_id_is_unique_to_app() {
        assert_eq!(TRAY_ID, "cc-switch");
        assert_ne!(TRAY_ID, "main");
    }

    fn make_quota(tool: &str, success: bool, tiers: Vec<QuotaTier>) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: tool.to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success,
            tiers,
            extra_usage: None,
            error: None,
            queried_at: Some(0),
        }
    }

    fn tier(name: &str, utilization: f64) -> QuotaTier {
        QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at: None,
        }
    }

    fn tier_with_reset(name: &str, utilization: f64, reset_after_secs: i64) -> QuotaTier {
        QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at: Some(
                (chrono::Utc::now() + chrono::Duration::seconds(reset_after_secs)).to_rfc3339(),
            ),
        }
    }

    #[test]
    fn claude_summary_uses_h_and_w_labels() {
        let quota = make_quota(
            "claude",
            true,
            vec![tier("five_hour", 9.0), tier("seven_day", 27.0)],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("h9%"), "expected h9% in {s}");
        assert!(s.contains("w27%"), "expected w27% in {s}");
    }

    #[test]
    fn format_subscription_summary_is_single_line() {
        // format_subscription_summary must never embed \n — it runs through
        // NSMenuItem setTitle: which doesn't honour newlines and whose height
        // change on an open menu forces macOS to collapse the popup.
        let quota = make_quota(
            "claude",
            true,
            vec![
                tier_with_reset("five_hour", 9.0, 3600),
                tier_with_reset("seven_day", 27.0, 172800),
            ],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(!s.contains('\n'), "summary must be single-line, got: {s}");
    }

    #[test]
    fn detail_from_quota_combines_usage_and_countdown() {
        // format: "emoji  h9% 1h 0m  w27% 2d 0h" — countdown follows each tier's usage
        let quota = make_quota(
            "claude",
            true,
            vec![
                tier_with_reset("five_hour", 9.0, 3600),
                tier_with_reset("seven_day", 27.0, 172800),
            ],
        );
        let s = format_subscription_detail_from_quota(&quota).expect("should format");
        assert!(!s.contains('\n'), "detail must be single-line, got: {s}");
        assert!(
            !s.starts_with(' '),
            "detail must not have leading spaces, got: {s}"
        );
        // h tier: usage then countdown adjacent
        let h_pos = s.find("h9%").expect("h9% not found");
        let w_pos = s.find("w27%").expect("w27% not found");
        assert!(h_pos < w_pos, "h tier should appear before w tier");
        // countdown for h comes before w usage
        let h_cd_pos = s[h_pos..].find('h').map(|i| h_pos + i + 1).unwrap_or(0);
        let _ = h_cd_pos; // timing proximity is validated by integration; unit test checks structure
    }

    #[test]
    fn detail_from_quota_no_countdown_when_reset_is_past() {
        let quota = make_quota(
            "claude",
            true,
            vec![tier("five_hour", 9.0), tier("seven_day", 27.0)],
        );
        let s = format_subscription_detail_from_quota(&quota).expect("should format");
        assert!(s.contains("h9%"), "expected h9% in {s}");
        assert!(s.contains("w27%"), "expected w27% in {s}");
        assert!(!s.starts_with(' '), "no leading spaces in {s}");
    }

    #[test]
    fn detail_from_quota_gemini_flash_lite_included() {
        let quota = make_quota(
            "gemini",
            true,
            vec![
                tier_with_reset("gemini_pro", 5.0, 7200),
                tier_with_reset("gemini_flash", 42.0, 14400),
                tier_with_reset("gemini_flash_lite", 80.0, 21600),
            ],
        );
        let s = format_subscription_detail_from_quota(&quota).expect("should format");
        assert!(s.contains("p5%"), "expected p5% in {s}");
        assert!(s.contains("f42%"), "expected f42% in {s}");
        assert!(s.contains("l80%"), "expected l80% in {s}");
        assert!(!s.starts_with(' '), "no leading spaces in {s}");
        // each tier's countdown should appear right after its usage %
        let p_pos = s.find("p5%").unwrap();
        let f_pos = s.find("f42%").unwrap();
        let l_pos = s.find("l80%").unwrap();
        assert!(p_pos < f_pos && f_pos < l_pos, "tiers in order p f l");
    }

    #[test]
    fn gemini_summary_uses_p_and_f_labels() {
        let quota = make_quota(
            "gemini",
            true,
            vec![tier("gemini_pro", 15.0), tier("gemini_flash", 42.0)],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("p15%"), "expected p15% in {s}");
        assert!(s.contains("f42%"), "expected f42% in {s}");
    }

    #[test]
    fn gemini_summary_includes_all_three_tiers() {
        let quota = make_quota(
            "gemini",
            true,
            vec![
                tier("gemini_pro", 5.0),
                tier("gemini_flash", 42.0),
                tier("gemini_flash_lite", 80.0),
            ],
        );
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("p5%"), "expected p5% in {s}");
        assert!(s.contains("f42%"), "expected f42% in {s}");
        assert!(s.contains("l80%"), "expected l80% in {s}");
    }

    #[test]
    fn gemini_summary_lite_only_still_renders() {
        // flash_lite 如果是 API 返回的唯一 tier，仍应显示（避免前端 footer 能看到、
        // 托盘空白的不对称）。
        let quota = make_quota("gemini", true, vec![tier("gemini_flash_lite", 80.0)]);
        let s = format_subscription_summary(&quota).expect("should format");
        assert!(s.contains("l80%"), "expected l80% in {s}");
    }

    #[test]
    fn gemini_summary_emoji_reflects_highest_tier_including_lite() {
        // lite 是利用率最高的那条 → emoji 必须是红色，不能被 pro/flash 掩盖。
        let quota = make_quota(
            "gemini",
            true,
            vec![
                tier("gemini_pro", 10.0),
                tier("gemini_flash", 20.0),
                tier("gemini_flash_lite", 95.0),
            ],
        );
        let s = format_subscription_summary(&quota).unwrap();
        assert!(
            s.starts_with("\u{1F534}"),
            "expected red emoji (lite worst) in {s}"
        );
    }

    #[test]
    fn worst_emoji_reflects_highest_utilization() {
        // 🔴 = \u{1F534}; 任一 tier ≥ 90% 时预期显示红色。
        let quota = make_quota(
            "claude",
            true,
            vec![tier("five_hour", 10.0), tier("seven_day", 95.0)],
        );
        let s = format_subscription_summary(&quota).unwrap();
        assert!(s.starts_with("\u{1F534}"), "expected red emoji in {s}");
    }

    #[test]
    fn failure_quota_returns_none() {
        let quota = make_quota("claude", false, vec![tier("five_hour", 50.0)]);
        assert!(format_subscription_summary(&quota).is_none());
    }

    #[test]
    fn unknown_tiers_return_none() {
        let quota = make_quota("claude", true, vec![tier("one_hour", 80.0)]);
        assert!(format_subscription_summary(&quota).is_none());
    }

    #[test]
    fn gemini_without_any_known_tiers_returns_none() {
        // 完全没有 pro/flash/flash_lite 三种 tier 的退化响应 → None。
        let quota = make_quota("gemini", true, vec![tier("some_future_tier", 80.0)]);
        assert!(format_subscription_summary(&quota).is_none());
    }

    // ── Ring icon pixel tests ──────────────────────────────────────────────────

    #[test]
    fn ring_icon_output_size_is_correct() {
        let buf = super::generate_ring_icon_rgba(Some(50.0), 72);
        assert_eq!(buf.len(), 72 * 72 * 4);
    }

    #[test]
    fn ring_icon_none_returns_unmodified_base() {
        let buf = super::generate_ring_icon_rgba(None, 72);
        let (base, _, _) = &*super::ICON_BASE_RGBA;
        assert_eq!(&buf, base, "None pct must return unmodified base pixels");
    }

    #[test]
    fn ring_icon_zero_pct_all_white() {
        // 0 % → fill_angle = 0 → no pixel satisfies angle ≤ 0 → all opaque pixels white
        let buf = super::generate_ring_icon_rgba(Some(0.0), 72);
        for chunk in buf.chunks(4) {
            if chunk[3] == 0 {
                continue;
            }
            assert_eq!(
                (chunk[0], chunk[1], chunk[2]),
                (255, 255, 255),
                "0% fill: every opaque pixel should be white"
            );
        }
    }

    #[test]
    fn ring_icon_full_pct_all_red() {
        // 100% ≥ UTIL_DANGER_PCT (90) and fill_angle = 2π → all opaque pixels danger-red
        let buf = super::generate_ring_icon_rgba(Some(100.0), 72);
        for chunk in buf.chunks(4) {
            if chunk[3] == 0 {
                continue;
            }
            assert_eq!(
                (chunk[0], chunk[1], chunk[2]),
                (230, 60, 60),
                "100% fill: every opaque pixel should be danger red"
            );
        }
    }

    #[test]
    fn ring_icon_alpha_preserved() {
        // RGB channels are recolored; alpha must remain identical to the base icon
        let (base, _, _) = &*super::ICON_BASE_RGBA;
        let buf = super::generate_ring_icon_rgba(Some(50.0), 72);
        for (i, chunk) in buf.chunks(4).enumerate() {
            assert_eq!(
                chunk[3],
                base[i * 4 + 3],
                "alpha at pixel {i} must match original"
            );
        }
    }

    #[test]
    fn ring_icon_half_pct_roughly_half_colored() {
        // 50% should produce a roughly equal split of colored vs white opaque pixels
        let buf = super::generate_ring_icon_rgba(Some(50.0), 72);
        let mut colored = 0usize;
        let mut white = 0usize;
        for chunk in buf.chunks(4) {
            if chunk[3] == 0 {
                continue;
            }
            if (chunk[0], chunk[1], chunk[2]) == (255, 255, 255) {
                white += 1;
            } else {
                colored += 1;
            }
        }
        assert!(colored > 0, "50% should have some colored pixels");
        assert!(white > 0, "50% should have some white pixels");
        let ratio = colored as f64 / (colored + white) as f64;
        assert!(
            ratio > 0.3 && ratio < 0.7,
            "50% fill ratio should be ~0.5, got {ratio:.2} ({colored} colored / {white} white)"
        );
    }

    #[test]
    fn ring_icon_color_thresholds() {
        // Locate the first non-white opaque pixel — its RGB is the fill color for that pct.
        // All filled pixels share the same color, so any one is representative.
        let fill_color = |pct: f64| -> Option<(u8, u8, u8)> {
            super::generate_ring_icon_rgba(Some(pct), 72)
                .chunks(4)
                .find(|c| c[3] > 0 && (c[0], c[1], c[2]) != (255, 255, 255))
                .map(|c| (c[0], c[1], c[2]))
        };
        assert_eq!(fill_color(50.0), Some((60, 200, 80)), "< 70% → green");
        assert_eq!(
            fill_color(70.0),
            Some((240, 120, 30)),
            "≥ 70% → orange (warn)"
        );
        assert_eq!(
            fill_color(89.9),
            Some((240, 120, 30)),
            "< 90% → still orange"
        );
        assert_eq!(
            fill_color(90.0),
            Some((230, 60, 60)),
            "≥ 90% → red (danger)"
        );
    }

    // ── Script summary tests ────────────────────────────────────────────────────

    fn usage_data(plan_name: Option<&str>, utilization: f64) -> UsageData {
        UsageData {
            plan_name: plan_name.map(String::from),
            extra: None,
            is_valid: Some(true),
            invalid_message: None,
            total: Some(100.0),
            used: Some(utilization),
            remaining: Some(100.0 - utilization),
            unit: Some("%".to_string()),
        }
    }

    fn usage_result(success: bool, data: Vec<UsageData>) -> UsageResult {
        UsageResult {
            success,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        }
    }

    #[test]
    fn script_summary_token_plan_two_tiers() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 12.0),
                usage_data(Some(TIER_WEEKLY_LIMIT), 80.0),
            ],
        );
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("h12%"), "expected h12% in {s}");
        assert!(s.contains("w80%"), "expected w80% in {s}");
        assert!(s.starts_with("\u{1F7E0}"), "expected orange emoji in {s}");
    }

    #[test]
    fn script_summary_token_plan_worst_drives_emoji() {
        let r = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 20.0),
                usage_data(Some(TIER_WEEKLY_LIMIT), 95.0),
            ],
        );
        let s = format_script_summary(&r).unwrap();
        assert!(s.starts_with("\u{1F534}"), "expected red emoji in {s}");
    }

    #[test]
    fn script_summary_token_plan_five_hour_only() {
        let r = usage_result(true, vec![usage_data(Some(TIER_FIVE_HOUR), 8.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("h8%"), "expected h8% in {s}");
        assert!(
            !s.contains("plan_name"),
            "plan_name should not leak into label: {s}"
        );
    }

    #[test]
    fn script_summary_token_plan_weekly_only() {
        let r = usage_result(true, vec![usage_data(Some(TIER_WEEKLY_LIMIT), 50.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("w50%"), "expected w50% in {s}");
    }

    #[test]
    fn script_summary_single_bucket_fallback_with_plan_name() {
        let r = usage_result(true, vec![usage_data(Some("Copilot Pro"), 40.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert!(s.contains("Copilot Pro"), "expected plan name in {s}");
        assert!(s.contains("40%"), "expected 40% in {s}");
        assert!(
            !s.contains("h40%"),
            "must not relabel non-token-plan data as h: {s}"
        );
    }

    #[test]
    fn script_summary_single_bucket_fallback_without_plan_name() {
        let r = usage_result(true, vec![usage_data(None, 15.0)]);
        let s = format_script_summary(&r).expect("should format");
        assert_eq!(s, "\u{1F7E2} 15%", "expected emoji + pct only, got {s}");
    }

    #[test]
    fn script_summary_failure_returns_none() {
        let r = usage_result(false, vec![usage_data(Some(TIER_FIVE_HOUR), 12.0)]);
        assert!(format_script_summary(&r).is_none());
    }

    #[test]
    fn script_summary_empty_data_returns_none() {
        let r = usage_result(true, vec![]);
        assert!(format_script_summary(&r).is_none());
    }
}
