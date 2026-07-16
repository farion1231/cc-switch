use super::{
    credential_fingerprint, extract_provider_credentials, mask_credential, CredentialDiff,
};
use crate::app_config::AppType;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::ProviderService;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationState {
    Consistent,
    Inconsistent,
}

impl ConfigurationState {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Consistent => "consistent",
            Self::Inconsistent => "inconsistent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMode {
    ProjectDbToLive,
    ImportLiveToDb,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryResult {
    pub state: ConfigurationState,
    pub revision: i64,
    pub live_fingerprint_verified: bool,
    pub audit_written: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecurityStatus {
    pub provider_id: String,
    pub app_type: String,
    pub revision: i64,
    pub credential_valid: bool,
    pub conflicts: Vec<CredentialDiff>,
    pub configuration_state: ConfigurationState,
}

pub(crate) fn read_configuration_state(
    db: &Database,
    app_type: &AppType,
) -> Result<ConfigurationState, AppError> {
    let conn = lock_conn!(db.conn);
    let value = conn
        .query_row(
            "SELECT state FROM app_configuration_state WHERE app_type = ?1",
            params![app_type.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| AppError::Database(format!("read app configuration state: {e}")))?;
    Ok(match value.as_deref() {
        Some("inconsistent") => ConfigurationState::Inconsistent,
        _ => ConfigurationState::Consistent,
    })
}

pub(crate) fn persist_configuration_state(
    db: &Database,
    app_type: &AppType,
    state: ConfigurationState,
    reason: Option<&str>,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().timestamp_millis();
    let conn = lock_conn!(db.conn);
    conn.execute(
        "INSERT INTO app_configuration_state (app_type, state, reason, detected_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(app_type) DO UPDATE SET
             state = excluded.state,
             reason = excluded.reason,
             detected_at = CASE
                 WHEN excluded.state = 'inconsistent' THEN excluded.detected_at
                 ELSE NULL
             END,
             updated_at = excluded.updated_at",
        params![app_type.as_str(), state.as_str(), reason, now],
    )
    .map_err(|e| AppError::Database(format!("persist app configuration state: {e}")))?;
    Ok(())
}

fn diff_field(field: &str, stored: Option<&str>, live: Option<&str>) -> Option<CredentialDiff> {
    if stored == live {
        return None;
    }
    Some(CredentialDiff {
        field: field.to_string(),
        stored_masked: stored.map(mask_credential),
        live_masked: live.map(mask_credential),
        stored_fingerprint: stored.map(|value| credential_fingerprint(field, value)),
        live_fingerprint: live.map(|value| credential_fingerprint(field, value)),
    })
}

pub(crate) fn credential_diffs(
    stored: &Provider,
    live_settings: &serde_json::Value,
    app_type: &AppType,
) -> Vec<CredentialDiff> {
    let stored_fields = extract_provider_credentials(stored, app_type);
    let mut live_provider = stored.clone();
    live_provider.settings_config = live_settings.clone();
    let live_fields = extract_provider_credentials(&live_provider, app_type);

    let mut result = Vec::new();
    if let Some(diff) = diff_field(
        "apiKey",
        stored_fields.api_key.as_deref(),
        live_fields.api_key.as_deref(),
    ) {
        result.push(diff);
    }
    if let Some(diff) = diff_field(
        "baseUrl",
        stored_fields.base_url.as_deref(),
        live_fields.base_url.as_deref(),
    ) {
        result.push(diff);
    }
    result
}

pub fn get_security_status(
    db: &Database,
    app_type: AppType,
    provider_id: &str,
) -> Result<ProviderSecurityStatus, AppError> {
    let provider = db
        .get_provider_by_id(provider_id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput("provider not found".to_string()))?;
    let revision = db
        .get_provider_revision(app_type.as_str(), provider_id)?
        .ok_or_else(|| AppError::InvalidInput("provider revision missing".to_string()))?;
    let stored = extract_provider_credentials(&provider, &app_type);
    let credential_valid = stored.api_key.is_some() || stored.base_url.is_some();
    let live_settings = ProviderService::read_live_settings(app_type.clone()).ok();
    let conflicts = live_settings
        .as_ref()
        .map(|settings| credential_diffs(&provider, settings, &app_type))
        .unwrap_or_default();

    Ok(ProviderSecurityStatus {
        provider_id: provider_id.to_string(),
        app_type: app_type.as_str().to_string(),
        revision,
        credential_valid,
        conflicts,
        configuration_state: read_configuration_state(db, &app_type)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(settings_config: serde_json::Value) -> Provider {
        Provider::with_id("p1".to_string(), "P1".to_string(), settings_config, None)
    }

    #[test]
    fn credential_diffs_are_masked_and_never_contain_raw_values() {
        let stored = provider(json!({"env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-stored-secret",
            "ANTHROPIC_BASE_URL": "https://db.example"
        }}));
        let live = json!({"env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-live-secret",
            "ANTHROPIC_BASE_URL": "https://live.example"
        }});
        let diffs = credential_diffs(&stored, &live, &AppType::Claude);
        let encoded = serde_json::to_string(&diffs).unwrap();
        assert_eq!(diffs.len(), 2);
        assert!(!encoded.contains("sk-stored-secret"));
        assert!(!encoded.contains("sk-live-secret"));
        assert!(!encoded.contains("https://db.example"));
        assert!(!encoded.contains("https://live.example"));
    }
}
