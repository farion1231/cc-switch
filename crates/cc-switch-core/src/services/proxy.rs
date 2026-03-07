//! Proxy service - business logic for proxy management

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::database::Database;
use crate::error::AppError;
use crate::store::AppState;

/// Proxy status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatus {
    pub running: bool,
    #[serde(rename = "listenAddr")]
    pub listen_addr: Option<String>,
}

/// Proxy config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub port: u16,
    pub host: String,
    #[serde(rename = "logEnabled")]
    pub log_enabled: bool,
}

/// Proxy takeover status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTakeoverStatus {
    pub apps: std::collections::HashMap<String, bool>,
}

/// Saved live config backup used by proxy takeover mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveBackup {
    #[serde(rename = "appType")]
    pub app_type: String,
    #[serde(rename = "originalConfig")]
    pub original_config: String,
    #[serde(rename = "backedUpAt")]
    pub backed_up_at: String,
}

/// Failover queue item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverQueueItem {
    pub priority: i32,
    #[serde(rename = "providerId")]
    pub provider_id: String,
    #[serde(rename = "providerName")]
    pub provider_name: String,
}

/// Provider health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub status: String,
    #[serde(rename = "failureCount")]
    pub failure_count: u32,
    #[serde(rename = "lastFailureTime")]
    pub last_failure_time: Option<String>,
}

/// Circuit breaker config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    #[serde(rename = "failureThreshold")]
    pub failure_threshold: u32,
    #[serde(rename = "recoveryTimeout")]
    pub recovery_timeout: u64,
    #[serde(rename = "halfOpenRequests")]
    pub half_open_requests: u32,
}

/// Proxy business logic service
pub struct ProxyService;

impl ProxyService {
    /// Get proxy status
    pub fn get_status(_state: &AppState) -> Result<ProxyStatus, AppError> {
        Ok(ProxyStatus {
            running: false,
            listen_addr: None,
        })
    }

    /// Get proxy config
    pub fn get_config(state: &AppState) -> Result<ProxyConfig, AppError> {
        state.db.get_proxy_config("claude")
    }

    /// Set takeover for app
    pub fn set_takeover_for_app(
        state: &AppState,
        app: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        state.db.set_proxy_takeover(app, enabled)
    }

    /// Get takeover status
    pub fn get_takeover_status(state: &AppState) -> Result<ProxyTakeoverStatus, AppError> {
        state.db.get_proxy_takeover_status()
    }

    /// Switch proxy target
    pub fn switch_proxy_target(
        state: &AppState,
        app: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let app_type = app.parse::<crate::app_config::AppType>()?;
        state.db.switch_proxy_target(app, provider_id)?;
        crate::settings::set_current_provider(&app_type, Some(provider_id))?;

        if state.db.get_live_backup(app)?.is_some() {
            let provider = state
                .db
                .get_provider_by_id(provider_id, app)?
                .ok_or_else(|| {
                    AppError::Message(format!(
                        "Provider '{provider_id}' not found for app '{app}'"
                    ))
                })?;
            Self::update_live_backup_from_provider(&state.db, app, &provider)?;
        }

        Ok(())
    }

    /// Reset provider circuit breaker
    pub fn reset_provider_circuit_breaker(
        state: &AppState,
        provider_id: &str,
        app: &str,
    ) -> Result<(), AppError> {
        state.db.reset_provider_health(provider_id, app)
    }
}

impl ProxyService {
    fn update_live_backup_from_provider(
        db: &Database,
        app_type: &str,
        provider: &crate::provider::Provider,
    ) -> Result<(), AppError> {
        let backup_json = match app_type {
            "claude" | "codex" => {
                serde_json::to_string(&provider.settings_config).map_err(|err| {
                    AppError::Message(format!("Failed to serialize {app_type} backup: {err}"))
                })?
            }
            "gemini" => {
                let env_backup = if let Some(env) = provider.settings_config.get("env") {
                    json!({ "env": env })
                } else {
                    json!({ "env": {} })
                };
                serde_json::to_string(&env_backup).map_err(|err| {
                    AppError::Message(format!("Failed to serialize gemini backup: {err}"))
                })?
            }
            _ => {
                return Err(AppError::InvalidInput(format!(
                    "Unsupported proxy app type: {app_type}"
                )))
            }
        };

        db.save_live_backup(app_type, &backup_json)
    }
}

/// Usage summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    #[serde(rename = "totalRequests")]
    pub total_requests: u64,
    #[serde(rename = "totalTokens")]
    pub total_tokens: u64,
    #[serde(rename = "totalCost")]
    pub total_cost: f64,
    #[serde(rename = "requestsByModel")]
    pub requests_by_model: std::collections::HashMap<String, u64>,
}

/// Request log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLog {
    pub timestamp: String,
    pub model: String,
    #[serde(rename = "totalTokens")]
    pub total_tokens: u64,
    pub cost: f64,
}

/// Usage stats service
pub struct UsageStatsService;

impl UsageStatsService {
    /// Get usage summary
    pub fn get_summary(db: &Database, app: &str, days: u32) -> Result<UsageSummary, AppError> {
        db.get_usage_summary(app, days)
    }

    /// Get request logs
    pub fn get_logs(
        db: &Database,
        app: &str,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Vec<RequestLog>, AppError> {
        db.get_request_logs(app, from, to)
    }

    /// Export usage to CSV
    pub fn export_csv(db: &Database, app: &str, output: &str) -> Result<String, AppError> {
        let logs = Self::get_logs(db, app, None, None)?;

        let mut wtr = csv::Writer::from_path(output)
            .map_err(|e| AppError::Message(format!("CSV write error: {}", e)))?;

        wtr.write_record(["Timestamp", "Model", "Tokens", "Cost"])
            .map_err(|e| AppError::Message(format!("CSV write error: {}", e)))?;

        for log in &logs {
            wtr.write_record([
                &log.timestamp,
                &log.model,
                &log.total_tokens.to_string(),
                &format!("{:.4}", log.cost),
            ])
            .map_err(|e| AppError::Message(format!("CSV write error: {}", e)))?;
        }

        wtr.flush()
            .map_err(|e| AppError::Message(format!("CSV write error: {}", e)))?;

        Ok(output.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;

    use crate::database::Database;
    use crate::provider::Provider;
    use crate::settings::AppSettings;

    #[test]
    #[serial]
    fn switch_proxy_target_updates_live_backup_when_present() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        crate::settings::update_settings(AppSettings::default())?;

        let state = AppState::new(Database::memory()?);
        let provider_a = Provider::with_id(
            "a".to_string(),
            "Provider A".to_string(),
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "stale"}}),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "Provider B".to_string(),
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "fresh"}}),
            None,
        );

        state.db.save_provider("claude", &provider_a)?;
        state.db.save_provider("claude", &provider_b)?;
        state.db.set_current_provider("claude", "a")?;
        crate::settings::set_current_provider(&crate::app_config::AppType::Claude, Some("a"))?;
        state.db.save_live_backup("claude", "{\"env\":{}}")?;

        ProxyService::switch_proxy_target(&state, "claude", "b")?;

        let backup = state
            .db
            .get_live_backup("claude")?
            .expect("backup should exist");
        assert_eq!(
            backup.original_config,
            serde_json::to_string(&provider_b.settings_config).unwrap()
        );
        assert_eq!(
            crate::settings::get_current_provider(&crate::app_config::AppType::Claude),
            Some("b".to_string())
        );

        Ok(())
    }
}
