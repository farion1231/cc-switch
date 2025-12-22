//! 托盘菜单管理模块
//!
//! 负责系统托盘图标和菜单的创建、更新和事件处理。

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::{
    App, LogicalPosition, Manager, Monitor, PhysicalPosition, Position, Rect, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

pub const TRAY_WINDOW_LABEL: &str = "tray-popover";
const TRAY_POPUP_WIDTH: f64 = 360.0;
const TRAY_POPUP_HEIGHT: f64 = 680.0;
const TRAY_POPUP_MARGIN: f64 = 10.0;

#[cfg(target_os = "macos")]
fn set_tray_highlight(app: &tauri::AppHandle, highlighted: bool) {
    if let Some(tray_icon) = app.tray_by_id("main") {
        let _ = tray_icon.with_inner_tray_icon(move |inner| {
            if let Some(status_item) = inner.ns_status_item() {
                #[allow(deprecated)]
                unsafe {
                    use objc2::msg_send;
                    use objc2::runtime::AnyObject;
                    let button_ptr: *mut AnyObject = msg_send![&*status_item, button];
                    if !button_ptr.is_null() {
                        let _: () = msg_send![button_ptr, highlight: highlighted];
                    }
                    status_item.setHighlightMode(highlighted);
                }
            }
        });
    }
}

#[cfg(not(target_os = "macos"))]
fn set_tray_highlight(_app: &tauri::AppHandle, _highlighted: bool) {}

/// 托盘菜单文本（国际化）
#[derive(Clone, Copy)]
pub struct TrayTexts {
    pub show_main: &'static str,
    pub no_provider_hint: &'static str,
    pub quit: &'static str,
}

impl TrayTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" => Self {
                show_main: "Open main window",
                no_provider_hint: "  (No providers yet, please add them from the main window)",
                quit: "Quit",
            },
            "ja" => Self {
                show_main: "メインウィンドウを開く",
                no_provider_hint:
                    "  (プロバイダーがまだありません。メイン画面から追加してください)",
                quit: "終了",
            },
            _ => Self {
                show_main: "打开主界面",
                no_provider_hint: "  (无供应商，请在主界面添加)",
                quit: "退出",
            },
        }
    }
}

/// 托盘应用分区配置
pub struct TrayAppSection {
    pub app_type: AppType,
    pub prefix: &'static str,
    pub header_id: &'static str,
    pub empty_id: &'static str,
    pub header_label: &'static str,
    pub log_name: &'static str,
}

pub const TRAY_SECTIONS: [TrayAppSection; 3] = [
    TrayAppSection {
        app_type: AppType::Claude,
        prefix: "claude_",
        header_id: "claude_header",
        empty_id: "claude_empty",
        header_label: "─── Claude ───",
        log_name: "Claude",
    },
    TrayAppSection {
        app_type: AppType::Codex,
        prefix: "codex_",
        header_id: "codex_header",
        empty_id: "codex_empty",
        header_label: "─── Codex ───",
        log_name: "Codex",
    },
    TrayAppSection {
        app_type: AppType::Gemini,
        prefix: "gemini_",
        header_id: "gemini_header",
        empty_id: "gemini_empty",
        header_label: "─── Gemini ───",
        log_name: "Gemini",
    },
];

/// 初始化托盘弹窗窗口（仅创建一次，默认隐藏）
pub fn init_tray_popover_window(app: &mut App) -> tauri::Result<()> {
    if app.get_webview_window(TRAY_WINDOW_LABEL).is_some() {
        return Ok(());
    }

    let tray_window = WebviewWindowBuilder::new(
        app,
        TRAY_WINDOW_LABEL,
        WebviewUrl::App("index.html#tray".into()),
    )
    .title("")
    .inner_size(TRAY_POPUP_WIDTH, TRAY_POPUP_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .skip_taskbar(true)
    .shadow(true)
    .decorations(false)
    .always_on_top(true)
    .visible(false)
    .focused(false)
    .build()?;

    #[cfg(target_os = "macos")]
    apply_tray_window_corner(&tray_window, 14.0);

    let handle = tray_window.clone();
    tray_window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            let app_handle = handle.app_handle();
            set_tray_highlight(&app_handle, false);
            let _ = handle.hide();
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let app_handle = handle.app_handle();
            set_tray_highlight(&app_handle, false);
            let _ = handle.hide();
        }
        _ => {}
    });

    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_tray_window_corner(window: &tauri::WebviewWindow, radius: f64) {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSColor;

    unsafe {
        if let Ok(ns_window_ptr) = window.ns_window() {
            if let Some(ns_window) = Retained::retain(ns_window_ptr as *mut AnyObject) {
                let _: () = msg_send![&*ns_window, setOpaque: false];
                let clear_color = NSColor::clearColor();
                let _: () = msg_send![&*ns_window, setBackgroundColor: &*clear_color];

                let content_view: *mut AnyObject = msg_send![&*ns_window, contentView];
                if content_view.is_null() {
                    return;
                }
                let super_view: *mut AnyObject = msg_send![content_view, superview];
                let target_view = if super_view.is_null() {
                    content_view
                } else {
                    super_view
                };

                let _: () = msg_send![target_view, setWantsLayer: true];
                let layer: *mut AnyObject = msg_send![target_view, layer];
                if layer.is_null() {
                    return;
                }
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];

                // content view 也设置，避免 webview 超出时露边
                if content_view.is_null() {
                    return;
                }
                let _: () = msg_send![content_view, setWantsLayer: true];
                let layer: *mut AnyObject = msg_send![content_view, layer];
                if layer.is_null() {
                    return;
                }
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];
            }
        }
    }
}

/// 计算弹窗在当前屏幕内的坐标
fn constrained_position(x: f64, y: f64, monitor: Option<&Monitor>) -> (f64, f64) {
    let Some(monitor) = monitor else {
        return (x, y);
    };

    let monitor_x = monitor.position().x as f64;
    let monitor_y = monitor.position().y as f64;
    let monitor_width = monitor.size().width as f64;
    let monitor_height = monitor.size().height as f64;
    let min_x = monitor_x + TRAY_POPUP_MARGIN;
    let max_x = monitor_x + monitor_width - TRAY_POPUP_WIDTH - TRAY_POPUP_MARGIN;
    let min_y = monitor_y + TRAY_POPUP_MARGIN;
    let max_y = monitor_y + monitor_height - TRAY_POPUP_HEIGHT - TRAY_POPUP_MARGIN;

    let clamped_x = if min_x > max_x {
        min_x
    } else {
        x.clamp(min_x, max_x)
    };
    let clamped_y = if min_y > max_y {
        min_y
    } else {
        y.clamp(min_y, max_y)
    };

    (clamped_x, clamped_y)
}

fn position_tray_window(
    window: &tauri::WebviewWindow,
    rect: Rect,
    fallback_position: PhysicalPosition<f64>,
) -> tauri::Result<()> {
    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let rect_position = rect.position.to_physical::<f64>(scale_factor);
    let rect_size = rect.size.to_physical::<f64>(scale_factor);

    let monitor = window
        .monitor_from_point(
            rect_position.x + rect_size.width / 2.0,
            rect_position.y + rect_size.height / 2.0,
        )?
        .or_else(|| {
            window
                .monitor_from_point(fallback_position.x, fallback_position.y)
                .ok()
                .flatten()
        });

    #[cfg(target_os = "macos")]
    let (mut x, mut y) = {
        let center_x = rect_position.x + rect_size.width / 2.0;
        (
            center_x - (TRAY_POPUP_WIDTH / 2.0),
            rect_position.y + rect_size.height + TRAY_POPUP_MARGIN,
        )
    };

    #[cfg(not(target_os = "macos"))]
    let (mut x, mut y) = {
        let anchor_x = rect_position.x + rect_size.width;
        let anchor_y = rect_position.y;
        let mut y = anchor_y - TRAY_POPUP_HEIGHT - TRAY_POPUP_MARGIN;
        if y < TRAY_POPUP_MARGIN {
            y = rect_position.y + rect_size.height + TRAY_POPUP_MARGIN;
        }
        (anchor_x - TRAY_POPUP_WIDTH, y)
    };

    (x, y) = constrained_position(x, y, monitor.as_ref());

    let logical_x = x / scale_factor;
    let logical_y = y / scale_factor;
    window.set_position(Position::Logical(LogicalPosition::new(
        logical_x, logical_y,
    )))
}

/// 显示或隐藏托盘弹窗
pub fn toggle_tray_popover(
    app: &tauri::AppHandle,
    click_position: PhysicalPosition<f64>,
    rect: Rect,
) {
    let Some(window) = app.get_webview_window(TRAY_WINDOW_LABEL) else {
        log::warn!("托盘弹窗窗口尚未初始化");
        return;
    };

    let is_visible = window.is_visible().unwrap_or(false);
    if is_visible {
        set_tray_highlight(app, false);
        let _ = window.hide();
        return;
    }

    if let Err(err) = position_tray_window(&window, rect, click_position) {
        log::warn!("定位托盘弹窗失败: {err}");
    }

    if let Err(err) = window.show() {
        log::error!("显示托盘弹窗失败: {err}");
        return;
    }

    set_tray_highlight(app, true);
    let _ = window.set_focus();
}

/// 隐藏托盘弹窗
pub fn hide_tray_popover(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        set_tray_highlight(app, false);
        let _ = window.hide();
    }
}

/// 添加供应商分区到菜单
fn append_provider_section<'a>(
    app: &'a tauri::AppHandle,
    mut menu_builder: MenuBuilder<'a, tauri::Wry, tauri::AppHandle<tauri::Wry>>,
    manager: Option<&crate::provider::ProviderManager>,
    section: &TrayAppSection,
    tray_texts: &TrayTexts,
) -> Result<MenuBuilder<'a, tauri::Wry, tauri::AppHandle<tauri::Wry>>, AppError> {
    let Some(manager) = manager else {
        return Ok(menu_builder);
    };

    let header = MenuItem::with_id(
        app,
        section.header_id,
        section.header_label,
        false,
        None::<&str>,
    )
    .map_err(|e| AppError::Message(format!("创建{}标题失败: {e}", section.log_name)))?;
    menu_builder = menu_builder.item(&header);

    if manager.providers.is_empty() {
        let empty_hint = MenuItem::with_id(
            app,
            section.empty_id,
            tray_texts.no_provider_hint,
            false,
            None::<&str>,
        )
        .map_err(|e| AppError::Message(format!("创建{}空提示失败: {e}", section.log_name)))?;
        return Ok(menu_builder.item(&empty_hint));
    }

    let mut sorted_providers: Vec<_> = manager.providers.iter().collect();
    sorted_providers.sort_by(|(_, a), (_, b)| {
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

    for (id, provider) in sorted_providers {
        let is_current = manager.current == *id;
        let item = CheckMenuItem::with_id(
            app,
            format!("{}{}", section.prefix, id),
            &provider.name,
            true,
            is_current,
            None::<&str>,
        )
        .map_err(|e| AppError::Message(format!("创建{}菜单项失败: {e}", section.log_name)))?;
        menu_builder = menu_builder.item(&item);
    }

    Ok(menu_builder)
}

/// 创建动态托盘菜单
pub fn create_tray_menu(
    app: &tauri::AppHandle,
    app_state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    let app_settings = crate::settings::get_settings();
    let tray_texts = TrayTexts::from_language(app_settings.language.as_deref().unwrap_or("zh"));

    let mut menu_builder = MenuBuilder::new(app);

    // 顶部：打开主界面
    let show_main_item =
        MenuItem::with_id(app, "show_main", tray_texts.show_main, true, None::<&str>)
            .map_err(|e| AppError::Message(format!("创建打开主界面菜单失败: {e}")))?;
    menu_builder = menu_builder.item(&show_main_item).separator();

    // 直接添加所有供应商到主菜单（扁平化结构，更简单可靠）
    for section in TRAY_SECTIONS.iter() {
        let app_type_str = section.app_type.as_str();
        let providers = app_state.db.get_all_providers(app_type_str)?;

        // 使用有效的当前供应商 ID（验证存在性，自动清理失效 ID）
        let current_id =
            crate::settings::get_effective_current_provider(&app_state.db, &section.app_type)?
                .unwrap_or_default();

        let manager = crate::provider::ProviderManager {
            providers,
            current: current_id,
        };

        menu_builder =
            append_provider_section(app, menu_builder, Some(&manager), section, &tray_texts)?;
    }

    // 分隔符和退出菜单
    let quit_item = MenuItem::with_id(app, "quit", tray_texts.quit, true, None::<&str>)
        .map_err(|e| AppError::Message(format!("创建退出菜单失败: {e}")))?;

    menu_builder = menu_builder.separator().item(&quit_item);

    menu_builder
        .build()
        .map_err(|e| AppError::Message(format!("构建菜单失败: {e}")))
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

/// 显示主窗口并聚焦
pub fn show_main_window(app: &tauri::AppHandle) {
    set_tray_highlight(app, false);
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_skip_taskbar(false);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        #[cfg(target_os = "macos")]
        {
            apply_tray_policy(app, true);
        }
    } else {
    }
}
