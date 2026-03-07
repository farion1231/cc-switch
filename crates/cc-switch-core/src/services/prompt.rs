//! Prompt service - business logic for prompt management.

use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::prompt_files::prompt_file_path;
use crate::store::AppState;

fn unix_timestamp() -> Result<i64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|err| AppError::Message(format!("Failed to get system time: {err}")))
}

pub struct PromptService;

impl PromptService {
    pub fn list(state: &AppState, app_type: AppType) -> Result<IndexMap<String, Prompt>, AppError> {
        state.db.get_all_prompts(app_type.as_str())
    }

    pub fn get(state: &AppState, app_type: AppType, id: &str) -> Result<Option<Prompt>, AppError> {
        state.db.get_prompt(app_type.as_str(), id)
    }

    pub fn save(state: &AppState, app_type: AppType, prompt: &Prompt) -> Result<(), AppError> {
        Self::upsert_prompt(state, app_type, &prompt.id, prompt.clone())
    }

    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        Self::delete_prompt(state, app_type, id)
    }

    pub fn enable(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        Self::enable_prompt(state, app_type, id)
    }

    pub fn disable(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let mut prompt = state
            .db
            .get_prompt(app_type.as_str(), id)?
            .ok_or_else(|| AppError::Message(format!("Prompt {id} not found")))?;
        prompt.enabled = false;
        state.db.save_prompt(app_type.as_str(), &prompt)?;

        let prompts = state.db.get_all_prompts(app_type.as_str())?;
        if !prompts.values().any(|item| item.enabled) {
            let target_path = prompt_file_path(&app_type)?;
            if target_path.exists() {
                write_text_file(&target_path, "")?;
            }
        }

        Ok(())
    }

    pub fn import_from_files(state: &AppState, app_type: AppType) -> Result<usize, AppError> {
        Self::import_from_file_on_first_launch(state, app_type)
    }

    pub fn upsert_prompt(
        state: &AppState,
        app_type: AppType,
        _id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        let is_enabled = prompt.enabled;
        state.db.save_prompt(app_type.as_str(), &prompt)?;

        if is_enabled {
            let target_path = prompt_file_path(&app_type)?;
            write_text_file(&target_path, &prompt.content)?;
        } else {
            let prompts = state.db.get_all_prompts(app_type.as_str())?;
            if !prompts.values().any(|item| item.enabled) {
                let target_path = prompt_file_path(&app_type)?;
                if target_path.exists() {
                    write_text_file(&target_path, "")?;
                }
            }
        }

        Ok(())
    }

    pub fn delete_prompt(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let prompts = state.db.get_all_prompts(app_type.as_str())?;
        if let Some(prompt) = prompts.get(id) {
            if prompt.enabled {
                return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
            }
        }

        state.db.delete_prompt(app_type.as_str(), id)
    }

    pub fn enable_prompt(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let target_path = prompt_file_path(&app_type)?;
        if target_path.exists() {
            if let Ok(live_content) = std::fs::read_to_string(&target_path) {
                if !live_content.trim().is_empty() {
                    let mut prompts = state.db.get_all_prompts(app_type.as_str())?;

                    if let Some((enabled_id, enabled_prompt)) = prompts
                        .iter_mut()
                        .find(|(_, prompt)| prompt.enabled)
                        .map(|(prompt_id, prompt)| (prompt_id.clone(), prompt))
                    {
                        let timestamp = unix_timestamp()?;
                        enabled_prompt.content = live_content.clone();
                        enabled_prompt.updated_at = Some(timestamp);
                        log::info!("Backfilled live prompt content to enabled item: {enabled_id}");
                        state.db.save_prompt(app_type.as_str(), enabled_prompt)?;
                    } else {
                        let content_exists = prompts
                            .values()
                            .any(|prompt| prompt.content.trim() == live_content.trim());
                        if !content_exists {
                            let timestamp = unix_timestamp()?;
                            let backup_prompt = Prompt {
                                id: format!("backup-{timestamp}"),
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
                            state.db.save_prompt(app_type.as_str(), &backup_prompt)?;
                        }
                    }
                }
            }
        }

        let mut prompts = state.db.get_all_prompts(app_type.as_str())?;
        for prompt in prompts.values_mut() {
            prompt.enabled = false;
        }

        let target = prompts
            .get_mut(id)
            .ok_or_else(|| AppError::InvalidInput(format!("提示词 {id} 不存在")))?;
        target.enabled = true;
        write_text_file(&target_path, &target.content)?;
        state.db.save_prompt(app_type.as_str(), target)?;

        for prompt in prompts.values() {
            state.db.save_prompt(app_type.as_str(), prompt)?;
        }

        Ok(())
    }

    pub fn import_from_file(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        let file_path = prompt_file_path(&app_type)?;
        if !file_path.exists() {
            return Err(AppError::Message("提示词文件不存在".to_string()));
        }

        let content =
            std::fs::read_to_string(&file_path).map_err(|err| AppError::io(&file_path, err))?;
        let timestamp = unix_timestamp()?;
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

        Self::upsert_prompt(state, app_type, &id, prompt)?;
        Ok(id)
    }

    pub fn get_current_file_content(app_type: AppType) -> Result<Option<String>, AppError> {
        let file_path = prompt_file_path(&app_type)?;
        if !file_path.exists() {
            return Ok(None);
        }

        std::fs::read_to_string(&file_path)
            .map(Some)
            .map_err(|err| AppError::io(&file_path, err))
    }

    pub fn import_from_file_on_first_launch(
        state: &AppState,
        app_type: AppType,
    ) -> Result<usize, AppError> {
        if !state.db.get_all_prompts(app_type.as_str())?.is_empty() {
            return Ok(0);
        }

        let file_path = prompt_file_path(&app_type)?;
        if !file_path.exists() {
            return Ok(0);
        }

        let content = match std::fs::read_to_string(&file_path) {
            Ok(content) => content,
            Err(err) => {
                log::warn!(
                    "Failed to read prompt file {}: {}",
                    file_path.display(),
                    err
                );
                return Ok(0);
            }
        };

        if content.trim().is_empty() {
            return Ok(0);
        }

        let timestamp = unix_timestamp()?;
        let prompt = Prompt {
            id: format!("auto-imported-{timestamp}"),
            name: format!(
                "Auto-imported Prompt {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("Automatically imported on first launch".to_string()),
            enabled: true,
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        state.db.save_prompt(app_type.as_str(), &prompt)?;
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    use crate::database::Database;
    use crate::settings::AppSettings;

    #[test]
    #[serial]
    fn enable_prompt_writes_live_file() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let state = AppState::new(Database::memory()?);
        let prompt = Prompt {
            id: "prompt-a".to_string(),
            name: "Prompt A".to_string(),
            content: "hello from prompt".to_string(),
            description: None,
            enabled: false,
            created_at: Some(unix_timestamp()?),
            updated_at: None,
        };
        state.db.save_prompt("openclaw", &prompt)?;

        PromptService::enable_prompt(&state, AppType::OpenClaw, "prompt-a")?;

        let path = prompt_file_path(&AppType::OpenClaw)?;
        let content = std::fs::read_to_string(&path).map_err(|err| AppError::io(&path, err))?;
        assert_eq!(content, "hello from prompt");

        Ok(())
    }

    #[test]
    #[serial]
    fn first_launch_import_reads_live_file() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let path = prompt_file_path(&AppType::OpenClaw)?;
        write_text_file(&path, "seed prompt")?;

        let state = AppState::new(Database::memory()?);
        let imported = PromptService::import_from_file_on_first_launch(&state, AppType::OpenClaw)?;
        let prompts = state.db.get_all_prompts("openclaw")?;

        assert_eq!(imported, 1);
        assert_eq!(prompts.len(), 1);
        assert!(prompts.values().next().is_some_and(|prompt| prompt.enabled));

        Ok(())
    }
}
