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
use std::cell::OnceCell;
use std::collections::{HashMap, VecDeque};
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
    /// 缓存命中 Tokens：命中率的分子。
    pub cache_read_tokens: u64,
    /// 可缓存输入（新增输入 + 缓存写入 + 缓存命中）：命中率的分母，与指标卡同口径。
    pub cacheable_input_tokens: u64,
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
    /// 首字时间中位数（毫秒）：经路由服务、成功的流式明细请求；没有这样的请求时为 None。
    pub first_token_median_ms: Option<u64>,
    /// 估算首字中位数（毫秒）：会话日志导入、按时间戳推出了首字的明细请求。
    /// 和上面分开：口径不同，界面上只在没有精确值时显示，带 ≈。
    pub est_first_token_median_ms: Option<u64>,
}

/// 计速度的门槛：输出少于这个数的请求（工具调用这类）不算速度，避免 0.1 秒回 15 个 token 算出离谱的数。
pub const SPEED_MIN_OUTPUT_TOKENS: i64 = 100;

/// 生成窗口（耗时 − 首字）短于这个毫秒数时不算速度：中转站缓冲后一次性吐出、
/// 或短回复整段落在同一个网络包里，算出来的是传输突发而不是生成速度。
pub const SPEED_MIN_GENERATION_MS: i64 = 100;

/// 明细行能不能计速度的 SQL 条件（和前端 `isSpeedEligible` 同口径）。会话日志导入的行
/// 也可能有首字（按时间戳推出来的），所以先按来源分开，不能只看有没有首字。
fn speed_eligible_sql(alias: &str) -> String {
    let data_source = data_source_expr(alias);
    format!(
        "{data_source} = 'proxy' AND {alias}.first_token_ms IS NOT NULL \
         AND {alias}.output_tokens >= {SPEED_MIN_OUTPUT_TOKENS} \
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
/// 会话日志导入的行，耗时是导入时按日志时间戳估的。估算速度仍按整段耗时算（含首字
/// 等待），不减推出来的首字：只有部分请求推得出首字，减了会让同一行里两种算法混在一起。
fn speed_estimate_eligible_sql(alias: &str) -> String {
    let data_source = data_source_expr(alias);
    format!(
        "{data_source} <> 'proxy' \
         AND {alias}.output_tokens >= {SPEED_ESTIMATE_MIN_OUTPUT_TOKENS} \
         AND {alias}.latency_ms >= {SPEED_ESTIMATE_MIN_DURATION_MS}"
    )
}

/// 明细行能不能计首字时间的 SQL 条件：经路由服务、成功、有首字计时（流式请求）。
fn first_token_eligible_sql(alias: &str) -> String {
    let data_source = data_source_expr(alias);
    format!(
        "{data_source} = 'proxy' AND {alias}.first_token_ms IS NOT NULL \
         AND {alias}.status_code >= 200 AND {alias}.status_code < 300"
    )
}

/// 估算首字的 SQL 条件：会话日志导入、导入时按时间戳推出了首字的行。
/// 和路由的精确首字分开取中位数：估算的起点是上一条日志写盘的时刻，含客户端
/// 组装请求的时间，系统性偏高。
fn est_first_token_eligible_sql(alias: &str) -> String {
    let data_source = data_source_expr(alias);
    format!("{data_source} <> 'proxy' AND {alias}.first_token_ms IS NOT NULL")
}

/// Read exact and estimated samples together, then select each median in O(n).
/// SQL need not sort either population or scan the same detail rows twice.
type MedianGroups = HashMap<String, u64>;
fn first_token_medians(
    conn: &Connection,
    key_sql: &str,
    from_sql: &str,
    detail_where: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<(MedianGroups, MedianGroups), AppError> {
    let connector = if detail_where.is_empty() {
        "WHERE"
    } else {
        "AND"
    };
    let exact = first_token_eligible_sql("l");
    let estimated = est_first_token_eligible_sql("l");
    let sql = format!(
        "SELECT {key_sql}, l.first_token_ms, ({exact}) {from_sql} {detail_where}
         {connector} (({exact}) OR ({estimated}))"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?.max(0) as u64,
            row.get::<_, bool>(2)?,
        ))
    })?;
    let mut exact_groups: HashMap<String, Vec<u64>> = HashMap::new();
    let mut estimated_groups: HashMap<String, Vec<u64>> = HashMap::new();
    for row in rows {
        let (key, ms, exact) = row?;
        let groups = if exact {
            &mut exact_groups
        } else {
            &mut estimated_groups
        };
        groups.entry(key).or_default().push(ms);
    }
    let medians = |groups: HashMap<String, Vec<u64>>| {
        groups
            .into_iter()
            .map(|(key, mut values)| (key, select_median(&mut values)))
            .collect()
    };
    Ok((medians(exact_groups), medians(estimated_groups)))
}

fn select_median(values: &mut [u64]) -> u64 {
    let mid = values.len() / 2;
    let even = values.len() % 2 == 0;
    let (lower, middle, _) = values.select_nth_unstable(mid);
    if even {
        let low = *lower.iter().max().expect("nonempty lower half");
        low + (*middle - low) / 2
    } else {
        *middle
    }
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
    /// 缓存命中 Tokens：命中率的分子。
    pub cache_read_tokens: u64,
    /// 可缓存输入（新增输入 + 缓存写入 + 缓存命中）：命中率的分母，与指标卡同口径。
    pub cacheable_input_tokens: u64,
    /// 速度的分子分母，口径同 [`ProviderStats::speed_output_tokens`] 那四个字段。
    pub speed_output_tokens: u64,
    pub speed_generation_ms: u64,
    pub est_speed_output_tokens: u64,
    pub est_speed_duration_ms: u64,
    /// 首字时间中位数，口径同 [`ProviderStats`] 的那两个字段。
    pub first_token_median_ms: Option<u64>,
    pub est_first_token_median_ms: Option<u64>,
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
    pub fresh_input_tokens: u32,
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

/// 把 27 列的查询结果映射为 `RequestLogDetail`。
///
/// 调用方的 SELECT **必须**按以下顺序返回 27 列：
/// `request_id, provider_id, provider_name, app_type, model, request_model,
///  cost_multiplier, input_tokens, output_tokens, cache_read_tokens,
///  cache_creation_tokens, input_cost_usd, output_cost_usd, cache_read_cost_usd,
///  cache_creation_cost_usd, total_cost_usd, is_streaming, latency_ms,
///  first_token_ms, duration_ms, status_code, error_message, created_at,
///  data_source, pricing_model, input_token_semantics, fresh_input_sql(..)`
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
        fresh_input_tokens: row.get::<_, i64>(26)? as u32,
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
         WHEN '_agy_session' THEN 'Antigravity (Session)' \
         WHEN '_kimi_session' THEN 'Kimi Code (Session)' \
         WHEN '_copilot_session' THEN 'Copilot CLI (Session)' \
         WHEN '_workbuddy_session' THEN 'WorkBuddy (Session)' \
         WHEN '_dsh_session' THEN 'DeepSeek Harness (Session)' \
         WHEN '_dsh_account_session' THEN 'DeepSeek Harness (OAuth)' \
         WHEN '_mcode_session' THEN 'MiniMax Code (Session)' \
         WHEN '_pi_session' THEN 'Pi (Session)' \
         WHEN '_omp_session' THEN 'Oh My Pi (Session)' \
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
/// [`dedup_app_type_match_sql`]。
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
/// model。统计展示可合并同价的推理档位，原始请求仍保留此计价基准。
fn effective_model_sql(alias: &str) -> String {
    format!("COALESCE(NULLIF({alias}.pricing_model, ''), {alias}.model)")
}

/// 一条查询的 `WHERE` 条件和按 `?` 出现顺序绑定的参数，两者一起追加，顺序不会错开。
#[derive(Default)]
struct SqlWhere {
    conditions: Vec<String>,
    params: Vec<Box<dyn rusqlite::ToSql>>,
}

impl SqlWhere {
    fn push_condition(&mut self, condition: impl Into<String>) {
        self.conditions.push(condition.into());
    }

    fn push(&mut self, condition: impl Into<String>, param: impl rusqlite::ToSql + 'static) {
        self.conditions.push(condition.into());
        self.params.push(Box::new(param));
    }

    /// `WHERE a AND b`；没有条件时是空串。
    fn into_where(self) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
        let clause = if self.conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.conditions.join(" AND "))
        };
        (clause, self.params)
    }

    /// 接在已有 `WHERE` 后面的 `AND a AND b`；没有条件时是空串。
    fn into_and(self) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
        let clause = self
            .conditions
            .iter()
            .map(|c| format!("AND {c}"))
            .collect::<Vec<_>>()
            .join(" ");
        (clause, self.params)
    }
}

/// Dashboard 顶部的筛选：应用、Provider、模型。
///
/// 应用按 [`folded_app_type_sql`] 匹配（claude-desktop 算 claude）；Provider 按展示名
/// 精确匹配（复用 [`provider_name_coalesce`]，会话占位行的可读名如 "Claude (Session)"
/// 也能选中）；模型按 [`effective_model_sql`] 匹配。
struct UsageFilter<'a> {
    app_type: Option<&'a str>,
    provider_name: Option<&'a str>,
    model: Option<&'a str>,
    pricing: OnceCell<PricingCatalog>,
}

impl UsageFilter<'_> {
    fn pricing(&self, conn: &Connection) -> Result<&PricingCatalog, AppError> {
        if self.pricing.get().is_none() {
            let _ = self.pricing.set(PricingCatalog::load(conn)?);
        }
        Ok(self.pricing.get().expect("loaded above"))
    }

    /// 追加筛选条件。传了 `provider_name` 时 FROM 里必须有 [`Self::join`]（或同样的
    /// `LEFT JOIN providers`），否则 `{provider_alias}.name` 无法解析。
    fn push_to(
        &self,
        conn: &Connection,
        conditions: &mut SqlWhere,
        log_alias: &str,
        provider_alias: &str,
    ) -> Result<(), AppError> {
        if let Some(app_type) = self.app_type {
            let column = format!("{log_alias}.app_type");
            // Keep the indexed column bare; CASE remains only in grouping.
            if app_type == "claude" {
                conditions.push_condition(format!("{column} IN ('claude', 'claude-desktop')"));
            } else if app_type == "claude-desktop" {
                // This UI filter names folded apps; Desktop is part of Claude.
                conditions.push_condition("0");
            } else {
                conditions.push(format!("{column} = ?"), app_type.to_string());
            }
        }
        if let Some(name) = self.provider_name {
            conditions.push(
                format!("{} = ?", provider_name_coalesce(log_alias, provider_alias)),
                name.to_string(),
            );
        }
        if let Some(model) = self.model {
            let expr = effective_model_sql(log_alias);
            let suffixes = merged_reasoning_suffixes_for_filter(self.pricing(conn)?, model)?;
            if suffixes.is_empty() {
                conditions.push(format!("{expr} = ?"), model.to_string());
            } else {
                let char_len = i64::try_from(model.chars().count()).unwrap_or(i64::MAX);
                let placeholders = vec!["?"; suffixes.len()].join(", ");
                conditions.push_condition(format!(
                    "({expr} = ? OR (SUBSTR({expr}, 1, ?) = ? AND LOWER(SUBSTR({expr}, ?)) IN ({placeholders})))"
                ));
                conditions.params.push(Box::new(model.to_string()));
                conditions.params.push(Box::new(char_len));
                conditions.params.push(Box::new(model.to_string()));
                conditions.params.push(Box::new(char_len.saturating_add(1)));
                for suffix in suffixes {
                    conditions.params.push(Box::new(suffix));
                }
            }
        }
        Ok(())
    }

    /// 只有按 Provider 筛选时才需要连 providers 表。
    fn join(&self, log_alias: &str, provider_alias: &str) -> String {
        if self.provider_name.is_some() {
            providers_join(log_alias, provider_alias)
        } else {
            String::new()
        }
    }

    /// 明细表 `l`（providers 别名 `p`）：跨源去重、时间范围、筛选。
    fn detail_where(
        &self,
        conn: &Connection,
        start_date: Option<i64>,
        end_date: Option<i64>,
    ) -> Result<SqlWhere, AppError> {
        let mut conditions = SqlWhere::default();
        conditions.push_condition(effective_usage_log_filter_for_range(
            conn, "l", start_date, end_date,
        )?);
        if let Some(start) = start_date {
            conditions.push("l.created_at >= ?", start);
        }
        if let Some(end) = end_date {
            conditions.push("l.created_at <= ?", end);
        }
        self.push_to(conn, &mut conditions, "l", "p")?;
        Ok(conditions)
    }

    /// 汇总表 `r`（providers 别名 `p2`）：只算被范围完整覆盖的本地自然日，再加筛选。
    fn rollup_where(
        &self,
        conn: &Connection,
        bounds: &RollupDateBounds,
    ) -> Result<SqlWhere, AppError> {
        let mut conditions = SqlWhere::default();
        push_rollup_date_filters(&mut conditions, "r.date", bounds);
        self.push_to(conn, &mut conditions, "r", "p2")?;
        Ok(conditions)
    }
}

/// 仅合并四项 token 单价完全一致且基础模型本身有明确价格的推理档位；
/// 缺失基础定价、或任意一项存在自定义差价时保持独立。
fn statistics_model_with(conn: &impl PricingLookup, model: &str) -> Result<String, AppError> {
    let targets = reasoning_tier_merge_targets(model);
    if targets.is_empty() {
        return Ok(model.to_string());
    }
    let parse = |row: (String, String, String, String)| {
        ModelPricing::from_strings(&row.0, &row.1, &row.2, &row.3).ok()
    };
    let Some(pricing) = lookup_model_pricing(conn, model)?.and_then(parse) else {
        return Ok(model.to_string());
    };
    for target in targets {
        if let Some(base_pricing) = find_direct_model_pricing_row(conn, target)?.and_then(parse) {
            if pricing.input_cost_per_million == base_pricing.input_cost_per_million
                && pricing.output_cost_per_million == base_pricing.output_cost_per_million
                && pricing.cache_read_cost_per_million == base_pricing.cache_read_cost_per_million
                && pricing.cache_creation_cost_per_million
                    == base_pricing.cache_creation_cost_per_million
            {
                return Ok(target.to_string());
            }
        }
    }
    Ok(model.to_string())
}

fn merged_reasoning_suffixes_for_filter(
    conn: &impl PricingLookup,
    model: &str,
) -> Result<Vec<String>, AppError> {
    if statistics_model_with(conn, model)? != model {
        return Ok(Vec::new());
    }
    let mut candidate_suffixes: Vec<String> = REASONING_EFFORT_SUFFIXES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let tiered_probe = format!("{model}-tiered");
    if strip_statistics_tiered_suffix(&tiered_probe).is_some() {
        candidate_suffixes.push("-tiered".to_string());
        for effort in REASONING_EFFORT_SUFFIXES {
            candidate_suffixes.push(format!("-tiered{effort}"));
        }
    }
    let mut matched = Vec::new();
    for suffix in candidate_suffixes {
        let variant = format!("{model}{suffix}");
        if statistics_model_with(conn, &variant)? == model {
            matched.push(suffix);
        }
    }
    Ok(matched)
}

fn strip_model_prefix_sql(expr: &str) -> String {
    format!("REPLACE(TRIM({expr}), RTRIM(TRIM({expr}), REPLACE(TRIM({expr}), '/', '')), '')")
}

/// 跨来源去重的模型匹配条件。
///
/// 常规路径按上游回显模型匹配（支持剥离 `provider/` 等命名空间前缀，如 `antigravity/`）。
/// 仅 Codex 会话允许再匹配代理记录的请求模型：
/// Codex rollout 保留客户端别名，而代理 `model` 可能已被映射为实际上游模型。
/// 空白或 `unknown` 请求模型不能放宽匹配，避免无有效别名时扩大启发式去重范围。
fn session_proxy_model_match_sql(
    proxy_model: &str,
    proxy_request_model: &str,
    session_model: &str,
    session_app_type: &str,
    session_data_source: &str,
) -> String {
    let p_model = strip_model_prefix_sql(proxy_model);
    let s_model = strip_model_prefix_sql(session_model);
    let p_req_model = strip_model_prefix_sql(proxy_request_model);
    format!(
        "(
            (
                NULLIF({p_model}, '') IS NOT NULL
                AND NULLIF({s_model}, '') IS NOT NULL
                AND LOWER({p_model}) = LOWER({s_model})
            )
            OR LOWER(TRIM({proxy_model})) = 'unknown'
            OR LOWER(TRIM({session_model})) = 'unknown'
            OR (
                {session_app_type} = 'codex'
                AND {session_data_source} = 'codex_session'
                AND NULLIF({p_req_model}, '') IS NOT NULL
                AND LOWER({p_req_model}) <> 'unknown'
                AND NULLIF({s_model}, '') IS NOT NULL
                AND LOWER({p_req_model}) = LOWER({s_model})
            )
        )"
    )
}

pub(crate) fn effective_usage_log_filter(log_alias: &str) -> String {
    let data_source = data_source_expr(log_alias);
    let proxy_data_source = data_source_expr("proxy_dedup");
    let app_type_match =
        dedup_app_type_match_sql("proxy_dedup.app_type", &format!("{log_alias}.app_type"));
    let model_match = session_proxy_model_match_sql(
        "proxy_dedup.model",
        "proxy_dedup.request_model",
        &format!("{log_alias}.model"),
        &format!("{log_alias}.app_type"),
        &data_source,
    );
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
                  AND {model_match}
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
    let model_match = session_proxy_model_match_sql(
        "dedup_p.model",
        "dedup_p.request_model",
        "dedup_s.model",
        "dedup_s.app_type",
        &ds_source,
    );
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
             AND {model_match}
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
/// `data_source`：会话记录的实际来源；别名匹配只对
/// `app_type = codex` 且 `data_source = codex_session` 生效。
/// `cache_creation_tokens`：Gemini/OpenCode 会话日志和旧版 Codex rollout 不暴露
/// 该字段，调用方传 0 表示"未知"，匹配器会放行 proxy 侧任意 cache_creation_tokens
/// 值。反过来，v3.17.0 之前的代理不记录 Codex 缓存写入，legacy 语义且写入量为 0
/// 的 Codex 代理行也放行任意 session 写入量。
#[derive(Debug, Clone, Copy)]
pub(crate) struct DedupKey<'a> {
    pub app_type: &'a str,
    pub data_source: &'a str,
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
    let model_match =
        session_proxy_model_match_sql("l.model", "l.request_model", "?2", "?1", "?11");
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
              AND {model_match}
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
                    key.data_source,
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

fn push_rollup_date_filters(conditions: &mut SqlWhere, column: &str, bounds: &RollupDateBounds) {
    if bounds.is_empty {
        conditions.push_condition("1 = 0");
        return;
    }

    if let Some(start) = &bounds.start {
        conditions.push(format!("{column} >= ?"), start.clone());
    }

    if let Some(end) = &bounds.end {
        conditions.push(format!("{column} <= ?"), end.clone());
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

        let filter = UsageFilter {
            app_type,
            provider_name,
            model,
            pricing: OnceCell::new(),
        };
        let (where_clause, params_vec) = filter
            .detail_where(&conn, start_date, end_date)?
            .into_where();
        let detail_join = filter.join("l", "p");

        // Only include rolled-up rows for full local days that are fully covered by the range.
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        let (rollup_where, rollup_params) =
            filter.rollup_where(&conn, &rollup_bounds)?.into_where();
        let rollup_join = filter.join("r", "p2");

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

        let filter = UsageFilter {
            app_type: None,
            provider_name,
            model,
            pricing: OnceCell::new(),
        };
        let (detail_where, detail_params) = filter
            .detail_where(&conn, start_date, end_date)?
            .into_where();
        let detail_join = filter.join("l", "p");

        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        let (rollup_where, rollup_params) =
            filter.rollup_where(&conn, &rollup_bounds)?.into_where();
        let rollup_join = filter.join("r", "p2");

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
        let filter = UsageFilter {
            app_type,
            provider_name,
            model,
            pricing: OnceCell::new(),
        };

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

            let mut extra = SqlWhere::default();
            filter.push_to(&conn, &mut extra, "l", "p")?;
            let (extra_filter, extra_params) = extra.into_and();
            let detail_join = filter.join("l", "p");

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

        let mut extra = SqlWhere::default();
        filter.push_to(&conn, &mut extra, "l", "p")?;
        let (extra_filter, extra_params) = extra.into_and();
        let detail_join = filter.join("l", "p");

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
        let (rollup_where, rollup_params) =
            filter.rollup_where(&conn, &rollup_bounds)?.into_where();
        let rollup_join = filter.join("r", "p2");

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

        let filter = UsageFilter {
            app_type,
            provider_name,
            model,
            pricing: OnceCell::new(),
        };
        let mut detail = SqlWhere::default();
        filter.push_to(&conn, &mut detail, "l", "p")?;
        let (detail_where, detail_params) = detail.into_where();
        let detail_join = filter.join("l", "p");
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

        let mut rollup = SqlWhere::default();
        filter.push_to(&conn, &mut rollup, "r", "p2")?;
        let (rollup_where, rollup_params) = rollup.into_where();
        let rollup_join = filter.join("r", "p2");
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

        let filter = UsageFilter {
            app_type,
            provider_name,
            model,
            pricing: OnceCell::new(),
        };
        let (detail_where, detail_params) = filter
            .detail_where(&conn, start_date, end_date)?
            .into_where();

        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        let (rollup_where, rollup_params) =
            filter.rollup_where(&conn, &rollup_bounds)?.into_where();

        // UNION detail logs + rollup data, then aggregate
        let detail_pname = provider_name_coalesce("l", "p");
        let rollup_pname = provider_name_coalesce("r", "p2");
        let real_total_detail = real_total_tokens_sql("l");
        let real_total_rollup = real_total_tokens_sql("r");
        let speed_ok = speed_eligible_sql("l");
        let est_ok = speed_estimate_eligible_sql("l");
        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
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
                SUM(est_ms) as est_ms,
                SUM(cache_read) as cache_read,
                SUM(cacheable) as cacheable
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
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.latency_ms ELSE 0 END), 0) as est_ms,
                    COALESCE(SUM(l.cache_read_tokens), 0) as cache_read,
                    COALESCE(SUM(({fresh_input_detail}) + l.cache_creation_tokens + l.cache_read_tokens), 0) as cacheable
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
                    0,
                    COALESCE(SUM(r.cache_read_tokens), 0),
                    COALESCE(SUM(({fresh_input_rollup}) + r.cache_creation_tokens + r.cache_read_tokens), 0)
                FROM usage_daily_rollups r
                LEFT JOIN providers p2 ON r.provider_id = p2.id AND r.app_type = p2.app_type
                {rollup_where}
                GROUP BY r.provider_id, r.app_type
            )
            GROUP BY provider_id, app_type
            ORDER BY total_cost DESC"
        );

        // 分组键用不会出现在 ID 里的 0x1F 拼接，和下面主查询的 (provider_id, app_type) 对上
        let (first_token_ms, est_first_token_ms) = {
            let detail_refs: Vec<&dyn rusqlite::ToSql> =
                detail_params.iter().map(|p| p.as_ref()).collect();
            first_token_medians(
                &conn,
                "l.provider_id || char(31) || l.app_type",
                "FROM proxy_request_logs l
                 LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type",
                &detail_where,
                &detail_refs,
            )?
        };

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let row_mapper = |row: &rusqlite::Row| {
            let request_count: i64 = row.get(3)?;
            let provider_id: String = row.get(0)?;
            let app_type: String = row.get(1)?;
            let key = format!("{provider_id}\u{1f}{app_type}");
            let first_token_median_ms = first_token_ms.get(&key).copied();
            let est_first_token_median_ms = est_first_token_ms.get(&key).copied();
            let success_count: i64 = row.get(6)?;
            let success_rate = if request_count > 0 {
                (success_count as f32 / request_count as f32) * 100.0
            } else {
                0.0
            };

            Ok(ProviderStats {
                provider_id,
                app_type,
                provider_name: row.get(2)?,
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                total_cost: format!("{:.6}", row.get::<_, f64>(5)?),
                success_rate,
                avg_latency_ms: row.get::<_, f64>(7)? as u64,
                cache_read_tokens: row.get::<_, i64>(12)?.max(0) as u64,
                cacheable_input_tokens: row.get::<_, i64>(13)?.max(0) as u64,
                speed_output_tokens: row.get::<_, i64>(8)?.max(0) as u64,
                speed_generation_ms: row.get::<_, i64>(9)?.max(0) as u64,
                est_speed_output_tokens: row.get::<_, i64>(10)?.max(0) as u64,
                est_speed_duration_ms: row.get::<_, i64>(11)?.max(0) as u64,
                first_token_median_ms,
                est_first_token_median_ms,
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

        let filter = UsageFilter {
            app_type,
            provider_name,
            model,
            pricing: OnceCell::new(),
        };
        let (detail_where, detail_params) = filter
            .detail_where(&conn, start_date, end_date)?
            .into_where();
        let detail_join = filter.join("l", "p");

        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        let (rollup_where, rollup_params) =
            filter.rollup_where(&conn, &rollup_bounds)?.into_where();
        let rollup_join = filter.join("r", "p2");

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
        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
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
                SUM(est_ms) as est_ms,
                SUM(cache_read) as cache_read,
                SUM(cacheable) as cacheable
            FROM (
                SELECT {detail_model} as model,
                    COUNT(*) as request_count,
                    COALESCE(SUM({real_total_detail}), 0) as total_tokens,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count,
                    COALESCE(SUM(CASE WHEN {speed_ok} THEN l.output_tokens ELSE 0 END), 0) as speed_output,
                    COALESCE(SUM(CASE WHEN {speed_ok} THEN l.latency_ms - l.first_token_ms ELSE 0 END), 0) as speed_gen_ms,
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.output_tokens ELSE 0 END), 0) as est_output,
                    COALESCE(SUM(CASE WHEN {est_ok} THEN l.latency_ms ELSE 0 END), 0) as est_ms,
                    COALESCE(SUM(l.cache_read_tokens), 0) as cache_read,
                    COALESCE(SUM(({fresh_input_detail}) + l.cache_creation_tokens + l.cache_read_tokens), 0) as cacheable
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
                    0,
                    COALESCE(SUM(r.cache_read_tokens), 0),
                    COALESCE(SUM(({fresh_input_rollup}) + r.cache_creation_tokens + r.cache_read_tokens), 0)
                FROM usage_daily_rollups r
                {rollup_join}
                {rollup_where}
                GROUP BY {rollup_model}
            )
            GROUP BY model
            ORDER BY total_cost DESC"
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> =
            detail_params.iter().map(|p| p.as_ref()).collect();
        params.extend(rollup_params.iter().map(|p| p.as_ref()));
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
                row.get::<_, i64>(2)?.max(0) as u64,
                row.get::<_, f64>(3)?,
                row.get::<_, i64>(4)?.max(0) as u64,
                row.get::<_, i64>(5)?.max(0) as u64,
                row.get::<_, i64>(6)?.max(0) as u64,
                row.get::<_, i64>(7)?.max(0) as u64,
                row.get::<_, i64>(8)?.max(0) as u64,
                row.get::<_, i64>(9)?.max(0) as u64,
                row.get::<_, i64>(10)?.max(0) as u64,
            ))
        })?;
        let mut grouped: HashMap<String, (ModelStats, f64, u64)> = HashMap::new();
        let mut model_cases = Vec::new();
        for row in rows {
            let (
                raw,
                count,
                tokens,
                cost,
                successes,
                output,
                ms,
                est_output,
                est_ms,
                cache_read,
                cacheable,
            ) = row?;
            let model = statistics_model_with(filter.pricing(&conn)?, &raw)?;
            if model != raw {
                // 来自数据库的模型名作为 SQL 字符串字面量使用。
                model_cases.push(format!(
                    "WHEN '{}' THEN '{}'",
                    raw.replace('\'', "''"),
                    model.replace('\'', "''")
                ));
            }
            let (stats, total_cost, success_count) =
                grouped.entry(model.clone()).or_insert_with(|| {
                    (
                        ModelStats {
                            model,
                            request_count: 0,
                            total_tokens: 0,
                            total_cost: String::new(),
                            avg_cost_per_request: String::new(),
                            success_rate: 0.0,
                            cache_read_tokens: 0,
                            cacheable_input_tokens: 0,
                            speed_output_tokens: 0,
                            speed_generation_ms: 0,
                            est_speed_output_tokens: 0,
                            est_speed_duration_ms: 0,
                            first_token_median_ms: None,
                            est_first_token_median_ms: None,
                        },
                        0.0,
                        0,
                    )
                });
            stats.request_count += count;
            stats.total_tokens += tokens;
            stats.cache_read_tokens += cache_read;
            stats.cacheable_input_tokens += cacheable;
            stats.speed_output_tokens += output;
            stats.speed_generation_ms += ms;
            stats.est_speed_output_tokens += est_output;
            stats.est_speed_duration_ms += est_ms;
            *total_cost += cost;
            *success_count += successes;
        }

        // 首字时间按合并后的全部样本取中位数，不能平均各档位的中位数。
        let median_model = if model_cases.is_empty() {
            detail_model
        } else {
            format!(
                "CASE {detail_model} {} ELSE {detail_model} END",
                model_cases.join(" ")
            )
        };
        let detail_refs: Vec<&dyn rusqlite::ToSql> =
            detail_params.iter().map(|p| p.as_ref()).collect();
        let from_sql = format!("FROM proxy_request_logs l {detail_join}");
        let (first_token_ms, est_first_token_ms) =
            first_token_medians(&conn, &median_model, &from_sql, &detail_where, &detail_refs)?;
        let mut stats: Vec<_> = grouped
            .into_values()
            .map(|(mut stat, cost, successes)| {
                stat.total_cost = format!("{cost:.6}");
                if stat.request_count > 0 {
                    stat.avg_cost_per_request = format!("{:.6}", cost / stat.request_count as f64);
                    stat.success_rate = successes as f32 / stat.request_count as f32 * 100.0;
                } else {
                    stat.avg_cost_per_request = "0.000000".to_string();
                }
                stat.first_token_median_ms = first_token_ms.get(&stat.model).copied();
                stat.est_first_token_median_ms = est_first_token_ms.get(&stat.model).copied();
                (stat, cost)
            })
            .collect();
        stats.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.model.cmp(&b.0.model)));
        let stats = stats.into_iter().map(|(stat, _)| stat).collect();

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

        let filter = UsageFilter {
            app_type: filters.app_type.as_deref(),
            provider_name: filters.provider_name.as_deref(),
            model: filters.model.as_deref(),
            pricing: OnceCell::new(),
        };
        let mut conditions = SqlWhere::default();
        conditions.push_condition(effective_usage_log_filter_for_range(
            &conn,
            "l",
            filters.start_date,
            filters.end_date,
        )?);
        // 应用仅过滤口径折叠 claude-desktop→claude；行投影仍返回原始 app_type，
        // 详情面板据此展示真实入口（路由接管账单审计需要）。Provider/模型与 Dashboard
        // 顶部下拉筛选同口径（会话占位行如 "Claude (Session)" 也能命中）。
        filter.push_to(&conn, &mut conditions, "l", "p")?;
        if let Some(status) = filters.status_code {
            conditions.push("l.status_code = ?", status as i64);
        }
        if let Some(start) = filters.start_date {
            conditions.push("l.created_at >= ?", start);
        }
        if let Some(end) = filters.end_date {
            conditions.push("l.created_at <= ?", end);
        }
        let (where_clause, mut params) = conditions.into_where();

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
        let logs_fresh_input = fresh_input_sql("l");
        let sql = format!(
            "SELECT l.request_id, l.provider_id, {logs_pname} as provider_name, l.app_type, l.model,
                    l.request_model, l.cost_multiplier,
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at, l.data_source, l.pricing_model,
                    l.input_token_semantics, {logs_fresh_input} AS fresh_input_tokens
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
        let detail_fresh_input = fresh_input_sql("l");
        let detail_sql = format!(
            "SELECT l.request_id, l.provider_id, {detail_pname} as provider_name, l.app_type, l.model,
                    l.request_model, l.cost_multiplier,
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at, l.data_source, l.pricing_model,
                    l.input_token_semantics, {detail_fresh_input} AS fresh_input_tokens
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
        let fresh_input = fresh_input_sql("");
        let base_sql = format!(
            "SELECT request_id, provider_id, NULL AS provider_name, app_type, model, request_model,
                        cost_multiplier,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd, is_streaming, latency_ms,
                        first_token_ms, duration_ms, status_code, error_message, created_at,
                        data_source, pricing_model, input_token_semantics,
                        {fresh_input} AS fresh_input_tokens
             FROM proxy_request_logs
             WHERE CAST(total_cost_usd AS REAL) <= 0
               AND (input_tokens > 0 OR output_tokens > 0
                    OR cache_read_tokens > 0 OR cache_creation_tokens > 0)"
        );

        let mut logs = {
            let mut stmt = conn.prepare(&base_sql)?;
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

        let multiplier =
            rust_decimal::Decimal::from_str(&log.cost_multiplier).unwrap_or_else(|e| {
                log::warn!(
                    "历史用量倍率解析失败 request_id={}: {} - {e}",
                    log.request_id,
                    log.cost_multiplier
                );
                rust_decimal::Decimal::ONE
            });
        // 倍率为 0 的行总价本来就是 0，不是缺价：回填只会重写分项费用、每次读日志都 UPDATE 一遍。
        if multiplier.is_zero() {
            return Ok(false);
        }

        let pricing = match Self::get_log_model_pricing_cached(conn, pricing_cache, log)? {
            Some(info) => info,
            None => return Ok(false),
        };

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
        // 单价全为 0（免费模型）时算出来仍是 0，写回去下次照样当缺价，只是白白 UPDATE。
        if total_cost.is_zero() {
            return Ok(false);
        }

        // 至少 6 位小数、不截断：只存 6 位会把极小额（几个 token、低倍率）写成 0，
        // 下次读取又被当成缺价重算。界面显示时再舍入。
        fn cost_string(value: rust_decimal::Decimal) -> String {
            let value = value.normalize();
            if value.scale() < 6 {
                format!("{value:.6}")
            } else {
                value.to_string()
            }
        }
        log.input_cost_usd = cost_string(input_cost);
        log.output_cost_usd = cost_string(output_cost);
        log.cache_read_cost_usd = cost_string(cache_read_cost);
        log.cache_creation_cost_usd = cost_string(cache_creation_cost);
        log.total_cost_usd = cost_string(total_cost);

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct PricingCandidate {
    model_id: String,
    allow_prefix_match: bool,
}

/// 直接定价查询：只认精确匹配及命名空间/版本/日期/点号标准化后的精确匹配，
/// 不走推理档位回退，也不走 `LIKE 'base-%'` 前缀匹配，避免缺失基础定价时借用邻近档位价格。
fn find_direct_model_pricing_row(
    conn: &impl PricingLookup,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    let cleaned = clean_model_id_for_pricing(model_id);
    if is_placeholder_pricing_model(&cleaned) {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    let mut queue = VecDeque::from([cleaned]);
    while let Some(candidate) = queue.pop_front() {
        if candidate.is_empty() || candidates.iter().any(|existing| existing == &candidate) {
            continue;
        }
        candidates.push(candidate.clone());
        if let Some(stripped) = strip_known_model_namespace(&candidate) {
            queue.push_back(stripped);
        }
        if let Some(stripped) = strip_claude_desktop_non_anthropic_prefix(&candidate) {
            queue.push_back(stripped);
        }
        if let Some(stripped) = strip_bedrock_model_version_suffix(&candidate) {
            queue.push_back(stripped);
        }
        if candidate.starts_with("claude-")
            && candidate.contains('.')
            && strip_claude_desktop_non_anthropic_prefix(&candidate).is_none()
        {
            queue.push_back(candidate.replace('.', "-"));
        }
        if let Some(stripped) = strip_model_date_suffix(&candidate) {
            queue.push_back(stripped);
        }
    }

    for candidate in &candidates {
        if let Some(row) = conn.exact(candidate)? {
            return Ok(Some(row));
        }
    }

    Ok(None)
}

pub(crate) fn find_model_pricing_row(
    conn: &Connection,
    model: &str,
) -> Result<Option<PricingRow>, AppError> {
    lookup_model_pricing(conn, model)
}

#[cfg(test)]
fn statistics_model(conn: &Connection, model: &str) -> Result<String, AppError> {
    statistics_model_with(conn, model)
}

fn lookup_model_pricing(
    conn: &impl PricingLookup,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    let candidates = model_pricing_candidates(model_id);
    if candidates.is_empty() {
        return Ok(None);
    }

    for candidate in &candidates {
        if let Some(row) = conn.exact(&candidate.model_id)? {
            return Ok(Some(row));
        }
    }

    for candidate in &candidates {
        if candidate.allow_prefix_match && should_try_pricing_prefix_match(&candidate.model_id) {
            if let Some(row) = conn.prefix(&candidate.model_id)? {
                return Ok(Some(row));
            }
        }
    }

    Ok(None)
}

/// 精准回填的行筛选：log 的任一模型字段归一化后与目标模型的 candidates 相交，
/// 或可按查价层的前缀规则命中目标，即视为相关。镜像 find_model_pricing_row 的
/// 匹配语义，宁可误纳（后续查价会兜底）不可漏筛。
fn log_pricing_scope_matches(
    log: &RequestLogDetail,
    target_candidates: &[PricingCandidate],
) -> bool {
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
                target.model_id == candidate.model_id
                    || (candidate.allow_prefix_match
                        && should_try_pricing_prefix_match(&candidate.model_id)
                        && target
                            .model_id
                            .strip_prefix(candidate.model_id.as_str())
                            .is_some_and(|rest| rest.starts_with('-')))
            })
        })
    })
}

pub(crate) fn is_placeholder_pricing_model(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    normalized.is_empty() || matches!(normalized.as_str(), "unknown" | "null" | "none")
}

type PricingRow = (String, String, String, String);

trait PricingLookup {
    fn exact(&self, model: &str) -> Result<Option<PricingRow>, AppError>;
    fn prefix(&self, model: &str) -> Result<Option<PricingRow>, AppError>;
}

impl PricingLookup for Connection {
    fn exact(&self, model: &str) -> Result<Option<PricingRow>, AppError> {
        query_model_pricing_exact(self, model)
    }
    fn prefix(&self, model: &str) -> Result<Option<PricingRow>, AppError> {
        query_model_pricing_prefix(self, model)
    }
}

/// A statistics request owns one immutable pricing snapshot. Every reasoning
/// suffix lookup is then a hash lookup, and subsequent requests see all edits,
/// imports and transaction rollbacks without a second invalidation protocol.
struct PricingCatalog {
    exact: HashMap<String, PricingRow>,
    prefixes: HashMap<String, PricingRow>,
}

impl PricingCatalog {
    fn load(conn: &Connection) -> Result<Self, AppError> {
        let mut stmt = conn.prepare(
            "SELECT model_id, input_cost_per_million, output_cost_per_million,
            cache_read_cost_per_million, cache_creation_cost_per_million FROM model_pricing
            ORDER BY LENGTH(model_id), rowid",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ),
            ))
        })?;
        let mut catalog = Self {
            exact: HashMap::new(),
            prefixes: HashMap::new(),
        };
        for row in rows {
            let (model, pricing) = row?;
            let model = model.to_ascii_lowercase();
            for (at, _) in model.match_indices('-') {
                catalog
                    .prefixes
                    .entry(model[..at].to_string())
                    .or_insert_with(|| pricing.clone());
            }
            catalog.exact.entry(model).or_insert(pricing);
        }
        Ok(catalog)
    }
}

impl PricingLookup for PricingCatalog {
    fn exact(&self, model: &str) -> Result<Option<PricingRow>, AppError> {
        Ok(self.exact.get(&model.to_ascii_lowercase()).cloned())
    }
    fn prefix(&self, model: &str) -> Result<Option<PricingRow>, AppError> {
        Ok(self.prefixes.get(&model.to_ascii_lowercase()).cloned())
    }
}

fn query_model_pricing_exact(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<(String, String, String, String)>, AppError> {
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE model_id = ?1 COLLATE NOCASE ORDER BY rowid LIMIT 1",
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
    let escaped = model_id
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("{escaped}-%");
    conn.query_row(
        "SELECT input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         WHERE model_id LIKE ?1 ESCAPE '\\'
         ORDER BY LENGTH(model_id) ASC, rowid
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

fn model_pricing_candidates(model_id: &str) -> Vec<PricingCandidate> {
    let cleaned = clean_model_id_for_pricing(model_id);
    if is_placeholder_pricing_model(&cleaned) {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut queue = VecDeque::from([PricingCandidate {
        model_id: cleaned,
        allow_prefix_match: true,
    }]);

    while let Some(candidate) = queue.pop_front() {
        let allow_prefix_match = candidate.allow_prefix_match;
        if !push_unique_candidate(&mut candidates, candidate.clone()) {
            continue;
        }
        let candidate = candidate.model_id;
        let push_derived = |queue: &mut VecDeque<PricingCandidate>, model_id: String| {
            queue.push_back(PricingCandidate {
                model_id,
                allow_prefix_match,
            });
        };

        if let Some(stripped) = strip_known_model_namespace(&candidate) {
            push_derived(&mut queue, stripped);
        }
        if let Some(stripped) = strip_claude_desktop_non_anthropic_prefix(&candidate) {
            push_derived(&mut queue, stripped);
        }
        if let Some(stripped) = strip_bedrock_model_version_suffix(&candidate) {
            push_derived(&mut queue, stripped);
        }
        if candidate.starts_with("claude-")
            && candidate.contains('.')
            && strip_claude_desktop_non_anthropic_prefix(&candidate).is_none()
        {
            push_derived(&mut queue, candidate.replace('.', "-"));
        }
        if let Some(stripped) = strip_model_date_suffix(&candidate) {
            push_derived(&mut queue, stripped);
        }
        if let Some(stripped) = strip_reasoning_effort_suffix(&candidate) {
            push_derived(&mut queue, stripped);
        }
        // AGY's built-in subagents (such as browser) and native generations report
        // Gemini Flash thinking-level endpoints as `gemini-<ver>-flash-tiered`.
        // Keep the raw ID and any exact custom price; otherwise fall back only to an
        // exact `gemini-<ver>-flash` row so missing base Flash models never prefix-match
        // sibling `gemini-<ver>-flash-lite` or custom sub-tier entries.
        if let Some(stripped) = strip_gemini_flash_tiered_suffix(&candidate) {
            queue.push_back(PricingCandidate {
                model_id: stripped,
                allow_prefix_match: false,
            });
        }
        // AGY records some Hydrogen generations as `gemini-4-hydrogen-tiered[-<effort>]`.
        // FIFO keeps exact variant prices ahead of this exact-only base estimate.
        if candidate == "gemini-4-hydrogen-tiered" {
            queue.push_back(PricingCandidate {
                model_id: "gemini-4-hydrogen".to_string(),
                allow_prefix_match: false,
            });
        }
        // AGY's "Gemini 4 Hydrogen (Orchestrator)" and per-step worker alias
        // `gemini-flash-worker-agy` are the same Hydrogen worker.
        // Keep the raw ID and any exact custom price; otherwise fall back only to an
        // exact `gemini-4-hydrogen` row.
        if matches!(
            candidate.as_str(),
            "gemini-4-hydrogen-orchestrator" | "gemini-flash-worker-agy"
        ) {
            queue.push_back(PricingCandidate {
                model_id: "gemini-4-hydrogen".to_string(),
                allow_prefix_match: false,
            });
        }
        // AGY records "Claude Opus 4.6 (Thinking)" as `claude-opus-4.6-thinking`. Extended
        // thinking is billed at the model's own rates, so price it exactly like the base
        // Claude ID, including the usual short-ID-to-dated-row match (Sonnet 4.5 only has a
        // dated row). Only Claude: other `-thinking` IDs (such as `kimi-k2-thinking`) are
        // separately priced models.
        if let Some(stripped) = strip_claude_thinking_suffix(&candidate) {
            push_derived(&mut queue, stripped);
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

fn push_unique_candidate(
    candidates: &mut Vec<PricingCandidate>,
    candidate: PricingCandidate,
) -> bool {
    if candidate.model_id.is_empty() {
        return false;
    }
    if let Some(existing) = candidates
        .iter_mut()
        .find(|existing| existing.model_id == candidate.model_id)
    {
        if !existing.allow_prefix_match && candidate.allow_prefix_match {
            existing.allow_prefix_match = true;
            return true;
        }
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

const REASONING_EFFORT_SUFFIXES: &[&str] = &["-minimal", "-low", "-medium", "-high", "-xhigh"];

fn strip_ascii_case_insensitive_suffix<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    if s.len() <= suffix.len() {
        return None;
    }
    let split = s.len() - suffix.len();
    if !s.is_char_boundary(split) {
        return None;
    }
    s[split..]
        .eq_ignore_ascii_case(suffix)
        .then_some(&s[..split])
}

fn strip_reasoning_effort_suffix(model_id: &str) -> Option<String> {
    for suffix in REASONING_EFFORT_SUFFIXES {
        if let Some(stripped) = strip_ascii_case_insensitive_suffix(model_id, suffix) {
            return Some(stripped.to_string());
        }
    }
    None
}

fn strip_statistics_tiered_suffix(model_id: &str) -> Option<&str> {
    let stripped = strip_ascii_case_insensitive_suffix(model_id, "-tiered")?;
    let tail = model_id.rsplit_once('/').map_or(model_id, |(_, r)| r);
    let lower_tail = tail.to_ascii_lowercase();
    let normalized_tail = strip_known_model_namespace(&lower_tail).unwrap_or(lower_tail);
    let is_known_tiered = normalized_tail == "gemini-4-hydrogen-tiered"
        || strip_gemini_flash_tiered_suffix(&normalized_tail).is_some();
    is_known_tiered.then_some(stripped)
}

fn reasoning_tier_merge_targets(model_id: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let after_effort = REASONING_EFFORT_SUFFIXES
        .iter()
        .find_map(|suffix| strip_ascii_case_insensitive_suffix(model_id, suffix));
    if let Some(base_after_effort) = after_effort {
        if let Some(base_after_tiered) = strip_statistics_tiered_suffix(base_after_effort) {
            targets.push(base_after_tiered);
        }
        targets.push(base_after_effort);
    } else if let Some(base_after_tiered) = strip_statistics_tiered_suffix(model_id) {
        targets.push(base_after_tiered);
    }
    targets
}

fn strip_gemini_flash_tiered_suffix(model_id: &str) -> Option<String> {
    let version = model_id
        .strip_prefix("gemini-")?
        .strip_suffix("-flash-tiered")?;
    (!version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
    .then(|| format!("gemini-{version}-flash"))
}

fn strip_claude_thinking_suffix(model_id: &str) -> Option<String> {
    if !model_id.starts_with("claude-")
        || strip_claude_desktop_non_anthropic_prefix(model_id).is_some()
    {
        return None;
    }
    let base = model_id.strip_suffix("-thinking")?;
    (base.len() > "claude-".len()).then(|| base.to_string())
}

fn should_try_pricing_prefix_match(model_id: &str) -> bool {
    if matches!(
        model_id,
        "gemini-4-hydrogen-tiered" | "gemini-4-hydrogen-orchestrator" | "gemini-flash-worker-agy"
    ) || strip_gemini_flash_tiered_suffix(model_id).is_some()
    {
        return false;
    }

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
mod tests;
