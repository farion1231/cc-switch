//! 托盘面板：点托盘图标（左右键都一样）在图标旁弹出一块玻璃面板，列出各应用和在用的供应商；
//! 悬停 / 点一个应用，在旁边展开它的供应商子面板，可搜索、切换。
//!
//! - macOS：窗口透明，按前端回报的卡片位置在网页下面垫原生玻璃——macOS 26 用
//!   `NSGlassEffectView`（Liquid Glass），更早的系统用 `NSVisualEffectView` 的 Popover 材质。
//!   两块卡片各垫一块，中间留缝。
//! - Windows：固定透明画布，网页只为卡片绘制不透明背景，两块卡片并成两栏；
//!   原生窗口区域跟随卡片裁剪，画布其余部分透明且不拦截鼠标。
//! - Linux：AppIndicator 不派发点击事件，点图标仍是原生菜单，菜单最上面一项打开面板；没有原生模糊，
//!   网页画兼容的毛玻璃。
//!
//! 面板只换了外观：每一行沿用原生菜单的事件 id，点击走 `tray::handle_tray_menu_event`，
//! 切换、路由、故障转移的行为和原生菜单一致。窗口懒创建，之后只隐藏不销毁；失焦即隐藏。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::store::AppState;

pub const PANEL_LABEL: &str = "tray-panel";
/// Linux 原生菜单里「打开快捷面板」的事件 id。
pub const OPEN_PANEL_ID: &str = "panel:open";
/// Linux 点图标只能出原生菜单；其它平台不挂菜单，点图标出面板。
pub const USES_NATIVE_MENU: bool = cfg!(target_os = "linux");

/// 弹出前发给面板：带上这次的方位，前端据此排版后回报尺寸（`tray_panel_layout`）。
const SHOW_EVENT: &str = "tray-panel-show";
/// 托盘状态变了（切换、额度、问题区），面板重新取数据。
const CHANGED_EVENT: &str = "tray-panel-changed";

const MAIN_WIDTH: f64 = 320.0;
const SUB_WIDTH: f64 = 300.0;
/// 面板离托盘图标的距离、离屏幕边缘至少留的距离（逻辑像素）。
const ICON_GAP: f64 = 6.0;
const SCREEN_MARGIN: f64 = 8.0;
/// 前端没在这段时间内回报尺寸时，按上次的尺寸显示；Windows 必须先完成区域裁剪。
const REVEAL_FALLBACK: Duration = Duration::from_millis(600);
/// 点托盘图标关面板时，失焦隐藏先于点击事件到达；这段时间内的点击当作「关」，不再弹出。
const BLUR_CLICK_GRACE: Duration = Duration::from_millis(300);

#[cfg(target_os = "macos")]
mod style {
    pub const LAYOUT: &str = "floating";
    pub const GLASS: &str = "native";
    pub const CARD_GAP: f64 = 8.0;
    pub const RADIUS: f64 = 14.0;
}
#[cfg(target_os = "windows")]
mod style {
    pub const LAYOUT: &str = "joined";
    pub const GLASS: &str = "frosted";
    pub const CARD_GAP: f64 = 0.0;
    pub const RADIUS: f64 = 8.0;
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod style {
    pub const LAYOUT: &str = "joined";
    /// 没有原生模糊：网页自己画兼容的毛玻璃
    pub const GLASS: &str = "frosted";
    pub const CARD_GAP: f64 = 0.0;
    pub const RADIUS: f64 = 12.0;
}

// ─── 文案 ────────────────────────────────────────────────────────────────────

/// 面板自己的文案（和托盘菜单同一语言）；随数据一起发给前端，面板不加载界面的 i18n。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelTexts {
    pub switch_provider: &'static str,
    pub search_placeholder: &'static str,
    pub in_use: &'static str,
    pub no_match: &'static str,
    pub open_main: &'static str,
    pub quit: &'static str,
    pub settings: &'static str,
    pub lightweight_mode: &'static str,
    pub open_panel: &'static str,
    pub config_tab: &'static str,
    pub usage_tab: &'static str,
    /// 某一档的重置时间：`{when}`
    pub resets_at: &'static str,
    /// 可用的额度重置次数：`{count}`
    pub reset_credits: &'static str,
}

impl PanelTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" | "tr" => Self {
                switch_provider: "Switch provider",
                search_placeholder: "Search providers…",
                in_use: "In use",
                no_match: "No matching providers",
                open_main: "Open main window",
                quit: "Quit",
                settings: "Settings",
                lightweight_mode: if language == "tr" {
                    "Hafif mod"
                } else {
                    "Lightweight mode"
                },
                open_panel: "Open quick panel",
                config_tab: "Configuration",
                usage_tab: "Usage",
                resets_at: "Resets {when}",
                reset_credits: "{count} resets available",
            },
            "ja" => Self {
                switch_provider: "プロバイダーを切り替え",
                search_placeholder: "プロバイダーを検索…",
                in_use: "使用中",
                no_match: "一致するプロバイダーはありません",
                open_main: "メインウィンドウを開く",
                quit: "終了",
                settings: "設定",
                lightweight_mode: "軽量モード",
                open_panel: "クイックパネルを開く",
                config_tab: "構成",
                usage_tab: "使用量",
                resets_at: "{when} にリセット",
                reset_credits: "リセット可能 {count} 回",
            },
            "zh-TW" => Self {
                switch_provider: "切換供應商",
                search_placeholder: "搜尋供應商…",
                in_use: "使用中",
                no_match: "沒有符合的供應商",
                open_main: "開啟主視窗",
                quit: "結束",
                settings: "設定",
                lightweight_mode: "輕量模式",
                open_panel: "開啟快捷面板",
                config_tab: "配置",
                usage_tab: "用量",
                resets_at: "{when} 重置",
                reset_credits: "可重置 {count} 次",
            },
            _ => Self {
                switch_provider: "切换供应商",
                search_placeholder: "搜索供应商…",
                in_use: "使用中",
                no_match: "没有匹配的供应商",
                open_main: "打开主窗口",
                quit: "退出",
                settings: "设置",
                lightweight_mode: "轻量模式",
                open_panel: "打开快捷面板",
                config_tab: "配置",
                usage_tab: "用量",
                resets_at: "{when} 重置",
                reset_credits: "可重置 {count} 次",
            },
        }
    }

    pub fn current() -> Self {
        Self::from_language(&crate::tray::tray_language())
    }
}

// ─── 方位 ────────────────────────────────────────────────────────────────────

/// 这次弹出的方位，发给前端排版。坐标都是逻辑像素。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPlacement {
    /// 每次弹出加一；前端回报尺寸时带回来，过期的回报不用。
    pub seq: u64,
    /// 窗口平台；Windows 使用固定画布配合原生窗口区域，避免展开子面板时调整 WebView 大小。
    pub platform: &'static str,
    /// 子面板在主面板的哪一边：`left` / `right`
    pub side: &'static str,
    /// 主面板哪条边贴着图标：`top`（向下弹出）/ `bottom`（向上弹出）
    pub edge: &'static str,
    /// 工作区上下边界，相对贴图标那条边的纵坐标（向上为负）。
    pub bounds_top: f64,
    pub bounds_bottom: f64,
    pub main_width: f64,
    pub sub_width: f64,
    pub gap: f64,
    pub radius: f64,
    /// `floating`：两块卡片分开、子面板对齐到行；`joined`：并成两栏（整窗一块背景的平台）
    pub layout: &'static str,
    /// `native`：背后有系统玻璃，网页只叠一层淡色；`frosted`：系统没有可用的玻璃材质，
    /// 网页自己画兼容的毛玻璃（半透明底色 + 高光描边）
    pub glass: &'static str,
}

#[derive(Debug, Clone)]
struct Anchor {
    /// 主面板左边、贴图标那条边的屏幕坐标（逻辑像素）。
    x: f64,
    y: f64,
    /// 所在显示器的缩放（Windows / Linux 换回物理像素用）。
    scale: f64,
    placement: PanelPlacement,
}

#[derive(Debug, Clone, Copy)]
struct Area {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

static ANCHOR: Mutex<Option<Anchor>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);
/// 已经要弹出、还在等前端回报尺寸。
static PENDING_REVEAL: AtomicBool = AtomicBool::new(false);
static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);
/// 上次套给窗口的深浅色（内层 `None` = 跟随系统），变了才重设。
static APPLIED_DARK: Mutex<Option<Option<bool>>> = Mutex::new(None);
/// 这次弹出里上次摆的窗口位置和大小；没变就不动窗口，只挪玻璃（展开子面板不跳）。
static APPLIED_FRAME: Mutex<Option<Frame>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// 主面板放在图标旁边：水平居中于图标、不出工作区；图标在屏幕下半（Windows 任务栏）时向上弹出。
/// 子面板默认向右展开，右边放不下就向左。`icon` 和 `work` 是同一显示器上的逻辑坐标。
fn place(icon: (f64, f64, f64, f64), work: Area, seq: u64) -> (f64, f64, PanelPlacement) {
    let (ix, iy, iw, ih) = icon;
    let gap = style::CARD_GAP;
    let min_x = work.left + SCREEN_MARGIN;
    let max_x = (work.right - SCREEN_MARGIN - MAIN_WIDTH).max(min_x);
    let x = (ix + iw / 2.0 - MAIN_WIDTH / 2.0).clamp(min_x, max_x);

    let upward = iy + ih / 2.0 > (work.top + work.bottom) / 2.0;
    let (edge, y) = if upward {
        ("bottom", (iy - ICON_GAP).min(work.bottom))
    } else {
        ("top", (iy + ih + ICON_GAP).max(work.top))
    };

    let room_right = work.right - SCREEN_MARGIN - (x + MAIN_WIDTH + gap);
    let room_left = x - gap - (work.left + SCREEN_MARGIN);
    let side = if room_right >= SUB_WIDTH || room_right >= room_left {
        "right"
    } else {
        "left"
    };

    let placement = PanelPlacement {
        seq,
        platform: std::env::consts::OS,
        side,
        edge,
        bounds_top: work.top + SCREEN_MARGIN - y,
        bounds_bottom: work.bottom - SCREEN_MARGIN - y,
        main_width: MAIN_WIDTH,
        sub_width: SUB_WIDTH,
        gap,
        radius: style::RADIUS,
        layout: style::LAYOUT,
        glass: platform::glass(),
    };
    (x, y, placement)
}

/// 找出点（物理像素）所在的显示器，返回它的工作区（逻辑像素）和缩放。
fn monitor_at(app: &AppHandle, px: f64, py: f64) -> Option<(Area, f64)> {
    let monitors = app.available_monitors().ok()?;
    let to_area = |m: &tauri::Monitor| {
        let scale = m.scale_factor();
        let work = m.work_area();
        let area = Area {
            left: work.position.x as f64 / scale,
            top: work.position.y as f64 / scale,
            right: (work.position.x as f64 + work.size.width as f64) / scale,
            bottom: (work.position.y as f64 + work.size.height as f64) / scale,
        };
        (area, scale)
    };
    monitors
        .iter()
        .find(|m| {
            let (pos, size) = (m.position(), m.size());
            px >= pos.x as f64
                && px < pos.x as f64 + size.width as f64
                && py >= pos.y as f64
                && py < pos.y as f64 + size.height as f64
        })
        .or_else(|| monitors.first())
        .map(to_area)
        .or_else(|| app.primary_monitor().ok().flatten().as_ref().map(to_area))
}

// ─── 窗口 ────────────────────────────────────────────────────────────────────

fn panel_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL_LABEL)
}

fn ensure_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = panel_window(app) {
        return Some(window);
    }
    match create_window(app) {
        Ok(window) => Some(window),
        Err(e) => {
            log::error!("[TrayPanel] 创建面板窗口失败: {e}");
            None
        }
    }
}

fn create_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window =
        WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("tray-panel.html".into()))
            .title("CC Switch")
            .inner_size(MAIN_WIDTH, 420.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            // Windows 使用固定透明画布，整窗系统阴影会暴露画布的外框。
            .shadow(!cfg!(target_os = "windows"))
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            .visible(false)
            .focused(false)
            .build()?;
    platform::init(&window);

    let handle = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(false) if handle.is_visible().unwrap_or(false) => {
            *lock(&LAST_BLUR_HIDE) = Some(Instant::now());
            hide_window(&handle);
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_window(&handle);
        }
        _ => {}
    });
    Ok(window)
}

fn hide_window(window: &WebviewWindow) {
    PENDING_REVEAL.store(false, Ordering::Release);
    let _ = window.hide();
    // 面板里已经显示过反馈行了，下次弹出不再显示。
    crate::tray::mark_feedback_seen();
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(window) = panel_window(app) {
        hide_window(&window);
    }
}

fn reveal(window: &WebviewWindow) {
    if !PENDING_REVEAL.swap(false, Ordering::AcqRel) {
        return;
    }
    let _ = window.show();
    let _ = window.set_focus();
}

/// 在 `icon`（物理像素的图标矩形）旁弹出面板。
fn show_at(app: &AppHandle, window: &WebviewWindow, icon: (f64, f64, f64, f64)) {
    let (px, py, pw, ph) = icon;
    let Some((work, scale)) = monitor_at(app, px + pw / 2.0, py + ph / 2.0) else {
        log::warn!("[TrayPanel] 找不到图标所在的显示器");
        return;
    };
    let seq = SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    let (x, y, placement) = place((px / scale, py / scale, pw / scale, ph / scale), work, seq);
    *lock(&ANCHOR) = Some(Anchor {
        x,
        y,
        scale,
        placement: placement.clone(),
    });
    PENDING_REVEAL.store(true, Ordering::Release);
    *lock(&APPLIED_FRAME) = None;
    let _ = window.emit_to(PANEL_LABEL, SHOW_EVENT, placement);

    let fallback = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REVEAL_FALLBACK).await;
        if SEQ.load(Ordering::Acquire) == seq && PENDING_REVEAL.load(Ordering::Acquire) {
            if cfg!(target_os = "windows") {
                // 固定画布尚未排版和裁剪时不能显示，避免露出尚未准备好的窗口。
                log::debug!("[TrayPanel] 等待 Windows 面板区域就绪");
                return;
            }
            log::debug!("[TrayPanel] 前端没回报尺寸，直接显示");
            reveal(&fallback);
        }
    });

    // 额度在弹出时顺带刷新：在用的那家（内部 10 秒防抖），以及子面板里其余开了自动查询、
    // 缓存里还没有或已过期的脚本供应商；查完经 `notify_changed` 更新面板。
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::tray::refresh_all_usage_in_tray(&app).await;
        crate::tray::refresh_panel_provider_usage(&app).await;
    });
}

/// 点托盘图标：开着就关，关着就在图标旁弹出。`rect` 是点击事件带的图标矩形。
pub fn toggle(app: &AppHandle, rect: &tauri::Rect) {
    let Some(window) = ensure_window(app) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide_window(&window);
        return;
    }
    let just_blurred = lock(&LAST_BLUR_HIDE)
        .take()
        .is_some_and(|at| at.elapsed() < BLUR_CLICK_GRACE);
    if just_blurred {
        return;
    }
    // 托盘矩形本来就是物理像素；万一是逻辑的，按 1 倍算也只是位置略偏。
    let pos = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    show_at(app, &window, (pos.x, pos.y, size.width, size.height));
}

/// Linux 原生菜单里的「打开快捷面板」：没有图标位置，弹在鼠标旁边
/// （Wayland 下窗口位置由合成器决定）。
pub fn show_at_cursor(app: &AppHandle) {
    let Some(window) = ensure_window(app) else {
        return;
    };
    let (x, y) = app
        .cursor_position()
        .map(|p| (p.x, p.y))
        .unwrap_or((0.0, 0.0));
    show_at(app, &window, (x, y, 1.0, 1.0));
}

/// 托盘状态变了：开着的面板重新取数据。
pub fn notify_changed(app: &AppHandle) {
    if panel_window(app).is_some() {
        let _ = app.emit_to(PANEL_LABEL, CHANGED_EVENT, ());
    }
}

// ─── 平台：玻璃和摆放 ─────────────────────────────────────────────────────────

/// 卡片在窗口里的位置（逻辑像素，左上角为原点）。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
// macOS 按卡片位置垫玻璃，Windows 据此裁剪原生窗口区域。
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub struct CardRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 窗口在屏幕上的位置和大小（逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::RefCell;

    use objc2_06::rc::Retained;
    use objc2_06::runtime::{AnyClass, AnyObject, NSObjectProtocol};
    use objc2_06::{msg_send, sel, MainThreadMarker};
    use objc2_app_kit_03::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSGlassEffectView, NSGlassEffectViewStyle, NSScreen, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
        NSWindowOrderingMode,
    };
    use objc2_foundation_03::{NSPoint, NSRect, NSSize};
    use tauri::WebviewWindow;

    use super::{style, CardRect, Frame};

    pub fn glass() -> &'static str {
        style::GLASS
    }

    thread_local! {
        /// 垫在网页下面的玻璃，一块卡片一块（只在主线程上碰）。
        static GLASS: RefCell<Vec<Retained<NSView>>> = const { RefCell::new(Vec::new()) };
    }

    /// 不用系统窗口阴影：透明窗口的阴影按两块卡片的形状算，向下偏移、连成一片，
    /// macOS 26 上还会描一圈深色边。阴影由网页给每块卡片画（窗口四周留了边距）。
    pub fn init(window: &WebviewWindow) {
        let _ = window.set_shadow(false);
    }

    fn ns_window(window: &WebviewWindow) -> Option<&NSWindow> {
        let ptr = window.ns_window().ok()?;
        // SAFETY: 指针来自 tao，窗口存活期间有效；只在主线程上借用，不持有。
        Some(unsafe { &*ptr.cast::<NSWindow>() })
    }

    /// 网页透明，玻璃铺在网页下面：系统有 `NSGlassEffectView`（macOS 26+）就用它，否则用
    /// Popover 材质的毛玻璃。
    fn make_glass(mtm: MainThreadMarker) -> Retained<NSView> {
        if AnyClass::get(c"NSGlassEffectView").is_some() {
            let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
            glass.setStyle(NSGlassEffectViewStyle::Regular);
            glass.setCornerRadius(style::RADIUS);
            return Retained::into_super(glass);
        }
        let effect = NSVisualEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        effect.setMaterial(NSVisualEffectMaterial::Popover);
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        // 失焦也保持毛玻璃（失焦就会隐藏，但避免弹出瞬间闪灰）
        effect.setState(NSVisualEffectState::Active);
        // 和 window-vibrancy 一样用 `setCornerRadius:`；没有这个方法时退回图层圆角。
        if effect.respondsToSelector(sel!(setCornerRadius:)) {
            // SAFETY: 上面确认过能响应，参数是 CGFloat。
            unsafe {
                let _: () = msg_send![&*effect, setCornerRadius: style::RADIUS];
            }
        } else {
            effect.setWantsLayer(true);
            // SAFETY: setWantsLayer 之后 layer 是 CALayer；只设圆角和裁切。
            unsafe {
                let layer: *mut AnyObject = msg_send![&*effect, layer];
                if !layer.is_null() {
                    let _: () = msg_send![layer, setCornerRadius: style::RADIUS];
                    let _: () = msg_send![layer, setMasksToBounds: true];
                }
            }
        }
        Retained::into_super(effect)
    }

    /// 摆好窗口（`frame` 为空 = 窗口不动）和每块玻璃，在同一个主线程回合里做完。
    pub fn apply(window: &WebviewWindow, frame: Option<Frame>, _scale: f64, cards: Vec<CardRect>) {
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let Some(ns_window) = ns_window(&target) else {
                return;
            };
            if let Some(frame) = frame {
                // AppKit 屏幕坐标的原点在主屏左下角。
                let primary_height = NSScreen::screens(mtm)
                    .firstObject()
                    .map_or(0.0, |screen| screen.frame().size.height);
                ns_window.setFrame_display(
                    NSRect::new(
                        NSPoint::new(frame.x, primary_height - frame.y - frame.height),
                        NSSize::new(frame.width, frame.height),
                    ),
                    true,
                );
            }
            let Some(content) = ns_window.contentView() else {
                return;
            };
            let height = content.bounds().size.height;
            let flipped = content.isFlipped();
            GLASS.with(|glass| {
                let mut glass = glass.borrow_mut();
                while glass.len() < cards.len() {
                    let view = make_glass(mtm);
                    // 放在所有子视图（WKWebView）下面
                    content.addSubview_positioned_relativeTo(
                        &view,
                        NSWindowOrderingMode::Below,
                        None,
                    );
                    glass.push(view);
                }
                for (index, view) in glass.iter().enumerate() {
                    match cards.get(index) {
                        Some(card) => {
                            let y = if flipped {
                                card.y
                            } else {
                                height - card.y - card.height
                            };
                            view.setFrame(NSRect::new(
                                NSPoint::new(card.x, y),
                                NSSize::new(card.width, card.height),
                            ));
                            if view.isHidden() {
                                // 刚出现的玻璃（展开子面板）跟着网页一起淡入
                                view.setAlphaValue(0.0);
                                view.setHidden(false);
                                // SAFETY: animator 是 NSView 的动画代理，setAlphaValue: 参数是 CGFloat。
                                unsafe {
                                    let animator: *mut AnyObject = msg_send![&**view, animator];
                                    if !animator.is_null() {
                                        let _: () = msg_send![animator, setAlphaValue: 1.0_f64];
                                    }
                                }
                            }
                        }
                        None => view.setHidden(true),
                    }
                }
            });
        });
    }

    /// 只改面板窗口的深浅色（`set_theme` 在 macOS 上改的是整个应用）；`None` 跟随系统。
    pub fn set_dark(window: &WebviewWindow, dark: Option<bool>) {
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            let Some(ns_window) = ns_window(&target) else {
                return;
            };
            let appearance = dark.and_then(|dark| {
                // SAFETY: 系统常量，进程内一直有效。
                let name = unsafe {
                    if dark {
                        NSAppearanceNameDarkAqua
                    } else {
                        NSAppearanceNameAqua
                    }
                };
                NSAppearance::appearanceNamed(name)
            });
            ns_window.setAppearance(appearance.as_deref());
        });
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use tauri::WebviewWindow;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        ClientToScreen, CreateRoundRectRgn, DeleteObject, SetWindowRgn,
    };
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    use super::{style, CardRect, Frame};

    /// 整窗 Acrylic 的合成背景不遵循 SetWindowRgn，会把预留画布涂成灰色；
    /// Windows 在网页卡片内绘制背景，画布本身保持透明。
    pub fn glass() -> &'static str {
        style::GLASS
    }

    pub fn init(_window: &WebviewWindow) {}

    /// 前端回报的是客户区尺寸；带阴影的无边框窗口仍有非客户区，需补回四边，
    /// 否则 WebView 的右侧和底部会被裁切。坐标也按客户区原点换算，保持卡片贴住图标。
    fn outer_rect(frame: Frame, scale: f64, outer: RECT, client: RECT, origin: POINT) -> RECT {
        let left = origin.x - outer.left;
        let top = origin.y - outer.top;
        let right = outer.right - origin.x - (client.right - client.left);
        let bottom = outer.bottom - origin.y - (client.bottom - client.top);
        let x = (frame.x * scale).round() as i32;
        let y = (frame.y * scale).round() as i32;
        RECT {
            left: x - left,
            top: y - top,
            right: x + (frame.width * scale).round() as i32 + right,
            bottom: y + (frame.height * scale).round() as i32 + bottom,
        }
    }

    fn set_client_frame(hwnd: HWND, frame: Frame, scale: f64) {
        let mut outer = RECT::default();
        let mut client = RECT::default();
        let mut origin = POINT::default();
        // SAFETY: 只在窗口存活时由主线程调用；传入的矩形和坐标指针均有效。
        unsafe {
            if GetWindowRect(hwnd, &mut outer) == 0
                || GetClientRect(hwnd, &mut client) == 0
                || ClientToScreen(hwnd, &mut origin) == 0
            {
                log::warn!("[TrayPanel] 读取 Windows 窗口边距失败");
                return;
            }
            let rect = outer_rect(frame, scale, outer, client, origin);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn card_bounds(cards: &[CardRect]) -> Option<Frame> {
        let first = cards.first()?;
        let mut left = first.x;
        let mut top = first.y;
        let mut right = first.x + first.width;
        let mut bottom = first.y + first.height;
        for card in &cards[1..] {
            left = left.min(card.x);
            top = top.min(card.y);
            right = right.max(card.x + card.width);
            bottom = bottom.max(card.y + card.height);
        }
        Some(Frame {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }

    /// 裁掉固定画布中没有卡片的部分，避免不可见的区域拦截鼠标。
    /// 展开子面板只改 region，不改变 HWND / WebView 尺寸，避免 DWM 重用旧尺寸的画面造成闪烁。
    fn set_card_region(hwnd: HWND, scale: f64, cards: &[CardRect]) -> bool {
        let Some(bounds) = card_bounds(cards) else {
            return false;
        };
        let mut outer = RECT::default();
        let mut origin = POINT::default();
        // SAFETY: 主线程上的存活窗口；region 成功交给系统后由系统管理，失败时自行释放。
        unsafe {
            if GetWindowRect(hwnd, &mut outer) == 0 || ClientToScreen(hwnd, &mut origin) == 0 {
                return false;
            }
            let x = origin.x - outer.left;
            let y = origin.y - outer.top;
            let diameter = (style::RADIUS * 2.0 * scale).round() as i32;
            let region = CreateRoundRectRgn(
                x + (bounds.x * scale).round() as i32,
                y + (bounds.y * scale).round() as i32,
                x + ((bounds.x + bounds.width) * scale).round() as i32 + 1,
                y + ((bounds.y + bounds.height) * scale).round() as i32 + 1,
                diameter,
                diameter,
            );
            if region.is_null() {
                log::warn!("[TrayPanel] 创建 Windows 面板区域失败");
                false
            } else if SetWindowRgn(hwnd, region, 1) == 0 {
                DeleteObject(region);
                log::warn!("[TrayPanel] 设置 Windows 面板区域失败");
                false
            } else {
                true
            }
        }
    }

    /// 首次弹出时一次设好位置和客户区大小，之后只更新卡片的原生可见区域。
    pub fn apply(window: &WebviewWindow, frame: Option<Frame>, scale: f64, cards: Vec<CardRect>) {
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            let Ok(hwnd) = target.hwnd() else {
                return;
            };
            if let Some(frame) = frame {
                // SAFETY: hwnd 来自 tao，窗口存活。
                unsafe {
                    let dpi = GetDpiForWindow(hwnd.0 as _);
                    set_client_frame(hwnd.0 as _, frame, scale);
                    // 跨到不同缩放的显示器时，WM_DPICHANGED 会改变非客户区；按新的边距再校准。
                    if GetDpiForWindow(hwnd.0 as _) != dpi {
                        set_client_frame(hwnd.0 as _, frame, scale);
                    }
                }
            }
            if set_card_region(hwnd.0 as _, scale, &cards) {
                // 先裁剪、再显示；不能在排队的主线程任务执行前露出整个预留画布。
                super::reveal(&target);
            }
        });
    }

    pub fn set_dark(window: &WebviewWindow, dark: Option<bool>) {
        let _ = window.set_theme(dark.map(|dark| {
            if dark {
                tauri::Theme::Dark
            } else {
                tauri::Theme::Light
            }
        }));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_visible_cards_belong_to_the_window_region() {
            let main = CardRect {
                x: 300.0,
                y: 433.0,
                width: 320.0,
                height: 327.0,
            };
            let sub = CardRect {
                x: 0.0,
                width: 300.0,
                ..main
            };
            assert_eq!(card_bounds(&[]), None);
            assert_eq!(
                card_bounds(&[main]),
                Some(Frame {
                    x: 300.0,
                    y: 433.0,
                    width: 320.0,
                    height: 327.0,
                })
            );
            assert_eq!(
                card_bounds(&[main, sub]),
                Some(Frame {
                    x: 0.0,
                    y: 433.0,
                    width: 620.0,
                    height: 327.0,
                })
            );
        }

        #[test]
        fn shadow_borders_do_not_clip_the_webview() {
            // 实际观察到的 Windows 窗口：外框 620×327，客户区仅 604×318。
            let rect = outer_rect(
                Frame {
                    x: 1755.0,
                    y: 1002.0,
                    width: 620.0,
                    height: 327.0,
                },
                1.0,
                RECT {
                    left: 1755,
                    top: 1002,
                    right: 2375,
                    bottom: 1329,
                },
                RECT {
                    left: 0,
                    top: 0,
                    right: 604,
                    bottom: 318,
                },
                POINT { x: 1763, y: 1003 },
            );
            assert_eq!((rect.left, rect.top), (1747, 1001));
            assert_eq!((rect.right, rect.bottom), (2383, 1337));
            assert_eq!(rect.right - rect.left - 16, 620);
            assert_eq!(rect.bottom - rect.top - 9, 327);
        }

        #[test]
        fn scaled_submenu_expansion_keeps_the_client_right_and_bottom_fixed() {
            // 左侧的副屏可以有负坐标；125% / 150% 下也应按客户区锚定，不能把边框重复缩放。
            for (scale, border) in [(1.25, 10), (1.5, 12)] {
                let outer = RECT {
                    left: -1000,
                    top: 100,
                    right: 0,
                    bottom: 800,
                };
                let client = RECT {
                    left: 0,
                    top: 0,
                    right: 1000 - border * 2,
                    bottom: 699 - border,
                };
                let origin = POINT {
                    x: -1000 + border,
                    y: 101,
                };
                let collapsed = outer_rect(
                    Frame {
                        x: -500.0,
                        y: 373.0,
                        width: 320.0,
                        height: 327.0,
                    },
                    scale,
                    outer,
                    client,
                    origin,
                );
                let expanded = outer_rect(
                    Frame {
                        x: -800.0,
                        y: 300.0,
                        width: 620.0,
                        height: 400.0,
                    },
                    scale,
                    outer,
                    client,
                    origin,
                );
                assert_eq!(collapsed.right, expanded.right);
                // 高度分别取整后可相差一个物理像素。
                assert!((collapsed.bottom - expanded.bottom).abs() <= 1);
                assert_eq!(
                    expanded.right - expanded.left - border * 2,
                    (620.0 * scale) as i32
                );
                assert_eq!(
                    expanded.bottom - expanded.top - border - 1,
                    (400.0 * scale) as i32
                );
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

    use super::{style, CardRect, Frame};

    pub fn glass() -> &'static str {
        style::GLASS
    }

    pub fn init(_window: &WebviewWindow) {}

    pub fn apply(window: &WebviewWindow, frame: Option<Frame>, scale: f64, _cards: Vec<CardRect>) {
        let Some(frame) = frame else {
            return;
        };
        let _ = window.set_size(PhysicalSize::new(
            (frame.width * scale).round() as u32,
            (frame.height * scale).round() as u32,
        ));
        let _ = window.set_position(PhysicalPosition::new(
            (frame.x * scale).round() as i32,
            (frame.y * scale).round() as i32,
        ));
    }

    /// GTK 的主题是整个应用的，面板只靠网页自己的配色。
    pub fn set_dark(_window: &WebviewWindow, _dark: Option<bool>) {}
}

// ─── 命令 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayPanelState {
    pub texts: PanelTexts,
    #[serde(flatten)]
    pub model: crate::tray::PanelModel,
}

#[tauri::command]
pub fn tray_panel_state(state: tauri::State<'_, AppState>) -> Result<TrayPanelState, String> {
    Ok(TrayPanelState {
        texts: PanelTexts::current(),
        model: crate::tray::collect_panel_model(state.inner()).map_err(|e| e.to_string())?,
    })
}

/// 面板挂载时取一次：第一次弹出时窗口是新建的，`tray-panel-show` 可能早于前端开始监听。
/// 页面加载慢于 `REVEAL_FALLBACK` 时窗口已经按兜底显示出来（待显示标记已清），这时也要
/// 返回这次的方位，否则面板一直是空的。
#[tauri::command]
pub fn tray_panel_placement(app: AppHandle) -> Option<PanelPlacement> {
    let visible = panel_window(&app).is_some_and(|window| window.is_visible().unwrap_or(false));
    if !PENDING_REVEAL.load(Ordering::Acquire) && !visible {
        return None;
    }
    lock(&ANCHOR)
        .as_ref()
        .map(|anchor| anchor.placement.clone())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelLayout {
    pub seq: u64,
    pub width: f64,
    pub height: f64,
    /// 主面板左边在窗口里的横坐标。
    pub offset_x: f64,
    /// 窗口顶边相对贴图标那条边的纵坐标。
    pub offset_y: f64,
    pub cards: Vec<CardRect>,
    /// 界面主题：深 / 浅；跟随系统时为空。
    pub dark: Option<bool>,
}

/// 前端排好版后回报尺寸：摆好窗口和玻璃；这次弹出还没显示的话随即显示。
#[tauri::command]
pub fn tray_panel_layout(app: AppHandle, layout: PanelLayout) {
    let Some(anchor) = lock(&ANCHOR).clone() else {
        return;
    };
    if layout.seq != anchor.placement.seq || layout.width <= 0.0 || layout.height <= 0.0 {
        return;
    }
    let Some(window) = panel_window(&app) else {
        return;
    };
    {
        let mut applied = lock(&APPLIED_DARK);
        if *applied != Some(layout.dark) {
            *applied = Some(layout.dark);
            platform::set_dark(&window, layout.dark);
        }
    }
    let frame = Frame {
        x: anchor.x - layout.offset_x,
        y: anchor.y + layout.offset_y,
        width: layout.width,
        height: layout.height,
    };
    let move_window = {
        let mut applied = lock(&APPLIED_FRAME);
        let changed = *applied != Some(frame);
        *applied = Some(frame);
        changed
    };
    platform::apply(
        &window,
        move_window.then_some(frame),
        anchor.scale,
        layout.cards,
    );
    #[cfg(not(target_os = "windows"))]
    reveal(&window);
}

#[tauri::command]
pub fn tray_panel_hide(app: AppHandle) {
    hide_panel(&app);
}

/// 面板里点了一行：按原生菜单的事件 id 处理。切换供应商、项目、轻量模式之后面板留着
/// （看得到勾移到哪家），其余（打开页面、退出）先收起面板。
///
/// 必须是 async 命令：轻量模式下打开主窗口要新建窗口，Windows 上在同步命令里
/// `WebviewWindowBuilder::build()` 会死锁（WebView2 限制）。原生菜单的事件回调没有这个问题。
#[tauri::command]
pub async fn tray_panel_action(app: AppHandle, id: String) {
    let stays_open =
        id.starts_with("prov:") || id.starts_with("profile_") || id == "lightweight_mode";
    if !stays_open {
        hide_panel(&app);
    }
    crate::tray::handle_tray_menu_event(&app, &id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area {
        left: 0.0,
        top: 25.0,
        right: 1440.0,
        bottom: 900.0,
    };

    #[test]
    fn menu_bar_icon_opens_downward_and_flips_submenu_left_near_the_right_edge() {
        let (x, y, placement) = place((1300.0, 0.0, 22.0, 24.0), WORK, 1);
        assert_eq!(placement.edge, "top");
        assert_eq!(y, 24.0 + ICON_GAP);
        assert_eq!(x, WORK.right - SCREEN_MARGIN - MAIN_WIDTH);
        assert_eq!(placement.side, "left");
    }

    #[test]
    fn icon_on_the_left_expands_submenu_to_the_right() {
        let (x, _, placement) = place((200.0, 0.0, 22.0, 24.0), WORK, 1);
        assert_eq!(x, 200.0 + 11.0 - MAIN_WIDTH / 2.0);
        assert_eq!(placement.side, "right");
    }

    #[test]
    fn bottom_taskbar_icon_opens_upward() {
        let work = Area {
            left: 0.0,
            top: 0.0,
            right: 1920.0,
            bottom: 1032.0,
        };
        let (_, y, placement) = place((1700.0, 1040.0, 24.0, 40.0), work, 1);
        assert_eq!(placement.edge, "bottom");
        assert_eq!(y, work.bottom);
        assert_eq!(placement.bounds_top, SCREEN_MARGIN - y);
    }
}
