//! Pre-exhaustion protection for native ChatGPT requests in the Codex proxy.
//!
//! Every official response already carries the account's usage in its
//! `x-codex-*` rate-limit headers, so the normal path costs no extra request:
//! the proxy records those headers per account and checks the next request
//! against them. `wham/usage` is queried only when no recent reading exists
//! (first request, after an idle period, or once a window has reset).
//!
//! Protection is best effort and never fails closed: when usage is unknown
//! (lookup failure, expired login, no 5-hour or weekly window) the request goes
//! upstream unchanged. It cannot reserve quota for requests already admitted.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use http::HeaderMap;
use sha2::{Digest, Sha256};

use super::ProxyError;
use crate::{
    services::subscription::{
        query_codex_usage, SubscriptionQuota, TIER_FIVE_HOUR, TIER_SEVEN_DAY, TIER_WEEKLY_LIMIT,
    },
    settings::CodexQuotaProtection,
};

/// A reading older than this is confirmed again before it can block or admit.
const SNAPSHOT_TTL_SECS: i64 = 300;
/// Bounds the extra latency of a cold-cache lookup; a slow lookup admits.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ACCOUNTS: usize = 256;
const FIVE_HOUR_MINUTES: i64 = 300;
const WEEKLY_MINUTES: i64 = 10_080;

const RECOVERY: &str = "已暂停新的官方请求，第三方模型仍可使用。使用率回落到阈值以下或窗口重置后自动恢复（约 5 分钟内重新确认）；可在 CC Switch 设置 → 应用配置 → Codex 调整或关闭官方额度保护。如果额度已经用完、Codex 锁住了发送键，在 CC Switch 的 Codex 页面打开路由或聚合页签右侧的设置，打开「暂停使用官方登录」并重启 Codex。 (Official requests paused; third-party models remain available. Requests resume once usage is below the thresholds or the window resets, rechecked within about 5 minutes. Adjust or disable protection in CC Switch settings. If the quota is already used up and Codex has locked the send button, open the Routing or Aggregation settings on the Codex page in CC Switch, turn on \"Pause the official login\" and restart Codex.)";

#[derive(Debug, Clone, PartialEq)]
struct Window {
    used_percent: f64,
    /// Unix seconds; `None` when upstream did not report a reset time.
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    five_hour: Option<Window>,
    weekly: Option<Window>,
    observed_at: i64,
}

impl Snapshot {
    fn empty(now: i64) -> Self {
        Self {
            five_hour: None,
            weekly: None,
            observed_at: now,
        }
    }

    fn windows(&self) -> impl Iterator<Item = &Window> {
        [&self.five_hour, &self.weekly].into_iter().flatten()
    }

    fn is_fresh(&self, now: i64) -> bool {
        now - self.observed_at < SNAPSHOT_TTL_SECS
            && self
                .windows()
                .all(|window| window.resets_at.is_none_or(|reset| reset > now))
    }

    fn check(&self, config: &CodexQuotaProtection) -> Result<(), ProxyError> {
        let mut reasons = Vec::new();
        for (label, window, threshold) in [
            ("五小时 (5h)", &self.five_hour, config.five_hour_threshold),
            ("每周 (weekly)", &self.weekly, config.weekly_threshold),
        ] {
            let Some(window) = window else { continue };
            if window.used_percent >= f64::from(threshold) {
                let reset = window
                    .resets_at
                    .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                    .map(|time| time.to_rfc3339())
                    .unwrap_or_else(|| "未知 / unknown".to_string());
                reasons.push(format!(
                    "{label}: {:.2}% ≥ {threshold}%（重置 / reset: {reset}）",
                    window.used_percent
                ));
            }
        }
        if reasons.is_empty() {
            return Ok(());
        }
        Err(ProxyError::CodexQuotaProtection(format!(
            "{}。{RECOVERY}",
            reasons.join("; ")
        )))
    }

    fn from_quota(quota: &SubscriptionQuota, now: i64) -> Self {
        let mut snapshot = Self::empty(now);
        for tier in &quota.tiers {
            let slot = match tier.name.as_str() {
                TIER_FIVE_HOUR => &mut snapshot.five_hour,
                TIER_SEVEN_DAY | TIER_WEEKLY_LIMIT => &mut snapshot.weekly,
                _ => continue,
            };
            if !tier.utilization.is_finite() || tier.utilization < 0.0 {
                continue;
            }
            *slot = Some(Window {
                used_percent: tier.utilization,
                resets_at: tier
                    .resets_at
                    .as_deref()
                    .and_then(|iso| chrono::DateTime::parse_from_rfc3339(iso).ok())
                    .map(|time| time.timestamp()),
            });
        }
        snapshot
    }

    /// Reads `x-<limit>-{primary,secondary}-*` usage headers. `None` when the
    /// response carries no usage headers at all, so the cached reading stays.
    fn from_headers(headers: &HeaderMap, now: i64) -> Option<Self> {
        let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
        let active = header("x-codex-active-limit")
            .map(str::trim)
            .filter(|limit| !limit.is_empty() && *limit != "codex");
        for prefix in active.into_iter().chain(["codex"]) {
            let mut reported = false;
            let mut snapshot = Self::empty(now);
            for slot in ["primary", "secondary"] {
                let field = |suffix: &str| header(&format!("x-{prefix}-{slot}-{suffix}"));
                let Some(used) = field("used-percent").and_then(|v| v.trim().parse::<f64>().ok())
                else {
                    continue;
                };
                reported = true;
                if !used.is_finite() || used < 0.0 {
                    continue;
                }
                let target = match field("window-minutes").and_then(|v| v.trim().parse().ok()) {
                    Some(FIVE_HOUR_MINUTES) => &mut snapshot.five_hour,
                    Some(WEEKLY_MINUTES) => &mut snapshot.weekly,
                    _ => continue,
                };
                let resets_at = field("reset-at")
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .or_else(|| {
                        field("reset-after-seconds")
                            .and_then(|v| v.trim().parse::<i64>().ok())
                            .map(|after| now + after)
                    });
                *target = Some(Window {
                    used_percent: used,
                    resets_at,
                });
            }
            if reported {
                return Some(snapshot);
            }
        }
        None
    }
}

struct QuotaCache {
    accounts: Mutex<HashMap<String, Snapshot>>,
    lookup_timeout: Duration,
}

impl Default for QuotaCache {
    fn default() -> Self {
        Self {
            accounts: Mutex::default(),
            lookup_timeout: LOOKUP_TIMEOUT,
        }
    }
}

impl QuotaCache {
    fn fresh(&self, key: &str, now: i64) -> Option<Snapshot> {
        let accounts = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        accounts
            .get(key)
            .filter(|snapshot| snapshot.is_fresh(now))
            .cloned()
    }

    fn store(&self, key: String, snapshot: Snapshot) {
        let mut accounts = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        Self::insert(&mut accounts, key, snapshot);
    }

    /// Stores a lookup result unless a fresh reading arrived while it was in
    /// flight (response headers or a concurrent lookup); the newer one wins.
    fn store_lookup(&self, key: String, snapshot: Snapshot, now: i64) -> Snapshot {
        let mut accounts = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(newer) = accounts.get(&key).filter(|cached| cached.is_fresh(now)) {
            return newer.clone();
        }
        Self::insert(&mut accounts, key, snapshot.clone());
        snapshot
    }

    fn insert(accounts: &mut HashMap<String, Snapshot>, key: String, snapshot: Snapshot) {
        if accounts.len() >= MAX_ACCOUNTS && !accounts.contains_key(&key) {
            let oldest = accounts
                .iter()
                .min_by_key(|(_, snapshot)| snapshot.observed_at)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                accounts.remove(&oldest);
            }
        }
        accounts.insert(key, snapshot);
    }

    async fn admit_with<F, Q>(
        &self,
        key: String,
        config: &CodexQuotaProtection,
        now: i64,
        query: Q,
    ) -> Result<(), ProxyError>
    where
        F: Future<Output = Result<SubscriptionQuota, String>>,
        Q: FnOnce() -> F,
    {
        if !config.enabled || config.validate().is_err() {
            return Ok(());
        }
        let snapshot = match self.fresh(&key, now) {
            Some(snapshot) => snapshot,
            None => {
                // Unknown usage admits. Remember the failure too, so an outage
                // costs one lookup per TTL rather than one per request.
                let snapshot = match tokio::time::timeout(self.lookup_timeout, query()).await {
                    Ok(Ok(quota)) if quota.success => Snapshot::from_quota(&quota, now),
                    _ => Snapshot::empty(now),
                };
                self.store_lookup(key, snapshot, now)
            }
        };
        snapshot.check(config)
    }
}

/// Uses the JWT owner plus workspace when available so token refresh and
/// duplicate cards share one reading. The key only identifies the cache entry.
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

fn request_account_key(token: &str, request_headers: &HeaderMap) -> String {
    let account_id = request_headers
        .get("chatgpt-account-id")
        .and_then(|value| value.to_str().ok());
    account_key(token, account_id)
}

fn cache() -> &'static QuotaCache {
    static CACHE: OnceLock<QuotaCache> = OnceLock::new();
    CACHE.get_or_init(QuotaCache::default)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Rejects the request when the account's latest usage reaches a threshold.
pub(crate) async fn admit(token: &str, request_headers: &HeaderMap) -> Result<(), ProxyError> {
    let config = crate::settings::get_settings().codex_quota_protection;
    let account_id = request_headers
        .get("chatgpt-account-id")
        .and_then(|value| value.to_str().ok());
    cache()
        .admit_with(account_key(token, account_id), &config, now_unix(), || {
            query_codex_usage(token, account_id, "codex", "ChatGPT login needs refreshing")
        })
        .await
}

/// Records the usage headers of an official response (success or error).
pub(crate) fn observe_response(token: &str, request_headers: &HeaderMap, response: &HeaderMap) {
    if !crate::settings::get_settings()
        .codex_quota_protection
        .enabled
    {
        return;
    }
    if let Some(snapshot) = Snapshot::from_headers(response, now_unix()) {
        cache().store(request_account_key(token, request_headers), snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::{CredentialStatus, QuotaTier};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const NOW: i64 = 1_793_617_110;

    fn enabled() -> CodexQuotaProtection {
        CodexQuotaProtection {
            enabled: true,
            ..Default::default()
        }
    }

    fn quota(five_hour: Option<f64>, weekly: Option<f64>) -> SubscriptionQuota {
        let mut quota = SubscriptionQuota::not_found("codex");
        quota.success = true;
        quota.credential_status = CredentialStatus::Valid;
        quota.tiers = [(TIER_FIVE_HOUR, five_hour), (TIER_SEVEN_DAY, weekly)]
            .into_iter()
            .filter_map(|(name, used)| {
                used.map(|utilization| QuotaTier {
                    name: name.to_string(),
                    utilization,
                    resets_at: Some("2026-11-05T09:28:09Z".to_string()),
                    used_value_usd: None,
                    max_value_usd: None,
                })
            })
            .collect();
        quota
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        headers
    }

    fn window(used_percent: f64, resets_at: Option<i64>) -> Option<Window> {
        Some(Window {
            used_percent,
            resets_at,
        })
    }

    #[test]
    fn defaults_are_on_and_saved_thresholds_validate() {
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
    }

    #[test]
    fn reads_usage_headers_captured_from_a_pro_account() {
        // Captured 2026-11-02 from chatgpt.com/backend-api/codex/responses.
        let captured = headers(&[
            ("x-codex-active-limit", "premium"),
            ("x-codex-plan-type", "pro"),
            ("x-codex-primary-used-percent", "22"),
            ("x-codex-secondary-used-percent", "0"),
            ("x-codex-primary-window-minutes", "10080"),
            ("x-codex-secondary-window-minutes", "0"),
            ("x-codex-primary-reset-after-seconds", "160180"),
            ("x-codex-secondary-reset-after-seconds", "0"),
            ("x-codex-primary-reset-at", "1793777289"),
            ("x-codex-secondary-reset-at", ""),
        ]);
        assert_eq!(
            Snapshot::from_headers(&captured, NOW),
            Some(Snapshot {
                five_hour: None,
                weekly: window(22.0, Some(1_793_777_289)),
                observed_at: NOW,
            })
        );
    }

    #[test]
    fn maps_both_windows_and_prefers_the_active_limit_prefix() {
        let both = headers(&[
            ("x-codex-primary-used-percent", "96.5"),
            ("x-codex-primary-window-minutes", "300"),
            ("x-codex-primary-reset-after-seconds", "600"),
            ("x-codex-secondary-used-percent", "40"),
            ("x-codex-secondary-window-minutes", "10080"),
        ]);
        let snapshot = Snapshot::from_headers(&both, NOW).unwrap();
        assert_eq!(snapshot.five_hour, window(96.5, Some(NOW + 600)));
        assert_eq!(snapshot.weekly, window(40.0, None));

        let named = headers(&[
            ("x-codex-active-limit", "premium"),
            ("x-premium-primary-used-percent", "97"),
            ("x-premium-primary-window-minutes", "300"),
            ("x-codex-primary-used-percent", "10"),
            ("x-codex-primary-window-minutes", "300"),
        ]);
        assert_eq!(
            Snapshot::from_headers(&named, NOW).unwrap().five_hour,
            window(97.0, None)
        );
    }

    #[test]
    fn responses_without_usage_headers_keep_the_cached_reading() {
        assert_eq!(Snapshot::from_headers(&HeaderMap::new(), NOW), None);
        // A monthly-only account still reports usage, just no protected window.
        let monthly = headers(&[
            ("x-codex-primary-used-percent", "3"),
            ("x-codex-primary-window-minutes", "43200"),
        ]);
        assert_eq!(
            Snapshot::from_headers(&monthly, NOW),
            Some(Snapshot::empty(NOW))
        );
        let garbage = headers(&[
            ("x-codex-primary-used-percent", "NaN"),
            ("x-codex-primary-window-minutes", "300"),
        ]);
        assert_eq!(
            Snapshot::from_headers(&garbage, NOW),
            Some(Snapshot::empty(NOW))
        );
    }

    #[test]
    fn either_window_blocks_at_the_exact_threshold() {
        let config = enabled();
        let snapshot = |five_hour, weekly| Snapshot {
            five_hour: window(five_hour, None),
            weekly: window(weekly, None),
            observed_at: NOW,
        };
        assert!(snapshot(94.99, 97.99).check(&config).is_ok());
        assert!(snapshot(95.0, 10.0).check(&config).is_err());
        assert!(snapshot(0.0, 98.0).check(&config).is_err());
        let custom = CodexQuotaProtection {
            five_hour_threshold: 90,
            ..enabled()
        };
        assert!(snapshot(91.0, 0.0).check(&custom).is_err());
        assert!(Snapshot::empty(NOW).check(&config).is_ok());
    }

    #[test]
    fn snapshots_expire_after_ttl_or_a_window_reset() {
        let snapshot = Snapshot {
            five_hour: window(99.0, Some(NOW + 60)),
            weekly: None,
            observed_at: NOW,
        };
        assert!(snapshot.is_fresh(NOW + 59));
        assert!(!snapshot.is_fresh(NOW + 60));
        assert!(!Snapshot::empty(NOW).is_fresh(NOW + SNAPSHOT_TTL_SECS));
    }

    #[tokio::test]
    async fn disabled_never_queries() {
        let cache = QuotaCache::default();
        let disabled = CodexQuotaProtection {
            enabled: false,
            ..CodexQuotaProtection::default()
        };
        assert!(cache
            .admit_with("a".into(), &disabled, NOW, || async {
                panic!("must not query")
            })
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn fresh_header_readings_decide_without_querying() {
        let cache = QuotaCache::default();
        let config = enabled();
        let no_query = || async { panic!("must not query") };
        cache.store(
            "a".into(),
            Snapshot {
                five_hour: window(50.0, None),
                weekly: None,
                observed_at: NOW,
            },
        );
        assert!(cache
            .admit_with("a".into(), &config, NOW + 10, no_query)
            .await
            .is_ok());
        cache.store(
            "a".into(),
            Snapshot {
                five_hour: window(95.0, None),
                weekly: None,
                observed_at: NOW,
            },
        );
        assert!(matches!(
            cache
                .admit_with("a".into(), &config, NOW + 10, no_query)
                .await,
            Err(ProxyError::CodexQuotaProtection(_))
        ));
        // Raising the threshold applies to the next request immediately.
        let raised = CodexQuotaProtection {
            five_hour_threshold: 99,
            ..enabled()
        };
        assert!(cache
            .admit_with("a".into(), &raised, NOW + 10, no_query)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn stale_readings_are_confirmed_once_then_cached() {
        let cache = QuotaCache::default();
        let config = enabled();
        let queries = AtomicUsize::new(0);
        cache.store(
            "a".into(),
            Snapshot {
                five_hour: window(99.0, None),
                weekly: None,
                observed_at: NOW - SNAPSHOT_TTL_SECS,
            },
        );
        for _ in 0..3 {
            assert!(cache
                .admit_with("a".into(), &config, NOW, || async {
                    queries.fetch_add(1, Ordering::SeqCst);
                    Ok(quota(Some(10.0), Some(10.0)))
                })
                .await
                .is_ok());
        }
        assert_eq!(queries.load(Ordering::SeqCst), 1);
        assert!(cache
            .admit_with("b".into(), &config, NOW, || async {
                Ok(quota(None, Some(98.0)))
            })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn unknown_usage_admits_and_backs_off() {
        let cache = QuotaCache::default();
        let config = enabled();
        let queries = AtomicUsize::new(0);
        let expired = SubscriptionQuota::error(
            "codex",
            CredentialStatus::Expired,
            "secret-token-in-upstream-message".to_string(),
        );
        let mut monthly = quota(None, None);
        monthly.tiers.push(QuotaTier {
            name: "30_day".to_string(),
            utilization: 99.0,
            resets_at: None,
            used_value_usd: None,
            max_value_usd: None,
        });
        let outcomes: Vec<Result<SubscriptionQuota, String>> = vec![
            Err("secret-token-in-network-error".to_string()),
            Ok(expired),
            Ok(monthly),
            Ok(quota(None, None)),
        ];
        for (index, outcome) in outcomes.into_iter().enumerate() {
            let key = format!("account-{index}");
            for _ in 0..2 {
                let outcome = outcome.clone();
                assert!(cache
                    .admit_with(key.clone(), &config, NOW, || async {
                        queries.fetch_add(1, Ordering::SeqCst);
                        outcome
                    })
                    .await
                    .is_ok());
            }
        }
        assert_eq!(queries.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn a_lookup_never_overwrites_a_reading_recorded_while_it_was_in_flight() {
        let cache = QuotaCache::default();
        let config = enabled();
        let blocked = Snapshot {
            five_hour: window(95.0, None),
            weekly: None,
            observed_at: NOW + 1,
        };
        // Another request's response headers land before this lookup fails.
        let result = cache
            .admit_with("a".into(), &config, NOW, || async {
                cache.store("a".into(), blocked.clone());
                Err("timeout".to_string())
            })
            .await;
        assert!(matches!(result, Err(ProxyError::CodexQuotaProtection(_))));
        assert!(cache
            .admit_with("a".into(), &config, NOW + 2, || async {
                panic!("must not query")
            })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn slow_lookups_admit_after_the_timeout() {
        let cache = QuotaCache {
            lookup_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        assert!(cache
            .admit_with("a".into(), &enabled(), NOW, || async {
                std::future::pending::<Result<SubscriptionQuota, String>>().await
            })
            .await
            .is_ok());
    }

    #[test]
    fn renewed_bearers_share_owner_entry_but_workspace_and_owner_are_isolated() {
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

    #[test]
    fn cache_evicts_the_oldest_account_when_full() {
        let cache = QuotaCache::default();
        for index in 0..MAX_ACCOUNTS {
            cache.store(format!("{index}"), Snapshot::empty(NOW + index as i64));
        }
        cache.store("new".into(), Snapshot::empty(NOW + 1_000));
        let accounts = cache.accounts.lock().unwrap();
        assert_eq!(accounts.len(), MAX_ACCOUNTS);
        assert!(!accounts.contains_key("0"));
        assert!(accounts.contains_key("new"));
    }
}
