use serde_json::Value;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use tauri_plugin_store::StoreExt;

/// Store 中的键名
const STORE_KEY_APP_CONFIG_DIR: &str = "app_config_dir_override";

/// 全局缓存的 AppHandle (在应用启动时设置)
static APP_HANDLE: OnceLock<RwLock<Option<tauri::AppHandle>>> = OnceLock::new();

/// 设置全局 AppHandle
pub fn set_app_handle(handle: tauri::AppHandle) {
    let store = APP_HANDLE.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = store.write() {
        *guard = Some(handle);
    }
}

/// 获取全局 AppHandle
fn get_app_handle() -> Option<tauri::AppHandle> {
    let store = APP_HANDLE.get()?;
    let guard = store.read().ok()?;
    guard.as_ref().cloned()
}

/// 从 Tauri Store 读取 app_config_dir 覆盖配置 (无需 AppHandle 版本)
pub fn get_app_config_dir_override() -> Option<PathBuf> {
    let app = get_app_handle()?;
    get_app_config_dir_from_store(&app)
}

/// 从 Tauri Store 读取 app_config_dir 覆盖配置（公开函数）
pub fn get_app_config_dir_from_store(app: &tauri::AppHandle) -> Option<PathBuf> {
    let store = app.store_builder("app_paths.json").build();

    if let Err(e) = &store {
        log::warn!("无法创建 Store: {}", e);
        return None;
    }

    let store = store.unwrap();

    match store.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return None;
            }

            let path = resolve_path(path_str);

            // 验证路径是否存在
            if !path.exists() {
                log::warn!(
                    "Store 中配置的 app_config_dir 不存在: {:?}\n\
                     将使用默认路径。",
                    path
                );
                return None;
            }

            log::info!("使用 Store 中的 app_config_dir: {:?}", path);
            Some(path)
        }
        Some(_) => {
            log::warn!("Store 中的 {} 类型不正确，应为字符串", STORE_KEY_APP_CONFIG_DIR);
            None
        }
        None => None,
    }
}

/// 写入 app_config_dir 到 Tauri Store
pub fn set_app_config_dir_to_store(
    app: &tauri::AppHandle,
    path: Option<&str>,
) -> Result<(), String> {
    let store = app
        .store_builder("app_paths.json")
        .build()
        .map_err(|e| format!("创建 Store 失败: {}", e))?;

    match path {
        Some(p) => {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                store.set(STORE_KEY_APP_CONFIG_DIR, Value::String(trimmed.to_string()));
                log::info!("已将 app_config_dir 写入 Store: {}", trimmed);
            } else {
                // 空字符串 = 删除配置
                store.delete(STORE_KEY_APP_CONFIG_DIR);
                log::info!("已从 Store 中删除 app_config_dir 配置");
            }
        }
        None => {
            // None = 删除配置
            store.delete(STORE_KEY_APP_CONFIG_DIR);
            log::info!("已从 Store 中删除 app_config_dir 配置");
        }
    }

    store.save().map_err(|e| format!("保存 Store 失败: {}", e))?;

    Ok(())
}

/// 解析路径，支持 ~ 开头的相对路径
fn resolve_path(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

/// 从旧的 settings.json 迁移 app_config_dir 到 Store
pub fn migrate_app_config_dir_from_settings(app: &tauri::AppHandle) -> Result<(), String> {
    // app_config_dir 已从 settings.json 移除，此函数保留但不再执行迁移
    // 如果用户在旧版本设置过 app_config_dir，需要在 Store 中手动配置
    log::info!("app_config_dir 迁移功能已移除，请在设置中重新配置");

    // 确保 Store 初始化正常
    let _ = get_app_config_dir_from_store(app);

    Ok(())
}