//! 使用统计服务
//!
//! 提供使用量数据的聚合查询功能

use crate::database::{lock_conn, Database};
use crate::error::AppError;
#[cfg(test)]
use crate::proxy::usage::calculator::ModelPricing;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};
use cc_switch_core::{CoreError, UsageScope, UsageService};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::str::FromStr;

/// 将桌面命令的离散筛选参数收敛成 Core 的统一作用域，参数含义与 Tauri 旧接口保持不变。
fn usage_scope(
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<&str>,
    provider_name: Option<&str>,
    model: Option<&str>,
) -> UsageScope {
    UsageScope {
        start_date,
        end_date,
        app_type: app_type.map(str::to_owned),
        provider_name: provider_name.map(str::to_owned),
        model: model.map(str::to_owned),
    }
}

/// 桌面仍沿用 `AppError` 序列化边界；超大响应保留稳定错误码，其余查询错误按数据库故障呈现。
pub(crate) fn map_core_usage_error(error: CoreError) -> AppError {
    match error {
        CoreError::PayloadTooLarge { .. } => {
            AppError::Message(format!("PAYLOAD_TOO_LARGE: {error}"))
        }
        _ => AppError::Database(error.to_string()),
    }
}

/// 桌面命令继续从本模块导出 DTO，但结构与序列化契约只由 Core 维护，避免本地/远程字段漂移。
pub use cc_switch_core::{
    DailyStats, LogFilters, ModelStats, PaginatedLogs, ProviderLimitStatus, ProviderStats,
    RequestLogDetail, UsageSummary, UsageSummaryByApp,
};
/// 把 26 列的查询结果映射为 `RequestLogDetail`。
///
/// 调用方的 SELECT **必须**按以下顺序返回 26 列：
/// `request_id, provider_id, provider_name, app_type, model, request_model,
///  cost_multiplier, input_tokens, output_tokens, cache_read_tokens,
///  cache_creation_tokens, input_cost_usd, output_cost_usd, cache_read_cost_usd,
///  cache_creation_cost_usd, total_cost_usd, is_streaming, latency_ms,
///  first_token_ms, duration_ms, status_code, error_message, created_at,
///  data_source, pricing_model, input_token_semantics`
///
/// 不需要 provider_name 时（如 backfill）SELECT `NULL AS provider_name` 占位即可。
fn row_to_request_log_detail(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestLogDetail> {
    Ok(RequestLogDetail {
        request_id: row.get(0)?,
        provider_id: row.get(1)?,
        provider_name: row.get(2)?,
        app_type: row.get(3)?,
        model: row.get(4)?,
        request_model: row.get(5)?,
        cost_multiplier: row
            .get::<_, Option<String>>(6)?
            .unwrap_or_else(|| "1".to_string()),
        input_tokens: row.get::<_, i64>(7)? as u32,
        output_tokens: row.get::<_, i64>(8)? as u32,
        cache_read_tokens: row.get::<_, i64>(9)? as u32,
        cache_creation_tokens: row.get::<_, i64>(10)? as u32,
        input_cost_usd: row.get(11)?,
        output_cost_usd: row.get(12)?,
        cache_read_cost_usd: row.get(13)?,
        cache_creation_cost_usd: row.get(14)?,
        total_cost_usd: row.get(15)?,
        is_streaming: row.get::<_, i64>(16)? != 0,
        latency_ms: row.get::<_, i64>(17)? as u64,
        first_token_ms: row.get::<_, Option<i64>>(18)?.map(|v| v as u64),
        duration_ms: row.get::<_, Option<i64>>(19)?.map(|v| v as u64),
        status_code: row.get::<_, i64>(20)? as u16,
        error_message: row.get(21)?,
        created_at: row.get(22)?,
        data_source: row.get(23)?,
        pricing_model: row.get(24)?,
        input_token_semantics: row.get::<_, i64>(25)?,
    })
}

pub(crate) const SESSION_PROXY_DEDUP_WINDOW_SECONDS: i64 = 10 * 60;

/// SQL 片段：把指定别名的 `data_source` 包成 COALESCE，NULL 视作 'proxy'。
///
/// 防御 schema v9 之前可能写入的 NULL data_source 行（见
/// `tests::create_legacy_nullable_logs_table`）。所有用到 data_source 的查询
/// 都应通过此 helper 生成片段，避免遗漏。
fn data_source_expr(log_alias: &str) -> String {
    format!("COALESCE({log_alias}.data_source, 'proxy')")
}

pub(crate) fn effective_usage_log_filter(log_alias: &str) -> String {
    let data_source = data_source_expr(log_alias);
    let proxy_data_source = data_source_expr("proxy_dedup");
    format!(
        "NOT (
            {data_source} IN ('session_log', 'codex_session', 'gemini_session', 'opencode_session')
            AND EXISTS (
                SELECT 1
                FROM proxy_request_logs proxy_dedup
                WHERE {proxy_data_source} = 'proxy'
                  AND proxy_dedup.app_type = {log_alias}.app_type
                  AND proxy_dedup.status_code >= 200
                  AND proxy_dedup.status_code < 300
                  AND proxy_dedup.input_tokens = {log_alias}.input_tokens
                  AND proxy_dedup.output_tokens = {log_alias}.output_tokens
                  AND proxy_dedup.cache_read_tokens = {log_alias}.cache_read_tokens
                  AND (
                      proxy_dedup.cache_creation_tokens = {log_alias}.cache_creation_tokens
                      OR (
                          {log_alias}.cache_creation_tokens = 0
                          AND {data_source} IN ('codex_session', 'gemini_session', 'opencode_session')
                      )
                  )
                  AND proxy_dedup.created_at BETWEEN
                      {log_alias}.created_at - {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                      AND {log_alias}.created_at + {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                  AND (
                      LOWER(proxy_dedup.model) = LOWER({log_alias}.model)
                      OR LOWER(proxy_dedup.model) = 'unknown'
                      OR LOWER({log_alias}.model) = 'unknown'
                  )
            )
        )"
    )
}

/// 跨源去重指纹键。
///
/// `cache_creation_tokens`：Codex/Gemini session 日志不暴露该字段，调用方传 0
/// 表示"未知"，匹配器会放行 proxy 侧任意 cache_creation_tokens 值。
#[derive(Debug, Clone, Copy)]
#[cfg(test)]
pub(crate) struct DedupKey<'a> {
    pub app_type: &'a str,
    pub model: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub created_at: i64,
}

/// session 日志写入前的统一去重判定。
///
/// 命中以下任一条件即跳过插入：① `request_id` 已存在；② 时间窗口内存在
/// 与 `key` 匹配的 proxy 日志（指纹去重）。
#[cfg(test)]
pub(crate) fn should_skip_session_insert(
    conn: &Connection,
    request_id: &str,
    key: &DedupKey,
) -> Result<bool, AppError> {
    if proxy_request_id_exists(conn, request_id)? {
        return Ok(true);
    }
    has_matching_proxy_usage_log(conn, key)
}

#[cfg(test)]
fn proxy_request_id_exists(conn: &Connection, request_id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = ?1)",
        params![request_id],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|e| AppError::Database(format!("查询 request_id 失败: {e}")))
}

#[cfg(test)]
pub(crate) fn has_matching_proxy_usage_log(
    conn: &Connection,
    key: &DedupKey,
) -> Result<bool, AppError> {
    let allow_missing_cache_creation =
        matches!(key.app_type, "codex" | "gemini" | "opencode") && key.cache_creation_tokens == 0;

    let l_data_source = data_source_expr("l");
    let sql = format!(
        "SELECT EXISTS (
            SELECT 1
            FROM proxy_request_logs l
            WHERE {l_data_source} = 'proxy'
              AND l.app_type = ?1
              AND l.status_code >= 200
              AND l.status_code < 300
              AND l.input_tokens = ?3
              AND l.output_tokens = ?4
              AND l.cache_read_tokens = ?5
              AND (l.cache_creation_tokens = ?6 OR ?9 = 1)
              AND l.created_at BETWEEN ?7 - ?8 AND ?7 + ?8
              AND (
                  LOWER(l.model) = LOWER(?2)
                  OR LOWER(l.model) = 'unknown'
                  OR LOWER(?2) = 'unknown'
              )
        )"
    );

    conn.query_row(
        &sql,
        params![
            key.app_type,
            key.model,
            key.input_tokens as i64,
            key.output_tokens as i64,
            key.cache_read_tokens as i64,
            key.cache_creation_tokens as i64,
            key.created_at,
            SESSION_PROXY_DEDUP_WINDOW_SECONDS,
            allow_missing_cache_creation as i64,
        ],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|e| AppError::Database(format!("查询重复代理用量日志失败: {e}")))
}

/// grokbuild 会话导入的接管活动守卫：给定时刻 ±窗口内存在任何 grokbuild
/// 代理直录行，即认为当时处于代理接管态，会话事件应整体跳过——同一请求
/// 已由代理逐请求记账，会话侧再入账必双算。
///
/// 不复用 [`has_matching_proxy_usage_log`] 的指纹匹配：Grok 会话事件是
/// 逐轮聚合值，与代理逐请求行的 token 值结构性不相等，指纹永不命中。
/// 这里按"接管态检测"而非"行匹配"设计，故不过滤 status_code——失败的
/// 代理请求同样证明流量正走代理。
///
/// 已知局限（有意取舍，方向保守只漏不双）：窗口不含 session 维度，任一
/// grokbuild 代理行会给 ±窗口内的全部会话事件投下阴影——接管/官方两态在
/// 十分钟内交替或并行使用时，官方侧轮次会被跳过（漏记而非双算）。
#[cfg(test)]
pub(crate) fn has_recent_grokbuild_proxy_activity(
    conn: &Connection,
    created_at: i64,
) -> Result<bool, AppError> {
    let l_data_source = data_source_expr("l");
    let sql = format!(
        "SELECT EXISTS (
            SELECT 1
            FROM proxy_request_logs l
            WHERE {l_data_source} = 'proxy'
              AND l.app_type = 'grokbuild'
              AND l.created_at BETWEEN ?1 - ?2 AND ?1 + ?2
        )"
    );
    conn.query_row(
        &sql,
        params![created_at, SESSION_PROXY_DEDUP_WINDOW_SECONDS],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|e| AppError::Database(format!("查询 Grok 接管活动失败: {e}")))
}

#[cfg(test)]
pub(crate) fn has_suspected_codex_session_duplicate(
    conn: &Connection,
    request_id: &str,
    key: &DedupKey,
) -> Result<bool, AppError> {
    let data_source = data_source_expr("l");
    let sql = format!(
        "SELECT EXISTS (
            SELECT 1
            FROM proxy_request_logs l
            WHERE l.app_type = 'codex'
              AND {data_source} = 'codex_session'
              AND l.request_id <> ?1
              AND LOWER(l.model) = LOWER(?2)
              AND l.input_tokens = ?3
              AND l.output_tokens = ?4
              AND l.cache_read_tokens = ?5
              AND l.created_at BETWEEN ?6 - ?7 AND ?6 + ?7
        )"
    );
    conn.query_row(
        &sql,
        params![
            request_id,
            key.model,
            key.input_tokens as i64,
            key.output_tokens as i64,
            key.cache_read_tokens as i64,
            key.created_at,
            SESSION_PROXY_DEDUP_WINDOW_SECONDS,
        ],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|error| AppError::Database(format!("查询疑似重复 Codex 会话用量失败: {error}")))
}

impl Database {
    /// 获取使用量汇总
    pub fn get_usage_summary(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::summary(
            &*conn,
            usage_scope(start_date, end_date, app_type, provider_name, model),
        )
        .map_err(map_core_usage_error)
    }

    /// 按 app_type 维度拆分汇总；桌面与 Agent 共用同一折叠及排序语义。
    pub fn get_usage_summary_by_app(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<UsageSummaryByApp>, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::summary_by_app(
            &*conn,
            usage_scope(start_date, end_date, None, provider_name, model),
        )
        .map_err(map_core_usage_error)
    }

    /// 获取趋势序列；时间桶与空桶补齐规则由 Core 统一维护。
    pub fn get_daily_trends(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<DailyStats>, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::trends(
            &*conn,
            usage_scope(start_date, end_date, app_type, provider_name, model),
        )
        .map_err(map_core_usage_error)
    }

    /// 获取 Provider 聚合统计；名称回退与成功率权重统一由 Core 计算。
    pub fn get_provider_stats(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<ProviderStats>, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::provider_stats(
            &*conn,
            usage_scope(start_date, end_date, app_type, provider_name, model),
        )
        .map_err(map_core_usage_error)
    }

    /// 获取模型聚合统计；有效计价模型的回退规则统一由 Core 维护。
    pub fn get_model_stats(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<ModelStats>, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::model_stats(
            &*conn,
            usage_scope(start_date, end_date, app_type, provider_name, model),
        )
        .map_err(map_core_usage_error)
    }

    /// 获取分页请求日志；响应帧大小上限由 Core 在序列化前统一执行。
    pub fn get_request_logs(
        &self,
        filters: &LogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedLogs, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::logs(&*conn, filters.clone(), page, page_size).map_err(map_core_usage_error)
    }

    /// 获取单个请求详情；Provider 会话占位名称与兼容字段映射由 Core 统一处理。
    pub fn get_request_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::detail(&*conn, request_id).map_err(map_core_usage_error)
    }

    /// 检查 Provider 使用限额；桌面只负责连接锁，日/月窗口与 rollup 口径由 Core 维护。
    pub fn check_provider_limits(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, AppError> {
        let conn = lock_conn!(self.conn);
        UsageService::limits_on_connection(&conn, provider_id, app_type)
            .map_err(map_core_usage_error)
    }
}

#[derive(Clone)]
struct PricingInfo {
    input: rust_decimal::Decimal,
    output: rust_decimal::Decimal,
    cache_read: rust_decimal::Decimal,
    cache_creation: rust_decimal::Decimal,
}

impl Database {
    /// 定价文件同步或批量写入后回填全部零成本历史行；生产写侧服务与测试共用此入口，
    /// 不能仅在测试构建中开放，否则 models.dev 自动同步会在普通桌面构建中缺少回填能力。
    pub(crate) fn backfill_missing_usage_costs(&self) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        Self::backfill_missing_usage_costs_on_conn(&conn, None)
    }

    /// 仅回填指定 model_id 相关的零成本行；用于单条定价更新后的精准回填。
    pub(crate) fn backfill_missing_usage_costs_for_model(
        &self,
        model_id: &str,
    ) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        Self::backfill_missing_usage_costs_on_conn(&conn, Some(model_id))
    }

    pub(crate) fn backfill_missing_usage_costs_on_conn(
        conn: &Connection,
        only_model_id: Option<&str>,
    ) -> Result<u64, AppError> {
        const BASE_SQL: &str =
            "SELECT request_id, provider_id, NULL AS provider_name, app_type, model, request_model,
                        cost_multiplier,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd, is_streaming, latency_ms,
                        first_token_ms, duration_ms, status_code, error_message, created_at,
                        data_source, pricing_model, input_token_semantics
             FROM proxy_request_logs
             WHERE CAST(total_cost_usd AS REAL) <= 0
               AND (input_tokens > 0 OR output_tokens > 0
                    OR cache_read_tokens > 0 OR cache_creation_tokens > 0)";

        let mut logs = {
            let mut stmt = conn.prepare(BASE_SQL)?;
            let rows = stmt.query_map([], row_to_request_log_detail)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // 精准回填的行筛选必须与查价层共用 candidates 归一化：SQL 精确匹配会漏掉
        // 以原始别名落库的行（如 openrouter/anthropic/claude-sonnet-4.5:free），
        // 这些行查价时能归一化命中新定价，却在筛选层被挡掉，导致导入定价后
        // 历史成本要等下次全量回填才更新。误纳无害——查不到价的行会被跳过。
        if let Some(model_id) = only_model_id {
            let target = model_pricing_candidates(model_id);
            logs.retain(|log| log_pricing_scope_matches(log, &target));
        }

        if logs.is_empty() {
            return Ok(0);
        }

        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::Database(format!("启动用量成本回填事务失败: {e}")))?;

        let mut updated = 0u64;
        let mut pricing_cache = HashMap::new();
        for log in &mut logs {
            if Self::maybe_backfill_log_costs(&tx, log, &mut pricing_cache)? {
                updated += 1;
            }
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交用量成本回填事务失败: {e}")))?;

        if updated > 0 {
            log::info!("已回填 {updated} 条缺失的用量成本");
        }

        Ok(updated)
    }

    /// 尝试为单条 log 回填成本字段。返回是否实际写入（true=已 UPDATE，false=跳过）。
    fn maybe_backfill_log_costs(
        conn: &Connection,
        log: &mut RequestLogDetail,
        pricing_cache: &mut HashMap<String, PricingInfo>,
    ) -> Result<bool, AppError> {
        let existing_cost = rust_decimal::Decimal::from_str(&log.total_cost_usd)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let has_cost = existing_cost > rust_decimal::Decimal::ZERO;
        let has_usage = log.input_tokens > 0
            || log.output_tokens > 0
            || log.cache_read_tokens > 0
            || log.cache_creation_tokens > 0;

        if has_cost || !has_usage {
            return Ok(false);
        }

        let pricing = match Self::get_log_model_pricing_cached(conn, pricing_cache, log)? {
            Some(info) => info,
            None => return Ok(false),
        };
        let multiplier =
            rust_decimal::Decimal::from_str(&log.cost_multiplier).unwrap_or_else(|e| {
                log::warn!(
                    "历史用量倍率解析失败 request_id={}: {} - {e}",
                    log.request_id,
                    log.cost_multiplier
                );
                rust_decimal::Decimal::ONE
            });

        let million = rust_decimal::Decimal::from(1_000_000u64);

        // 与 CostCalculator::calculate_for_app 保持一致的计算逻辑：
        // 1. 历史 cache-inclusive 行只包含 cache read；新 total 行还包含 cache write。
        // 2. Claude/Anthropic 的 input_tokens 已经是 fresh input，不能再次扣减
        // 3. 各项成本是基础成本（不含倍率），倍率只作用于最终总价
        let cache_inclusive_app =
            crate::services::sql_helpers::is_cache_inclusive_app(log.app_type.as_str());
        let billable_input_tokens =
            if !cache_inclusive_app || log.input_token_semantics == INPUT_TOKEN_SEMANTICS_FRESH {
                log.input_tokens as u64
            } else if log.input_token_semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
                (log.input_tokens as u64)
                    .saturating_sub(log.cache_read_tokens as u64)
                    .saturating_sub(log.cache_creation_tokens as u64)
            } else {
                // v12 and earlier: input included cache reads but excluded cache writes.
                (log.input_tokens as u64).saturating_sub(log.cache_read_tokens as u64)
            };
        let input_cost =
            rust_decimal::Decimal::from(billable_input_tokens) * pricing.input / million;
        let output_cost =
            rust_decimal::Decimal::from(log.output_tokens as u64) * pricing.output / million;
        let cache_read_cost = rust_decimal::Decimal::from(log.cache_read_tokens as u64)
            * pricing.cache_read
            / million;
        let cache_creation_cost = rust_decimal::Decimal::from(log.cache_creation_tokens as u64)
            * pricing.cache_creation
            / million;
        // 总成本 = 基础成本之和 × 倍率
        let base_total = input_cost + output_cost + cache_read_cost + cache_creation_cost;
        let total_cost = base_total * multiplier;

        log.input_cost_usd = format!("{input_cost:.6}");
        log.output_cost_usd = format!("{output_cost:.6}");
        log.cache_read_cost_usd = format!("{cache_read_cost:.6}");
        log.cache_creation_cost_usd = format!("{cache_creation_cost:.6}");
        log.total_cost_usd = format!("{total_cost:.6}");

        conn.execute(
            "UPDATE proxy_request_logs
             SET input_cost_usd = ?1,
                 output_cost_usd = ?2,
                 cache_read_cost_usd = ?3,
                 cache_creation_cost_usd = ?4,
                 total_cost_usd = ?5
             WHERE request_id = ?6",
            params![
                log.input_cost_usd,
                log.output_cost_usd,
                log.cache_read_cost_usd,
                log.cache_creation_cost_usd,
                log.total_cost_usd,
                log.request_id
            ],
        )
        .map_err(|e| AppError::Database(format!("更新请求成本失败: {e}")))?;

        Ok(true)
    }

    fn get_model_pricing_cached(
        conn: &Connection,
        cache: &mut HashMap<String, PricingInfo>,
        model: &str,
    ) -> Result<Option<PricingInfo>, AppError> {
        if let Some(info) = cache.get(model) {
            return Ok(Some(info.clone()));
        }

        let row = find_model_pricing_row(conn, model)?;
        let Some((input, output, cache_read, cache_creation)) = row else {
            return Ok(None);
        };

        let pricing = PricingInfo {
            input: rust_decimal::Decimal::from_str(&input)
                .map_err(|e| AppError::Database(format!("解析输入价格失败: {e}")))?,
            output: rust_decimal::Decimal::from_str(&output)
                .map_err(|e| AppError::Database(format!("解析输出价格失败: {e}")))?,
            cache_read: rust_decimal::Decimal::from_str(&cache_read)
                .map_err(|e| AppError::Database(format!("解析缓存读取价格失败: {e}")))?,
            cache_creation: rust_decimal::Decimal::from_str(&cache_creation)
                .map_err(|e| AppError::Database(format!("解析缓存写入价格失败: {e}")))?,
        };

        cache.insert(model.to_string(), pricing.clone());
        Ok(Some(pricing))
    }

    fn get_log_model_pricing_cached(
        conn: &Connection,
        cache: &mut HashMap<String, PricingInfo>,
        log: &RequestLogDetail,
    ) -> Result<Option<PricingInfo>, AppError> {
        // 写入时的计价基准已落库（v11+）：回填只按它重算，找不到就保持 0 成本
        // 等补价。不能换用 model/request_model 猜——路由接管 + request 计价模式下
        // 三者可能各不相同（model=上游回显、request_model=客户端别名、
        // pricing_model=实际出站模型），换基准会按错误价格永久固化。
        // 占位符（"" = 未计价错误行 / "unknown"）视同缺失，走历史行逻辑。
        if let Some(pricing_model) = log
            .pricing_model
            .as_deref()
            .filter(|pm| !is_placeholder_pricing_model(pm))
        {
            return Self::get_model_pricing_cached(conn, cache, pricing_model);
        }

        if let Some(pricing) = Self::get_model_pricing_cached(conn, cache, &log.model)? {
            return Ok(Some(pricing));
        }

        // 仅当 model 列是占位符（解析失败留下的 ""/"unknown" 等）时才回退到
        // request_model 定价。model 是真实模型名但缺定价时必须保持 0 成本等待
        // 补价：路由接管下 request_model 是客户端别名（如 claude-sonnet-4-6），
        // 按别名回填会把真实上游模型的 tokens 按错误价格永久固化（行一旦有成本
        // 就不再进入回填范围）。
        if !is_placeholder_pricing_model(&log.model) {
            return Ok(None);
        }

        let Some(request_model) = log.request_model.as_deref() else {
            return Ok(None);
        };
        if request_model == log.model {
            return Ok(None);
        }

        Self::get_model_pricing_cached(conn, cache, request_model)
    }
}

#[cfg(test)]
pub(crate) fn find_model_pricing(conn: &Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing_row(conn, model_id)
        .ok()
        .flatten()
        .and_then(|(input, output, cache_read, cache_creation)| {
            ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation).ok()
        })
}

pub(crate) fn find_model_pricing_row(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    let candidates = model_pricing_candidates(model_id);
    if candidates.is_empty() {
        return Ok(None);
    }

    for candidate in &candidates {
        if let Some(row) = query_model_pricing_exact(conn, candidate)? {
            return Ok(Some(row));
        }
    }

    for candidate in &candidates {
        if should_try_pricing_prefix_match(candidate) {
            if let Some(row) = query_model_pricing_prefix(conn, candidate)? {
                return Ok(Some(row));
            }
        }
    }

    Ok(None)
}

/// 精准回填的行筛选：log 的任一模型字段归一化后与目标模型的 candidates 相交，
/// 或可按查价层的前缀规则命中目标，即视为相关。镜像 find_model_pricing_row 的
/// 匹配语义，宁可误纳（后续查价会兜底）不可漏筛。
fn log_pricing_scope_matches(log: &RequestLogDetail, target_candidates: &[String]) -> bool {
    [
        Some(log.model.as_str()),
        log.request_model.as_deref(),
        log.pricing_model.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|field| {
        model_pricing_candidates(field).iter().any(|candidate| {
            target_candidates.iter().any(|target| {
                target == candidate
                    || (should_try_pricing_prefix_match(candidate)
                        && target
                            .strip_prefix(candidate.as_str())
                            .is_some_and(|rest| rest.starts_with('-')))
            })
        })
    })
}

pub(crate) fn is_placeholder_pricing_model(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    normalized.is_empty() || matches!(normalized.as_str(), "unknown" | "null" | "none")
}

fn query_model_pricing_exact(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE model_id = ?1",
        [model_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询模型定价失败: {e}")))
}

fn query_model_pricing_prefix(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    let pattern = format!("{model_id}-%");
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE model_id LIKE ?1
         ORDER BY LENGTH(model_id) ASC
         LIMIT 1",
        [pattern],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询模型前缀定价失败: {e}")))
}

fn model_pricing_candidates(model_id: &str) -> Vec<String> {
    let cleaned = clean_model_id_for_pricing(model_id);
    if is_placeholder_pricing_model(&cleaned) {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut queue = vec![cleaned];

    while let Some(candidate) = queue.pop() {
        if !push_unique_candidate(&mut candidates, candidate.clone()) {
            continue;
        }

        if let Some(stripped) = strip_known_model_namespace(&candidate) {
            queue.push(stripped);
        }
        if let Some(stripped) = strip_claude_desktop_non_anthropic_prefix(&candidate) {
            queue.push(stripped);
        }
        if let Some(stripped) = strip_bedrock_model_version_suffix(&candidate) {
            queue.push(stripped);
        }
        if let Some(stripped) = strip_model_date_suffix(&candidate) {
            queue.push(stripped);
        }
        if let Some(stripped) = strip_reasoning_effort_suffix(&candidate) {
            queue.push(stripped);
        }
        if candidate.starts_with("claude-") && candidate.contains('.') {
            queue.push(candidate.replace('.', "-"));
        }
    }

    candidates
}

fn clean_model_id_for_pricing(model_id: &str) -> String {
    let normalized = model_id
        .rsplit_once('/')
        .map_or(model_id, |(_, r)| r)
        .split(':')
        .next()
        .unwrap_or(model_id)
        .trim()
        .replace('@', "-")
        .to_ascii_lowercase();

    normalized
        .trim_end_matches(crate::claude_desktop_config::ONE_M_CONTEXT_MARKER)
        .trim()
        .to_string()
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: String) -> bool {
    if candidate.is_empty() || candidates.iter().any(|existing| existing == &candidate) {
        return false;
    }
    candidates.push(candidate);
    true
}

fn strip_known_model_namespace(model_id: &str) -> Option<String> {
    if let Some(pos) = model_id.rfind("claude-") {
        if pos > 0 {
            return Some(model_id[pos..].to_string());
        }
    }

    for marker in [
        "openai.",
        "anthropic.",
        "google.",
        "moonshot.",
        "moonshotai.",
        "bedrock.",
        "global.",
    ] {
        if let Some(stripped) = model_id.strip_prefix(marker) {
            return Some(stripped.to_string());
        }
    }

    None
}

fn strip_claude_desktop_non_anthropic_prefix(model_id: &str) -> Option<String> {
    const NON_ANTHROPIC_MARKERS: &[&str] = &[
        "abab",
        "ark-code",
        "arctic",
        "astron",
        "codex",
        "command-r",
        "deepseek",
        "doubao",
        "ernie",
        "gemini",
        "gemma",
        "glm",
        "gpt",
        "grok",
        "hermes",
        "hy3",
        "hunyuan",
        "jamba",
        "kimi",
        "lfm",
        "llama",
        "longcat",
        "mercury",
        "mimo",
        "minimax",
        "mistral",
        "mixtral",
        "moonshot",
        "nemotron",
        "nova-",
        "openai",
        "qianfan",
        "qwen",
        "seed-",
        "solar",
        "stepfun",
    ];

    let rest = model_id.strip_prefix("claude-")?;
    NON_ANTHROPIC_MARKERS
        .iter()
        .any(|marker| rest.starts_with(marker))
        .then(|| rest.to_string())
}

fn strip_bedrock_model_version_suffix(model_id: &str) -> Option<String> {
    let (base, suffix) = model_id.rsplit_once("-v")?;
    (!base.is_empty() && !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
        .then(|| base.to_string())
}

fn strip_model_date_suffix(model_id: &str) -> Option<String> {
    let bytes = model_id.as_bytes();
    if bytes.len() > 11 {
        let start = bytes.len() - 11;
        let suffix = &bytes[start..];
        let is_iso_date = suffix[0] == b'-'
            && suffix[1..5].iter().all(|b| b.is_ascii_digit())
            && suffix[5] == b'-'
            && suffix[6..8].iter().all(|b| b.is_ascii_digit())
            && suffix[8] == b'-'
            && suffix[9..11].iter().all(|b| b.is_ascii_digit());
        if is_iso_date {
            return Some(model_id[..start].to_string());
        }
    }

    let (base, suffix) = model_id.rsplit_once('-')?;
    if base.is_empty() || !suffix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // 8 位 YYYYMMDD（如 -20250615；OpenAI / Claude / 通义千问等）。
    if suffix.len() == 8 {
        return Some(base.to_string());
    }
    // 6 位 YYMMDD（如 -260628；火山方舟 doubao-seed-*、部分国产厂商）。
    // 6 位比 8 位更易误伤非日期尾巴（如 -123456 的版本号），故额外校验
    // 月 01-12、日 01-31 才剥离；剥不动时退回 None 由上层精确匹配兜底。
    if suffix.len() == 6 {
        let month: u32 = suffix[2..4].parse().unwrap_or(0);
        let day: u32 = suffix[4..6].parse().unwrap_or(0);
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            return Some(base.to_string());
        }
    }
    None
}

fn strip_reasoning_effort_suffix(model_id: &str) -> Option<String> {
    for suffix in ["-minimal", "-low", "-medium", "-high", "-xhigh"] {
        if let Some(stripped) = model_id.strip_suffix(suffix) {
            if !stripped.is_empty() {
                return Some(stripped.to_string());
            }
        }
    }
    None
}

fn should_try_pricing_prefix_match(model_id: &str) -> bool {
    let dash_count = model_id.matches('-').count();

    if model_id.starts_with("claude-") {
        return dash_count >= 3;
    }

    if ["o1", "o3", "o4", "o5"]
        .iter()
        .any(|prefix| model_id.starts_with(prefix))
    {
        return dash_count >= 1;
    }

    const PREFIX_MATCH_FAMILIES: &[&str] = &[
        "gpt-",
        "gemini-",
        "deepseek-",
        "qwen-",
        "glm-",
        "kimi-",
        "minimax-",
    ];

    PREFIX_MATCH_FAMILIES
        .iter()
        .any(|prefix| model_id.starts_with(prefix))
        && dash_count >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn local_ts(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        match Local.with_ymd_and_hms(year, month, day, hour, minute, second) {
            chrono::LocalResult::Single(dt) => dt.timestamp(),
            chrono::LocalResult::Ambiguous(earliest, _) => earliest.timestamp(),
            chrono::LocalResult::None => panic!("valid local datetime"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_usage_log(
        conn: &Connection,
        request_id: &str,
        app_type: &str,
        provider_id: &str,
        model: &str,
        data_source: &str,
        created_at: i64,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_creation_tokens: i64,
        status_code: i64,
        total_cost_usd: &str,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, created_at, data_source
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '0', '0', '0', '0', ?, 100, ?, ?, ?)",
            params![
                request_id,
                provider_id,
                app_type,
                model,
                model,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                total_cost_usd,
                status_code,
                created_at,
                data_source
            ],
        )?;
        Ok(())
    }

    fn create_legacy_nullable_logs_table(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                status_code INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                data_source TEXT
            )",
            [],
        )?;
        Ok(())
    }

    #[test]
    fn test_effective_filter_keeps_legacy_null_data_source_proxy_rows() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('legacy-proxy', 'codex', 'gpt-5.5', 10, 2, 1, 0, 200, 1000, NULL)",
            [],
        )?;

        let filter = effective_usage_log_filter("l");
        let sql = format!("SELECT COUNT(*) FROM proxy_request_logs l WHERE {filter}");
        let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_matching_proxy_log_treats_legacy_null_data_source_as_proxy() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('legacy-proxy', 'codex', 'gpt-5.5', 10, 2, 1, 0, 200, 1000, NULL)",
            [],
        )?;

        let key = DedupKey {
            app_type: "codex",
            model: "gpt-5.5",
            input_tokens: 10,
            output_tokens: 2,
            cache_read_tokens: 1,
            cache_creation_tokens: 0,
            created_at: 1000,
        };
        assert!(has_matching_proxy_usage_log(&conn, &key)?);

        Ok(())
    }

    #[test]
    fn test_claude_desktop_folds_into_claude_for_display() -> Result<(), AppError> {
        let db = Database::memory()?;
        let ts = local_ts(2026, 6, 10, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            // 一条 Claude Code 行 + 一条 Claude Desktop 网关行，同一时间窗。
            insert_usage_log(
                &conn,
                "cc-1",
                "claude",
                "p-claude",
                "claude-sonnet-4-5",
                "proxy",
                ts,
                100,
                10,
                0,
                0,
                200,
                "0.5",
            )?;
            insert_usage_log(
                &conn,
                "cd-1",
                "claude-desktop",
                "p-desktop",
                "claude-opus-4-8",
                "proxy",
                ts,
                200,
                20,
                0,
                0,
                200,
                "1.5",
            )?;
        }

        // ① 分应用汇总：desktop 折叠进 claude，不再单列 claude-desktop 桶。
        let by_app = db.get_usage_summary_by_app(None, None, None, None)?;
        assert_eq!(by_app.len(), 1, "应只剩一个合并后的 claude 桶");
        assert_eq!(by_app[0].app_type, "claude");
        assert_eq!(by_app[0].summary.total_requests, 2, "两条行都计入 claude");
        assert!(
            !by_app.iter().any(|a| a.app_type == "claude-desktop"),
            "不应再出现 claude-desktop 桶"
        );

        // ② 选中 claude 过滤：汇总应同时覆盖 desktop 行。
        let claude_summary = db.get_usage_summary(None, None, Some("claude"), None, None)?;
        assert_eq!(claude_summary.total_requests, 2);

        // ③ 请求日志按 claude 过滤返回两行，且 desktop 行投影仍是原始 app_type。
        let logs = db.get_request_logs(
            &LogFilters {
                app_type: Some("claude".to_string()),
                ..Default::default()
            },
            0, // 页码从 0 开始
            50,
        )?;
        assert_eq!(logs.total, 2, "claude 过滤含 desktop 行");
        assert!(
            logs.data.iter().any(|r| r.app_type == "claude-desktop"),
            "详情面板需要看到真实入口，行投影不可被折叠"
        );

        // ④ 折叠不外溢：codex 过滤为空。
        let codex_summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
        assert_eq!(codex_summary.total_requests, 0);

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_uses_new_gpt_5_5_pricing() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "codex-gpt-5-5-zero-cost",
                "codex",
                "_codex_session",
                "gpt-5.5",
                "codex_session",
                1000,
                1_000_000,
                1_000_000,
                0,
                0,
                200,
                "0",
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, output_cost, total_cost): (String, String, String) = conn.query_row(
            "SELECT input_cost_usd, output_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-gpt-5-5-zero-cost'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(input_cost, "5.000000");
        assert_eq!(output_cost, "30.000000");
        assert_eq!(total_cost, "35.000000");

        Ok(())
    }

    #[test]
    fn test_backfill_distinguishes_legacy_and_total_cache_semantics() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // v12 mirror row: input = fresh + read; creation was reported separately.
            insert_usage_log(
                &conn,
                "legacy-cache-semantics",
                "codex",
                "p1",
                "gpt-5.5",
                "proxy",
                1000,
                800_000,
                0,
                600_000,
                200_000,
                200,
                "0",
            )?;
            // v13 proxy row: input = fresh + read + creation.
            insert_usage_log(
                &conn,
                "total-cache-semantics",
                "codex",
                "p1",
                "gpt-5.5",
                "proxy",
                1001,
                1_000_000,
                0,
                600_000,
                200_000,
                200,
                "0",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs
                 SET input_token_semantics = ?1
                 WHERE request_id = 'total-cache-semantics'",
                [INPUT_TOKEN_SEMANTICS_TOTAL],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 2);

        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT request_id, input_cost_usd
             FROM proxy_request_logs
             WHERE request_id IN ('legacy-cache-semantics', 'total-cache-semantics')
             ORDER BY request_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                ("legacy-cache-semantics".to_string(), "1.000000".to_string()),
                ("total-cache-semantics".to_string(), "1.000000".to_string()),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_backfill_deducts_cache_read_for_grokbuild_total_rows() -> Result<(), AppError> {
        // 回归：回填侧的 cache-inclusive 判定曾硬编码 codex|gemini 漏掉
        // grokbuild，导致 TOTAL 行按全量 input 计价、cache_read 双算。
        // 判定收敛到 sql_helpers::is_cache_inclusive_app 后按 450 fresh 计价。
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "grokbuild-total-backfill",
                "grokbuild",
                "_grok_session",
                "grok-4.5",
                "grok_session",
                1000,
                700,
                100,
                250,
                0,
                200,
                "0",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs
                 SET input_token_semantics = ?1
                 WHERE request_id = 'grokbuild-total-backfill'",
                [INPUT_TOKEN_SEMANTICS_TOTAL],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, cache_read_cost, total_cost): (String, String, String) = conn.query_row(
            "SELECT input_cost_usd, cache_read_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'grokbuild-total-backfill'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        // grok-4.5 定价 2/6/0.50：input = (700-250)×2/1M，cache_read = 250×0.5/1M
        assert_eq!(input_cost, "0.000900");
        assert_eq!(cache_read_cost, "0.000125");
        assert_eq!(total_cost, "0.001625");
        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_uses_stored_multiplier() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "codex-gpt-5-5-multiplier",
                "codex",
                "_codex_session",
                "gpt-5.5",
                "codex_session",
                1000,
                1_000_000,
                0,
                0,
                0,
                200,
                "0",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs
                 SET cost_multiplier = '1.5'
                 WHERE request_id = 'codex-gpt-5-5-multiplier'",
                [],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, total_cost): (String, String) = conn.query_row(
            "SELECT input_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-gpt-5-5-multiplier'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(input_cost, "5.000000");
        assert_eq!(total_cost, "7.500000");

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_falls_back_to_request_model() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'codex-request-model-fallback', '_codex_session', 'codex', 'unknown', 'gpt-5.5',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'codex_session'
                )",
                [],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-request-model-fallback'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(total_cost, "5.000000");

        Ok(())
    }

    #[test]
    fn test_backfill_skips_request_model_fallback_for_real_unpriced_model() -> Result<(), AppError>
    {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // 路由接管场景：model 是上游回显的真实模型（缺定价），request_model
            // 是客户端别名（有定价）。回填不得按别名定价，必须保持 0 成本等待补价。
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'takeover-unpriced-model', 'provider-1', 'claude',
                    'takeover-real-model-unpriced', 'claude-sonnet-4-6',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'proxy'
                )",
                [],
            )?;
        }

        // request_model（claude-sonnet-4-6）有定价，但 model 是真实模型名：不得回退
        assert_eq!(db.backfill_missing_usage_costs()?, 0);

        {
            let conn = lock_conn!(db.conn);
            let total_cost: String = conn.query_row(
                "SELECT total_cost_usd
                 FROM proxy_request_logs WHERE request_id = 'takeover-unpriced-model'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(total_cost, "0");

            // 补上真实模型定价后，回填必须按真实模型价格修复（0 成本行未被污染固化）
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('takeover-real-model-unpriced', 'Takeover Real Model', '0.6', '2.5')",
                [],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'takeover-unpriced-model'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(total_cost, "0.600000");

        Ok(())
    }

    #[test]
    fn test_backfill_uses_persisted_pricing_model() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // request 计价模式 + 接管：写入时锚定出站模型 kimi-k2-novel（当时缺价），
            // 但上游回显了别名 → model/request_model 都是 claude-sonnet-4-6（有定价）。
            // 回填必须按落库的 pricing_model 重算，不得换用 model 列的别名价格。
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'persisted-pricing-model', 'provider-1', 'claude',
                    'claude-sonnet-4-6', 'claude-sonnet-4-6', 'kimi-k2-novel',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'proxy'
                )",
                [],
            )?;
        }

        // pricing_model（kimi-k2-novel）缺价：不得回退到 model 列的别名价格
        assert_eq!(db.backfill_missing_usage_costs()?, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('kimi-k2-novel', 'Kimi K2 Novel', '0.6', '2.5')",
                [],
            )?;
        }

        // 按 pricing_model 也能定位到该行（model/request_model 都不是 kimi-k2-novel）
        assert_eq!(
            db.backfill_missing_usage_costs_for_model("kimi-k2-novel")?,
            1
        );

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'persisted-pricing-model'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(total_cost, "0.600000");

        Ok(())
    }

    #[test]
    fn test_scoped_backfill_matches_raw_alias_rows() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // 代理日志按上游原文落库：带路由前缀和 :free 后缀的别名形式。
            // 精准回填的筛选必须归一化后匹配，否则这类行要等全量回填才更新。
            insert_usage_log(
                &conn,
                "openrouter-alias-zero-cost",
                "claude",
                "provider-1",
                "openrouter/moonshot/kimi-k2-novel:free",
                "proxy",
                1000,
                1_000_000,
                0,
                0,
                0,
                200,
                "0",
            )?;
        }

        // 定价缺失时不应回填
        assert_eq!(db.backfill_missing_usage_costs()?, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('kimi-k2-novel', 'Kimi K2 Novel', '0.6', '2.5')",
                [],
            )?;
        }

        // 按归一化 ID 精准回填，应命中以原始别名落库的行
        assert_eq!(
            db.backfill_missing_usage_costs_for_model("kimi-k2-novel")?,
            1
        );

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'openrouter-alias-zero-cost'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(total_cost, "0.600000");

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_keeps_claude_fresh_input() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "claude-cache-fresh-input",
                "claude",
                "_session",
                "claude-haiku-4-5",
                "session_log",
                1000,
                100,
                0,
                200,
                0,
                200,
                "0",
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, cache_read_cost, total_cost): (String, String, String) = conn.query_row(
            "SELECT input_cost_usd, cache_read_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'claude-cache-fresh-input'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(input_cost, "0.000100");
        assert_eq!(cache_read_cost, "0.000020");
        assert_eq!(total_cost, "0.000120");

        Ok(())
    }

    #[test]
    fn test_get_usage_summary() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试数据
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["req1", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["req2", "p1", "claude", "claude-3", 200, 100, "0.02", 150, 200, 2000],
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.success_rate, 100.0);

        Ok(())
    }

    #[test]
    fn test_get_usage_summary_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 1, 1, 12, 0, 0);
        let end = local_ts(2024, 1, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-01",
                    "claude",
                    "p1",
                    "claude-3",
                    10,
                    10,
                    1000,
                    500,
                    0,
                    0,
                    "1.00",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-02",
                    "claude",
                    "p1",
                    "claude-3",
                    20,
                    19,
                    2000,
                    1000,
                    0,
                    0,
                    "2.00",
                    120
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-03",
                    "claude",
                    "p1",
                    "claude-3",
                    30,
                    29,
                    3000,
                    1500,
                    0,
                    0,
                    "3.00",
                    140
                ],
            )?;
        }

        let summary = db.get_usage_summary(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(summary.total_requests, 20);
        assert_eq!(summary.total_input_tokens, 2000);
        assert_eq!(summary.total_output_tokens, 1000);

        Ok(())
    }

    #[test]
    fn test_provider_and_model_filters_cover_detail_and_rollup() -> Result<(), AppError> {
        let db = Database::memory()?;
        let detail_ts = local_ts(2026, 6, 10, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES
                 ('prov-a', 'claude', 'Packy', '{}'),
                 ('prov-b', 'claude', 'DeepSeek', '{}')",
                [],
            )?;

            insert_usage_log(
                &conn,
                "a-1",
                "claude",
                "prov-a",
                "claude-sonnet-4-6",
                "proxy",
                detail_ts,
                100,
                10,
                0,
                0,
                200,
                "1.0",
            )?;
            insert_usage_log(
                &conn,
                "b-1",
                "claude",
                "prov-b",
                "deepseek-v3",
                "proxy",
                detail_ts,
                200,
                20,
                0,
                0,
                200,
                "2.0",
            )?;
            // 会话占位行：providers 表无此 id，展示名走 CASE 映射。
            insert_usage_log(
                &conn,
                "s-1",
                "claude",
                "_session",
                "claude-sonnet-4-6",
                "session_log",
                detail_ts,
                999,
                99,
                0,
                0,
                200,
                "0.5",
            )?;
            // 计价模型与请求模型不同的行：模型筛选必须按有效计价模型命中。
            insert_usage_log(
                &conn,
                "a-2",
                "claude",
                "prov-a",
                "alias-model",
                "proxy",
                detail_ts,
                50,
                5,
                0,
                0,
                200,
                "0.3",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs SET pricing_model = 'real-model' WHERE request_id = 'a-2'",
                [],
            )?;

            // rollup 历史日行：无范围过滤时全部计入。
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES
                ('2026-06-08', 'claude', 'prov-a', 'claude-sonnet-4-6', 5, 5, 500, 50, 0, 0, '5.0', 100),
                ('2026-06-08', 'claude', 'prov-b', 'deepseek-v3', 7, 7, 700, 70, 0, 0, '7.0', 100)",
                [],
            )?;
        }

        // ① 汇总按 Provider 展示名过滤：明细 + rollup 都命中。
        let packy = db.get_usage_summary(None, None, None, Some("Packy"), None)?;
        assert_eq!(packy.total_requests, 7, "a-1 + a-2 + rollup 5");

        // ② 汇总按模型过滤（有效计价模型口径）。
        let deepseek = db.get_usage_summary(None, None, None, None, Some("deepseek-v3"))?;
        assert_eq!(deepseek.total_requests, 8, "b-1 + rollup 7");

        // ③ pricing_model 优先于 model：alias-model 查不到，real-model 查得到。
        let by_alias = db.get_usage_summary(None, None, None, None, Some("alias-model"))?;
        assert_eq!(by_alias.total_requests, 0);
        let by_real = db.get_usage_summary(None, None, None, None, Some("real-model"))?;
        assert_eq!(by_real.total_requests, 1);

        // ④ 会话占位行可按可读名选中。
        let session = db.get_usage_summary(None, None, None, Some("Claude (Session)"), None)?;
        assert_eq!(session.total_requests, 1);

        // ⑤ Provider 统计 + 模型过滤：只剩 DeepSeek 一行。
        let provider_stats = db.get_provider_stats(None, None, None, None, Some("deepseek-v3"))?;
        assert_eq!(provider_stats.len(), 1);
        assert_eq!(provider_stats[0].provider_name, "DeepSeek");
        assert_eq!(provider_stats[0].request_count, 8);

        // ⑥ 模型统计 + Provider 过滤：只剩 Packy 名下的模型。
        let model_stats = db.get_model_stats(None, None, None, Some("Packy"), None)?;
        let models: Vec<&str> = model_stats.iter().map(|m| m.model.as_str()).collect();
        assert!(models.contains(&"claude-sonnet-4-6"));
        assert!(models.contains(&"real-model"));
        assert!(!models.contains(&"deepseek-v3"));

        // ⑦ 分应用汇总（Hero 卡片数据源）同样受过滤影响。
        let by_app = db.get_usage_summary_by_app(None, None, Some("Packy"), None)?;
        assert_eq!(by_app.len(), 1);
        assert_eq!(by_app[0].app_type, "claude");
        assert_eq!(by_app[0].summary.total_requests, 7);

        // ⑧ 趋势（>24h 走天分桶 + rollup 分支）。
        let t_start = local_ts(2026, 6, 8, 0, 0, 0);
        let t_end = local_ts(2026, 6, 10, 23, 59, 0);
        let trends = db.get_daily_trends(Some(t_start), Some(t_end), None, Some("Packy"), None)?;
        let total_req: u64 = trends.iter().map(|d| d.request_count).sum();
        assert_eq!(total_req, 7, "明细 2 + rollup 5");

        // ⑨ 趋势 ≤24h 走小时分桶分支（?1/?2/?3 编号参数与追加过滤混用的路径），
        //    同时验证 Provider + 模型组合过滤。
        let h_start = local_ts(2026, 6, 10, 0, 0, 0);
        let h_end = local_ts(2026, 6, 10, 20, 0, 0);
        let hourly = db.get_daily_trends(
            Some(h_start),
            Some(h_end),
            None,
            Some("Packy"),
            Some("claude-sonnet-4-6"),
        )?;
        let hourly_req: u64 = hourly.iter().map(|d| d.request_count).sum();
        assert_eq!(hourly_req, 1, "仅 a-1 命中（a-2 计价模型不同）");

        // ⑩ 请求日志列表与下拉同口径：精确名 + 有效计价模型。
        let logs = db.get_request_logs(
            &LogFilters {
                provider_name: Some("Packy".to_string()),
                model: Some("real-model".to_string()),
                ..Default::default()
            },
            0,
            10,
        )?;
        assert_eq!(logs.total, 1);
        assert_eq!(logs.data[0].request_id, "a-2");

        Ok(())
    }

    #[test]
    fn test_get_usage_summary_includes_end_day_rollup_for_minute_precision_end_time(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 1, 1, 0, 0, 0);
        let end = local_ts(2024, 1, 2, 23, 59, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-01",
                    "claude",
                    "p1",
                    "claude-3",
                    10,
                    10,
                    1000,
                    500,
                    0,
                    0,
                    "1.00",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-02",
                    "claude",
                    "p1",
                    "claude-3",
                    20,
                    19,
                    2000,
                    1000,
                    0,
                    0,
                    "2.00",
                    120
                ],
            )?;
        }

        let summary = db.get_usage_summary(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(summary.total_requests, 30);
        assert_eq!(summary.total_input_tokens, 3000);
        assert_eq!(summary.total_output_tokens, 1500);

        Ok(())
    }

    #[test]
    fn test_effective_usage_dedup_prefers_proxy_for_session_sources() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "codex-proxy",
                "codex",
                "openai",
                "GPT-5.4",
                "proxy",
                10_000,
                100,
                20,
                10,
                7,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "codex-session-dup",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                10_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "claude-proxy",
                "claude",
                "openai-compatible",
                "claude-sonnet-4-5",
                "proxy",
                25_000,
                300,
                60,
                20,
                5,
                200,
                "0.30",
            )?;
            insert_usage_log(
                &conn,
                "claude-session-dup",
                "claude",
                "_session",
                "claude-sonnet-4-5",
                "session_log",
                25_060,
                300,
                60,
                20,
                5,
                200,
                "0.30",
            )?;
            insert_usage_log(
                &conn,
                "gemini-proxy",
                "gemini",
                "google",
                "gemini-2.5-pro",
                "proxy",
                20_000,
                200,
                40,
                30,
                0,
                200,
                "0.20",
            )?;
            insert_usage_log(
                &conn,
                "gemini-session-dup",
                "gemini",
                "_gemini_session",
                "gemini-2.5-pro",
                "gemini_session",
                20_060,
                200,
                40,
                30,
                0,
                200,
                "0.20",
            )?;
            insert_usage_log(
                &conn,
                "codex-session-only",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                30_000,
                50,
                5,
                0,
                0,
                200,
                "0.02",
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 4);
        // codex-proxy contributes 100-10=90; gemini-proxy contributes 200-30=170
        // (both cache-inclusive providers). claude-proxy=300, codex-session-only=50.
        // 90 + 170 + 300 + 50 = 610.
        assert_eq!(summary.total_input_tokens, 610);
        assert_eq!(summary.total_output_tokens, 125);
        assert_eq!(summary.total_cache_read_tokens, 60);
        assert_eq!(summary.total_cache_creation_tokens, 12);
        // real_total = fresh_input(610) + output(125) + cache_create(12) + cache_read(60) = 807
        assert_eq!(summary.real_total_tokens, 807);
        // hit_rate = 60 / (610 + 12 + 60) = 60 / 682
        let expected_hit_rate = 60.0_f64 / 682.0_f64;
        assert!((summary.cache_hit_rate - expected_hit_rate).abs() < 1e-9);

        let trends = db.get_daily_trends(Some(0), Some(40_000), None, None, None)?;
        assert_eq!(trends.iter().map(|stat| stat.request_count).sum::<u64>(), 4);

        let provider_stats = db.get_provider_stats(None, None, None, None, None)?;
        assert_eq!(
            provider_stats
                .iter()
                .map(|stat| stat.request_count)
                .sum::<u64>(),
            4
        );
        assert!(provider_stats
            .iter()
            .any(|stat| stat.provider_id == "_codex_session" && stat.request_count == 1));
        assert!(!provider_stats
            .iter()
            .any(|stat| stat.provider_id == "_gemini_session"));
        assert!(!provider_stats
            .iter()
            .any(|stat| stat.provider_id == "_session"));

        let model_stats = db.get_model_stats(None, None, None, None, None)?;
        assert_eq!(
            model_stats
                .iter()
                .map(|stat| stat.request_count)
                .sum::<u64>(),
            4
        );

        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        let request_ids: Vec<&str> = logs
            .data
            .iter()
            .map(|log| log.request_id.as_str())
            .collect();
        assert_eq!(logs.total, 4);
        assert!(request_ids.contains(&"codex-proxy"));
        assert!(request_ids.contains(&"claude-proxy"));
        assert!(request_ids.contains(&"gemini-proxy"));
        assert!(request_ids.contains(&"codex-session-only"));
        assert!(!request_ids.contains(&"codex-session-dup"));
        assert!(!request_ids.contains(&"claude-session-dup"));
        assert!(!request_ids.contains(&"gemini-session-dup"));

        let breakdown = {
            let conn = lock_conn!(db.conn);
            UsageService::data_sources(&*conn).map_err(map_core_usage_error)?
        };
        let proxy_count = breakdown
            .iter()
            .find(|item| item.data_source == "proxy")
            .map(|item| item.request_count);
        let codex_session_count = breakdown
            .iter()
            .find(|item| item.data_source == "codex_session")
            .map(|item| item.request_count);
        let gemini_session_count = breakdown
            .iter()
            .find(|item| item.data_source == "gemini_session")
            .map(|item| item.request_count);
        let session_log_count = breakdown
            .iter()
            .find(|item| item.data_source == "session_log")
            .map(|item| item.request_count);
        assert_eq!(proxy_count, Some(3));
        assert_eq!(codex_session_count, Some(1));
        assert_eq!(gemini_session_count, None);
        assert_eq!(session_log_count, None);

        Ok(())
    }

    #[test]
    fn test_effective_usage_dedup_keeps_non_matching_session_rows() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "proxy-base",
                "codex",
                "openai",
                "gpt-5.4",
                "proxy",
                10_000,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-outside-window",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                10_601,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-token-mismatch",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                10_060,
                101,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-app-mismatch",
                "gemini",
                "_gemini_session",
                "gpt-5.4",
                "gemini_session",
                10_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-model-mismatch",
                "codex",
                "_codex_session",
                "different-model",
                "codex_session",
                10_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "proxy-error",
                "codex",
                "openai",
                "gpt-5.4",
                "proxy",
                20_000,
                300,
                60,
                0,
                0,
                500,
                "0.00",
            )?;
            insert_usage_log(
                &conn,
                "session-matches-error-proxy",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                20_060,
                300,
                60,
                0,
                0,
                200,
                "0.30",
            )?;
            insert_usage_log(
                &conn,
                "claude-proxy-cache-creation",
                "claude",
                "anthropic",
                "claude-sonnet-4-5",
                "proxy",
                30_000,
                100,
                20,
                10,
                5,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "claude-session-cache-creation-mismatch",
                "claude",
                "_session",
                "claude-sonnet-4-5",
                "session_log",
                30_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 9);

        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        let request_ids: Vec<&str> = logs
            .data
            .iter()
            .map(|log| log.request_id.as_str())
            .collect();
        assert_eq!(logs.total, 9);
        assert!(request_ids.contains(&"session-outside-window"));
        assert!(request_ids.contains(&"session-token-mismatch"));
        assert!(request_ids.contains(&"session-app-mismatch"));
        assert!(request_ids.contains(&"session-model-mismatch"));
        assert!(request_ids.contains(&"session-matches-error-proxy"));
        assert!(request_ids.contains(&"claude-session-cache-creation-mismatch"));

        Ok(())
    }

    #[test]
    fn test_get_model_stats() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试数据
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req1",
                    "p1",
                    "claude",
                    "claude-3-sonnet",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    1000
                ],
            )?;
        }

        let stats = db.get_model_stats(None, None, None, None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "claude-3-sonnet");
        assert_eq!(stats[0].request_count, 1);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_with_time_filter() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["old", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["new", "p1", "claude", "claude-3", 200, 75, "0.02", 120, 200, 2000],
            )?;
        }

        let stats = db.get_provider_stats(Some(1500), Some(2500), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_id, "p1");
        assert_eq!(stats[0].request_count, 1);
        assert_eq!(stats[0].total_tokens, 275);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_labels_opencode_session_provider() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "opencode-session",
                "opencode",
                "_opencode_session",
                "opencode-model",
                "opencode_session",
                1000,
                100,
                50,
                0,
                0,
                200,
                "0.01",
            )?;
        }

        let stats = db.get_provider_stats(None, None, Some("opencode"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_id, "_opencode_session");
        assert_eq!(stats[0].provider_name, "OpenCode (Session)");

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 2, 1, 12, 0, 0);
        let end = local_ts(2024, 2, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-02-01",
                    "claude",
                    "p-rollup",
                    "claude-3",
                    5,
                    5,
                    500,
                    250,
                    0,
                    0,
                    "0.50",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-02-02",
                    "claude",
                    "p-rollup",
                    "claude-3",
                    8,
                    7,
                    800,
                    400,
                    0,
                    0,
                    "0.80",
                    120
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-02-03",
                    "claude",
                    "p-rollup",
                    "claude-3",
                    12,
                    11,
                    1200,
                    600,
                    0,
                    0,
                    "1.20",
                    140
                ],
            )?;
        }

        let stats = db.get_provider_stats(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_id, "p-rollup");
        assert_eq!(stats[0].request_count, 8);
        assert_eq!(stats[0].total_tokens, 1200);

        Ok(())
    }

    #[test]
    fn test_get_daily_trends_respects_shorter_than_24_hours() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-short",
                    "p1",
                    "claude",
                    "claude-3",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    10_800
                ],
            )?;
        }

        let stats = db.get_daily_trends(Some(0), Some(15 * 60 * 60), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 15);
        assert_eq!(stats[3].request_count, 1);

        Ok(())
    }

    #[test]
    fn test_get_daily_trends_groups_ranges_longer_than_24_hours_by_local_day(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 3, 1, 12, 0, 0);
        let end = local_ts(2024, 3, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "day-1-detail",
                    "p1",
                    "claude",
                    "claude-3",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    local_ts(2024, 3, 1, 13, 0, 0)
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "day-3-detail",
                    "p1",
                    "claude",
                    "claude-3",
                    200,
                    75,
                    "0.02",
                    110,
                    200,
                    local_ts(2024, 3, 3, 10, 0, 0)
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-03-02",
                    "claude",
                    "p1",
                    "claude-3",
                    4,
                    4,
                    400,
                    200,
                    0,
                    0,
                    "0.40",
                    120
                ],
            )?;
        }

        let stats = db.get_daily_trends(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].request_count, 1);
        assert_eq!(stats[0].total_tokens, 150);
        assert_eq!(stats[1].request_count, 4);
        assert_eq!(stats[1].total_tokens, 600);
        assert_eq!(stats[2].request_count, 1);
        assert_eq!(stats[2].total_tokens, 275);

        Ok(())
    }

    #[test]
    fn test_get_model_stats_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 4, 1, 12, 0, 0);
        let end = local_ts(2024, 4, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-04-01",
                    "claude",
                    "p1",
                    "claude-3-haiku",
                    6,
                    6,
                    600,
                    300,
                    0,
                    0,
                    "0.60",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-04-02",
                    "claude",
                    "p1",
                    "claude-3-haiku",
                    9,
                    8,
                    900,
                    450,
                    0,
                    0,
                    "0.90",
                    110
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-04-03",
                    "claude",
                    "p1",
                    "claude-3-haiku",
                    12,
                    11,
                    1200,
                    600,
                    0,
                    0,
                    "1.20",
                    130
                ],
            )?;
        }

        let stats = db.get_model_stats(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "claude-3-haiku");
        assert_eq!(stats[0].request_count, 9);
        assert_eq!(stats[0].total_tokens, 1350);

        Ok(())
    }

    #[test]
    fn test_strip_model_date_suffix_is_utf8_safe() {
        assert_eq!(
            strip_model_date_suffix("模型-2026-05-14").as_deref(),
            Some("模型")
        );
        assert_eq!(strip_model_date_suffix("abc🚀12345678"), None);
    }

    #[test]
    fn test_strip_model_date_suffix_handles_six_digit_yymmdd() {
        // 火山方舟 6 位 YYMMDD 后缀应被剥离（doubao 全系都用这种格式）。
        assert_eq!(
            strip_model_date_suffix("doubao-seed-2-1-pro-260628").as_deref(),
            Some("doubao-seed-2-1-pro")
        );
        assert_eq!(
            strip_model_date_suffix("doubao-seed-1-6-250615").as_deref(),
            Some("doubao-seed-1-6")
        );
        // 8 位 YYYYMMDD 仍照旧剥离。
        assert_eq!(
            strip_model_date_suffix("claude-3-5-sonnet-20241022").as_deref(),
            Some("claude-3-5-sonnet")
        );
        // 月/日非法的 6 位尾巴（版本号等）不剥离，避免误伤。
        assert_eq!(strip_model_date_suffix("foo-bar-123456"), None); // 月=34
        assert_eq!(strip_model_date_suffix("widget-209900"), None); // 月=99
        assert_eq!(strip_model_date_suffix("gizmo-251200"), None); // 日=00
    }

    #[test]
    fn test_pricing_resolves_volcengine_dated_model_to_bare_seed_row() -> Result<(), AppError> {
        // 回归：火山真实用量带 6 位日期后缀（doubao-seed-2-1-pro-260628），
        // 必须能归一化命中定价表里的裸名 seed 行（doubao-seed-2-1-pro），否则成本显示 $0。
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES ('doubao-seed-2-1-pro', 'Doubao Seed 2.1 Pro', '0.84', '4.2', '0.17', '0')",
            [],
        )?;

        let row = find_model_pricing_row(&conn, "doubao-seed-2-1-pro-260628")?;
        assert!(
            row.is_some(),
            "带日期的火山模型应通过 6 位日期剥离命中裸名定价行"
        );
        let (input, output, ..) = row.unwrap();
        assert_eq!(input, "0.84");
        assert_eq!(output, "4.2");

        Ok(())
    }

    #[test]
    fn test_prefix_pricing_does_not_match_short_base_model_to_variant() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        conn.execute("DELETE FROM model_pricing WHERE model_id LIKE 'gpt-5%'", [])?;
        for (model_id, display_name) in [("gpt-5-mini", "GPT-5 Mini"), ("gpt-5-pro", "GPT-5 Pro")] {
            conn.execute(
                "INSERT INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES (?1, ?2, '1', '2', '0', '0')",
                params![model_id, display_name],
            )?;
        }

        let result = find_model_pricing_row(&conn, "gpt-5")?;
        assert!(
            result.is_none(),
            "缺少 gpt-5 基础定价时，不应前缀误匹配到 gpt-5-mini/gpt-5-pro"
        );

        Ok(())
    }

    #[test]
    fn test_model_pricing_matching() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);

        // 准备额外定价数据，覆盖前缀/后缀清洗场景
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                "claude-haiku-4.5",
                "Claude Haiku 4.5",
                "1.0",
                "2.0",
                "0.0",
                "0.0"
            ],
        )?;

        // 测试精确匹配（seed_model_pricing 已预置 claude-sonnet-4-5-20250929）
        let result = find_model_pricing_row(&conn, "claude-sonnet-4-5-20250929")?;
        assert!(
            result.is_some(),
            "应该能精确匹配 claude-sonnet-4-5-20250929"
        );

        // 清洗：去除前缀和冒号后缀
        let result = find_model_pricing_row(&conn, "anthropic/claude-haiku-4.5")?;
        assert!(
            result.is_some(),
            "带前缀的模型 anthropic/claude-haiku-4.5 应能匹配到 claude-haiku-4.5"
        );
        let result = find_model_pricing_row(&conn, "moonshotai/kimi-k2-0905:exa")?;
        assert!(
            result.is_some(),
            "带前缀+冒号后缀的模型应清洗后匹配到 kimi-k2-0905"
        );

        // 清洗：@ 替换为 -（seed_model_pricing 已预置 gpt-5.2-codex-low）
        let result = find_model_pricing_row(&conn, "gpt-5.2-codex@low")?;
        assert!(
            result.is_some(),
            "带 @ 分隔符的模型 gpt-5.2-codex@low 应能匹配到 gpt-5.2-codex-low"
        );
        let result = find_model_pricing_row(&conn, "OpenAI/GPT-5.5@HIGH")?;
        assert!(
            result.is_some(),
            "大小写混合的 GPT-5.5 模型应能归一化匹配到 gpt-5.5-high"
        );
        let result = find_model_pricing_row(&conn, "OpenAI/GPT-5.5-2026-05-14")?;
        assert!(
            result.is_some(),
            "OpenAI 日期后缀模型应能回退到 gpt-5.5 基础定价"
        );
        let result = find_model_pricing_row(&conn, "google/gemini-3-pro-preview-20260514")?;
        assert!(
            result.is_some(),
            "Gemini 日期后缀模型应能回退到 gemini-3-pro-preview 基础定价"
        );

        // Claude Desktop route 短 ID：应通过前缀匹配到带日期的定价
        let result = find_model_pricing_row(&conn, "claude-haiku-4-5")?;
        assert!(
            result.is_some(),
            "Claude Desktop 短路由 claude-haiku-4-5 应能匹配到 claude-haiku-4-5-20251001"
        );
        let result = find_model_pricing_row(&conn, "anthropic/claude-opus-4.8")?;
        assert!(
            result.is_some(),
            "聚合商点号格式 anthropic/claude-opus-4.8 应能匹配到 claude-opus-4-8"
        );

        // Claude Desktop 旧版/异常包装的非 Anthropic route：claude-gpt-5.5 → gpt-5.5
        let result = find_model_pricing_row(&conn, "claude-gpt-5.5")?;
        assert!(
            result.is_some(),
            "带 claude- 包装的非 Anthropic 模型应能剥离后匹配到真实模型定价"
        );

        // Bedrock/Vertex 常见形态：provider 前缀 + -vN 后缀 + :0 修饰
        let result =
            find_model_pricing_row(&conn, "global.anthropic.claude-haiku-4-5-20251001-v1:0")?;
        assert!(
            result.is_some(),
            "Bedrock/Vertex 风格 Claude 模型 ID 应能归一化到基础 Claude 模型定价"
        );
        let result = find_model_pricing_row(&conn, "global.anthropic.claude-opus-4-8-v1:0")?;
        assert!(
            result.is_some(),
            "Bedrock 风格 Claude Opus 4.8 模型 ID 应能归一化到基础 Claude 模型定价"
        );
        let result = find_model_pricing_row(&conn, "claude-opus-4-8@20260527")?;
        assert!(
            result.is_some(),
            "Vertex 风格 Claude Opus 4.8 模型 ID 应能归一化到基础 Claude 模型定价"
        );

        // Reasoning effort 后缀：没有专门价格时回退到基础模型
        let result = find_model_pricing_row(&conn, "gpt-5.4@low")?;
        assert!(
            result.is_some(),
            "缺少专门 effort 价格时应回退到 gpt-5.4 基础模型定价"
        );

        // Kimi Code 是订阅/额度模型，不应伪装成公开按 token 计费模型
        let result = find_model_pricing_row(&conn, "kimi-for-coding")?;
        assert!(result.is_none(), "kimi-for-coding 没有固定 token 单价");

        // 测试不存在的模型
        let result = find_model_pricing_row(&conn, "unknown-model-123")?;
        assert!(result.is_none(), "不应该匹配不存在的模型");

        Ok(())
    }
}
