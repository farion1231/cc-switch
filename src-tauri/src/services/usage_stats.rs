//! 使用统计服务
//!
//! 提供使用量数据的聚合查询功能

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::ModelPricing;
use crate::services::sql_helpers::{
    fresh_input_sql, real_total_tokens_sql, INPUT_TOKEN_SEMANTICS_FRESH,
    INPUT_TOKEN_SEMANTICS_LEGACY, INPUT_TOKEN_SEMANTICS_TOTAL,
};
use chrono::{Local, NaiveDate, TimeZone, Timelike};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::LazyLock;

/// 使用量汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_requests: u64,
    pub total_cost: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub success_rate: f32,
    /// input + output + cache_creation + cache_read — the total tokens
    /// actually processed by the model (including cache hits). Used as the
    /// headline "real consumption" number in the usage hero.
    pub real_total_tokens: u64,
    /// cache_read / (input + cache_creation + cache_read). Range 0.0–1.0.
    /// Reported as a fraction; multiply by 100 in UI for percentage display.
    pub cache_hit_rate: f64,
}

/// Per-app-type usage summary used by the dashboard breakdown rail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryByApp {
    pub app_type: String,
    pub summary: UsageSummary,
}

/// Helper: compute (real_total, hit_rate) from the four token counters.
/// All inputs must already be cache-normalized (i.e. input excludes cache).
fn derive_real_total_and_hit_rate(
    fresh_input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
) -> (u64, f64) {
    let real_total = fresh_input + output + cache_creation + cache_read;
    let cacheable_input = fresh_input + cache_creation + cache_read;
    let hit_rate = if cacheable_input > 0 {
        cache_read as f64 / cacheable_input as f64
    } else {
        0.0
    };
    (real_total, hit_rate)
}

/// 汇总查询的一行（请求数、花费、四类 Token、成功数）转成 [`UsageSummary`]。
/// 列顺序须与 `get_usage_summary` / `get_session_usage_summary` 的 SELECT 一致。
fn usage_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSummary> {
    let total_requests: i64 = row.get(0)?;
    let total_cost: f64 = row.get(1)?;
    let total_input_tokens: i64 = row.get(2)?;
    let total_output_tokens: i64 = row.get(3)?;
    let total_cache_creation_tokens: i64 = row.get(4)?;
    let total_cache_read_tokens: i64 = row.get(5)?;
    let success_count: i64 = row.get(6)?;

    let success_rate = if total_requests > 0 {
        (success_count as f32 / total_requests as f32) * 100.0
    } else {
        0.0
    };

    let (real_total_tokens, cache_hit_rate) = derive_real_total_and_hit_rate(
        total_input_tokens as u64,
        total_output_tokens as u64,
        total_cache_creation_tokens as u64,
        total_cache_read_tokens as u64,
    );

    Ok(UsageSummary {
        total_requests: total_requests as u64,
        total_cost: format!("{total_cost:.6}"),
        total_input_tokens: total_input_tokens as u64,
        total_output_tokens: total_output_tokens as u64,
        total_cache_creation_tokens: total_cache_creation_tokens as u64,
        total_cache_read_tokens: total_cache_read_tokens as u64,
        success_rate,
        real_total_tokens,
        cache_hit_rate,
    })
}

/// 每日统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStats {
    pub date: String,
    pub request_count: u64,
    pub total_cost: String,
    pub total_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

/// Provider 统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStats {
    pub provider_id: String,
    pub app_type: String,
    pub provider_name: String,
    pub request_count: u64,
    /// 真实消耗 Tokens（新增输入 + 输出 + 缓存写入 + 缓存命中），与指标卡同口径。
    pub total_tokens: u64,
    pub total_cost: String,
    pub success_rate: f32,
    pub avg_latency_ms: u64,
    /// 速度的分子：满足条件（有首字、输出 >= 100 token、耗时 > 首字）的明细请求的输出 token 之和。
    /// 汇总速度 = speed_output_tokens / (speed_generation_ms / 1000)，不是逐条平均。
    /// 日汇总（rollup）没有逐条计时，不计入。
    pub speed_output_tokens: u64,
    /// 速度的分母：同一批请求的 (latency_ms - first_token_ms) 之和，单位毫秒。
    pub speed_generation_ms: u64,
    /// 估算速度的分子：会话日志导入的请求里，有估算耗时、输出 >= 200 token 的那些的输出之和。
    /// 和上面那组分开累计：估算的耗时含首字等待，口径不同，不能加在一起。
    pub est_speed_output_tokens: u64,
    /// 估算速度的分母：同一批请求的 latency_ms 之和，单位毫秒。
    pub est_speed_duration_ms: u64,
}

/// 计速度的门槛：输出少于这个数的请求（工具调用这类）不算速度，避免 0.1 秒回 15 个 token 算出离谱的数。
pub const SPEED_MIN_OUTPUT_TOKENS: i64 = 100;

/// 生成窗口（耗时 − 首字）短于这个毫秒数时不算速度：中转站缓冲后一次性吐出、
/// 或短回复整段落在同一个网络包里，算出来的是传输突发而不是生成速度。
pub const SPEED_MIN_GENERATION_MS: i64 = 100;

/// 明细行能不能计速度的 SQL 条件（和前端 `isSpeedEligible` 同口径）。
fn speed_eligible_sql(alias: &str) -> String {
    format!(
        "{alias}.first_token_ms IS NOT NULL AND {alias}.output_tokens >= {SPEED_MIN_OUTPUT_TOKENS} \
         AND {alias}.latency_ms - {alias}.first_token_ms >= {SPEED_MIN_GENERATION_MS}"
    )
}

/// 估算速度的输出门槛：估算的耗时含首字等待，输出越少首字占比越大、算出来越偏低，
/// 所以比精确口径的门槛高。实测 200–300 token 的请求比长请求低约四分之一，
/// 100–200 的低约四成；而 Codex 的请求只有四分之一超过 500，门槛再高大半行都是空的。
pub const SPEED_ESTIMATE_MIN_OUTPUT_TOKENS: i64 = 200;

/// 估算耗时短于这个毫秒数时不估速度：输出 200 token 以上却不到 1 秒，多半是起点取晚了。
pub const SPEED_ESTIMATE_MIN_DURATION_MS: i64 = 1000;

/// 明细行能不能估速度的 SQL 条件（和前端 `isSpeedEstimateEligible` 同口径）：
/// 会话日志导入的行（没有首字计时），耗时是导入时按日志时间戳估的。
fn speed_estimate_eligible_sql(alias: &str) -> String {
    let data_source = data_source_expr(alias);
    format!(
        "{alias}.first_token_ms IS NULL AND {data_source} <> 'proxy' \
         AND {alias}.output_tokens >= {SPEED_ESTIMATE_MIN_OUTPUT_TOKENS} \
         AND {alias}.latency_ms >= {SPEED_ESTIMATE_MIN_DURATION_MS}"
    )
}

/// 模型统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    pub model: String,
    pub request_count: u64,
    /// 真实消耗 Tokens（新增输入 + 输出 + 缓存写入 + 缓存命中），与指标卡同口径。
    pub total_tokens: u64,
    pub total_cost: String,
    pub avg_cost_per_request: String,
    pub success_rate: f32,
    /// 速度的分子分母，口径同 [`ProviderStats::speed_output_tokens`] 那四个字段。
    pub speed_output_tokens: u64,
    pub speed_generation_ms: u64,
    pub est_speed_output_tokens: u64,
    pub est_speed_duration_ms: u64,
}

/// 请求日志过滤器
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilters {
    pub app_type: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<u16>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
}

/// 分页请求日志响应
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedLogs {
    pub data: Vec<RequestLogDetail>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

/// 请求日志详情
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogDetail {
    pub request_id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub app_type: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_model: Option<String>,
    pub cost_multiplier: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    /// Internal storage semantics; omitted from the UI/API payload.
    #[serde(skip)]
    pub input_token_semantics: i64,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub total_cost_usd: String,
    pub is_streaming: bool,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source: Option<String>,
    /// 写入时实际用于计价的模型名。None = v11 前的历史行，"" = 未计价的错误行。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_model: Option<String>,
}

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

/// SQL fragment: resolve provider_name with fallback for session-based entries.
/// Session logs use placeholder provider_ids (e.g., `_session`, `_<app>_session`)
/// that don't exist in the providers table — the CASE expression below is the
/// authoritative mapping from placeholder to readable name.
fn provider_name_coalesce(log_alias: &str, provider_alias: &str) -> String {
    format!(
        "COALESCE({provider_alias}.name, CASE {log_alias}.provider_id \
         WHEN '_session' THEN 'Claude (Session)' \
         WHEN '_codex_session' THEN 'Codex (Session)' \
         WHEN '_gemini_session' THEN 'Gemini (Session)' \
         WHEN '_opencode_session' THEN 'OpenCode (Session)' \
         WHEN '_grok_session' THEN 'Grok Build (Session)' \
         WHEN '_mcode_session' THEN 'MiniMax Code (Session)' \
         WHEN '_pi_session' THEN 'Pi (Session)' \
         ELSE {log_alias}.provider_id END)"
    )
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

fn dedup_app_type_match_sql(left: &str, right: &str) -> String {
    format!(
        "{left} IN ({right}, CASE WHEN {right} = 'claude' THEN 'claude-desktop' ELSE {right} END)"
    )
}

/// SQL 标量表达式：把 Claude Desktop 网关的 `claude-desktop` app_type 在“展示口径”
/// 上折叠进 `claude`，其余 app_type 原样返回。
///
/// 背景：Desktop 网关流量在记账层按各自入口写为 `app_type='claude-desktop'`，
/// 以保留路由接管的账单审计精度（不要回退这一点）。但 Dashboard 把它当作
/// Claude Code 呈现——它本质就是跑在 Desktop 壳里的内嵌 Claude Code 运行时，
/// 且 Desktop 聊天用量永远不经过本软件，单列只会让用户误以为是“桌面版全部用量”。
///
/// 用法：把任一参与“按应用筛选/分组”的 `app_type` 列包进此表达式即可，
/// 这样 `= 'claude'` 过滤会同时命中 `claude-desktop`、`GROUP BY` 会把两者合并，
/// 而不改动任何已存储的行（详情面板仍读原始 `app_type`）。
///
/// 注意：包裹后该列上的索引在此比较中失效，但这些都是已带时间过滤的聚合扫描，
/// app_type 本就不是主访问路径，可接受。仅用于读侧；跨源去重使用更窄的
/// [`dedup_app_type_match_sql`]，额度检查（`check_provider_limits`）仍保留原始精确比较。
fn folded_app_type_sql(column: &str) -> String {
    format!("CASE WHEN {column} = 'claude-desktop' THEN 'claude' ELSE {column} END")
}

/// SQL 片段：把日志/汇总行 LEFT JOIN 到 providers 表以取得供应商名称。
/// `proxy_request_logs` 与 `usage_daily_rollups` 的 (provider_id, app_type)
/// 形状相同，两者皆可作为 `log_alias`。providers 主键即 (id, app_type)，
/// 连接至多 1:1，不会放大行数。
fn providers_join(log_alias: &str, provider_alias: &str) -> String {
    format!(
        "LEFT JOIN providers {provider_alias} \
         ON {log_alias}.provider_id = {provider_alias}.id \
         AND {log_alias}.app_type = {provider_alias}.app_type"
    )
}

/// SQL 标量表达式：行的「有效计价模型」—— pricing_model 非空优先，NULL/'' 回落
/// model。这是 `get_model_stats` 的分组键，也是 Dashboard 模型筛选的匹配口径：
/// 筛选值来自模型统计列表，两边必须用同一表达式才能选得中。
fn effective_model_sql(alias: &str) -> String {
    format!("COALESCE(NULLIF({alias}.pricing_model, ''), {alias}.model)")
}

/// 把 Dashboard 顶部的 Provider/模型筛选追加到查询条件。
///
/// Provider 按展示名精确匹配（复用 [`provider_name_coalesce`]，会话占位行的
/// 可读名如 "Claude (Session)" 也能选中）；模型按 [`effective_model_sql`] 匹配。
/// 注意：传入 `provider_name` 时调用方必须把 [`providers_join`] 拼进 FROM，
/// 否则 `{provider_alias}.name` 无法解析。
fn push_provider_model_filters(
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
    log_alias: &str,
    provider_alias: &str,
    provider_name: Option<&str>,
    model: Option<&str>,
) {
    if let Some(name) = provider_name {
        conditions.push(format!(
            "{} = ?",
            provider_name_coalesce(log_alias, provider_alias)
        ));
        params.push(Box::new(name.to_string()));
    }
    if let Some(m) = model {
        conditions.push(format!("{} = ?", effective_model_sql(log_alias)));
        params.push(Box::new(m.to_string()));
    }
}

pub(crate) fn effective_usage_log_filter(log_alias: &str) -> String {
    let data_source = data_source_expr(log_alias);
    let proxy_data_source = data_source_expr("proxy_dedup");
    let app_type_match =
        dedup_app_type_match_sql("proxy_dedup.app_type", &format!("{log_alias}.app_type"));
    format!(
        "NOT (
            {data_source} IN ('session_log', 'codex_session', 'gemini_session', 'opencode_session')
            AND EXISTS (
                SELECT 1
                FROM proxy_request_logs proxy_dedup
                WHERE {proxy_data_source} = 'proxy'
                  AND {app_type_match}
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
                      OR (
                          proxy_dedup.cache_creation_tokens = 0
                          AND proxy_dedup.input_token_semantics = {INPUT_TOKEN_SEMANTICS_LEGACY}
                          AND {data_source} = 'codex_session'
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

/// 参与跨源去重的会话日志来源（和 [`effective_usage_log_filter`] 同口径）。
const DEDUP_SESSION_SOURCES_SQL: &str =
    "'session_log', 'codex_session', 'gemini_session', 'opencode_session'";

/// Dashboard 读路径用的去重条件：语义和 [`effective_usage_log_filter`] 完全一致，
/// 只是先看时间窗口里两类日志各有多少，再挑便宜的写法。
///
/// - 窗口里没有成功的代理日志、或没有会话日志：不可能有重复，直接不加条件。
/// - 代理日志更少：先从代理日志出发找出重复的会话行（一次性子查询），
///   主查询只做 `rowid NOT IN`，不再逐行关联。会话日志占绝大多数时快很多。
/// - 会话日志更少：保留原来的逐行 `EXISTS`。
///
/// 时间边界放宽 [`SESSION_PROXY_DEDUP_WINDOW_SECONDS`]，窗口外的代理行
/// 不可能匹配窗口内的会话行；边界是整数，直接拼进 SQL。
pub(crate) fn effective_usage_log_filter_for_range(
    conn: &Connection,
    log_alias: &str,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<String, AppError> {
    let lo = start_date
        .map(|v| v.saturating_sub(SESSION_PROXY_DEDUP_WINDOW_SECONDS))
        .unwrap_or(i64::MIN / 2);
    let hi = end_date
        .map(|v| v.saturating_add(SESSION_PROXY_DEDUP_WINDOW_SECONDS))
        .unwrap_or(i64::MAX / 2);
    let data_source = data_source_expr("c");
    let (proxy_count, session_count): (i64, i64) = conn.query_row(
        &format!(
            "SELECT
                COALESCE(SUM(CASE WHEN {data_source} = 'proxy'
                    AND c.status_code >= 200 AND c.status_code < 300 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN {data_source} IN ({DEDUP_SESSION_SOURCES_SQL})
                    THEN 1 ELSE 0 END), 0)
             FROM proxy_request_logs c
             WHERE c.created_at BETWEEN ?1 AND ?2"
        ),
        rusqlite::params![lo, hi],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    if proxy_count == 0 || session_count == 0 {
        return Ok("1 = 1".to_string());
    }
    if proxy_count > session_count {
        return Ok(effective_usage_log_filter(log_alias));
    }

    let dp_source = data_source_expr("dedup_p");
    let ds_source = data_source_expr("dedup_s");
    Ok(format!(
        "{log_alias}.rowid NOT IN (
            SELECT dedup_s.rowid
            FROM proxy_request_logs dedup_p
            JOIN proxy_request_logs dedup_s
              ON dedup_s.app_type IN (
                     dedup_p.app_type,
                     CASE WHEN dedup_p.app_type = 'claude-desktop' THEN 'claude' ELSE dedup_p.app_type END
                 )
             AND {ds_source} IN ({DEDUP_SESSION_SOURCES_SQL})
             AND dedup_s.input_tokens = dedup_p.input_tokens
             AND dedup_s.output_tokens = dedup_p.output_tokens
             AND dedup_s.cache_read_tokens = dedup_p.cache_read_tokens
             AND dedup_s.created_at BETWEEN
                 dedup_p.created_at - {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                 AND dedup_p.created_at + {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
             AND (
                 dedup_p.cache_creation_tokens = dedup_s.cache_creation_tokens
                 OR (
                     dedup_s.cache_creation_tokens = 0
                     AND {ds_source} IN ('codex_session', 'gemini_session', 'opencode_session')
                 )
                 OR (
                     dedup_p.cache_creation_tokens = 0
                     AND dedup_p.input_token_semantics = {INPUT_TOKEN_SEMANTICS_LEGACY}
                     AND {ds_source} = 'codex_session'
                 )
             )
             AND (
                 LOWER(dedup_p.model) = LOWER(dedup_s.model)
                 OR LOWER(dedup_p.model) = 'unknown'
                 OR LOWER(dedup_s.model) = 'unknown'
             )
            WHERE {dp_source} = 'proxy'
              AND dedup_p.status_code >= 200
              AND dedup_p.status_code < 300
              AND dedup_p.created_at BETWEEN {lo} AND {hi}
        )"
    ))
}

/// 请求日志总数缓存：每次刷新都 `COUNT(*)` 一遍明细，大范围下最耗时。
/// 筛选条件、起点没变，且连接上没有任何写入（`total_changes` 没变，增删改都算）
/// 时复用上次的总数；结束时间往后推时（「当天」这类活动窗口每次刷新都会变），
/// 再确认新增的时间段里没有行。另设 [`LOG_COUNT_CACHE_TTL`] 兜底。
pub(crate) struct LogCountCache {
    key: String,
    end_date: Option<i64>,
    changes: u64,
    computed_at: std::time::Instant,
    total: u32,
}

const LOG_COUNT_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// 这条连接打开以来增删改过的总行数（SQLite 内置函数），任何写入都会让它变。
fn connection_total_changes(conn: &Connection) -> Result<u64, AppError> {
    Ok(conn.query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))? as u64)
}

fn log_count_cache_key(filters: &LogFilters) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}",
        filters.app_type,
        filters.provider_name,
        filters.model,
        filters.status_code,
        filters.start_date
    )
}

/// 能复用缓存时返回缓存的总数。
fn cached_log_count(
    cache: &Option<LogCountCache>,
    conn: &Connection,
    filters: &LogFilters,
) -> Result<Option<u32>, AppError> {
    let Some(entry) = cache.as_ref() else {
        return Ok(None);
    };
    if entry.key != log_count_cache_key(filters)
        || entry.changes != connection_total_changes(conn)?
        || entry.computed_at.elapsed() > LOG_COUNT_CACHE_TTL
    {
        return Ok(None);
    }
    match (entry.end_date, filters.end_date) {
        (cached, current) if cached == current => Ok(Some(entry.total)),
        (Some(cached), Some(current)) if current > cached => {
            // 结束时间往后推了：新增区间里没有任何行，总数就不变
            let has_new_rows: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM proxy_request_logs
                  WHERE created_at > ?1 AND created_at <= ?2)",
                params![cached, current],
                |row| row.get(0),
            )?;
            Ok((!has_new_rows).then_some(entry.total))
        }
        _ => Ok(None),
    }
}

/// 跨源去重指纹键。
///
/// `cache_creation_tokens`：Gemini/OpenCode 会话日志和旧版 Codex rollout 不暴露
/// 该字段，调用方传 0 表示"未知"，匹配器会放行 proxy 侧任意 cache_creation_tokens
/// 值。反过来，v3.17.0 之前的代理不记录 Codex 缓存写入，legacy 语义且写入量为 0
/// 的 Codex 代理行也放行任意 session 写入量。
#[derive(Debug, Clone, Copy)]
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

fn proxy_request_id_exists(conn: &Connection, request_id: &str) -> Result<bool, AppError> {
    conn.prepare_cached("SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = ?1)")
        .and_then(|mut stmt| stmt.query_row(params![request_id], |row| row.get::<_, bool>(0)))
        .map_err(|e| AppError::Database(format!("查询 request_id 失败: {e}")))
}

// 会话重导每个 token 事件都要跑一次这条查询；SQL 文本静态化让
// prepare_cached 稳定命中，也省掉每行的 format! 分配。
static MATCHING_PROXY_USAGE_LOG_SQL: LazyLock<String> = LazyLock::new(|| {
    let l_data_source = data_source_expr("l");
    let app_type_match = dedup_app_type_match_sql("l.app_type", "?1");
    format!(
        "SELECT EXISTS (
            SELECT 1
            FROM proxy_request_logs l
            WHERE {l_data_source} = 'proxy'
              AND {app_type_match}
              AND l.status_code >= 200
              AND l.status_code < 300
              AND l.input_tokens = ?3
              AND l.output_tokens = ?4
              AND l.cache_read_tokens = ?5
              AND (
                  l.cache_creation_tokens = ?6
                  OR ?9 = 1
                  OR (
                      ?10 = 1
                      AND l.cache_creation_tokens = 0
                      AND l.input_token_semantics = {INPUT_TOKEN_SEMANTICS_LEGACY}
                  )
              )
              AND l.created_at BETWEEN ?7 - ?8 AND ?7 + ?8
              AND (
                  LOWER(l.model) = LOWER(?2)
                  OR LOWER(l.model) = 'unknown'
                  OR LOWER(?2) = 'unknown'
              )
        )"
    )
});

pub(crate) fn has_matching_proxy_usage_log(
    conn: &Connection,
    key: &DedupKey,
) -> Result<bool, AppError> {
    let allow_missing_cache_creation =
        matches!(key.app_type, "codex" | "gemini" | "opencode") && key.cache_creation_tokens == 0;
    let allow_legacy_proxy_cache_creation = key.app_type == "codex";

    conn.prepare_cached(&MATCHING_PROXY_USAGE_LOG_SQL)
        .and_then(|mut stmt| {
            stmt.query_row(
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
                    allow_legacy_proxy_cache_creation as i64,
                ],
                |row| row.get::<_, bool>(0),
            )
        })
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

static SUSPECTED_CODEX_DUPLICATE_SQL: LazyLock<String> = LazyLock::new(|| {
    let data_source = data_source_expr("l");
    format!(
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
    )
});

pub(crate) fn has_suspected_codex_session_duplicate(
    conn: &Connection,
    request_id: &str,
    key: &DedupKey,
) -> Result<bool, AppError> {
    conn.prepare_cached(&SUSPECTED_CODEX_DUPLICATE_SQL)
        .and_then(|mut stmt| {
            stmt.query_row(
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
        })
        .map_err(|error| AppError::Database(format!("查询疑似重复 Codex 会话用量失败: {error}")))
}

#[derive(Debug, Clone, Default)]
struct RollupDateBounds {
    start: Option<String>,
    end: Option<String>,
    is_empty: bool,
}

fn local_datetime_from_timestamp(ts: i64) -> Result<chrono::DateTime<Local>, AppError> {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .ok_or_else(|| AppError::Database(format!("无法解析本地时间戳: {ts}")))
}

fn compute_rollup_date_bounds(
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<RollupDateBounds, AppError> {
    let start = match start_ts {
        Some(ts) => {
            let local = local_datetime_from_timestamp(ts)?;
            let day = local.date_naive();
            if local.time().num_seconds_from_midnight() == 0 {
                Some(day.format("%Y-%m-%d").to_string())
            } else {
                day.succ_opt()
                    .map(|next| next.format("%Y-%m-%d").to_string())
            }
        }
        None => None,
    };

    let end = match end_ts {
        Some(ts) => {
            let local = local_datetime_from_timestamp(ts)?;
            let day = local.date_naive();
            if local.time().hour() == 23 && local.time().minute() == 59 {
                Some(day.format("%Y-%m-%d").to_string())
            } else {
                day.pred_opt()
                    .map(|prev| prev.format("%Y-%m-%d").to_string())
            }
        }
        None => None,
    };

    let is_empty = matches!((&start, &end), (Some(start), Some(end)) if start > end);

    Ok(RollupDateBounds {
        start,
        end,
        is_empty,
    })
}

fn push_rollup_date_filters(
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
    column: &str,
    bounds: &RollupDateBounds,
) {
    if bounds.is_empty {
        conditions.push("1 = 0".to_string());
        return;
    }

    if let Some(start) = &bounds.start {
        conditions.push(format!("{column} >= ?"));
        params.push(Box::new(start.clone()));
    }

    if let Some(end) = &bounds.end {
        conditions.push(format!("{column} <= ?"));
        params.push(Box::new(end.clone()));
    }
}

fn local_day_start_rfc3339(day: NaiveDate) -> String {
    let local_midnight = day
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(dt) => Some(dt),
            chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest),
            chrono::LocalResult::None => None,
        })
        .unwrap_or_else(Local::now);

    local_midnight.to_rfc3339()
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

        // Build detail WHERE clause
        let mut conditions = vec![effective_usage_log_filter_for_range(
            &conn, "l", start_date, end_date,
        )?];
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            conditions.push("l.created_at >= ?".to_string());
            params_vec.push(Box::new(start));
        }
        if let Some(end) = end_date {
            conditions.push("l.created_at <= ?".to_string());
            params_vec.push(Box::new(end));
        }
        if let Some(at) = app_type {
            conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            params_vec.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut conditions,
            &mut params_vec,
            "l",
            "p",
            provider_name,
            model,
        );

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        // Only include rolled-up rows for full local days that are fully covered by the range.
        let mut rollup_conditions: Vec<String> = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;

        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );

        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
        let sql = format!(
            "SELECT
                COALESCE(d.total_requests, 0) + COALESCE(r.total_requests, 0),
                COALESCE(d.total_cost, 0) + COALESCE(r.total_cost, 0),
                COALESCE(d.total_input_tokens, 0) + COALESCE(r.total_input_tokens, 0),
                COALESCE(d.total_output_tokens, 0) + COALESCE(r.total_output_tokens, 0),
                COALESCE(d.total_cache_creation_tokens, 0) + COALESCE(r.total_cache_creation_tokens, 0),
                COALESCE(d.total_cache_read_tokens, 0) + COALESCE(r.total_cache_read_tokens, 0),
                COALESCE(d.success_count, 0) + COALESCE(r.success_count, 0)
            FROM
                (SELECT
                    COUNT(*) as total_requests,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM({fresh_input_detail}), 0) as total_input_tokens,
                    COALESCE(SUM(l.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as total_cache_creation_tokens,
                    COALESCE(SUM(l.cache_read_tokens), 0) as total_cache_read_tokens,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
                 FROM proxy_request_logs l {detail_join} {where_clause}) d,
                (SELECT
                    COALESCE(SUM(r.request_count), 0) as total_requests,
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM({fresh_input_rollup}), 0) as total_input_tokens,
                    COALESCE(SUM(r.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(r.cache_creation_tokens), 0) as total_cache_creation_tokens,
                    COALESCE(SUM(r.cache_read_tokens), 0) as total_cache_read_tokens,
                    COALESCE(SUM(r.success_count), 0) as success_count
                 FROM usage_daily_rollups r {rollup_join} {rollup_where}) r"
        );

        // Combine params: detail params first, then rollup params
        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = params_vec;
        all_params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

        let result = conn.query_row(&sql, param_refs.as_slice(), usage_summary_from_row)?;

        Ok(result)
    }

    /// 单个会话的用量汇总（会话阅读页头部）：总 Token 和花费的口径同 Dashboard。
    ///
    /// 只数会话日志导入的行：它们带客户端自己的会话 ID，经不经过路由都会导入；
    /// 代理行的会话 ID 是代理侧推断的，未必对得上，两边都数还会重复。
    /// 明细 30 天后汇总进按天表并删除（按天表没有会话维度），更早的会话查不到。
    pub fn get_session_usage_summary(
        &self,
        app_type: &str,
        session_id: &str,
    ) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let fresh_input = fresh_input_sql("l");
        let app_type_expr = folded_app_type_sql("l.app_type");
        let data_source = data_source_expr("l");
        let sql = format!(
            "SELECT
                COUNT(*),
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh_input}), 0),
                COALESCE(SUM(l.output_tokens), 0),
                COALESCE(SUM(l.cache_creation_tokens), 0),
                COALESCE(SUM(l.cache_read_tokens), 0),
                COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0)
             FROM proxy_request_logs l
             WHERE l.session_id = ?1 AND {app_type_expr} = ?2 AND {data_source} <> 'proxy'"
        );

        let result = conn.query_row(
            &sql,
            rusqlite::params![session_id, app_type],
            usage_summary_from_row,
        )?;

        Ok(result)
    }

    /// 按 app_type 维度拆分的使用量汇总，用于 Dashboard 的分应用展示条。
    /// 返回所有有数据的 app_type，按 real_total_tokens 降序。
    ///
    /// Single SQL with `GROUP BY app_type` — avoids the N+1 round-trip that
    /// would result from invoking `get_usage_summary` once per app_type.
    pub fn get_usage_summary_by_app(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<UsageSummaryByApp>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = vec![effective_usage_log_filter_for_range(
            &conn, "l", start_date, end_date,
        )?];
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_date {
            detail_conditions.push("l.created_at >= ?".to_string());
            detail_params.push(Box::new(start));
        }
        if let Some(end) = end_date {
            detail_conditions.push("l.created_at <= ?".to_string());
            detail_params.push(Box::new(end));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = format!("WHERE {}", detail_conditions.join(" AND "));
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        let mut rollup_conditions: Vec<String> = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
        // 折叠 claude-desktop → claude：内层投影成同一桶名，外层 GROUP BY 自然合并。
        let detail_app_type = folded_app_type_sql("l.app_type");
        let rollup_app_type = folded_app_type_sql("r.app_type");

        let sql = format!(
            "SELECT app_type,
                SUM(req_count) as req_count,
                SUM(cost) as cost,
                SUM(input_t) as input_t,
                SUM(output_t) as output_t,
                SUM(cache_create_t) as cache_create_t,
                SUM(cache_read_t) as cache_read_t,
                SUM(success_count) as success_count
            FROM (
                SELECT {detail_app_type} as app_type,
                    COUNT(*) as req_count,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as cost,
                    COALESCE(SUM({fresh_input_detail}), 0) as input_t,
                    COALESCE(SUM(l.output_tokens), 0) as output_t,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as cache_create_t,
                    COALESCE(SUM(l.cache_read_tokens), 0) as cache_read_t,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
                FROM proxy_request_logs l {detail_join} {detail_where}
                GROUP BY l.app_type
                UNION ALL
                SELECT {rollup_app_type} as app_type,
                    COALESCE(SUM(r.request_count), 0),
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM({fresh_input_rollup}), 0),
                    COALESCE(SUM(r.output_tokens), 0),
                    COALESCE(SUM(r.cache_creation_tokens), 0),
                    COALESCE(SUM(r.cache_read_tokens), 0),
                    COALESCE(SUM(r.success_count), 0)
                FROM usage_daily_rollups r {rollup_join} {rollup_where}
                GROUP BY r.app_type
            )
            GROUP BY app_type"
        );

        let mut combined: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        combined.extend(rollup_params);
        let refs: Vec<&dyn rusqlite::ToSql> = combined.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            let app_type: String = row.get(0)?;
            let total_requests: i64 = row.get(1)?;
            let total_cost: f64 = row.get(2)?;
            let total_input_tokens: i64 = row.get(3)?;
            let total_output_tokens: i64 = row.get(4)?;
            let total_cache_creation_tokens: i64 = row.get(5)?;
            let total_cache_read_tokens: i64 = row.get(6)?;
            let success_count: i64 = row.get(7)?;

            let success_rate = if total_requests > 0 {
                (success_count as f32 / total_requests as f32) * 100.0
            } else {
                0.0
            };
            let (real_total_tokens, cache_hit_rate) = derive_real_total_and_hit_rate(
                total_input_tokens as u64,
                total_output_tokens as u64,
                total_cache_creation_tokens as u64,
                total_cache_read_tokens as u64,
            );

            Ok(UsageSummaryByApp {
                app_type,
                summary: UsageSummary {
                    total_requests: total_requests as u64,
                    total_cost: format!("{total_cost:.6}"),
                    total_input_tokens: total_input_tokens as u64,
                    total_output_tokens: total_output_tokens as u64,
                    total_cache_creation_tokens: total_cache_creation_tokens as u64,
                    total_cache_read_tokens: total_cache_read_tokens as u64,
                    success_rate,
                    real_total_tokens,
                    cache_hit_rate,
                },
            })
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            let item = row?;
            if item.summary.total_requests == 0 && item.summary.real_total_tokens == 0 {
                continue;
            }
            summaries.push(item);
        }
        summaries.sort_by(|a, b| {
            b.summary
                .real_total_tokens
                .cmp(&a.summary.real_total_tokens)
        });
        Ok(summaries)
    }

    /// 获取每日趋势（滑动窗口，<=24h 按小时，>24h 按天，窗口与汇总一致）
    pub fn get_daily_trends(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<DailyStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let end_ts = end_date.unwrap_or_else(|| Local::now().timestamp());
        let mut start_ts = start_date.unwrap_or_else(|| end_ts - 24 * 60 * 60);

        if start_ts >= end_ts {
            start_ts = end_ts - 24 * 60 * 60;
        }

        let duration = end_ts - start_ts;
        if duration <= 24 * 60 * 60 {
            let bucket_seconds: i64 = 60 * 60;
            let mut bucket_count: i64 = if duration <= 0 {
                1
            } else {
                (duration + bucket_seconds - 1) / bucket_seconds
            };

            if bucket_count < 1 {
                bucket_count = 1;
            }

            let mut extra_conditions: Vec<String> = Vec::new();
            let mut extra_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(at) = app_type {
                extra_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
                extra_params.push(Box::new(at.to_string()));
            }
            push_provider_model_filters(
                &mut extra_conditions,
                &mut extra_params,
                "l",
                "p",
                provider_name,
                model,
            );
            let extra_filter = extra_conditions
                .iter()
                .map(|c| format!("AND {c}"))
                .collect::<Vec<_>>()
                .join(" ");
            let detail_join = if provider_name.is_some() {
                providers_join("l", "p")
            } else {
                String::new()
            };

            let effective_filter =
                effective_usage_log_filter_for_range(&conn, "l", Some(start_ts), Some(end_ts))?;
            let fresh_input = fresh_input_sql("l");
            let sql = format!(
                "SELECT
                    CAST((l.created_at - ?1) / ?3 AS INTEGER) as bucket_idx,
                    COUNT(*) as request_count,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM({fresh_input} + l.output_tokens), 0) as total_tokens,
                    COALESCE(SUM({fresh_input}), 0) as total_input_tokens,
                    COALESCE(SUM(l.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as total_cache_creation_tokens,
                    COALESCE(SUM(l.cache_read_tokens), 0) as total_cache_read_tokens
                FROM proxy_request_logs l {detail_join}
                WHERE l.created_at >= ?1 AND l.created_at <= ?2
                  AND {effective_filter} {extra_filter}
                GROUP BY bucket_idx
                ORDER BY bucket_idx ASC"
            );

            let mut stmt = conn.prepare(&sql)?;
            let row_mapper = |row: &rusqlite::Row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    DailyStats {
                        date: String::new(),
                        request_count: row.get::<_, i64>(1)? as u64,
                        total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                        total_tokens: row.get::<_, i64>(3)? as u64,
                        total_input_tokens: row.get::<_, i64>(4)? as u64,
                        total_output_tokens: row.get::<_, i64>(5)? as u64,
                        total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
                        total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
                    },
                ))
            };

            let mut map: HashMap<i64, DailyStats> = HashMap::new();

            let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                Box::new(start_ts),
                Box::new(end_ts),
                Box::new(bucket_seconds),
            ];
            all_params.extend(extra_params);
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                all_params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_mapper)?;
            for row in rows {
                let (mut bucket_idx, stat) = row?;
                if bucket_idx < 0 {
                    continue;
                }
                if bucket_idx >= bucket_count {
                    bucket_idx = bucket_count - 1;
                }
                map.insert(bucket_idx, stat);
            }

            let mut stats = Vec::with_capacity(bucket_count as usize);
            for i in 0..bucket_count {
                let bucket_start_ts = start_ts + i * bucket_seconds;
                let bucket_start = local_datetime_from_timestamp(bucket_start_ts)?;
                let date = bucket_start.to_rfc3339();

                if let Some(mut stat) = map.remove(&i) {
                    stat.date = date;
                    stats.push(stat);
                } else {
                    stats.push(DailyStats {
                        date,
                        request_count: 0,
                        total_cost: "0.000000".to_string(),
                        total_tokens: 0,
                        total_input_tokens: 0,
                        total_output_tokens: 0,
                        total_cache_creation_tokens: 0,
                        total_cache_read_tokens: 0,
                    });
                }
            }

            return Ok(stats);
        }

        let start_day = local_datetime_from_timestamp(start_ts)?.date_naive();
        let end_day = local_datetime_from_timestamp(end_ts)?.date_naive();
        let bucket_count = (end_day.signed_duration_since(start_day).num_days() + 1) as usize;

        let mut extra_conditions: Vec<String> = Vec::new();
        let mut extra_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(at) = app_type {
            extra_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            extra_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut extra_conditions,
            &mut extra_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let extra_filter = extra_conditions
            .iter()
            .map(|c| format!("AND {c}"))
            .collect::<Vec<_>>()
            .join(" ");
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        let effective_filter =
            effective_usage_log_filter_for_range(&conn, "l", Some(start_ts), Some(end_ts))?;
        let fresh_input = fresh_input_sql("l");
        let detail_sql = format!(
            "SELECT
                date(l.created_at, 'unixepoch', 'localtime') as bucket_date,
                COUNT(*) as request_count,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM({fresh_input} + l.output_tokens), 0) as total_tokens,
                COALESCE(SUM({fresh_input}), 0) as total_input_tokens,
                COALESCE(SUM(l.output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(l.cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(l.cache_read_tokens), 0) as total_cache_read_tokens
            FROM proxy_request_logs l {detail_join}
            WHERE l.created_at >= ?1 AND l.created_at <= ?2
              AND {effective_filter} {extra_filter}
            GROUP BY bucket_date
            ORDER BY bucket_date ASC"
        );

        let mut detail_stmt = conn.prepare(&detail_sql)?;
        let detail_row_mapper = |row: &rusqlite::Row| {
            Ok((
                row.get::<_, String>(0)?,
                DailyStats {
                    date: String::new(),
                    request_count: row.get::<_, i64>(1)? as u64,
                    total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                    total_tokens: row.get::<_, i64>(3)? as u64,
                    total_input_tokens: row.get::<_, i64>(4)? as u64,
                    total_output_tokens: row.get::<_, i64>(5)? as u64,
                    total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
                    total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
                },
            ))
        };

        let mut map: HashMap<NaiveDate, DailyStats> = HashMap::new();
        let mut detail_all_params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(start_ts), Box::new(end_ts)];
        detail_all_params.extend(extra_params);
        let detail_param_refs: Vec<&dyn rusqlite::ToSql> =
            detail_all_params.iter().map(|p| p.as_ref()).collect();
        let detail_rows = detail_stmt.query_map(detail_param_refs.as_slice(), detail_row_mapper)?;

        for row in detail_rows {
            let (bucket_date, stat) = row?;
            let date = NaiveDate::parse_from_str(&bucket_date, "%Y-%m-%d")
                .map_err(|err| AppError::Database(format!("解析趋势日期失败: {err}")))?;
            map.insert(date, stat);
        }

        let rollup_bounds = compute_rollup_date_bounds(Some(start_ts), Some(end_ts))?;
        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );

        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        let fresh_input_rollup = fresh_input_sql("r");
        let rollup_sql = format!(
            "SELECT
                r.date,
                COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh_input_rollup} + r.output_tokens), 0),
                COALESCE(SUM({fresh_input_rollup}), 0),
                COALESCE(SUM(r.output_tokens), 0),
                COALESCE(SUM(r.cache_creation_tokens), 0),
                COALESCE(SUM(r.cache_read_tokens), 0)
            FROM usage_daily_rollups r {rollup_join}
            {rollup_where}
            GROUP BY r.date
            ORDER BY r.date ASC"
        );

        let mut rollup_stmt = conn.prepare(&rollup_sql)?;
        let rollup_row_mapper = |row: &rusqlite::Row| {
            Ok((
                row.get::<_, String>(0)?,
                (
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                    row.get::<_, i64>(7)? as u64,
                ),
            ))
        };
        let rollup_param_refs: Vec<&dyn rusqlite::ToSql> =
            rollup_params.iter().map(|param| param.as_ref()).collect();
        let rollup_rows = rollup_stmt.query_map(rollup_param_refs.as_slice(), rollup_row_mapper)?;

        for row in rollup_rows {
            let (bucket_date, (req, cost, tok, inp, out, cc, cr)) = row?;
            let date = NaiveDate::parse_from_str(&bucket_date, "%Y-%m-%d")
                .map_err(|err| AppError::Database(format!("解析 rollup 趋势日期失败: {err}")))?;
            let entry = map.entry(date).or_insert_with(|| DailyStats {
                date: String::new(),
                request_count: 0,
                total_cost: "0.000000".to_string(),
                total_tokens: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_cache_creation_tokens: 0,
                total_cache_read_tokens: 0,
            });
            entry.request_count += req;
            let existing_cost: f64 = entry.total_cost.parse().unwrap_or(0.0);
            entry.total_cost = format!("{:.6}", existing_cost + cost);
            entry.total_tokens += tok;
            entry.total_input_tokens += inp;
            entry.total_output_tokens += out;
            entry.total_cache_creation_tokens += cc;
            entry.total_cache_read_tokens += cr;
        }

        let mut stats = Vec::with_capacity(bucket_count);
        let mut current_day = start_day;
        for _ in 0..bucket_count {
            let date = local_day_start_rfc3339(current_day);

            if let Some(mut stat) = map.remove(&current_day) {
                stat.date = date;
                stats.push(stat);
            } else {
                stats.push(DailyStats {
                    date,
                    request_count: 0,
                    total_cost: "0.000000".to_string(),
                    total_tokens: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_cache_creation_tokens: 0,
                    total_cache_read_tokens: 0,
                });
            }

            current_day = current_day.succ_opt().unwrap_or(current_day);
        }

        Ok(stats)
    }

    /// 最早有用量记录的那一天（本地日期 `YYYY-MM-DD`），明细和日聚合一起看；没有记录时为 `None`。
    ///
    /// 给「全部」的按年热力图定起始年份用，所以不套跨源去重：被去重掉的会话行
    /// 和对应的代理行只差几分钟，不会改变最早的日期。
    pub fn get_first_usage_date(
        &self,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Option<String>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = Vec::new();
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(at) = app_type {
            detail_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            detail_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = if detail_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", detail_conditions.join(" AND "))
        };
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };
        let detail_param_refs: Vec<&dyn rusqlite::ToSql> =
            detail_params.iter().map(|p| p.as_ref()).collect();
        let first_detail_ts: Option<i64> = conn.query_row(
            &format!(
                "SELECT MIN(l.created_at) FROM proxy_request_logs l {detail_join} {detail_where}"
            ),
            detail_param_refs.as_slice(),
            |row| row.get(0),
        )?;
        let first_detail = first_detail_ts
            .map(|ts| local_datetime_from_timestamp(ts).map(|dt| dt.date_naive()))
            .transpose()?;

        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };
        let rollup_param_refs: Vec<&dyn rusqlite::ToSql> =
            rollup_params.iter().map(|p| p.as_ref()).collect();
        let first_rollup_raw: Option<String> = conn.query_row(
            &format!("SELECT MIN(r.date) FROM usage_daily_rollups r {rollup_join} {rollup_where}"),
            rollup_param_refs.as_slice(),
            |row| row.get(0),
        )?;
        let first_rollup = first_rollup_raw
            .map(|raw| {
                NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
                    .map_err(|err| AppError::Database(format!("解析 rollup 日期失败: {err}")))
            })
            .transpose()?;

        let first = match (first_detail, first_rollup) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Ok(first.map(|day| day.format("%Y-%m-%d").to_string()))
    }

    /// 获取 Provider 统计
    pub fn get_provider_stats(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<ProviderStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = vec![effective_usage_log_filter_for_range(
            &conn, "l", start_date, end_date,
        )?];
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_date {
            detail_conditions.push("l.created_at >= ?".to_string());
            detail_params.push(Box::new(start));
        }
        if let Some(end) = end_date {
            detail_conditions.push("l.created_at <= ?".to_string());
            detail_params.push(Box::new(end));
        }
        if let Some(at) = app_type {
            detail_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            detail_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = if detail_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", detail_conditions.join(" AND "))
        };

        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };

        // UNION detail logs + rollup data, then aggregate
        let detail_pname = provider_name_coalesce("l", "p");
        let rollup_pname = provider_name_coalesce("r", "p2");
        let real_total_detail = real_total_tokens_sql("l");
        let real_total_rollup = real_total_tokens_sql("r");
        let speed_ok = speed_eligible_sql("l");
        let est_ok = speed_estimate_eligible_sql("l");
        let sql = format!(
            "SELECT
                provider_id, app_type, provider_name,
                SUM(request_count) as request_count,
                SUM(total_tokens) as total_tokens,
                SUM(total_cost) as total_cost,
                SUM(success_count) as success_count,
                CASE WHEN SUM(request_count) > 0
                    THEN SUM(latency_sum) / SUM(request_count)
                    ELSE 0 END as avg_latency,
                SUM(speed_output) as speed_output,
                SUM(speed_gen_ms) as speed_gen_ms,
                SUM(est_output) as est_output,
                SUM(est_ms) as est_ms
            FROM (
                SELECT l.provider_id, l.app_type,
                    {detail_pname} as provider_name,
                    COUNT(*) as request_count,
                    COALESCE(SUM({real_total_detail}), 0) as total_tokens,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count,
                    COALESCE(SUM(l.latency_ms), 0) as latency_sum,
                    COALESCE(SUM(CASE WHEN {speed_ok} THEN l.output_tokens ELSE 0 END), 0) as speed_output,
                    COALESCE(SUM(CASE WHEN {speed_ok} THEN l.latency_ms - l.first_token_ms ELSE 0 END), 0) as speed_gen_ms,
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.output_tokens ELSE 0 END), 0) as est_output,
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.latency_ms ELSE 0 END), 0) as est_ms
                FROM proxy_request_logs l
                LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
                {detail_where}
                GROUP BY l.provider_id, l.app_type
                UNION ALL
                SELECT r.provider_id, r.app_type,
                    {rollup_pname} as provider_name,
                    COALESCE(SUM(r.request_count), 0),
                    COALESCE(SUM({real_total_rollup}), 0),
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM(r.success_count), 0),
                    COALESCE(SUM(r.avg_latency_ms * r.request_count), 0),
                    0,
                    0,
                    0,
                    0
                FROM usage_daily_rollups r
                LEFT JOIN providers p2 ON r.provider_id = p2.id AND r.app_type = p2.app_type
                {rollup_where}
                GROUP BY r.provider_id, r.app_type
            )
            GROUP BY provider_id, app_type
            ORDER BY total_cost DESC"
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let row_mapper = |row: &rusqlite::Row| {
            let request_count: i64 = row.get(3)?;
            let success_count: i64 = row.get(6)?;
            let success_rate = if request_count > 0 {
                (success_count as f32 / request_count as f32) * 100.0
            } else {
                0.0
            };

            Ok(ProviderStats {
                provider_id: row.get(0)?,
                app_type: row.get(1)?,
                provider_name: row.get(2)?,
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                total_cost: format!("{:.6}", row.get::<_, f64>(5)?),
                success_rate,
                avg_latency_ms: row.get::<_, f64>(7)? as u64,
                speed_output_tokens: row.get::<_, i64>(8)?.max(0) as u64,
                speed_generation_ms: row.get::<_, i64>(9)?.max(0) as u64,
                est_speed_output_tokens: row.get::<_, i64>(10)?.max(0) as u64,
                est_speed_duration_ms: row.get::<_, i64>(11)?.max(0) as u64,
            })
        };

        let rows = stmt.query_map(param_refs.as_slice(), row_mapper)?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取模型统计
    pub fn get_model_stats(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<ModelStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = vec![effective_usage_log_filter_for_range(
            &conn, "l", start_date, end_date,
        )?];
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_date {
            detail_conditions.push("l.created_at >= ?".to_string());
            detail_params.push(Box::new(start));
        }
        if let Some(end) = end_date {
            detail_conditions.push("l.created_at <= ?".to_string());
            detail_params.push(Box::new(end));
        }
        if let Some(at) = app_type {
            detail_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            detail_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = if detail_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", detail_conditions.join(" AND "))
        };
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        // UNION detail logs + rollup data
        //
        // 分组键用「有效计价模型」：pricing_model 非空时优先（成本就是按它的
        // 定价算的，金额与定价表自洽），NULL/'' 回落 model。默认 response 计价
        // 模式下两者相同，行为不变；request 模式 + 路由接管下，钱挂在实际计价
        // 基准名下，而不是上游回显/客户端别名名下。
        let real_total_detail = real_total_tokens_sql("l");
        let real_total_rollup = real_total_tokens_sql("r");
        let detail_model = effective_model_sql("l");
        let rollup_model = effective_model_sql("r");
        let speed_ok = speed_eligible_sql("l");
        let est_ok = speed_estimate_eligible_sql("l");
        let sql = format!(
            "SELECT
                model,
                SUM(request_count) as request_count,
                SUM(total_tokens) as total_tokens,
                SUM(total_cost) as total_cost,
                SUM(success_count) as success_count,
                SUM(speed_output) as speed_output,
                SUM(speed_gen_ms) as speed_gen_ms,
                SUM(est_output) as est_output,
                SUM(est_ms) as est_ms
            FROM (
                SELECT {detail_model} as model,
                    COUNT(*) as request_count,
                    COALESCE(SUM({real_total_detail}), 0) as total_tokens,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count,
                    COALESCE(SUM(CASE WHEN {speed_ok} THEN l.output_tokens ELSE 0 END), 0) as speed_output,
                    COALESCE(SUM(CASE WHEN {speed_ok} THEN l.latency_ms - l.first_token_ms ELSE 0 END), 0) as speed_gen_ms,
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.output_tokens ELSE 0 END), 0) as est_output,
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.latency_ms ELSE 0 END), 0) as est_ms
                FROM proxy_request_logs l
                {detail_join}
                {detail_where}
                GROUP BY {detail_model}
                UNION ALL
                SELECT {rollup_model},
                    COALESCE(SUM(r.request_count), 0),
                    COALESCE(SUM({real_total_rollup}), 0),
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM(r.success_count), 0),
                    0,
                    0,
                    0,
                    0
                FROM usage_daily_rollups r
                {rollup_join}
                {rollup_where}
                GROUP BY {rollup_model}
            )
            GROUP BY model
            ORDER BY total_cost DESC"
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let row_mapper = |row: &rusqlite::Row| {
            let request_count: i64 = row.get(1)?;
            let total_cost: f64 = row.get(3)?;
            let avg_cost = if request_count > 0 {
                total_cost / request_count as f64
            } else {
                0.0
            };
            let success_count: i64 = row.get(4)?;
            let success_rate = if request_count > 0 {
                (success_count as f32 / request_count as f32) * 100.0
            } else {
                0.0
            };

            Ok(ModelStats {
                model: row.get(0)?,
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(2)? as u64,
                total_cost: format!("{total_cost:.6}"),
                avg_cost_per_request: format!("{avg_cost:.6}"),
                success_rate,
                speed_output_tokens: row.get::<_, i64>(5)?.max(0) as u64,
                speed_generation_ms: row.get::<_, i64>(6)?.max(0) as u64,
                est_speed_output_tokens: row.get::<_, i64>(7)?.max(0) as u64,
                est_speed_duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
            })
        };

        let rows = stmt.query_map(param_refs.as_slice(), row_mapper)?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取请求日志列表（分页）
    pub fn get_request_logs(
        &self,
        filters: &LogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedLogs, AppError> {
        let conn = lock_conn!(self.conn);

        let mut conditions = vec![effective_usage_log_filter_for_range(
            &conn,
            "l",
            filters.start_date,
            filters.end_date,
        )?];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref app_type) = filters.app_type {
            // 仅过滤口径折叠 claude-desktop→claude；行投影仍返回原始 app_type，
            // 详情面板据此展示真实入口（路由接管账单审计需要）。
            conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            params.push(Box::new(app_type.clone()));
        }
        // 与 Dashboard 顶部下拉筛选同口径：Provider 按展示名精确匹配（会话占位
        // 行如 "Claude (Session)" 也能命中），模型按有效计价模型匹配。
        push_provider_model_filters(
            &mut conditions,
            &mut params,
            "l",
            "p",
            filters.provider_name.as_deref(),
            filters.model.as_deref(),
        );
        if let Some(status) = filters.status_code {
            conditions.push("l.status_code = ?".to_string());
            params.push(Box::new(status as i64));
        }
        if let Some(start) = filters.start_date {
            conditions.push("l.created_at >= ?".to_string());
            params.push(Box::new(start));
        }
        if let Some(end) = filters.end_date {
            conditions.push("l.created_at <= ?".to_string());
            params.push(Box::new(end));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // 获取总数
        let count_sql = format!(
            "SELECT COUNT(*) FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}"
        );
        let mut count_cache = lock_conn!(self.log_count_cache);
        let total: u32 = match cached_log_count(&count_cache, &conn, filters)? {
            Some(total) => total,
            None => {
                let count_params: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let total = conn.query_row(&count_sql, count_params.as_slice(), |row| {
                    row.get::<_, i64>(0).map(|v| v as u32)
                })?;
                *count_cache = Some(LogCountCache {
                    key: log_count_cache_key(filters),
                    end_date: filters.end_date,
                    changes: connection_total_changes(&conn)?,
                    computed_at: std::time::Instant::now(),
                    total,
                });
                total
            }
        };
        drop(count_cache);

        // 获取数据
        let offset = page * page_size;
        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));

        let logs_pname = provider_name_coalesce("l", "p");
        let sql = format!(
            "SELECT l.request_id, l.provider_id, {logs_pname} as provider_name, l.app_type, l.model,
                    l.request_model, l.cost_multiplier,
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at, l.data_source, l.pricing_model,
                    l.input_token_semantics
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             {where_clause}
             ORDER BY l.created_at DESC
             LIMIT ? OFFSET ?"
        );

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), row_to_request_log_detail)?;

        let mut logs = Vec::new();
        let mut pricing_cache = HashMap::new();

        for row in rows {
            let mut log = row?;
            Self::maybe_backfill_log_costs(&conn, &mut log, &mut pricing_cache)?;
            logs.push(log);
        }

        Ok(PaginatedLogs {
            data: logs,
            total,
            page,
            page_size,
        })
    }

    /// 获取单个请求详情
    pub fn get_request_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);

        let detail_pname = provider_name_coalesce("l", "p");
        let detail_sql = format!(
            "SELECT l.request_id, l.provider_id, {detail_pname} as provider_name, l.app_type, l.model,
                    l.request_model, l.cost_multiplier,
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at, l.data_source, l.pricing_model,
                    l.input_token_semantics
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE l.request_id = ?"
        );
        let result = conn.query_row(&detail_sql, [request_id], row_to_request_log_detail);

        match result {
            Ok(mut detail) => {
                let mut pricing_cache = HashMap::new();
                Self::maybe_backfill_log_costs(&conn, &mut detail, &mut pricing_cache)?;
                Ok(Some(detail))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 检查 Provider 使用限额
    pub fn check_provider_limits(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, AppError> {
        let conn = lock_conn!(self.conn);

        // 获取 provider 的限额设置
        let (limit_daily, limit_monthly) = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = ? AND app_type = ?",
                params![provider_id, app_type],
                |row| {
                    let meta_str: String = row.get(0)?;
                    Ok(meta_str)
                },
            )
            .ok()
            .and_then(|meta_str| serde_json::from_str::<serde_json::Value>(&meta_str).ok())
            .map(|meta| {
                let daily = meta
                    .get("limitDailyUsd")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                let monthly = meta
                    .get("limitMonthlyUsd")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                (daily, monthly)
            })
            .unwrap_or((None, None));

        // 计算今日使用量 (detail logs + rollup)
        let daily_usage: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(cost), 0) FROM (
                    SELECT CAST(total_cost_usd AS REAL) as cost
                    FROM proxy_request_logs
                    WHERE provider_id = ? AND app_type = ?
                      AND date(datetime(created_at, 'unixepoch', 'localtime')) = date('now', 'localtime')
                    UNION ALL
                    SELECT CAST(total_cost_usd AS REAL)
                    FROM usage_daily_rollups
                    WHERE provider_id = ? AND app_type = ?
                      AND date = date('now', 'localtime')
                )",
                params![provider_id, app_type, provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        // 计算本月使用量 (detail logs + rollup)
        let monthly_usage: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(cost), 0) FROM (
                    SELECT CAST(total_cost_usd AS REAL) as cost
                    FROM proxy_request_logs
                    WHERE provider_id = ? AND app_type = ?
                      AND strftime('%Y-%m', datetime(created_at, 'unixepoch', 'localtime')) = strftime('%Y-%m', 'now', 'localtime')
                    UNION ALL
                    SELECT CAST(total_cost_usd AS REAL)
                    FROM usage_daily_rollups
                    WHERE provider_id = ? AND app_type = ?
                      AND strftime('%Y-%m', date) = strftime('%Y-%m', 'now', 'localtime')
                )",
                params![provider_id, app_type, provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let daily_exceeded = limit_daily
            .map(|limit| daily_usage >= limit)
            .unwrap_or(false);
        let monthly_exceeded = limit_monthly
            .map(|limit| monthly_usage >= limit)
            .unwrap_or(false);

        Ok(ProviderLimitStatus {
            provider_id: provider_id.to_string(),
            daily_usage: format!("{daily_usage:.6}"),
            daily_limit: limit_daily.map(|l| format!("{l:.2}")),
            daily_exceeded,
            monthly_usage: format!("{monthly_usage:.6}"),
            monthly_limit: limit_monthly.map(|l| format!("{l:.2}")),
            monthly_exceeded,
        })
    }
}

/// Provider 限额状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimitStatus {
    pub provider_id: String,
    pub daily_usage: String,
    pub daily_limit: Option<String>,
    pub daily_exceeded: bool,
    pub monthly_usage: String,
    pub monthly_limit: Option<String>,
    pub monthly_exceeded: bool,
}

#[derive(Clone)]
struct PricingInfo {
    input: rust_decimal::Decimal,
    output: rust_decimal::Decimal,
    cache_read: rust_decimal::Decimal,
    cache_creation: rust_decimal::Decimal,
}

impl Database {
    /// Recalculate stored zero-cost usage rows once pricing becomes available.
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
                data_source TEXT,
                input_token_semantics INTEGER NOT NULL DEFAULT 0
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

    /// 日志总数缓存：没有写入时复用；插入、删除或结束时间推后有新行时都要重数。
    #[test]
    fn test_request_log_total_cache_invalidates_on_writes() -> Result<(), AppError> {
        let db = Database::memory()?;
        let insert = |id: &str, created_at: i64| -> Result<(), AppError> {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                id,
                "claude",
                "anthropic",
                "claude-sonnet-4-5",
                "proxy",
                created_at,
                10,
                2,
                0,
                0,
                200,
                "0.01",
            )
        };
        insert("a", 1_000)?;
        insert("b", 2_000)?;

        let mut filters = LogFilters {
            start_date: Some(0),
            end_date: Some(5_000),
            ..LogFilters::default()
        };
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);
        // 没有写入：复用缓存，结果不变
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);

        // 插入后必须重数
        insert("c", 3_000)?;
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 3);

        // 删除后也要重数
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM proxy_request_logs WHERE request_id = 'a'", [])?;
        }
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);

        // 结束时间往后推：新增区间里没有行就复用，有行（之前就存在的未来时间行）就重数
        filters.end_date = Some(6_000);
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);
        insert("d", 9_000)?;
        filters.end_date = Some(5_000);
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 2);
        filters.end_date = Some(10_000);
        assert_eq!(db.get_request_logs(&filters, 0, 10)?.total, 3);

        Ok(())
    }

    /// 范围版去重条件不管选哪种写法，结果都必须和逐行 EXISTS 的原写法一致。
    #[test]
    fn test_range_filter_matches_original_dedup() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        // request_id, app_type, model, input, output, cache_read, cache_creation,
        // status_code, created_at, data_source
        type LogRow = (
            &'static str,
            &'static str,
            &'static str,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            Option<&'static str>,
        );
        let rows: &[LogRow] = &[
            // 和 proxy-1 重复（应被去掉）
            (
                "p1",
                "claude",
                "opus",
                10,
                2,
                1,
                5,
                200,
                1000,
                Some("proxy"),
            ),
            (
                "s1",
                "claude",
                "opus",
                10,
                2,
                1,
                5,
                200,
                1100,
                Some("session_log"),
            ),
            // claude-desktop 的代理行也能去掉 claude 会话行
            (
                "p2",
                "claude-desktop",
                "sonnet",
                20,
                3,
                0,
                0,
                200,
                5000,
                Some("proxy"),
            ),
            (
                "s2",
                "claude",
                "SONNET",
                20,
                3,
                0,
                0,
                200,
                5300,
                Some("session_log"),
            ),
            // codex 会话没有 cache_creation：0 视作未知
            ("p3", "codex", "gpt", 30, 4, 2, 9, 200, 9000, Some("proxy")),
            (
                "s3",
                "codex",
                "gpt",
                30,
                4,
                2,
                0,
                200,
                9100,
                Some("codex_session"),
            ),
            // 超出 10 分钟窗口：保留
            (
                "s4",
                "claude",
                "opus",
                10,
                2,
                1,
                5,
                200,
                1000 + 601,
                Some("session_log"),
            ),
            // 代理行失败：不参与去重，会话行保留
            (
                "p5",
                "claude",
                "haiku",
                7,
                7,
                7,
                7,
                500,
                20000,
                Some("proxy"),
            ),
            (
                "s5",
                "claude",
                "haiku",
                7,
                7,
                7,
                7,
                200,
                20010,
                Some("session_log"),
            ),
            // 模型名 unknown 也算匹配
            ("p6", "claude", "unknown", 8, 8, 8, 8, 200, 30000, None),
            (
                "s6",
                "claude",
                "opus",
                8,
                8,
                8,
                8,
                200,
                30010,
                Some("session_log"),
            ),
            // 不相关的会话行，让会话行多于代理行
            (
                "s7",
                "claude",
                "opus",
                1,
                1,
                1,
                1,
                200,
                40000,
                Some("session_log"),
            ),
            (
                "s8",
                "claude",
                "opus",
                2,
                2,
                2,
                2,
                200,
                40001,
                Some("session_log"),
            ),
            (
                "s9",
                "claude",
                "opus",
                3,
                3,
                3,
                3,
                200,
                40002,
                Some("session_log"),
            ),
        ];
        for r in rows {
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, app_type, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9],
            )?;
        }

        let ids = |filter: &str, start: i64, end: i64| -> Result<Vec<String>, AppError> {
            let sql = format!(
                "SELECT request_id FROM proxy_request_logs l
                 WHERE l.created_at BETWEEN ?1 AND ?2 AND {filter}
                 ORDER BY request_id"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![start, end], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        };

        let original = effective_usage_log_filter("l");
        // 全量（代理少于会话 → NOT IN）、只含代理行、只含会话行（直接跳过）
        for (start, end) in [(0, 50000), (900, 1050), (40000, 40002), (1100, 9100)] {
            let ranged = effective_usage_log_filter_for_range(&conn, "l", Some(start), Some(end))?;
            assert_eq!(
                ids(&ranged, start, end)?,
                ids(&original, start, end)?,
                "range {start}..{end}"
            );
        }
        let all = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
        assert!(all.contains("NOT IN"));
        assert_eq!(ids(&all, 0, 50000)?, ids(&original, 0, 50000)?);
        assert_eq!(
            ids(&all, 0, 50000)?,
            vec!["p1", "p2", "p3", "p5", "p6", "s4", "s5", "s7", "s8", "s9"]
        );

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
    fn test_matching_proxy_log_matches_claude_desktop_for_claude_session() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('desktop-proxy', 'claude-desktop', 'claude-sonnet-4-5', 100, 20, 10, 5, 200, 1000, 'proxy')",
            [],
        )?;

        let key = DedupKey {
            app_type: "claude",
            model: "claude-sonnet-4-5",
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 10,
            cache_creation_tokens: 5,
            created_at: 1060,
        };
        assert!(has_matching_proxy_usage_log(&conn, &key)?);

        let mut outside_window = key;
        outside_window.created_at = 1_601;
        assert!(!has_matching_proxy_usage_log(&conn, &outside_window)?);

        let mut different_model = key;
        different_model.model = "claude-opus-4-5";
        assert!(!has_matching_proxy_usage_log(&conn, &different_model)?);

        let mut different_input = key;
        different_input.input_tokens += 1;
        assert!(!has_matching_proxy_usage_log(&conn, &different_input)?);

        let mut different_cache_creation = key;
        different_cache_creation.cache_creation_tokens += 1;
        assert!(!has_matching_proxy_usage_log(
            &conn,
            &different_cache_creation
        )?);

        Ok(())
    }

    #[test]
    fn test_effective_filter_dedups_claude_session_against_desktop_proxy() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        conn.execute_batch(
            "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES
                ('desktop-proxy', 'claude-desktop', 'claude-sonnet-4-5', 100, 20, 10, 5, 200, 1000, 'proxy'),
                ('claude-session', 'claude', 'claude-sonnet-4-5', 100, 20, 10, 5, 200, 1060, 'session_log');",
        )?;

        let filter = effective_usage_log_filter("l");
        let sql = format!("SELECT request_id FROM proxy_request_logs l WHERE {filter}");
        let request_ids = conn
            .prepare(&sql)?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(request_ids, vec!["desktop-proxy"]);

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
    fn test_backfill_new_anthropic_openai_pricing_after_upgrade() -> Result<(), AppError> {
        let db = Database::memory()?;
        let cases = [
            (
                "anthropic/claude-opus-5.5",
                "claude",
                1_000_000,
                ["4.000000", "20.000000", "0.200000", "5.000000", "29.200000"],
            ),
            (
                "OpenAI/GPT-6-SOL@HIGH",
                "codex",
                3_000_000,
                ["2.000000", "10.000000", "0.200000", "2.500000", "14.700000"],
            ),
            (
                "OpenAI/GPT-6.1-SOL@HIGH",
                "codex",
                3_000_000,
                ["2.000000", "10.000000", "0.100000", "2.500000", "14.600000"],
            ),
            (
                "gpt-6-luna",
                "codex",
                3_000_000,
                ["0.100000", "0.500000", "0.010000", "0.125000", "0.735000"],
            ),
            (
                "gpt-5.6-cyber",
                "codex",
                3_000_000,
                [
                    "12.500000",
                    "75.000000",
                    "1.250000",
                    "15.625000",
                    "104.375000",
                ],
            ),
            // Pro 系列无缓存折扣，缓存列记 0
            (
                "gpt-5.5-pro",
                "codex",
                3_000_000,
                [
                    "30.000000",
                    "180.000000",
                    "0.000000",
                    "0.000000",
                    "210.000000",
                ],
            ),
            // 剥日期后缀后精确命中 gpt-4o-mini，不会落到更短的 gpt-4o
            (
                "gpt-4o-mini-2024-07-18",
                "codex",
                3_000_000,
                ["0.150000", "0.600000", "0.075000", "0.000000", "0.825000"],
            ),
        ];
        {
            let conn = lock_conn!(db.conn);
            // Simulate an existing database with unpriced usage before the update.
            conn.execute(
                "DELETE FROM model_pricing WHERE model_id IN
                 ('claude-opus-5-5', 'gpt-6-sol', 'gpt-6.1-sol', 'gpt-6-luna', 'gpt-5.6-cyber',
                  'gpt-5.5-pro', 'gpt-4o-mini')",
                [],
            )?;
            for (model, app, input, _) in &cases {
                insert_usage_log(
                    &conn, model, app, "p1", model, "proxy", 1000, *input, 1_000_000, 1_000_000,
                    1_000_000, 200, "0",
                )?;
            }
            conn.execute(
                "UPDATE proxy_request_logs SET input_token_semantics = ?1",
                [INPUT_TOKEN_SEMANTICS_TOTAL],
            )?;
        }
        assert_eq!(db.backfill_missing_usage_costs()?, 0);
        db.ensure_model_pricing_seeded()?;
        assert_eq!(db.backfill_missing_usage_costs()?, cases.len() as u64);

        let conn = lock_conn!(db.conn);
        for (model, _, _, expected) in cases {
            let costs: [String; 5] = conn.query_row(
                "SELECT input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd
                 FROM proxy_request_logs WHERE request_id = ?1",
                [model],
                |row| {
                    Ok([
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ])
                },
            )?;
            assert_eq!(costs, expected, "{model}");
        }
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
        // grok-4.5 定价 2/6/0.30：input = (700-250)×2/1M，cache_read = 250×0.3/1M
        assert_eq!(input_cost, "0.000900");
        assert_eq!(cache_read_cost, "0.000075");
        assert_eq!(total_cost, "0.001575");
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
    fn test_get_session_usage_summary_counts_only_session_log_rows() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // (request_id, app_type, session_id, data_source, input, output, cache_read, cost)
            let rows = [
                (
                    "s1",
                    "claude",
                    "sess-a",
                    "session_log",
                    10,
                    200,
                    5000,
                    "0.10",
                ),
                (
                    "s2",
                    "claude-desktop",
                    "sess-a",
                    "session_log",
                    20,
                    300,
                    7000,
                    "0.20",
                ),
                // 代理行即便会话 ID 相同也不数，避免与会话日志重复
                ("p1", "claude", "sess-a", "proxy", 10, 200, 5000, "0.10"),
                ("s3", "claude", "sess-b", "session_log", 1, 1, 1, "9.00"),
                ("s4", "codex", "sess-a", "codex_session", 1, 1, 0, "9.00"),
            ];
            for (id, app, session, source, input, output, cache_read, cost) in rows {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, cache_read_tokens, total_cost_usd,
                        latency_ms, status_code, created_at, session_id, data_source
                    ) VALUES (?, 'p1', ?, 'claude-x', ?, ?, ?, ?, 0, 200, 1000, ?, ?)",
                    params![id, app, input, output, cache_read, cost, session, source],
                )?;
            }
        }

        let summary = db.get_session_usage_summary("claude", "sess-a")?;
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.real_total_tokens, 10 + 200 + 5000 + 20 + 300 + 7000);
        assert_eq!(summary.total_cost, "0.300000");

        let empty = db.get_session_usage_summary("claude", "missing")?;
        assert_eq!(empty.total_requests, 0);

        Ok(())
    }

    /// 最早日期：明细和日聚合取较早的一天，筛选与 Dashboard 同口径（claude 包含 claude-desktop）。
    #[test]
    fn test_get_first_usage_date_spans_rollups_and_logs() -> Result<(), AppError> {
        let db = Database::memory()?;
        assert_eq!(db.get_first_usage_date(None, None, None)?, None);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES ('2024-03-05', 'claude', 'p1', 'claude-3', 1, 1, 10, 5, 0, 0, '0', 100)",
                [],
            )?;
            insert_usage_log(
                &conn,
                "codex-1",
                "codex",
                "p2",
                "gpt-5.5",
                "proxy",
                local_ts(2025, 1, 2, 10, 0, 0),
                10,
                5,
                0,
                0,
                200,
                "0",
            )?;
            insert_usage_log(
                &conn,
                "desktop-1",
                "claude-desktop",
                "p3",
                "claude-3",
                "proxy",
                local_ts(2023, 12, 31, 23, 30, 0),
                10,
                5,
                0,
                0,
                200,
                "0",
            )?;
        }

        assert_eq!(
            db.get_first_usage_date(None, None, None)?.as_deref(),
            Some("2023-12-31")
        );
        assert_eq!(
            db.get_first_usage_date(Some("claude"), None, None)?
                .as_deref(),
            Some("2023-12-31")
        );
        assert_eq!(
            db.get_first_usage_date(Some("codex"), None, None)?
                .as_deref(),
            Some("2025-01-02")
        );
        assert_eq!(
            db.get_first_usage_date(None, None, Some("claude-3"))?
                .as_deref(),
            Some("2023-12-31")
        );
        assert_eq!(db.get_first_usage_date(Some("gemini"), None, None)?, None);

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
    fn test_get_request_detail_reads_proxy_and_session_rows() -> Result<(), AppError> {
        let db = Database::memory()?;
        let ts = local_ts(2026, 6, 10, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            // providers 也有 created_at / cost_multiplier 列：详情查询的列必须带表别名，
            // 否则 SQLite 报 ambiguous column name，整个命令失败。
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, created_at) VALUES
                 ('prov-a', 'claude', 'Packy', '{}', 1)",
                [],
            )?;
            insert_usage_log(
                &conn,
                "a-1",
                "claude",
                "prov-a",
                "claude-sonnet-4-6",
                "proxy",
                ts,
                100,
                10,
                0,
                0,
                200,
                "1.0",
            )?;
            insert_usage_log(
                &conn,
                "session:msg_1",
                "claude",
                "_session",
                "claude-sonnet-4-6",
                "session_log",
                ts,
                999,
                99,
                0,
                0,
                200,
                "0.5",
            )?;
        }

        let proxy = db.get_request_detail("a-1")?.expect("proxy row");
        assert_eq!(proxy.provider_name.as_deref(), Some("Packy"));
        assert_eq!(proxy.created_at, ts);
        assert_eq!(proxy.input_tokens, 100);

        let session = db
            .get_request_detail("session:msg_1")?
            .expect("session row");
        assert_eq!(session.provider_name.as_deref(), Some("Claude (Session)"));
        assert_eq!(session.created_at, ts);

        assert!(db.get_request_detail("missing")?.is_none());

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

        let breakdown = crate::services::session_usage::get_data_source_breakdown(&db)?;
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
    fn test_effective_usage_dedup_matches_legacy_codex_proxy_without_cache_writes(
    ) -> Result<(), AppError> {
        // v3.17.0 之前的代理不记录 Codex 缓存写入；新语义代理行仍要求写入量一致。
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            for (request_id, provider_id, data_source, created_at, cache_creation) in [
                ("legacy-proxy", "openai", "proxy", 10_000, 0),
                (
                    "session-vs-legacy",
                    "_codex_session",
                    "codex_session",
                    10_060,
                    600,
                ),
                ("total-proxy", "openai", "proxy", 20_000, 0),
                (
                    "session-vs-total",
                    "_codex_session",
                    "codex_session",
                    20_060,
                    600,
                ),
            ] {
                insert_usage_log(
                    &conn,
                    request_id,
                    "codex",
                    provider_id,
                    "gpt-5.6",
                    data_source,
                    created_at,
                    1000,
                    50,
                    300,
                    cache_creation,
                    200,
                    "0.01",
                )?;
            }
            conn.execute(
                "UPDATE proxy_request_logs SET input_token_semantics = ?1
                 WHERE request_id != 'legacy-proxy'",
                [INPUT_TOKEN_SEMANTICS_TOTAL],
            )?;
        }

        // 逐行 EXISTS 和按窗口的 rowid NOT IN 两种写法都要去掉 session-vs-legacy。
        let conn = lock_conn!(db.conn);
        let ranged = effective_usage_log_filter_for_range(&conn, "l", None, None)?;
        assert!(ranged.contains("NOT IN"));
        for filter in [effective_usage_log_filter("l"), ranged] {
            let mut stmt = conn.prepare(&format!(
                "SELECT request_id FROM proxy_request_logs l WHERE {filter} ORDER BY request_id"
            ))?;
            let request_ids = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(
                request_ids,
                ["legacy-proxy", "session-vs-total", "total-proxy"]
            );
        }

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
    fn test_get_model_stats_success_rate_and_speed_per_model() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            let insert = |id: &str,
                          model: &str,
                          output: i64,
                          latency: i64,
                          first: Option<i64>,
                          status: i64,
                          source: &str| {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, first_token_ms, status_code, created_at, data_source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        id, "p1", "claude", model, 10, output, "0", latency, first, status, 1000,
                        source
                    ],
                )
            };
            // fast：精确速度 1000 token / 10000 ms，另有一条失败请求
            insert("fast-ok", "fast", 1_000, 11_000, Some(1_000), 200, "proxy")?;
            insert("fast-err", "fast", 0, 500, None, 500, "proxy")?;
            // slow：只有会话日志，估算速度 400 token / 8000 ms
            insert("slow-a", "slow", 400, 8_000, None, 200, "session_log")?;
        }

        let stats = db.get_model_stats(None, None, None, None, None)?;
        let fast = stats.iter().find(|s| s.model == "fast").expect("fast");
        assert_eq!(fast.request_count, 2);
        assert!((fast.success_rate - 50.0).abs() < f32::EPSILON);
        assert_eq!(fast.speed_output_tokens, 1_000);
        assert_eq!(fast.speed_generation_ms, 10_000);
        assert_eq!(fast.est_speed_output_tokens, 0);

        let slow = stats.iter().find(|s| s.model == "slow").expect("slow");
        assert!((slow.success_rate - 100.0).abs() < f32::EPSILON);
        assert_eq!(slow.speed_output_tokens, 0);
        assert_eq!(slow.est_speed_output_tokens, 400);
        assert_eq!(slow.est_speed_duration_ms, 8_000);

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
        assert_eq!(stats[0].app_type, "claude");
        assert_eq!(stats[0].request_count, 1);
        assert_eq!(stats[0].total_tokens, 275);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_preserves_app_type_identity_across_apps() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES (?, ?, ?, '{}')",
                params!["shared-p1", "claude", "Shared Provider"],
            )?;
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES (?, ?, ?, '{}')",
                params!["shared-p1", "claude-desktop", "Shared Provider"],
            )?;

            // Detail log under claude
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-1",
                    "shared-p1",
                    "claude",
                    "m1",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    1000
                ],
            )?;

            // Rollup under claude-desktop
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, input_tokens, output_tokens,
                    cache_creation_tokens, cache_read_tokens, total_cost_usd,
                    success_count, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2026-10-08",
                    "claude-desktop",
                    "shared-p1",
                    "m1",
                    10,
                    1000,
                    500,
                    0,
                    0,
                    "0.10",
                    10,
                    120
                ],
            )?;
        }

        // Folded query for "claude" matches both claude and claude-desktop
        let stats = db.get_provider_stats(None, None, Some("claude"), None, None)?;
        assert_eq!(stats.len(), 2);

        let claude_stat = stats
            .iter()
            .find(|s| s.app_type == "claude")
            .expect("claude stat");
        assert_eq!(claude_stat.provider_id, "shared-p1");
        assert_eq!(claude_stat.provider_name, "Shared Provider");
        assert_eq!(claude_stat.request_count, 1);

        let desktop_stat = stats
            .iter()
            .find(|s| s.app_type == "claude-desktop")
            .expect("claude-desktop stat");
        assert_eq!(desktop_stat.provider_id, "shared-p1");
        assert_eq!(desktop_stat.provider_name, "Shared Provider");
        assert_eq!(desktop_stat.request_count, 10);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_speed_sums_only_eligible_requests() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            let insert = |id: &str, output: i64, latency: i64, first: Option<i64>| {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, first_token_ms, status_code, created_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![id, "p1", "claude", "m", 10, output, "0", latency, first, 200, 1000],
                )
            };
            // 计入：1000 token / (11000 - 1000) ms
            insert("ok-a", 1000, 11_000, Some(1_000))?;
            // 计入：300 token / (4000 - 1000) ms
            insert("ok-b", 300, 4_000, Some(1_000))?;
            // 不计：输出不到 100
            insert("short", 50, 2_000, Some(100))?;
            // 不计：没有首字（会话日志 / 非流式）
            insert("no-ttft", 5_000, 9_000, None)?;
            // 不计：耗时不大于首字
            insert("zero-gen", 500, 1_000, Some(1_000))?;
            // 不计：生成窗口不到 100ms，是传输突发
            insert("burst", 500, 1_050, Some(1_000))?;
        }

        let stats = db.get_provider_stats(None, None, None, None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].request_count, 6);
        assert_eq!(stats[0].speed_output_tokens, 1_300);
        assert_eq!(stats[0].speed_generation_ms, 13_000);
        // 路由服务的行没有首字（非流式）也不算估算速度
        assert_eq!(stats[0].est_speed_output_tokens, 0);
        assert_eq!(stats[0].est_speed_duration_ms, 0);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_estimated_speed_sums_session_rows() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            let insert = |id: &str, output: i64, latency: i64, source: &str| {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model,
                        input_tokens, output_tokens, total_cost_usd,
                        latency_ms, status_code, created_at, data_source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        id, "_session", "claude", "m", 10, output, "0", latency, 200, 1000, source
                    ],
                )
            };
            // 计入：2000 token / 20000 ms
            insert("ok-a", 2_000, 20_000, "session_log")?;
            // 计入：500 token / 5000 ms
            insert("ok-b", 200, 5_000, "session_log")?;
            // 不计：输出不到 200
            insert("short", 199, 5_000, "session_log")?;
            // 不计：没估出耗时
            insert("no-timing", 3_000, 0, "session_log")?;
            // 不计：耗时不到 1 秒
            insert("too-fast", 800, 900, "session_log")?;
        }

        let stats = db.get_provider_stats(None, None, None, None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].est_speed_output_tokens, 2_200);
        assert_eq!(stats[0].est_speed_duration_ms, 25_000);
        // 估算的不混进精确口径
        assert_eq!(stats[0].speed_output_tokens, 0);
        assert_eq!(stats[0].speed_generation_ms, 0);

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
    fn test_provider_and_model_stats_tokens_sum_to_summary_real_total() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // Claude：input 不含缓存。真实消耗 = 100 + 200 + 1000 + 5000
            insert_usage_log(
                &conn,
                "claude-1",
                "claude",
                "p-claude",
                "claude-x",
                "session_log",
                1000,
                100,
                200,
                5000,
                1000,
                200,
                "0.10",
            )?;
            // Codex：input 已含缓存命中 600。真实消耗 = (1000 - 600) + 50 + 600
            insert_usage_log(
                &conn,
                "codex-1",
                "codex",
                "p-codex",
                "gpt-x",
                "codex_session",
                1000,
                1000,
                50,
                600,
                0,
                200,
                "0.05",
            )?;
            // 日汇总行同样要带上缓存。真实消耗 = (900 - 300) + 40 + 300 + 0
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2020-01-01",
                    "codex",
                    "p-codex",
                    "gpt-x",
                    3,
                    3,
                    900,
                    40,
                    300,
                    0,
                    "0.30",
                    100
                ],
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.real_total_tokens, 6300 + 1050 + 940);

        let providers = db.get_provider_stats(None, None, None, None, None)?;
        let tokens_of = |id: &str| {
            providers
                .iter()
                .find(|s| s.provider_id == id)
                .map(|s| s.total_tokens)
        };
        assert_eq!(tokens_of("p-claude"), Some(6300));
        assert_eq!(tokens_of("p-codex"), Some(1050 + 940));
        let provider_sum: u64 = providers.iter().map(|s| s.total_tokens).sum();
        assert_eq!(provider_sum, summary.real_total_tokens);

        let models = db.get_model_stats(None, None, None, None, None)?;
        let model_sum: u64 = models.iter().map(|s| s.total_tokens).sum();
        assert_eq!(model_sum, summary.real_total_tokens);

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
