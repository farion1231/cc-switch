// Droid 配置文件模块
use std::path::PathBuf;
use crate::config::{read_json_file, write_json_file};
use crate::error::AppError;

/// 获取 Droid 配置目录路径 (~/.factory)
pub fn get_droid_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_droid_override_dir() {
        return custom;
    }

    dirs::home_dir()
        .expect("无法获取用户主目录")
        .join(".factory")
}

/// 获取 Droid settings.json 路径
pub fn get_droid_settings_path() -> PathBuf {
    get_droid_config_dir().join("settings.json")
}

/// 获取 Droid 配置状态
pub fn get_droid_config_status() -> super::config::ConfigStatus {
    let path = get_droid_settings_path();
    super::config::ConfigStatus {
        exists: path.exists(),
        path: path.to_string_lossy().to_string(),
    }
}

/// 读取 Droid settings.json
pub fn read_droid_settings() -> Result<serde_json::Value, AppError> {
    let path = get_droid_settings_path();
    read_json_file(&path)
}

/// 写入 Droid settings.json
#[allow(dead_code)]
pub fn write_droid_settings(settings: &serde_json::Value) -> Result<(), AppError> {
    let path = get_droid_settings_path();
    write_json_file(&path, settings)
}
