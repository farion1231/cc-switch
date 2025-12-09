//! 托盘菜单管理模块
//!
//! 负责系统托盘图标和菜单的创建、更新和事件处理。

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::{Emitter, Manager};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

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

/// 处理供应商托盘事件
pub fn handle_provider_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    for section in TRAY_SECTIONS.iter() {
        if let Some(provider_id) = event_id.strip_prefix(section.prefix) {
            log::info!("切换到{}供应商: {provider_id}", section.log_name);
            let app_handle = app.clone();
            let provider_id = provider_id.to_string();
            let app_type = section.app_type.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = switch_provider_internal(&app_handle, app_type, provider_id) {
                    log::error!("切换{}供应商失败: {e}", section.log_name);
                }
            });
            return true;
        }
    }
    false
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
                #[cfg(target_os = "macos")]
                {
                    apply_tray_policy(app, true);
                }
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

/// 内部切换供应商函数
pub fn switch_provider_internal(
    app: &tauri::AppHandle,
    app_type: AppType,
    provider_id: String,
) -> Result<(), AppError> {
    if let Some(app_state) = app.try_state::<AppState>() {
        // 在使用前先保存需要的值
        let app_type_str = app_type.as_str().to_string();
        let provider_id_clone = provider_id.clone();

        crate::commands::switch_provider(app_state.clone(), app_type_str.clone(), provider_id)
            .map_err(AppError::Message)?;

        // 切换成功后重新创建托盘菜单
        if let Ok(new_menu) = create_tray_menu(app, app_state.inner()) {
            if let Some(tray) = app.tray_by_id("main") {
                if let Err(e) = tray.set_menu(Some(new_menu)) {
                    log::error!("更新托盘菜单失败: {e}");
                }
            }
        }

        // 发射事件到前端，通知供应商已切换
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "providerId": provider_id_clone
        });
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射供应商切换事件失败: {e}");
        }
    }
    Ok(())
}
