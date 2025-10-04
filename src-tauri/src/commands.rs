#![allow(non_snake_case)]

use std::collections::HashMap;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::claude_plugin;
use crate::codex_config;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::provider::Provider;
use crate::store::AppState;

fn validate_provider_settings(app_type: &AppType, provider: &Provider) -> Result<(), String> {
    match app_type {
        AppType::Claude => {
            if !provider.settings_config.is_object() {
                return Err("Claude 配置必须是 JSON 对象".to_string());
            }
        }
        AppType::Codex => {
            let settings = provider
                .settings_config
                .as_object()
                .ok_or_else(|| "Codex 配置必须是 JSON 对象".to_string())?;
            let auth = settings
                .get("auth")
                .ok_or_else(|| "Codex 配置缺少 auth 字段".to_string())?;
            if !auth.is_object() {
                return Err("Codex auth 配置必须是 JSON 对象".to_string());
            }
            if let Some(config_value) = settings.get("config") {
                if !(config_value.is_string() || config_value.is_null()) {
                    return Err("Codex config 字段必须是字符串".to_string());
                }
                if let Some(cfg_text) = config_value.as_str() {
                    codex_config::validate_config_toml(cfg_text)?;
                }
            }
        }
    }
    Ok(())
}

/// 获取所有供应商
#[tauri::command]
pub async fn get_providers(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<HashMap<String, Provider>, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let manager = config
        .get_manager(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    Ok(manager.get_all_providers().clone())
}

/// 获取当前供应商ID
#[tauri::command]
pub async fn get_current_provider(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<String, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let manager = config
        .get_manager(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    Ok(manager.current.clone())
}

/// 添加供应商
#[tauri::command]
pub async fn add_provider(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    provider: Provider,
) -> Result<bool, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    validate_provider_settings(&app_type, &provider)?;

    // 读取当前是否是激活供应商（短锁）
    let is_current = {
        let config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        let manager = config
            .get_manager(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
        manager.current == provider.id
    };

    // 若目标为当前供应商，则先写 live，成功后再落盘配置
    if is_current {
        match app_type {
            AppType::Claude => {
                let settings_path = crate::config::get_claude_settings_path();
                crate::config::write_json_file(&settings_path, &provider.settings_config)?;
            }
            AppType::Codex => {
                let auth = provider
                    .settings_config
                    .get("auth")
                    .ok_or_else(|| "目标供应商缺少 auth 配置".to_string())?;
                let cfg_text = provider
                    .settings_config
                    .get("config")
                    .and_then(|v| v.as_str());
                crate::codex_config::write_codex_live_atomic(auth, cfg_text)?;
            }
        }
    }

    // 更新内存并保存配置
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        let manager = config
            .get_manager_mut(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
        manager
            .providers
            .insert(provider.id.clone(), provider.clone());
    }
    state.save()?;

    Ok(true)
}

/// 更新供应商
#[tauri::command]
pub async fn update_provider(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    provider: Provider,
) -> Result<bool, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    validate_provider_settings(&app_type, &provider)?;

    // 读取校验 & 是否当前（短锁）
    let (exists, is_current) = {
        let config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        let manager = config
            .get_manager(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
        (
            manager.providers.contains_key(&provider.id),
            manager.current == provider.id,
        )
    };
    if !exists {
        return Err(format!("供应商不存在: {}", provider.id));
    }

    // 若更新的是当前供应商，先写 live 成功再保存
    if is_current {
        match app_type {
            AppType::Claude => {
                let settings_path = crate::config::get_claude_settings_path();
                crate::config::write_json_file(&settings_path, &provider.settings_config)?;
            }
            AppType::Codex => {
                let auth = provider
                    .settings_config
                    .get("auth")
                    .ok_or_else(|| "目标供应商缺少 auth 配置".to_string())?;
                let cfg_text = provider
                    .settings_config
                    .get("config")
                    .and_then(|v| v.as_str());
                crate::codex_config::write_codex_live_atomic(auth, cfg_text)?;
            }
        }
    }

    // 更新内存并保存
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        let manager = config
            .get_manager_mut(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
        manager
            .providers
            .insert(provider.id.clone(), provider.clone());
    }
    state.save()?;

    Ok(true)
}

/// 删除供应商
#[tauri::command]
pub async fn delete_provider(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    id: String,
) -> Result<bool, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let manager = config
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    // 检查是否为当前供应商
    if manager.current == id {
        return Err("不能删除当前正在使用的供应商".to_string());
    }

    // 获取供应商信息
    let provider = manager
        .providers
        .get(&id)
        .ok_or_else(|| format!("供应商不存在: {}", id))?
        .clone();

    // 删除配置文件
    match app_type {
        AppType::Codex => {
            codex_config::delete_codex_provider_config(&id, &provider.name)?;
        }
        AppType::Claude => {
            use crate::config::{delete_file, get_provider_config_path};
            // 兼容历史两种命名：settings-{name}.json 与 settings-{id}.json
            let by_name = get_provider_config_path(&id, Some(&provider.name));
            let by_id = get_provider_config_path(&id, None);
            delete_file(&by_name)?;
            delete_file(&by_id)?;
        }
    }

    // 从管理器删除
    manager.providers.remove(&id);

    // 保存配置
    drop(config); // 释放锁
    state.save()?;

    Ok(true)
}

/// 切换供应商
#[tauri::command]
pub async fn switch_provider(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    id: String,
) -> Result<bool, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let manager = config
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    // 检查供应商是否存在
    let provider = manager
        .providers
        .get(&id)
        .ok_or_else(|| format!("供应商不存在: {}", id))?
        .clone();

    // SSOT 切换：先回填 live 配置到当前供应商，然后从内存写入目标主配置
    match app_type {
        AppType::Codex => {
            use serde_json::Value;

            // 回填：读取 live（auth.json + config.toml）写回当前供应商 settings_config
            if !manager.current.is_empty() {
                let auth_path = codex_config::get_codex_auth_path();
                let config_path = codex_config::get_codex_config_path();
                if auth_path.exists() {
                    let auth: Value = crate::config::read_json_file(&auth_path)?;
                    let config_str = if config_path.exists() {
                        std::fs::read_to_string(&config_path)
                            .map_err(|e| format!("读取 config.toml 失败: {}", e))?
                    } else {
                        String::new()
                    };

                    let live = serde_json::json!({
                        "auth": auth,
                        "config": config_str,
                    });

                    if let Some(cur) = manager.providers.get_mut(&manager.current) {
                        cur.settings_config = live;
                    }
                }
            }

            // 切换：从目标供应商 settings_config 写入主配置（Codex 双文件原子+回滚）
            let auth = provider
                .settings_config
                .get("auth")
                .ok_or_else(|| "目标供应商缺少 auth 配置".to_string())?;
            let cfg_text = provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str());
            crate::codex_config::write_codex_live_atomic(auth, cfg_text)?;
        }
        AppType::Claude => {
            use crate::config::{read_json_file, write_json_file};

            let settings_path = get_claude_settings_path();

            // 回填：读取 live settings.json 写回当前供应商 settings_config
            if settings_path.exists() && !manager.current.is_empty() {
                if let Ok(live) = read_json_file::<serde_json::Value>(&settings_path) {
                    if let Some(cur) = manager.providers.get_mut(&manager.current) {
                        cur.settings_config = live;
                    }
                }
            }

            // 切换：从目标供应商 settings_config 写入主配置
            if let Some(parent) = settings_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }

            // 不做归档，直接写入
            write_json_file(&settings_path, &provider.settings_config)?;
        }
    }

    // 更新当前供应商
    manager.current = id;

    log::info!("成功切换到供应商: {}", provider.name);

    // 保存配置
    drop(config); // 释放锁
    state.save()?;

    Ok(true)
}

/// 导入当前配置为默认供应商
#[tauri::command]
pub async fn import_default_config(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<bool, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    // 仅当 providers 为空时才从 live 导入一条默认项
    {
        let config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;

        if let Some(manager) = config.get_manager(&app_type) {
            if !manager.get_all_providers().is_empty() {
                return Ok(true);
            }
        }
    }

    // 根据应用类型导入配置
    // 读取当前主配置为默认供应商（不再写入副本文件）
    let settings_config = match app_type {
        AppType::Codex => {
            let auth_path = codex_config::get_codex_auth_path();
            if !auth_path.exists() {
                return Err("Codex 配置文件不存在".to_string());
            }
            let auth: serde_json::Value =
                crate::config::read_json_file::<serde_json::Value>(&auth_path)?;
            let config_str = match crate::codex_config::read_and_validate_codex_config_text() {
                Ok(s) => s,
                Err(e) => return Err(e),
            };
            serde_json::json!({ "auth": auth, "config": config_str })
        }
        AppType::Claude => {
            let settings_path = get_claude_settings_path();
            if !settings_path.exists() {
                return Err("Claude Code 配置文件不存在".to_string());
            }
            crate::config::read_json_file::<serde_json::Value>(&settings_path)?
        }
    };

    // 创建默认供应商（仅首次初始化）
    let provider = Provider::with_id(
        "default".to_string(),
        "default".to_string(),
        settings_config,
        None,
    );

    // 添加到管理器
    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let manager = config
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    manager.providers.insert(provider.id.clone(), provider);
    // 设置当前供应商为默认项
    manager.current = "default".to_string();

    // 保存配置
    drop(config); // 释放锁
    state.save()?;

    Ok(true)
}

/// 获取 Claude Code 配置状态
#[tauri::command]
pub async fn get_claude_config_status() -> Result<ConfigStatus, String> {
    Ok(crate::config::get_claude_config_status())
}

/// 获取应用配置状态（通用）
/// 兼容两种参数：`app_type`（推荐）或 `app`（字符串）
#[tauri::command]
pub async fn get_config_status(
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<ConfigStatus, String> {
    let app = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    match app {
        AppType::Claude => Ok(crate::config::get_claude_config_status()),
        AppType::Codex => {
            use crate::codex_config::{get_codex_auth_path, get_codex_config_dir};
            let auth_path = get_codex_auth_path();

            // 放宽：只要 auth.json 存在即可认为已配置；config.toml 允许为空
            let exists = auth_path.exists();
            let path = get_codex_config_dir().to_string_lossy().to_string();

            Ok(ConfigStatus { exists, path })
        }
    }
}

/// 获取 Claude Code 配置文件路径
#[tauri::command]
pub async fn get_claude_code_config_path() -> Result<String, String> {
    Ok(get_claude_settings_path().to_string_lossy().to_string())
}

/// 获取当前生效的配置目录
#[tauri::command]
pub async fn get_config_dir(
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<String, String> {
    let app = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    let dir = match app {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::Codex => codex_config::get_codex_config_dir(),
    };

    Ok(dir.to_string_lossy().to_string())
}

/// 打开配置文件夹
/// 兼容两种参数：`app_type`（推荐）或 `app`（字符串）
#[tauri::command]
pub async fn open_config_folder(
    handle: tauri::AppHandle,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<bool, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    let config_dir = match app_type {
        AppType::Claude => crate::config::get_claude_config_dir(),
        AppType::Codex => crate::codex_config::get_codex_config_dir(),
    };

    // 确保目录存在
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    // 使用 opener 插件打开文件夹
    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {}", e))?;

    Ok(true)
}

/// 弹出系统目录选择器并返回用户选择的路径
#[tauri::command]
pub async fn pick_directory(
    app: tauri::AppHandle,
    default_path: Option<String>,
) -> Result<Option<String>, String> {
    let initial = default_path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        if let Some(path) = initial {
            builder = builder.set_directory(path);
        }
        builder.blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("弹出目录选择器失败: {}", e))?;

    match result {
        Some(file_path) => {
            let resolved = file_path
                .simplified()
                .into_path()
                .map_err(|e| format!("解析选择的目录失败: {}", e))?;
            Ok(Some(resolved.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

/// 打开外部链接
#[tauri::command]
pub async fn open_external(app: tauri::AppHandle, url: String) -> Result<bool, String> {
    // 规范化 URL，缺少协议时默认加 https://
    let url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else {
        format!("https://{}", url)
    };

    // 使用 opener 插件打开链接
    app.opener()
        .open_url(&url, None::<String>)
        .map_err(|e| format!("打开链接失败: {}", e))?;

    Ok(true)
}

/// 获取应用配置文件路径
#[tauri::command]
pub async fn get_app_config_path() -> Result<String, String> {
    use crate::config::get_app_config_path;

    let config_path = get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

/// 打开应用配置文件夹
#[tauri::command]
pub async fn open_app_config_folder(handle: tauri::AppHandle) -> Result<bool, String> {
    use crate::config::get_app_config_dir;

    let config_dir = get_app_config_dir();

    // 确保目录存在
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    // 使用 opener 插件打开文件夹
    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {}", e))?;

    Ok(true)
}

/// 获取设置
#[tauri::command]
pub async fn get_settings() -> Result<crate::settings::AppSettings, String> {
    Ok(crate::settings::get_settings())
}

/// 保存设置
#[tauri::command]
pub async fn save_settings(settings: crate::settings::AppSettings) -> Result<bool, String> {
    crate::settings::update_settings(settings)?;
    Ok(true)
}

/// 检查更新
#[tauri::command]
pub async fn check_for_updates(handle: tauri::AppHandle) -> Result<bool, String> {
    // 打开 GitHub releases 页面
    handle
        .opener()
        .open_url(
            "https://github.com/farion1231/cc-switch/releases/latest",
            None::<String>,
        )
        .map_err(|e| format!("打开更新页面失败: {}", e))?;

    Ok(true)
}

/// 判断是否为便携版（绿色版）运行
#[tauri::command]
pub async fn is_portable_mode() -> Result<bool, String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("获取可执行路径失败: {}", e))?;
    if let Some(dir) = exe_path.parent() {
        Ok(dir.join("portable.ini").is_file())
    } else {
        Ok(false)
    }
}

/// Claude 插件：获取 ~/.claude/config.json 状态
#[tauri::command]
pub async fn get_claude_plugin_status() -> Result<ConfigStatus, String> {
    match claude_plugin::claude_config_status() {
        Ok((exists, path)) => Ok(ConfigStatus {
            exists,
            path: path.to_string_lossy().to_string(),
        }),
        Err(err) => Err(err),
    }
}

/// Claude 插件：读取配置内容（若不存在返回 Ok(None)）
#[tauri::command]
pub async fn read_claude_plugin_config() -> Result<Option<String>, String> {
    claude_plugin::read_claude_config()
}

/// Claude 插件：写入/清除固定配置
#[tauri::command]
pub async fn apply_claude_plugin_config(official: bool) -> Result<bool, String> {
    if official {
        claude_plugin::clear_claude_config()
    } else {
        claude_plugin::write_claude_config()
    }
}

/// Claude 插件：检测是否已写入目标配置
#[tauri::command]
pub async fn is_claude_plugin_applied() -> Result<bool, String> {
    claude_plugin::is_claude_config_applied()
}
