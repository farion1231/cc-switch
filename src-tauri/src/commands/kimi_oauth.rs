//! Kimi OAuth state and Kimi-specific commands.

use crate::proxy::providers::kimi_oauth_api::{
    KimiExtraUsage, KimiModelInfo, KimiUsageReport, KimiUsageTier,
};
use crate::proxy::providers::kimi_oauth_auth::KimiOAuthManager;
use crate::services::model_fetch::FetchedModel;
use crate::services::subscription::{CredentialStatus, ExtraUsage, QuotaTier, SubscriptionQuota};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Shared Kimi OAuth manager state registered with Tauri.
pub struct KimiOAuthState(pub Arc<RwLock<KimiOAuthManager>>);

/// Queries live Kimi usage for a selected or default managed account.
pub(crate) async fn query_kimi_oauth_quota_for(
    state: &KimiOAuthState,
    account_id: Option<String>,
) -> Result<SubscriptionQuota, String> {
    let manager = state.0.read().await;
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Ok(SubscriptionQuota::not_found("kimi_oauth"));
    };
    match manager.fetch_usage_for_account(&id).await {
        Ok(report) => Ok(quota_from_report(report, manager.now_millis())),
        Err(
            error @ (crate::proxy::providers::kimi_oauth_auth::KimiOAuthError::AccountNotFound(_)
            | crate::proxy::providers::kimi_oauth_auth::KimiOAuthError::RefreshTokenInvalid
            | crate::proxy::providers::kimi_oauth_auth::KimiOAuthError::ReauthRequired(_)),
        ) => Ok(SubscriptionQuota::error(
            "kimi_oauth",
            CredentialStatus::Expired,
            format!("Kimi OAuth token unavailable: {error}"),
        )),
        Err(error) => Err(format!("Kimi usage request failed: {error}")),
    }
}

/// Returns live Kimi managed subscription usage.
#[tauri::command(rename_all = "camelCase")]
pub async fn get_kimi_oauth_quota(
    account_id: Option<String>,
    state: State<'_, KimiOAuthState>,
) -> Result<SubscriptionQuota, String> {
    query_kimi_oauth_quota_for(&state, account_id).await
}

/// Queries the live Kimi model catalog for a selected or default account.
pub(crate) async fn query_kimi_oauth_models_for(
    state: &KimiOAuthState,
    account_id: Option<String>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = state.0.read().await;
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let id = resolved.ok_or_else(|| "No usable Kimi account available".to_string())?;
    manager
        .fetch_models_for_account(&id)
        .await
        .map(models_from_infos)
        .map_err(|error| format!("Kimi models request failed: {error}"))
}

/// Returns the live Anthropic-protocol model catalog for a Kimi account.
#[tauri::command(rename_all = "camelCase")]
pub async fn get_kimi_oauth_models(
    account_id: Option<String>,
    state: State<'_, KimiOAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    query_kimi_oauth_models_for(&state, account_id).await
}

fn models_from_infos(models: Vec<KimiModelInfo>) -> Vec<FetchedModel> {
    models
        .into_iter()
        .map(|model| FetchedModel {
            id: model.id,
            owned_by: Some("moonshot".to_string()),
        })
        .collect()
}

fn quota_from_report(report: KimiUsageReport, queried_at: i64) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "kimi_oauth".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers: report.tiers.into_iter().map(quota_tier_from_kimi).collect(),
        extra_usage: report.extra_usage.map(extra_usage_from_kimi),
        error: None,
        queried_at: Some(queried_at),
    }
}

fn quota_tier_from_kimi(tier: KimiUsageTier) -> QuotaTier {
    QuotaTier {
        name: tier.name,
        utilization: tier.utilization,
        resets_at: tier.resets_at,
        used_value_usd: None,
        max_value_usd: None,
    }
}

fn extra_usage_from_kimi(extra: KimiExtraUsage) -> ExtraUsage {
    ExtraUsage {
        is_enabled: extra.is_enabled,
        monthly_limit: extra.monthly_limit,
        used_credits: extra.used_credits,
        utilization: extra.utilization,
        currency: extra.currency,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::kimi_oauth_api::{
        KimiExtraUsage, KimiModelInfo, KimiUsageReport, KimiUsageTier,
    };

    #[test]
    fn live_models_are_mapped_to_the_existing_frontend_contract() {
        let models = models_from_infos(vec![
            KimiModelInfo {
                id: "k3".to_string(),
                context_length: 262_144,
            },
            KimiModelInfo {
                id: "k3-256k".to_string(),
                context_length: 262_144,
            },
        ]);

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "k3");
        assert_eq!(models[0].owned_by.as_deref(), Some("moonshot"));
    }

    #[test]
    fn live_usage_is_mapped_without_losing_reset_or_monthly_fields() {
        let quota = quota_from_report(
            KimiUsageReport {
                tiers: vec![KimiUsageTier {
                    name: "five_hour".to_string(),
                    utilization: 25.0,
                    resets_at: Some("2030-01-01T05:00:00Z".to_string()),
                }],
                extra_usage: Some(KimiExtraUsage {
                    is_enabled: true,
                    monthly_limit: Some(200.0),
                    used_credits: Some(50.0),
                    utilization: Some(25.0),
                    currency: Some("USD".to_string()),
                }),
            },
            1_700_000_000_000,
        );

        assert!(quota.success);
        assert!(matches!(quota.credential_status, CredentialStatus::Valid));
        assert_eq!(quota.queried_at, Some(1_700_000_000_000));
        assert_eq!(quota.tiers[0].name, "five_hour");
        assert_eq!(
            quota.tiers[0].resets_at.as_deref(),
            Some("2030-01-01T05:00:00Z")
        );
        let extra = quota.extra_usage.unwrap();
        assert_eq!(extra.monthly_limit, Some(200.0));
        assert_eq!(extra.used_credits, Some(50.0));
    }
}
