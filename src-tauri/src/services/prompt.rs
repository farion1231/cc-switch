use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::prompt_files::{prompt_file_path, prompt_file_path_for_db};
use crate::store::AppState;

/// 安全地获取当前 Unix 时间戳
fn get_unix_timestamp() -> Result<i64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| AppError::Message(format!("Failed to get system time: {e}")))
}

pub struct PromptService;

impl PromptService {
    pub fn get_prompts(
        state: &AppState,
        app: AppType,
    ) -> Result<IndexMap<String, Prompt>, AppError> {
        state.db.get_prompts(app.as_str())
    }

    pub fn upsert_prompt(
        state: &AppState,
        app: AppType,
        _id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        // 检查是否为已启用的提示词
        let is_enabled = prompt.enabled;

        state.db.save_prompt(app.as_str(), &prompt)?;

        if is_enabled {
            // 启用提示词：写入内容到文件
            Self::write_prompt_files_for_app(state, &app, &prompt.content)?;
        } else {
            // 禁用提示词：检查是否还有其他已启用的提示词
            let prompts = state.db.get_prompts(app.as_str())?;
            let any_enabled = prompts.values().any(|p| p.enabled);

            if !any_enabled {
                // 所有提示词都已禁用，清空文件
                Self::clear_prompt_files_for_app(state, &app)?;
            }
        }

        Ok(())
    }

    fn write_prompt_files_for_app(
        state: &AppState,
        app: &AppType,
        content: &str,
    ) -> Result<(), AppError> {
        let target_path = prompt_file_path_for_db(state.db.as_ref(), app)?;
        write_text_file(&target_path, content)?;

        if matches!(app, AppType::Claude) {
            let configured_path = prompt_file_path(app)?;
            if configured_path != target_path {
                write_text_file(&configured_path, content)?;
            }
        }

        Ok(())
    }

    fn clear_prompt_files_for_app(state: &AppState, app: &AppType) -> Result<(), AppError> {
        let target_path = prompt_file_path_for_db(state.db.as_ref(), app)?;
        if target_path.exists() {
            write_text_file(&target_path, "")?;
        }

        if matches!(app, AppType::Claude) {
            let configured_path = prompt_file_path(app)?;
            if configured_path != target_path && configured_path.exists() {
                write_text_file(&configured_path, "")?;
            }
        }

        Ok(())
    }

    pub fn delete_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        let prompts = state.db.get_prompts(app.as_str())?;

        if let Some(prompt) = prompts.get(id) {
            if prompt.enabled {
                return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
            }
        }

        state.db.delete_prompt(app.as_str(), id)?;
        Ok(())
    }

    pub fn enable_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        // 回填当前 live 文件内容到已启用的提示词，或创建备份
        let target_path = prompt_file_path_for_db(state.db.as_ref(), &app)?;
        if target_path.exists() {
            if let Ok(live_content) = std::fs::read_to_string(&target_path) {
                if !live_content.trim().is_empty() {
                    let mut prompts = state.db.get_prompts(app.as_str())?;

                    // 尝试回填到当前已启用的提示词
                    if let Some((enabled_id, enabled_prompt)) = prompts
                        .iter_mut()
                        .find(|(_, p)| p.enabled)
                        .map(|(id, p)| (id.clone(), p))
                    {
                        let timestamp = get_unix_timestamp()?;
                        enabled_prompt.content = live_content.clone();
                        enabled_prompt.updated_at = Some(timestamp);
                        log::info!("回填 live 提示词内容到已启用项: {enabled_id}");
                        state.db.save_prompt(app.as_str(), enabled_prompt)?;
                    } else {
                        // 没有已启用的提示词，则创建一次备份（避免重复备份）
                        let content_exists = prompts
                            .values()
                            .any(|p| p.content.trim() == live_content.trim());
                        if !content_exists {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            let backup_id = format!("backup-{timestamp}");
                            let backup_prompt = Prompt {
                                id: backup_id.clone(),
                                name: format!(
                                    "原始提示词 {}",
                                    chrono::Local::now().format("%Y-%m-%d %H:%M")
                                ),
                                content: live_content,
                                description: Some("自动备份的原始提示词".to_string()),
                                enabled: false,
                                created_at: Some(timestamp),
                                updated_at: Some(timestamp),
                            };
                            log::info!("回填 live 提示词内容，创建备份: {backup_id}");
                            state.db.save_prompt(app.as_str(), &backup_prompt)?;
                        }
                    }
                }
            }
        }

        // 启用目标提示词并写入文件
        let mut prompts = state.db.get_prompts(app.as_str())?;

        for prompt in prompts.values_mut() {
            prompt.enabled = false;
        }

        if let Some(prompt) = prompts.get_mut(id) {
            prompt.enabled = true;
            Self::write_prompt_files_for_app(state, &app, &prompt.content)?;
            state.db.save_prompt(app.as_str(), prompt)?;
        } else {
            return Err(AppError::InvalidInput(format!("提示词 {id} 不存在")));
        }

        // Save all prompts to disable others
        for (_, prompt) in prompts.iter() {
            state.db.save_prompt(app.as_str(), prompt)?;
        }

        Ok(())
    }

    pub fn import_from_file(state: &AppState, app: AppType) -> Result<String, AppError> {
        let file_path = prompt_file_path_for_db(state.db.as_ref(), &app)?;

        if !file_path.exists() {
            return Err(AppError::Message("提示词文件不存在".to_string()));
        }

        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        let timestamp = get_unix_timestamp()?;

        let id = format!("imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "导入的提示词 {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("从现有配置文件导入".to_string()),
            enabled: false,
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        Self::upsert_prompt(state, app, &id, prompt)?;
        Ok(id)
    }

    pub fn get_current_file_content(
        state: &AppState,
        app: AppType,
    ) -> Result<Option<String>, AppError> {
        let file_path = prompt_file_path_for_db(state.db.as_ref(), &app)?;
        if !file_path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        Ok(Some(content))
    }

    /// 首次启动时从现有提示词文件自动导入（如果存在）
    /// 返回导入的数量
    pub fn import_from_file_on_first_launch(
        state: &AppState,
        app: AppType,
    ) -> Result<usize, AppError> {
        // 幂等性保护：该应用已有提示词则跳过
        let existing = state.db.get_prompts(app.as_str())?;
        if !existing.is_empty() {
            return Ok(0);
        }

        let file_path = prompt_file_path_for_db(state.db.as_ref(), &app)?;

        // 检查文件是否存在
        if !file_path.exists() {
            return Ok(0);
        }

        // 读取文件内容
        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("读取提示词文件失败: {file_path:?}, 错误: {e}");
                return Ok(0);
            }
        };

        // 检查内容是否为空
        if content.trim().is_empty() {
            return Ok(0);
        }

        log::info!("发现提示词文件，自动导入: {file_path:?}");

        // 创建提示词对象
        let timestamp = get_unix_timestamp()?;
        let id = format!("auto-imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "Auto-imported Prompt {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("Automatically imported on first launch".to_string()),
            enabled: true, // 首次导入时自动启用
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        // 保存到数据库
        state.db.save_prompt(app.as_str(), &prompt)?;

        log::info!("自动导入完成: {}", app.as_str());
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{ClaudeActivationMode, Provider, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct TempHome {
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }

        fn path(&self) -> &std::path::Path {
            self.dir.path()
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = crate::settings::update_settings(crate::settings::AppSettings::default());
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    fn claude_provider_with_profile(
        id: &str,
        profile_dir: &std::path::Path,
        activation_mode: ClaudeActivationMode,
    ) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Provider {id}"),
            settings_config: json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "test-token",
                    "ANTHROPIC_BASE_URL": "https://api.example.com"
                }
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: Some(0),
            notes: None,
            meta: Some(ProviderMeta {
                claude_profile_dir: Some(profile_dir.to_string_lossy().to_string()),
                claude_activation_mode: Some(activation_mode),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    #[serial]
    fn enable_prompt_writes_to_current_profile_and_configured_claude_paths() {
        let home = TempHome::new();
        let configured_claude_dir = home.path().join("configured-claude");
        let profile_dir = home.path().join("profile-and-config");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(configured_claude_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set configured claude dir");

        let db = Arc::new(Database::memory().expect("memory db"));
        let state = AppState::new(db.clone());
        let provider = claude_provider_with_profile(
            "profile-and-config",
            &profile_dir,
            ClaudeActivationMode::ProfileAndConfig,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        db.set_current_provider(AppType::Claude.as_str(), "profile-and-config")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("profile-and-config"))
            .expect("set local current provider");

        let prompt = Prompt {
            id: "prompt-1".to_string(),
            name: "Prompt 1".to_string(),
            content: "profile prompt".to_string(),
            description: None,
            enabled: false,
            created_at: Some(1),
            updated_at: None,
        };
        db.save_prompt(AppType::Claude.as_str(), &prompt)
            .expect("save prompt");
        let configured_prompt_path = configured_claude_dir.join("CLAUDE.md");
        std::fs::create_dir_all(configured_prompt_path.parent().unwrap()).expect("configured dir");
        std::fs::write(&configured_prompt_path, "stale configured prompt")
            .expect("seed stale configured prompt");

        PromptService::enable_prompt(&state, AppType::Claude, "prompt-1").expect("enable prompt");

        assert_eq!(
            std::fs::read_to_string(profile_dir.join("CLAUDE.md")).expect("profile prompt file"),
            "profile prompt"
        );
        assert_eq!(
            std::fs::read_to_string(&configured_prompt_path).expect("configured prompt file"),
            "profile prompt",
            "profile-and-config prompt sync should mirror the configured/default Claude file"
        );
    }

    #[test]
    #[serial]
    fn disabling_last_claude_prompt_clears_profile_and_configured_files() {
        let home = TempHome::new();
        let configured_claude_dir = home.path().join("configured-claude");
        let profile_dir = home.path().join("profile-and-config");

        crate::settings::update_settings(crate::settings::AppSettings {
            claude_config_dir: Some(configured_claude_dir.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .expect("set configured claude dir");

        let db = Arc::new(Database::memory().expect("memory db"));
        let state = AppState::new(db.clone());
        let provider = claude_provider_with_profile(
            "profile-and-config",
            &profile_dir,
            ClaudeActivationMode::ProfileAndConfig,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider");
        db.set_current_provider(AppType::Claude.as_str(), "profile-and-config")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("profile-and-config"))
            .expect("set local current provider");

        let profile_prompt_path = profile_dir.join("CLAUDE.md");
        let configured_prompt_path = configured_claude_dir.join("CLAUDE.md");
        std::fs::create_dir_all(profile_prompt_path.parent().unwrap()).expect("profile dir");
        std::fs::create_dir_all(configured_prompt_path.parent().unwrap()).expect("configured dir");
        std::fs::write(&profile_prompt_path, "profile stale prompt").expect("profile prompt");
        std::fs::write(&configured_prompt_path, "configured stale prompt")
            .expect("configured prompt");

        let prompt = Prompt {
            id: "prompt-1".to_string(),
            name: "Prompt 1".to_string(),
            content: "disabled prompt".to_string(),
            description: None,
            enabled: false,
            created_at: Some(1),
            updated_at: None,
        };

        PromptService::upsert_prompt(&state, AppType::Claude, "prompt-1", prompt)
            .expect("disable prompt");

        assert_eq!(
            std::fs::read_to_string(&profile_prompt_path).expect("profile prompt file"),
            "",
            "active profile prompt file should be cleared"
        );
        assert_eq!(
            std::fs::read_to_string(&configured_prompt_path).expect("configured prompt file"),
            "",
            "configured/default Claude prompt file should also be cleared"
        );
    }
}
