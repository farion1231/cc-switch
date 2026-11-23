use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// Store 中的键名
const STORE_KEY_APP_CONFIG_DIR: &str = "app_config_dir_override";
const APP_PATHS_STORE: &str = "app_paths.json";

/// 缓存当前的 app_config_dir 覆盖路径，避免存储 AppHandle
static APP_CONFIG_DIR_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn override_cache() -> &'static RwLock<Option<PathBuf>> {
    APP_CONFIG_DIR_OVERRIDE.get_or_init(|| RwLock::new(None))
}

fn update_cached_override(value: Option<PathBuf>) {
    if let Ok(mut guard) = override_cache().write() {
        *guard = value;
    }
}

/// 获取缓存中的 app_config_dir 覆盖路径
pub fn get_app_config_dir_override() -> Option<PathBuf> {
    override_cache().read().ok()?.clone()
}

fn read_override_from_store(app: &tauri::AppHandle) -> Option<PathBuf> {
    let store = match app.store_builder(APP_PATHS_STORE).build() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("无法创建 Store: {e}");
            return None;
        }
    };

    resolve_override(store.get(STORE_KEY_APP_CONFIG_DIR))
}

fn resolve_override(value: Option<Value>) -> Option<PathBuf> {
    match value {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return None;
            }

            let path = resolve_path(path_str);

            if !path.exists() {
                log::warn!(
                    "Store 中配置的 app_config_dir 不存在: {path:?}\n\
                     将使用默认路径。"
                );
                return None;
            }

            log::info!("使用 Store 中的 app_config_dir: {path:?}");
            Some(path)
        }
        Some(_) => {
            log::warn!("Store 中的 {STORE_KEY_APP_CONFIG_DIR} 类型不正确，应为字符串");
            None
        }
        None => None,
    }
}

/// 从 Store 刷新 app_config_dir 覆盖值并更新缓存
pub fn refresh_app_config_dir_override(app: &tauri::AppHandle) -> Option<PathBuf> {
    let value = read_override_from_store(app);
    update_cached_override(value.clone());
    value
}

/// The store plugin resolves relative filenames under Tauri's AppData directory.
/// Read the same JSON without creating a Tauri application for CLI commands.
pub(crate) fn refresh_app_config_dir_override_headless() -> Result<(), AppError> {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|error| AppError::json("tauri.conf.json", error))?;
    let identifier = config["identifier"]
        .as_str()
        .ok_or_else(|| AppError::Config("Missing CC Switch application identifier".into()))?;
    let directory = dirs::data_dir()
        .ok_or_else(|| {
            AppError::Config("Cannot resolve the CC Switch application data directory".into())
        })?
        .join(identifier);
    refresh_app_config_dir_override_from_file(&directory.join(APP_PATHS_STORE))
}

fn refresh_app_config_dir_override_from_file(path: &Path) -> Result<(), AppError> {
    let mut values: serde_json::Map<String, Value> = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| AppError::json(path, error))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    update_cached_override(resolve_override(values.remove(STORE_KEY_APP_CONFIG_DIR)));
    Ok(())
}

/// 写入 app_config_dir 到 Tauri Store
pub fn set_app_config_dir_to_store(
    app: &tauri::AppHandle,
    path: Option<&str>,
) -> Result<(), AppError> {
    let store = app
        .store_builder(APP_PATHS_STORE)
        .build()
        .map_err(|e| AppError::Message(format!("创建 Store 失败: {e}")))?;

    match path {
        Some(p) => {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                store.set(STORE_KEY_APP_CONFIG_DIR, Value::String(trimmed.to_string()));
                log::info!("已将 app_config_dir 写入 Store: {trimmed}");
            } else {
                store.delete(STORE_KEY_APP_CONFIG_DIR);
                log::info!("已从 Store 中删除 app_config_dir 配置");
            }
        }
        None => {
            store.delete(STORE_KEY_APP_CONFIG_DIR);
            log::info!("已从 Store 中删除 app_config_dir 配置");
        }
    }

    store
        .save()
        .map_err(|e| AppError::Message(format!("保存 Store 失败: {e}")))?;

    refresh_app_config_dir_override(app);
    Ok(())
}

/// 解析路径，支持 ~ 开头的相对路径
fn resolve_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return crate::config::get_home_dir().join(stripped);
    }

    PathBuf::from(raw)
}

/// 从旧的 settings.json 迁移 app_config_dir 到 Store
pub fn migrate_app_config_dir_from_settings(app: &tauri::AppHandle) -> Result<(), AppError> {
    // app_config_dir 已从 settings.json 移除，此函数保留但不再执行迁移
    // 如果用户在旧版本设置过 app_config_dir，需要在 Store 中手动配置
    log::info!("app_config_dir 迁移功能已移除，请在设置中重新配置");

    let _ = refresh_app_config_dir_override(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        get_app_config_dir_override, refresh_app_config_dir_override_from_file, resolve_path,
        update_cached_override, PathBuf, APP_PATHS_STORE,
    };
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::test_support::EnvVars;

    #[test]
    #[serial_test::serial]
    fn resolve_path_honors_test_home_override() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvVars::new().set("CC_SWITCH_TEST_HOME", temp.path());

        assert_eq!(resolve_path("~"), temp.path());
        assert_eq!(resolve_path("~/custom/dir"), temp.path().join("custom/dir"));
        assert_eq!(
            resolve_path(r"~\custom\dir"),
            temp.path().join(r"custom\dir")
        );
    }

    #[test]
    #[serial_test::serial]
    fn headless_store_override_selects_the_configured_database() {
        struct RestoreOverride(Option<PathBuf>);
        impl Drop for RestoreOverride {
            fn drop(&mut self) {
                update_cached_override(self.0.take());
            }
        }
        let _restore = RestoreOverride(get_app_config_dir_override());
        let home = tempfile::tempdir().unwrap();
        let _env = EnvVars::new().set("CC_SWITCH_TEST_HOME", home.path());
        let configured = home.path().join("configured");
        std::fs::create_dir(&configured).unwrap();
        let store = home.path().join(APP_PATHS_STORE);
        std::fs::write(&store, r#"{"app_config_dir_override":" ~/configured "}"#).unwrap();
        refresh_app_config_dir_override_from_file(&store).unwrap();
        assert_eq!(crate::config::get_app_config_dir(), configured);

        let db = Database::init().unwrap();
        let provider = Provider::with_id(
            "override-fixture".into(),
            "Configured provider".into(),
            serde_json::json!({}),
            None,
        );
        db.save_provider(&AppType::Claude, &provider).unwrap();
        let cli = Database::open_for_provider_bundle(false).unwrap();
        assert!(cli
            .get_provider_by_id("override-fixture", &AppType::Claude)
            .unwrap()
            .is_some());
        assert!(!home.path().join(".cc-switch/cc-switch.db").exists());
    }
}
