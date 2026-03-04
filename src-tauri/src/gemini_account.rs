use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiAccount {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub google_account_id: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub expiry_date: Option<i64>,
    pub source: String,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiProviderBinding {
    pub provider_id: String,
    pub account_id: String,
    pub auto_bound: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageState {
    pub account_id: String,
    pub cooldown_until: Option<i64>,
    pub last_error: Option<String>,
    pub last_refresh_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageView {
    pub provider_id: String,
    pub account: Option<GeminiAccount>,
    pub binding: Option<GeminiProviderBinding>,
    pub usage: Option<GeminiUsageState>,
    pub available: bool,
    pub cooldown_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLoginSession {
    pub session_id: String,
    pub provider_id: String,
    pub started_at_ms: i64,
    pub expires_at_ms: i64,
    pub expected_files_dir: String,
    pub auth_url: Option<String>,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiLoginStatus {
    pub session_id: String,
    pub provider_id: String,
    pub status: String,
    pub updated_at_ms: i64,
    pub expires_at_ms: i64,
    pub remaining_seconds: i64,
    pub expected_files_dir: Option<String>,
    pub auth_url: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiPoolStatus {
    pub total_accounts: usize,
    pub active_accounts: usize,
    pub bound_providers: usize,
    pub providers_with_available_account: usize,
    pub providers_in_cooldown: usize,
    pub providers_with_error: usize,
}
