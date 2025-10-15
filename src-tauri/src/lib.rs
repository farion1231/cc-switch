mod app_config;
mod app_store;
mod claude_mcp;
mod claude_plugin;
mod codex_config;
mod commands;
mod config;
mod import_export;
mod mcp;
mod migration;
mod provider;
mod settings;
mod speedtest;
mod usage_script;
mod store;

use store::AppState;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
};
#[cfg(target_os = "macos")]
use tauri::{ActivationPolicy, RunEvent};
use tauri::{Emitter, Manager};

/// 创建动态托盘菜单
fn create_tray_menu(
    app: &tauri::AppHandle,
    app_state: &AppState,
) -> Result<Menu<tauri::Wry>, String> {
    let config = app_state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let mut menu_builder = MenuBuilder::new(app);

    // 顶部：打开主界面
    let show_main_item = MenuItem::with_id(app, "show_main", "打开主界面", true, None::<&str>)
        .map_err(|e| format!("创建打开主界面菜单失败: {}", e))?;
    menu_builder = menu_builder.item(&show_main_item).separator();

    // 直接添加所有供应商到主菜单（扁平化结构，更简单可靠）
    if let Some(claude_manager) = config.get_manager(&crate::app_config::AppType::Claude) {
        // 添加Claude标题（禁用状态，仅作为分组标识）
        let claude_header =
            MenuItem::with_id(app, "claude_header", "─── Claude ───", false, None::<&str>)
                .map_err(|e| format!("创建Claude标题失败: {}", e))?;
        menu_builder = menu_builder.item(&claude_header);

        if !claude_manager.providers.is_empty() {
            // Sort providers by sortIndex, then by createdAt, then by name
            let mut sorted_providers: Vec<_> = claude_manager.providers.iter().collect();
            sorted_providers.sort_by(|(_, a), (_, b)| {
                // Priority 1: sortIndex
                match (a.sort_index, b.sort_index) {
                    (Some(idx_a), Some(idx_b)) => return idx_a.cmp(&idx_b),
                    (Some(_), None) => return std::cmp::Ordering::Less,
                    (None, Some(_)) => return std::cmp::Ordering::Greater,
                    _ => {}
                }
                // Priority 2: createdAt
                match (a.created_at, b.created_at) {
                    (Some(time_a), Some(time_b)) => return time_a.cmp(&time_b),
                    (Some(_), None) => return std::cmp::Ordering::Greater,
                    (None, Some(_)) => return std::cmp::Ordering::Less,
                    _ => {}
                }
                // Priority 3: name
                a.name.cmp(&b.name)
            });

            for (id, provider) in sorted_providers {
                let is_current = claude_manager.current == *id;
                let item = CheckMenuItem::with_id(
                    app,
                    format!("claude_{}", id),
                    &provider.name,
                    true,
                    is_current,
                    None::<&str>,
                )
                .map_err(|e| format!("创建菜单项失败: {}", e))?;
                menu_builder = menu_builder.item(&item);
            }
        } else {
            // 没有供应商时显示提示
            let empty_hint = MenuItem::with_id(
                app,
                "claude_empty",
                "  (无供应商，请在主界面添加)",
                false,
                None::<&str>,
            )
            .map_err(|e| format!("创建Claude空提示失败: {}", e))?;
            menu_builder = menu_builder.item(&empty_hint);
        }
    }

    if let Some(codex_manager) = config.get_manager(&crate::app_config::AppType::Codex) {
        // 添加Codex标题（禁用状态，仅作为分组标识）
        let codex_header =
            MenuItem::with_id(app, "codex_header", "─── Codex ───", false, None::<&str>)
                .map_err(|e| format!("创建Codex标题失败: {}", e))?;
        menu_builder = menu_builder.item(&codex_header);

        if !codex_manager.providers.is_empty() {
            // Sort providers by sortIndex, then by createdAt, then by name
            let mut sorted_providers: Vec<_> = codex_manager.providers.iter().collect();
            sorted_providers.sort_by(|(_, a), (_, b)| {
                // Priority 1: sortIndex
                match (a.sort_index, b.sort_index) {
                    (Some(idx_a), Some(idx_b)) => return idx_a.cmp(&idx_b),
                    (Some(_), None) => return std::cmp::Ordering::Less,
                    (None, Some(_)) => return std::cmp::Ordering::Greater,
                    _ => {}
                }
                // Priority 2: createdAt
                match (a.created_at, b.created_at) {
                    (Some(time_a), Some(time_b)) => return time_a.cmp(&time_b),
                    (Some(_), None) => return std::cmp::Ordering::Greater,
                    (None, Some(_)) => return std::cmp::Ordering::Less,
                    _ => {}
                }
                // Priority 3: name
                a.name.cmp(&b.name)
            });

            for (id, provider) in sorted_providers {
                let is_current = codex_manager.current == *id;
                let item = CheckMenuItem::with_id(
                    app,
                    format!("codex_{}", id),
                    &provider.name,
                    true,
                    is_current,
                    None::<&str>,
                )
                .map_err(|e| format!("创建菜单项失败: {}", e))?;
                menu_builder = menu_builder.item(&item);
            }
        } else {
            // 没有供应商时显示提示
            let empty_hint = MenuItem::with_id(
                app,
                "codex_empty",
                "  (无供应商，请在主界面添加)",
                false,
                None::<&str>,
            )
            .map_err(|e| format!("创建Codex空提示失败: {}", e))?;
            menu_builder = menu_builder.item(&empty_hint);
        }
    }

    // 分隔符和退出菜单
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .map_err(|e| format!("创建退出菜单失败: {}", e))?;

    menu_builder = menu_builder.separator().item(&quit_item);

    menu_builder
        .build()
        .map_err(|e| format!("构建菜单失败: {}", e))
}

#[cfg(target_os = "macos")]
fn apply_tray_policy(app: &tauri::AppHandle, dock_visible: bool) {
    let desired_policy = if dock_visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };

    if let Err(err) = app.set_dock_visibility(dock_visible) {
        log::warn!("设置 Dock 显示状态失败: {}", err);
    }

    if let Err(err) = app.set_activation_policy(desired_policy) {
        log::warn!("设置激活策略失败: {}", err);
    }
}

/// 处理托盘菜单事件
fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    log::info!("处理托盘菜单事件: {}", event_id);

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
        id if id.starts_with("claude_") => {
            let provider_id = id.strip_prefix("claude_").unwrap();
            log::info!("切换到Claude供应商: {}", provider_id);

            // 执行切换
            let app_handle = app.clone();
            let provider_id = provider_id.to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = switch_provider_internal(
                    &app_handle,
                    crate::app_config::AppType::Claude,
                    provider_id,
                )
                .await
                {
                    log::error!("切换Claude供应商失败: {}", e);
                }
            });
        }
        id if id.starts_with("codex_") => {
            let provider_id = id.strip_prefix("codex_").unwrap();
            log::info!("切换到Codex供应商: {}", provider_id);

            // 执行切换
            let app_handle = app.clone();
            let provider_id = provider_id.to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = switch_provider_internal(
                    &app_handle,
                    crate::app_config::AppType::Codex,
                    provider_id,
                )
                .await
                {
                    log::error!("切换Codex供应商失败: {}", e);
                }
            });
        }
        _ => {
            log::warn!("未处理的菜单事件: {}", event_id);
        }
    }
}

//

/// 内部切换供应商函数
async fn switch_provider_internal(
    app: &tauri::AppHandle,
    app_type: crate::app_config::AppType,
    provider_id: String,
) -> Result<(), String> {
    if let Some(app_state) = app.try_state::<AppState>() {
        // 在使用前先保存需要的值
        let app_type_str = app_type.as_str().to_string();
        let provider_id_clone = provider_id.clone();

        crate::commands::switch_provider(
            app_state.clone(),
            Some(app_type),
            None,
            None,
            provider_id,
        )
        .await?;

        // 切换成功后重新创建托盘菜单
        if let Ok(new_menu) = create_tray_menu(app, app_state.inner()) {
            if let Some(tray) = app.tray_by_id("main") {
                if let Err(e) = tray.set_menu(Some(new_menu)) {
                    log::error!("更新托盘菜单失败: {}", e);
                }
            }
        }

        // 发射事件到前端，通知供应商已切换
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "providerId": provider_id_clone
        });
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射供应商切换事件失败: {}", e);
        }
    }
    Ok(())
}

/// 更新托盘菜单的Tauri命令
#[tauri::command]
async fn update_tray_menu(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    if let Ok(new_menu) = create_tray_menu(&app, state.inner()) {
        if let Some(tray) = app.tray_by_id("main") {
            tray.set_menu(Some(new_menu))
                .map_err(|e| format!("更新托盘菜单失败: {}", e))?;
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }

    let builder = builder
        // 拦截窗口关闭：根据设置决定是否最小化到托盘
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let settings = crate::settings::get_settings();

                if settings.minimize_to_tray_on_close {
                    api.prevent_close();
                    let _ = window.hide();
                    #[cfg(target_os = "windows")]
                    {
                        let _ = window.set_skip_taskbar(true);
                    }
                    #[cfg(target_os = "macos")]
                    {
                        apply_tray_policy(window.app_handle(), false);
                    }
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            // 设置全局 AppHandle 以供 Store 使用
            app_store::set_app_handle(app.handle().clone());
            // 注册 Updater 插件（桌面端）
            #[cfg(desktop)]
            {
                if let Err(e) = app
                    .handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())
                {
                    // 若配置不完整（如缺少 pubkey），跳过 Updater 而不中断应用
                    log::warn!("初始化 Updater 插件失败，已跳过：{}", e);
                }
            }
            #[cfg(target_os = "macos")]
            {
                // 设置 macOS 标题栏背景色为主界面蓝色
                if let Some(window) = app.get_webview_window("main") {
                    use objc2::rc::Retained;
                    use objc2::runtime::AnyObject;
                    use objc2_app_kit::NSColor;

                    let ns_window_ptr = window.ns_window().unwrap();
                    let ns_window: Retained<AnyObject> =
                        unsafe { Retained::retain(ns_window_ptr as *mut AnyObject).unwrap() };

                    // 使用与主界面 banner 相同的蓝色 #3498db
                    // #3498db = RGB(52, 152, 219)
                    let bg_color = unsafe {
                        NSColor::colorWithRed_green_blue_alpha(
                            52.0 / 255.0,  // R: 52
                            152.0 / 255.0, // G: 152
                            219.0 / 255.0, // B: 219
                            1.0,           // Alpha: 1.0
                        )
                    };

                    unsafe {
                        use objc2::msg_send;
                        let _: () = msg_send![&*ns_window, setBackgroundColor: &*bg_color];
                    }
                }
            }

            // 初始化日志
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // 初始化应用状态（仅创建一次，并在本函数末尾注入 manage）
            let app_state = AppState::new();

            // 迁移旧的 app_config_dir 配置到 Store
            if let Err(e) = app_store::migrate_app_config_dir_from_settings(&app.handle()) {
                log::warn!("迁移 app_config_dir 失败: {}", e);
            }

            // 首次启动迁移：扫描副本文件，合并到 config.json，并归档副本；旧 config.json 先归档
            {
                let mut config_guard = app_state.config.lock().unwrap();
                let migrated = migration::migrate_copies_into_config(&mut config_guard)?;
                if migrated {
                    log::info!("已将副本文件导入到 config.json，并完成归档");
                }
                // 确保两个 App 条目存在
                config_guard.ensure_app(&app_config::AppType::Claude);
                config_guard.ensure_app(&app_config::AppType::Codex);
            }

            // 保存配置
            let _ = app_state.save();

            // 创建动态托盘菜单
            let menu = create_tray_menu(app.handle(), &app_state)?;

            // 构建托盘
            let mut tray_builder = TrayIconBuilder::with_id("main")
                .on_tray_icon_event(|_tray, event| match event {
                    // 左键点击已通过 show_menu_on_left_click(true) 打开菜单，这里不再额外处理
                    TrayIconEvent::Click { .. } => {}
                    _ => log::debug!("unhandled event {event:?}"),
                })
                .menu(&menu)
                .on_menu_event(|app, event| {
                    handle_tray_menu_event(app, &event.id.0);
                })
                .show_menu_on_left_click(true);

            // 统一使用应用默认图标；待托盘模板图标就绪后再启用
            tray_builder = tray_builder.icon(app.default_window_icon().unwrap().clone());

            let _tray = tray_builder.build(app)?;
            // 将同一个实例注入到全局状态，避免重复创建导致的不一致
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_providers,
            commands::get_current_provider,
            commands::add_provider,
            commands::update_provider,
            commands::delete_provider,
            commands::switch_provider,
            commands::import_default_config,
            commands::get_claude_config_status,
            commands::get_config_status,
            commands::get_claude_code_config_path,
            commands::get_config_dir,
            commands::open_config_folder,
            commands::pick_directory,
            commands::open_external,
            commands::get_app_config_path,
            commands::open_app_config_folder,
            commands::read_live_provider_settings,
            commands::get_settings,
            commands::save_settings,
            commands::restart_app,
            commands::check_for_updates,
            commands::is_portable_mode,
            commands::get_claude_plugin_status,
            commands::read_claude_plugin_config,
            commands::apply_claude_plugin_config,
            commands::is_claude_plugin_applied,
            // Claude MCP management
            commands::get_claude_mcp_status,
            commands::read_claude_mcp_config,
            commands::upsert_claude_mcp_server,
            commands::delete_claude_mcp_server,
            commands::validate_mcp_command,
            // usage query
            commands::query_provider_usage,
            // New MCP via config.json (SSOT)
            commands::get_mcp_config,
            commands::upsert_mcp_server_in_config,
            commands::delete_mcp_server_in_config,
            commands::set_mcp_enabled,
            commands::sync_enabled_mcp_to_claude,
            commands::sync_enabled_mcp_to_codex,
            commands::import_mcp_from_claude,
            commands::import_mcp_from_codex,
            // ours: endpoint speed test + custom endpoint management
            commands::test_api_endpoints,
            commands::get_custom_endpoints,
            commands::add_custom_endpoint,
            commands::remove_custom_endpoint,
            commands::update_endpoint_last_used,
            // app_config_dir override via Store
            commands::get_app_config_dir_override,
            commands::set_app_config_dir_override,
            // provider sort order management
            commands::update_providers_sort_order,
            // theirs: config import/export and dialogs
            import_export::export_config_to_file,
            import_export::import_config_from_file,
            import_export::save_file_dialog,
            import_export::open_file_dialog,
            update_tray_menu,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        // macOS 在 Dock 图标被点击并重新激活应用时会触发 Reopen 事件，这里手动恢复主窗口
        if let RunEvent::Reopen { .. } = event {
            if let Some(window) = app_handle.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                {
                    let _ = window.set_skip_taskbar(false);
                }
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                apply_tray_policy(app_handle, true);
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app_handle, event);
        }
    });
}