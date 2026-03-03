use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccount {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub account_id: String,
    pub plan_type: Option<String>,
    pub auth_mode: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub last_refresh_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub source: String,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderBinding {
    pub provider_id: String,
    pub account_id: String,
    pub auto_bound: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsageState {
    pub account_id: String,
    pub allowed: Option<bool>,
    pub limit_reached: Option<bool>,
    pub primary_used_percent: Option<f64>,
    pub primary_limit_window_seconds: Option<i64>,
    pub primary_reset_at: Option<i64>,
    pub primary_reset_after_seconds: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_limit_window_seconds: Option<i64>,
    pub secondary_reset_at: Option<i64>,
    pub secondary_reset_after_seconds: Option<i64>,
    pub credits_has_credits: Option<bool>,
    pub credits_balance: Option<f64>,
    pub credits_unlimited: Option<bool>,
    pub last_refresh_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsageView {
    pub provider_id: String,
    pub account: Option<CodexAccount>,
    pub binding: Option<CodexProviderBinding>,
    pub usage: Option<CodexUsageState>,
    pub available: bool,
    pub cooldown_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub bindings_updated: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub refreshed_accounts: usize,
    pub success_accounts: usize,
    pub failed_accounts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginSession {
    pub session_id: String,
    pub provider_id: String,
    pub auth_url: String,
}
