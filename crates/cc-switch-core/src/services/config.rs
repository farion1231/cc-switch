//! Config service - business logic for app settings

use base64::prelude::*;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

use crate::database::Database;
use crate::error::AppError;
use crate::services::skill::SkillService;
use crate::services::webdav_sync::archive::{restore_skills_zip, zip_skills_ssot};
use crate::settings;
use crate::settings::AppSettings;
use crate::store::AppState;

/// Config business logic service
pub struct ConfigService;

const SKILLS_ARCHIVE_FORMAT: &str = "zip";
const SKILLS_ARCHIVE_ENCODING: &str = "base64";

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
        let skills_archive = export_skills_archive()?;

        Ok(serde_json::json!({
            "settings": settings,
            "providers": providers,
            "mcpServers": mcp_servers,
            "prompts": prompts,
            "skills": skills,
            "skillsArchive": skills_archive,
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

        let skills = data.get("skills").and_then(|v| v.as_array());
        match data.get("skillsArchive").filter(|value| !value.is_null()) {
            Some(skills_archive) => import_skills_archive(skills_archive)?,
            None if skills.is_some_and(|items| !items.is_empty()) => {
                return Err(AppError::Message(
                    "Import payload contains skills but no skillsArchive".to_string(),
                ));
            }
            None if !merge => clear_skills_ssot()?,
            None => {}
        }

        if let Some(skills) = skills {
            for skill_value in skills {
                if let Ok(skill) = serde_json::from_value(skill_value.clone()) {
                    db.save_skill(&skill)?;
                }
            }
        }

        Ok(())
    }
}

fn export_skills_archive() -> Result<Value, AppError> {
    let tmp = tempdir().map_err(|err| {
        AppError::Message(format!(
            "Failed to create temporary directory for skills export: {err}"
        ))
    })?;
    let zip_path = tmp.path().join("skills.zip");
    zip_skills_ssot(&zip_path)?;
    let bytes = fs::read(&zip_path).map_err(|e| AppError::io(&zip_path, e))?;

    Ok(serde_json::json!({
        "format": SKILLS_ARCHIVE_FORMAT,
        "encoding": SKILLS_ARCHIVE_ENCODING,
        "data": BASE64_STANDARD.encode(bytes),
    }))
}

fn import_skills_archive(payload: &Value) -> Result<(), AppError> {
    let format = payload
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Message("skillsArchive.format is required".to_string()))?;
    if format != SKILLS_ARCHIVE_FORMAT {
        return Err(AppError::Message(format!(
            "Unsupported skillsArchive.format: {format}"
        )));
    }

    let encoding = payload
        .get("encoding")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Message("skillsArchive.encoding is required".to_string()))?;
    if encoding != SKILLS_ARCHIVE_ENCODING {
        return Err(AppError::Message(format!(
            "Unsupported skillsArchive.encoding: {encoding}"
        )));
    }

    let data = payload
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Message("skillsArchive.data is required".to_string()))?;
    let bytes = BASE64_STANDARD
        .decode(data)
        .map_err(|err| AppError::Message(format!("Invalid skillsArchive.data: {err}")))?;

    restore_skills_zip(&bytes)
}

fn clear_skills_ssot() -> Result<(), AppError> {
    let ssot = SkillService::get_ssot_dir()?;
    if ssot.exists() {
        fs::remove_dir_all(&ssot).map_err(|e| AppError::io(&ssot, e))?;
    }
    fs::create_dir_all(&ssot).map_err(|e| AppError::io(&ssot, e))?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InstalledSkill, SkillApps};
    use serial_test::serial;
    use tempfile::tempdir;

    fn with_test_home<T>(home: &std::path::Path, f: impl FnOnce(&Database) -> T) -> T {
        let previous = std::env::var("CC_SWITCH_TEST_HOME").ok();
        let previous_home = std::env::var("HOME").ok();
        std::env::set_var("CC_SWITCH_TEST_HOME", home);
        std::env::set_var("HOME", home);
        let db = Database::new().expect("file database");
        let result = f(&db);
        match previous {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        result
    }

    #[test]
    #[serial]
    fn export_import_round_trip_preserves_skill_files() {
        let source = tempdir().expect("tempdir");
        let target = tempdir().expect("tempdir");

        let export = with_test_home(source.path(), |db| {
            let ssot = SkillService::get_ssot_dir()
                .expect("skills ssot")
                .join("demo-skill");
            fs::create_dir_all(&ssot).expect("create skill dir");
            fs::write(ssot.join("SKILL.md"), "# Demo Skill\n").expect("write skill file");
            fs::write(ssot.join("notes.txt"), "hello skill archive\n").expect("write skill note");
            db.save_skill(&InstalledSkill {
                id: "local:demo-skill".to_string(),
                name: "Demo Skill".to_string(),
                description: Some("demo".to_string()),
                directory: "demo-skill".to_string(),
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: SkillApps::default(),
                installed_at: 1,
            })
            .expect("save skill");

            ConfigService::export_all(db).expect("export config")
        });

        assert!(
            export
                .get("skillsArchive")
                .and_then(|item| item.get("data"))
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty()),
            "export should embed a non-empty skills archive"
        );

        with_test_home(target.path(), |db| {
            ConfigService::import_all(db, &export, false).expect("import config");
            let skills = db.get_all_installed_skills().expect("load imported skills");
            assert!(
                skills.contains_key("local:demo-skill"),
                "import should restore skill metadata"
            );

            let restored = SkillService::get_ssot_dir()
                .expect("skills ssot")
                .join("demo-skill");
            assert_eq!(
                fs::read_to_string(restored.join("SKILL.md")).expect("restored skill file"),
                "# Demo Skill\n"
            );
            assert_eq!(
                fs::read_to_string(restored.join("notes.txt")).expect("restored skill note"),
                "hello skill archive\n"
            );
        });
    }

    #[test]
    #[serial]
    fn import_with_skills_but_without_archive_fails() {
        let source = tempdir().expect("tempdir");
        let target = tempdir().expect("tempdir");

        let mut export = with_test_home(source.path(), |db| {
            db.save_skill(&InstalledSkill {
                id: "local:demo-skill".to_string(),
                name: "Demo Skill".to_string(),
                description: Some("demo".to_string()),
                directory: "demo-skill".to_string(),
                repo_owner: None,
                repo_name: None,
                repo_branch: None,
                readme_url: None,
                apps: SkillApps::default(),
                installed_at: 1,
            })
            .expect("save skill");

            ConfigService::export_all(db).expect("export config")
        });

        export
            .as_object_mut()
            .expect("export object")
            .remove("skillsArchive");

        with_test_home(target.path(), |db| {
            let err = ConfigService::import_all(db, &export, false)
                .expect_err("import should reject skill metadata without archive");
            assert!(
                err.to_string().contains("skillsArchive"),
                "expected missing archive error, got {err}"
            );
        });
    }
}
