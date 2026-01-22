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
    pub auto_label: &'static str,
}

impl TrayTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" => Self {
                show_main: "Open main window",
                no_provider_hint: "  (No providers yet, please add them from the main window)",
                quit: "Quit",
                auto_label: "Auto (Failover)",
            },
            "ja" => Self {
                show_main: "メインウィンドウを開く",
                no_provider_hint:
                    "  (プロバイダーがまだありません。メイン画面から追加してください)",
                quit: "終了",
                auto_label: "自動 (フェイルオーバー)",
            },
            _ => Self {
                show_main: "打开主界面",
                no_provider_hint: "  (无供应商，请在主界面添加)",
                quit: "退出",
                auto_label: "自动 (故障转移)",
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

/// Auto 菜单项后缀
pub const AUTO_SUFFIX: &str = "auto";

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
    app_state: &AppState,
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

    // 获取 proxy 状态，决定 Auto 是否选中
    let (proxy_enabled, auto_failover) =
        app_state.db.get_proxy_flags_sync(section.app_type.as_str());
    let auto_mode = proxy_enabled && auto_failover;

    // 添加 Auto 菜单项（始终显示在供应商列表前）
    let auto_item = CheckMenuItem::with_id(
        app,
        format!("{}{}", section.prefix, AUTO_SUFFIX),
        tray_texts.auto_label,
        true,
        auto_mode,
        None::<&str>,
    )
    .map_err(|e| AppError::Message(format!("创建{}Auto菜单项失败: {e}", section.log_name)))?;
    menu_builder = menu_builder.item(&auto_item);

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
        // Auto 模式下所有供应商都不选中
        let is_current = !auto_mode && manager.current == *id;
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
            if let Some(tray) = app.tray_by_id("main") {
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
            if let Some(tray) = app.tray_by_id("main") {
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

    // 顶部：打开主界面
    let show_main_item =
        MenuItem::with_id(app, "show_main", tray_texts.show_main, true, None::<&str>)
            .map_err(|e| AppError::Message(format!("创建打开主界面菜单失败: {e}")))?;
    menu_builder = menu_builder.item(&show_main_item).separator();

    // 直接添加所有供应商到主菜单（扁平化结构，更简单可靠）
    // Only add visible app sections
    for section in TRAY_SECTIONS.iter() {
        // Skip hidden apps
        if !visible_apps.is_visible(&section.app_type) {
            continue;
        }

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

        menu_builder = append_provider_section(
            app,
            menu_builder,
            Some(&manager),
            section,
            &tray_texts,
            app_state,
        )?;
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
