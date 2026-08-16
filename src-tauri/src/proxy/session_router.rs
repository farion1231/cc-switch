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
    /// 轮转指针 (app_type → index)
    round_robin_index: Arc<RwLock<HashMap<String, usize>>>,
}

impl SessionRouter {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
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
        // 1. 检查是否已存在映射
        if let Some(route) = self.db.get_session_route(session_id, app_type)? {
            if let Some(provider) = self.db.get_provider_by_id(&route.provider_id, app_type)? {
                // 更新最后使用时间 + 请求计数
                self.db.touch_session_route(session_id, app_type)?;
                return Ok((provider, false));
            }
            // provider 已被删除，清理映射
            self.db.delete_session_route(session_id, app_type)?;
        }

        // 2. 分配新 provider
        let config = self
            .db
            .get_session_routing_config(app_type)?
            .unwrap_or_default();

        let provider = match config.strategy {
            RoutingStrategy::RoundRobin => self.assign_round_robin(app_type).await?,
            RoutingStrategy::LeastLoaded => self.assign_least_loaded(app_type).await?,
        };

        // 3. 保存映射
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

    /// Session 故障转移 — 当前 provider 不可用时切换到下一个
    pub async fn failover(
        &self,
        session_id: &str,
        app_type: &str,
        failed_provider_id: &str,
    ) -> Result<Provider, AppError> {
        let providers = self.db.get_failover_providers(app_type)?;
        if providers.is_empty() {
            return Err(AppError::NoProvidersConfigured);
        }

        // 找当前 provider 在队列中的位置，选下一个
        let failed_idx = providers
            .iter()
            .position(|p| p.id == failed_provider_id)
            .unwrap_or(0);

        for i in 1..providers.len() {
            let next = &providers[(failed_idx + i) % providers.len()];
            if next.id != failed_provider_id {
                let now = chrono::Utc::now().timestamp_millis();
                self.db
                    .update_session_route_provider(session_id, app_type, &next.id, now)?;
                self.db.increment_session_failover(session_id, app_type)?;

                log::info!(
                    "[SessionRouter] 故障转移: session={} app={} {} → {}",
                    &session_id[..8.min(session_id.len())],
                    app_type,
                    failed_provider_id,
                    next.id,
                );

                return Ok(next.clone());
            }
        }

        Err(AppError::AllProvidersCircuitOpen)
    }

    /// Round-Robin 分配
    async fn assign_round_robin(&self, app_type: &str) -> Result<Provider, AppError> {
        let providers = self.db.get_failover_providers(app_type)?;
        if providers.is_empty() {
            return Err(AppError::NoProvidersConfigured);
        }

        let mut index = self.round_robin_index.write().await;
        let idx = index.entry(app_type.to_string()).or_insert(0);
        let provider = providers[*idx % providers.len()].clone();
        *idx += 1;
        Ok(provider)
    }

    /// Least-Loaded 分配
    async fn assign_least_loaded(&self, app_type: &str) -> Result<Provider, AppError> {
        let providers = self.db.get_failover_providers(app_type)?;
        if providers.is_empty() {
            return Err(AppError::NoProvidersConfigured);
        }

        if providers.len() == 1 {
            return Ok(providers[0].clone());
        }

        let session_counts = self.db.count_sessions_per_provider(app_type)?;

        let best = providers
            .iter()
            .min_by_key(|p| session_counts.get(&p.id).copied().unwrap_or(0))
            .ok_or(AppError::NoProvidersConfigured)?;

        Ok(best.clone())
    }

    /// 清理过期 session
    pub async fn cleanup_expired(&self, ttl_seconds: u64) -> Result<u64, AppError> {
        let cutoff = chrono::Utc::now().timestamp_millis() - (ttl_seconds as i64 * 1000);
        let count = self.db.delete_expired_session_routes(cutoff)?;
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
