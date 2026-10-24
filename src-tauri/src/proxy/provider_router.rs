//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{AllowResult, CircuitBreaker, CircuitBreakerConfig};
use crate::proxy::load_balance::SessionLoadBalancer;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Codex Official requests carry the selected account's native Authorization
/// header. Reusing that request against another account card would cross the
/// account boundary, so these cards must never participate in provider retry.
pub(crate) fn provider_supports_failover(app_type: &str, provider: &Provider) -> bool {
    app_type != AppType::Codex.as_str()
        || !crate::proxy::providers::is_codex_official_provider(provider)
}

/// 供应商路由器
pub struct ProviderRouter {
    /// 数据库连接
    db: Arc<Database>,
    /// 熔断器管理器 - key 格式: "app_type:provider_id"
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<CircuitBreaker>>>>,
    /// 会话粘性负载均衡状态（粘性表/游标/在途计数/冷却）
    load_balancer: Arc<SessionLoadBalancer>,
}

impl ProviderRouter {
    /// 创建新的供应商路由器
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            load_balancer: Arc::new(SessionLoadBalancer::new()),
        }
    }

    /// 供 forwarder 记录重映射/冷却/在途（内部挂点用）
    #[cfg_attr(not(test), allow(dead_code))] // 测试与后续状态 UI 使用；lib 内经包装方法调用
    pub fn load_balancer(&self) -> &SessionLoadBalancer {
        &self.load_balancer
    }

    /// 选择可用的供应商（支持故障转移）
    ///
    /// 返回按优先级排序的可用供应商列表：
    /// - 故障转移关闭时：仅返回当前供应商
    /// - 故障转移开启时：仅使用故障转移队列，按队列顺序依次尝试（P1 → P2 → ...）
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        // 代理模式下路由到代理路由那家，和直连指针无关。Hermes 按 profile 独立路由的
        // 伪键（hermes:<profile>）没有 AppType，路由目标从它自己的 live-state 读。
        let current_provider = match current_provider_of(&self.db, app_type) {
            Ok(provider) => provider,
            Err(error) => {
                log::warn!("[{app_type}] 读当前供应商失败（{error}），按未配置处理");
                None
            }
        };
        self.select_providers_with_current(app_type, current_provider)
            .await
    }

    /// 同 [`Self::select_providers`]，正在用的那家由调用方给出：处理请求时上下文已经读过
    /// 一次（要读 `live-state.json` 和这一行），不用每个请求再读一遍，两处用的也一定是
    /// 同一家。
    pub async fn select_providers_with_current(
        &self,
        app_type: &str,
        current_provider: Option<Provider>,
    ) -> Result<Vec<Provider>, AppError> {
        // 检查该应用的自动故障转移开关是否开启（从 proxy_config 表读取）
        let auto_failover_enabled = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(config) => config.auto_failover_enabled,
            Err(e) => {
                log::error!("[{app_type}] 读取 proxy_config 失败: {e}，默认禁用故障转移");
                false
            }
        };

        if auto_failover_enabled
            && current_provider
                .as_ref()
                .is_some_and(|provider| !provider_supports_failover(app_type, provider))
        {
            // A selected Codex Official account is an explicit account choice.
            // Keep it as a single route even if an old failover setting remains
            // enabled; retrying would reuse its inbound token for another card.
            return Ok(vec![current_provider.expect("checked above")]);
        }

        if !auto_failover_enabled {
            // 故障转移关闭：仅使用当前供应商，跳过熔断器检查
            if let Some(current) = current_provider {
                return Ok(vec![current]);
            }
            log::warn!("[{app_type}] [FO-005] 未配置供应商");
            return Err(AppError::NoProvidersConfigured);
        }

        // 故障转移开启：仅按队列顺序依次尝试（P1 → P2 → ...）。
        // Hermes 例外：接管指针（proxy_route）优先于池序——见 prioritize_hermes_route。
        let (result, total_providers, circuit_open_count) =
            self.circuit_filtered_queue(app_type).await?;
        let result = self
            .prioritize_hermes_route(app_type, current_provider.clone(), result)
            .await;
        if result.is_empty() {
            if total_providers > 0 && circuit_open_count == total_providers {
                log::warn!("[{app_type}] [FO-004] 所有供应商均已熔断");
                return Err(AppError::AllProvidersCircuitOpen);
            }
            // 队列为空但当前供应商可用：开关开着、队列还没配，是常见过渡状态——
            // 回落到单路由而不是 503（把用户的请求打死比少一层兜底更糟）。
            if let Some(current) = current_provider {
                log::warn!("[{app_type}] 故障转移队列是空的，按当前供应商单路由处理");
                return Ok(vec![current]);
            }
            log::warn!("[{app_type}] [FO-005] 未配置供应商");
            return Err(AppError::NoProvidersConfigured);
        }
        Ok(result)
    }

    /// Hermes 接管语义：路由指针（proxy_route，经 `current_provider_of` 读出）
    /// 优先于池序——代理模式下整个应用跟随 UI 选定的那家，池（队列/全量）
    /// 其余条目只作故障转移链。指针条目熔断打开时不置首（保持池序，让健康的
    /// 首位继续服务，指针恢复后自动回位）。
    ///
    /// 背景：接管把客户端模型名固定为指针条目的发布名（如 deepseek-v4-pro），
    /// 若队列首位是别家（如 SGLM），"仅按队列序"会把该模型名发到不认它的
    /// 上游——指针必须赢，队列只兜底。
    ///
    /// 非 Hermes 应用不受影响（"failover 开启仅按队列序"的既有语义保留）。
    async fn prioritize_hermes_route(
        &self,
        app_type: &str,
        current: Option<Provider>,
        pool: Vec<Provider>,
    ) -> Vec<Provider> {
        let is_hermes_key = app_type == AppType::Hermes.as_str()
            || crate::services::provider::hermes_profile::profile_of(app_type).is_some();
        let Some(route) = current else {
            return pool;
        };
        if !is_hermes_key || pool.is_empty() || pool[0].id == route.id {
            return pool;
        }
        let breaker = self
            .get_or_create_circuit_breaker(&format!("{app_type}:{}", route.id))
            .await;
        if !breaker.is_available().await {
            log::warn!(
                "[{app_type}] 接管路由 {} 已熔断，保持池序由队列首位服务",
                route.name
            );
            return pool;
        }
        log::debug!(
            "[{app_type}] 接管路由 {} 置首（池序降为故障转移链）",
            route.name
        );
        let mut result = vec![route.clone()];
        result.extend(pool.into_iter().filter(|p| p.id != route.id));
        result
    }

    /// 故障转移队列 -> 熔断过滤后的候选池（返回 (池, 总数, 熔断数)）。
    ///
    /// `select_providers` 与 `select_providers_for_session` 共用，保证
    /// 两者的池构造语义（队列顺序 + provider_supports_failover + 熔断过滤）一致。
    async fn circuit_filtered_queue(
        &self,
        app_type: &str,
    ) -> Result<(Vec<Provider>, usize, usize), AppError> {
        // Hermes 按 profile 独立路由的伪键：池是 Hermes 应用的全部供应商，顺序按
        // sort_index（独立配置不单独维护队列）；其余应用用各自的故障转移队列。
        let (pool_app, ordered_ids): (&str, Vec<String>) =
            match crate::services::provider::hermes_profile::profile_of(app_type) {
                Some(_) => (
                    AppType::Hermes.as_str(),
                    self.db
                        .get_all_providers(AppType::Hermes.as_str())?
                        .keys()
                        .cloned()
                        .collect(),
                ),
                None => (
                    app_type,
                    self.db
                        .get_failover_queue(app_type)?
                        .into_iter()
                        .map(|item| item.provider_id)
                        .collect(),
                ),
            };
        let all_providers = self.db.get_all_providers(pool_app)?;

        let mut result = Vec::new();
        let mut total_providers = 0usize;
        let mut circuit_open_count = 0usize;
        for provider_id in ordered_ids {
            let Some(provider) = all_providers.get(&provider_id).cloned() else {
                continue;
            };
            if !provider_supports_failover(app_type, &provider) {
                continue;
            }
            total_providers += 1;

            let circuit_key = format!("{app_type}:{}", provider.id);
            let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

            if breaker.is_available().await {
                result.push(provider);
            } else {
                circuit_open_count += 1;
            }
        }
        Ok((result, total_providers, circuit_open_count))
    }

    /// failover 成功后的会话重映射（forwarder wrapper 调用）
    pub(crate) fn remap_session(
        &self,
        app_type: &str,
        session_id: &str,
        client_provided: bool,
        new_provider_id: &str,
    ) {
        if !client_provided {
            return; // Generated 会话不写粘性表（设计决策 3）
        }
        self.load_balancer
            .remap_sticky(app_type, session_id, new_provider_id);
    }

    /// 为指定供应商获取在途计数句柄（forwarder 每 attempt 调用）
    pub(crate) fn acquire_for_provider(
        &self,
        _app_type: &str,
        provider: &Provider,
        body: &serde_json::Value,
    ) -> crate::proxy::load_balance::InFlightHandle {
        let request_model = body
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let outbound_model =
            crate::proxy::load_balance::resolve_outbound_model(provider, request_model);
        self.load_balancer
            .acquire_in_flight(&provider.id, &outbound_model)
    }

    /// forwarder 挂点：限流冷却记录（429/403）
    pub(crate) fn record_lb_rate_limit(
        &self,
        app_type: &str,
        provider_id: &str,
        retry_after_secs: Option<u64>,
    ) {
        self.load_balancer
            .record_rate_limit(app_type, provider_id, retry_after_secs);
    }

    /// forwarder 挂点：成功服务，清冷却 + 重置退避
    pub(crate) fn record_lb_success(&self, app_type: &str, provider_id: &str) {
        self.load_balancer.record_success(app_type, provider_id);
    }

    /// 会话感知选号：load_balance_mode = off 时与 select_providers 完全一致；
    /// session 模式下粘性命中优先，新会话按"冷却过滤 + 轮询游标 + 饱和跳过"选号并写粘性表。
    ///
    /// 返回 [选中账号] + 其余候选（按队列序），forwarder 顺序尝试语义不变，
    /// 请求内 failover 自动保留。
    ///
    /// 注意：Gemini 的模型名在选号时刻尚未从 URI 解析（`with_model_from_uri`
    /// 在 ctx 创建之后），模型级并发覆盖对 Gemini 不生效，账号默认上限仍生效。
    #[cfg_attr(not(test), allow(dead_code))] // 测试与后续状态 UI 使用；生产走 _with_current 变体
    pub async fn select_providers_for_session(
        &self,
        app_type: &str,
        session_id: &str,
        session_client_provided: bool,
        request_model: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let current_provider = match current_provider_of(&self.db, app_type) {
            Ok(provider) => provider,
            Err(error) => {
                log::warn!("[{app_type}] 读当前供应商失败（{error}），按未配置处理");
                None
            }
        };
        self.select_providers_for_session_with_current(
            app_type,
            current_provider,
            session_id,
            session_client_provided,
            request_model,
        )
        .await
    }

    /// 同 [`Self::select_providers_for_session`]，正在用的那家由调用方给出
    /// （与 [`Self::select_providers_with_current`] 同一理由：上下文已读过一次，
    /// 两处用同一家、也省一次读）。
    pub async fn select_providers_for_session_with_current(
        &self,
        app_type: &str,
        current_provider: Option<Provider>,
        session_id: &str,
        session_client_provided: bool,
        request_model: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let config = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(config) => config,
            Err(e) => {
                // 与 select_providers 的容错语义对齐：读不到配置按关闭处理
                log::error!("[{app_type}] 读取 proxy_config 失败: {e}，负载均衡不生效");
                return self
                    .select_providers_with_current(app_type, current_provider)
                    .await;
            }
        };
        if config.load_balance_mode != "session" || !config.auto_failover_enabled {
            // off 模式 / 故障转移关闭（池只有当前供应商，粘性无意义）
            return self
                .select_providers_with_current(app_type, current_provider)
                .await;
        }

        // 与 select_providers_with_current 同款守卫：Codex Official 是显式的账号
        // 选择，即使残留旧的故障转移设置也保持单路由——重试会把它的 inbound
        // token 用到别的卡上；session 模式同样不该把流量摊给队列里的第三方。
        if current_provider
            .as_ref()
            .is_some_and(|provider| !provider_supports_failover(app_type, provider))
        {
            return Ok(vec![current_provider.expect("checked above")]);
        }

        let (pool, total_providers, circuit_open_count) =
            self.circuit_filtered_queue(app_type).await?;
        // Hermes 接管指针优先（同 select_providers_with_current）；session 模式下
        // 粘性命中仍可跟随实际成功家，新会话游标从指针开始
        let pool = self
            .prioritize_hermes_route(app_type, current_provider.clone(), pool)
            .await;
        if pool.is_empty() {
            if total_providers > 0 && circuit_open_count == total_providers {
                return Err(AppError::AllProvidersCircuitOpen);
            }
            // 与 select_providers_with_current 同款回落：空队列 + 可用当前供应商
            if let Some(current) = current_provider {
                log::warn!("[{app_type}] 故障转移队列是空的，按当前供应商单路由处理");
                return Ok(vec![current]);
            }
            return Err(AppError::NoProvidersConfigured);
        }
        if pool.len() == 1 {
            // 单账号池：无需粘性，避免污染粘性表
            return Ok(pool);
        }

        // 1) 粘性命中且账号仍在池内 → 沿用（不查并发上限，设计决策 4）
        let mut stale_hit = false;
        if session_client_provided {
            if let Some(hit) = self.load_balancer.lookup_sticky(app_type, session_id) {
                if let Some(idx) = pool.iter().position(|p| p.id == hit) {
                    let mut result = vec![pool[idx].clone()];
                    result.extend(
                        pool.iter()
                            .enumerate()
                            .filter(|(i, _)| *i != idx)
                            .map(|(_, p)| p.clone()),
                    );
                    return Ok(result);
                }
                // 粘性账号被熔断/移出队列 → 按新会话重选（设计 5.1），映射需改写
                stale_hit = true;
            }
        }

        // 2) 新会话：冷却过滤 + 游标 + 饱和跳过 + 全饱和最少在途（设计 4.1.b）；
        //    Generated 会话按在途最少选号、不写粘性表（设计 4.1.d）
        let idx = if session_client_provided {
            let idx = self
                .load_balancer
                .select_new_session(app_type, &pool, request_model);
            if stale_hit {
                // 旧映射指向已出池账号：无条件改写（insert 的先到者优先会拒绝覆盖）
                self.load_balancer
                    .remap_sticky(app_type, session_id, &pool[idx].id);
            } else {
                // 并发抢建：后到者采纳先到者（设计 4.1.c）
                self.load_balancer
                    .insert_sticky(app_type, session_id, &pool[idx].id);
            }
            idx
        } else {
            self.load_balancer
                .select_least_in_flight(app_type, &pool, request_model)
        };
        let mut result = vec![pool[idx].clone()];
        result.extend(
            pool.iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, p)| p.clone()),
        );
        Ok(result)
    }

    /// 请求执行前获取熔断器“放行许可”
    ///
    /// - Closed：直接放行
    /// - Open：超时到达后切到 HalfOpen 并放行一次探测
    /// - HalfOpen：按限流规则放行探测
    ///
    /// 注意：调用方必须在请求结束后通过 `record_result()` 释放 HalfOpen 名额，
    /// 否则会导致该 Provider 长时间无法进入探测状态。
    pub async fn allow_provider_request(&self, provider_id: &str, app_type: &str) -> AllowResult {
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
        breaker.allow_request().await
    }

    /// 记录供应商请求结果
    pub async fn record_result(
        &self,
        provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
        success: bool,
        error_msg: Option<String>,
    ) -> Result<(), AppError> {
        // 1. 按应用独立获取熔断器配置
        let failure_threshold = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(app_config) => app_config.circuit_failure_threshold,
            Err(_) => 5, // 默认值
        };

        // 2. 更新熔断器状态
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

        if success {
            breaker.record_success(used_half_open_permit).await;
        } else {
            breaker.record_failure(used_half_open_permit).await;
        }

        // 3. 更新数据库健康状态（使用配置的阈值）
        self.db
            .update_provider_health_with_threshold(
                provider_id,
                app_type,
                success,
                error_msg.clone(),
                failure_threshold,
            )
            .await?;

        Ok(())
    }

    /// 重置熔断器（手动恢复）
    pub async fn reset_circuit_breaker(&self, circuit_key: &str) {
        let breakers = self.circuit_breakers.read().await;
        if let Some(breaker) = breakers.get(circuit_key) {
            breaker.reset().await;
        }
    }

    /// 重置指定供应商的熔断器
    pub async fn reset_provider_breaker(&self, provider_id: &str, app_type: &str) {
        let circuit_key = format!("{app_type}:{provider_id}");
        self.reset_circuit_breaker(&circuit_key).await;
    }

    /// 仅释放 HalfOpen permit，不影响健康统计（neutral 接口）
    ///
    /// 用于整流器等场景：请求结果不应计入 Provider 健康度，
    /// 但仍需释放占用的探测名额，避免 HalfOpen 状态卡死
    pub async fn release_permit_neutral(
        &self,
        provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
    ) {
        if !used_half_open_permit {
            return;
        }
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
        breaker.release_half_open_permit();
    }

    /// 更新所有熔断器的配置（热更新）
    pub async fn update_all_configs(&self, config: CircuitBreakerConfig) {
        let breakers = self.circuit_breakers.read().await;
        for breaker in breakers.values() {
            breaker.update_config(config.clone()).await;
        }
    }

    /// 更新指定应用已创建熔断器的配置（热更新）
    pub async fn update_app_configs(&self, app_type: &str, config: CircuitBreakerConfig) {
        let prefix = format!("{app_type}:");
        let breakers = self.circuit_breakers.read().await;
        for (key, breaker) in breakers.iter() {
            if key.starts_with(&prefix) {
                breaker.update_config(config.clone()).await;
            }
        }
    }

    /// 获取熔断器状态
    #[allow(dead_code)]
    pub async fn get_circuit_breaker_stats(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<crate::proxy::circuit_breaker::CircuitBreakerStats> {
        let circuit_key = format!("{app_type}:{provider_id}");
        let breakers = self.circuit_breakers.read().await;

        if let Some(breaker) = breakers.get(&circuit_key) {
            Some(breaker.get_stats().await)
        } else {
            None
        }
    }

    /// 获取或创建熔断器
    async fn get_or_create_circuit_breaker(&self, key: &str) -> Arc<CircuitBreaker> {
        // 先尝试读锁获取
        {
            let breakers = self.circuit_breakers.read().await;
            if let Some(breaker) = breakers.get(key) {
                return breaker.clone();
            }
        }

        // 如果不存在，获取写锁创建
        let mut breakers = self.circuit_breakers.write().await;

        // 双重检查，防止竞争条件
        if let Some(breaker) = breakers.get(key) {
            return breaker.clone();
        }

        // 从 key 中提取 app_type (格式: "app_type:provider_id")
        let app_type = key.split(':').next().unwrap_or("claude");

        // 按应用独立读取熔断器配置
        let config = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(app_config) => crate::proxy::circuit_breaker::CircuitBreakerConfig {
                failure_threshold: app_config.circuit_failure_threshold,
                success_threshold: app_config.circuit_success_threshold,
                timeout_seconds: app_config.circuit_timeout_seconds as u64,
                error_rate_threshold: app_config.circuit_error_rate_threshold,
                min_requests: app_config.circuit_min_requests,
            },
            Err(_) => crate::proxy::circuit_breaker::CircuitBreakerConfig::default(),
        };

        let breaker = Arc::new(CircuitBreaker::new(config));
        breakers.insert(key.to_string(), breaker.clone());

        breaker
    }
}

/// 当前供应商：真应用经 mode::current::provider_in_use；Hermes 伪键（hermes:<profile>）
/// 的路由目标从它自己的 live-state 读（不在代理模式时没有）。
pub(crate) fn current_provider_of(
    db: &Database,
    app_type: &str,
) -> Result<Option<Provider>, AppError> {
    match crate::services::provider::hermes_profile::profile_of(app_type) {
        Some(profile) => crate::services::provider::hermes_profile::route_provider(db, profile),
        None => Ok(AppType::from_str(app_type).ok().and_then(|app_enum| {
            crate::mode::current::provider_in_use(db, &app_enum)
                .ok()
                .flatten()
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    #[tokio::test]
    #[serial]
    async fn hermes_route_pointer_wins_over_queue_order() {
        // 场景回放（2026-10-24 线上 400）：接管指针 moa-deepseek（模型发布名
        // deepseek-v4-pro），队列 [SGLM, XGLM]，failover 开——"仅按队列序"
        // 会把 deepseek 模型名发到只认 glm-5.3 的智谱。指针必须赢。
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let route = Provider::with_id(
            "route".to_string(),
            "moa-deepseek".to_string(),
            json!({"base_url": "https://ds.example", "api_key": "sk"}),
            None,
        );
        let mut b = Provider::with_id("b".to_string(), "SGLM".to_string(), json!({}), None);
        b.sort_index = Some(0);
        let mut c = Provider::with_id("c".to_string(), "XGLM".to_string(), json!({}), None);
        c.sort_index = Some(1);
        for p in [&route, &b, &c] {
            db.save_provider("hermes", p).unwrap();
        }
        db.add_to_failover_queue("hermes", "b").unwrap();
        db.add_to_failover_queue("hermes", "c").unwrap();
        let mut config = db.get_proxy_config_for_app("hermes").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        // 接管：代理模式 + proxy_route 指向 route
        crate::mode::state::update(&crate::live::engine::DeviceStore::for_device(), |s| {
            use crate::mode::state::{Mode, ModeState};
            s.apps
                .entry("hermes".to_string())
                .or_default()
                .set_mode_state(ModeState {
                    mode: Some(Mode::Proxy),
                    attached: true,
                    proxy_route: Some("route".to_string()),
                    contract: None,
                });
        })
        .unwrap();

        let selected = ProviderRouter::new(db)
            .select_providers("hermes")
            .await
            .unwrap();
        let ids: Vec<&str> = selected.iter().map(|p| p.id.as_str()).collect();
        // 指针置首（池外插入），队列其余条目降为故障转移链
        assert_eq!(ids, vec!["route", "b", "c"]);
    }

    #[tokio::test]
    #[serial]
    async fn hermes_route_pointer_circuit_open_keeps_queue_order() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let route = Provider::with_id(
            "route".to_string(),
            "moa-deepseek".to_string(),
            json!({}),
            None,
        );
        let mut b = Provider::with_id("b".to_string(), "SGLM".to_string(), json!({}), None);
        b.sort_index = Some(0);
        for p in [&route, &b] {
            db.save_provider("hermes", p).unwrap();
        }
        db.add_to_failover_queue("hermes", "b").unwrap();
        let mut config = db.get_proxy_config_for_app("hermes").await.unwrap();
        config.auto_failover_enabled = true;
        config.circuit_failure_threshold = 1;
        db.update_proxy_config_for_app(config).await.unwrap();

        crate::mode::state::update(&crate::live::engine::DeviceStore::for_device(), |s| {
            use crate::mode::state::{Mode, ModeState};
            s.apps
                .entry("hermes".to_string())
                .or_default()
                .set_mode_state(ModeState {
                    mode: Some(Mode::Proxy),
                    attached: true,
                    proxy_route: Some("route".to_string()),
                    contract: None,
                });
        })
        .unwrap();

        // 指针条目熔断：保持队列序
        let router = ProviderRouter::new(db);
        router
            .record_result("route", "hermes", false, false, Some("fail".to_string()))
            .await
            .unwrap();
        let selected = router.select_providers("hermes").await.unwrap();
        let ids: Vec<&str> = selected.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[tokio::test]
    async fn failover_enabled_with_empty_queue_falls_back_to_current() {
        // 开关开着、队列没配是常见过渡状态：不应 503，回落当前供应商单路由。
        let db = Arc::new(Database::memory().expect("memory db"));
        let mut provider = crate::provider::Provider::with_id(
            "only".to_string(),
            "Only".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-test"
                }
            }),
            None,
        );
        provider.sort_index = Some(1);
        db.save_provider("claude", &provider).unwrap();
        db.set_current_provider("claude", "only").unwrap();
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true; // 队列仍为空
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let current = crate::mode::current::provider_in_use(&db, &AppType::Claude)
            .expect("read current")
            .expect("current exists");
        let selected = router
            .select_providers_with_current("claude", Some(current))
            .await
            .expect("falls back");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "only");
    }

    use crate::provider::{AuthBinding, AuthBindingSource, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    fn managed_codex_official(id: &str, account_id: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        provider.category = Some("official".to_string());
        provider.meta = Some(ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            auth_binding: Some(AuthBinding {
                source: AuthBindingSource::ManagedAccount,
                auth_provider: Some("codex_oauth".to_string()),
                account_id: Some(account_id.to_string()),
            }),
            ..Default::default()
        });
        provider
    }

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }

            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }

            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn remap_session_updates_sticky_mapping() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "session").await;
        let router = ProviderRouter::new(db);
        router.load_balancer().insert_sticky("claude", "s1", "a");
        // Generated 会话不写表
        router.remap_session("claude", "gen", false, "b");
        assert_eq!(router.load_balancer().lookup_sticky("claude", "gen"), None);
        // 真实会话重映射生效
        router.remap_session("claude", "s1", true, "b");
        assert_eq!(
            router.load_balancer().lookup_sticky("claude", "s1"),
            Some("b".to_string())
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_provider_router_creation() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = ProviderRouter::new(db);

        let breaker = router.get_or_create_circuit_breaker("claude:test").await;
        assert!(breaker.allow_request().await.allowed);
    }

    // ==================== 会话粘性负载均衡 ====================

    async fn seed_two_providers_with_mode(db: &Arc<Database>, mode: &str) {
        let mut a = Provider::with_id("a".to_string(), "A".to_string(), json!({}), None);
        a.sort_index = Some(1);
        let mut b = Provider::with_id("b".to_string(), "B".to_string(), json!({}), None);
        b.sort_index = Some(2);
        db.save_provider("claude", &a).unwrap();
        db.save_provider("claude", &b).unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        config.load_balance_mode = mode.to_string();
        db.update_proxy_config_for_app(config).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn session_mode_off_behaves_identically_to_select_providers() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "off").await;
        let router = ProviderRouter::new(db);
        let via_session = router
            .select_providers_for_session("claude", "s1", true, "unknown")
            .await
            .unwrap();
        let plain = router.select_providers("claude").await.unwrap();
        assert_eq!(
            via_session
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            plain.iter().map(|p| p.id.as_str()).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    #[serial]
    async fn session_mode_round_robins_new_sessions_and_writes_sticky() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "session").await;
        let router = ProviderRouter::new(db);

        let first = router
            .select_providers_for_session("claude", "s1", true, "unknown")
            .await
            .unwrap();
        // 返回 [选中] + 其余队列（forwarder 零改动，failover 语义保留）
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].id, "a");
        assert_eq!(first[1].id, "b");
        assert_eq!(
            router.load_balancer().lookup_sticky("claude", "s1"),
            Some("a".to_string())
        );

        let second = router
            .select_providers_for_session("claude", "s2", true, "unknown")
            .await
            .unwrap();
        assert_eq!(second[0].id, "b"); // 游标推进到 b
        assert_eq!(second[1].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn sticky_hit_returns_provider_first_without_cursor_advance() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "session").await;
        let router = ProviderRouter::new(db);
        router.load_balancer().insert_sticky("claude", "s1", "b");
        let result = router
            .select_providers_for_session("claude", "s1", true, "unknown")
            .await
            .unwrap();
        assert_eq!(result[0].id, "b");
        assert_eq!(result[1].id, "a");
        // 粘性命中不动游标：下一个新会话仍从 a 开始
        let next = router
            .select_providers_for_session("claude", "s2", true, "unknown")
            .await
            .unwrap();
        assert_eq!(next[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn generated_session_round_robins_without_sticky_write() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "session").await;
        let router = ProviderRouter::new(db);
        router
            .select_providers_for_session("claude", "gen", false, "unknown")
            .await
            .unwrap();
        assert_eq!(router.load_balancer().lookup_sticky("claude", "gen"), None);
    }

    #[tokio::test]
    #[serial]
    async fn sticky_provider_dropped_from_pool_falls_back_to_new_selection() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "session").await;
        let router = ProviderRouter::new(db);
        router
            .load_balancer()
            .insert_sticky("claude", "s1", "ghost");
        let result = router
            .select_providers_for_session("claude", "s1", true, "unknown")
            .await
            .unwrap();
        // ghost 不在池内（熔断/移出队列）→ 按新会话重选并改写粘性
        assert_eq!(result[0].id, "a");
        assert_eq!(
            router.load_balancer().lookup_sticky("claude", "s1"),
            Some("a".to_string())
        );
    }

    #[tokio::test]
    #[serial]
    async fn session_mode_with_failover_disabled_falls_back_to_current_only() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        seed_two_providers_with_mode(&db, "session").await;
        db.set_current_provider("claude", "a").unwrap();
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = false;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db);
        let result = router
            .select_providers_for_session("claude", "s1", true, "unknown")
            .await
            .unwrap();
        // 故障转移关闭 → 池只有当前供应商，粘性无意义
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_disabled_uses_current_provider() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_uses_queue_order_ignoring_current() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 设置 sort_index 来控制顺序：b=1, a=2
        let mut provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        provider_a.sort_index = Some(2);
        let mut provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        provider_b.sort_index = Some(1);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        db.add_to_failover_queue("claude", "b").unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();

        // 启用自动故障转移（使用新的 proxy_config API）
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 2);
        // 故障转移开启时：仅按队列顺序选择（忽略当前供应商）
        assert_eq!(providers[0].id, "b");
        assert_eq!(providers[1].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_uses_queue_only_even_if_current_not_in_queue() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let mut provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        provider_b.sort_index = Some(1);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        // 只把 b 加入故障转移队列（模拟“当前供应商不在队列里”的常见配置）
        db.add_to_failover_queue("claude", "b").unwrap();

        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "b");
    }

    #[tokio::test]
    #[serial]
    async fn codex_official_current_stays_single_route_when_failover_is_stale() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let official = managed_codex_official("official-a", "account-a");
        let fallback = Provider::with_id(
            "fallback".to_string(),
            "Fallback".to_string(),
            json!({}),
            None,
        );
        db.save_provider("codex", &official).unwrap();
        db.save_provider("codex", &fallback).unwrap();
        db.set_current_provider("codex", &official.id).unwrap();
        db.add_to_failover_queue("codex", &fallback.id).unwrap();

        let mut config = db.get_proxy_config_for_app("codex").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let providers = ProviderRouter::new(db)
            .select_providers("codex")
            .await
            .unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, official.id);
    }

    #[tokio::test]
    #[serial]
    async fn stale_codex_official_queue_entries_are_not_retry_targets() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let current = Provider::with_id(
            "third-party".to_string(),
            "Third Party".to_string(),
            json!({}),
            None,
        );
        let official = managed_codex_official("official-a", "account-a");
        let fallback = Provider::with_id(
            "fallback".to_string(),
            "Fallback".to_string(),
            json!({}),
            None,
        );
        db.save_provider("codex", &current).unwrap();
        db.save_provider("codex", &official).unwrap();
        db.save_provider("codex", &fallback).unwrap();
        db.set_current_provider("codex", &current.id).unwrap();
        db.add_to_failover_queue("codex", &official.id).unwrap();
        db.add_to_failover_queue("codex", &fallback.id).unwrap();

        let mut config = db.get_proxy_config_for_app("codex").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let providers = ProviderRouter::new(db)
            .select_providers("codex")
            .await
            .unwrap();
        assert_eq!(
            providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<Vec<_>>(),
            vec!["fallback"]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_select_providers_does_not_consume_half_open_permit() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();

        db.add_to_failover_queue("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();

        // 启用自动故障转移（使用新的 proxy_config API）
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        router
            .record_result("b", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        let providers = router.select_providers("claude").await.unwrap();
        assert_eq!(providers.len(), 2);

        assert!(router.allow_provider_request("b", "claude").await.allowed);
    }

    #[tokio::test]
    #[serial]
    async fn test_release_permit_neutral_frees_half_open_slot() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 配置熔断器：1 次失败即熔断，0 秒超时立即进入 HalfOpen
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();

        // 启用自动故障转移
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 触发熔断：1 次失败
        router
            .record_result("a", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        // 第一次请求：获取 HalfOpen 探测名额
        let first = router.allow_provider_request("a", "claude").await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit);

        // 第二次请求应被拒绝（名额已被占用）
        let second = router.allow_provider_request("a", "claude").await;
        assert!(!second.allowed);

        // 使用 release_permit_neutral 释放名额（不影响健康统计）
        router
            .release_permit_neutral("a", "claude", first.used_half_open_permit)
            .await;

        // 第三次请求应被允许（名额已释放）
        let third = router.allow_provider_request("a", "claude").await;
        assert!(third.allowed);
        assert!(third.used_half_open_permit);
    }
}
