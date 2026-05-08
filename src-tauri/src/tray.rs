//! 托盘菜单管理模块
//!
//! 负责系统托盘图标和菜单的创建、更新和事件处理。

use once_cell::sync::Lazy;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, Submenu, SubmenuBuilder};
use tauri::{Emitter, Manager};

use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

/// 每个 app 分区的子菜单句柄，用于 usage 更新时就地改 label 而非整菜单重建。
/// `create_tray_menu` 每次重建都会整表覆盖写入，保证句柄始终指向当前活跃菜单。
static TRAY_SECTION_SUBMENUS: Lazy<
    std::sync::Mutex<std::collections::HashMap<AppType, Submenu<tauri::Wry>>>,
> = Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

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

fn format_usage_suffix(
    app_state: &AppState,
    app_type: &AppType,
    provider: &crate::provider::Provider,
    provider_id: &str,
) -> Option<String> {
    // 当前脚本是否启用：禁用/删除时不再沿用旧 UsageCache 结果，
    // 并顺手 invalidate，防止后续重建继续命中过期数据。
    if provider.has_usage_script_enabled() {
        // 脚本缓存优先（覆盖 Copilot/coding_plan/balance/自定义脚本），借用访问避免克隆整条 UsageResult。
        if let Some(Some(s)) =
            app_state
                .usage_cache
                .with_script(app_type, provider_id, format_script_summary)
        {
            return Some(format!(" · {s}"));
        }
    } else {
        app_state
            .usage_cache
            .invalidate_script(app_type, provider_id);
    }

    if provider.category.as_deref() == Some("official") {
        if let Some(Some(s)) = app_state
            .usage_cache
            .with_subscription(app_type, format_subscription_summary)
        {
            return Some(format!(" · {s}"));
        }
    }
    None
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
    let mut section_handles: std::collections::HashMap<AppType, Submenu<tauri::Wry>> =
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
                Some(p) => {
                    let suffix = format_usage_suffix(app_state, &section.app_type, p, &current_id)
                        .unwrap_or_default();
                    format!("{} · {}{}", section.header_label, p.name, suffix)
                }
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
            section_handles.insert(section.app_type.clone(), submenu.clone());
            menu_builder = menu_builder.item(&submenu);
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

    *TRAY_SECTION_SUBMENUS
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = section_handles;

    Ok(menu)
}

/// 就地更新各 app 分区子菜单的标题（usage 后缀变化时走这条），
/// 避免 `set_menu` 导致用户打开中的菜单被关闭。
/// 句柄由上一次 `create_tray_menu` 填充；为空（从未构建过菜单）时无事发生。
fn update_tray_usage_labels(app: &tauri::AppHandle) {
    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };
    let handles = match TRAY_SECTION_SUBMENUS.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    for section in TRAY_SECTIONS.iter() {
        let Some(submenu) = handles.get(&section.app_type) else {
            continue;
        };
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
        let suffix = format_usage_suffix(&app_state, &section.app_type, provider, &current_id)
            .unwrap_or_default();
        let new_label = format!("{} · {}{}", section.header_label, provider.name, suffix);
        if let Err(e) = submenu.set_text(&new_label) {
            log::debug!("[Tray] 更新{}子菜单标题失败: {e}", section.log_name);
        }
    }
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
/// 刷新面与 `format_usage_suffix` 的展示面严格对齐 —— 每次悬停最多发
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

        // 与 format_usage_suffix 同一优先级：脚本启用 → 查脚本；
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
    use super::{format_script_summary, format_subscription_summary, TRAY_ID};
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
