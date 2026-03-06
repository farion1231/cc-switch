//! Prompt service - business logic for prompt management

use indexmap::IndexMap;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::store::AppState;

/// Prompt business logic service
pub struct PromptService;

impl PromptService {
    /// List all prompts for an app type
    pub fn list(state: &AppState, app_type: AppType) -> Result<IndexMap<String, Prompt>, AppError> {
        state.db.get_all_prompts(app_type.as_str())
    }

    /// Get a single prompt
    pub fn get(state: &AppState, app_type: AppType, id: &str) -> Result<Option<Prompt>, AppError> {
        state.db.get_prompt(app_type.as_str(), id)
    }

    /// Save or update a prompt
    pub fn save(state: &AppState, app_type: AppType, prompt: &Prompt) -> Result<(), AppError> {
        state.db.save_prompt(app_type.as_str(), prompt)
    }

    /// Delete a prompt
    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        state.db.delete_prompt(app_type.as_str(), id)
    }

    /// Enable a prompt
    pub fn enable(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let mut prompt = state
            .db
            .get_prompt(app_type.as_str(), id)?
            .ok_or_else(|| AppError::Message(format!("Prompt {} not found", id)))?;

        prompt.enabled = true;
        state.db.save_prompt(app_type.as_str(), &prompt)
    }

    /// Disable a prompt
    pub fn disable(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let mut prompt = state
            .db
            .get_prompt(app_type.as_str(), id)?
            .ok_or_else(|| AppError::Message(format!("Prompt {} not found", id)))?;

        prompt.enabled = false;
        state.db.save_prompt(app_type.as_str(), &prompt)
    }

    /// Import prompts from files
    pub fn import_from_files(state: &AppState, app_type: AppType) -> Result<usize, AppError> {
        let config_dir = match app_type {
            AppType::Claude => crate::config::get_claude_config_dir().join("prompts"),
            AppType::Codex => crate::config::get_codex_config_dir().join("prompts"),
            AppType::Gemini => crate::config::get_gemini_config_dir().join("prompts"),
            AppType::OpenCode => crate::config::get_opencode_config_dir().join("prompts"),
        };

        if !config_dir.exists() {
            return Ok(0);
        }

        let mut imported = 0;
        let entries = std::fs::read_dir(&config_dir)
            .map_err(|e| crate::error::AppError::io(&config_dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| crate::error::AppError::io(&config_dir, e))?;
            let path = entry.path();

            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    if state.db.get_prompt(app_type.as_str(), id)?.is_some() {
                        continue;
                    }

                    let content = std::fs::read_to_string(&path)
                        .map_err(|e| crate::error::AppError::io(&path, e))?;

                    let prompt = Prompt {
                        id: id.to_string(),
                        name: id.to_string(),
                        content,
                        description: None,
                        enabled: true,
                        created_at: Some(chrono::Utc::now().timestamp_millis()),
                        updated_at: None,
                    };

                    state.db.save_prompt(app_type.as_str(), &prompt)?;
                    imported += 1;
                }
            }
        }

        Ok(imported)
    }
}
