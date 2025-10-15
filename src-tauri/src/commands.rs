#![allow(non_snake_case)]

use std::collections::HashMap;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::claude_mcp;
use crate::claude_plugin;
use crate::codex_config;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::provider::{Provider, ProviderMeta};
use crate::speedtest;
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

    // 更新内存并保存（保留/合并已有的 meta.custom_endpoints，避免丢失在编辑流程中新增的自定义端点）
    {
        let mut config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;
        let manager = config
            .get_manager_mut(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

        // 若已存在旧供应商，合并其 meta（尤其是 custom_endpoints）到新对象
        let merged_provider = if let Some(existing) = manager.providers.get(&provider.id) {
            // 克隆入参作为基准
            let mut updated = provider.clone();

            match (existing.meta.as_ref(), updated.meta.take()) {
                // 入参未携带 meta：直接沿用旧 meta
                (Some(old_meta), None) => {
                    updated.meta = Some(old_meta.clone());
                }
                // 入参携带 meta：与旧 meta 合并（以旧值为准，保留新增项）
                (Some(old_meta), Some(mut new_meta)) => {
                    // 合并 custom_endpoints（URL 去重，保留旧端点的时间信息，补充新增端点）
                    let mut merged_map = old_meta.custom_endpoints.clone();
                    for (url, ep) in new_meta.custom_endpoints.drain() {
                        merged_map.entry(url).or_insert(ep);
                    }
                    updated.meta = Some(crate::provider::ProviderMeta {
                        custom_endpoints: merged_map,
                        usage_script: new_meta.usage_script.clone(),
                    });
                }
                // 旧 meta 不存在：使用入参（可能为 None）
                (None, maybe_new) => {
                    updated.meta = maybe_new;
                }
            }

            updated
        } else {
            // 不存在旧供应商（理论上不应发生，因为前面已校验 exists）
            provider.clone()
        };

        manager
            .providers
            .insert(merged_provider.id.clone(), merged_provider);
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

    // 为避免长期可变借用，尽快获取必要数据并缩小借用范围
    let provider = {
        let manager = config
            .get_manager_mut(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

        // 检查供应商是否存在
        let provider = manager
            .providers
            .get(&id)
            .ok_or_else(|| format!("供应商不存在: {}", id))?
            .clone();
        provider
    };

    // SSOT 切换：先回填 live 配置到当前供应商，然后从内存写入目标主配置
    match app_type {
        AppType::Codex => {
            use serde_json::Value;

            // 回填：读取 live（auth.json + config.toml）写回当前供应商 settings_config
            if !{
                let cur = config
                    .get_manager_mut(&app_type)
                    .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
                cur.current.is_empty()
            } {
                let auth_path = codex_config::get_codex_auth_path();
                let config_path = codex_config::get_codex_config_path();
                if auth_path.exists() {
                    let auth: Value = crate::config::read_json_file(&auth_path)?;
                    let config_str = if config_path.exists() {
                        std::fs::read_to_string(&config_path).map_err(|e| {
                            format!("读取 config.toml 失败: {}: {}", config_path.display(), e)
                        })?
                    } else {
                        String::new()
                    };

                    let live = serde_json::json!({
                        "auth": auth,
                        "config": config_str,
                    });

                    let cur_id2 = {
                        let m = config
                            .get_manager(&app_type)
                            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
                        m.current.clone()
                    };
                    let m = config
                        .get_manager_mut(&app_type)
                        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
                    if let Some(cur) = m.providers.get_mut(&cur_id2) {
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
            if settings_path.exists() {
                let cur_id = {
                    let m = config
                        .get_manager(&app_type)
                        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
                    m.current.clone()
                };
                if !cur_id.is_empty() {
                    if let Ok(live) = read_json_file::<serde_json::Value>(&settings_path) {
                        let m = config
                            .get_manager_mut(&app_type)
                            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
                        if let Some(cur) = m.providers.get_mut(&cur_id) {
                            cur.settings_config = live;
                        }
                    }
                }
            }

            // 切换：从目标供应商 settings_config 写入主配置
            if let Some(parent) = settings_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }

            // 不做归档，直接写入
            write_json_file(&settings_path, &provider.settings_config)?;

            // 写入后回读 live，并回填到目标供应商的 SSOT，保证一致
            if settings_path.exists() {
                if let Ok(live_after) = read_json_file::<serde_json::Value>(&settings_path) {
                    let m = config
                        .get_manager_mut(&app_type)
                        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
                    if let Some(target) = m.providers.get_mut(&id) {
                        target.settings_config = live_after;
                    }
                }
            }
        }
    }

    // 更新当前供应商（短借用范围）
    {
        let manager = config
            .get_manager_mut(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
        manager.current = id;
    }

    // 对 Codex：切换完成后，同步 MCP 到 config.toml，并将最新的 config.toml 回填到当前供应商 settings_config.config
    if let AppType::Codex = app_type {
        // 1) 依据 SSOT 将启用的 MCP 投影到 ~/.codex/config.toml
        crate::mcp::sync_enabled_to_codex(&config)?;

        // 2) 读取投影后的 live config.toml 文本
        let cfg_text_after = crate::codex_config::read_and_validate_codex_config_text()?;

        // 3) 回填到当前（目标）供应商的 settings_config.config，确保编辑面板读取到最新 MCP
        let cur_id = {
            let m = config
                .get_manager(&app_type)
                .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
            m.current.clone()
        };
        let m = config
            .get_manager_mut(&app_type)
            .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;
        if let Some(p) = m.providers.get_mut(&cur_id) {
            if let Some(obj) = p.settings_config.as_object_mut() {
                obj.insert(
                    "config".to_string(),
                    serde_json::Value::String(cfg_text_after),
                );
            }
        }
    }

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

// =====================
// Claude MCP 管理命令
// =====================

/// 获取 Claude MCP 状态（settings.local.json 与 mcp.json）
#[tauri::command]
pub async fn get_claude_mcp_status() -> Result<crate::claude_mcp::McpStatus, String> {
    claude_mcp::get_mcp_status()
}

/// 读取 mcp.json 文本内容（不存在则返回 Ok(None)）
#[tauri::command]
pub async fn read_claude_mcp_config() -> Result<Option<String>, String> {
    claude_mcp::read_mcp_json()
}

/// 新增或更新一个 MCP 服务器条目
#[tauri::command]
pub async fn upsert_claude_mcp_server(id: String, spec: serde_json::Value) -> Result<bool, String> {
    claude_mcp::upsert_mcp_server(&id, spec)
}

/// 删除一个 MCP 服务器条目
#[tauri::command]
pub async fn delete_claude_mcp_server(id: String) -> Result<bool, String> {
    claude_mcp::delete_mcp_server(&id)
}

/// 校验命令是否在 PATH 中可用（不执行）
#[tauri::command]
pub async fn validate_mcp_command(cmd: String) -> Result<bool, String> {
    claude_mcp::validate_command_in_path(&cmd)
}

// =====================
// 用量查询命令
// =====================

/// 查询供应商用量
#[tauri::command]
pub async fn query_provider_usage(
    state: State<'_, AppState>,
    provider_id: Option<String>,
    providerId: Option<String>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<crate::provider::UsageResult, String> {
    use crate::provider::{UsageData, UsageResult};

    // 解析参数
    let provider_id = provider_id
        .or(providerId)
        .ok_or("缺少 providerId 参数")?;

    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    // 1. 获取供应商配置并克隆所需数据
    let (api_key, base_url, usage_script_code, timeout) = {
        let config = state
            .config
            .lock()
            .map_err(|e| format!("获取锁失败: {}", e))?;

        let manager = config
            .get_manager(&app_type)
            .ok_or("应用类型不存在")?;

        let provider = manager
            .providers
            .get(&provider_id)
            .ok_or("供应商不存在")?;

        // 2. 检查脚本配置
        let usage_script = provider
            .meta
            .as_ref()
            .and_then(|m| m.usage_script.as_ref())
            .ok_or("未配置用量查询脚本")?;

        if !usage_script.enabled {
            return Err("用量查询未启用".to_string());
        }

        // 3. 提取凭证和脚本配置
        let (api_key, base_url) = extract_credentials(provider, &app_type)?;
        let timeout = usage_script.timeout.unwrap_or(10);
        let code = usage_script.code.clone();

        // 显式释放锁
        drop(config);

        (api_key, base_url, code, timeout)
    };

    // 5. 执行脚本
    let result = crate::usage_script::execute_usage_script(
        &usage_script_code,
        &api_key,
        &base_url,
        timeout,
    )
    .await;

    // 6. 构建结果（支持单对象或数组）
    match result {
        Ok(data) => {
            // 尝试解析为数组
            let usage_list: Vec<UsageData> = if data.is_array() {
                // 直接解析为数组
                serde_json::from_value(data)
                    .map_err(|e| format!("数据格式错误: {}", e))?
            } else {
                // 单对象包装为数组（向后兼容）
                let single: UsageData = serde_json::from_value(data)
                    .map_err(|e| format!("数据格式错误: {}", e))?;
                vec![single]
            };

            Ok(UsageResult {
                success: true,
                data: Some(usage_list),
                error: None,
            })
        }
        Err(e) => {
            Ok(UsageResult {
                success: false,
                data: None,
                error: Some(e),
            })
        }
    }
}

/// 从供应商配置中提取 API Key 和 Base URL
fn extract_credentials(
    provider: &crate::provider::Provider,
    app_type: &AppType,
) -> Result<(String, String), String> {
    match app_type {
        AppType::Claude => {
            let env = provider
                .settings_config
                .get("env")
                .and_then(|v| v.as_object())
                .ok_or("配置格式错误: 缺少 env")?;

            let api_key = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|v| v.as_str())
                .ok_or("缺少 API Key")?
                .to_string();

            let base_url = env
                .get("ANTHROPIC_BASE_URL")
                .and_then(|v| v.as_str())
                .ok_or("缺少 ANTHROPIC_BASE_URL 配置")?
                .to_string();

            Ok((api_key, base_url))
        }
        AppType::Codex => {
            let auth = provider
                .settings_config
                .get("auth")
                .and_then(|v| v.as_object())
                .ok_or("配置格式错误: 缺少 auth")?;

            let api_key = auth
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .ok_or("缺少 API Key")?
                .to_string();

            // 从 config TOML 中提取 base_url
            let config_toml = provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let base_url = if config_toml.contains("base_url") {
                let re = regex::Regex::new(r#"base_url\s*=\s*["']([^"']+)["']"#).unwrap();
                re.captures(config_toml)
                    .and_then(|caps| caps.get(1))
                    .map(|m| m.as_str().to_string())
                    .ok_or("config.toml 中 base_url 格式错误")?
            } else {
                return Err("config.toml 中缺少 base_url 配置".to_string());
            };

            Ok((api_key, base_url))
        }
    }
}

// =====================
// 新：集中以 config.json 为 SSOT 的 MCP 配置命令
// =====================

#[derive(serde::Serialize)]
pub struct McpConfigResponse {
    pub config_path: String,
    pub servers: std::collections::HashMap<String, serde_json::Value>,
}

/// 获取 MCP 配置（来自 ~/.cc-switch/config.json）
#[tauri::command]
pub async fn get_mcp_config(
    state: State<'_, AppState>,
    app: Option<String>,
) -> Result<McpConfigResponse, String> {
    let config_path = crate::config::get_app_config_path()
        .to_string_lossy()
        .to_string();
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let app_ty = crate::app_config::AppType::from(app.as_deref().unwrap_or("claude"));
    let (servers, normalized) = crate::mcp::get_servers_snapshot_for(&mut cfg, &app_ty);
    let need_save = normalized > 0;
    drop(cfg);
    if need_save {
        state.save()?;
    }
    Ok(McpConfigResponse {
        config_path,
        servers,
    })
}

/// 在 config.json 中新增或更新一个 MCP 服务器定义
#[tauri::command]
pub async fn upsert_mcp_server_in_config(
    state: State<'_, AppState>,
    app: Option<String>,
    id: String,
    spec: serde_json::Value,
    sync_other_side: Option<bool>,
) -> Result<bool, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let app_ty = crate::app_config::AppType::from(app.as_deref().unwrap_or("claude"));
    let mut sync_targets: Vec<crate::app_config::AppType> = Vec::new();

    let changed = crate::mcp::upsert_in_config_for(&mut cfg, &app_ty, &id, spec.clone())?;

    let should_sync_current = cfg
        .mcp_for(&app_ty)
        .servers
        .get(&id)
        .and_then(|entry| entry.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if should_sync_current {
        sync_targets.push(app_ty.clone());
    }

    if sync_other_side.unwrap_or(false) {
        let other_app = match app_ty.clone() {
            crate::app_config::AppType::Claude => crate::app_config::AppType::Codex,
            crate::app_config::AppType::Codex => crate::app_config::AppType::Claude,
        };
        crate::mcp::upsert_in_config_for(&mut cfg, &other_app, &id, spec)?;

        let should_sync_other = cfg
            .mcp_for(&other_app)
            .servers
            .get(&id)
            .and_then(|entry| entry.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if should_sync_other {
            sync_targets.push(other_app.clone());
        }
    }
    drop(cfg);
    state.save()?;

    let cfg2 = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    for app_ty_to_sync in sync_targets {
        match app_ty_to_sync {
            crate::app_config::AppType::Claude => crate::mcp::sync_enabled_to_claude(&cfg2)?,
            crate::app_config::AppType::Codex => crate::mcp::sync_enabled_to_codex(&cfg2)?,
        };
    }
    Ok(changed)
}

/// 在 config.json 中删除一个 MCP 服务器定义
#[tauri::command]
pub async fn delete_mcp_server_in_config(
    state: State<'_, AppState>,
    app: Option<String>,
    id: String,
) -> Result<bool, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let app_ty = crate::app_config::AppType::from(app.as_deref().unwrap_or("claude"));
    let existed = crate::mcp::delete_in_config_for(&mut cfg, &app_ty, &id)?;
    drop(cfg);
    state.save()?;
    // 若删除的是 Claude/Codex 客户端的条目，则同步一次，确保启用项从对应 live 配置中移除
    let cfg2 = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    match app_ty {
        crate::app_config::AppType::Claude => crate::mcp::sync_enabled_to_claude(&cfg2)?,
        crate::app_config::AppType::Codex => crate::mcp::sync_enabled_to_codex(&cfg2)?,
    }
    Ok(existed)
}

/// 设置启用状态并同步到 ~/.claude.json
#[tauri::command]
pub async fn set_mcp_enabled(
    state: State<'_, AppState>,
    app: Option<String>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let app_ty = crate::app_config::AppType::from(app.as_deref().unwrap_or("claude"));
    let changed = crate::mcp::set_enabled_and_sync_for(&mut cfg, &app_ty, &id, enabled)?;
    drop(cfg);
    state.save()?;
    Ok(changed)
}

/// 手动同步：将启用的 MCP 投影到 ~/.claude.json（不更改 config.json）
#[tauri::command]
pub async fn sync_enabled_mcp_to_claude(state: State<'_, AppState>) -> Result<bool, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let normalized = crate::mcp::normalize_servers_for(&mut cfg, &AppType::Claude);
    crate::mcp::sync_enabled_to_claude(&cfg)?;
    let need_save = normalized > 0;
    drop(cfg);
    if need_save {
        state.save()?;
    }
    Ok(true)
}

/// 手动同步：将启用的 MCP 投影到 ~/.codex/config.toml（不更改 config.json）
#[tauri::command]
pub async fn sync_enabled_mcp_to_codex(state: State<'_, AppState>) -> Result<bool, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let normalized = crate::mcp::normalize_servers_for(&mut cfg, &AppType::Codex);
    crate::mcp::sync_enabled_to_codex(&cfg)?;
    let need_save = normalized > 0;
    drop(cfg);
    if need_save {
        state.save()?;
    }
    Ok(true)
}

/// 从 ~/.claude.json 导入 MCP 定义到 config.json，返回变更数量
#[tauri::command]
pub async fn import_mcp_from_claude(state: State<'_, AppState>) -> Result<usize, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let changed = crate::mcp::import_from_claude(&mut cfg)?;
    drop(cfg);
    if changed > 0 {
        state.save()?;
    }
    Ok(changed)
}

/// 从 ~/.codex/config.toml 导入 MCP 定义到 config.json（Codex 作用域），返回变更数量
#[tauri::command]
pub async fn import_mcp_from_codex(state: State<'_, AppState>) -> Result<usize, String> {
    let mut cfg = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let changed = crate::mcp::import_from_codex(&mut cfg)?;
    drop(cfg);
    if changed > 0 {
        state.save()?;
    }
    Ok(changed)
}

/// 读取当前生效（live）的配置内容，返回可直接作为 provider.settings_config 的对象
/// - Codex: 返回 { auth: JSON, config: string }
/// - Claude: 返回 settings.json 的 JSON 内容
#[tauri::command]
pub async fn read_live_provider_settings(
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
) -> Result<serde_json::Value, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);

    match app_type {
        AppType::Codex => {
            let auth_path = crate::codex_config::get_codex_auth_path();
            if !auth_path.exists() {
                return Err("Codex 配置文件不存在：缺少 auth.json".to_string());
            }
            let auth: serde_json::Value = crate::config::read_json_file(&auth_path)?;
            let cfg_text = crate::codex_config::read_and_validate_codex_config_text()?;
            Ok(serde_json::json!({ "auth": auth, "config": cfg_text }))
        }
        AppType::Claude => {
            let path = crate::config::get_claude_settings_path();
            if !path.exists() {
                return Err("Claude Code 配置文件不存在".to_string());
            }
            let v: serde_json::Value = crate::config::read_json_file(&path)?;
            Ok(v)
        }
    }
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

/// 重启应用程序（当 app_config_dir 变更后使用）
#[tauri::command]
pub async fn restart_app(app: tauri::AppHandle) -> Result<bool, String> {
    // 使用 tauri-plugin-process 重启应用
    app.restart();
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

/// 测试第三方/自定义供应商端点的网络延迟
#[tauri::command]
pub async fn test_api_endpoints(
    urls: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<Vec<speedtest::EndpointLatency>, String> {
    let filtered: Vec<String> = urls
        .into_iter()
        .filter(|url| !url.trim().is_empty())
        .collect();
    speedtest::test_endpoints(filtered, timeout_secs).await
}

/// 获取自定义端点列表
#[tauri::command]
pub async fn get_custom_endpoints(
    state: State<'_, crate::store::AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    provider_id: Option<String>,
    providerId: Option<String>,
) -> Result<Vec<crate::settings::CustomEndpoint>, String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);
    let provider_id = provider_id
        .or(providerId)
        .ok_or_else(|| "缺少 providerId".to_string())?;
    let mut cfg_guard = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;

    let manager = cfg_guard
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    let Some(provider) = manager.providers.get_mut(&provider_id) else {
        return Ok(vec![]);
    };

    // 首选从 provider.meta 读取
    let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
    if !meta.custom_endpoints.is_empty() {
        let mut result: Vec<_> = meta.custom_endpoints.values().cloned().collect();
        result.sort_by(|a, b| b.added_at.cmp(&a.added_at));
        return Ok(result);
    }

    Ok(vec![])
}

/// 添加自定义端点
#[tauri::command]
pub async fn add_custom_endpoint(
    state: State<'_, crate::store::AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    provider_id: Option<String>,
    providerId: Option<String>,
    url: String,
) -> Result<(), String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);
    let provider_id = provider_id
        .or(providerId)
        .ok_or_else(|| "缺少 providerId".to_string())?;
    let normalized = url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err("URL 不能为空".to_string());
    }

    let mut cfg_guard = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let manager = cfg_guard
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    let Some(provider) = manager.providers.get_mut(&provider_id) else {
        return Err("供应商不存在或未选择".to_string());
    };
    let meta = provider.meta.get_or_insert_with(ProviderMeta::default);

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let endpoint = crate::settings::CustomEndpoint {
        url: normalized.clone(),
        added_at: timestamp,
        last_used: None,
    };
    meta.custom_endpoints.insert(normalized, endpoint);
    drop(cfg_guard);
    state.save()?;
    Ok(())
}

/// 删除自定义端点
#[tauri::command]
pub async fn remove_custom_endpoint(
    state: State<'_, crate::store::AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    provider_id: Option<String>,
    providerId: Option<String>,
    url: String,
) -> Result<(), String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);
    let provider_id = provider_id
        .or(providerId)
        .ok_or_else(|| "缺少 providerId".to_string())?;
    let normalized = url.trim().trim_end_matches('/').to_string();

    let mut cfg_guard = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let manager = cfg_guard
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    if let Some(provider) = manager.providers.get_mut(&provider_id) {
        if let Some(meta) = provider.meta.as_mut() {
            meta.custom_endpoints.remove(&normalized);
        }
    }
    drop(cfg_guard);
    state.save()?;
    Ok(())
}

/// 更新端点最后使用时间
#[tauri::command]
pub async fn update_endpoint_last_used(
    state: State<'_, crate::store::AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    provider_id: Option<String>,
    providerId: Option<String>,
    url: String,
) -> Result<(), String> {
    let app_type = app_type
        .or_else(|| app.as_deref().map(|s| s.into()))
        .or_else(|| appType.as_deref().map(|s| s.into()))
        .unwrap_or(AppType::Claude);
    let provider_id = provider_id
        .or(providerId)
        .ok_or_else(|| "缺少 providerId".to_string())?;
    let normalized = url.trim().trim_end_matches('/').to_string();

    let mut cfg_guard = state
        .config
        .lock()
        .map_err(|e| format!("获取锁失败: {}", e))?;
    let manager = cfg_guard
        .get_manager_mut(&app_type)
        .ok_or_else(|| format!("应用类型不存在: {:?}", app_type))?;

    if let Some(provider) = manager.providers.get_mut(&provider_id) {
        if let Some(meta) = provider.meta.as_mut() {
            if let Some(endpoint) = meta.custom_endpoints.get_mut(&normalized) {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;
                endpoint.last_used = Some(timestamp);
            }
        }
    }
    drop(cfg_guard);
    state.save()?;
    Ok(())
}

/// 获取 app_config_dir 覆盖配置 (从 Store)
#[tauri::command]
pub async fn get_app_config_dir_override(app: tauri::AppHandle) -> Result<Option<String>, String> {
    Ok(crate::app_store::get_app_config_dir_from_store(&app)
        .map(|p| p.to_string_lossy().to_string()))
}

/// 设置 app_config_dir 覆盖配置 (到 Store)
#[tauri::command]
pub async fn set_app_config_dir_override(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<bool, String> {
    crate::app_store::set_app_config_dir_to_store(&app, path.as_deref())?;
    Ok(true)
}

// =====================
// Provider Sort Order Management
// =====================

#[derive(serde::Deserialize)]
pub struct ProviderSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: usize,
}

/// Update sort order for multiple providers
#[tauri::command]
pub async fn update_providers_sort_order(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<String>,
    appType: Option<String>,
    updates: Vec<ProviderSortUpdate>,
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

    // Update sort_index for each provider
    for update in updates {
        if let Some(provider) = manager.providers.get_mut(&update.id) {
            provider.sort_index = Some(update.sort_index);
        }
    }

    drop(config);
    state.save()?;

    Ok(true)
}