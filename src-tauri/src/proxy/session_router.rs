//! Session 路由模块
//!
//! 基于 X-Claude-Code-Session-Id 实现 session 级 provider 路由。
//! 每个 Claude Code 终端都有唯一的 session ID，通过此模块实现：
//! - 新 session → 自动分配 provider（Round-Robin / Least-Loaded）
//! - 已有 session → 使用已分配的 provider（会话一致性）
//! - Provider 熔断 → 故障转移到下一个可用 provider

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::provider_router::{provider_supports_failover, ProviderRouter};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 分配策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    /// 轮转分配，依次选择 provider
    RoundRobin,
    /// 最少负载，选活跃 session 最少的 provider
    LeastLoaded,
}

impl RoutingStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoutingStrategy::RoundRobin => "round_robin",
            RoutingStrategy::LeastLoaded => "least_loaded",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "least_loaded" => RoutingStrategy::LeastLoaded,
            _ => RoutingStrategy::RoundRobin,
        }
    }
}

/// Session 路由配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRoutingConfig {
    pub enabled: bool,
    pub strategy: RoutingStrategy,
    pub session_ttl_seconds: u64,
    pub max_sessions_per_provider: u32,
}

impl Default for SessionRoutingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: RoutingStrategy::RoundRobin,
            session_ttl_seconds: 3600,
            max_sessions_per_provider: 0,
        }
    }
}

/// Session 路由状态（用于 UI 展示）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRouteInfo {
    pub session_id: String,
    pub session_name: String,
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    pub assigned_at: i64,
    pub last_used_at: i64,
    pub request_count: u64,
    pub failover_count: u64,
}

/// Provider 负载信息（用于 UI 展示，含名称）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLoadInfo {
    pub provider_id: String,
    pub provider_name: String,
    pub session_count: u64,
}

/// Session 路由器
pub struct SessionRouter {
    db: Arc<Database>,
    /// 共享的 ProviderRouter（用于熔断状态检查，避免 session 故障转移选中熔断中的 provider）
    provider_router: Arc<ProviderRouter>,
    /// 轮转指针 (app_type → index)
    round_robin_index: Arc<RwLock<HashMap<String, usize>>>,
}

impl SessionRouter {
    pub fn new(db: Arc<Database>, provider_router: Arc<ProviderRouter>) -> Self {
        Self {
            db,
            provider_router,
            round_robin_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 获取或分配 session 的 provider
    ///
    /// 返回 `(Provider, 是否为新分配)`
    pub async fn get_or_assign_provider(
        &self,
        session_id: &str,
        app_type: &str,
    ) -> Result<(Provider, bool), AppError> {
        let config = self
            .db
            .get_session_routing_config(app_type)?
            .unwrap_or_default();

        // 1. 检查是否已存在映射
        if let Some(route) = self.db.get_session_route(session_id, app_type)? {
            if let Some(provider) = self.db.get_provider_by_id(&route.provider_id, app_type)? {
                // 2. TTL 过期：删除映射，走重新分配。
                //    否则已关闭终端的过期路由会永久复用旧 provider，
                //    并持续污染 least-loaded 的负载计数。
                let ttl_ms = config.session_ttl_seconds as i64 * 1000;
                let now = chrono::Utc::now().timestamp_millis();
                if ttl_ms > 0 && now - route.last_used_at > ttl_ms {
                    log::info!(
                        "[SessionRouter] session 路由已过期 (TTL={}s)，重新分配: session={} app={}",
                        config.session_ttl_seconds,
                        &session_id[..8.min(session_id.len())],
                        app_type,
                    );
                    self.db.delete_session_route(session_id, app_type)?;
                } else {
                    // 未过期：更新最后使用时间 + 请求计数，保持会话一致性
                    self.db.touch_session_route(session_id, app_type)?;
                    return Ok((provider, false));
                }
            } else {
                // provider 已被删除，清理映射
                self.db.delete_session_route(session_id, app_type)?;
            }
        }

        // 3. 分配新 provider
        let provider = match config.strategy {
            RoutingStrategy::RoundRobin => self.assign_round_robin(app_type, &config).await?,
            RoutingStrategy::LeastLoaded => self.assign_least_loaded(app_type, &config).await?,
        };

        // 4. 保存映射
        let now = chrono::Utc::now().timestamp_millis();
        self.db.insert_session_route(session_id, app_type, &provider.id, now)?;

        log::info!(
            "[SessionRouter] 新 session 分配: session={} app={} provider={} ({})",
            &session_id[..8.min(session_id.len())],
            app_type,
            provider.id,
            provider.name,
        );

        Ok((provider, true))
    }

    /// 构建以 pinned 为首的完整故障转移链（交给 forwarder 做逐家熔断检查与重试）
    ///
    /// 这里不按熔断状态预过滤：让 forwarder 在迭代时用 `allow_request` 逐家判断，
    /// 既正确管理 HalfOpen 探测名额，又避免「选中 → 熔断」之间的竞态。
    /// 只排除重复项与不支持故障转移的 provider（如 codex official，防止跨账号复用 token）。
    pub fn build_failover_chain(&self, app_type: &str, pinned: &Provider) -> Vec<Provider> {
        let mut chain = vec![pinned.clone()];

        // pinned 本身不允许重试（codex official 等）时保持单一 route
        if !provider_supports_failover(app_type, pinned) {
            return chain;
        }

        if let Ok(providers) = self.db.get_failover_providers(app_type) {
            for p in providers {
                if p.id == pinned.id {
                    continue;
                }
                if !provider_supports_failover(app_type, &p) {
                    continue;
                }
                chain.push(p);
            }
        }

        chain
    }

    /// Session 故障转移 — 当前 provider 熔断时，选下一个「可用」的 provider 并更新路由
    ///
    /// 候选会先用熔断器状态过滤（非占用式 `is_available`，不消耗 HalfOpen 探测名额），
    /// 跳过熔断中的 provider，避免把请求发给已知不可用的上游。
    pub async fn failover_to_available(
        &self,
        session_id: &str,
        app_type: &str,
        failed_provider_id: &str,
    ) -> Result<Provider, AppError> {
        let providers = self.db.get_failover_providers(app_type)?;
        if providers.is_empty() {
            return Err(AppError::NoProvidersConfigured);
        }

        // 不允许重试的 provider（如 codex official 账号）不做 session 故障转移，
        // 防止跨账号复用上游 token。
        if let Ok(Some(failed)) = self.db.get_provider_by_id(failed_provider_id, app_type) {
            if !provider_supports_failover(app_type, &failed) {
                return Err(AppError::AllProvidersCircuitOpen);
            }
        }

        let failed_idx = providers
            .iter()
            .position(|p| p.id == failed_provider_id)
            .unwrap_or(0);

        // 从 failed 之后环形扫描，跳过熔断中的 provider，选第一个可用的
        for offset in 1..=providers.len() {
            let idx = (failed_idx + offset) % providers.len();
            let candidate = &providers[idx];
            if candidate.id == failed_provider_id {
                continue;
            }
            if !provider_supports_failover(app_type, candidate) {
                continue;
            }
            if self.provider_router.is_available(&candidate.id, app_type).await {
                let now = chrono::Utc::now().timestamp_millis();
                self.db
                    .update_session_route_provider(session_id, app_type, &candidate.id, now)?;
                self.db.increment_session_failover(session_id, app_type)?;

                log::info!(
                    "[SessionRouter] 故障转移: session={} app={} {} → {}",
                    &session_id[..8.min(session_id.len())],
                    app_type,
                    failed_provider_id,
                    candidate.id,
                );

                return Ok(candidate.clone());
            }
        }

        Err(AppError::AllProvidersCircuitOpen)
    }

    /// Round-Robin 分配
    async fn assign_round_robin(
        &self,
        app_type: &str,
        config: &SessionRoutingConfig,
    ) -> Result<Provider, AppError> {
        let providers = self.db.get_failover_providers(app_type)?;
        if providers.is_empty() {
            return Err(AppError::NoProvidersConfigured);
        }

        // 优先在未达到 max_sessions_per_provider 上限的 provider 里轮转
        let candidates = self.within_capacity(&providers, app_type, config)?;
        let pool: Vec<&Provider> = if candidates.is_empty() {
            // 全部达到上限：退化为全部轮转，避免新 session 无法分配
            providers.iter().collect()
        } else {
            candidates
        };

        let mut index = self.round_robin_index.write().await;
        let idx = index.entry(app_type.to_string()).or_insert(0);
        let provider = (*pool[*idx % pool.len()]).clone();
        *idx += 1;
        Ok(provider)
    }

    /// Least-Loaded 分配
    async fn assign_least_loaded(
        &self,
        app_type: &str,
        config: &SessionRoutingConfig,
    ) -> Result<Provider, AppError> {
        let providers = self.db.get_failover_providers(app_type)?;
        if providers.is_empty() {
            return Err(AppError::NoProvidersConfigured);
        }

        if providers.len() == 1 {
            return Ok(providers[0].clone());
        }

        let session_counts = self.db.count_sessions_per_provider(app_type)?;

        // 优先在未达到上限的 provider 里选负载最小的；
        // 全部达到上限时退化为全局最少负载，保证新 session 始终能分配。
        let candidates = self.within_capacity(&providers, app_type, config)?;
        let pool: Vec<&Provider> = if candidates.is_empty() {
            providers.iter().collect()
        } else {
            candidates
        };

        let best = pool
            .into_iter()
            .min_by_key(|p| session_counts.get(&p.id).copied().unwrap_or(0))
            .cloned()
            .ok_or(AppError::NoProvidersConfigured)?;

        Ok(best)
    }

    /// 过滤掉已达到 `max_sessions_per_provider` 上限的 provider。
    /// 上限为 0 表示不限制。
    fn within_capacity<'a>(
        &self,
        providers: &'a [Provider],
        app_type: &str,
        config: &SessionRoutingConfig,
    ) -> Result<Vec<&'a Provider>, AppError> {
        if config.max_sessions_per_provider == 0 {
            return Ok(providers.iter().collect());
        }
        let counts = self.db.count_sessions_per_provider(app_type)?;
        Ok(providers
            .iter()
            .filter(|p| {
                counts.get(&p.id).copied().unwrap_or(0) < config.max_sessions_per_provider as u64
            })
            .collect())
    }

    /// 清理过期 session
    ///
    /// 供后台定时任务调用（当前由 UI 的「清理过期」触发对应的 DAO 方法，
    /// 此方法保留为未来调度任务接入）。
    #[allow(dead_code)]
    pub async fn cleanup_expired(&self, app_type: &str, ttl_seconds: u64) -> Result<u64, AppError> {
        let cutoff = chrono::Utc::now().timestamp_millis() - (ttl_seconds as i64 * 1000);
        let count = self.db.delete_expired_session_routes(app_type, cutoff)?;
        if count > 0 {
            log::info!("[SessionRouter] 清理了 {} 个过期 session 路由", count);
        }
        Ok(count)
    }
}
#[cfg(test)]
mod serde_tests {
    use super::*;

    #[test]
    fn frontend_round_trip() {
        // 模拟前端 get 返回后，再原样发回 update 的 JSON
        let frontend_json = r#"{"enabled":true,"strategy":"round_robin","sessionTtlSeconds":3600,"maxSessionsPerProvider":0}"#;
        let cfg: SessionRoutingConfig = serde_json::from_str(frontend_json).expect("deserialize");
        assert!(cfg.enabled);
        assert_eq!(cfg.strategy, RoutingStrategy::RoundRobin);
        assert_eq!(cfg.session_ttl_seconds, 3600);
        // 序列化回去
        let out = serde_json::to_string(&cfg).unwrap();
        println!("round-trip json: {}", out);
        assert!(out.contains("sessionTtlSeconds"));
        assert!(out.contains("round_robin"));
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::database::Database;
    use crate::proxy::ProviderRouter;

    fn provider(id: &str) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), serde_json::json!({}), None)
    }

    fn setup_router() -> (Arc<Database>, Arc<SessionRouter>) {
        let db = Arc::new(Database::memory().unwrap());
        let provider_router = Arc::new(ProviderRouter::new(db.clone()));
        let router = Arc::new(SessionRouter::new(db.clone(), provider_router));
        (db, router)
    }

    fn seed_claude_providers(db: &Arc<Database>, ids: &[&str]) {
        for id in ids {
            db.save_provider("claude", &provider(id)).unwrap();
            db.add_to_failover_queue("claude", id).unwrap();
        }
    }

    fn enable_config(
        db: &Arc<Database>,
        strategy: RoutingStrategy,
        ttl: u64,
        max_per_provider: u32,
    ) {
        db.update_session_routing_config(
            "claude",
            &SessionRoutingConfig {
                enabled: true,
                strategy,
                session_ttl_seconds: ttl,
                max_sessions_per_provider: max_per_provider,
            },
        )
        .unwrap();
    }

    /// 回归测试：TTL 过期后应重新分配 provider（Codex review #4）
    #[tokio::test]
    async fn ttl_expiry_reassigns_provider() {
        let (db, router) = setup_router();
        seed_claude_providers(&db, &["a", "b"]);
        enable_config(&db, RoutingStrategy::RoundRobin, 3600, 0);

        let now = chrono::Utc::now().timestamp_millis();
        db.insert_session_route("s1", "claude", "a", now).unwrap();
        // 把 last_used_at 改到 2 小时前，模拟过期
        db.update_session_route_provider("s1", "claude", "a", now - 2 * 3600 * 1000)
            .unwrap();

        let (_, is_new) = router.get_or_assign_provider("s1", "claude").await.unwrap();
        assert!(is_new, "TTL 过期后应重新分配 provider");
    }

    /// 回归测试：TTL 未过期时应复用原 provider，保持会话一致性
    #[tokio::test]
    async fn ttl_not_expired_reuses_provider() {
        let (db, router) = setup_router();
        seed_claude_providers(&db, &["a", "b"]);
        enable_config(&db, RoutingStrategy::RoundRobin, 3600, 0);

        let now = chrono::Utc::now().timestamp_millis();
        db.insert_session_route("s1", "claude", "a", now).unwrap();

        let (p, is_new) = router.get_or_assign_provider("s1", "claude").await.unwrap();
        assert!(!is_new, "TTL 未过期应复用原 provider");
        assert_eq!(p.id, "a");
    }

    /// TTL = 0 表示永不过期
    #[tokio::test]
    async fn ttl_zero_never_expires() {
        let (db, router) = setup_router();
        seed_claude_providers(&db, &["a", "b"]);
        enable_config(&db, RoutingStrategy::RoundRobin, 0, 0);

        let now = chrono::Utc::now().timestamp_millis();
        db.insert_session_route("s1", "claude", "a", now).unwrap();
        db.update_session_route_provider("s1", "claude", "a", now - 999_999 * 1000)
            .unwrap();

        let (p, is_new) = router.get_or_assign_provider("s1", "claude").await.unwrap();
        assert!(!is_new, "TTL=0 表示不过期，应复用");
        assert_eq!(p.id, "a");
    }

    /// 故障转移链：pinned 必须排第一，且不出现重复
    #[tokio::test]
    async fn failover_chain_pins_first_and_deduplicates() {
        let (db, router) = setup_router();
        seed_claude_providers(&db, &["a", "b", "c"]);
        enable_config(&db, RoutingStrategy::RoundRobin, 3600, 0);

        let pinned = db.get_provider_by_id("a", "claude").unwrap().unwrap();
        let chain = router.build_failover_chain("claude", &pinned);

        assert_eq!(chain.len(), 3, "应为 pinned + 队列内其余 provider");
        assert_eq!(chain[0].id, "a");

        let mut ids: Vec<&str> = chain.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 3, "链路中不应有重复 provider");
    }

    /// max_sessions_per_provider 生效：least-loaded 应跳过已达上限的 provider
    #[tokio::test]
    async fn least_loaded_respects_max_sessions_per_provider() {
        let (db, router) = setup_router();
        seed_claude_providers(&db, &["a", "b"]);
        enable_config(&db, RoutingStrategy::LeastLoaded, 3600, 1);

        // a 已有一个 session，达到上限 1
        let now = chrono::Utc::now().timestamp_millis();
        db.insert_session_route("s1", "claude", "a", now).unwrap();

        let (p, _) = router.get_or_assign_provider("s2", "claude").await.unwrap();
        assert_eq!(p.id, "b", "a 已达上限，新 session 应分到 b");
    }
}
