use crate::database::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmoGlobalConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sisyphus_agent: Option<serde_json::Value>,
    #[serde(default)]
    pub disabled_agents: Vec<String>,
    #[serde(default)]
    pub disabled_mcps: Vec<String>,
    #[serde(default)]
    pub disabled_hooks: Vec<String>,
    #[serde(default)]
    pub disabled_skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lsp: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_task: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_automation_engine: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_code: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_fields: Option<serde_json::Value>,
    pub updated_at: String,
}

impl Default for OmoGlobalConfig {
    fn default() -> Self {
        Self {
            id: "global".to_string(),
            schema_url: None,
            sisyphus_agent: None,
            disabled_agents: vec![],
            disabled_mcps: vec![],
            disabled_hooks: vec![],
            disabled_skills: vec![],
            lsp: None,
            experimental: None,
            background_task: None,
            browser_automation_engine: None,
            claude_code: None,
            other_fields: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl Database {
    pub fn get_omo_global_config(&self, key: &str) -> Result<OmoGlobalConfig, AppError> {
        let json_str = self.get_setting(key)?;
        match json_str {
            Some(s) => serde_json::from_str::<OmoGlobalConfig>(&s)
                .map_err(|e| AppError::Config(format!("Failed to parse {key}: {e}"))),
            None => Ok(OmoGlobalConfig::default()),
        }
    }

    pub fn save_omo_global_config(
        &self,
        key: &str,
        config: &OmoGlobalConfig,
    ) -> Result<(), AppError> {
        let json_str = serde_json::to_string(config)
            .map_err(|e| AppError::Config(format!("JSON serialization failed: {e}")))?;
        self.set_setting(key, &json_str)?;
        Ok(())
    }
}
