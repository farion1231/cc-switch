use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::app_config::AppType;
use crate::error::AppError;

/// 自定义端点配置（历史兼容，实际存储在 provider.meta.custom_endpoints）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

/// 应用设置结构
///
/// 存储设备级别设置，保存在本地 `~/.cc-switch/settings.json`，不随数据库同步。
/// 这确保了云同步场景下多设备可以独立运作。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    // ===== 设备级 UI 设置 =====
    #[serde(default = "default_show_in_tray")]
    pub show_in_tray: bool,
    #[serde(default = "default_minimize_to_tray_on_close")]
    pub minimize_to_tray_on_close: bool,
    /// 是否启用 Claude 插件联动
    #[serde(default)]
    pub enable_claude_plugin_integration: bool,
    /// 是否跳过 Claude Code 初次安装确认
    #[serde(default = "default_true")]
    pub skip_claude_onboarding: bool,
    /// 是否开机自启
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    // ===== 设备级目录覆盖 =====
    /// 是否启用 Claude/Codex/Gemini/OpenCode 配置目录覆盖（适用于 WSL 等场景）
    #[serde(default = "default_true")]
    pub enable_config_dir_overrides: bool,
    /// 切换供应商时，是否同步写入默认目录与覆盖目录（用于同时切换 Windows/WSL 配置）
    #[serde(default)]
    pub sync_provider_switch_to_both_config_dirs: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_config_dir: Option<String>,

    // ===== 当前供应商 ID（设备级）=====
    /// 当前 Claude 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude: Option<String>,
    /// 当前 Codex 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_codex: Option<String>,
    /// 当前 Gemini 供应商 ID（本地存储，优先于数据库 is_current）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_gemini: Option<String>,
    /// 当前 OpenCode 供应商 ID（本地存储，对 OpenCode 可能无意义，但保持结构一致）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_opencode: Option<String>,
}

fn default_show_in_tray() -> bool {
    true
}

fn default_minimize_to_tray_on_close() -> bool {
    true
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_in_tray: true,
            minimize_to_tray_on_close: true,
            enable_claude_plugin_integration: false,
            skip_claude_onboarding: true,
            launch_on_startup: false,
            language: None,
            enable_config_dir_overrides: true,
            sync_provider_switch_to_both_config_dirs: false,
            claude_config_dir: None,
            codex_config_dir: None,
            gemini_config_dir: None,
            opencode_config_dir: None,
            current_provider_claude: None,
            current_provider_codex: None,
            current_provider_gemini: None,
            current_provider_opencode: None,
        }
    }
}

impl AppSettings {
    fn settings_path() -> Option<PathBuf> {
        // settings.json 保留用于旧版本迁移和无数据库场景
        crate::paths::home_dir().map(|h| h.join(".cc-switch").join("settings.json"))
    }

    fn normalize_paths(&mut self) {
        self.claude_config_dir = self
            .claude_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.codex_config_dir = self
            .codex_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.gemini_config_dir = self
            .gemini_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.opencode_config_dir = self
            .opencode_config_dir
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        self.language = self
            .language
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| matches!(*s, "en" | "zh" | "ja"))
            .map(|s| s.to_string());
    }

    fn load_from_file() -> Self {
        let Some(path) = Self::settings_path() else {
            return Self::default();
        };
        if let Ok(content) = fs::read_to_string(&path) {
            match serde_json::from_str::<AppSettings>(&content) {
                Ok(mut settings) => {
                    settings.normalize_paths();
                    settings
                }
                Err(err) => {
                    log::warn!(
                        "解析设置文件失败，将使用默认设置。路径: {}, 错误: {}",
                        path.display(),
                        err
                    );
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }
}

fn save_settings_file(settings: &AppSettings) -> Result<(), AppError> {
    let mut normalized = settings.clone();
    normalized.normalize_paths();
    let Some(path) = AppSettings::settings_path() else {
        return Err(AppError::Config("无法获取用户主目录".to_string()));
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json = serde_json::to_string_pretty(&normalized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&path, json).map_err(|e| AppError::io(&path, e))?;
    Ok(())
}

static SETTINGS_STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();

fn settings_store() -> &'static RwLock<AppSettings> {
    SETTINGS_STORE.get_or_init(|| RwLock::new(AppSettings::load_from_file()))
}

fn resolve_override_path(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = crate::paths::home_dir() {
            return home;
        }
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = crate::paths::home_dir() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        if let Some(home) = crate::paths::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

pub fn get_settings() -> AppSettings {
    settings_store()
        .read()
        .unwrap_or_else(|e| {
            log::warn!("设置锁已毒化，使用恢复值: {e}");
            e.into_inner()
        })
        .clone()
}

pub fn update_settings(mut new_settings: AppSettings) -> Result<(), AppError> {
    new_settings.normalize_paths();
    save_settings_file(&new_settings)?;

    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = new_settings;
    Ok(())
}

/// 从文件重新加载设置到内存缓存
/// 用于导入配置等场景，确保内存缓存与文件同步
pub fn reload_settings() -> Result<(), AppError> {
    let fresh_settings = AppSettings::load_from_file();
    let mut guard = settings_store().write().unwrap_or_else(|e| {
        log::warn!("设置锁已毒化，使用恢复值: {e}");
        e.into_inner()
    });
    *guard = fresh_settings;
    Ok(())
}

pub fn get_claude_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    if !settings.enable_config_dir_overrides {
        return None;
    }
    settings
        .claude_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_claude_override_dir_configured() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .claude_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_codex_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    if !settings.enable_config_dir_overrides {
        return None;
    }
    settings
        .codex_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_codex_override_dir_configured() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .codex_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_gemini_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    if !settings.enable_config_dir_overrides {
        return None;
    }
    settings
        .gemini_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_gemini_override_dir_configured() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .gemini_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_opencode_override_dir() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    if !settings.enable_config_dir_overrides {
        return None;
    }
    settings
        .opencode_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn get_opencode_override_dir_configured() -> Option<PathBuf> {
    let settings = settings_store().read().ok()?;
    settings
        .opencode_config_dir
        .as_ref()
        .map(|p| resolve_override_path(p))
}

pub fn config_dir_overrides_enabled() -> bool {
    settings_store()
        .read()
        .map(|s| s.enable_config_dir_overrides)
        .unwrap_or(true)
}

pub fn sync_provider_switch_to_both_config_dirs_enabled() -> bool {
    settings_store()
        .read()
        .map(|s| s.sync_provider_switch_to_both_config_dirs)
        .unwrap_or(false)
}

// ===== 当前供应商管理函数 =====

/// 获取指定应用类型的当前供应商 ID（从本地 settings 读取）
///
/// 这是设备级别的设置，不随数据库同步。
/// 如果本地没有设置，调用者应该 fallback 到数据库的 `is_current` 字段。
pub fn get_current_provider(app_type: &AppType) -> Option<String> {
    let settings = settings_store().read().ok()?;
    match app_type {
        AppType::Claude => settings.current_provider_claude.clone(),
        AppType::Codex => settings.current_provider_codex.clone(),
        AppType::Gemini => settings.current_provider_gemini.clone(),
        AppType::OpenCode => settings.current_provider_opencode.clone(),
    }
}

/// 设置指定应用类型的当前供应商 ID（保存到本地 settings）
///
/// 这是设备级别的设置，不随数据库同步。
/// 传入 `None` 会清除当前供应商设置。
pub fn set_current_provider(app_type: &AppType, id: Option<&str>) -> Result<(), AppError> {
    let mut settings = get_settings();

    match app_type {
        AppType::Claude => settings.current_provider_claude = id.map(|s| s.to_string()),
        AppType::Codex => settings.current_provider_codex = id.map(|s| s.to_string()),
        AppType::Gemini => settings.current_provider_gemini = id.map(|s| s.to_string()),
        AppType::OpenCode => settings.current_provider_opencode = id.map(|s| s.to_string()),
    }

    update_settings(settings)
}

/// 获取有效的当前供应商 ID（验证存在性）
///
/// 逻辑：
/// 1. 从本地 settings 读取当前供应商 ID
/// 2. 验证该 ID 在数据库中存在
/// 3. 如果不存在则清理本地 settings，fallback 到数据库的 is_current
///
/// 这确保了返回的 ID 一定是有效的（在数据库中存在）。
/// 多设备云同步场景下，配置导入后本地 ID 可能失效，此函数会自动修复。
pub fn get_effective_current_provider(
    db: &crate::database::Database,
    app_type: &AppType,
) -> Result<Option<String>, AppError> {
    // 1. 从本地 settings 读取
    if let Some(local_id) = get_current_provider(app_type) {
        // 2. 验证该 ID 在数据库中存在
        let providers = db.get_all_providers(app_type.as_str())?;
        if providers.contains_key(&local_id) {
            // 存在，直接返回
            return Ok(Some(local_id));
        }

        // 3. 不存在，清理本地 settings
        log::warn!(
            "本地 settings 中的供应商 {} ({}) 在数据库中不存在，将清理并 fallback 到数据库",
            local_id,
            app_type.as_str()
        );
        let _ = set_current_provider(app_type, None);
    }

    // Fallback 到数据库的 is_current
    db.get_current_provider(app_type.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    fn test_mutex() -> &'static Mutex<()> {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        MUTEX.get_or_init(|| Mutex::new(()))
    }

    fn ensure_test_home() -> PathBuf {
        let pid = std::process::id();
        let base = std::env::temp_dir().join(format!("cc-switch-settings-test-home-{pid}"));
        if base.exists() {
            let _ = std::fs::remove_dir_all(&base);
        }
        std::fs::create_dir_all(&base).expect("create test home");
        std::env::set_var("HOME", &base);
        #[cfg(windows)]
        std::env::set_var("USERPROFILE", &base);
        base
    }

    fn reset_test_fs(home: &Path) {
        for sub in [".claude", ".codex", ".cc-switch", ".gemini", "wsl"] {
            let path = home.join(sub);
            if path.exists() {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
        let claude_json = home.join(".claude.json");
        if claude_json.exists() {
            let _ = std::fs::remove_file(&claude_json);
        }

        let _ = update_settings(AppSettings::default());
    }

    #[test]
    #[serial]
    fn override_toggle_preserves_configured_paths_and_switches_effective_dir() {
        let _guard = test_mutex().lock().expect("acquire test mutex");

        let home = ensure_test_home();
        reset_test_fs(&home);

        let override_codex_dir = home.join("wsl").join(".codex");
        let override_codex_dir_str = override_codex_dir.to_string_lossy().to_string();

        let default_dir = home.join(".codex");

        let mut settings = AppSettings::default();
        settings.codex_config_dir = Some(override_codex_dir_str.clone());
        settings.enable_config_dir_overrides = false;
        settings.sync_provider_switch_to_both_config_dirs = false;
        update_settings(settings).expect("update settings");

        assert_eq!(
            get_codex_override_dir(),
            None,
            "override disabled should make override dir non-effective"
        );
        assert_eq!(
            get_codex_override_dir_configured(),
            Some(override_codex_dir.clone()),
            "override dir should remain configured even when disabled"
        );

        assert_eq!(
            crate::codex_config::get_codex_auth_path(),
            default_dir.join("auth.json"),
            "override disabled should make default codex dir effective"
        );

        let settings_path = home.join(".cc-switch").join("settings.json");
        let on_disk: serde_json::Value =
            crate::config::read_json_file(&settings_path).expect("read settings.json");
        assert_eq!(
            on_disk.get("codexConfigDir").and_then(|v| v.as_str()),
            Some(override_codex_dir_str.as_str()),
            "settings.json should retain codexConfigDir"
        );
        assert_eq!(
            on_disk
                .get("enableConfigDirOverrides")
                .and_then(|v| v.as_bool()),
            Some(false),
            "settings.json should reflect overrides disabled"
        );

        let mut settings = AppSettings::default();
        settings.codex_config_dir = Some(override_codex_dir_str);
        settings.enable_config_dir_overrides = true;
        settings.sync_provider_switch_to_both_config_dirs = false;
        update_settings(settings).expect("re-enable overrides");

        assert_eq!(
            get_codex_override_dir(),
            Some(override_codex_dir),
            "override enabled should make override dir effective"
        );
    }
}
