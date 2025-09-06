mod app_config;
mod codex_config;
mod commands;
mod config;
mod migration;
mod provider;
mod store;

use store::AppState;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
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

    // 直接添加所有供应商到主菜单（扁平化结构，更简单可靠）
    if let Some(claude_manager) = config.get_manager(&crate::app_config::AppType::Claude) {
        // 添加Claude标题（禁用状态，仅作为分组标识）
        let claude_header =
            MenuItem::with_id(app, "claude_header", "─── Claude ───", false, None::<&str>)
                .map_err(|e| format!("创建Claude标题失败: {}", e))?;
        menu_builder = menu_builder.item(&claude_header);

        if !claude_manager.providers.is_empty() {
            for (id, provider) in &claude_manager.providers {
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
            for (id, provider) in &codex_manager.providers {
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

/// 处理托盘菜单事件
fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    println!("处理托盘菜单事件: {}", event_id);

    match event_id {
        "quit" => {
            println!("退出应用");
            app.exit(0);
        }
        id if id.starts_with("claude_") => {
            let provider_id = id.strip_prefix("claude_").unwrap();
            println!("切换到Claude供应商: {}", provider_id);

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
                    eprintln!("切换Claude供应商失败: {}", e);
                }
            });
        }
        id if id.starts_with("codex_") => {
            let provider_id = id.strip_prefix("codex_").unwrap();
            println!("切换到Codex供应商: {}", provider_id);

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
                    eprintln!("切换Codex供应商失败: {}", e);
                }
            });
        }
        _ => {
            println!("未处理的菜单事件: {}", event_id);
        }
    }
}

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
            app_state.clone().into(),
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
                    eprintln!("更新托盘菜单失败: {}", e);
                }
            }
        }

        // 发射事件到前端，通知供应商已切换
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "providerId": provider_id_clone
        });
        if let Err(e) = app.emit("provider-switched", event_data) {
            eprintln!("发射供应商切换事件失败: {}", e);
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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
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

            // 首次启动迁移：扫描副本文件，合并到 config.json，并归档副本；旧 config.json 先归档
            {
                let mut config_guard = app_state.config.lock().unwrap();
                let migrated = migration::migrate_copies_into_config(&mut *config_guard)?;
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
            let menu = create_tray_menu(&app.handle(), &app_state)?;

            let _tray = TrayIconBuilder::with_id("main")
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        println!("left click pressed and released");
                        // 在这个例子中，当点击托盘图标时，将展示并聚焦于主窗口
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {
                        println!("unhandled event {event:?}");
                    }
                })
                .menu(&menu)
                .on_menu_event(|app, event| {
                    handle_tray_menu_event(app, &event.id.0);
                })
                .icon(app.default_window_icon().unwrap().clone())
                .show_menu_on_left_click(true)
                .build(app)?;
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
            commands::open_config_folder,
            commands::open_external,
            update_tray_menu,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
