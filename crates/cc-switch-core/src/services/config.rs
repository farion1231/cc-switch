//! Config service - business logic for app settings

use serde_json::Value;

use crate::database::Database;
use crate::error::AppError;
use crate::settings;
use crate::settings::AppSettings;
use crate::store::AppState;

/// Config business logic service
pub struct ConfigService;

impl ConfigService {
    /// Get app settings
    pub fn get_settings(_db: &Database) -> Result<AppSettings, AppError> {
        Ok(settings::get_settings())
    }

    /// Get a single setting value
    pub fn get_setting(_db: &Database, key: &str) -> Result<Option<String>, AppError> {
        let settings = settings::get_settings();
        let value = match key {
            "language" => settings.language,
            "claudeConfigDir" => settings.claude_config_dir,
            "codexConfigDir" => settings.codex_config_dir,
            "geminiConfigDir" => settings.gemini_config_dir,
            "opencodeConfigDir" => settings.opencode_config_dir,
            "openclawConfigDir" => settings.openclaw_config_dir,
            "currentProviderClaude" => settings.current_provider_claude,
            "currentProviderCodex" => settings.current_provider_codex,
            "currentProviderGemini" => settings.current_provider_gemini,
            "currentProviderOpenCode" => settings.current_provider_opencode,
            "currentProviderOpenClaw" => settings.current_provider_openclaw,
            "skillSyncMethod" => Some(settings.skill_sync_method.to_string()),
            "preferredTerminal" => settings.preferred_terminal,
            _ => None,
        };

        Ok(value)
    }

    /// Set a setting value
    pub fn set_setting(_db: &Database, key: &str, value: &str) -> Result<(), AppError> {
        let mut settings = settings::get_settings();
        match key {
            "language" => settings.language = Some(value.to_string()),
            "claudeConfigDir" => settings.claude_config_dir = Some(value.to_string()),
            "codexConfigDir" => settings.codex_config_dir = Some(value.to_string()),
            "geminiConfigDir" => settings.gemini_config_dir = Some(value.to_string()),
            "opencodeConfigDir" => settings.opencode_config_dir = Some(value.to_string()),
            "openclawConfigDir" => settings.openclaw_config_dir = Some(value.to_string()),
            "currentProviderClaude" => settings.current_provider_claude = Some(value.to_string()),
            "currentProviderCodex" => settings.current_provider_codex = Some(value.to_string()),
            "currentProviderGemini" => settings.current_provider_gemini = Some(value.to_string()),
            "currentProviderOpenCode" => {
                settings.current_provider_opencode = Some(value.to_string())
            }
            "currentProviderOpenClaw" => {
                settings.current_provider_openclaw = Some(value.to_string())
            }
            "skillSyncMethod" => settings.skill_sync_method = value.parse().unwrap_or_default(),
            "preferredTerminal" => settings.preferred_terminal = Some(value.to_string()),
            other => {
                return Err(AppError::InvalidInput(format!(
                    "Unsupported setting key: {other}"
                )));
            }
        }

        settings::update_settings(settings)
    }

    /// Export all configuration
    pub fn export_all(db: &Database) -> Result<Value, AppError> {
        let settings = settings::get_settings();
        let providers = db.export_all_providers()?;
        let mcp_servers = db.export_all_mcp_servers()?;
        let prompts = db.export_all_prompts()?;
        let skills = db.export_all_skills()?;

        Ok(serde_json::json!({
            "settings": settings,
            "providers": providers,
            "mcpServers": mcp_servers,
            "prompts": prompts,
            "skills": skills,
        }))
    }

    /// Import all configuration
    pub fn import_all(db: &Database, data: &Value, merge: bool) -> Result<(), AppError> {
        if !merge {
            db.clear_all_data()?;
        }

        if let Some(settings) = data.get("settings") {
            if let Ok(s) = serde_json::from_value(settings.clone()) {
                settings::update_settings(s)?;
            }
        }

        if let Some(providers) = data.get("providers").and_then(|v| v.as_object()) {
            for (app_type, app_providers) in providers {
                if let Some(providers_map) = app_providers.as_object() {
                    for (_, provider_value) in providers_map {
                        if let Ok(provider) = serde_json::from_value(provider_value.clone()) {
                            db.save_provider(app_type, &provider)?;
                        }
                    }
                }
            }
        }

        if let Some(mcp_servers) = data.get("mcpServers").and_then(|v| v.as_object()) {
            for (_, server_value) in mcp_servers {
                if let Ok(server) = serde_json::from_value(server_value.clone()) {
                    db.save_mcp_server(&server)?;
                }
            }
        }

        if let Some(prompts) = data.get("prompts").and_then(|v| v.as_object()) {
            for (app_type, app_prompts) in prompts {
                if let Some(prompts_map) = app_prompts.as_object() {
                    for (_, prompt_value) in prompts_map {
                        if let Ok(prompt) = serde_json::from_value(prompt_value.clone()) {
                            db.save_prompt(app_type, &prompt)?;
                        }
                    }
                }
            }
        }

        if let Some(skills) = data.get("skills").and_then(|v| v.as_array()) {
            for skill_value in skills {
                if let Ok(skill) = serde_json::from_value(skill_value.clone()) {
                    db.save_skill(&skill)?;
                }
            }
        }

        Ok(())
    }
}

/// Deeplink import result
pub struct DeeplinkImportResult {
    pub item_type: String,
    pub warnings: Vec<String>,
}

/// Deeplink service
pub struct DeeplinkService;

impl DeeplinkService {
    /// Import from deeplink URL
    pub fn import(url: &str, state: &AppState) -> Result<DeeplinkImportResult, AppError> {
        let request = crate::parse_deeplink_url(url)?;

        match request.resource.as_str() {
            "provider" => {
                crate::import_provider_from_deeplink(state, request)?;
                Ok(DeeplinkImportResult {
                    item_type: "provider".to_string(),
                    warnings: vec![],
                })
            }
            "prompt" => {
                crate::import_prompt_from_deeplink(state, request)?;
                Ok(DeeplinkImportResult {
                    item_type: "prompt".to_string(),
                    warnings: vec![],
                })
            }
            "mcp" => {
                let result = crate::import_mcp_from_deeplink(state, request)?;
                Ok(DeeplinkImportResult {
                    item_type: "mcp".to_string(),
                    warnings: result
                        .failed
                        .into_iter()
                        .map(|item| format!("{}: {}", item.id, item.error))
                        .collect(),
                })
            }
            "skill" => {
                crate::import_skill_from_deeplink(state, request)?;
                Ok(DeeplinkImportResult {
                    item_type: "skill".to_string(),
                    warnings: vec![],
                })
            }
            other => Err(AppError::Message(format!("Unknown import type: {other}"))),
        }
    }
}
