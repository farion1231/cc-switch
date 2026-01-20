// unused imports removed
use std::path::PathBuf;

use crate::config::{
    atomic_write, delete_file, sanitize_provider_name, write_json_file, write_text_file,
};
use crate::error::AppError;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// 获取用户主目录，带回退和日志
fn get_home_dir() -> PathBuf {
    crate::paths::home_dir().unwrap_or_else(|| {
        log::warn!("无法获取用户主目录，回退到当前目录");
        PathBuf::from(".")
    })
}

fn get_default_codex_config_dir() -> PathBuf {
    get_home_dir().join(".codex")
}

fn sync_codex_live_to_secondary_dir(primary_dir: &PathBuf, auth: &Value, cfg_text: &str) {
    if !crate::settings::sync_provider_switch_to_both_config_dirs_enabled() {
        return;
    }

    let Some(override_dir) = crate::settings::get_codex_override_dir_configured() else {
        return;
    };

    let default_dir = get_default_codex_config_dir();
    let secondary_dir = if primary_dir == &override_dir {
        default_dir
    } else {
        override_dir
    };

    if secondary_dir == *primary_dir {
        return;
    }

    let secondary_auth_path = secondary_dir.join("auth.json");
    let secondary_config_path = secondary_dir.join("config.toml");

    if let Some(parent) = secondary_auth_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            log::warn!(
                "Failed to create secondary Codex config dir {}: {err}",
                parent.display()
            );
            return;
        }
    }

    let secondary_old_auth = if secondary_auth_path.exists() {
        match fs::read(&secondary_auth_path) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                log::warn!(
                    "Failed to read secondary Codex auth.json for rollback ({}): {err}",
                    secondary_auth_path.display()
                );
                None
            }
        }
    } else {
        None
    };

    if let Err(err) = write_json_file(&secondary_auth_path, auth) {
        log::warn!(
            "Failed to sync Codex auth.json to secondary dir {}: {err}",
            secondary_dir.display()
        );
    } else if let Err(err) = write_text_file(&secondary_config_path, cfg_text) {
        // Rollback auth.json to keep the pair consistent.
        if let Some(bytes) = secondary_old_auth {
            let _ = atomic_write(&secondary_auth_path, &bytes);
        } else {
            let _ = delete_file(&secondary_auth_path);
        }

        log::warn!(
            "Failed to sync Codex config.toml to secondary dir {}: {err}",
            secondary_dir.display()
        );
    }
}

/// 获取 Codex 配置目录路径
pub fn get_codex_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_codex_override_dir() {
        return custom;
    }

    get_default_codex_config_dir()
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// 获取 Codex config.toml 路径
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

/// 获取 Codex 供应商配置文件路径
#[allow(dead_code)]
pub fn get_codex_provider_paths(
    provider_id: &str,
    provider_name: Option<&str>,
) -> (PathBuf, PathBuf) {
    let base_name = provider_name
        .map(sanitize_provider_name)
        .unwrap_or_else(|| sanitize_provider_name(provider_id));

    let auth_path = get_codex_config_dir().join(format!("auth-{base_name}.json"));
    let config_path = get_codex_config_dir().join(format!("config-{base_name}.toml"));

    (auth_path, config_path)
}

/// 删除 Codex 供应商配置文件
#[allow(dead_code)]
pub fn delete_codex_provider_config(
    provider_id: &str,
    provider_name: &str,
) -> Result<(), AppError> {
    let (auth_path, config_path) = get_codex_provider_paths(provider_id, Some(provider_name));

    delete_file(&auth_path).ok();
    delete_file(&config_path).ok();

    Ok(())
}

/// 原子写 Codex 的 `auth.json` 与 `config.toml`，在第二步失败时回滚第一步
pub fn write_codex_live_atomic(
    auth: &Value,
    config_text_opt: Option<&str>,
) -> Result<(), AppError> {
    let primary_dir = get_codex_config_dir();
    let auth_path = primary_dir.join("auth.json");
    let config_path = primary_dir.join("config.toml");

    if let Some(parent) = auth_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 读取旧内容用于回滚
    let old_auth = if auth_path.exists() {
        Some(fs::read(&auth_path).map_err(|e| AppError::io(&auth_path, e))?)
    } else {
        None
    };
    let _old_config = if config_path.exists() {
        Some(fs::read(&config_path).map_err(|e| AppError::io(&config_path, e))?)
    } else {
        None
    };

    // 准备写入内容
    let cfg_text = match config_text_opt {
        Some(s) => s.to_string(),
        None => String::new(),
    };
    if !cfg_text.trim().is_empty() {
        toml::from_str::<toml::Table>(&cfg_text).map_err(|e| AppError::toml(&config_path, e))?;
    }

    // 第一步：写 auth.json
    write_json_file(&auth_path, auth)?;

    // 第二步：写 config.toml（失败则回滚 auth.json）
    if let Err(e) = write_text_file(&config_path, &cfg_text) {
        // 回滚 auth.json
        if let Some(bytes) = old_auth {
            let _ = atomic_write(&auth_path, &bytes);
        } else {
            let _ = delete_file(&auth_path);
        }
        return Err(e);
    }

    sync_codex_live_to_secondary_dir(&primary_dir, auth, &cfg_text);

    Ok(())
}

/// 读取 `~/.codex/config.toml`，若不存在返回空字符串
pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

/// 对非空的 TOML 文本进行语法校验
pub fn validate_config_toml(text: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    toml::from_str::<toml::Table>(text)
        .map(|_| ())
        .map_err(|e| AppError::toml(Path::new("config.toml"), e))
}

/// 读取并校验 `~/.codex/config.toml`，返回文本（可能为空）
pub fn read_and_validate_codex_config_text() -> Result<String, AppError> {
    let s = read_codex_config_text()?;
    validate_config_toml(&s)?;
    Ok(s)
}
