//! Config service - business logic for app settings

use serde_json::Value;

use crate::database::Database;
use crate::error::AppError;
use crate::settings::AppSettings;

/// Config business logic service
pub struct ConfigService;

impl ConfigService {
    /// Get app settings
    pub fn get_settings(db: &Database) -> Result<AppSettings, AppError> {
        db.get_settings()
    }

    /// Get a single setting value
    pub fn get_setting(db: &Database, key: &str) -> Result<Option<String>, AppError> {
        db.get_setting(key)
    }

    /// Set a setting value
    pub fn set_setting(db: &Database, key: &str, value: &str) -> Result<(), AppError> {
        db.set_setting(key, value)
    }

    /// Export all configuration
    pub fn export_all(db: &Database) -> Result<Value, AppError> {
        let settings = db.get_settings()?;
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
                db.save_settings(&s)?;
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
    pub fn import(url: &str, db: &Database) -> Result<DeeplinkImportResult, AppError> {
        let parsed =
            url::Url::parse(url).map_err(|e| AppError::Message(format!("Invalid URL: {}", e)))?;

        let scheme = parsed.scheme();
        if scheme != "ccswitch" {
            return Err(AppError::Message(format!("Invalid scheme: {}", scheme)));
        }

        let host = parsed
            .host_str()
            .ok_or_else(|| AppError::Message("Missing host in URL".to_string()))?;

        match host {
            "provider" => Self::import_provider(&parsed, db),
            "mcp" => Self::import_mcp(&parsed, db),
            "skill" => Self::import_skill(&parsed, db),
            _ => Err(AppError::Message(format!("Unknown import type: {}", host))),
        }
    }

    fn import_provider(url: &url::Url, db: &Database) -> Result<DeeplinkImportResult, AppError> {
        let query: std::collections::HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let name = query
            .get("name")
            .ok_or_else(|| AppError::Message("Missing name parameter".to_string()))?;
        let base_url = query
            .get("baseUrl")
            .ok_or_else(|| AppError::Message("Missing baseUrl parameter".to_string()))?;
        let api_key = query
            .get("apiKey")
            .ok_or_else(|| AppError::Message("Missing apiKey parameter".to_string()))?;
        let app = query.get("app").map(|s| s.as_str()).unwrap_or("claude");

        let id = uuid::Uuid::new_v4().to_string();

        let settings_config = if app == "claude" {
            serde_json::json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    "ANTHROPIC_AUTH_TOKEN": api_key,
                }
            })
        } else if app == "codex" {
            serde_json::json!({
                "auth": {
                    "OPENAI_API_KEY": api_key
                },
                "config": format!(r#"base_url = "{}""#, base_url)
            })
        } else {
            serde_json::json!({
                "env": {
                    "GOOGLE_GEMINI_BASE_URL": base_url,
                    "GEMINI_API_KEY": api_key,
                }
            })
        };

        let provider = crate::provider::Provider {
            id: id.clone(),
            name: name.clone(),
            settings_config,
            website_url: None,
            category: None,
            created_at: Some(chrono::Utc::now().timestamp_millis()),
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        db.save_provider(app, &provider)?;

        Ok(DeeplinkImportResult {
            item_type: "provider".to_string(),
            warnings: vec![],
        })
    }

    fn import_mcp(url: &url::Url, db: &Database) -> Result<DeeplinkImportResult, AppError> {
        let query: std::collections::HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let id = query
            .get("id")
            .ok_or_else(|| AppError::Message("Missing id parameter".to_string()))?;
        let command = query
            .get("command")
            .ok_or_else(|| AppError::Message("Missing command parameter".to_string()))?;

        let args: Vec<String> = query
            .get("args")
            .map(|s| s.split(',').map(|a| a.trim().to_string()).collect())
            .unwrap_or_default();

        let server = crate::app_config::McpServer {
            id: id.clone(),
            name: id.clone(),
            server: serde_json::json!({
                "command": command,
                "args": args,
            }),
            apps: crate::app_config::McpApps {
                claude: true,
                codex: false,
                gemini: false,
                opencode: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        };

        db.save_mcp_server(&server)?;

        Ok(DeeplinkImportResult {
            item_type: "mcp".to_string(),
            warnings: vec![],
        })
    }

    fn import_skill(url: &url::Url, db: &Database) -> Result<DeeplinkImportResult, AppError> {
        Err(AppError::Message(
            "Skill import not implemented".to_string(),
        ))
    }
}
