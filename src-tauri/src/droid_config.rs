// Droid 配置文件模块
//
// Droid 配置机制说明：
// - 主配置文件: ~/.factory/config.json (用户编辑此文件)
// - 运行时配置: ~/.factory/settings.json (重启后从 config.json 同步)
// - config.json 支持多行格式
//
// 让配置生效的步骤：
// 1. 编辑 config.json 添加自定义模型
// 2. 在 settings.json 中删除 "customModels": [] 里的空列表
// 3. 删除 "sessionDefaultSettings": {"model": "..."} 中的 model key
// 4. 重启 Droid
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

/// 获取 Droid config.json 路径 (主配置文件，用户编辑此文件)
pub fn get_droid_config_path() -> PathBuf {
    get_droid_config_dir().join("config.json")
}

/// 获取 Droid settings.json 路径 (运行时配置，从 config.json 同步)
pub fn get_droid_settings_path() -> PathBuf {
    get_droid_config_dir().join("settings.json")
}

/// 获取 Droid 配置状态 (检查 config.json)
pub fn get_droid_config_status() -> super::config::ConfigStatus {
    let path = get_droid_config_path();
    super::config::ConfigStatus {
        exists: path.exists(),
        path: path.to_string_lossy().to_string(),
    }
}

/// 读取 Droid config.json (主配置文件)
pub fn read_droid_config() -> Result<serde_json::Value, AppError> {
    let path = get_droid_config_path();
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    read_json_file(&path)
}

/// 写入 Droid config.json (主配置文件)
pub fn write_droid_config(config: &serde_json::Value) -> Result<(), AppError> {
    let path = get_droid_config_path();
    write_json_file(&path, config)
}

/// 读取 Droid settings.json (运行时配置)
pub fn read_droid_settings() -> Result<serde_json::Value, AppError> {
    let path = get_droid_settings_path();
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    read_json_file(&path)
}

/// 写入 Droid settings.json (运行时配置)
#[allow(dead_code)]
pub fn write_droid_settings(settings: &serde_json::Value) -> Result<(), AppError> {
    let path = get_droid_settings_path();
    write_json_file(&path, settings)
}

/// 清理 settings.json 中阻止新配置生效的字段
/// 删除 customModels 空列表和 sessionDefaultSettings.model
pub fn cleanup_settings_for_new_config() -> Result<(), AppError> {
    let path = get_droid_settings_path();
    if !path.exists() {
        return Ok(());
    }

    let mut settings = read_droid_settings()?;
    let mut modified = false;

    // 删除空的 customModels 列表
    if let Some(obj) = settings.as_object_mut() {
        if let Some(custom_models) = obj.get("customModels") {
            if custom_models.as_array().map(|a| a.is_empty()).unwrap_or(false) {
                obj.remove("customModels");
                modified = true;
                log::info!("已从 settings.json 删除空的 customModels");
            }
        }

        // 删除 sessionDefaultSettings.model
        if let Some(session_settings) = obj.get_mut("sessionDefaultSettings") {
            if let Some(session_obj) = session_settings.as_object_mut() {
                if session_obj.remove("model").is_some() {
                    modified = true;
                    log::info!("已从 settings.json 删除 sessionDefaultSettings.model");
                }
            }
        }
    }

    if modified {
        write_droid_settings(&settings)?;
    }

    Ok(())
}
