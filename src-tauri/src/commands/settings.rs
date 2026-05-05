#![allow(non_snake_case)]

use tauri::AppHandle;

fn merge_settings_for_save(
    mut incoming: crate::settings::AppSettings,
    existing: &crate::settings::AppSettings,
) -> crate::settings::AppSettings {
    match (&mut incoming.webdav_sync, &existing.webdav_sync) {
        // incoming 没有 webdav → 保留现有
        (None, _) => {
            incoming.webdav_sync = existing.webdav_sync.clone();
        }
        // incoming 有 webdav 但密码为空，且现有有密码 → 填回现有密码
        // （get_settings_for_frontend 总是清空密码，所以通过 save_settings
        //   传入的空密码意味着"保持现有"而非"用户主动清空"）
        (Some(incoming_sync), Some(existing_sync))
            if incoming_sync.password.is_empty() && !existing_sync.password.is_empty() =>
        {
            incoming_sync.password = existing_sync.password.clone();
        }
        _ => {}
    }
    incoming
}

/// 获取设置
#[tauri::command]
pub async fn get_settings() -> Result<crate::settings::AppSettings, String> {
    Ok(crate::settings::get_settings_for_frontend())
}

/// 保存设置
#[tauri::command]
pub async fn save_settings(settings: crate::settings::AppSettings) -> Result<bool, String> {
    let existing = crate::settings::get_settings();
    let merged = merge_settings_for_save(settings, &existing);
    crate::settings::update_settings(merged).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 重启应用程序（当 app_config_dir 变更后使用）
#[tauri::command]
pub async fn restart_app(app: AppHandle) -> Result<bool, String> {
    crate::save_window_state_before_exit(&app);

    // 在后台延迟重启，让函数有时间返回响应
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        app.restart();
    });
    Ok(true)
}

/// 获取 app_config_dir 覆盖配置 (从 Store)
#[tauri::command]
pub async fn get_app_config_dir_override(app: AppHandle) -> Result<Option<String>, String> {
    Ok(crate::app_store::refresh_app_config_dir_override(&app)
        .map(|p| p.to_string_lossy().to_string()))
}

/// 设置 app_config_dir 覆盖配置 (到 Store)
#[tauri::command]
pub async fn set_app_config_dir_override(
    app: AppHandle,
    path: Option<String>,
) -> Result<bool, String> {
    crate::app_store::set_app_config_dir_to_store(&app, path.as_deref())?;
    Ok(true)
}

/// 设置开机自启
#[tauri::command]
pub async fn set_auto_launch(enabled: bool) -> Result<bool, String> {
    if enabled {
        crate::auto_launch::enable_auto_launch().map_err(|e| format!("启用开机自启失败: {e}"))?;
    } else {
        crate::auto_launch::disable_auto_launch().map_err(|e| format!("禁用开机自启失败: {e}"))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::merge_settings_for_save;
    use crate::settings::{AppSettings, WebDavSyncSettings};

    #[test]
    fn save_settings_should_preserve_existing_webdav_when_payload_omits_it() {
        let mut existing = AppSettings::default();
        existing.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        });

        let incoming = AppSettings::default();
        let merged = merge_settings_for_save(incoming, &existing);

        assert!(merged.webdav_sync.is_some());
        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.base_url.as_str()),
            Some("https://dav.example.com")
        );
    }

    #[test]
    fn save_settings_should_keep_incoming_webdav_when_present() {
        let mut existing = AppSettings::default();
        existing.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.old.example.com".to_string(),
            username: "old".to_string(),
            password: "old-pass".to_string(),
            ..WebDavSyncSettings::default()
        });

        let mut incoming = AppSettings::default();
        incoming.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.new.example.com".to_string(),
            username: "new".to_string(),
            password: "new-pass".to_string(),
            ..WebDavSyncSettings::default()
        });

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.base_url.as_str()),
            Some("https://dav.new.example.com")
        );
    }

    /// Regression test: frontend always receives empty password from
    /// get_settings_for_frontend(). If a component accidentally spreads
    /// the full settings object into save_settings, the empty password
    /// must NOT overwrite the existing one.
    #[test]
    fn save_settings_should_preserve_password_when_incoming_has_empty_password() {
        let mut existing = AppSettings::default();
        existing.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        });

        // Simulate frontend sending settings with cleared password
        let mut incoming = AppSettings::default();
        incoming.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "".to_string(),
            ..WebDavSyncSettings::default()
        });

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.password.as_str()),
            Some("secret"),
            "empty password from frontend must not overwrite existing password"
        );
    }

    /// When both incoming and existing have no password, merge should
    /// work without panicking and keep the empty state.
    #[test]
    fn save_settings_should_handle_both_empty_passwords() {
        let mut existing = AppSettings::default();
        existing.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "".to_string(),
            ..WebDavSyncSettings::default()
        });

        let mut incoming = AppSettings::default();
        incoming.webdav_sync = Some(WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: "".to_string(),
            ..WebDavSyncSettings::default()
        });

        let merged = merge_settings_for_save(incoming, &existing);

        assert_eq!(
            merged.webdav_sync.as_ref().map(|v| v.password.as_str()),
            Some("")
        );
    }
}

/// 获取开机自启状态
#[tauri::command]
pub async fn get_auto_launch_status() -> Result<bool, String> {
    crate::auto_launch::is_auto_launch_enabled().map_err(|e| format!("获取开机自启状态失败: {e}"))
}

/// 获取整流器配置
#[tauri::command]
pub async fn get_rectifier_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::RectifierConfig, String> {
    state.db.get_rectifier_config().map_err(|e| e.to_string())
}

/// 设置整流器配置
#[tauri::command]
pub async fn set_rectifier_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::RectifierConfig,
) -> Result<bool, String> {
    state
        .db
        .set_rectifier_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取优化器配置
#[tauri::command]
pub async fn get_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::OptimizerConfig, String> {
    state.db.get_optimizer_config().map_err(|e| e.to_string())
}

/// 设置优化器配置
#[tauri::command]
pub async fn set_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::OptimizerConfig,
) -> Result<bool, String> {
    // Validate cache_ttl: only allow known values
    match config.cache_ttl.as_str() {
        "5m" | "1h" => {}
        other => {
            return Err(format!(
                "Invalid cache_ttl value: '{other}'. Allowed values: '5m', '1h'"
            ))
        }
    }
    state
        .db
        .set_optimizer_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取 Copilot 优化器配置
#[tauri::command]
pub async fn get_copilot_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::CopilotOptimizerConfig, String> {
    state
        .db
        .get_copilot_optimizer_config()
        .map_err(|e| e.to_string())
}

/// 设置 Copilot 优化器配置
#[tauri::command]
pub async fn set_copilot_optimizer_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::CopilotOptimizerConfig,
) -> Result<bool, String> {
    state
        .db
        .set_copilot_optimizer_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 获取日志配置
#[tauri::command]
pub async fn get_log_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::LogConfig, String> {
    state.db.get_log_config().map_err(|e| e.to_string())
}

/// 获取敏感词过滤配置
#[tauri::command]
pub async fn get_sensitive_word_config(
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::proxy::types::SensitiveWordConfig, String> {
    state.db.get_sensitive_word_config().map_err(|e| e.to_string())
}

/// 设置敏感词过滤配置
#[tauri::command]
pub async fn set_sensitive_word_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::SensitiveWordConfig,
) -> Result<bool, String> {
    state
        .db
        .set_sensitive_word_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitiveWordCacheInfo {
    pub loaded: bool,
    pub file_path: String,
    pub words: Vec<String>,
    pub modified_at: Option<String>,
}

/// 重新加载敏感词文件到缓存
#[tauri::command]
pub async fn reload_sensitive_word_cache(
    state: tauri::State<'_, crate::AppState>,
) -> Result<SensitiveWordCacheInfo, String> {
    let config = state.db.get_sensitive_word_config().map_err(|e| e.to_string())?;

    if config.file_path.trim().is_empty() {
        return Err("请先选择敏感词文件".to_string());
    }

    let words = crate::proxy::sensitive_word_filter::reload_sensitive_words(&config.file_path)?;
    let modified_at = std::fs::metadata(&config.file_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(format_system_time);

    Ok(SensitiveWordCacheInfo {
        loaded: true,
        file_path: config.file_path,
        words,
        modified_at,
    })
}

/// 获取当前敏感词缓存详情
#[tauri::command]
pub async fn get_sensitive_word_cache_info() -> Result<SensitiveWordCacheInfo, String> {
    Ok(
        match crate::proxy::sensitive_word_filter::get_sensitive_word_cache() {
            Some((file_path, words, modified_time)) => SensitiveWordCacheInfo {
                loaded: true,
                file_path,
                words,
                modified_at: modified_time.map(format_system_time),
            },
            None => SensitiveWordCacheInfo {
                loaded: false,
                file_path: String::new(),
                words: Vec::new(),
                modified_at: None,
            },
        },
    )
}

fn format_system_time(time: std::time::SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 设置日志配置
#[tauri::command]
pub async fn set_log_config(
    state: tauri::State<'_, crate::AppState>,
    config: crate::proxy::types::LogConfig,
) -> Result<bool, String> {
    state
        .db
        .set_log_config(&config)
        .map_err(|e| e.to_string())?;
    log::set_max_level(config.to_level_filter());
    log::info!(
        "日志配置已更新: enabled={}, level={}",
        config.enabled,
        config.level
    );
    Ok(true)
}
