//! Pre-exhaustion admission for native ChatGPT requests in the Codex proxy.
//! Queries use the request's credentials, never the UI's current-account cache.
//! A permit lasts through the response body so concurrent turns cannot all pass
//! on the same pre-request reading. This cannot reserve quota against other apps
//! or bound the consumption of an already admitted request.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, OnceLock},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures::StreamExt;
use http::HeaderMap;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard};

use super::{hyper_client::ProxyResponse, ProxyError};
use crate::{
    services::subscription::{
        query_codex_usage, SubscriptionQuota, TIER_FIVE_HOUR, TIER_SEVEN_DAY, TIER_WEEKLY_LIMIT,
    },
    settings::CodexQuotaProtection,
};

const RECOVERY: &str = "已暂停新的官方请求，第三方模型仍可使用。额度重新查询低于阈值后自动恢复；可在 CC Switch 设置 → 应用配置 → Codex 调整或关闭官方额度保护。 (Official requests paused; third-party models remain available. Retry after usage falls below the thresholds, or adjust/disable protection in CC Switch settings.)";

#[derive(Default)]
struct Observation {
    five_hour: Option<(f64, Option<String>)>,
    weekly: Option<(f64, Option<String>)>,
}

impl Observation {
    fn observe(
        &mut self,
        quota: &SubscriptionQuota,
        config: &CodexQuotaProtection,
    ) -> Result<(), ProxyError> {
        let mut five_hour_reported = false;
        let mut weekly_reported = false;
        let mut invalid = !quota.success;
        if quota.success {
            for tier in &quota.tiers {
                let slot = match tier.name.as_str() {
                    TIER_FIVE_HOUR => {
                        five_hour_reported = true;
                        &mut self.five_hour
                    }
                    TIER_SEVEN_DAY | TIER_WEEKLY_LIMIT => {
                        weekly_reported = true;
                        &mut self.weekly
                    }
                    _ => continue,
                };
                if !tier.utilization.is_finite() || !(0.0..=100.0).contains(&tier.utilization) {
                    invalid = true;
                    continue;
                }
                *slot = Some((tier.utilization, tier.resets_at.clone()));
            }
        }
        // A missing window or failed query must never erase an observed block.
        let mut reasons = Vec::new();
        for (label, window, threshold) in [
            ("五小时 (5h)", &self.five_hour, config.five_hour_threshold),
            ("每周 (weekly)", &self.weekly, config.weekly_threshold),
        ] {
            if let Some((used, resets_at)) = window {
                if *used >= f64::from(threshold) {
                    let reset = resets_at.as_deref().unwrap_or("未知 / unknown");
                    reasons.push(format!(
                        "{label}: {used:.2}% ≥ {threshold}%（重置 / reset: {reset}）"
                    ));
                }
            }
        }
        if !reasons.is_empty() {
            return Err(ProxyError::CodexQuotaProtection(format!(
                "{}。{RECOVERY}",
                reasons.join("; ")
            )));
        }
        if invalid
            || (!five_hour_reported && !weekly_reported)
            || (self.five_hour.is_some() && !five_hour_reported)
            || (self.weekly.is_some() && !weekly_reported)
        {
            // No stale success fallback and no response bodies/tokens in the error.
            return Err(unavailable());
        }
        Ok(())
    }
}

fn unavailable() -> ProxyError {
    ProxyError::CodexQuotaProtection(format!(
        "无法确认账号的五小时或周额度，请检查 ChatGPT 登录与网络 (quota unavailable; check ChatGPT login and network)。{RECOVERY}"
    ))
}

#[derive(Default)]
struct QuotaGuard {
    accounts: Mutex<HashMap<String, Arc<Mutex<Observation>>>>,
}

/// Uses the JWT owner plus workspace when available so token refresh and
/// duplicate cards keep the same gate. Claims identify a lock only: upstream
/// authorization is still performed using the original bearer on every query.
fn account_key(token: &str, account_id: Option<&str>) -> String {
    let claims = token
        .split('.')
        .nth(1)
        .and_then(|part| URL_SAFE_NO_PAD.decode(part).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let owner = claims
        .as_ref()
        .and_then(|claims| claims.get("sub"))
        .and_then(|v| v.as_str());
    let mut hash = Sha256::new();
    hash.update(owner.unwrap_or(token).as_bytes());
    hash.update([0]);
    hash.update(account_id.unwrap_or("").as_bytes());
    format!("{:x}", hash.finalize())
}

impl QuotaGuard {
    async fn check_with<F, Q>(
        &self,
        key: String,
        config: impl Fn() -> CodexQuotaProtection,
        query: Q,
    ) -> Result<Option<QuotaPermit>, ProxyError>
    where
        F: Future<Output = Result<SubscriptionQuota, String>>,
        Q: FnOnce() -> F,
    {
        if !config().enabled {
            return Ok(None);
        }
        let account = {
            let mut accounts = self.accounts.lock().await;
            // Bound idle identities without ever evicting an active/queued gate.
            if accounts.len() >= 256 && !accounts.contains_key(&key) {
                accounts.retain(|_, gate| Arc::strong_count(gate) > 1);
            }
            accounts.entry(key).or_default().clone()
        };
        let mut permit = account.lock_owned().await;
        if !config().enabled {
            return Ok(None);
        }
        // Never reuse a successful quota snapshot for the next inference.
        let result = query().await;
        let config = config();
        if !config.enabled {
            return Ok(None);
        }
        config.validate().map_err(|_| unavailable())?;
        let quota = result.map_err(|_| unavailable())?;
        permit.observe(&quota, &config)?;
        Ok(Some(QuotaPermit { _guard: permit }))
    }
}

pub(crate) struct QuotaPermit {
    _guard: OwnedMutexGuard<Observation>,
}

pub(crate) async fn acquire(
    token: &str,
    headers: &HeaderMap,
) -> Result<Option<QuotaPermit>, ProxyError> {
    static GUARD: OnceLock<QuotaGuard> = OnceLock::new();
    let account_id = headers
        .get("chatgpt-account-id")
        .and_then(|value| value.to_str().ok());
    GUARD
        .get_or_init(QuotaGuard::default)
        .check_with(
            account_key(token, account_id),
            || crate::settings::get_settings().codex_quota_protection,
            || query_codex_usage(token, account_id, "codex", "ChatGPT login needs refreshing"),
        )
        .await
}

pub(crate) fn hold_response(response: ProxyResponse, permit: QuotaPermit) -> ProxyResponse {
    let status = response.status();
    let headers = response.headers().clone();
    let mut upstream = response.bytes_stream();
    ProxyResponse::streamed(
        status,
        headers,
        async_stream::stream! {
            let _permit = permit;
            while let Some(chunk) = upstream.next().await {
                let failed = chunk.is_err();
                yield chunk;
                if failed { break; }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::QuotaTier;
    use bytes::Bytes;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn quota(five_hour: Option<f64>, weekly: Option<f64>) -> SubscriptionQuota {
        let mut quota = SubscriptionQuota::not_found("codex");
        quota.success = true;
        quota.tiers = [(TIER_FIVE_HOUR, five_hour), (TIER_SEVEN_DAY, weekly)]
            .into_iter()
            .filter_map(|(name, used)| {
                used.map(|utilization| QuotaTier {
                    name: name.to_string(),
                    utilization,
                    resets_at: Some("2026-10-16T00:00:00Z".to_string()),
                    used_value_usd: None,
                    max_value_usd: None,
                })
            })
            .collect();
        quota
    }

    #[test]
    fn defaults_and_saved_thresholds_validate() {
        let defaults: CodexQuotaProtection = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults, CodexQuotaProtection::default());
        assert!(defaults.enabled);
        assert_eq!(
            (defaults.five_hour_threshold, defaults.weekly_threshold),
            (95, 98)
        );
        for value in [0, 100, 255] {
            assert!(CodexQuotaProtection {
                five_hour_threshold: value,
                ..defaults.clone()
            }
            .validate()
            .is_err());
            assert!(CodexQuotaProtection {
                weekly_threshold: value,
                ..defaults.clone()
            }
            .validate()
            .is_err());
        }
        assert!(
            serde_json::from_str::<CodexQuotaProtection>(r#"{"fiveHourThreshold":95.5}"#).is_err()
        );
        let custom = CodexQuotaProtection {
            five_hour_threshold: 91,
            weekly_threshold: 97,
            ..defaults
        };
        assert_eq!(
            serde_json::from_str::<CodexQuotaProtection>(&serde_json::to_string(&custom).unwrap())
                .unwrap(),
            custom
        );
    }

    #[test]
    fn either_window_blocks_at_the_exact_threshold_and_fresh_recovery_releases() {
        let config = CodexQuotaProtection::default();
        let mut observed = Observation::default();
        assert!(observed
            .observe(&quota(Some(94.99), Some(97.99)), &config)
            .is_ok());
        assert!(observed
            .observe(&quota(Some(95.0), Some(10.0)), &config)
            .is_err());
        assert!(observed
            .observe(&quota(Some(0.0), Some(98.0)), &config)
            .is_err());
        assert!(observed
            .observe(&quota(Some(95.0), Some(98.0)), &config)
            .is_err());
        assert!(observed
            .observe(&quota(Some(0.0), Some(98.0)), &config)
            .is_err());
        assert!(observed.observe(&quota(Some(0.0), None), &config).is_err());
        assert!(observed
            .observe(&quota(Some(0.0), Some(97.99)), &config)
            .is_ok());
    }

    #[test]
    fn malformed_or_failed_observations_never_clear_protection() {
        let config = CodexQuotaProtection::default();
        for value in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
            assert!(Observation::default()
                .observe(&quota(Some(value), Some(0.0)), &config)
                .is_err());
        }
        let mut observed = Observation::default();
        assert!(observed
            .observe(&quota(Some(95.0), Some(0.0)), &config)
            .is_err());
        assert!(observed
            .observe(&SubscriptionQuota::not_found("codex"), &config)
            .is_err());
        assert!(observed.observe(&quota(None, Some(0.0)), &config).is_err());
        assert!(Observation::default()
            .observe(&quota(None, None), &config)
            .is_err());
        let custom = CodexQuotaProtection {
            five_hour_threshold: 90,
            weekly_threshold: 92,
            ..config
        };
        assert!(Observation::default()
            .observe(&quota(Some(91.0), Some(0.0)), &custom)
            .is_err());
    }

    #[test]
    fn renewed_bearers_share_owner_gate_but_workspace_and_owner_are_isolated() {
        let token = |owner, generation| {
            format!(
                "header.{}.{}",
                URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{owner}"}}"#)),
                generation
            )
        };
        assert_eq!(
            account_key(&token("a", 1), Some("workspace")),
            account_key(&token("a", 2), Some("workspace"))
        );
        assert_ne!(
            account_key(&token("a", 1), Some("workspace")),
            account_key(&token("b", 1), Some("workspace"))
        );
        assert_ne!(
            account_key(&token("a", 1), Some("workspace")),
            account_key(&token("a", 1), Some("other"))
        );
        assert_ne!(account_key("opaque-a", None), account_key("opaque-b", None));
    }

    #[tokio::test]
    async fn disabled_never_queries_and_lookup_errors_do_not_leak_secrets() {
        let guard = QuotaGuard::default();
        let disabled = || CodexQuotaProtection {
            enabled: false,
            ..Default::default()
        };
        assert!(guard
            .check_with("account".into(), disabled, || async {
                panic!("must not query")
            })
            .await
            .unwrap()
            .is_none());
        let error = guard
            .check_with("account".into(), CodexQuotaProtection::default, || async {
                Err("secret-token-in-network-error".to_string())
            })
            .await
            .err()
            .unwrap();
        assert!(!error.to_string().contains("secret-token"));
    }

    #[tokio::test]
    async fn response_lifetime_serializes_same_account_then_rechecks_fresh_quota() {
        let guard = QuotaGuard::default();
        let queries = AtomicUsize::new(0);
        let first = guard
            .check_with("a".into(), CodexQuotaProtection::default, || async {
                queries.fetch_add(1, Ordering::SeqCst);
                Ok(quota(Some(94.0), Some(0.0)))
            })
            .await
            .unwrap()
            .unwrap();
        let response = hold_response(
            ProxyResponse::buffered(
                http::StatusCode::OK,
                HeaderMap::new(),
                Bytes::from_static(b"done"),
            ),
            first,
        );
        // A different account does not wait behind the held stream.
        drop(
            guard
                .check_with("b".into(), CodexQuotaProtection::default, || async {
                    Ok(quota(Some(0.0), Some(0.0)))
                })
                .await
                .unwrap(),
        );
        let next = guard.check_with("a".into(), CodexQuotaProtection::default, || async {
            queries.fetch_add(1, Ordering::SeqCst);
            Ok(quota(Some(95.0), Some(0.0)))
        });
        tokio::pin!(next);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut next)
                .await
                .is_err()
        );
        assert_eq!(queries.load(Ordering::SeqCst), 1);
        // Dropping even an unpolled body releases the permit (client cancellation).
        drop(response);
        assert!(next.await.is_err());
        assert_eq!(queries.load(Ordering::SeqCst), 2);
        // Fresh usage below both cutoffs automatically restores admission.
        let permit = guard
            .check_with("a".into(), CodexQuotaProtection::default, || async {
                Ok(quota(Some(0.0), Some(0.0)))
            })
            .await
            .unwrap()
            .unwrap();
        let response = hold_response(
            ProxyResponse::buffered(
                http::StatusCode::OK,
                HeaderMap::new(),
                Bytes::from_static(b"complete"),
            ),
            permit,
        );
        assert_eq!(
            response.bytes_stream().next().await.unwrap().unwrap(),
            Bytes::from_static(b"complete")
        );
        assert!(guard
            .check_with("a".into(), CodexQuotaProtection::default, || async {
                Ok(quota(Some(0.0), Some(0.0)))
            })
            .await
            .is_ok());
    }
}
