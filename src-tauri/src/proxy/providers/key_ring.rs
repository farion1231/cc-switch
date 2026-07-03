//! Per-Provider API Key Ring
//!
//! 维护每 provider 多把 API key 的运行时状态——cooldown、failure_count、
//! round-robin 游标。**不**持久化自身，进程启动时从 `provider_api_keys`
//! 表 reload（`reload_from_db`）；运行时变更（cooldown、failure）通过
//! `flush_dirty` 写回，每 30s tick 触发一次（由 `ProxyService::start` 启动）。
//!
//! 设计参考 [`CopilotAuthManager`](super::copilot_auth.rs) 的多账号结构：
//! in-memory HashMap of pools + per-provider mutex（防止并发选 key 时
//! 同一个 cursor 被多个请求抢到）。
//!
//! ## 与 OAuth provider 的关系
//!
//! OAuth-managed provider（GitHub Copilot、Codex OAuth）由 `meta.provider_type`
//! 标识，凭据通过 device-code 流程动态获取。KeyRing 只处理静态 key pool：
//! 对 OAuth provider，`next_key` 返回 `None`，forwarder 走原本的占位路径。
//! 这是显式设计选择——避免把 OAuth 的 token 刷新逻辑和静态 key 的冷却逻辑
//! 缠在一起。

use crate::app_config::AppType;
use crate::database::dao::api_keys::ProviderApiKey;
use crate::database::Database;
use crate::error::AppError;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// 单 key 的运行时快照——KeyRing 持有此结构（不含 raw api_key 之外的字段）。
///
/// `cooldown_until` 与 `failure_count` 在每次 `mark_*` 时更新；
/// `flush_dirty` 时写回 DB 行。
#[derive(Debug, Clone)]
pub struct KeyState {
    pub key_id: String,
    pub api_key: String,
    pub enabled: bool,
    pub sort_index: i64,
    pub is_active: bool,
    pub cooldown_until: i64,
    pub failure_count: i64,
    pub last_used_at: Option<i64>,
    pub last_error: Option<String>,
    pub label: String,
}

impl KeyState {
    /// 从 DAO 行构造。仅拷贝运行时需要的字段——`tags`/`notes` 等静态
    /// 属性在 UI 层直接读 DB，不进 KeyRing。
    fn from_db_row(row: &ProviderApiKey) -> Self {
        Self {
            key_id: row.id.clone(),
            api_key: row.api_key.clone(),
            enabled: row.enabled,
            sort_index: row.sort_index,
            is_active: row.is_active,
            cooldown_until: row.cooldown_until,
            failure_count: row.failure_count,
            last_used_at: row.last_used_at,
            last_error: row.last_error.clone(),
            label: row.label.clone(),
        }
    }

    /// key 是否在 cooldown（now < cooldown_until）。
    fn is_cooling(&self, now: i64) -> bool {
        self.cooldown_until > now
    }
}

/// 历史阈值常量（保留以便 UI 与日志引用）——同 key 连续 N 次失败
/// 后**不再**自动 disable。所有 key 始终可被 `next_key` 选中：失败的
/// key 只会进入 cooldown，过了 `reap_expired_cooldowns` 窗口（30s tick）
/// 后 failure_count 重置回 0，key 立刻重新可用。
///
/// 旧实现会在这里设 `enabled = false` + 推进 cursor 到下一把；
/// 现在的语义是「只轮换、不永久停用」：让故障 provider 的所有 key
/// 都被试过一遍后（每个 key 30s 冷却 + request 内的 max_key_attempts），
/// forwarder 在同一请求内 break 'rotate 推进到下一家 provider。
/// 跨请求的轮换是 `next_key` 的 sticky 行为，cursor 不会因为失败而
/// 推进（直到整池被 cooldown 耗尽）。
///
/// UI 仍然用这个数字显示"已失败 N/5 次"——保留常量供前端 i18n 复用。
#[allow(dead_code)]
const AUTO_DISABLE_FAILURE_THRESHOLD: i64 = 5;

/// key 冷却的下限：即便上游 `Retry-After: 1`，我们也至少冷却 30s，
/// 避免 1s 后被同一批并发请求立即打回去。
const MIN_COOLDOWN_SECS: i64 = 30;

pub struct KeyRing {
    /// 每 provider 一个池子。Key = (provider_id, app_type)。
    pools: Arc<RwLock<HashMap<(String, String), Vec<KeyState>>>>,
    /// round-robin 游标：每 provider 一把，独立递增。
    cursors: Arc<RwLock<HashMap<(String, String), usize>>>,
    /// 防止并发选 key 时同一个 cursor 被两个请求同时消费。
    /// Key 与 pools 一致；锁释放后 cursor 已递增，下一个请求拿到下一把。
    selection_locks: Arc<RwLock<HashMap<(String, String), Arc<Mutex<()>>>>>,
    /// 自上次 flush 以来改动过 cooldown/failure 的 key_id 集合。
    /// 写读分离：mark_* 立即更新内存，flush 时只把 dirty 的 key 写回 DB。
    dirty: Arc<RwLock<HashSet<String>>>,
}

impl Default for KeyRing {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyRing {
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            cursors: Arc::new(RwLock::new(HashMap::new())),
            selection_locks: Arc::new(RwLock::new(HashMap::new())),
            dirty: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// 从 DB reload 所有 provider 的 key 池。
    ///
    /// **销毁**当前内存状态并用最新 DB 数据替换——通常在启动时调用。
    /// 已被 mark_* 改动但尚未 flush 的 dirty 集合会丢失（启动时为空，
    /// 这是正确的）。
    pub async fn reload_from_db(&self, db: &Database) -> Result<(), AppError> {
        let provider_keys = collect_all_provider_keys(db)?;
        let mut pools = self.pools.write().await;
        let mut cursors = self.cursors.write().await;
        let mut locks = self.selection_locks.write().await;
        let mut dirty = self.dirty.write().await;

        pools.clear();
        cursors.clear();
        locks.clear();
        dirty.clear();

        for (pid, app, key) in provider_keys {
            let entry = pools.entry((pid.clone(), app.clone())).or_default();
            entry.push(KeyState::from_db_row(&key));
            locks
                .entry((pid.clone(), app.clone()))
                .or_insert_with(|| Arc::new(Mutex::new(())));
        }
        // 初始化 cursor 到 active key 的位置——首次 next_key 用 active key。
        // 没有 active key 时退回 sort_index=0（与 active_key() 行为一致）。
        // 旧实现固定从 0 开始，会让 round-robin 把 active key 当成普通 key 处理。
        for (key, pool) in pools.iter() {
            let active_idx = pool.iter().position(|k| k.is_active).unwrap_or(0);
            cursors.insert(key.clone(), active_idx);
        }
        Ok(())
    }

    /// 增量 reload 一个 provider 的 key 池——UI 添加/删除 key 时调用，
    /// 避免整库 reload 的抖动。
    ///
    /// 顺便把 cursor 重置到 active key 的位置。理由：
    ///   - 池子大小变了（新增/删除 key），cursor 原来指的位置可能
    ///     越界或指向一把被删掉的 key
    ///   - 用户切换 active key 后，cursor 必须立即落到新 active 上，
    ///     否则下次请求会继续打老 key
    /// 旧实现 cursor 留在原值，会出现"刚切了 active 但请求仍走老 key"的回归。
    pub async fn reload_provider(
        &self,
        db: &Database,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), AppError> {
        let keys = db.list_api_keys(provider_id, app_type)?;
        let key_tuple = (provider_id.to_string(), app_type.to_string());
        let mut pools = self.pools.write().await;
        let mut locks = self.selection_locks.write().await;
        let active_idx = keys.iter().position(|k| k.is_active).unwrap_or(0);
        pools.insert(
            key_tuple.clone(),
            keys.iter().map(KeyState::from_db_row).collect(),
        );
        locks
            .entry(key_tuple.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())));
        drop(pools);
        drop(locks);

        // cursor 单独更新——不持 pools 锁时写 cursors，避免和 mark_failure
        // 形成 pools→cursors 反向锁顺序。
        let mut cursors = self.cursors.write().await;
        cursors.insert(key_tuple, active_idx);
        Ok(())
    }

    /// 选下一把可用 key。
    ///
    /// 选中规则（按 sort_index ASC 顺序循环）：
    /// 1. `enabled == true`
    /// 2. `now >= cooldown_until`
    ///
    /// **Round-robin 行为**：每次成功选到 key 之后，把 cursor 推到
    /// `(idx + 1) % len`——下次 next_key 从下一个位置开始，强制轮换
    /// 穿透整池。这与早期 sticky 实现相反：sticky 让 cursor 钉在
    /// 同一把 key 上直到它失败，结果是失败后只在该 key 冷却期内
    /// 临时切到下一把，等冷却结束又跳回去「同两把 key 之间反复」。
    /// round-robin 让 cursor 持续推进，确保所有 key 都被均匀使用。
    ///
    /// 返回 `None` 表示：
    /// - 池为空（provider 还没配置 key）
    /// - 全部 disabled 或在 cooldown（调用方应退到下一家 provider）
    pub async fn next_key(
        &self,
        provider_id: &str,
        app_type: &AppType,
    ) -> Option<KeyState> {
        let app_str = app_type.as_str().to_string();
        let key = (provider_id.to_string(), app_str.clone());

        // 获取（或创建）该 provider 的 selection lock——防止并发 next_key
        // 把同一个 cursor 消费两次。锁本身创建在 reload 阶段；
        // 这里如未创建，池为空，直接返回 None。
        let locks_guard = self.selection_locks.read().await;
        let lock = locks_guard.get(&key)?.clone();
        drop(locks_guard);

        // 取锁保护 cursor 修改；这是 per-provider 的，跨 provider 并行。
        let _guard = lock.lock().await;

        let pools = self.pools.read().await;
        let pool = pools.get(&key)?;
        if pool.is_empty() {
            return None;
        }
        let now = chrono::Utc::now().timestamp();

        let cursors = self.cursors.read().await;
        let cursor = cursors.get(&key).copied().unwrap_or(0);
        drop(cursors);
        let len = pool.len();

        // cursor 可能因 key 被删而越界，clamp 到 [0, len)。
        let start = cursor % len;

        // 最多绕一圈（len 次尝试）。从 cursor 开始，逐项检查可用性。
        // **命中时推进 cursor 到 (idx + 1) % len** —— round-robin 行为：
        // 每次成功选到 key 后，cursor 推到下一把，下一次 next_key 必然
        // 落到不同位置。这样即便所有请求都成功，cursor 也会逐步推进，
        // 不会卡在同一把 key 上。
        //
        // 跳过冷却 / 停用 的 key 不会推进 cursor——cursor 由「成功选到」
        // 那一刻推进，cooldown 中的 key 不算成功选中。
        for offset in 0..len {
            let idx = (start + offset) % len;
            let candidate = &pool[idx];
            if candidate.enabled && !candidate.is_cooling(now) {
                // 命中：推进 cursor 到 (idx + 1) % len
                let next_cursor = (idx + 1) % len;
                let mut cursors = self.cursors.write().await;
                cursors.insert(key.clone(), next_cursor);
                return Some(candidate.clone());
            }
        }
        // 整池都不可用——不推进 cursor（避免跳过未来可用的位置）
        None
    }

    /// 标记某把 key 被限流/配额耗尽。设置 cooldown_until + failure_count += 1。
    /// `retry_after_secs` 来自上游 Retry-After（None 或 0 走最小冷却）。
    pub async fn mark_rate_limited(
        &self,
        key_id: &str,
        retry_after_secs: Option<u64>,
    ) -> KeyStateUpdate {
        let cooldown = retry_after_secs
            .unwrap_or(0)
            .max(MIN_COOLDOWN_SECS as u64) as i64;
        self.apply_update(
            key_id,
            cooldown,
            |count| count + 1,
            Some(format!(
                "rate limited; cooldown {}s",
                retry_after_secs.unwrap_or(MIN_COOLDOWN_SECS as u64)
            )),
        )
        .await
    }

    /// 标记某把 key 用量接近上限（proactive rotation）
    ///
    /// 当自动查询（autoQueryInterval 触发）发现某把 key 的 `usage_percent`
    /// 超过 `USAGE_WARN_PERCENT`（默认 90%），由前端调用本接口把这把
    /// key 提前送进 cooldown——`cooldown_until` 设为 `reset_at`（即下一个
    /// 5h 窗口重置时间戳）。
    ///
    /// **为什么是 proactive 而不是等 429**：等到上游返回 429/5xx 才切
    /// key 的问题是请求已经白扔了。提前在 quota 临界点切换可以：
    /// 1) 减少「先失败再切 key」的来回延迟（upstream 失败本身要 1-3s）
    /// 2) 给前端的 5h/7d 进度条一个真实的「切换原因」展示
    /// 3) 让 5h 重置后 key 自动可用（cooldown_until = reset_at，reap 后
    ///    next_key 重新选中这把 key）
    ///
    /// **threshold 等级**：
    /// - 90-99%：warning，cooldown 到 reset_at（5h 重置后自动恢复）
    /// - >= 100%：exhausted，cooldown 到 max(reset_at, now + 30min)，给
    ///   系统一个额外缓冲避免反复打爆
    ///
    /// **不修改 failure_count**：proactive rotation 不代表「失败」，只
    /// 代表「已接近上限」。UI 仍然只显示 react 429/5xx 路径产生的失败
    /// 计数。
    ///
    /// **不自动 disable**：即便 100% 也不动 `enabled` 字段——`enabled`
    /// 仍由用户手动控制（settings 页面）。
    pub async fn mark_usage_high(
        &self,
        key_id: &str,
        usage_percent: f64,
        reset_at: i64,
    ) -> bool {
        const USAGE_WARN_PERCENT: f64 = 90.0;
        const USAGE_EXHAUSTED_PERCENT: f64 = 100.0;
        const EXHAUSTED_MIN_COOLDOWN_SECS: i64 = 30 * 60; // 30 min

        let now = chrono::Utc::now().timestamp();
        let cooldown_until = if usage_percent >= USAGE_EXHAUSTED_PERCENT {
            // 完全耗尽：cooldown 至少撑到 reset 之后 30min
            std::cmp::max(reset_at, now + EXHAUSTED_MIN_COOLDOWN_SECS)
        } else if usage_percent >= USAGE_WARN_PERCENT {
            // 接近耗尽：cooldown 到 reset_at（5h 重置后恢复）
            reset_at
        } else {
            // 还在安全区，不动
            return false;
        };

        self.apply_update(
            key_id,
            cooldown_until.saturating_sub(now).max(MIN_COOLDOWN_SECS),
            |count| count, // 不 bump failure_count
            Some(format!(
                "proactive rotation: usage {:.1}%",
                usage_percent
            )),
        )
        .await;
        true
    }

    /// 标记某把 key 成功——清零 cooldown + failure_count。
    pub async fn mark_success(&self, key_id: &str) {
        self.apply_update(
            key_id,
            0,
            |_| 0,
            None,
        )
        .await;
    }

    /// 标记某把 key 非限流失败（5xx、超时等）——bump failure_count 给 UI
    /// 显示用，**不**设置 cooldown 也**不**自动 disable。
    ///
    /// 历史行为：阈值达到 `AUTO_DISABLE_FAILURE_THRESHOLD` 自动 disable。
    /// 现在的语义是「轮换但不永久停用」——这把 key 仍然在池中，
    /// `next_key` 仍会选中它；下一次同 key 调用失败只在 UI 上把
    /// "已失败 N/5 次" 累加。`reap_expired_cooldowns` 在 30s tick 里把
    /// cooldown 过期时清零 `failure_count`，所以 UI 看到的是"短时间内
    /// N 次连续失败 → 5/5 短暂出现 → 冷却结束 → 重置 0/5"。
    ///
    /// 跨 provider 轮换由 forwarder 的 `'rotate` 循环负责：
    /// 同一 provider 内 `max_key_attempts` 把每把 key 试完后，
    /// `KeyRing::next_key` 返回 None，forwarder 推进到 failover 队列
    /// 里的下一家 provider。
    ///
    /// 锁顺序：pools (write) → dirty (write)。与 `flush_dirty` 反向。
    pub async fn mark_failure(&self, key_id: &str, err: &str) {
        let now = chrono::Utc::now().timestamp();
        let mut found = false;

        {
            let mut pools = self.pools.write().await;
            for (_pid, pool) in pools.iter_mut() {
                if let Some(k) = pool.iter_mut().find(|k| k.key_id == key_id) {
                    k.failure_count += 1;
                    k.last_used_at = Some(now);
                    k.last_error = Some(err.to_string());
                    found = true;
                    break;
                }
            }
        } // pools 锁释放

        if found {
            let mut dirty = self.dirty.write().await;
            dirty.insert(key_id.to_string());
        }
    }

    /// 把内存中的 dirty 改动（cooldown / failure_count / last_used_at /
    /// last_error）持久化到 DB。失败仅告警——内存已更新，下次 tick 重试。
    ///
    /// 锁顺序：
    ///   1. `dirty.read()` 收集所有 dirty key 列表（短持锁）
    ///   2. `pools.write()` 短暂持锁，clone 出每个 key 的运行时快照
    ///   3. 释放 pools 锁
    ///   4. **DB 写在锁外**——`db.write_api_key_runtime` 是 sync 调用（~1ms/key），
    ///      不能阻塞 forwarder 的 `next_key` / `snapshot` 读路径
    ///   5. 拿 `dirty.write()`，移除已成功持久化的 key
    ///
    /// 与 `mark_*` 反向锁序（pools → dirty）：flush 路径"先 dirty 后 pools"，
    /// mark 路径"先 pools 后 dirty"。两路径各取一个锁时不会循环等待。
    pub async fn flush_dirty(&self, db: &Database) -> Result<usize, AppError> {
        let dirty_keys: Vec<String> = {
            let dirty = self.dirty.read().await;
            dirty.iter().cloned().collect()
        };
        if dirty_keys.is_empty() {
            return Ok(0);
        }

        // Step 1: 在持有 pools 锁期间收集快照（DB 写需要的字段）。
        // 这一段保持锁，迫使 forwarder 短暂等待，但写入逻辑很轻（仅 clone 字段），
        // 不是原来的"边 hold 锁边做 sync DB 写"那种长临界区。
        let now = chrono::Utc::now().timestamp();
        let to_persist: Vec<(String, i64, i64, Option<i64>, Option<String>)> = {
            let mut pools = self.pools.write().await;
            let mut out = Vec::with_capacity(dirty_keys.len());
            for key_id in &dirty_keys {
                if let Some(snapshot) = pools.values_mut().find_map(|pool| {
                    pool.iter_mut().find(|k| &k.key_id == key_id).map(|k| {
                        if k.last_used_at.is_none() {
                            k.last_used_at = Some(now);
                        }
                        (
                            k.key_id.clone(),
                            k.cooldown_until,
                            k.failure_count,
                            k.last_used_at,
                            k.last_error.clone(),
                        )
                    })
                }) {
                    out.push(snapshot);
                }
            }
            out
        };
        // 锁在此释放（pools write guard drop）

        // Step 2: 在无锁状态下写 DB（失败仅告警，不阻断 forwarder 读路径）。
        let mut persisted_keys: Vec<String> = Vec::with_capacity(to_persist.len());
        for (key_id, cooldown_until, failure_count, last_used_at, last_error) in to_persist {
            if let Err(e) = db.write_api_key_runtime(
                &key_id,
                cooldown_until,
                failure_count,
                last_used_at,
                last_error.as_deref(),
                now,
            ) {
                log::warn!("[KeyRing] flush dirty key={} 失败: {e}", key_id);
                continue;
            }
            persisted_keys.push(key_id);
        }

        // Step 3: 把"已被处理"的 key（写成功 + 池中找不到 / 已被删除）从 dirty 集合移除。
        // —— 之前只移写成功的，留下了"已被 delete 但仍在 dirty"的脏 entry，
        // 下次 flush_dirty 又重新尝试并失败，永久 stuck。修复：池中找不到也视作处理完。
        let mut dirty = self.dirty.write().await;
        // 先把写成功的移走
        for key_id in &persisted_keys {
            dirty.remove(key_id);
        }
        // 再扫描 dirty 集合：如果某 key 在 pools 里找不到，永久清掉（避免 stuck）；
        // 写失败但 pools 仍有 → 留在 dirty 等下次 tick 重试。
        let stale: Vec<String> = {
            let pools = self.pools.read().await;
            dirty
                .iter()
                .filter(|key_id| {
                    !persisted_keys.contains(*key_id)
                        && !pools
                            .values()
                            .any(|pool| pool.iter().any(|k| &k.key_id == *key_id))
                })
                .cloned()
                .collect()
        };
        for key_id in &stale {
            dirty.remove(key_id);
            log::debug!("[KeyRing] dropped stale dirty key={key_id} (no longer in pool)");
        }
        Ok(persisted_keys.len())
    }

    /// 清扫所有「cooldown 已过期」的 key——把它们拉回「完全可用」状态：
    ///   - `cooldown_until = 0`
    ///   - `failure_count = 0`
    ///   - `last_error = None`
    ///
    /// 设计原因：之前 mark_rate_limited 只设 cooldown + failure_count += 1，
    /// 没有「cooldown 过期就清零」的清扫。后果是：
    ///   - 一把 key 被 rate-limit 一次（failure_count=1）后冷却 5 分钟；
    ///   - 5 分钟后 cursor 回到这把 key 时，UI 仍显示「已失败 1/5 次」，
    ///     即便这把 key 现在完全可用——给用户"这把 key 一直有点问题"的错觉。
    ///   - 极端情况下，一把 key 累加到 4 次失败就冷却；冷却结束后仍显示
    ///     4/5；下一次失败直接到 5/5 触发自动停用。中间没有给"诚实的恢复期"。
    ///
    /// 现在：cooldown 过期的那一刹那就重置，行为是「5 次连续失败才停用」，
    /// 而不是「跨多个 cooldown 累计」。这把 key 标记为 dirty，下一次
    /// `flush_dirty` 会把重置写回 DB。
    ///
    /// 由 30s flush tick 在 `flush_dirty` 之前调用——保证最坏 30s 内
    /// 完成清扫（用户感知"刚恢复"≈立即）。
    pub async fn reap_expired_cooldowns(&self) -> usize {
        let now = chrono::Utc::now().timestamp();
        let mut to_reset: Vec<String> = Vec::new();

        {
            let pools = self.pools.read().await;
            for pool in pools.values() {
                for k in pool {
                    // cooldown_until > 0 意味着曾经设过；== now 已经过。
                    // 顺手清掉 last_error。
                    if k.cooldown_until > 0 && k.cooldown_until <= now && k.enabled {
                        to_reset.push(k.key_id.clone());
                    }
                }
            }
        }

        if to_reset.is_empty() {
            return 0;
        }

        {
            let mut pools = self.pools.write().await;
            for pool in pools.values_mut() {
                for k in pool.iter_mut() {
                    if to_reset.contains(&k.key_id) {
                        k.cooldown_until = 0;
                        k.failure_count = 0;
                        k.last_error = None;
                    }
                }
            }
        }

        // 标 dirty，下一次 flush_dirty 会写回 DB
        let mut dirty = self.dirty.write().await;
        for key_id in &to_reset {
            dirty.insert(key_id.clone());
        }

        log::debug!(
            "[KeyRing] reaped {} expired-cooldown keys (reset failure_count=0)",
            to_reset.len()
        );
        to_reset.len()
    }

    /// 重置某 provider 的 round-robin 游标到 active key 的位置。
    ///
    /// 用途：
    /// - 用户切换 active key 后，cursor 必须落到新 active 上，
    ///   否则下次 next_key 会跳过它（旧实现固定 reset 到 0 也有同样问题）
    /// - `reload_provider` 在 add/delete key 后也调它，保证 cursor 不越界
    /// 找不到 active key 时退回 sort_index=0（与 `active_key()` 一致）。
    pub async fn reset_cursor(&self, provider_id: &str, app_type: &AppType) {
        let key = (provider_id.to_string(), app_type.as_str().to_string());
        let pools = self.pools.read().await;
        let active_idx = pools
            .get(&key)
            .and_then(|pool| pool.iter().position(|k| k.is_active))
            .unwrap_or(0);
        drop(pools);
        let mut cursors = self.cursors.write().await;
        cursors.insert(key, active_idx);
    }

    /// 失效某 provider 的池——通常在 user 删除 provider / 删除所有 key 后调用。
    pub async fn invalidate_provider(&self, provider_id: &str, app_type: &AppType) {
        let key = (provider_id.to_string(), app_type.as_str().to_string());
        let mut pools = self.pools.write().await;
        let mut cursors = self.cursors.write().await;
        let mut locks = self.selection_locks.write().await;
        pools.remove(&key);
        cursors.remove(&key);
        locks.remove(&key);
    }

    /// 给前端 / 日志展示用：返回某 provider 当前池的快照。
    pub async fn snapshot(&self, provider_id: &str, app_type: &AppType) -> Vec<KeyState> {
        let key = (provider_id.to_string(), app_type.as_str().to_string());
        let pools = self.pools.read().await;
        pools.get(&key).cloned().unwrap_or_default()
    }

    /// 取当前应该用的 key（不推进 cursor）。
    ///
    /// 优先 `is_active=1` 的 key；没有则取 `sort_index=0` 的那把；都没有返回 None。
    /// 用途：forwarder 在进入 per-provider 循环时初始化 `current_key_id`，
    /// 让第一次 429 能正确 mark 当前 active key（Blocker #3）。
    pub async fn active_key(
        &self,
        provider_id: &str,
        app_type: &AppType,
    ) -> Option<KeyState> {
        let key = (provider_id.to_string(), app_type.as_str().to_string());
        let pools = self.pools.read().await;
        let pool = pools.get(&key)?;
        // 1. is_active=1
        if let Some(k) = pool.iter().find(|k| k.is_active).cloned() {
            return Some(k);
        }
        // 2. fallback：sort_index ASC 的第一把
        pool.iter().min_by_key(|k| k.sort_index).cloned()
    }

    /// apply_update 内部辅助：合并 mark_rate_limited / mark_success 共有的
    /// 内存改动 + dirty 标记逻辑。
    ///
    /// `cooldown_secs_from_now` 是相对秒数（0 = 立即可用，30 = 30s 后）——
    /// 函数内部加 `now` 转成绝对 unix 时间戳写入行。
    /// 这样 mark_rate_limited 调用方语义直观（"key 至少冷却 30s"），而不必关心
    /// "now 到底是哪一秒"。
    async fn apply_update(
        &self,
        key_id: &str,
        cooldown_secs_from_now: i64,
        bump_failure: impl Fn(i64) -> i64,
        last_error: Option<String>,
    ) -> KeyStateUpdate {
        let now = chrono::Utc::now().timestamp();
        let cooldown_until = if cooldown_secs_from_now > 0 {
            now + cooldown_secs_from_now
        } else {
            0
        };

        // 锁顺序：pools (write) → dirty (write) —— 与 mark_failure 一致，
        // 与 flush_dirty 反向（flush_dirty 持 dirty 后拿 pools）；只要不在
        // 一个方法内"同时持有 pools + dirty 进行长操作"，就不会死锁。
        let mut found = None;
        {
            let mut pools = self.pools.write().await;
            for pool in pools.values_mut() {
                if let Some(k) = pool.iter_mut().find(|k| k.key_id == key_id) {
                    k.cooldown_until = cooldown_until;
                    k.failure_count = bump_failure(k.failure_count);
                    k.last_used_at = Some(now);
                    if last_error.is_some() {
                        k.last_error = last_error;
                    } else if cooldown_secs_from_now == 0 {
                        // success path：清空 last_error
                        k.last_error = None;
                    }
                    found = Some(KeyStateUpdate {
                        key_id: k.key_id.clone(),
                        cooldown_until: k.cooldown_until,
                        failure_count: k.failure_count,
                    });
                    break;
                }
            }
        } // pools 锁释放
        if found.is_some() {
            let mut dirty = self.dirty.write().await;
            dirty.insert(key_id.to_string());
        }
        found.unwrap_or(KeyStateUpdate {
            key_id: key_id.to_string(),
            cooldown_until,
            failure_count: 0,
        })
    }
}

/// mark_rate_limited / apply_update 的返回值——记录刚刚应用的变更，
/// 便于调用方（forwarder）做日志。
#[derive(Debug, Clone)]
pub struct KeyStateUpdate {
    pub key_id: String,
    pub cooldown_until: i64,
    pub failure_count: i64,
}

/// 同步 helper：跨所有 (provider_id, app_type) 对收集 ProviderApiKey 行。
/// 用 sync API（lock_conn!）以匹配 KeyRing 启动时无 runtime 的场景。
fn collect_all_provider_keys(
    db: &Database,
) -> Result<Vec<(String, String, ProviderApiKey)>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    // 用 list_api_keys 一家一家取——简单且不引入新 SQL。
    // 先查所有 (provider_id, app_type) pair：用 providers 表 + GROUP BY。
    let mut stmt = conn
        .prepare("SELECT DISTINCT id, app_type FROM providers")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let pairs: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;
    drop(stmt);
    drop(conn);

    let mut out = Vec::new();
    for (pid, app) in pairs {
        // 跳过 OAuth-managed provider——它们没有静态 key 池。
        // 检测：meta.providerType in {"github_copilot","codex_oauth"}
        let conn = crate::database::lock_conn!(db.conn);
        let provider_type: Option<String> = conn
            .query_row(
                "SELECT json_extract(meta, '$.providerType') FROM providers
                 WHERE id = ?1 AND app_type = ?2",
                rusqlite::params![&pid, &app],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        drop(conn);
        if matches!(
            provider_type.as_deref(),
            Some("github_copilot") | Some("codex_oauth")
        ) {
            continue;
        }
        let keys = db.list_api_keys(&pid, &app)?;
        for k in keys {
            out.push((pid.clone(), app.clone(), k));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_key(id: &str, sort_index: i64, enabled: bool) -> ProviderApiKey {
        let now = chrono::Utc::now().timestamp();
        ProviderApiKey {
            id: id.to_string(),
            provider_id: "p1".to_string(),
            app_type: "claude".to_string(),
            label: id.to_string(),
            api_key: format!("sk-{id}"),
            tags: vec![],
            notes: String::new(),
            enabled,
            sort_index,
            is_active: sort_index == 0,
            cooldown_until: 0,
            failure_count: 0,
            last_used_at: None,
            last_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn seed_provider(db: &Database, pid: &str, app: &str) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR IGNORE INTO providers (id, app_type, name, settings_config)
             VALUES (?1, ?2, ?3, '{}')",
            rusqlite::params![pid, app, format!("P-{pid}")],
        )
        .expect("seed provider");
        Ok(())
    }

    #[tokio::test]
    async fn next_key_returns_none_when_pool_empty() {
        let ring = KeyRing::new();
        assert!(ring
            .next_key("p1", &AppType::Claude)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn next_key_returns_first_when_pool_has_one() {
        let ring = KeyRing::new();
        let k = make_key("only", 0, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(("p1".to_string(), "claude".to_string()), vec![KeyState::from_db_row(&k)]);
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        let picked = ring.next_key("p1", &AppType::Claude).await.expect("some");
        assert_eq!(picked.key_id, "only");
    }

    /// Round-robin 行为：cursor 在命中时推进到下一把，下次 next_key
    /// 必然落到不同位置——避免 sticky 实现下「同两把 key 之间反复」
    /// 的 ping-pong。所有 key 在正常请求路径上都会被均匀使用。
    #[tokio::test]
    async fn next_key_is_round_robin() {
        let ring = KeyRing::new();
        let k0 = make_key("k0", 0, true);
        let k1 = make_key("k1", 1, true);
        let k2 = make_key("k2", 2, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![
                    KeyState::from_db_row(&k0),
                    KeyState::from_db_row(&k1),
                    KeyState::from_db_row(&k2),
                ],
            );
            ring.cursors
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }

        // 连选四次，cursor 应逐次推进：k0 → k1 → k2 → k0（绕回）
        let p0 = ring.next_key("p1", &AppType::Claude).await.unwrap();
        let p1 = ring.next_key("p1", &AppType::Claude).await.unwrap();
        let p2 = ring.next_key("p1", &AppType::Claude).await.unwrap();
        let p3 = ring.next_key("p1", &AppType::Claude).await.unwrap();
        assert_eq!(p0.key_id, "k0");
        assert_eq!(p1.key_id, "k1", "round-robin: 命中后 cursor 应推进");
        assert_eq!(p2.key_id, "k2", "round-robin: 命中后 cursor 应推进");
        assert_eq!(p3.key_id, "k0", "round-robin: 绕回池头");
    }

    /// cursor 初始位置 = active key 位置（reload_from_db / reset_cursor 的契约）。
    /// 用户切到 k2 为 active 后，next_key 应直接落到 k2。
    #[tokio::test]
    async fn next_key_starts_at_active() {
        let ring = KeyRing::new();
        let mut k0 = make_key("k0", 0, true);
        k0.is_active = false;
        let mut k1 = make_key("k1", 1, true);
        k1.is_active = false;
        let mut k2 = make_key("k2", 2, true);
        k2.is_active = true; // k2 是 active
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![
                    KeyState::from_db_row(&k0),
                    KeyState::from_db_row(&k1),
                    KeyState::from_db_row(&k2),
                ],
            );
            ring.cursors
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        // 模拟 reload_from_db / 切换 active key 之后：把 cursor 拨到 active 位置
        ring.reset_cursor("p1", &AppType::Claude).await;

        let p = ring.next_key("p1", &AppType::Claude).await.unwrap();
        assert_eq!(p.key_id, "k2", "cursor 应指向 active key (k2)");
    }

    #[tokio::test]
    async fn next_key_skips_disabled() {
        let ring = KeyRing::new();
        let k0 = make_key("k0", 0, false); // disabled
        let k1 = make_key("k1", 1, true);
        let k2 = make_key("k2", 2, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![
                    KeyState::from_db_row(&k0),
                    KeyState::from_db_row(&k1),
                    KeyState::from_db_row(&k2),
                ],
            );
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }

        // cursor=0 命中 k0（disabled）→ 跳过到 k1
        let p = ring.next_key("p1", &AppType::Claude).await.expect("some");
        assert_eq!(p.key_id, "k1");
    }

    #[tokio::test]
    async fn next_key_skips_cooling() {
        let ring = KeyRing::new();
        let mut k0 = make_key("k0", 0, true);
        k0.cooldown_until = chrono::Utc::now().timestamp() + 3600; // 冷却中
        let k1 = make_key("k1", 1, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![KeyState::from_db_row(&k0), KeyState::from_db_row(&k1)],
            );
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        // cursor=0 命中 k0（冷却）→ 跳过到 k1
        let p = ring.next_key("p1", &AppType::Claude).await.expect("some");
        assert_eq!(p.key_id, "k1");
    }

    #[tokio::test]
    async fn next_key_returns_none_when_all_disabled_or_cooling() {
        let ring = KeyRing::new();
        let mut k0 = make_key("k0", 0, false);
        k0.cooldown_until = chrono::Utc::now().timestamp() + 3600;
        let k1 = make_key("k1", 1, false);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![KeyState::from_db_row(&k0), KeyState::from_db_row(&k1)],
            );
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        assert!(ring.next_key("p1", &AppType::Claude).await.is_none());
    }

    #[tokio::test]
    async fn mark_rate_limited_sets_min_cooldown_when_retry_after_missing() {
        let ring = KeyRing::new();
        let k = make_key("k0", 0, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(("p1".to_string(), "claude".to_string()), vec![KeyState::from_db_row(&k)]);
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        let update = ring.mark_rate_limited("k0", None).await;
        let now = chrono::Utc::now().timestamp();
        assert!(
            update.cooldown_until >= now + MIN_COOLDOWN_SECS - 1,
            "min cooldown = {}s, got cooldown {} (now {})",
            MIN_COOLDOWN_SECS,
            update.cooldown_until,
            now
        );
        assert_eq!(update.failure_count, 1);
    }

    #[tokio::test]
    async fn mark_rate_limited_respects_retry_after() {
        let ring = KeyRing::new();
        let k = make_key("k0", 0, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(("p1".to_string(), "claude".to_string()), vec![KeyState::from_db_row(&k)]);
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        let update = ring.mark_rate_limited("k0", Some(120)).await;
        let now = chrono::Utc::now().timestamp();
        assert!(update.cooldown_until >= now + 119);
        assert!(update.cooldown_until <= now + 121);
    }

    /// Proactive rotation：90-99% 设 cooldown 到 reset_at；>=100% 设
    /// cooldown 到 max(reset_at, now + 30min)；<90% 不动。
    #[tokio::test]
    async fn mark_usage_high_sets_cooldown_by_threshold() {
        let ring = KeyRing::new();
        let k = make_key("k0", 0, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(("p1".to_string(), "claude".to_string()), vec![KeyState::from_db_row(&k)]);
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        let now = chrono::Utc::now().timestamp();
        let reset_at = now + 3600; // 5h 重置

        // 1) < 90%：不动
        let triggered = ring.mark_usage_high("k0", 80.0, reset_at).await;
        assert!(!triggered, "80% 不应触发 cooldown");
        {
            let pools = ring.pools.read().await;
            let k0 = &pools[&("p1".to_string(), "claude".to_string())][0];
            assert_eq!(k0.cooldown_until, 0, "80% 时 cooldown_until 应保持 0");
        }

        // 2) 90% ≤ p < 100%：cooldown 到 reset_at
        let triggered = ring.mark_usage_high("k0", 95.0, reset_at).await;
        assert!(triggered, "95% 应触发 cooldown");
        {
            let pools = ring.pools.read().await;
            let k0 = &pools[&("p1".to_string(), "claude".to_string())][0];
            assert_eq!(k0.cooldown_until, reset_at, "95% 时 cooldown = reset_at");
            assert_eq!(k0.failure_count, 0, "proactive rotation 不动 failure_count");
        }

        // 3) ≥ 100%：cooldown 到 max(reset_at, now + 30min)
        //    用一个 1h 后的 reset_at 来验证 30min 缓冲占主导
        let soon_reset = now + 3600;
        let triggered = ring.mark_usage_high("k0", 100.0, soon_reset).await;
        assert!(triggered, "100% 应触发 cooldown");
        {
            let pools = ring.pools.read().await;
            let k0 = &pools[&("p1".to_string(), "claude".to_string())][0];
            // 30 min > 1h？不，30min < 1h → max = soon_reset (1h)
            // 改用 10min 后的 reset 来测 30min 缓冲
            assert_eq!(k0.cooldown_until, soon_reset);
        }
        let very_soon_reset = now + 600; // 10 min
        ring.mark_usage_high("k0", 100.0, very_soon_reset).await;
        {
            let pools = ring.pools.read().await;
            let k0 = &pools[&("p1".to_string(), "claude".to_string())][0];
            // 30 min 缓冲 > 10 min reset → 应取 now + 30min
            let expected_min = now + 30 * 60;
            assert!(k0.cooldown_until >= expected_min - 1);
        }
    }

    /// Proactive rotation 触发的 cooldown 与 429 路径的 mark_rate_limited
    /// 行为一致：让 next_key 跳过这把 key。
    #[tokio::test]
    async fn mark_usage_high_makes_next_key_skip() {
        let ring = KeyRing::new();
        let k0 = make_key("k0", 0, true);
        let k1 = make_key("k1", 1, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![KeyState::from_db_row(&k0), KeyState::from_db_row(&k1)],
            );
            ring.cursors
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        // 标记 k0 接近上限
        let now = chrono::Utc::now().timestamp();
        let reset_at = now + 3600;
        ring.mark_usage_high("k0", 95.0, reset_at).await;

        // 选 key：cursor=0，k0 在 cooldown → 跳到 k1，cursor 推进到 1
        let p = ring.next_key("p1", &AppType::Claude).await.expect("k1 available");
        assert_eq!(p.key_id, "k1", "k0 应被跳过，next_key 应返回 k1");
    }

    #[tokio::test]
    async fn active_key_prefers_is_active_then_sort_index() {
        // 场景：forwarder 进入 per-provider 循环时调用 active_key 拿到当前应使用的 key，
        // 不推进 cursor。Blocker #3 修复要求这里返回非 None（否则 current_key_id 永远 None）。
        let ring = KeyRing::new();

        // 场景 1: 没有 active key → fallback sort_index ASC 第一把
        let k0 = make_key("k0", 0, true);
        let mut k1 = make_key("k1", 1, true);
        k1.is_active = true;
        let mut k2 = make_key("k2", 2, true);
        k2.is_active = true;
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![
                    KeyState::from_db_row(&k0),
                    KeyState::from_db_row(&k1),
                    KeyState::from_db_row(&k2),
                ],
            );
        }
        // 排序后 k0.sort_index=0, k1.sort_index=1, k2.sort_index=2——即使 k1/k2 is_active，
        // 优先取 sort_index 最低的。
        let active = ring.active_key("p1", &AppType::Claude).await.expect("some");
        assert_eq!(active.key_id, "k0", "active_key 在没有 is_active 时取 sort_index=0");

        // 场景 2: 有 is_active=1 → 优先取
        let mut k0_active = k0.clone();
        k0_active.is_active = true;
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![
                    KeyState::from_db_row(&k0_active),
                    KeyState::from_db_row(&k1),
                    KeyState::from_db_row(&k2),
                ],
            );
        }
        let active = ring.active_key("p1", &AppType::Claude).await.expect("some");
        assert_eq!(active.key_id, "k0", "is_active=1 wins over sort_index");

        // 场景 3: 池为空 → None
        ring.invalidate_provider("p2", &AppType::Claude).await;
        assert!(ring.active_key("p2", &AppType::Claude).await.is_none());
    }

    #[tokio::test]
    async fn active_key_does_not_advance_cursor() {
        // 不推进 cursor——调用 active_key 后 next_key 仍从原 cursor 位置开始。
        // 这是它与 next_key 的关键区别：Blocker #3 修复要求只在 forwarder 入口读一次，
        // 不应该 bump 后续轮换的起点。
        let ring = KeyRing::new();
        let k0 = make_key("k0", 0, true);
        let k1 = make_key("k1", 1, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![KeyState::from_db_row(&k0), KeyState::from_db_row(&k1)],
            );
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        // active_key 不应 bump cursor
        let _ = ring.active_key("p1", &AppType::Claude).await;
        let _ = ring.active_key("p1", &AppType::Claude).await;
        let _ = ring.active_key("p1", &AppType::Claude).await;
        // 第一次 next_key 仍然从 cursor=0 (k0) 开始
        let first = ring.next_key("p1", &AppType::Claude).await.unwrap();
        assert_eq!(first.key_id, "k0", "active_key 不能推进 cursor");
    }

    #[tokio::test]
    async fn mark_success_clears_cooldown_and_failure() {
        let ring = KeyRing::new();
        let mut k = make_key("k0", 0, true);
        k.cooldown_until = chrono::Utc::now().timestamp() + 3600;
        k.failure_count = 4;
        k.last_error = Some("rate limited".to_string());
        {
            let mut pools = ring.pools.write().await;
            pools.insert(("p1".to_string(), "claude".to_string()), vec![KeyState::from_db_row(&k)]);
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        ring.mark_success("k0").await;
        let snapshot = ring.snapshot("p1", &AppType::Claude).await;
        assert_eq!(snapshot[0].cooldown_until, 0);
        assert_eq!(snapshot[0].failure_count, 0);
        assert!(snapshot[0].last_error.is_none());
    }

    #[tokio::test]
    async fn mark_failure_keeps_key_enabled_after_threshold() {
        let ring = KeyRing::new();
        let k = make_key("k0", 0, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(("p1".to_string(), "claude".to_string()), vec![KeyState::from_db_row(&k)]);
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        for i in 1..=AUTO_DISABLE_FAILURE_THRESHOLD {
            ring.mark_failure("k0", "timeout").await;
            let snap = ring.snapshot("p1", &AppType::Claude).await;
            assert_eq!(snap[0].failure_count, i);
        }
        // 关键不变量（新行为）：阈值之后 key 仍 enabled，仍可被 next_key 选中
        let snap = ring.snapshot("p1", &AppType::Claude).await;
        assert!(
            snap[0].enabled,
            "mark_failure 不再自动 disable，key 应保持 enabled"
        );
        let picked = ring
            .next_key("p1", &AppType::Claude)
            .await
            .expect("key still selectable after 5 failures");
        assert_eq!(picked.key_id, "k0");
        assert_eq!(picked.failure_count, AUTO_DISABLE_FAILURE_THRESHOLD);
    }

    /// mark_failure 5 次后 cursor 由「命中时推进」自然流转——失败计数归 key
    /// 不动 cursor，但 next_key 命中 k0 后会把它推进到 1。
    /// 验证「只统计、不绕开 cooldown」语义：失败计数归 key，next_key 仍然
    /// 先试 k0（除非 k0 进 cooldown），并把 cursor 推到 k1。
    #[tokio::test]
    async fn mark_failure_does_not_advance_cursor() {
        let ring = KeyRing::new();
        let k0 = make_key("k0", 0, true);
        let k1 = make_key("k1", 1, true);
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![KeyState::from_db_row(&k0), KeyState::from_db_row(&k1)],
            );
            ring.cursors
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        // 触发 5 次失败：cursor 仍 0（mark_failure 不动 cursor）
        for _ in 0..AUTO_DISABLE_FAILURE_THRESHOLD {
            ring.mark_failure("k0", "timeout").await;
        }
        // next_key 仍返回 k0（cursor=0, k0 enabled & not cooling）
        // round-robin：这次返回后 cursor 推进到 1
        let p = ring.next_key("p1", &AppType::Claude).await.expect("k0 still available");
        assert_eq!(
            p.key_id, "k0",
            "5 次失败后 k0 仍 enabled 未冷却，next_key 应返回 k0"
        );

        // 现在 mark_rate_limited k0 → k0 冷却 → next_key 应跳到 k1
        ring.mark_rate_limited("k0", Some(60)).await;
        let p = ring.next_key("p1", &AppType::Claude).await.expect("k1 available");
        assert_eq!(p.key_id, "k1", "冷却后 next_key 应跳到 k1");
    }

    #[tokio::test]
    async fn flush_dirty_writes_only_dirty_keys() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        let k = make_key("k0", 0, true);
        db.insert_api_key(&k)?;

        let ring = KeyRing::new();
        ring.reload_from_db(&db).await?;
        ring.mark_rate_limited("k0", Some(60)).await;

        let written = ring.flush_dirty(&db).await?;
        assert_eq!(written, 1);

        // 重读 DB 应反映冷却
        let fresh = db.get_api_key("k0")?.expect("k0 exists");
        assert!(fresh.cooldown_until > chrono::Utc::now().timestamp());

        // dirty 集合应为空
        let written_again = ring.flush_dirty(&db).await?;
        assert_eq!(written_again, 0);
        Ok(())
    }

    /// 防回归：flush_dirty 一个 key 失败时不影响其他 key 的持久化（Review 关心点）。
    /// 模拟"中途失败一个 key 但其它继续"的真实 partial-failure 场景：
    /// 用户删除了某 key（DB + in-memory 都去掉），但 dirty 集合里仍残留该 key。
    /// 下次 flush 时该 key 找不到，stale 逻辑应把它从 dirty 中清掉。
    #[tokio::test]
    async fn flush_dirty_skips_missing_keys_without_aborting() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&make_key("k0", 0, true))?;
        db.insert_api_key(&make_key("k1", 1, true))?;

        let ring = std::sync::Arc::new(KeyRing::new());
        ring.reload_from_db(&db).await?;
        ring.mark_rate_limited("k0", Some(60)).await;
        ring.mark_rate_limited("k1", Some(60)).await;

        // 模拟"k0 被删了"：同时从 DB 删 + 从 in-memory pool 删（这是
        // cmd_remove_api_key 路径的实际行为：DB delete + KeyRing hot-reload）。
        db.delete_api_key("k0")?;
        ring.reload_provider(&db, "p1", "claude").await?;

        let written = ring.flush_dirty(&db).await?;
        // 至少 k1 应被写
        assert!(written >= 1, "k1 should be persisted; got {written}");

        // dirty 集合应被清空：k0（pool 找不到）被 stale 逻辑移除；
        // k1（写成功）也被移走。
        let dirty_size = {
            let d = ring.dirty.read().await;
            d.len()
        };
        assert_eq!(dirty_size, 0, "dirty must be drained after successful flush");
        Ok(())
    }

    #[tokio::test]
    async fn all_keys_cooling_returns_none_for_rotation() -> Result<(), AppError> {
        // forwarder 轮换路径在 KeyRing.next_key 返回 None 时
        // break 出 'rotate 进入 AllKeysRateLimited 分支。
        // 这里验证 KeyRing 的契约：所有 key 都 cooling 时 next_key = None。
        let ring = KeyRing::new();
        let now = chrono::Utc::now().timestamp();
        let mut k0 = make_key("k0", 0, true);
        k0.cooldown_until = now + 3600;
        let mut k1 = make_key("k1", 1, true);
        k1.cooldown_until = now + 3600;
        {
            let mut pools = ring.pools.write().await;
            pools.insert(
                ("p1".to_string(), "claude".to_string()),
                vec![KeyState::from_db_row(&k0), KeyState::from_db_row(&k1)],
            );
            ring.cursors.write().await.insert(("p1".to_string(), "claude".to_string()), 0);
            ring.selection_locks
                .write()
                .await
                .insert(("p1".to_string(), "claude".to_string()), Arc::new(Mutex::new(())));
        }
        // 池中无任何可用 key
        assert!(ring.next_key("p1", &AppType::Claude).await.is_none());
        Ok(())
    }

    /// 防回归：并发 mark_rate_limited / mark_failure / flush_dirty / next_key 不能死锁。
    /// 修复前 mark_failure 先拿 dirty 后拿 pools，flush_dirty 反向——
    /// 两个 await 同时撞上时极易死锁。修复后两边锁顺序一致 (pools → dirty)。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_mark_and_flush_does_not_deadlock() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        let k = make_key("k0", 0, true);
        db.insert_api_key(&k)?;

        let ring = std::sync::Arc::new(KeyRing::new());
        ring.reload_from_db(&db).await?;

        // 用 tokio::time::timeout 包裹：若发生死锁，测试会在 2s 内 panic。
        let work = async {
            // 5 个 forwarder-style 任务并发 mark
            let mut handles = Vec::new();
            for _ in 0..5 {
                let r = ring.clone();
                handles.push(tokio::spawn(async move {
                    for _ in 0..10 {
                        r.mark_rate_limited("k0", Some(30)).await;
                        r.mark_success("k0").await;
                    }
                }));
            }
            // 1 个 flush tick 任务并发跑
            let r2 = ring.clone();
            let flush_handle = tokio::spawn(async move {
                for _ in 0..5 {
                    let _ = r2.flush_dirty(&db).await;
                }
            });
            // 1 个 next_key 任务并发跑（模拟请求选 key）
            let r3 = ring.clone();
            let next_handle = tokio::spawn(async move {
                for _ in 0..10 {
                    let _ = r3.next_key("p1", &AppType::Claude).await;
                }
            });
            for h in handles {
                h.await.unwrap();
            }
            flush_handle.await.unwrap();
            next_handle.await.unwrap();
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), work)
            .await
            .expect("concurrent mark/flush/next must not deadlock");
        Ok(())
    }

    #[tokio::test]
    async fn reload_from_db_loads_multiple_providers() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        seed_provider(&db, "p2", "claude")?;

        for (pid, key) in [("p1", "k1"), ("p1", "k2"), ("p2", "k3")] {
            let mut k = make_key(key, 0, true);
            k.provider_id = pid.to_string();
            db.insert_api_key(&k)?;
        }

        let ring = KeyRing::new();
        ring.reload_from_db(&db).await?;

        assert_eq!(ring.snapshot("p1", &AppType::Claude).await.len(), 2);
        assert_eq!(ring.snapshot("p2", &AppType::Claude).await.len(), 1);
        assert!(ring.next_key("p2", &AppType::Claude).await.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn reload_from_db_skips_oauth_providers() -> Result<(), AppError> {
        let db = Database::memory()?;
        // OAuth provider：虽然也插入 key，KeyRing 应跳过
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            r#"INSERT INTO providers (id, app_type, name, settings_config, meta)
               VALUES ('copilot', 'claude', 'Copilot', '{}', '{"providerType":"github_copilot"}')"#,
            [],
        )?;
        drop(conn);
        let mut k = make_key("ck", 0, true);
        k.provider_id = "copilot".to_string();
        db.insert_api_key(&k)?;

        let ring = KeyRing::new();
        ring.reload_from_db(&db).await?;
        assert_eq!(ring.snapshot("copilot", &AppType::Claude).await.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn invalidate_provider_clears_pool() -> Result<(), AppError> {
        let db = Database::memory()?;
        seed_provider(&db, "p1", "claude")?;
        db.insert_api_key(&make_key("k0", 0, true))?;

        let ring = KeyRing::new();
        ring.reload_from_db(&db).await?;
        assert_eq!(ring.snapshot("p1", &AppType::Claude).await.len(), 1);

        ring.invalidate_provider("p1", &AppType::Claude).await;
        assert_eq!(ring.snapshot("p1", &AppType::Claude).await.len(), 0);
        Ok(())
    }
}

