//! Copilot Auto Session Manager
//!
//! Provides process-level session caching, single-flight resolution,
//! bounded LRU/TTL lifecycle management, and account isolation for Copilot Auto.

use super::error::ProxyError;
use super::providers::copilot_auto::{
    parse_expires_at_field, CopilotAutoClient, CopilotAutoResponse, ParsedSessionToken,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Maximum cached Auto sessions (LRU capacity)
pub const MAX_CACHED_SESSIONS: usize = 256;

/// Idle TTL before an unused session is evicted (30 minutes)
pub const SESSION_IDLE_TTL: Duration = Duration::from_secs(30 * 60);

fn resolution_timeout_error(timeout: Duration) -> ProxyError {
    ProxyError::Timeout(format!(
        "Copilot Auto resolution timed out after {timeout:?}"
    ))
}

/// Cache key uniquely identifying a Copilot Auto conversational session
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AutoSessionKey {
    pub app_namespace: String,
    pub provider_id: String,
    pub account_id: String,
    pub api_base: String,
    pub credential_generation: u64,
    pub client_session_id: String,
}

/// Cloned snapshot of an active resolution held by executing requests
#[derive(Clone)]
pub struct AutoSessionSnapshot {
    pub selected_model: String,
    pub session_token: String,
    expires_at: Instant,
}

impl std::fmt::Debug for AutoSessionSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoSessionSnapshot")
            .field("selected_model", &self.selected_model)
            .field("session_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Internal session record stored in the manager cache
#[derive(Debug, Clone)]
struct AutoSessionRecord {
    snapshot: AutoSessionSnapshot,
    last_accessed: Instant,
}

/// RAII guard to safely remove a gate from single_flight map upon completion or cancellation
struct SingleFlightGuard {
    locks: Arc<std::sync::Mutex<HashMap<AutoSessionKey, Arc<tokio::sync::Mutex<()>>>>>,
    key: AutoSessionKey,
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl Drop for SingleFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut locks) = self.locks.lock() {
            if Arc::strong_count(&self.gate) <= 2 {
                if let Some(existing) = locks.get(&self.key) {
                    if Arc::ptr_eq(existing, &self.gate) {
                        locks.remove(&self.key);
                    }
                }
            }
        }
    }
}

/// Process-level manager for Copilot Auto sessions
pub struct AutoSessionManager {
    sessions: RwLock<HashMap<AutoSessionKey, AutoSessionRecord>>,
    single_flight: Arc<std::sync::Mutex<HashMap<AutoSessionKey, Arc<tokio::sync::Mutex<()>>>>>,
}

impl Default for AutoSessionManager {
    fn default() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            single_flight: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

impl AutoSessionManager {
    /// Fast lookup: returns an active snapshot if present and valid
    pub async fn get(&self, key: &AutoSessionKey) -> Option<AutoSessionSnapshot> {
        let mut sessions = self.sessions.write().await;
        if let Some(record) = sessions.get_mut(key) {
            let now = Instant::now();
            if now.duration_since(record.last_accessed) > SESSION_IDLE_TTL {
                return None;
            }
            if record.snapshot.expires_at <= now {
                return None;
            }
            record.last_accessed = now;
            return Some(record.snapshot.clone());
        }
        None
    }

    /// Resolves or retrieves a cached resolution with single-flight concurrency
    /// bounded by the provided request timeout budget.
    pub async fn get_or_resolve(
        &self,
        key: &AutoSessionKey,
        copilot_token: &str,
        prompt: &str,
        has_image: bool,
        timeout: Duration,
    ) -> Result<AutoSessionSnapshot, ProxyError> {
        // Fast path: cached & valid
        if let Some(snapshot) = self.get(key).await {
            return Ok(snapshot);
        }

        // Single-flight coordination per unique AutoSessionKey
        let guard = {
            let mut locks = self
                .single_flight
                .lock()
                .map_err(|e| ProxyError::Internal(format!("single_flight lock poisoned: {e}")))?;
            let gate = locks
                .entry(key.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone();
            SingleFlightGuard {
                locks: Arc::clone(&self.single_flight),
                key: key.clone(),
                gate,
            }
        };

        // Enforce timeout budget across both the single-flight lock wait and the /auto network call
        tokio::time::timeout(timeout, async {
            let _gate_lock = guard.gate.lock().await;

            // Double-check cache after acquiring lock
            if let Some(snapshot) = self.get(key).await {
                return Ok(snapshot);
            }

            let snapshot = self
                .resolve_from_auto(key, copilot_token, prompt, has_image, timeout)
                .await?;

            // Store into cache and apply LRU / TTL eviction
            self.insert(key.clone(), snapshot.clone()).await;

            Ok(snapshot)
        })
        .await
        .map_err(|_| resolution_timeout_error(timeout))?
    }

    /// 会话 ID 不是客户端给的（本地每次随机生成）：只决议这一次，不进缓存。
    /// 这样的 ID 不会再出现，缓存了也命中不了，只会把真实会话挤出 LRU。
    pub async fn resolve_uncached(
        &self,
        key: &AutoSessionKey,
        copilot_token: &str,
        prompt: &str,
        has_image: bool,
        timeout: Duration,
    ) -> Result<AutoSessionSnapshot, ProxyError> {
        tokio::time::timeout(
            timeout,
            self.resolve_from_auto(key, copilot_token, prompt, has_image, timeout),
        )
        .await
        .map_err(|_| resolution_timeout_error(timeout))?
    }

    /// Network call to POST /auto, turned into a session snapshot.
    async fn resolve_from_auto(
        &self,
        key: &AutoSessionKey,
        copilot_token: &str,
        prompt: &str,
        has_image: bool,
        timeout: Duration,
    ) -> Result<AutoSessionSnapshot, ProxyError> {
        log::info!(
            "[Copilot/Auto] Resolving /auto for session='{}' app='{}'",
            key.client_session_id,
            key.app_namespace
        );

        let response = CopilotAutoClient::resolve(
            &crate::proxy::http_client::get_for_url(&key.api_base),
            &key.api_base,
            copilot_token,
            prompt,
            has_image,
            timeout,
        )
        .await?;

        Self::snapshot_from_response(response)
    }

    /// /auto 响应 → 会话快照。期限取顶层 `expires_at` 与 session token 可解码 `exp`
    /// 中较早者；两者都不可证（缺失、类型错误、已过期）时报解析错误，不虚构默认期限。
    /// token 的 exp 只是本地期限提示，不代表签名验证。
    fn snapshot_from_response(
        response: CopilotAutoResponse,
    ) -> Result<AutoSessionSnapshot, ProxyError> {
        let now = Instant::now();

        let top_level_secs =
            parse_expires_at_field(response.expires_at.as_ref()).map_err(|reason| {
                ProxyError::TransformError(format!(
                    "Copilot /auto 响应顶层 expires_at 无效: {reason}"
                ))
            })?;
        let token_secs = ParsedSessionToken::parse(&response.session_token)
            .and_then(|p| p.exp)
            .and_then(|exp| i64::try_from(exp).ok());
        let expires_at = top_level_secs
            .into_iter()
            .chain(token_secs)
            .min()
            .ok_or_else(|| ProxyError::TransformError("Copilot /auto 响应缺少可证期限".into()))?;
        let system_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_at = now
            .checked_add(Duration::from_secs(
                (expires_at.max(0) as u64).saturating_sub(system_now),
            ))
            .ok_or_else(|| {
                ProxyError::TransformError("Copilot /auto 响应的授权期限超出范围".into())
            })?;

        if expires_at <= now {
            return Err(ProxyError::TransformError(
                "Copilot /auto 响应的授权期限已过期，拒绝使用该决议".to_string(),
            ));
        }

        Ok(AutoSessionSnapshot {
            selected_model: response.selected_model.model_id().to_string(),
            session_token: response.session_token,
            expires_at,
        })
    }

    pub async fn insert(&self, key: AutoSessionKey, snapshot: AutoSessionSnapshot) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();

        // Evict expired / idle entries
        sessions.retain(|_, rec| {
            now.duration_since(rec.last_accessed) <= SESSION_IDLE_TTL
                && rec.snapshot.expires_at > now
        });

        // Evict oldest by last_accessed if capacity exceeded
        while sessions.len() >= MAX_CACHED_SESSIONS {
            if let Some(oldest_key) = sessions
                .iter()
                .min_by_key(|(_, rec)| rec.last_accessed)
                .map(|(k, _)| k.clone())
            {
                sessions.remove(&oldest_key);
            } else {
                break;
            }
        }

        sessions.insert(
            key,
            AutoSessionRecord {
                snapshot,
                last_accessed: now,
            },
        );
    }

    /// HTTP 2xx 不构成可证的用户轮语义边界；这里只作为 LRU 活跃度刷新，
    /// 不再驱动任何重决议决策。
    pub async fn touch(&self, key: &AutoSessionKey) {
        let mut sessions = self.sessions.write().await;
        if let Some(record) = sessions.get_mut(key) {
            record.last_accessed = Instant::now();
        }
    }

    /// Invalidate a specific session only if the currently cached session token matches
    /// the token that experienced the error. Prevents concurrent requests from deleting
    /// a newly resolved session token (Finding 1).
    pub async fn invalidate_if_token_matches(
        &self,
        key: &AutoSessionKey,
        expected_session_token: &str,
    ) {
        let mut sessions = self.sessions.write().await;
        if let Some(record) = sessions.get(key) {
            if record.snapshot.session_token == expected_session_token {
                sessions.remove(key);
                log::info!(
                    "[Copilot/Auto] Invalidated session '{}' matching token version",
                    key.client_session_id
                );
            } else {
                log::info!(
                    "[Copilot/Auto] Kept newer session '{}' (token version diverged from 401 error token)",
                    key.client_session_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_key(session_id: &str, generation: u64) -> AutoSessionKey {
        AutoSessionKey {
            app_namespace: "codex".to_string(),
            provider_id: "prov-1".to_string(),
            account_id: "acc-1".to_string(),
            api_base: "https://api.individual.githubcopilot.com".to_string(),
            credential_generation: generation,
            client_session_id: session_id.to_string(),
        }
    }

    fn dummy_snapshot(model: &str) -> AutoSessionSnapshot {
        AutoSessionSnapshot {
            selected_model: model.to_string(),
            session_token: "mock-session-token".to_string(),
            expires_at: Instant::now() + Duration::from_secs(3600),
        }
    }

    #[tokio::test]
    async fn test_session_manager_insert_and_get() {
        let manager = AutoSessionManager::default();
        let key = dummy_key("session-1", 1);
        let snapshot = dummy_snapshot("gpt-5.6-luna");

        manager.insert(key.clone(), snapshot.clone()).await;

        let retrieved = manager.get(&key).await.unwrap();
        assert_eq!(retrieved.selected_model, "gpt-5.6-luna");
        assert_eq!(retrieved.session_token, "mock-session-token");
    }

    #[tokio::test]
    async fn test_session_manager_account_isolation() {
        let manager = AutoSessionManager::default();
        let mut key1 = dummy_key("session-1", 1);
        key1.account_id = "user_a".to_string();

        let mut key2 = dummy_key("session-1", 1);
        key2.account_id = "user_b".to_string();

        manager
            .insert(key1.clone(), dummy_snapshot("model-a"))
            .await;
        manager
            .insert(key2.clone(), dummy_snapshot("model-b"))
            .await;

        assert_eq!(manager.get(&key1).await.unwrap().selected_model, "model-a");
        assert_eq!(manager.get(&key2).await.unwrap().selected_model, "model-b");

        manager
            .invalidate_if_token_matches(&key1, "mock-session-token")
            .await;
        assert!(manager.get(&key1).await.is_none());
        assert!(manager.get(&key2).await.is_some());
    }

    #[tokio::test]
    async fn test_session_manager_generation_mismatch() {
        let manager = AutoSessionManager::default();
        let key_v1 = dummy_key("session-1", 1);
        let key_v2 = dummy_key("session-1", 2);

        manager
            .insert(key_v1.clone(), dummy_snapshot("model-v1"))
            .await;

        assert!(manager.get(&key_v1).await.is_some());
        assert!(manager.get(&key_v2).await.is_none());
    }

    #[tokio::test]
    async fn test_session_manager_expired_token() {
        let manager = AutoSessionManager::default();
        let key = dummy_key("session-exp", 1);
        let mut snapshot = dummy_snapshot("model-1");
        snapshot.expires_at = Instant::now() - Duration::from_secs(1); // Expired

        manager.insert(key.clone(), snapshot).await;
        assert!(manager.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_session_manager_lru_capacity_eviction() {
        let manager = AutoSessionManager::default();

        // Fill up to MAX_CACHED_SESSIONS (256)
        for i in 0..MAX_CACHED_SESSIONS {
            let key = dummy_key(&format!("session-{i}"), 1);
            manager
                .insert(key, dummy_snapshot(&format!("model-{i}")))
                .await;
        }

        assert_eq!(manager.sessions.read().await.len(), MAX_CACHED_SESSIONS);

        // Access session-0 to make it recently used
        let key0 = dummy_key("session-0", 1);
        let _ = manager.get(&key0).await;

        // Insert session-256 (257th entry)
        let key_new = dummy_key("session-256", 1);
        manager
            .insert(key_new.clone(), dummy_snapshot("model-256"))
            .await;

        // Total should stay capped at 256
        assert_eq!(manager.sessions.read().await.len(), MAX_CACHED_SESSIONS);
        // session-0 was accessed, so session-1 (oldest accessed) should have been evicted
        let key1 = dummy_key("session-1", 1);
        assert!(manager.get(&key1).await.is_none());
        assert!(manager.get(&key0).await.is_some());
        assert!(manager.get(&key_new).await.is_some());
    }

    #[tokio::test]
    async fn test_past_expiration_token_rejected_immediately() {
        // JWT with exp in 2020 (UNIX: 1577836800)
        let token = "eyJhbGciOiJFUzI1NiJ9.eyJleHAiOjE1Nzc4MzY4MDAsImF1dG9fc2Vzc2lvbl9pZCI6IjEyMyIsImF1dG9fdGllciI6ImJhbGFuY2UiLCJleGVjdXRpb25fbW9kZSI6ImZ1c2lvbiJ9.sig";
        let auto_resp = CopilotAutoResponse {
            session_token: token.to_string(),
            selected_model: crate::proxy::providers::copilot_auto::SelectedModelPayload::String(
                "gpt-5.6-luna".to_string(),
            ),
            expires_at: None,
        };

        let result = AutoSessionManager::snapshot_from_response(auto_resp);
        assert!(
            matches!(result, Err(ProxyError::TransformError(_))),
            "an already-expired resolution must be rejected, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_snapshot_expiry_takes_earlier_of_top_level_and_token_exp() {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let token_exp = (now + 7200) as i64;
        let top_level = (now + 3600) as i64;
        let payload = serde_json::json!({ "exp": token_exp });
        let token = format!(
            "eyJhbGciOiJFUzI1NiJ9.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload).unwrap().as_bytes())
        );
        let auto_resp = CopilotAutoResponse {
            session_token: token,
            selected_model: crate::proxy::providers::copilot_auto::SelectedModelPayload::String(
                "gpt-5.6-luna".to_string(),
            ),
            expires_at: Some(serde_json::json!(top_level)),
        };

        let snapshot = AutoSessionManager::snapshot_from_response(auto_resp).expect("valid expiry");
        let remaining = snapshot
            .expires_at
            .saturating_duration_since(Instant::now());
        assert!(
            remaining > Duration::from_secs(3500) && remaining <= Duration::from_secs(3600),
            "the earlier (top-level) expiry must win, remaining {remaining:?}"
        );
    }

    #[tokio::test]
    async fn test_snapshot_without_provable_expiry_is_rejected() {
        // Neither top-level expires_at nor a decodable token exp: the previous
        // implementation fabricated a 24h grant; that fallback must stay gone.
        let auto_resp = CopilotAutoResponse {
            session_token: "not-a-jwt".to_string(),
            selected_model: crate::proxy::providers::copilot_auto::SelectedModelPayload::String(
                "gpt-5.6-luna".to_string(),
            ),
            expires_at: None,
        };

        let result = AutoSessionManager::snapshot_from_response(auto_resp);
        assert!(
            matches!(result, Err(ProxyError::TransformError(_))),
            "unprovable expiry must fail resolution instead of defaulting to 24h"
        );
    }

    #[tokio::test]
    async fn test_snapshot_with_unparseable_top_level_expiry_is_rejected() {
        let auto_resp = CopilotAutoResponse {
            session_token: "not-a-jwt".to_string(),
            selected_model: crate::proxy::providers::copilot_auto::SelectedModelPayload::String(
                "gpt-5.6-luna".to_string(),
            ),
            expires_at: Some(serde_json::json!({"deadline": "soon"})),
        };

        let result = AutoSessionManager::snapshot_from_response(auto_resp);
        assert!(
            matches!(result, Err(ProxyError::TransformError(_))),
            "a malformed expires_at must fail resolution, not fall back"
        );
    }

    #[tokio::test]
    async fn test_invalidate_if_token_matches_preserves_newer_token() {
        let manager = AutoSessionManager::default();
        let key = dummy_key("session-concurrent", 1);

        let mut old_snapshot = dummy_snapshot("gpt-5.6-luna");
        old_snapshot.session_token = "token-v1".to_string();

        let mut new_snapshot = dummy_snapshot("gpt-5.6-luna");
        new_snapshot.session_token = "token-v2".to_string();

        // 1. Initial state has token-v1
        manager.insert(key.clone(), old_snapshot).await;

        // 2. Request A re-resolves and stores token-v2
        manager.insert(key.clone(), new_snapshot).await;

        // Insert a dummy lock gate into single_flight to test lock preservation
        {
            let mut locks = manager.single_flight.lock().unwrap();
            locks.insert(key.clone(), Arc::new(tokio::sync::Mutex::new(())));
        }

        // 3. Stale Request B returns 401 referencing token-v1
        manager.invalidate_if_token_matches(&key, "token-v1").await;

        // 4. token-v2 MUST be preserved, AND single_flight gate must NOT be evicted!
        let current = manager
            .get(&key)
            .await
            .expect("new session must not be invalidated by stale token-v1 401");
        assert_eq!(current.session_token, "token-v2");
        assert_eq!(
            manager.single_flight.lock().unwrap().len(),
            1,
            "in-flight gate must be preserved when token diverges"
        );

        // 5. When matching token-v2 is invalidated, session is removed from cache,
        // while active in-flight gates are preserved (handled by SingleFlightGuard upon completion/drop)
        manager.invalidate_if_token_matches(&key, "token-v2").await;
        assert!(manager.get(&key).await.is_none());
        assert_eq!(manager.single_flight.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_single_flight_guard_drop_cleans_up_map() {
        let manager = AutoSessionManager::default();
        let key = dummy_key("session-cancelled", 1);

        {
            let gate = {
                let mut locks = manager.single_flight.lock().unwrap();
                locks
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                    .clone()
            };
            let _guard = SingleFlightGuard {
                locks: Arc::clone(&manager.single_flight),
                key: key.clone(),
                gate,
            };
            assert_eq!(manager.single_flight.lock().unwrap().len(), 1);
            // _guard dropped at end of scope (simulating future cancellation / abort)
        }

        // Must be automatically cleaned up!
        assert_eq!(manager.single_flight.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_get_or_resolve_failure_cleans_up_single_flight_map() {
        let manager = AutoSessionManager::default();
        let mut key = dummy_key("session-error", 1);
        // Use unreachable local port to ensure instant failure without live network calls
        key.api_base = "http://127.0.0.1:1".to_string();

        let res = manager
            .get_or_resolve(
                &key,
                "invalid-token",
                "hello",
                false,
                Duration::from_millis(500),
            )
            .await;

        assert!(res.is_err());
        assert_eq!(manager.single_flight.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_resolve_uncached_leaves_the_cache_empty() {
        let app = axum::Router::new().route(
            "/auto",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "session_token": "token-1",
                    "selected_model": "gpt-5.6-luna",
                    "expires_at": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        + 3600
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind /auto mock");
        let base = format!("http://{}", listener.local_addr().expect("mock address"));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve /auto mock");
        });

        let manager = AutoSessionManager::default();
        let mut key = dummy_key("generated-session-id", 1);
        key.api_base = base;
        let snapshot = manager
            .resolve_uncached(&key, "token", "hello", false, Duration::from_secs(5))
            .await
            .expect("uncached resolution");
        server.abort();

        assert_eq!(snapshot.selected_model, "gpt-5.6-luna");
        assert!(manager.sessions.read().await.is_empty());
        assert!(manager.single_flight.lock().unwrap().is_empty());
    }
}
