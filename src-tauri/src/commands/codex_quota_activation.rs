use crate::app_config::AppType;
use crate::database::CodexQuotaActivationPolicy;
use crate::provider::Provider;
use crate::store::AppState;
use tauri::State;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaActivationPolicyView {
    pub account_id: String,
    pub owner_provider_id: String,
    pub owner_provider_name: Option<String>,
    pub enabled: bool,
    pub can_edit: bool,
    pub updated_at: i64,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_window_type: Option<String>,
    pub last_model: Option<String>,
    pub last_attempt_at: Option<i64>,
    pub latest_attempt: Option<CodexQuotaActivationAttemptView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexQuotaActivationAttemptView {
    pub status: String,
    pub window_type: String,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

fn resolve_managed_provider(
    state: &AppState,
    provider_id: &str,
) -> Result<(Provider, String), String> {
    let provider = state
        .db
        .get_provider_by_id(provider_id, AppType::Codex.as_str())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Codex provider not found".to_string())?;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "This provider is not bound to a managed Codex OAuth account".to_string())?;
    let official_identity = provider.id == crate::database::CODEX_OFFICIAL_PROVIDER_ID
        || provider.category.as_deref() == Some("official")
        || provider.is_codex_oauth();
    if !official_identity {
        return Err("Quota activation is only available for official managed Codex OAuth".into());
    }
    Ok((provider, account_id))
}

fn to_view(
    state: &AppState,
    requested_provider_id: &str,
    policy: CodexQuotaActivationPolicy,
) -> CodexQuotaActivationPolicyView {
    let owner_provider_name = state
        .db
        .get_provider_by_id(&policy.owner_provider_id, AppType::Codex.as_str())
        .ok()
        .flatten()
        .map(|provider| provider.name);
    let latest_attempt = state
        .db
        .get_latest_codex_quota_activation_attempt(&policy.account_id)
        .ok()
        .flatten()
        .map(|attempt| CodexQuotaActivationAttemptView {
            status: attempt.status,
            window_type: attempt.limit_id,
            model: attempt.model,
            reasoning_effort: attempt.reasoning_effort,
            started_at: attempt.started_at,
            ended_at: attempt.ended_at,
            exit_code: attempt.exit_code,
            error: attempt.error,
        });
    CodexQuotaActivationPolicyView {
        // The renderer already owns the provider binding and needs a stable
        // key for its account-scoped status; event payloads remain masked.
        account_id: policy.account_id.clone(),
        owner_provider_id: policy.owner_provider_id.clone(),
        owner_provider_name,
        enabled: policy.enabled,
        can_edit: policy.owner_provider_id == requested_provider_id,
        updated_at: policy.updated_at,
        last_status: policy.last_status,
        last_error: policy.last_error,
        last_window_type: policy.last_window_type,
        last_model: policy.last_model,
        last_attempt_at: policy.last_attempt_at,
        latest_attempt,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_codex_quota_activation_policy(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Option<CodexQuotaActivationPolicyView>, String> {
    let (_provider, account_id) = resolve_managed_provider(state.inner(), &provider_id)?;
    let Some(policy) = state
        .db
        .get_codex_quota_activation_policy(&account_id)
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(to_view(state.inner(), &provider_id, policy)))
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_codex_quota_activation_policy(
    state: State<'_, AppState>,
    provider_id: String,
    enabled: bool,
) -> Result<CodexQuotaActivationPolicyView, String> {
    let (_provider, account_id) = resolve_managed_provider(state.inner(), &provider_id)?;
    let policy = state
        .db
        .upsert_codex_quota_activation_policy(
            &account_id,
            &provider_id,
            enabled,
            chrono::Utc::now().timestamp_millis(),
        )
        .map_err(|e| e.to_string())?;
    Ok(to_view(state.inner(), &provider_id, policy))
}
