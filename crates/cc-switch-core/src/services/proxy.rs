//! Proxy service - business logic for proxy management

use serde::{Deserialize, Serialize};

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
    pub fn get_status(state: &AppState) -> Result<ProxyStatus, AppError> {
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
        state.db.switch_proxy_target(app, provider_id)
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
