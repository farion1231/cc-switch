//! 订阅配额接力（issue #7493）
//!
//! 编程订阅按「滚动时间窗口 + 周期内 token 上限」计量（Claude Code 的 5 小时窗口、
//! Kimi / GLM / OpenCode Go 的周期额度等）。第三方订阅没有远程额度端点，只能用本地
//! 用量日志记账：provider meta 配置 [`crate::provider::QuotaCycleConfig`]（`quotaCycle`
//! 字段）后，本模块维护 per-(provider, app) 的内存窗口状态，转发循环据此把额度用尽
//! 的供应商从故障转移链里跳过。
//!
//! 设计约束（与熔断器同款模式，见 `circuit_breaker` / `provider_router`）：
//! - 状态纯内存，不落库、不迁移 schema；
//! - 不跑后台计时器，窗口过期在判定时惰性重置；
//! - `used_tokens` 预填复用本地用量日志（`proxy_request_logs` + `usage_daily_rollups`），
//!   口径与 Usage 页 total 一致（`real_total_tokens_sql`），保证数字对得上 Usage 页；
//! - 只有「本地路由 + usage logging 开启」才有记账源：usage logging 关闭时请求不写
//!   `proxy_request_logs`，重启后窗口只能从 0 起算（周期内重启会低估用量）。
//!
//! 记账语义：[`QuotaRelay::record_usage`] 记的是**增量**（响应解析出的 tokens 总量），
//! 窗口建立/重置时的**存量**由 [`seed_used_tokens_from_logs`] 从日志聚合。两者衔接：
//! 存量快照取自建窗瞬间，之后的增量只进内存、不重复聚合，直到窗口过期重新预填。

use crate::database::Database;
use crate::provider::{ProviderMeta, QuotaCycleConfig};
use crate::services::sql_helpers::real_total_tokens_sql;
use crate::services::usage_stats::effective_usage_log_filter;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::log_codes::qta as log_qta;

/// 单个 (provider, app) 的周期窗口状态
#[derive(Debug, Clone, Copy)]
struct QuotaWindowState {
    /// 当前窗口起点（Unix 秒）
    window_started_at: i64,
    /// 窗口内已消耗 tokens（建窗时预填 + 之后 record_usage 增量）
    used_tokens: i64,
}

/// 订阅配额接力器
///
/// 挂在 [`crate::proxy::provider_router::ProviderRouter`] 上共享（与熔断器同层），
/// 转发循环（`forwarder`）读、用量记录点（`response_processor`）写。
pub struct QuotaRelay {
    db: Arc<Database>,
    /// key 格式: "app_type:provider_id"（与熔断器的 circuit_key 同款）
    states: RwLock<HashMap<String, QuotaWindowState>>,
}

impl QuotaRelay {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            states: RwLock::new(HashMap::new()),
        }
    }

    /// 记账：把一次请求的真实消耗 tokens 计入该供应商的当前周期窗口。
    ///
    /// 供应商未配置 `quotaCycle` 时是 no-op（此时才查一次 meta；已建窗的快路径
    /// 不查库）。窗口不存在时建窗并从用量日志预填存量。
    pub async fn record_usage(&self, app_type: &str, provider_id: &str, tokens: i64) {
        if tokens <= 0 {
            return;
        }
        let key = quota_key(app_type, provider_id);

        // 快路径：窗口已在，直接累加（过期留给 is_exhausted 惰性重置修正归属）
        {
            let mut states = self.states.write().await;
            if let Some(state) = states.get_mut(&key) {
                state.used_tokens = state.used_tokens.saturating_add(tokens);
                return;
            }
        }

        // 无窗口：仅当供应商配置了周期额度才建窗（未配置的不为记账查库/建状态）
        let Some(config) = load_quota_cycle_config(&self.db, app_type, provider_id) else {
            return;
        };
        if config.token_cap <= 0 || config.duration_hours <= 0 {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let window_start = config.window_start_at(now);
        let seeded = seed_used_tokens_from_logs(&self.db, app_type, provider_id, window_start);
        log::debug!(
            "[{app_type}] [{}] provider={provider_id} 建窗预填: start={window_start}, used={seeded} tokens",
            log_qta::WINDOW_SEEDED,
        );

        let mut states = self.states.write().await;
        let state = states.entry(key).or_insert(QuotaWindowState {
            window_started_at: window_start,
            used_tokens: seeded,
        });
        // 竞态兜底：拿不到写锁期间别的调用可能已建窗（其预填可能已含本条日志行），
        // 再加一次增量最坏情况是单请求级别的瞬时高估，方向安全（宁可早切不漏切）。
        state.used_tokens = state.used_tokens.saturating_add(tokens);
    }

    /// 判断供应商当前周期额度是否已用尽。
    ///
    /// `config` 由调用方从内存中的 `Provider.meta` 传入（转发循环手里就有，避免每个
    /// 请求再查库）。窗口不存在时建窗并预填；窗口过期时惰性重置并按新窗口重新预填
    /// —— 重置后的预填能把「窗口边界刚越过、日志还带着旧窗口归属」的异步落库行捞回来。
    pub async fn is_exhausted(
        &self,
        app_type: &str,
        provider_id: &str,
        config: &QuotaCycleConfig,
    ) -> bool {
        if config.token_cap <= 0 || config.duration_hours <= 0 {
            return false; // 非法配置不拦截流量
        }
        let key = quota_key(app_type, provider_id);
        let now = chrono::Utc::now().timestamp();
        let window_start = config.window_start_at(now);

        let used = {
            let mut states = self.states.write().await;
            match states.get_mut(&key) {
                // 窗口仍有效：直接用内存增量
                Some(state) if now < state.window_started_at + config.duration_secs() => {
                    state.used_tokens
                }
                Some(state) => {
                    log::debug!(
                        "[{app_type}] [{}] provider={provider_id} 周期窗口过期，惰性重置: {} → {window_start}",
                        log_qta::WINDOW_RESET,
                        state.window_started_at,
                    );
                    let seeded =
                        seed_used_tokens_from_logs(&self.db, app_type, provider_id, window_start);
                    *state = QuotaWindowState {
                        window_started_at: window_start,
                        used_tokens: seeded,
                    };
                    seeded
                }
                None => {
                    let seeded =
                        seed_used_tokens_from_logs(&self.db, app_type, provider_id, window_start);
                    log::debug!(
                        "[{app_type}] [{}] provider={provider_id} 建窗预填: start={window_start}, used={seeded} tokens",
                        log_qta::WINDOW_SEEDED,
                    );
                    states.insert(
                        key.clone(),
                        QuotaWindowState {
                            window_started_at: window_start,
                            used_tokens: seeded,
                        },
                    );
                    seeded
                }
            }
        };

        used >= config.token_cap
    }

    /// 启动预填：把所有配置了 `quotaCycle` 的供应商的当前窗口用量从用量日志聚合进内存。
    ///
    /// 惰性路径（`is_exhausted` / `record_usage` 首次触达时建窗）已保证正确性，这里是
    /// 提前预热：代理启动时一次扫出，避免重启后的第一个请求才补账。失败只影响精度，
    /// 不阻断启动。
    pub async fn prefill_all(&self) {
        let configs = load_quota_cycle_configs(&self.db);
        if configs.is_empty() {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let mut states = self.states.write().await;
        let mut seeded_count = 0usize;
        for (app_type, provider_id, config) in &configs {
            if config.token_cap <= 0 || config.duration_hours <= 0 {
                continue;
            }
            let key = quota_key(app_type, provider_id);
            if let Some(state) = states.get(&key) {
                // 已有未过期窗口（理论上启动时不会有）不重复预填
                if now < state.window_started_at + config.duration_secs() {
                    continue;
                }
            }
            let window_start = config.window_start_at(now);
            let seeded = seed_used_tokens_from_logs(&self.db, app_type, provider_id, window_start);
            states.insert(
                key,
                QuotaWindowState {
                    window_started_at: window_start,
                    used_tokens: seeded,
                },
            );
            seeded_count += 1;
        }
        log::info!(
            "[{}] 已为 {seeded_count}/{} 个配置周期额度的供应商预填窗口用量",
            log_qta::PREFILL_DONE,
            configs.len(),
        );
    }
}

/// 状态 key：与熔断器的 circuit_key 同款格式
fn quota_key(app_type: &str, provider_id: &str) -> String {
    format!("{app_type}:{provider_id}")
}

/// 从用量日志聚合 `[window_started_at, now)` 的真实消耗 tokens（建窗/重置时的存量预填）。
///
/// 明细表（`proxy_request_logs`）与日汇总表（`usage_daily_rollups`）互补计数：
/// - 明细部分：`created_at >= window_start` 的行，带上与 Usage 页同款的跨源去重过滤；
/// - 汇总部分：本地日期**严格晚于**窗口起点当天的完整日。汇总日与明细日互斥
///   （rollup 与剪枝同事务完成，滚过的天明细已删），不会双算；窗口起点当天若已
///   被汇总（仅当窗口长于日志保留期时可能），当天窗口内的部分不计入——宁可低估
///   少切，不高估误切。
///
/// 任何查询失败都按 0 处理：预填失败只影响判断精度，不能阻断转发。
fn seed_used_tokens_from_logs(
    db: &Database,
    app_type: &str,
    provider_id: &str,
    window_started_at: i64,
) -> i64 {
    let real_total_detail = real_total_tokens_sql("l");
    let real_total_rollup = real_total_tokens_sql("r");
    let dedup_filter = effective_usage_log_filter("l");
    let sql = format!(
        "SELECT
            (SELECT COALESCE(SUM({real_total_detail}), 0) FROM proxy_request_logs l
              WHERE l.app_type = ?1 AND l.provider_id = ?2
                AND l.created_at >= ?3 AND {dedup_filter})
          +
            (SELECT COALESCE(SUM({real_total_rollup}), 0) FROM usage_daily_rollups r
              WHERE r.app_type = ?1 AND r.provider_id = ?2
                AND r.date > date(?3, 'unixepoch', 'localtime'))"
    );

    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            log::warn!("[Quota] 预填窗口用量时获取连接失败: {e}");
            return 0;
        }
    };
    conn.query_row(
        &sql,
        rusqlite::params![app_type, provider_id, window_started_at],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or_else(|e| {
        log::debug!("[Quota] 预填窗口用量查询失败（按 0 处理）: {e}");
        0
    })
}

/// 读取单个供应商 meta 里的 `quotaCycle` 配置
fn load_quota_cycle_config(
    db: &Database,
    app_type: &str,
    provider_id: &str,
) -> Option<QuotaCycleConfig> {
    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            log::warn!("[Quota] 读取配额配置时获取连接失败: {e}");
            return None;
        }
    };
    let meta_str: String = conn
        .query_row(
            "SELECT meta FROM providers WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
            |row| row.get(0),
        )
        .ok()?;
    let meta = serde_json::from_str::<ProviderMeta>(&meta_str).ok()?;
    meta.quota_cycle
}

/// 扫出所有配置了 `quotaCycle` 的供应商（启动预填用）
fn load_quota_cycle_configs(db: &Database) -> Vec<(String, String, QuotaCycleConfig)> {
    let conn = match db.conn.lock() {
        Ok(conn) => conn,
        Err(e) => {
            log::warn!("[Quota] 启动预填读取供应商配置失败: {e}");
            return Vec::new();
        }
    };
    // LIKE 预过滤：quotaCycle 是罕见配置，避免全表反序列化 meta
    let Ok(mut stmt) =
        conn.prepare("SELECT app_type, id, meta FROM providers WHERE meta LIKE '%quotaCycle%'")
    else {
        return Vec::new();
    };
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    });
    let mut out = Vec::new();
    if let Ok(rows) = rows {
        for (app_type, provider_id, meta_str) in rows.flatten() {
            let Ok(meta) = serde_json::from_str::<ProviderMeta>(&meta_str) else {
                continue;
            };
            if let Some(config) = meta.quota_cycle {
                out.push((app_type, provider_id, config));
            }
        }
    }
    out
}
