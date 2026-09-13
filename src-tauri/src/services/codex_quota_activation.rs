//! Optional account-scoped Codex quota-window activation.
//!
//! A successful managed OAuth quota query is the only entry point. Fresh raw
//! window data is evaluated here, claimed in SQLite, then an account-scoped
//! HTTP request is sent through the managed Codex OAuth transport.

use crate::database::{CodexQuotaActivationPolicy, Database};
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::codex_provider_upstream_model;
use crate::services::codex_quota_activation_http::{
    run_activation_http_for_account, ActivationModel, ActivationRunResult,
};
use crate::services::subscription::{
    query_codex_quota_with_observation, CodexQuotaObservation, CodexQuotaWindowObservation,
};
use crate::services::UsageCache;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

const FIVE_HOURS_SECONDS: i64 = 5 * 60 * 60;
const SEVEN_DAYS_SECONDS: i64 = 7 * 24 * 60 * 60;
const RESET_TOLERANCE_SECONDS: i64 = 5 * 60;
const STALE_CLAIM_AFTER_MS: i64 = 5 * 60 * 1000;
const MAX_ERROR_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq)]
struct TargetWindow {
    limit_id: String,
    window_type: &'static str,
    window_seconds: i64,
    used_percent: f64,
    reset_at: i64,
    reset_after_seconds: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivationEvent {
    account_id: String,
    provider_name: String,
    window_type: String,
    model: Option<String>,
    status: String,
    time: i64,
    error: Option<String>,
}

#[derive(Clone)]
pub struct CodexQuotaActivationCoordinator {
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    usage_cache: Arc<UsageCache>,
}

impl CodexQuotaActivationCoordinator {
    pub fn new(
        db: Arc<Database>,
        manager: Arc<CodexOAuthManager>,
        usage_cache: Arc<UsageCache>,
    ) -> Self {
        Self {
            db,
            manager,
            usage_cache,
        }
    }

    /// Evaluate one fresh successful managed-account quota response.
    /// Errors are returned to the caller for logging but never replace the quota result.
    pub(crate) async fn observe(
        &self,
        app: AppHandle,
        account_id: &str,
        observation: &CodexQuotaObservation,
    ) -> Result<(), String> {
        let Some(policy) = self
            .db
            .get_codex_quota_activation_policy(account_id)
            .map_err(|e| e.to_string())?
        else {
            return Ok(());
        };
        if !policy.enabled {
            return Ok(());
        }

        self.db
            .mark_stale_codex_quota_activation_attempts_unknown(
                account_id,
                chrono::Utc::now().timestamp_millis(),
                STALE_CLAIM_AFTER_MS,
            )
            .map_err(|e| e.to_string())?;

        let now_seconds = chrono::Utc::now().timestamp();
        let Some(target) = select_target_window(observation, now_seconds) else {
            return Ok(());
        };
        let should_activate = is_newly_available_window(&policy, &target);
        if !should_activate {
            self.db
                .record_codex_quota_observation(
                    account_id,
                    &target.limit_id,
                    target.window_seconds,
                    Some(target.reset_at),
                    Some(target.reset_after_seconds),
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }

        let provider = self
            .db
            .get_provider_by_id(&policy.owner_provider_id, "codex")
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Activation owner provider no longer exists".to_string())?;
        if !owner_provider_allows_activation(&provider) {
            // Keep the persisted policy intact for a later re-enable, but do
            // not let an Auth Center/manual quota query bypass the provider's
            // disabled usage-query switch.
            return Ok(());
        }
        let bound_account = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .map(|id| id.trim().to_string());
        if bound_account.as_deref() != Some(account_id) {
            return Err("Activation owner provider no longer binds this account".to_string());
        }
        let provider_name = provider.name.clone();
        let Some(model) = resolve_activation_model(&provider) else {
            let error = "model_unresolved: owner provider has no explicit model";
            self.db
                .update_codex_quota_activation_status_summary(
                    account_id,
                    Some("model_unresolved"),
                    Some(error),
                    Some(target.window_type),
                    None,
                    None,
                )
                .map_err(|e| e.to_string())?;
            emit_status(
                &app,
                account_id,
                &provider_name,
                target.window_type,
                None,
                "model_unresolved",
                Some(error),
            );
            // Do not advance the observed generation. A later quota query can
            // retry after the owner provider receives an explicit model.
            return Ok(());
        };
        let generation = format!("{}:{}", target.window_seconds, target.reset_at);
        let claimed = self
            .db
            .claim_codex_quota_activation(
                account_id,
                &target.limit_id,
                target.window_seconds,
                &generation,
                &policy.owner_provider_id,
                Some(model.model.as_str()),
                model.reasoning_effort.as_deref(),
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|e| e.to_string())?;
        if !claimed {
            return Ok(());
        }

        self.db
            .record_codex_quota_observation(
                account_id,
                &target.limit_id,
                target.window_seconds,
                Some(target.reset_at),
                Some(target.reset_after_seconds),
            )
            .map_err(|e| e.to_string())?;

        self.db
            .update_codex_quota_activation_summary(
                account_id,
                Some("reserved"),
                None,
                Some(target.window_type),
                Some(model.model.as_str()),
                Some(chrono::Utc::now().timestamp_millis()),
                Some(target.reset_at),
                Some(target.reset_after_seconds),
            )
            .map_err(|e| e.to_string())?;
        emit_status(
            &app,
            account_id,
            &provider_name,
            target.window_type,
            Some(model.model.as_str()),
            "reserved",
            None,
        );

        let coordinator = self.clone();
        let account_id = account_id.to_string();
        tauri::async_runtime::spawn(async move {
            coordinator
                .execute_claim(app, account_id, provider_name, target, generation, model)
                .await;
        });
        Ok(())
    }

    async fn execute_claim(
        &self,
        app: AppHandle,
        account_id: String,
        provider_name: String,
        target: TargetWindow,
        generation: String,
        model: ActivationModel,
    ) {
        let now = chrono::Utc::now().timestamp_millis();
        if let Err(error) = self.db.update_codex_quota_activation_attempt(
            &account_id,
            &target.limit_id,
            &generation,
            "running",
            None,
            None,
            None,
            Some(model.model.as_str()),
        ) {
            log::warn!("Failed to mark Codex quota activation running: {error}");
            return;
        }
        let _ = self.db.update_codex_quota_activation_summary(
            &account_id,
            Some("running"),
            None,
            Some(target.window_type),
            Some(model.model.as_str()),
            Some(now),
            Some(target.reset_at),
            Some(target.reset_after_seconds),
        );
        emit_status(
            &app,
            &account_id,
            &provider_name,
            target.window_type,
            Some(model.model.as_str()),
            "running",
            None,
        );

        let mut result = run_activation_http_for_account(&self.manager, &account_id, &model).await;
        if result.status == "succeeded" {
            result = match self.read_back_quota(&account_id).await {
                Ok(quota) => {
                    self.usage_cache.put_codex_oauth(account_id.clone(), quota);
                    crate::tray::schedule_tray_refresh(&app);
                    if let Err(error) = app.emit("codex-quota-cache-updated", ()) {
                        log::warn!("Failed to emit Codex quota cache refresh: {error}");
                    }
                    result
                }
                Err(error) => ActivationRunResult {
                    status: "unknown",
                    exit_code: result.exit_code,
                    error: Some(sanitize_error(&error, &[account_id.clone()])),
                    actual_model: result.actual_model,
                },
            };
        }

        let final_model = result
            .actual_model
            .as_deref()
            .or(Some(model.model.as_str()));
        let ended_at = chrono::Utc::now().timestamp_millis();
        if let Err(error) = self.db.update_codex_quota_activation_attempt(
            &account_id,
            &target.limit_id,
            &generation,
            result.status,
            Some(ended_at),
            result.exit_code,
            result.error.as_deref(),
            final_model,
        ) {
            log::warn!("Failed to finish Codex quota activation ledger: {error}");
        }
        if let Err(error) = self.db.update_codex_quota_activation_summary(
            &account_id,
            Some(result.status),
            result.error.as_deref(),
            Some(target.window_type),
            final_model,
            Some(ended_at),
            Some(target.reset_at),
            Some(target.reset_after_seconds),
        ) {
            log::warn!("Failed to update Codex quota activation summary: {error}");
        }
        emit_status(
            &app,
            &account_id,
            &provider_name,
            target.window_type,
            final_model,
            result.status,
            result.error.as_deref(),
        );
    }

    async fn read_back_quota(
        &self,
        account_id: &str,
    ) -> Result<crate::services::subscription::SubscriptionQuota, String> {
        let token = self
            .manager
            .get_valid_token_for_account(account_id)
            .await
            .map_err(|e| e.to_string())?;
        let workspace = self
            .manager
            .chatgpt_account_id_for_account(account_id)
            .await
            .map_err(|e| e.to_string())?;
        let readback = query_codex_quota_with_observation(
            &token,
            Some(&workspace),
            "codex_oauth",
            "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
            true,
        )
        .await?;
        if readback.quota.success {
            Ok(readback.quota)
        } else {
            Err(readback
                .quota
                .error
                .unwrap_or_else(|| "Quota readback failed".to_string()))
        }
    }
}

fn owner_provider_allows_activation(provider: &crate::provider::Provider) -> bool {
    let Some(script) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.usage_script.as_ref())
    else {
        // Older managed providers may not have a usage_script block yet; the
        // existing tray behavior treats those as enabled by default.
        return true;
    };
    if !script.enabled {
        return false;
    }
    if script
        .template_type
        .as_deref()
        .is_some_and(|template| template != "official_subscription")
    {
        return false;
    }
    script.auto_query_interval.unwrap_or(5) > 0
}

fn select_target_window(
    observation: &CodexQuotaObservation,
    now_seconds: i64,
) -> Option<TargetWindow> {
    let candidates_by_priority = if crate::services::subscription::is_weekly_only_codex_plan(
        observation.plan_type.as_deref(),
    ) {
        vec![(SEVEN_DAYS_SECONDS, "weekly")]
    } else {
        vec![
            (FIVE_HOURS_SECONDS, "five_hour"),
            (SEVEN_DAYS_SECONDS, "weekly"),
        ]
    };
    for (window_seconds, window_type) in candidates_by_priority {
        let candidates = observation
            .windows
            .iter()
            .filter(|window| window.limit_window_seconds == Some(window_seconds))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        // More than one bucket with the same duration is ambiguous. An
        // additional bucket with the target duration is equally unsafe: it
        // may represent a separate metered feature, so do not guess which
        // bucket is the account-wide window.
        if candidates.len() != 1 || candidates[0].is_additional {
            return None;
        }
        return normalize_target_window(
            observation,
            candidates[0],
            now_seconds,
            window_seconds,
            window_type,
        );
    }
    None
}

fn normalize_target_window(
    observation: &CodexQuotaObservation,
    window: &CodexQuotaWindowObservation,
    now_seconds: i64,
    window_seconds: i64,
    window_type: &'static str,
) -> Option<TargetWindow> {
    let allowed = consistent_bool([observation.allowed, window.allowed])?;
    let limit_reached = consistent_bool([observation.limit_reached, window.limit_reached])?;
    if !allowed || limit_reached || reached_type_is_blocking(observation, window) {
        return None;
    }
    let used_percent = window.used_percent?;
    if !used_percent.is_finite() || !(0.0..100.0).contains(&used_percent) {
        return None;
    }
    let reset_at = window.reset_at?;
    let derived_after = reset_at.saturating_sub(now_seconds);
    let reset_after_seconds = window.reset_after_seconds.unwrap_or(derived_after);
    if reset_at < now_seconds - RESET_TOLERANCE_SECONDS || reset_after_seconds < 0 {
        return None;
    }
    if window.reset_after_seconds.is_some()
        && (derived_after - reset_after_seconds).abs() > RESET_TOLERANCE_SECONDS
    {
        return None;
    }
    // A server value much larger than the declared window is contradictory.
    if reset_after_seconds > window_seconds + RESET_TOLERANCE_SECONDS {
        return None;
    }
    let limit_id = match window.limit_id.as_str() {
        "" | "primary" | "secondary" | "unknown" => window_type.to_string(),
        value => value.to_string(),
    };
    Some(TargetWindow {
        limit_id,
        window_type,
        window_seconds,
        used_percent,
        reset_at,
        reset_after_seconds,
    })
}

fn consistent_bool<const N: usize>(values: [Option<bool>; N]) -> Option<bool> {
    let mut resolved = None;
    for value in values.into_iter().flatten() {
        if resolved.is_some_and(|current| current != value) {
            return None;
        }
        resolved = Some(value);
    }
    resolved
}

fn reached_type_is_blocking(
    observation: &CodexQuotaObservation,
    window: &CodexQuotaWindowObservation,
) -> bool {
    [
        observation.rate_limit_reached_type.as_deref(),
        window.rate_limit_reached_type.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .any(|value| !matches!(value.to_ascii_lowercase().as_str(), "none" | "not_reached"))
}

fn is_newly_available_window(policy: &CodexQuotaActivationPolicy, target: &TargetWindow) -> bool {
    let first_observation = policy.last_observed_limit_id.is_none()
        && policy.last_observed_window_seconds.is_none()
        && policy.last_observed_reset_at.is_none();
    if first_observation {
        if target.used_percent > f64::EPSILON || target.reset_after_seconds < 0 {
            return false;
        }

        // A policy can be enabled after the current window has already reset.
        // Treat that as startup catch-up, but do not consume a still-future
        // window merely because its first observation happens to be empty.
        // `updated_at` is milliseconds; reset generations are seconds.
        if policy.updated_at > 0 {
            let policy_updated_seconds = policy.updated_at / 1000;
            let generation_started_at = target.reset_at.saturating_sub(target.window_seconds);
            return generation_started_at <= policy_updated_seconds;
        }

        // Legacy rows/tests without an enable timestamp retain the conservative
        // near-full-window behavior rather than guessing the generation.
        return target.reset_after_seconds >= target.window_seconds - RESET_TOLERANCE_SECONDS;
    }
    if policy.last_observed_limit_id.as_deref() != Some(target.limit_id.as_str())
        || policy.last_observed_window_seconds != Some(target.window_seconds)
    {
        return false;
    }
    match policy.last_observed_reset_at {
        Some(previous_reset_at) if target.reset_at > previous_reset_at => true,
        Some(previous_reset_at) if target.reset_at == previous_reset_at => policy
            .last_observed_reset_after_seconds
            .is_some_and(|previous_after| {
                target.reset_after_seconds > previous_after + RESET_TOLERANCE_SECONDS
            }),
        Some(_) => false,
        None => false,
    }
}

fn resolve_activation_model(provider: &crate::provider::Provider) -> Option<ActivationModel> {
    let model = codex_provider_upstream_model(provider)?;
    let supports_low = provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| {
            entry
                .get("model")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&model))
        })
        .and_then(|entry| entry.get("reasoningLevels"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|levels| {
            levels.iter().any(|level| {
                level
                    .as_str()
                    .is_some_and(|level| level.eq_ignore_ascii_case("low"))
            })
        });
    Some(ActivationModel {
        model,
        reasoning_effort: supports_low.then(|| "low".to_string()),
    })
}

fn sanitize_error(error: &str, secrets: &[String]) -> String {
    let mut sanitized = crate::redact_known_secrets_strict(error, secrets);
    let home_dir = crate::config::get_home_dir();
    sanitized = sanitized.replace(&home_dir.to_string_lossy().to_string(), "<home>");
    sanitized.chars().take(MAX_ERROR_CHARS).collect()
}

fn mask_account_id(account_id: &str) -> String {
    let chars = account_id.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    format!(
        "{}…{}",
        chars[..4].iter().collect::<String>(),
        chars[chars.len() - 4..].iter().collect::<String>()
    )
}

fn emit_status(
    app: &AppHandle,
    account_id: &str,
    provider_name: &str,
    window_type: &str,
    model: Option<&str>,
    status: &str,
    error: Option<&str>,
) {
    let payload = ActivationEvent {
        account_id: mask_account_id(account_id),
        provider_name: provider_name.to_string(),
        window_type: window_type.to_string(),
        model: model.map(str::to_string),
        status: status.to_string(),
        time: chrono::Utc::now().timestamp_millis(),
        error: error.map(|error| sanitize_error(error, &[])),
    };
    if let Err(error) = app.emit("codex-quota-activation-updated", payload) {
        log::warn!("Failed to emit Codex quota activation status: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(settings_config: serde_json::Value) -> crate::provider::Provider {
        crate::provider::Provider::with_id(
            "provider".to_string(),
            "Provider".to_string(),
            settings_config,
            None,
        )
    }

    fn observation(windows: Vec<CodexQuotaWindowObservation>) -> CodexQuotaObservation {
        CodexQuotaObservation {
            plan_type: Some("plus".into()),
            allowed: Some(true),
            limit_reached: Some(false),
            rate_limit_reached_type: None,
            windows,
        }
    }

    fn window(seconds: i64, used: f64, reset_at: i64, after: i64) -> CodexQuotaWindowObservation {
        CodexQuotaWindowObservation {
            limit_id: "primary".into(),
            is_additional: false,
            used_percent: Some(used),
            limit_window_seconds: Some(seconds),
            reset_at: Some(reset_at),
            reset_after_seconds: Some(after),
            allowed: Some(true),
            limit_reached: Some(false),
            rate_limit_reached_type: None,
        }
    }

    fn policy(
        previous_reset_at: Option<i64>,
        previous_after: Option<i64>,
    ) -> CodexQuotaActivationPolicy {
        CodexQuotaActivationPolicy {
            account_id: "a".into(),
            owner_provider_id: "p".into(),
            enabled: true,
            updated_at: 0,
            last_status: None,
            last_error: None,
            last_window_type: None,
            last_model: None,
            last_attempt_at: None,
            last_observed_limit_id: None,
            last_observed_window_seconds: None,
            last_observed_reset_at: previous_reset_at,
            last_observed_reset_after_seconds: previous_after,
        }
    }

    #[test]
    fn five_hour_wins_over_weekly_regardless_of_position() {
        let now = 1_000_000;
        let target = select_target_window(
            &observation(vec![
                window(
                    SEVEN_DAYS_SECONDS,
                    0.0,
                    now + SEVEN_DAYS_SECONDS,
                    SEVEN_DAYS_SECONDS,
                ),
                window(
                    FIVE_HOURS_SECONDS,
                    0.0,
                    now + FIVE_HOURS_SECONDS,
                    FIVE_HOURS_SECONDS,
                ),
            ]),
            now,
        )
        .unwrap();
        assert_eq!(target.window_type, "five_hour");
        assert_eq!(target.limit_id, "five_hour");
    }

    #[test]
    fn pro_uses_weekly_window_even_if_a_five_hour_bucket_is_present() {
        let now = 1_000_000;
        let mut observed = observation(vec![
            window(
                FIVE_HOURS_SECONDS,
                0.0,
                now + FIVE_HOURS_SECONDS,
                FIVE_HOURS_SECONDS,
            ),
            window(
                SEVEN_DAYS_SECONDS,
                0.0,
                now + SEVEN_DAYS_SECONDS,
                SEVEN_DAYS_SECONDS,
            ),
        ]);
        observed.plan_type = Some("pro".into());
        let target = select_target_window(&observed, now).unwrap();
        assert_eq!(target.window_type, "weekly");
    }

    #[test]
    fn disabled_owner_usage_query_blocks_activation_without_deleting_policy() {
        let mut provider = crate::provider::Provider::with_id(
            "provider-a".into(),
            "Provider A".into(),
            serde_json::json!({}),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            usage_script: Some(crate::provider::UsageScript {
                enabled: false,
                language: "javascript".into(),
                code: String::new(),
                timeout: None,
                api_key: None,
                base_url: None,
                access_token: None,
                user_id: None,
                template_type: Some("official_subscription".into()),
                auto_query_interval: Some(5),
                coding_plan_provider: None,
                access_key_id: None,
                secret_access_key: None,
                team_organization_id: None,
                team_project_id: None,
            }),
            ..Default::default()
        });
        assert!(!owner_provider_allows_activation(&provider));
    }

    #[test]
    fn weekly_only_is_supported_but_monthly_only_is_not() {
        let now = 1_000_000;
        assert_eq!(
            select_target_window(
                &observation(vec![window(
                    SEVEN_DAYS_SECONDS,
                    0.0,
                    now + SEVEN_DAYS_SECONDS,
                    SEVEN_DAYS_SECONDS
                )]),
                now
            )
            .unwrap()
            .window_type,
            "weekly"
        );
        assert!(select_target_window(
            &observation(vec![window(30 * 24 * 3600, 0.0, now + 100, 100)]),
            now
        )
        .is_none());
    }

    #[test]
    fn ambiguous_or_contradictory_buckets_fail_closed() {
        let now = 1_000_000;
        assert!(select_target_window(
            &observation(vec![
                window(
                    FIVE_HOURS_SECONDS,
                    0.0,
                    now + FIVE_HOURS_SECONDS,
                    FIVE_HOURS_SECONDS
                ),
                window(
                    FIVE_HOURS_SECONDS,
                    0.0,
                    now + FIVE_HOURS_SECONDS,
                    FIVE_HOURS_SECONDS
                ),
            ]),
            now
        )
        .is_none());
        let mut blocked = observation(vec![window(
            FIVE_HOURS_SECONDS,
            0.0,
            now + FIVE_HOURS_SECONDS,
            FIVE_HOURS_SECONDS,
        )]);
        blocked.limit_reached = Some(true);
        assert!(select_target_window(&blocked, now).is_none());
        let mut typed = observation(vec![window(
            FIVE_HOURS_SECONDS,
            0.0,
            now + FIVE_HOURS_SECONDS,
            FIVE_HOURS_SECONDS,
        )]);
        typed.rate_limit_reached_type = Some("primary".into());
        assert!(select_target_window(&typed, now).is_none());
        let mut disallowed = observation(vec![window(
            FIVE_HOURS_SECONDS,
            0.0,
            now + FIVE_HOURS_SECONDS,
            FIVE_HOURS_SECONDS,
        )]);
        disallowed.allowed = Some(false);
        assert!(select_target_window(&disallowed, now).is_none());
    }

    #[test]
    fn additional_bucket_with_target_duration_fails_closed() {
        let now = 1_000_000;
        let mut additional = window(
            FIVE_HOURS_SECONDS,
            0.0,
            now + FIVE_HOURS_SECONDS,
            FIVE_HOURS_SECONDS,
        );
        additional.is_additional = true;
        assert!(select_target_window(
            &observation(vec![
                window(
                    FIVE_HOURS_SECONDS,
                    0.0,
                    now + FIVE_HOURS_SECONDS,
                    FIVE_HOURS_SECONDS,
                ),
                additional
            ]),
            now
        )
        .is_none());
    }

    #[test]
    fn startup_catch_up_and_reset_generation_are_detected() {
        let now = 1_000_000;
        let fresh = select_target_window(
            &observation(vec![window(
                FIVE_HOURS_SECONDS,
                0.0,
                now + FIVE_HOURS_SECONDS,
                FIVE_HOURS_SECONDS,
            )]),
            now,
        )
        .unwrap();
        assert!(is_newly_available_window(&policy(None, None), &fresh));

        let in_progress = TargetWindow {
            used_percent: 20.0,
            reset_after_seconds: 600,
            reset_at: now + 600,
            ..fresh.clone()
        };
        assert!(!is_newly_available_window(
            &policy(None, None),
            &in_progress
        ));
        assert!(!is_newly_available_window(
            &policy(Some(fresh.reset_at), Some(fresh.reset_after_seconds - 60)),
            &fresh
        ));
        let mut previous = policy(Some(fresh.reset_at - FIVE_HOURS_SECONDS), Some(10));
        previous.last_observed_limit_id = Some("five_hour".into());
        previous.last_observed_window_seconds = Some(FIVE_HOURS_SECONDS);
        assert!(is_newly_available_window(&previous, &fresh));
    }

    #[test]
    fn startup_catch_up_after_reset_is_detected_without_full_window_remaining() {
        let now = 1_000_000;
        let mut enabled_policy = policy(None, None);
        enabled_policy.updated_at = (now - 2 * 60 * 60) * 1000;
        let target = TargetWindow {
            limit_id: "weekly".into(),
            window_type: "weekly",
            window_seconds: SEVEN_DAYS_SECONDS,
            used_percent: 0.0,
            reset_at: now + SEVEN_DAYS_SECONDS - 2 * 60 * 60,
            reset_after_seconds: SEVEN_DAYS_SECONDS - 2 * 60 * 60,
        };
        assert!(is_newly_available_window(&enabled_policy, &target));

        let mut future_window_policy = policy(None, None);
        future_window_policy.updated_at = (now - 60) * 1000;
        let future_window = TargetWindow {
            reset_at: now + SEVEN_DAYS_SECONDS,
            reset_after_seconds: SEVEN_DAYS_SECONDS,
            ..target
        };
        assert!(!is_newly_available_window(
            &future_window_policy,
            &future_window
        ));
    }

    #[test]
    fn model_and_low_effort_require_explicit_capability() {
        let with_low = serde_json::json!({
            "config": "model = \"gpt-test\"\n",
            "modelCatalog": { "models": [
                { "model": "gpt-test", "reasoningLevels": ["low", "high"] }
            ] }
        });
        assert_eq!(
            resolve_activation_model(&provider(with_low)),
            Some(ActivationModel {
                model: "gpt-test".into(),
                reasoning_effort: Some("low".into())
            })
        );
        let no_catalog = serde_json::json!({ "config": "model = \"gpt-test\"\n" });
        assert_eq!(
            resolve_activation_model(&provider(no_catalog)),
            Some(ActivationModel {
                model: "gpt-test".into(),
                reasoning_effort: None
            })
        );
    }

    #[test]
    fn missing_explicit_model_fails_closed() {
        let official = provider(serde_json::json!({ "auth": {}, "config": "" }));
        assert!(resolve_activation_model(&official).is_none());
    }
}
