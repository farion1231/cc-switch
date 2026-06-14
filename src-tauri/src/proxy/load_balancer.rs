//! Provider 级负载调度。
//!
//! 目标：
//! - 同一会话优先固定到同一 Provider，减少上游缓存 miss。
//! - 新会话分配时避开已满载 Provider；已绑定会话继续走原 Provider。
//! - 仅使用运行态计数，不写数据库。

use crate::provider::{Provider, ProviderLoadLimits};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};

const RPM_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
pub struct ProviderLoadBalancer {
    inner: Arc<RwLock<LoadState>>,
}

#[derive(Default)]
struct LoadState {
    semaphores: HashMap<String, Arc<Semaphore>>,
    rpm_windows: HashMap<String, VecDeque<Instant>>,
    sticky_sessions: HashMap<String, StickyProvider>,
}

struct StickyProvider {
    app_type: String,
    session_id: String,
    provider_id: String,
    last_seen: Instant,
    session_slot: Option<OwnedSemaphorePermit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveSessionTarget {
    pub app_type: String,
    pub provider_id: String,
    pub session_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapacityDecision {
    Available,
    RpmFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLoadRejectReason {
    ConcurrencyFull,
    RpmFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderLoadRejected {
    pub reason: ProviderLoadRejectReason,
}

pub struct ProviderLoadPermit {
    provider_key: String,
    semaphore_permit: Option<OwnedSemaphorePermit>,
    balancer: ProviderLoadBalancer,
}

impl Drop for ProviderLoadPermit {
    fn drop(&mut self) {
        let _ = self.semaphore_permit.take();
        let balancer = self.balancer.clone();
        let provider_key = self.provider_key.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                balancer.compact_provider_state(&provider_key).await;
            });
        }
    }
}

impl ProviderLoadBalancer {
    pub async fn order_providers_for_session(
        &self,
        app_type: &str,
        session_id: &str,
        providers: &[Provider],
    ) -> Vec<Provider> {
        let Some(sticky_provider_id) = self.sticky_provider_id(app_type, session_id).await else {
            return providers.to_vec();
        };

        let Some(index) = providers
            .iter()
            .position(|provider| provider.id == sticky_provider_id)
        else {
            return providers.to_vec();
        };

        let mut ordered = Vec::with_capacity(providers.len());
        ordered.push(providers[index].clone());
        ordered.extend(
            providers
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, provider)| provider.clone()),
        );
        ordered
    }

    pub async fn acquire(
        &self,
        app_type: &str,
        session_id: &str,
        provider: &Provider,
    ) -> Result<ProviderLoadPermit, ProviderLoadRejected> {
        let should_track_session = !session_id.trim().is_empty();
        if should_track_session
            && self
                .is_session_bound_to_provider(app_type, session_id, &provider.id)
                .await
        {
            return Ok(ProviderLoadPermit {
                provider_key: provider_key(app_type, &provider.id),
                semaphore_permit: None,
                balancer: self.clone(),
            });
        }

        let limits = provider_load_limits(provider);
        if !limits.has_limits() {
            return Ok(ProviderLoadPermit {
                provider_key: provider_key(app_type, &provider.id),
                semaphore_permit: None,
                balancer: self.clone(),
            });
        }

        let key = provider_key(app_type, &provider.id);
        let concurrency_limit = limits.max_concurrent_limit();
        let semaphore = match (should_track_session, concurrency_limit) {
            (true, Some(limit)) => {
                let mut inner = self.inner.write().await;
                let semaphore_key = semaphore_key(&key, limit);
                Some(
                    inner
                        .semaphores
                        .entry(semaphore_key)
                        .or_insert_with(|| Arc::new(Semaphore::new(limit)))
                        .clone(),
                )
            }
            _ => None,
        };

        let semaphore_permit = match semaphore {
            Some(semaphore) => match semaphore.try_acquire_owned() {
                Ok(permit) => Some(permit),
                Err(_) => {
                    log::debug!(
                        "[{app_type}] Provider {} 达到并发上限，尝试下一家",
                        provider.id
                    );
                    return Err(ProviderLoadRejected {
                        reason: ProviderLoadRejectReason::ConcurrencyFull,
                    });
                }
            },
            None => None,
        };

        let mut inner = self.inner.write().await;
        match check_and_reserve_rpm(&mut inner, &key, limits.rpm_limit()) {
            CapacityDecision::Available => Ok(ProviderLoadPermit {
                provider_key: key,
                semaphore_permit,
                balancer: self.clone(),
            }),
            CapacityDecision::RpmFull => {
                log::debug!(
                    "[{app_type}] Provider {} 达到 RPM 上限，尝试下一家",
                    provider.id
                );
                Err(ProviderLoadRejected {
                    reason: ProviderLoadRejectReason::RpmFull,
                })
            }
        }
    }

    pub async fn bind_success(
        &self,
        app_type: &str,
        session_id: &str,
        provider_id: &str,
        load_permit: &mut ProviderLoadPermit,
    ) {
        if session_id.trim().is_empty() {
            return;
        }

        let session_slot = load_permit.semaphore_permit.take();
        let mut inner = self.inner.write().await;
        bind_session_locked(&mut inner, app_type, session_id, provider_id, session_slot);
    }

    pub async fn is_session_bound_to_provider(
        &self,
        app_type: &str,
        session_id: &str,
        provider_id: &str,
    ) -> bool {
        self.sticky_provider_id(app_type, session_id)
            .await
            .is_some_and(|sticky_provider_id| sticky_provider_id == provider_id)
    }

    pub async fn active_session_targets(&self) -> Vec<ActiveSessionTarget> {
        let mut inner = self.inner.write().await;
        prune_sticky_sessions(&mut inner);

        let mut grouped: HashMap<(String, String), Vec<String>> = HashMap::new();
        for sticky in inner.sticky_sessions.values() {
            grouped
                .entry((sticky.app_type.clone(), sticky.provider_id.clone()))
                .or_default()
                .push(sticky.session_id.clone());
        }

        let mut targets: Vec<_> = grouped
            .into_iter()
            .map(|((app_type, provider_id), mut session_ids)| {
                session_ids.sort();
                ActiveSessionTarget {
                    app_type,
                    provider_id,
                    session_ids,
                }
            })
            .collect();
        targets.sort_by(|a, b| {
            a.app_type
                .cmp(&b.app_type)
                .then_with(|| a.provider_id.cmp(&b.provider_id))
        });
        targets
    }

    async fn sticky_provider_id(&self, app_type: &str, session_id: &str) -> Option<String> {
        let key = sticky_key(app_type, session_id);
        let mut inner = self.inner.write().await;
        prune_sticky_sessions(&mut inner);
        inner.sticky_sessions.get_mut(&key).map(|sticky| {
            sticky.last_seen = Instant::now();
            sticky.provider_id.clone()
        })
    }

    async fn compact_provider_state(&self, provider_key: &str) {
        let mut inner = self.inner.write().await;
        prune_rpm_window(&mut inner, provider_key);
        prune_sticky_sessions(&mut inner);
    }
}

fn provider_load_limits(provider: &Provider) -> ProviderLoadLimits {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.load_limits.clone())
        .unwrap_or_default()
}

fn check_and_reserve_rpm(
    inner: &mut LoadState,
    provider_key: &str,
    rpm_limit: Option<usize>,
) -> CapacityDecision {
    let Some(limit) = rpm_limit else {
        return CapacityDecision::Available;
    };

    let now = Instant::now();
    let window = inner
        .rpm_windows
        .entry(provider_key.to_string())
        .or_default();
    while window
        .front()
        .is_some_and(|timestamp| now.duration_since(*timestamp) >= RPM_WINDOW)
    {
        window.pop_front();
    }

    if window.len() >= limit {
        return CapacityDecision::RpmFull;
    }

    window.push_back(now);
    CapacityDecision::Available
}

fn prune_rpm_window(inner: &mut LoadState, provider_key: &str) {
    let Some(window) = inner.rpm_windows.get_mut(provider_key) else {
        return;
    };

    let now = Instant::now();
    while window
        .front()
        .is_some_and(|timestamp| now.duration_since(*timestamp) >= RPM_WINDOW)
    {
        window.pop_front();
    }

    if window.is_empty() {
        inner.rpm_windows.remove(provider_key);
    }
}

fn bind_session_locked(
    inner: &mut LoadState,
    app_type: &str,
    session_id: &str,
    provider_id: &str,
    session_slot: Option<OwnedSemaphorePermit>,
) {
    prune_sticky_sessions(inner);
    let key = sticky_key(app_type, session_id);
    let now = Instant::now();

    if let Some(sticky) = inner.sticky_sessions.get_mut(&key) {
        if sticky.provider_id == provider_id && session_slot.is_none() {
            sticky.last_seen = now;
            let _ = sticky.session_slot.is_some();
            return;
        }
    }

    inner.sticky_sessions.insert(
        key,
        StickyProvider {
            app_type: app_type.to_string(),
            session_id: session_id.to_string(),
            provider_id: provider_id.to_string(),
            last_seen: now,
            session_slot,
        },
    );
}

fn prune_sticky_sessions(inner: &mut LoadState) {
    let now = Instant::now();
    inner
        .sticky_sessions
        .retain(|_, sticky| now.duration_since(sticky.last_seen) < Duration::from_secs(60 * 60));
}

fn sticky_key(app_type: &str, session_id: &str) -> String {
    format!("{app_type}:{session_id}")
}

fn provider_key(app_type: &str, provider_id: &str) -> String {
    format!("{app_type}:{provider_id}")
}

fn semaphore_key(provider_key: &str, limit: usize) -> String {
    format!("{provider_key}:concurrency:{limit}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serde_json::json;

    fn provider(id: &str, max_concurrent: Option<u32>, rpm: Option<u32>) -> Provider {
        Provider {
            id: id.to_string(),
            name: id.to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                load_limits: Some(ProviderLoadLimits {
                    max_concurrent,
                    rpm,
                }),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: true,
        }
    }

    #[tokio::test]
    async fn acquire_respects_concurrency_limit() {
        let balancer = ProviderLoadBalancer::default();
        let p = provider("p1", Some(1), None);

        let mut first = balancer.acquire("claude", "s1", &p).await.unwrap();
        balancer
            .bind_success("claude", "s1", &p.id, &mut first)
            .await;

        assert_eq!(
            balancer
                .acquire("claude", "s2", &p)
                .await
                .err()
                .unwrap()
                .reason,
            ProviderLoadRejectReason::ConcurrencyFull
        );
        assert!(balancer.acquire("claude", "s1", &p).await.is_ok());

        drop(first);
        tokio::task::yield_now().await;
        assert_eq!(
            balancer
                .acquire("claude", "s2", &p)
                .await
                .err()
                .unwrap()
                .reason,
            ProviderLoadRejectReason::ConcurrencyFull
        );
    }

    #[tokio::test]
    async fn acquire_respects_rpm_limit() {
        let balancer = ProviderLoadBalancer::default();
        let p = provider("p1", None, Some(1));

        assert!(balancer.acquire("claude", "s1", &p).await.is_ok());
        assert_eq!(
            balancer
                .acquire("claude", "s2", &p)
                .await
                .err()
                .unwrap()
                .reason,
            ProviderLoadRejectReason::RpmFull
        );
    }

    #[tokio::test]
    async fn sticky_provider_is_first_when_available() {
        let balancer = ProviderLoadBalancer::default();
        let p1 = provider("p1", None, None);
        let p2 = provider("p2", None, None);

        let mut permit = balancer.acquire("claude", "session", &p2).await.unwrap();
        balancer
            .bind_success("claude", "session", &p2.id, &mut permit)
            .await;
        let ordered = balancer
            .order_providers_for_session("claude", "session", &[p1.clone(), p2.clone()])
            .await;

        assert_eq!(ordered[0].id, "p2");
        assert_eq!(ordered[1].id, "p1");
        assert!(
            balancer
                .is_session_bound_to_provider("claude", "session", &p2.id)
                .await
        );
        assert!(
            !balancer
                .is_session_bound_to_provider("claude", "session", &p1.id)
                .await
        );
    }

    #[tokio::test]
    async fn active_session_targets_group_by_app_and_provider() {
        let balancer = ProviderLoadBalancer::default();
        let p1 = provider("p1", None, None);
        let p2 = provider("p2", None, None);

        let mut permit = balancer.acquire("claude", "s2", &p2).await.unwrap();
        balancer
            .bind_success("claude", "s2", &p2.id, &mut permit)
            .await;
        let mut permit = balancer.acquire("claude", "s1", &p2).await.unwrap();
        balancer
            .bind_success("claude", "s1", &p2.id, &mut permit)
            .await;
        let mut permit = balancer.acquire("codex", "thread-1", &p1).await.unwrap();
        balancer
            .bind_success("codex", "thread-1", &p1.id, &mut permit)
            .await;

        assert_eq!(
            balancer.active_session_targets().await,
            vec![
                ActiveSessionTarget {
                    app_type: "claude".to_string(),
                    provider_id: "p2".to_string(),
                    session_ids: vec!["s1".to_string(), "s2".to_string()],
                },
                ActiveSessionTarget {
                    app_type: "codex".to_string(),
                    provider_id: "p1".to_string(),
                    session_ids: vec!["thread-1".to_string()],
                },
            ]
        );
    }
}
