//! Hermes Agent 配置文件读写模块
//!
//! 处理 `~/.hermes/config.yaml` 配置文件的读写操作（YAML 格式）。
//! Hermes 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。

use crate::settings::get_hermes_override_dir;
use std::path::PathBuf;

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 Hermes 配置目录
///
/// 默认路径: `~/.hermes/`
/// 可通过 settings.hermes_config_dir 覆盖
pub fn get_hermes_dir() -> PathBuf {
    if let Some(override_dir) = get_hermes_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir().join(".hermes")
}

/// 获取 Hermes 配置文件路径
///
/// 返回 `~/.hermes/config.yaml`
pub fn get_hermes_config_path() -> PathBuf {
    get_hermes_dir().join("config.yaml")
}

