//! Optional, UI-independent refresh of managed Codex account quotas.

use std::future::Future;
use std::time::{Duration, Instant};

use futures::{stream, StreamExt};
use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use crate::commands::CodexOAuthState;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::services::subscription::{query_codex_quota, CredentialStatus, SubscriptionQuota};
use crate::services::usage_cache::UsageCache;
use crate::store::AppState;

static BATCH_LOCK: Mutex<()> = Mutex::const_new(());
const MAX_CONCURRENT: usize = 3;
const TRANSPORT_ERROR: &str = "Network error while querying Codex quota";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountQuotaRefreshResult {
    pub account_id: String,
    pub quota: Option<SubscriptionQuota>,
    pub error: Option<String>,
}

/// A non-blocking gate prevents manual and scheduled batches from overlapping.
pub async fn refresh_all(
    app: &tauri::AppHandle,
) -> Result<Vec<CodexAccountQuotaRefreshResult>, String> {
    let _batch = BATCH_LOCK
        .try_lock()
        .map_err(|_| "Codex account quota refresh is already running".to_string())?;
    let manager = app.state::<CodexOAuthState>().0.clone();
    let accounts = manager.list_accounts().await;
    let results = refresh_accounts(
        accounts
            .into_iter()
            .map(|account| (account.id, account.reauth_required))
            .collect(),
        |id| {
            let manager = manager.clone();
            async move {
                let initial = manager.quota_credential_snapshot(&id, None).await
                    .ok_or_else(|| "Codex account was removed during refresh".to_string())?;
                let (result, snapshot) = match manager.get_valid_token_for_account(&id).await {
                    Ok(token) => {
                        let snapshot = manager.quota_credential_snapshot(&id, Some(&token)).await
                            .ok_or_else(|| "Codex account credentials changed during refresh".to_string())?;
                        let workspace = manager.chatgpt_account_id_for_account(&id).await
                            .map_err(|_| "Codex account credentials changed during refresh".to_string())?;
                        let result = query_codex_quota(&token, Some(&workspace), "codex_oauth",
                            "Codex OAuth access token expired or rejected. Please re-login via cc-switch.").await;
                        (result, snapshot)
                    }
                    Err(_) => (Ok(SubscriptionQuota::error("codex_oauth", CredentialStatus::Expired,
                        "Codex OAuth token unavailable. Please re-login via cc-switch.".to_string())), initial),
                };
                manager.with_current_quota_credentials(&id, &snapshot, || {
                    let payload = quota_update_payload(&app.state::<AppState>().usage_cache, &id, &result);
                    let _ = app.emit("usage-cache-updated", payload);
                    if result.is_ok() {
                        crate::tray::schedule_tray_refresh(app);
                    }
                }).await.ok_or_else(|| "Codex account credentials changed during refresh".to_string())?;
                // Do not expose transport diagnostics (which can include URLs) in events.
                result.map_err(|_| TRANSPORT_ERROR.to_string())
            }
        },
    )
    .await;
    Ok(results)
}

/// Structured failures replace invalid quotas. Transport errors publish a transient
/// failure for the UI's keep-last-good policy without replacing the tray snapshot.
fn quota_update_payload(
    cache: &UsageCache,
    id: &str,
    result: &Result<SubscriptionQuota, String>,
) -> serde_json::Value {
    let quota = match result {
        Ok(quota) => {
            cache.put_codex_oauth(id.to_string(), quota.clone());
            quota.clone()
        }
        Err(_) => SubscriptionQuota::error(
            "codex_oauth",
            CredentialStatus::Valid,
            TRANSPORT_ERROR.to_string(),
        ),
    };
    serde_json::json!({ "kind": "codex_oauth", "accountId": id, "data": quota })
}

async fn refresh_accounts<F, Fut>(
    accounts: Vec<(String, bool)>,
    query: F,
) -> Vec<CodexAccountQuotaRefreshResult>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<SubscriptionQuota, String>>,
{
    stream::iter(
        accounts
            .into_iter()
            .filter(|(_, reauth_required)| !reauth_required)
            .map(|(id, _)| {
                let query = &query;
                async move {
                    let result = query(id.clone()).await;
                    match result {
                        Ok(quota) => CodexAccountQuotaRefreshResult {
                            account_id: id,
                            error: if quota.success {
                                None
                            } else {
                                quota
                                    .error
                                    .clone()
                                    .or_else(|| quota.credential_message.clone())
                                    .or_else(|| Some("Codex quota unavailable".to_string()))
                            },
                            quota: Some(quota),
                        },
                        Err(error) => CodexAccountQuotaRefreshResult {
                            account_id: id,
                            quota: None,
                            error: Some(error),
                        },
                    }
                }
            }),
    )
    .buffer_unordered(MAX_CONCURRENT)
    .collect()
    .await
}

#[derive(Default)]
struct RefreshSchedule {
    enabled: bool,
    last_started: Option<Instant>,
}

impl RefreshSchedule {
    fn due(&mut self, minutes: Option<u32>, now: Instant) -> bool {
        let Some(minutes) = minutes.filter(|minutes| *minutes > 0) else {
            self.enabled = false;
            self.last_started = None;
            return false;
        };
        let interval = Duration::from_secs(u64::from(minutes.clamp(1, 60)) * 60);
        let due = !self.enabled
            || self
                .last_started
                .is_none_or(|last| now.duration_since(last) >= interval);
        self.enabled = true;
        if due {
            self.last_started = Some(now);
        }
        due
    }
}

pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut schedule = RefreshSchedule::default();
        loop {
            let minutes = crate::settings::get_settings().codex_account_quota_refresh_minutes;
            if schedule.due(minutes, Instant::now()) {
                let app = app.clone();
                // Settings are rechecked even if a network request is slow. The gate in
                // refresh_all makes a scheduled tick during an active batch a no-op.
                tauri::async_runtime::spawn(async move {
                    let _ = refresh_all(&app).await;
                });
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

pub(crate) async fn query_codex_oauth_quota_for(
    manager: &CodexOAuthManager,
    id: &str,
) -> Result<SubscriptionQuota, String> {
    // 获取（必要时自动刷新）access_token
    let token = match manager.get_valid_token_for_account(id).await {
        Ok(t) => t,
        Err(e) => {
            return Ok(SubscriptionQuota::error(
                "codex_oauth",
                CredentialStatus::Expired,
                format!("Codex OAuth token unavailable: {e}"),
            ));
        }
    };
    let chatgpt_account_id = manager
        .chatgpt_account_id_for_account(id)
        .await
        .map_err(|e| e.to_string())?;

    // 瞬时传输失败以 Err 传播（前端 reject → retry + 保留上次成功值）。
    query_codex_quota(
        &token,
        Some(&chatgpt_account_id),
        "codex_oauth",
        "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn schedule_tracks_enable_disable_and_live_interval_changes() {
        let mut schedule = RefreshSchedule::default();
        let now = Instant::now();
        assert!(!schedule.due(None, now));
        assert!(!schedule.due(Some(0), now));
        assert!(schedule.due(Some(5), now));
        assert!(!schedule.due(Some(5), now + Duration::from_secs(60)));
        // Shortening an interval applies against the last start, without restarting.
        assert!(schedule.due(Some(1), now + Duration::from_secs(60)));
        // Lengthening it does not trigger a premature refresh.
        assert!(!schedule.due(Some(10), now + Duration::from_secs(120)));
        assert!(!schedule.due(None, now + Duration::from_secs(125)));
        assert!(schedule.due(Some(10), now + Duration::from_secs(130)));
    }

    #[test]
    fn schedule_clamps_large_intervals() {
        let mut schedule = RefreshSchedule::default();
        let now = Instant::now();
        assert!(schedule.due(Some(u32::MAX), now));
        assert!(!schedule.due(Some(u32::MAX), now + Duration::from_secs(3599)));
        assert!(schedule.due(Some(u32::MAX), now + Duration::from_secs(3600)));
    }

    #[tokio::test]
    async fn batch_bounds_concurrency_preserves_partial_results_and_skips_reauth() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let calls = AtomicUsize::new(0);
        let results = refresh_accounts((0..8).map(|i| (i.to_string(), i == 7)).collect(), |id| {
            let active = &active;
            let peak = &peak;
            let calls = &calls;
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                match id.as_str() {
                    "1" => Err("transport failure".to_string()),
                    "2" => Err("Codex account was removed during refresh".to_string()),
                    "3" => Ok(SubscriptionQuota::error(
                        "codex_oauth",
                        CredentialStatus::Expired,
                        "expired".to_string(),
                    )),
                    _ => {
                        let mut quota = SubscriptionQuota::not_found("codex_oauth");
                        quota.success = true;
                        Ok(quota)
                    }
                }
            }
        })
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 7);
        assert_eq!(peak.load(Ordering::SeqCst), MAX_CONCURRENT);
        assert_eq!(results.len(), 7);
        assert!(results.iter().all(|result| result.account_id != "7"));
        assert_eq!(
            results
                .iter()
                .filter(|result| result.error.is_none())
                .count(),
            4
        );
        let expired = results
            .iter()
            .find(|result| result.account_id == "3")
            .unwrap();
        assert!(expired.quota.is_some());
        assert_eq!(expired.error.as_deref(), Some("expired"));
        for id in ["1", "2"] {
            let result = results
                .iter()
                .find(|result| result.account_id == id)
                .unwrap();
            assert!(result.quota.is_none());
            assert!(result.error.is_some());
        }
    }

    #[test]
    fn transport_failure_publishes_transient_error_without_replacing_good_cache() {
        let cache = UsageCache::new();
        let mut good = SubscriptionQuota::not_found("codex_oauth");
        good.success = true;
        good.queried_at = Some(123);
        cache.put_codex_oauth("account".to_string(), good);
        let payload = quota_update_payload(
            &cache,
            "account",
            &Err("private transport diagnostic".to_string()),
        );
        assert_eq!(payload["kind"], "codex_oauth");
        assert_eq!(payload["accountId"], "account");
        assert_eq!(payload["data"]["success"], false);
        assert_eq!(payload["data"]["credentialStatus"], "valid");
        assert_eq!(payload["data"]["error"], TRANSPORT_ERROR);
        assert!(!payload.to_string().contains("private transport diagnostic"));
        assert_eq!(
            cache.with_codex_oauth("account", |quota| (quota.success, quota.queried_at)),
            Some((true, Some(123)))
        );
    }

    #[test]
    fn structured_failure_replaces_cache_and_publishes_account_scoped_snapshot() {
        let cache = UsageCache::new();
        let mut good = SubscriptionQuota::not_found("codex_oauth");
        good.success = true;
        cache.put_codex_oauth("account".to_string(), good.clone());
        cache.put_codex_oauth("other".to_string(), good);
        let failure = SubscriptionQuota::error(
            "codex_oauth",
            CredentialStatus::Expired,
            "expired".to_string(),
        );
        let payload = quota_update_payload(&cache, "account", &Ok(failure));
        assert_eq!(payload["accountId"], "account");
        assert_eq!(payload["data"]["credentialStatus"], "expired");
        assert_eq!(
            cache.with_codex_oauth("account", |quota| quota.success),
            Some(false)
        );
        assert_eq!(
            cache.with_codex_oauth("other", |quota| quota.success),
            Some(true)
        );
    }
}
