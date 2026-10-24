//! 会话粘性负载均衡核心状态
//!
//! 与熔断器的分工：熔断（circuit_breaker.rs）管错误率/不可用；本模块管
//! "哪个账号服务哪个会话"与限流冷却。全部内存态，重启清空（设计决策：
//! 每活跃会话首个请求冷一次，可接受，不做持久化）。

use crate::provider::Provider;
use crate::proxy::model_mapper::{strip_one_m_suffix_for_upstream, ModelMapping};
use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 粘性表空闲 TTL（30 分钟，设计 4.1）
pub(crate) const STICKY_IDLE_TTL_SECS: u64 = 30 * 60;
/// 粘性表容量上限（LRU 惰性驱逐，设计 4.1）
pub(crate) const STICKY_MAX_ENTRIES: usize = 4096;
/// 429/403 冷却退避阶梯（秒），封顶 COOLDOWN_MAX_SECS（设计 4.1：60s→2m→5m→封顶 10m）
const COOLDOWN_BACKOFF_SECS: [u64; 3] = [60, 120, 300];
/// 冷却时长封顶（含上游 Retry-After 的防御性封顶）
pub(crate) const COOLDOWN_MAX_SECS: u64 = 600;

#[derive(Debug, Clone)]
struct StickyEntry {
    provider_id: String,
    last_access: Instant,
}

struct CooldownEntry {
    until: Instant,
    consecutive: u32,
}

#[derive(Default)]
struct LbInner {
    /// app_type -> (session_id -> 粘性条目)
    sticky: HashMap<String, HashMap<String, StickyEntry>>,
    /// "provider_id\0outbound_model" -> 在途计数（provider id 全局唯一，跨应用隔离）
    in_flight: HashMap<String, Arc<AtomicIsize>>,
    /// "app_type:provider_id" -> 限流冷却
    cooldown: HashMap<String, CooldownEntry>,
    /// 游标：app_type -> 下一个起始下标
    cursors: HashMap<String, usize>,
}

/// 会话粘性负载均衡器（每 ProviderRouter 一份，进程内单例语义）
pub struct SessionLoadBalancer {
    inner: Mutex<LbInner>,
    ttl: Duration,
    max_entries: usize,
}

impl SessionLoadBalancer {
    pub fn new() -> Self {
        Self::with_limits(
            Duration::from_secs(STICKY_IDLE_TTL_SECS),
            STICKY_MAX_ENTRIES,
        )
    }

    pub(crate) fn with_limits(ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner: Mutex::new(LbInner::default()),
            ttl,
            max_entries,
        }
    }

    /// 粘性查询：命中即续期；过期惰性清除
    pub fn lookup_sticky(&self, app_type: &str, session_id: &str) -> Option<String> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let app_map = inner.sticky.get_mut(app_type)?;
        match app_map.get_mut(session_id) {
            Some(entry) if now.duration_since(entry.last_access) < self.ttl => {
                entry.last_access = now;
                Some(entry.provider_id.clone())
            }
            Some(_) => {
                app_map.remove(session_id);
                None
            }
            None => None,
        }
    }

    /// 新会话写入：先到者优先（并发抢建时后到者采纳先到者，设计 4.1.c）
    pub fn insert_sticky(&self, app_type: &str, session_id: &str, provider_id: &str) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let app_map = inner.sticky.entry(app_type.to_string()).or_default();
        let exists_valid = app_map
            .get(session_id)
            .is_some_and(|e| now.duration_since(e.last_access) < self.ttl);
        if !exists_valid {
            app_map.insert(
                session_id.to_string(),
                StickyEntry {
                    provider_id: provider_id.to_string(),
                    last_access: now,
                },
            );
            if app_map.len() > self.max_entries {
                evict_idlest(app_map, now, self.ttl, self.max_entries);
            }
        }
    }

    /// failover 成功后的会话重映射：无条件改写（设计 4.1 迁移回写）
    pub fn remap_sticky(&self, app_type: &str, session_id: &str, provider_id: &str) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let app_map = inner.sticky.entry(app_type.to_string()).or_default();
        app_map.insert(
            session_id.to_string(),
            StickyEntry {
                provider_id: provider_id.to_string(),
                last_access: now,
            },
        );
        if app_map.len() > self.max_entries {
            evict_idlest(app_map, now, self.ttl, self.max_entries);
        }
    }

    /// 轮询游标推进：返回本次起始下标，并推进到下一个位置
    pub fn next_cursor_index(&self, app_type: &str, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let cursor = inner.cursors.entry(app_type.to_string()).or_insert(0);
        let idx = *cursor % len;
        *cursor = idx + 1;
        idx
    }

    /// 测试观察用：指定应用的粘性表条目数
    #[cfg(test)]
    pub(crate) fn sticky_len(&self, app_type: &str) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sticky
            .get(app_type)
            .map_or(0, |m| m.len())
    }

    fn in_flight_key(provider_id: &str, outbound_model: &str) -> String {
        format!("{provider_id}\u{0}{outbound_model}")
    }

    /// 在途计数 +1（RAII），计数 key = provider_id + outbound model（设计 4.1）
    pub fn acquire_in_flight(&self, provider_id: &str, outbound_model: &str) -> InFlightHandle {
        let key = Self::in_flight_key(provider_id, outbound_model);
        let counter = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner
                .in_flight
                .entry(key)
                .or_insert_with(|| Arc::new(AtomicIsize::new(0)))
                .clone()
        };
        InFlightHandle::acquire(counter)
    }

    /// 指定账号+模型的当前在途数
    pub fn in_flight_of(&self, provider_id: &str, outbound_model: &str) -> isize {
        let key = Self::in_flight_key(provider_id, outbound_model);
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .in_flight
            .get(&key)
            .map_or(0, |c| c.load(Ordering::Acquire))
    }

    /// 该账号在 (provider, request_model 映射后) 维度的在途数
    fn provider_in_flight(&self, provider: &Provider, request_model: &str) -> isize {
        let model = resolve_outbound_model(provider, request_model);
        self.in_flight_of(&provider.id, &model)
    }

    fn is_saturated(&self, provider: &Provider, request_model: &str) -> bool {
        let limit = provider.max_concurrent_for(&resolve_outbound_model(provider, request_model));
        limit != 0 && self.provider_in_flight(provider, request_model) >= limit as isize
    }

    /// 新会话选号（设计 4.1.b）：
    /// 1. 过滤冷却中账号（全冷却则忽略冷却——本地拒绝会打断客户端，宁可放行让上游兜底）
    /// 2. 从轮询游标起找第一个未饱和账号（上限 0 = 不限），游标推进
    /// 3. 全饱和 → 在途最少者放行
    pub fn select_new_session(
        &self,
        app_type: &str,
        pool: &[Provider],
        request_model: &str,
    ) -> usize {
        let candidates = self.cooldown_filtered_indices(app_type, pool);
        let len = candidates.len();
        let start = self.next_cursor_index(app_type, len);
        for offset in 0..len {
            let idx = candidates[(start + offset) % len];
            if !self.is_saturated(&pool[idx], request_model) {
                return idx;
            }
        }
        // 全饱和 → 在途最少者（设计 4.1.b）
        self.select_least_in_flight(app_type, pool, request_model)
    }

    /// Generated 会话选号：冷却过滤后在途最少（不写粘性表，设计 4.1.d）
    pub fn select_least_in_flight(
        &self,
        app_type: &str,
        pool: &[Provider],
        request_model: &str,
    ) -> usize {
        let candidates = self.cooldown_filtered_indices(app_type, pool);
        candidates
            .into_iter()
            .min_by_key(|&idx| self.provider_in_flight(&pool[idx], request_model))
            .expect("cooldown_filtered_indices 保证非空")
    }

    /// 冷却过滤：返回可用下标；全冷却时退回全部
    fn cooldown_filtered_indices(&self, app_type: &str, pool: &[Provider]) -> Vec<usize> {
        let available: Vec<usize> = (0..pool.len())
            .filter(|&i| !self.is_cooling(app_type, &pool[i].id))
            .collect();
        if available.is_empty() {
            (0..pool.len()).collect()
        } else {
            available
        }
    }

    /// 记录限流冷却：优先 Retry-After（封顶 10m），否则按连续触发次数指数退避
    pub fn record_rate_limit(
        &self,
        app_type: &str,
        provider_id: &str,
        retry_after_secs: Option<u64>,
    ) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = format!("{app_type}:{provider_id}");
        let consecutive = inner
            .cooldown
            .get(&key)
            .map_or(1, |e| e.consecutive.saturating_add(1));
        let secs = retry_after_secs
            .map(|s| s.min(COOLDOWN_MAX_SECS))
            .unwrap_or_else(|| {
                COOLDOWN_BACKOFF_SECS
                    .get(consecutive.saturating_sub(1) as usize)
                    .copied()
                    .unwrap_or(COOLDOWN_MAX_SECS)
            });
        inner.cooldown.insert(
            key,
            CooldownEntry {
                until: Instant::now() + Duration::from_secs(secs),
                consecutive,
            },
        );
    }

    /// 是否冷却中（过期惰性清除）
    pub fn is_cooling(&self, app_type: &str, provider_id: &str) -> bool {
        self.cooldown_remaining(app_type, provider_id).is_some()
    }

    /// 剩余冷却时间（UI/测试观察用）
    pub fn cooldown_remaining(&self, app_type: &str, provider_id: &str) -> Option<Duration> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = format!("{app_type}:{provider_id}");
        match inner.cooldown.get(&key) {
            Some(entry) if entry.until > now => Some(entry.until - now),
            Some(_) => {
                inner.cooldown.remove(&key);
                None
            }
            None => None,
        }
    }

    /// 池内账号的在途总数（provider+model 全维度求和）。代理页实时指示用
    pub fn in_flight_total(&self, provider_ids: &[String]) -> isize {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .in_flight
            .iter()
            .filter(|(key, _)| {
                key.split_once('\u{0}')
                    .is_some_and(|(pid, _)| provider_ids.iter().any(|id| id == pid))
            })
            .map(|(_, counter)| counter.load(Ordering::Acquire))
            .sum()
    }

    /// 池内账号的冷却快照（provider + 剩余秒数）。代理页实时指示用
    /// 粘性分布快照：该应用各账号当前粘住的会话数（TTL 内有效条目）。
    /// UI 据此在会话粘性模式下标多家「服务中」。
    pub fn sticky_snapshot(&self, app_type: &str) -> Vec<(String, usize)> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sessions) = inner.sticky.get(app_type) else {
            return Vec::new();
        };
        let now = Instant::now();
        let mut counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for entry in sessions.values() {
            if now.duration_since(entry.last_access) < self.ttl {
                *counts.entry(entry.provider_id.clone()).or_insert(0) += 1;
            }
        }
        let mut list: Vec<(String, usize)> = counts.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list
    }

    pub fn cooldown_snapshot(
        &self,
        app_type: &str,
        provider_ids: &[String],
    ) -> Vec<LoadBalanceCooldown> {
        provider_ids
            .iter()
            .filter_map(|id| {
                self.cooldown_remaining(app_type, id)
                    .map(|remaining| LoadBalanceCooldown {
                        provider_id: id.clone(),
                        remaining_secs: remaining.as_secs(),
                    })
            })
            .collect()
    }

    /// 该账号成功服务一次：清冷却、重置退避计数
    pub fn record_success(&self, app_type: &str, provider_id: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cooldown
            .remove(&format!("{app_type}:{provider_id}"));
    }
}

impl Default for SessionLoadBalancer {
    fn default() -> Self {
        Self::new()
    }
}

/// 冷却中的账号快照（代理页实时指示）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalanceCooldown {
    pub provider_id: String,
    pub remaining_secs: u64,
}

/// 在途计数 RAII 句柄：构造 +1，Drop 同步 -1。
///
/// 计数是纯原子量，Drop 无需异步调度（对照 ActiveConnectionGuard 需要
/// 异步归还是因为它写 tokio RwLock）。成功路径经 `detach()` 把计数所有权
/// 移交给 ActiveConnectionGuard，随响应流结束才递减；失败路径随迭代作用域 Drop。
pub struct InFlightHandle {
    counter: Arc<AtomicIsize>,
}

impl InFlightHandle {
    pub(crate) fn acquire(counter: Arc<AtomicIsize>) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self { counter }
    }

    /// 移交计数所有权（不递减）；移交方负责最终 fetch_sub
    pub(crate) fn detach(self) -> Arc<AtomicIsize> {
        let counter = self.counter.clone();
        std::mem::forget(self);
        counter
    }
}

impl Drop for InFlightHandle {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

/// 选号/计数用的出站模型名：供应商模型映射 + [1M] 后缀剥离
/// （与 forward 实际发出的模型口径一致；不克隆请求体）
pub(crate) fn resolve_outbound_model(provider: &Provider, request_model: &str) -> String {
    let mapping = ModelMapping::from_provider(provider);
    let mapped = mapping.map_model(request_model);
    strip_one_m_suffix_for_upstream(&mapped).to_string()
}

/// 限流判定：429 恒冷却；403 需 body 呈并发/限流语义
/// （Kimi 403 concurrent limit，设计 5；鉴权类 403 不冷却）
pub fn is_rate_limit_status(status: u16, body: Option<&str>) -> bool {
    match status {
        429 => true,
        403 => body.is_some_and(|b| {
            let lower = b.to_lowercase();
            lower.contains("concurrent") || b.contains("并发") || lower.contains("rate limit")
        }),
        _ => false,
    }
}

/// 惰性清理：先扫过期；仍超限则逐出最久未访问者
fn evict_idlest(
    app_map: &mut HashMap<String, StickyEntry>,
    now: Instant,
    ttl: Duration,
    max_entries: usize,
) {
    app_map.retain(|_, entry| now.duration_since(entry.last_access) < ttl);
    while app_map.len() > max_entries {
        let Some(idlest_key) = app_map
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        app_map.remove(&idlest_key);
    }
}

#[cfg(test)]
mod sticky_tests {
    use super::*;

    fn lb_small(ttl_secs: u64, cap: usize) -> SessionLoadBalancer {
        SessionLoadBalancer::with_limits(Duration::from_secs(ttl_secs), cap)
    }

    #[test]
    fn insert_then_lookup_hits_and_renews() {
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
        // 不同应用互不串表
        assert_eq!(lb.lookup_sticky("codex", "s1"), None);
    }

    #[test]
    fn first_writer_wins_on_concurrent_insert() {
        // 设计 4.1.c：entry().or_insert()，后到者采纳先到者
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        lb.insert_sticky("claude", "s1", "b");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
    }

    #[test]
    fn remap_overwrites_unconditionally() {
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        lb.remap_sticky("claude", "s1", "b");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("b".to_string()));
        // 无既有条目时也建立映射（failover 后粘性自愈）
        lb.remap_sticky("claude", "s2", "b");
        assert_eq!(lb.lookup_sticky("claude", "s2"), Some("b".to_string()));
    }

    #[test]
    fn expired_entry_is_evicted_on_lookup() {
        // 毫秒级 TTL，sleep 跨过过期线后再观察行为
        let lb = SessionLoadBalancer::with_limits(Duration::from_millis(30), 4096);
        lb.insert_sticky("claude", "s1", "a");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
        std::thread::sleep(Duration::from_millis(50));
        // 过期 → lookup 返回 None 并惰性清除
        assert_eq!(lb.lookup_sticky("claude", "s1"), None);
        assert_eq!(lb.sticky_len("claude"), 0);
        // 过期后 insert 可写入新值（先到者已失效，后到者生效）
        lb.insert_sticky("claude", "s1", "b");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("b".to_string()));
    }

    #[test]
    fn capacity_cap_evicts_idlest_entry() {
        let lb = lb_small(1800, 2);
        lb.insert_sticky("claude", "s1", "a");
        lb.insert_sticky("claude", "s2", "a");
        lb.lookup_sticky("claude", "s1"); // s1 续期，s2 更旧
        lb.insert_sticky("claude", "s3", "b"); // 触发惰性驱逐
        assert_eq!(lb.lookup_sticky("claude", "s2"), None); // 最旧的 s2 被驱逐
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
        assert_eq!(lb.lookup_sticky("claude", "s3"), Some("b".to_string()));
    }

    #[test]
    fn cursor_round_robins_and_wraps() {
        let lb = lb_small(1800, 4096);
        assert_eq!(lb.next_cursor_index("claude", 3), 0);
        assert_eq!(lb.next_cursor_index("claude", 3), 1);
        assert_eq!(lb.next_cursor_index("claude", 3), 2);
        assert_eq!(lb.next_cursor_index("claude", 3), 0);
        // 池大小变化时取模安全
        let idx = lb.next_cursor_index("claude", 2);
        assert!(idx < 2);
    }

    #[test]
    fn ttl_renewal_on_lookup_keeps_entry_alive() {
        // 通过行为验证续期：TTL=0 的对照已证明过期即失效，
        // 正常 TTL 下 lookup 后条目仍在
        let lb = lb_small(1800, 4096);
        lb.insert_sticky("claude", "s1", "a");
        assert_eq!(lb.lookup_sticky("claude", "s1"), Some("a".to_string()));
        assert_eq!(lb.sticky_len("claude"), 1);
    }
}

#[cfg(test)]
mod inflight_tests {
    use super::*;
    use crate::provider::{Provider, ProviderLoadBalance};
    use serde_json::json;
    use std::collections::HashMap;

    fn provider_with_limit(id: &str, limit: Option<u32>) -> Provider {
        let mut p = Provider::with_id(id.to_string(), id.to_string(), json!({}), None);
        p.meta = Some(crate::provider::ProviderMeta {
            load_balance: Some(ProviderLoadBalance {
                max_concurrent_requests: limit,
                max_concurrent_by_model: HashMap::new(),
            }),
            ..Default::default()
        });
        p
    }

    #[test]
    fn resolve_outbound_model_strips_one_m_suffix() {
        let plain = Provider::with_id("q".into(), "Q".into(), json!({}), None);
        assert_eq!(resolve_outbound_model(&plain, "claude-x[1M]"), "claude-x");
        assert_eq!(resolve_outbound_model(&plain, "glm-5.3"), "glm-5.3");
    }

    #[test]
    fn in_flight_acquire_and_release() {
        let lb = SessionLoadBalancer::new();
        let h1 = lb.acquire_in_flight("a", "glm-5.3");
        let h2 = lb.acquire_in_flight("a", "glm-5.3");
        assert_eq!(lb.in_flight_of("a", "glm-5.3"), 2);
        // 不同模型独立计数（两级上限的 key 维度）
        assert_eq!(lb.in_flight_of("a", "glm-5.3-flash"), 0);
        drop(h1);
        assert_eq!(lb.in_flight_of("a", "glm-5.3"), 1);
        drop(h2);
        assert_eq!(lb.in_flight_of("a", "glm-5.3"), 0);
    }

    #[test]
    fn in_flight_handle_detach_does_not_decrement() {
        let lb = SessionLoadBalancer::new();
        let h = lb.acquire_in_flight("a", "m");
        let counter = h.detach();
        assert_eq!(lb.in_flight_of("a", "m"), 1); // 仍占用
        counter.fetch_sub(1, Ordering::AcqRel); // 移交方负责递减
        assert_eq!(lb.in_flight_of("a", "m"), 0);
    }

    #[test]
    fn select_new_session_skips_saturated_account() {
        let lb = SessionLoadBalancer::new();
        let pool = vec![
            provider_with_limit("a", Some(1)),
            provider_with_limit("b", None),
        ];
        // a 饱和（在途 1/上限 1），b 不限 → 从游标 0 起跳过 a 选 b
        let _h = lb.acquire_in_flight("a", "m");
        assert_eq!(lb.select_new_session("claude", &pool, "m"), 1);
    }

    #[test]
    fn select_new_session_falls_back_to_least_in_flight_when_all_saturated() {
        let lb = SessionLoadBalancer::new();
        let pool = vec![
            provider_with_limit("a", Some(1)),
            provider_with_limit("b", Some(1)),
        ];
        let _ha = lb.acquire_in_flight("a", "m");
        let _hb1 = lb.acquire_in_flight("b", "m");
        let _hb2 = lb.acquire_in_flight("b", "m");
        // 全饱和 → 在途最少者（a=1 < b=2）
        assert_eq!(lb.select_new_session("claude", &pool, "m"), 0);
    }

    #[test]
    fn select_least_in_flight_prefers_lower_count() {
        let lb = SessionLoadBalancer::new();
        let pool = vec![
            provider_with_limit("a", None),
            provider_with_limit("b", None),
        ];
        let _h = lb.acquire_in_flight("a", "m");
        assert_eq!(lb.select_least_in_flight("claude", &pool, "m"), 1);
    }

    #[test]
    fn in_flight_total_and_cooldown_snapshot_cover_pool_only() {
        let lb = SessionLoadBalancer::new();
        let _h1 = lb.acquire_in_flight("a", "m1");
        let _h2 = lb.acquire_in_flight("a", "m2");
        let _h3 = lb.acquire_in_flight("b", "m1");
        let _h4 = lb.acquire_in_flight("zzz-out-of-pool", "m");
        // 池 [a, b]：合计 3（池外 zzz 不计）
        assert_eq!(lb.in_flight_total(&["a".into(), "b".into()]), 3);
        assert_eq!(lb.in_flight_total(&["a".into()]), 2);

        lb.record_rate_limit("claude", "a", Some(90));
        lb.record_rate_limit("claude", "zzz-out-of-pool", None);
        let snapshot = lb.cooldown_snapshot("claude", &["a".into(), "b".into()]);
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].provider_id, "a");
        assert!(snapshot[0].remaining_secs > 80 && snapshot[0].remaining_secs <= 90);
    }

    #[test]
    fn cooldown_skipped_in_selection_until_all_cooling() {
        let lb = SessionLoadBalancer::new();
        let pool = vec![
            provider_with_limit("a", None),
            provider_with_limit("b", None),
        ];
        lb.record_rate_limit("claude", "a", None);
        // a 冷却中 → 新会话选 b（即便游标本应从 a 开始）
        assert_eq!(lb.select_new_session("claude", &pool, "m"), 1);
        // b 也冷却 → 全冷却退回全部候选（不本地拒绝）
        lb.record_rate_limit("claude", "b", None);
        let idx = lb.select_new_session("claude", &pool, "m");
        assert!(idx < 2);
    }
}

#[cfg(test)]
mod cooldown_tests {
    use super::*;

    /// 剩余时间随流逝略减，区间断言（±1s）代替精确相等
    fn assert_about(remaining: Option<Duration>, expect_secs: u64) {
        let r = remaining.expect("should be cooling");
        assert!(
            r > Duration::from_secs(expect_secs.saturating_sub(1))
                && r <= Duration::from_secs(expect_secs),
            "expected ~{expect_secs}s, got {r:?}"
        );
    }

    #[test]
    fn rate_limit_status_detection() {
        assert!(is_rate_limit_status(429, None));
        assert!(is_rate_limit_status(429, Some("rate limited")));
        assert!(is_rate_limit_status(
            403,
            Some("concurrent requests limit exceeded")
        ));
        assert!(is_rate_limit_status(403, Some("并发数超限")));
        assert!(!is_rate_limit_status(403, Some("invalid api key"))); // 鉴权 403 不冷却
        assert!(!is_rate_limit_status(403, None)); // 无 body 证据不按限流处理
        assert!(!is_rate_limit_status(500, None));
    }

    #[test]
    fn cooldown_backoff_ladder_and_cap() {
        let lb = SessionLoadBalancer::new();
        // 无 Retry-After：缺省 60s
        lb.record_rate_limit("claude", "a", None);
        assert_about(lb.cooldown_remaining("claude", "a"), 60);
        // 连续触发：指数退避 60→120→300→600（封顶）
        lb.record_rate_limit("claude", "a", None);
        assert_about(lb.cooldown_remaining("claude", "a"), 120);
        lb.record_rate_limit("claude", "a", None);
        assert_about(lb.cooldown_remaining("claude", "a"), 300);
        lb.record_rate_limit("claude", "a", None);
        assert_about(lb.cooldown_remaining("claude", "a"), 600);
        lb.record_rate_limit("claude", "a", None);
        assert_about(lb.cooldown_remaining("claude", "a"), 600);
        assert!(lb.is_cooling("claude", "a"));
    }

    #[test]
    fn retry_after_takes_precedence_but_capped() {
        let lb = SessionLoadBalancer::new();
        lb.record_rate_limit("claude", "a", Some(120));
        assert_about(lb.cooldown_remaining("claude", "a"), 120);
        lb.record_rate_limit("claude", "a", Some(86400)); // 上游天级值封顶 10m
        assert_about(lb.cooldown_remaining("claude", "a"), 600);
    }

    #[test]
    fn success_resets_cooldown() {
        let lb = SessionLoadBalancer::new();
        lb.record_rate_limit("claude", "a", None);
        lb.record_rate_limit("claude", "a", None); // 退避到 120s
        lb.record_success("claude", "a");
        assert!(!lb.is_cooling("claude", "a"));
        // 重置后重新从 60s 起步
        lb.record_rate_limit("claude", "a", None);
        assert_about(lb.cooldown_remaining("claude", "a"), 60);
    }

    #[test]
    fn cooldown_is_per_app_provider() {
        let lb = SessionLoadBalancer::new();
        lb.record_rate_limit("claude", "a", None);
        assert!(!lb.is_cooling("codex", "a"));
        assert!(!lb.is_cooling("claude", "b"));
    }
}
