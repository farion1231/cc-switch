//! Grok Build (Grok CLI) 会话用量追踪
//!
//! 从 `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/updates.jsonl`
//! 的 `turn_completed` 事件中提取用量，写入 proxy_request_logs，实现官方
//! OAuth 直连态（无代理数据）下的用量统计。
//!
//! ## 数据流
//! ```text
//! updates.jsonl（逐轮 turn_completed） → 沉降窗/接管守卫 → 费用计算 →
//! proxy_request_logs + canonical session nodes/rollups
//! ```
//!
//! ## 事件口径（2026-07-23 单进程双 prompt 实测 + CLI 二进制逆向双重确证）
//! - `sessionUpdate == "turn_completed"` 事件的 usage 是【该 user prompt 一轮
//!   的独立总量】：轮内跨 inference loop 累加（`modelCalls`/`numTurns` = 本轮
//!   loop 数），下一轮从零起算。【不是】进程或会话累计——进程累计走 CLI 内
//!   另一条独立通道（`GetSessionUsage`，"since start or last resume"），不落
//!   updates.jsonl。🔴 勿改回相邻事件差分：那是把每轮总量误当累计快照，会把
//!   第二轮记成两轮之差造成巨量漏记（曾犯，实测单进程双 prompt 证伪）。
//! - 逐事件按面值入账即为正确的逐轮记录；两轮数值完全相同 = 两笔真实用量，
//!   照常都入账。
//! - `reasoningTokens` ⊂ `outputTokens`（totalTokens = input + output，且
//!   costUsdTicks 反推 output 未加计 reasoning），不参与计费。
//! - `costUsdTicks`（1 tick = 1e-10 USD）是 CLI 自报的本轮精确成本，6 个实测
//!   样本与本地定价 grok-4.5-build 2/6/0.30 分毫不差。**有自报且完整时
//!   total_cost 以自报为准**（回填只补 total<=0 的行、不修正错价，入账后无
//!   修复路径，所以定价漂移窗口不能押在本地价上）；本地定价负责 legacy
//!   raw compatibility 分项成本与漂移告警。`costIsPartial` 标记自报为下界：
//!   raw compatibility 可按既有规则回退本地价，但 canonical fact 只接受
//!   完整自报 ticks；partial 或未报告成本保持 `NULL` 并记录 cost_status，明确
//!   报告的零 tick 才写入精确字符串 `"0"`。cache creation 未被 Grok source
//!   证明，canonical fact 始终为 `NULL`，因此即使 cost 完整仍是 partial usage。
//! - 防接管态双算不用指纹去重：接管态下 CLI 照写 updates.jsonl，但轮事件是
//!   聚合值（多 loop 求和），与代理逐请求行结构性不相等。改用「沉降窗 +
//!   接管活动时间窗守卫」：只导入足够旧的事件（届时接管态的代理行必已
//!   落库），插入前按事件时刻查询附近是否存在代理直录行（见
//!   `has_recent_grokbuild_proxy_activity`）。
//! - 每个 `turn_completed × model` 以 `RequestExact` / `EventTime` / `AgentCall`
//!   写入 canonical bucket；先按完整日期/模型/source key 聚合，不能逐 turn
//!   覆盖同桶。`modelCalls` 仅保留上游 metadata，绝不冒充 HTTP 请求数。
//! - durable session ID 只有在 `summary.json.info.id == updates` 目录名时才
//!   接受；所有 Grok 节点都是 self-only/standalone。缺失或冲突的 summary
//!   fail closed，避免不同 cwd 的目录被误合并。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::agent_session_usage::{SessionNodeMetadata, SessionRelationClaim};
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::session_usage_pipeline::{
    publish_canonical_batch, CanonicalReplaceScope, CanonicalUsageBatch, UsagePublishTarget,
    UsageSourceSpec,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::{
    find_model_pricing, has_recent_grokbuild_proxy_activity, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;

const GROK_APP_TYPE: &str = "grokbuild";
const GROK_SESSION_DATA_SOURCE: &str = "grok_session";
const GROK_SESSION_PROVIDER_ID: &str = "_grok_session";

/// 事件沉降窗：只导入早于「现在 − 窗口」的事件。
///
/// 接管态下 CLI 照写 updates.jsonl，同一请求代理已逐请求记账；代理行与
/// 会话事件几乎同时产生，若导入抢在代理行落库前运行，接管守卫会因查不到
/// 代理行而放行，双算永久留存。让事件先「沉降」再导入后，守卫查询必然
/// 能看到已落库的代理行，竞态从源头消除。代价：官方态用量最多延迟约一个
/// 窗口 + 一次后台同步周期（60s）上屏。
const SETTLE_WINDOW_SECONDS: i64 = SESSION_PROXY_DEDUP_WINDOW_SECONDS;

/// 单个模型的本轮用量（从 `modelUsage` 或顶层 usage 提取，均为逐轮口径）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GrokCounters {
    input: u64,
    output: u64,
    cached: u64,
    /// `reasoningTokens` is a source-provided subset of output.  Preserve it
    /// independently when present, but never add it to output or totals.
    reasoning: Option<u64>,
    api_ms: u64,
    model_calls: u64,
    /// CLI 自报本轮成本，1 tick = 1e-10 USD；是否报告由 `cost_reported`
    /// 区分，因而可以保留明确报告的零成本。
    cost_ticks: u64,
    /// Whether `costUsdTicks` was present in the source payload.  A reported
    /// zero is meaningful (`Some("0")` in canonical buckets), whereas a
    /// missing field must remain `None`.
    cost_reported: bool,
    /// 上游标记 cost_ticks 只是部分费用（`costIsPartial`）：此时它是下界
    cost_partial: bool,
    /// Durable buckets require all token components to be present.  The raw
    /// legacy table cannot represent unknown token components without
    /// fabricating zeros, so such a model entry is skipped entirely.
    tokens_known: bool,
}

impl GrokCounters {
    fn reported_cost_usd(&self) -> Option<Decimal> {
        self.cost_reported
            .then(|| Decimal::from(self.cost_ticks) / Decimal::from(10_000_000_000u64))
    }
}

/// 一条 `turn_completed` 用量事件
#[derive(Debug)]
struct GrokUsageEvent {
    created_at: i64,
    prompt_id: String,
    /// 事件级 `costIsPartial`（顶层 usage 上观测到的位置；对本事件全部模型生效）
    cost_is_partial: bool,
    per_model: Vec<(String, GrokCounters)>,
}

/// Summary metadata is the only accepted proof that an updates directory name
/// is the stable Grok session ID.  Without it the adapter fails closed rather
/// than merging similarly named directories from different projects.
#[derive(Debug, Clone)]
struct GrokSessionIdentity {
    session_id: String,
    project_dir: Option<String>,
    title: Option<String>,
    created_at: Option<i64>,
    last_active_at: Option<i64>,
    summary_path: PathBuf,
}

/// Durable request evidence retained in `proxy_request_logs`.  This is the
/// source of truth for canonical request identity when an updates.jsonl
/// rewrite no longer contains a previously imported prompt.
#[derive(Debug, Clone)]
struct GrokDurableRawRow {
    request_id: String,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    total_cost_usd: Option<Decimal>,
    created_at: i64,
}

fn load_grok_durable_raw_rows(
    db: &Database,
    session_id: &str,
) -> Result<Vec<GrokDurableRawRow>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut statement = conn
        .prepare(
            "SELECT request_id, model, input_tokens, output_tokens,
                    cache_read_tokens, total_cost_usd, created_at
             FROM proxy_request_logs
             WHERE app_type = ?1 AND data_source = ?2 AND provider_id = ?3
               AND session_id = ?4
             ORDER BY request_id",
        )
        .map_err(|error| AppError::Database(format!("读取 Grok durable raw rows 失败: {error}")))?;
    let rows = statement
        .query_map(
            rusqlite::params![
                GROK_APP_TYPE,
                GROK_SESSION_DATA_SOURCE,
                GROK_SESSION_PROVIDER_ID,
                session_id
            ],
            |row| {
                let total_cost = row
                    .get::<_, Option<String>>(5)?
                    .and_then(|value| Decimal::from_str(&value).ok());
                Ok(GrokDurableRawRow {
                    request_id: row.get(0)?,
                    model: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    cache_read_tokens: row.get(4)?,
                    total_cost_usd: total_cost,
                    created_at: row.get(6)?,
                })
            },
        )
        .map_err(|error| AppError::Database(format!("读取 Grok durable raw rows 失败: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(format!("解析 Grok durable raw rows 失败: {error}")))?;
    Ok(rows)
}

fn grok_source() -> UsageSourceSpec {
    let mut source = UsageSourceSpec::new(
        GROK_APP_TYPE,
        GROK_SESSION_PROVIDER_ID,
        GROK_SESSION_DATA_SOURCE,
        crate::services::agent_session_usage::UsagePrecision::RequestExact,
        crate::services::agent_session_usage::TimeSemantics::EventTime,
        crate::services::agent_session_usage::RequestCountSemantics::AgentCall,
    );
    source.input_token_semantics = INPUT_TOKEN_SEMANTICS_TOTAL;
    source
}

fn grok_local_date(created_at: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(created_at, 0)
        .map(|utc| utc.with_timezone(&chrono::Local).date_naive().to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn grok_fact_from_turn(
    session_id: &str,
    model: &str,
    turn: &GrokCounters,
    cost_is_partial: bool,
    created_at: i64,
) -> crate::services::agent_session_usage::NormalizedUsageRollupFact {
    let mut fact = grok_source().fact(
        grok_local_date(created_at),
        session_id,
        model,
        model,
        String::new(),
    );
    fact.request_count = Some(1);
    fact.input_tokens = Some(turn.input.min(i64::MAX as u64) as i64);
    fact.output_tokens = Some(turn.output.min(i64::MAX as u64) as i64);
    fact.cache_read_tokens = Some(turn.cached.min(i64::MAX as u64) as i64);
    fact.reasoning_tokens = turn
        .reasoning
        .map(|value| value.min(i64::MAX as u64) as i64);
    let reported_cost = (!cost_is_partial)
        .then(|| turn.reported_cost_usd())
        .flatten();
    fact.total_cost_usd = reported_cost.map(|cost| cost.to_string());
    fact.cost_status = Some(
        if cost_is_partial {
            "partial"
        } else if reported_cost.is_some() {
            "complete"
        } else {
            "unknown"
        }
        .to_string(),
    );
    fact.cost_source = Some(GROK_SESSION_DATA_SOURCE.to_string());
    fact.first_event_at = Some(created_at);
    fact.last_event_at = Some(created_at);
    fact
}

fn grok_fact_from_durable_row(
    session_id: &str,
    row: &GrokDurableRawRow,
) -> crate::services::agent_session_usage::NormalizedUsageRollupFact {
    let mut fact = grok_source().fact(
        grok_local_date(row.created_at),
        session_id,
        row.model.clone(),
        row.model.clone(),
        String::new(),
    );
    fact.request_count = Some(1);
    fact.input_tokens = Some(row.input_tokens.max(0));
    fact.output_tokens = Some(row.output_tokens.max(0));
    fact.cache_read_tokens = Some(row.cache_read_tokens.max(0));
    // A compatibility raw row does not prove a source reasoning count or a
    // complete reported-cost quality, even if it retains a legacy total.
    fact.total_cost_usd = row.total_cost_usd.map(|cost| cost.to_string());
    fact.cost_status = Some("partial".to_string());
    fact.cost_source = Some(GROK_SESSION_DATA_SOURCE.to_string());
    fact.first_event_at = Some(row.created_at);
    fact.last_event_at = Some(row.created_at);
    fact
}

fn write_grok_rollups(
    db: &Database,
    session_id: &str,
    admitted_turns: HashMap<String, (GrokCounters, bool)>,
) -> Result<(), AppError> {
    // The current updates.jsonl scan is not a durable history boundary: a
    // source rewind can remove an already imported prompt while appending a
    // new one.  Rebuild every canonical bucket from all retained provider raw
    // rows, using current events only for richer per-request metadata.
    let durable_rows = load_grok_durable_raw_rows(db, session_id)?;
    if durable_rows.is_empty() {
        // Do not erase an older canonical generation when the compatibility
        // raw rows have already been pruned; there is no evidence to rebuild
        // it from in this sync pass.
        return Ok(());
    }

    let mut batch = CanonicalUsageBatch::default();
    batch.replace_scopes.push(CanonicalReplaceScope {
        app_type: GROK_APP_TYPE.to_string(),
        session_id: session_id.to_string(),
        data_source: GROK_SESSION_DATA_SOURCE.to_string(),
    });
    for row in &durable_rows {
        if let Some((turn, cost_is_partial)) = admitted_turns.get(&row.request_id) {
            batch.replace_observe(
                row.request_id.clone(),
                grok_fact_from_turn(
                    session_id,
                    &row.model,
                    turn,
                    *cost_is_partial,
                    row.created_at,
                ),
            );
        } else {
            // Historical rows still prove request/token/time identity, but do
            // not prove source reasoning or cost-report quality.  The raw
            // fallback preserves any stored legacy total while marking the
            // resulting bucket partial.
            batch.replace_observe(
                row.request_id.clone(),
                grok_fact_from_durable_row(session_id, row),
            );
            log::warn!(
                "[GROK-SYNC] canonical rebuild uses durable raw fallback for request_id={}",
                row.request_id
            );
        }
    }

    publish_canonical_batch(
        db,
        UsagePublishTarget::Published,
        batch,
        "Grok canonical 覆盖",
    )
}

fn parse_summary_timestamp(value: Option<&serde_json::Value>) -> Option<i64> {
    parse_event_timestamp(value)
}

fn read_grok_session_identity(file_path: &Path) -> Result<Option<GrokSessionIdentity>, AppError> {
    let Some(session_dir) = file_path.parent() else {
        return Ok(None);
    };
    let Some(folder_id) = session_dir.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    if folder_id.trim().is_empty() {
        return Ok(None);
    }
    let summary_path = session_dir.join("summary.json");
    let text = match fs::read_to_string(&summary_path) {
        Ok(text) => text,
        Err(error) => {
            log::warn!(
                "[GROK-SYNC] 缺少或无法读取 summary.json，跳过未证明的会话 {}: {error}",
                session_dir.display()
            );
            return Ok(None);
        }
    };
    let summary: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[GROK-SYNC] summary.json 解析失败，跳过未证明的会话 {}: {error}",
                summary_path.display()
            );
            return Ok(None);
        }
    };
    let Some(summary_id) = summary
        .get("info")
        .and_then(|info| info.get("id"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        log::warn!(
            "[GROK-SYNC] summary.info.id 缺失，跳过未证明的会话 {}",
            summary_path.display()
        );
        return Ok(None);
    };
    if summary_id != folder_id {
        log::warn!(
            "[GROK-SYNC] summary.info.id 与 updates 目录不一致，跳过会话: folder={} summary={}",
            folder_id,
            summary_id
        );
        return Ok(None);
    }

    let project_dir = summary
        .get("info")
        .and_then(|info| info.get("cwd"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let title = summary
        .get("generated_title")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            summary
                .get("session_summary")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
        })
        .map(str::to_string);
    let created_at = parse_summary_timestamp(summary.get("created_at"));
    let last_active_at = summary
        .get("last_active_at")
        .or_else(|| summary.get("updated_at"))
        .and_then(|value| parse_summary_timestamp(Some(value)));

    Ok(Some(GrokSessionIdentity {
        session_id: folder_id.to_string(),
        project_dir,
        title,
        created_at,
        last_active_at,
        summary_path,
    }))
}

fn grok_identity_conflicts(
    db: &Database,
    identity: &GrokSessionIdentity,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    let existing: Option<Option<String>> = conn
        .query_row(
            "SELECT project_dir FROM agent_session_nodes
             WHERE app_type = ?1 AND session_id = ?2",
            rusqlite::params![GROK_APP_TYPE, &identity.session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| AppError::Database(format!("读取 Grok 会话节点身份失败: {error}")))?;
    Ok(match (existing, identity.project_dir.as_deref()) {
        // No durable node exists yet, so this is the first observation and
        // cannot conflict with a prior source identity.
        (None, _) => false,
        (Some(Some(existing)), Some(incoming)) => existing.as_str() != incoming,
        (Some(None), None) => false,
        // If an existing node has only one side of cwd evidence, identity
        // cannot be proven equivalent; fail closed instead of folding the
        // sessions together.
        (Some(None), Some(_)) | (Some(Some(_)), None) => true,
    })
}

fn write_grok_session_node(
    db: &Database,
    identity: &GrokSessionIdentity,
    synced_at: i64,
) -> Result<bool, AppError> {
    if grok_identity_conflicts(db, identity)? {
        log::warn!(
            "[GROK-SYNC] 相同 session ID 出现不同 cwd，拒绝跨项目合并: {}",
            identity.session_id
        );
        return Ok(false);
    }

    let mut claim = SessionRelationClaim::standalone(GROK_APP_TYPE, &identity.session_id);
    claim.metadata = SessionNodeMetadata {
        title: identity.title.clone(),
        project_dir: identity.project_dir.clone(),
        source_path: Some(identity.summary_path.to_string_lossy().to_string()),
        created_at: identity.created_at,
        last_active_at: identity.last_active_at,
        last_synced_at: synced_at,
    };
    publish_canonical_batch(
        db,
        UsagePublishTarget::Published,
        CanonicalUsageBatch {
            relation_claims: vec![claim],
            ..CanonicalUsageBatch::default()
        },
        "Grok 会话节点",
    )?;
    Ok(true)
}

fn grok_raw_row_exists(db: &Database, request_id: &str) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM proxy_request_logs
            WHERE request_id = ?1 AND app_type = ?2 AND data_source = ?3
        )",
        rusqlite::params![request_id, GROK_APP_TYPE, GROK_SESSION_DATA_SOURCE],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("读取 Grok raw 行状态失败: {error}")))
}

/// 同步 Grok Build 使用数据（从 updates.jsonl 会话日志）
pub fn sync_grokbuild_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = collect_grok_updates_files();

    let mut result = SessionSyncResult {
        files_scanned: files.len() as u32,
        ..Default::default()
    };

    for file_path in &files {
        match sync_single_grok_file(db, file_path) {
            Ok(file_result) => result.merge(file_result),
            Err(e) => {
                let msg = format!("Grok Build 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[GROK-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[GROK-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件, 延后 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned,
            result.deferred_files
        );
    }

    Ok(result)
}

/// 收集所有 Grok 会话的 updates.jsonl（含归档会话，与会话浏览器同根）
fn collect_grok_updates_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in crate::session_manager::providers::grokbuild::session_roots() {
        collect_files_named(&root, "updates.jsonl", &mut files, 0);
    }
    files
}

/// 单个 updates.jsonl 文件读取上限（50 MiB）。JSONL 单行事件通常几 KiB，
/// 正常活跃会话数月也到不了这个量级；超过则视为异常/恶意文件，跳过。
const MAX_GROK_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// 递归收集 session 日志时的最大目录深度，防止 symlink 循环导致栈溢出。
const MAX_COLLECT_DEPTH: usize = 16;

/// 递归收集目录下指定文件名的文件（容忍布局深度变化，对齐会话浏览器的做法）
fn collect_files_named(root: &Path, name: &str, files: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_COLLECT_DEPTH {
        log::warn!(
            "Grok session directory traversal exceeded max depth {} at {}",
            MAX_COLLECT_DEPTH,
            root.display()
        );
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `entry.metadata()` 不跟随符号链接（不同于 `path.is_dir()`），这里据此
        // **无条件跳过一切 symlink**：目录 symlink 不递归（避免循环），文件
        // symlink 也不收集——同名文件若经 symlink 指向 sessions 根之外，会把用户
        // 意料之外的内容当作会话日志读入。代价：把 sessions 目录整体做成 symlink
        // 的用户会同步不到数据，所以跳过必须留日志，便于排查"用量数据静默缺失"。
        let metadata = entry.metadata();
        if metadata.as_ref().map(|m| m.is_symlink()).unwrap_or(false) {
            log::info!("[GROK-SYNC] 跳过符号链接（不跟随）: {}", path.display());
            continue;
        }
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        if is_dir {
            collect_files_named(&path, name, files, depth + 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            files.push(path);
        }
    }
}

/// 同步单个 updates.jsonl 文件
fn sync_single_grok_file(db: &Database, file_path: &Path) -> Result<SessionSyncResult, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 异常大文件直接跳过，避免一次性读取耗尽内存。
    if metadata.len() > MAX_GROK_FILE_BYTES {
        log::warn!(
            "Grok session log too large ({} bytes), skipping: {}",
            metadata.len(),
            file_path.display()
        );
        return Ok(SessionSyncResult::default());
    }

    let (last_modified, _last_offset) = get_sync_state(db, &file_path_str)?;
    if file_modified <= last_modified {
        return Ok(SessionSyncResult::default());
    }

    let Some(identity) = read_grok_session_identity(file_path)? else {
        // A matching summary.info.id is required before any raw or canonical
        // row is written; otherwise two similarly named project directories
        // could be merged under one durable session ID.
        return Ok(SessionSyncResult::default());
    };

    // 文件变更时全量重读：UPSERT 幂等使重读无害，且沉降窗延后的事件本就
    // 依赖下一轮重读补入。事件已是逐轮独立值，改 offset 增量读在正确性上
    // 可行（无差分基线依赖），但需另行处理延后事件的 offset 回退，收益
    // （活跃会话每周期省一次 O(N) 解析）暂不值得该复杂度。
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let events = parse_grok_usage_events(&content);

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if !write_grok_session_node(db, &identity, now)? {
        return Ok(SessionSyncResult::default());
    }

    let mut result = SessionSyncResult::default();
    let mut deferred = false;
    // Keep the latest admitted value for each stable prompt×model key.  The
    // raw UPSERT has the same last-value semantics for a (rare) duplicate
    // turn_completed prompt; aggregating directly while iterating would count
    // that replacement twice in the canonical bucket.
    let mut admitted_turns: HashMap<String, (GrokCounters, bool)> = HashMap::new();

    for (idx, event) in events.iter().enumerate() {
        // 沉降窗：事件按 append 顺序时间单调，遇到第一条未沉降的事件即停，
        // 后续事件与它一起等下一轮（保持"文件前缀已导入"的简单不变量）。
        // 已知局限：未来时间戳（时钟误设）会让该文件持续延后并整文件重扫，
        // 墙钟越过 事件时刻+窗口 后自愈；活跃会话每周期全量重读为设计代价。
        if now.saturating_sub(event.created_at) < SETTLE_WINDOW_SECONDS {
            deferred = true;
            break;
        }

        // 接管守卫按事件时刻判定一次，整条事件的所有模型行同进退；
        // 被守卫跳过的 token 已由代理行记账，跳过即终态（同步状态照常
        // 推进）。已知局限：守卫无 session 维度，见 usage_stats.rs 注释。
        let takeover_active = {
            let conn = lock_conn!(db.conn);
            has_recent_grokbuild_proxy_activity(&conn, event.created_at)?
        };

        for (model, turn) in &event.per_model {
            if !turn.tokens_known {
                // Canonical durable buckets require all token components; do
                // not turn an absent field into a fabricated zero.
                result.skipped += 1;
                continue;
            }
            if takeover_active {
                // 计入 skipped（对齐 gemini 指纹去重跳过的语义：未入账，代理
                // 行权威）。勿改用 suspected_duplicates——codex 对它的语义相反
                // （已入账待查），而 merge() 会把两义直接求和。
                result.skipped += 1;
                continue;
            }

            // 幂等键锚定上游稳定 ID（prompt_id 是每轮唯一的 UUID），不含文件
            // 内序号：updates.jsonl 前缀被改写（如 rewind 截断）导致事件序号
            // 前移时，幸存轮次仍命中原行不会双算；被移除轮次的行保留——
            // rewind 不退还已消耗的 token，留存即正确记账。若上游对同一
            // prompt_id 写多条 turn_completed（未观测到），UPSERT 取后者，
            // 方向是少记不双算。prompt_id 缺失时回退 "idx{N}"（UUID 形态的
            // prompt_id 不可能与之撞名）。
            let turn_key = if event.prompt_id.is_empty() {
                format!("idx{idx}")
            } else {
                event.prompt_id.clone()
            };
            let request_id = format!(
                "{GROK_SESSION_DATA_SOURCE}:{}:{turn_key}:{model}",
                identity.session_id
            );
            let raw_write_ok = match insert_grok_session_entry(
                db,
                &request_id,
                turn,
                event.cost_is_partial || turn.cost_partial,
                model,
                &identity.session_id,
                event.created_at,
            ) {
                Ok(true) => {
                    result.imported += 1;
                    true
                }
                Ok(false) => {
                    result.skipped += 1;
                    // `changes() == 0` is normally an idempotent Grok row,
                    // but can also mean a request_id collision with another
                    // data source rejected by the UPSERT guard.  Only the
                    // former is admitted for canonical coverage.
                    grok_raw_row_exists(db, &request_id)?
                }
                Err(e) => {
                    log::warn!("[GROK-SYNC] 插入失败 ({request_id}): {e}");
                    result.skipped += 1;
                    false
                }
            };

            // Aggregate all settled turn×model values after the scan crosses
            // the duplicate-key boundary.  One turn is one AgentCall
            // regardless of `modelCalls`; that field remains source metadata.
            if raw_write_ok {
                if let Some(existing) = admitted_turns.get_mut(&request_id) {
                    // Raw UPSERT keeps created_at from the first insert even
                    // when a duplicate prompt later replaces token values.
                    *existing = (*turn, event.cost_is_partial || turn.cost_partial);
                } else {
                    admitted_turns.insert(
                        request_id,
                        (*turn, event.cost_is_partial || turn.cost_partial),
                    );
                }
            }
        }
    }

    write_grok_rollups(db, &identity.session_id, admitted_turns)?;

    if deferred {
        // 不落同步状态：下一轮重读整个文件，把沉降后的事件补入。
        result.deferred_files += 1;
    } else {
        update_sync_state(db, &file_path_str, file_modified, events.len() as i64)?;
    }

    Ok(result)
}

/// 从 updates.jsonl 内容解析出全部逐轮用量事件（保持文件顺序）
fn parse_grok_usage_events(content: &str) -> Vec<GrokUsageEvent> {
    let mut events = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record.get("method").and_then(|v| v.as_str()) != Some("_x.ai/session/update") {
            continue;
        }
        let update = record.get("params").and_then(|p| p.get("update"));
        // 只认明确标记为 turn_completed 的事件（判别字段是 sessionUpdate，
        // serde internally-tagged）。字段缺失或显式标为其它类型的事件即使
        // 带 usage 也不导入——中途快照若与轮末事件并存，双导会双算。
        let kind = update
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str());
        if kind != Some("turn_completed") {
            continue;
        }
        let Some(usage) = update
            .and_then(|u| u.get("usage"))
            .filter(|u| u.is_object())
        else {
            continue;
        };
        // 沉降窗与接管守卫都依赖事件时刻，没有时间戳的事件无法安全导入。
        let Some(created_at) = parse_event_timestamp(record.get("timestamp")) else {
            continue;
        };

        let prompt_id = update
            .and_then(|u| u.get("prompt_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut per_model: Vec<(String, GrokCounters)> = usage
            .get("modelUsage")
            .and_then(|m| m.as_object())
            .map(|map| {
                map.iter()
                    .map(|(model, counters)| {
                        let model = if model.trim().is_empty() {
                            "unknown".to_string()
                        } else {
                            model.clone()
                        };
                        (model, parse_grok_counters(counters))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if per_model.is_empty() {
            // 缺 modelUsage 时退回顶层逐轮值；模型名未知，交由查价层兜底。
            per_model.push(("unknown".to_string(), parse_grok_counters(usage)));
        }
        // modelUsage 是 JSON object，遍历序不保证稳定；排序保证插入顺序
        // 与日志在多次重扫间确定。
        per_model.sort_by(|a, b| a.0.cmp(&b.0));

        events.push(GrokUsageEvent {
            created_at,
            prompt_id,
            cost_is_partial: usage
                .get("costIsPartial")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            per_model,
        });
    }

    events
}

fn parse_grok_counters(value: &serde_json::Value) -> GrokCounters {
    let get = |key: &str| value.get(key).and_then(|v| v.as_u64());
    let input = get("inputTokens");
    let output = get("outputTokens");
    let cached = get("cachedReadTokens");
    let reasoning = get("reasoningTokens");
    let api_ms = get("apiDurationMs");
    let model_calls = get("modelCalls");
    let cost_ticks = get("costUsdTicks");
    GrokCounters {
        input: input.unwrap_or(0),
        output: output.unwrap_or(0),
        cached: cached.unwrap_or(0),
        reasoning,
        api_ms: api_ms.unwrap_or(0),
        model_calls: model_calls.unwrap_or(0),
        cost_ticks: cost_ticks.unwrap_or(0),
        cost_reported: cost_ticks.is_some(),
        cost_partial: value
            .get("costIsPartial")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        tokens_known: input.is_some() && output.is_some() && cached.is_some(),
    }
}

/// updates.jsonl 顶层 `timestamp` 实测为数字 epoch 秒（勿与 summary.json 的
/// RFC3339 字符串混淆）；字符串形态仅作防御性兜底。
fn parse_event_timestamp(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    if let Some(n) = value.as_i64() {
        // 防未来毫秒形态：超过 1e11 视作毫秒
        return Some(if n > 100_000_000_000 { n / 1000 } else { n });
    }
    value
        .as_str()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.timestamp())
}

/// 插入单条 Grok 会话记录到 proxy_request_logs
fn insert_grok_session_entry(
    db: &Database,
    request_id: &str,
    turn: &GrokCounters,
    cost_is_partial: bool,
    model: &str,
    session_id: &str,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    // `proxy_request_logs` predates the canonical nullable-cost contract and
    // keeps non-null legacy pricing/zero values.  Canonical rollups below use
    // the source `costUsdTicks` presence/partial state instead of copying this
    // compatibility representation.

    let clamp = |v: u64| v.min(u32::MAX as u64) as u32;
    let usage = TokenUsage {
        input_tokens: clamp(turn.input),
        output_tokens: clamp(turn.output),
        cache_read_tokens: clamp(turn.cached),
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_model_pricing(&conn, model);
    let multiplier = Decimal::from(1);
    let reported = turn.reported_cost_usd();
    // 插入成功（changed）后才发，避免重扫时重复刷日志
    let mut deferred_warn: Option<String> = None;

    // total_cost 取值优先级（🔴 回填机制只补 total<=0 的行、从不修正已有正值，
    // 见 backfill_missing_usage_costs；本导入器 UPSERT 也不因 cost 单独变化而
    // 更新——所以入账时就必须写对，事后没有修复路径）：
    // 1. 有自报且完整 → 以自报为准（上游 ground truth，定价漂移窗口内也准确；
    //    本地定价负责分项与漂移告警，漂移时分项与 total 允许暂不自洽）；
    // 2. 自报不完整（costIsPartial）→ 有本地价用本地全额复算（token 数完整），
    //    并抑制此时无意义的漂移告警；无价则仍用自报下界（好过记 0）；
    // 3. 无自报 → 本地复算；彻底无价才整单记 0。
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app(GROK_APP_TYPE, &usage, &p, multiplier);
            let total = match reported {
                Some(reported) if !cost_is_partial => {
                    // 偏差超 1%（微额下限 1e-6）即本地定价漂移——xAI 调价时
                    // 最早的可观测信号，提醒更新 seed/repair。
                    let tolerance = (reported * Decimal::new(1, 2)).max(Decimal::new(1, 6));
                    if (cost.total_cost - reported).abs() > tolerance {
                        deferred_warn = Some(format!(
                            "本地定价与 CLI 自报成本偏差超阈值，total 已以自报为准，请更新本地定价: model={model} local={} reported={reported} request_id={request_id}",
                            cost.total_cost
                        ));
                    }
                    reported
                }
                _ => cost.total_cost,
            };
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                total.to_string(),
            )
        }
        None => {
            // 未 seed 的新别名：token 照常入账；有自报成本时直接采用（分项
            // 记 0），彻底无价才整单记 0。xAI 内部别名会周期性变动
            // （grok-4.5-build 即先例），两种情况都要留下可排查的痕迹。
            let total = match reported {
                Some(reported) => {
                    if model != "unknown" {
                        let partial_note = if cost_is_partial {
                            "（上游标记为部分费用，实际为下界）"
                        } else {
                            ""
                        };
                        deferred_warn = Some(format!(
                            "模型定价未找到，采用 CLI 自报成本入账{partial_note}: model={model} total={reported} request_id={request_id}"
                        ));
                    }
                    reported.to_string()
                }
                None => {
                    if model != "unknown" {
                        deferred_warn = Some(format!(
                            "模型定价未找到且无自报成本，成本记 0: model={model} request_id={request_id}"
                        ));
                    }
                    "0".to_string()
                }
            };
            (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                total,
            )
        }
    };

    // UPSERT：重扫幂等；解析口径修正后重扫时更新既有行（token/成本/
    // latency；created_at 保持首插值不动，避免行在沉降窗与 rollup 边界间漂移）。
    // WHERE 的 data_source 守卫是纵深防御：request_id 前缀命名空间已隔离，
    // 万一撞上非本导入器的行也绝不改写它。
    // input_token_semantics 显式写 TOTAL——xAI 口径 inputTokens 含 cache read，
    // 与代理路径的 grokbuild 行（logger）保持同一语义，勿依赖列默认值。
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            latency_ms = excluded.latency_ms
        WHERE data_source = 'grok_session'
          AND (input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR total_cost_usd != excluded.total_cost_usd
           OR latency_ms != excluded.latency_ms
           OR model != excluded.model)",
        rusqlite::params![
            request_id,
            GROK_SESSION_PROVIDER_ID,
            GROK_APP_TYPE,
            model,
            model,
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            0i64,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            turn.api_ms.min(i64::MAX as u64) as i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            session_id,
            Some(GROK_SESSION_DATA_SOURCE),
            1i64,
            "1.0",
            created_at,
            GROK_SESSION_DATA_SOURCE,
            INPUT_TOKEN_SEMANTICS_TOTAL,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Grok Build 会话日志失败: {e}")))?;

    let changed = conn.changes() > 0;
    if changed {
        if let Some(msg) = deferred_warn {
            log::warn!("[GROK-SYNC] {msg}");
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    /// 早于沉降窗的固定基准时刻（2023-11-14T22:13:20Z）
    const OLD_EPOCH: i64 = 1_700_000_000;

    /// 顶层 timestamp 用真实的数字 epoch 秒格式（RFC3339 兜底见 parses 测试）
    fn usage_event_line(epoch: i64, prompt_id: &str, model_usage: &str) -> String {
        format!(
            r#"{{"timestamp":{epoch},"method":"_x.ai/session/update","params":{{"update":{{"sessionUpdate":"turn_completed","prompt_id":"{prompt_id}","stop_reason":"end_turn","usage":{{"modelUsage":{{{model_usage}}}}}}}}}}}"#
        )
    }

    /// 带事件级 costIsPartial 标记的变体
    fn usage_event_line_partial(epoch: i64, prompt_id: &str, model_usage: &str) -> String {
        format!(
            r#"{{"timestamp":{epoch},"method":"_x.ai/session/update","params":{{"update":{{"sessionUpdate":"turn_completed","prompt_id":"{prompt_id}","stop_reason":"end_turn","usage":{{"costIsPartial":true,"modelUsage":{{{model_usage}}}}}}}}}}}"#
        )
    }

    fn model_counters(model: &str, input: u64, output: u64, cached: u64, calls: u64) -> String {
        model_counters_with_ticks(model, input, output, cached, calls, 0)
    }

    fn three_turn_fixture() -> Vec<String> {
        vec![
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            ),
            usage_event_line(
                OLD_EPOCH + 60,
                "p2",
                &model_counters("grok-4.5-build", 200, 20, 0, 1),
            ),
            usage_event_line(
                OLD_EPOCH + 120,
                "p3",
                &model_counters("grok-4.5-build", 300, 30, 0, 1),
            ),
        ]
    }

    fn model_counters_with_ticks(
        model: &str,
        input: u64,
        output: u64,
        cached: u64,
        calls: u64,
        ticks: u64,
    ) -> String {
        let cost = if ticks == 0 {
            String::new()
        } else {
            format!(",\"costUsdTicks\":{ticks}")
        };
        format!(
            r#""{model}":{{"inputTokens":{input},"outputTokens":{output},"cachedReadTokens":{cached},"reasoningTokens":0,"modelCalls":{calls},"apiDurationMs":1000{cost}}}"#
        )
    }

    fn model_counters_with_reasoning(
        model: &str,
        input: u64,
        output: u64,
        cached: u64,
        reasoning: u64,
        calls: u64,
    ) -> String {
        format!(
            r#""{model}":{{"inputTokens":{input},"outputTokens":{output},"cachedReadTokens":{cached},"reasoningTokens":{reasoning},"modelCalls":{calls},"apiDurationMs":1000}}"#
        )
    }

    fn write_session_file(dir: &Path, session_id: &str, lines: &[String]) -> PathBuf {
        let session_dir = dir.join("sessions").join("enc-project").join(session_id);
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        std::fs::write(
            session_dir.join("summary.json"),
            format!(
                r#"{{"info":{{"id":"{session_id}","cwd":"C:/fixture"}},"generated_title":"fixture Grok session","created_at":"2023-11-14T22:13:20Z","last_active_at":"2023-11-14T22:13:20Z"}}"#
            ),
        )
        .expect("write summary.json");
        let path = session_dir.join("updates.jsonl");
        let mut file = std::fs::File::create(&path).expect("create updates.jsonl");
        for line in lines {
            writeln!(file, "{line}").expect("write line");
        }
        path
    }

    fn clear_sync_cursor(db: &Database) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute("DELETE FROM session_log_sync", [])?;
        Ok(())
    }

    /// (request_id, input, output, cache_read, input_token_semantics)
    type GrokSessionRow = (String, u32, u32, u32, i64);

    fn query_rows(db: &Database) -> Result<Vec<GrokSessionRow>, AppError> {
        let conn = lock_conn!(db.conn);
        let mut stmt = conn
            .prepare(
                "SELECT request_id, input_tokens, output_tokens, cache_read_tokens, input_token_semantics
                 FROM proxy_request_logs WHERE data_source = 'grok_session' ORDER BY request_id",
            )
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .expect("query")
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct CanonicalGrokRow {
        date: String,
        session_id: String,
        model: String,
        request_count: Option<i64>,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_creation_tokens: Option<i64>,
        reasoning_tokens: Option<i64>,
        input_token_semantics: i64,
        total_cost_usd: Option<String>,
        cost_status: Option<String>,
        cost_source: Option<String>,
        precision: String,
        time_semantics: String,
        request_count_semantics: String,
    }

    fn query_canonical_rows(db: &Database) -> Result<Vec<CanonicalGrokRow>, AppError> {
        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT date, session_id, model, request_count, input_tokens,
                    output_tokens, cache_read_tokens, cache_creation_tokens,
                    reasoning_tokens, input_token_semantics, total_cost_usd,
                    cost_status, cost_source, precision, time_semantics,
                    request_count_semantics
             FROM agent_session_usage_rollups
             WHERE app_type = 'grokbuild' AND data_source = 'grok_session'
             ORDER BY date, session_id, model",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CanonicalGrokRow {
                    date: row.get(0)?,
                    session_id: row.get(1)?,
                    model: row.get(2)?,
                    request_count: row.get(3)?,
                    input_tokens: row.get(4)?,
                    output_tokens: row.get(5)?,
                    cache_read_tokens: row.get(6)?,
                    cache_creation_tokens: row.get(7)?,
                    reasoning_tokens: row.get(8)?,
                    input_token_semantics: row.get(9)?,
                    total_cost_usd: row.get(10)?,
                    cost_status: row.get(11)?,
                    cost_source: row.get(12)?,
                    precision: row.get(13)?,
                    time_semantics: row.get(14)?,
                    request_count_semantics: row.get(15)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn query_coverage_markers(db: &Database) -> Result<Vec<(String, Option<String>)>, AppError> {
        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT request_id, canonical_session_id
             FROM agent_session_canonical_coverage
             WHERE app_type = 'grokbuild' AND data_source = 'grok_session'
             ORDER BY request_id",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    #[test]
    fn parses_turn_completed_and_ignores_noise_and_other_kinds() {
        let content = concat!(
            "{\"timestamp\":\"2026-07-20T13:26:10Z\",\"method\":\"session/update\",\"params\":{\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{}}}}\n",
            "not json at all\n",
            // 缺少 sessionUpdate 的 usage 事件也不接受，必须显式 turn_completed
            "{\"timestamp\":\"2026-07-20T13:26:22Z\",\"method\":\"_x.ai/session/update\",\"params\":{\"update\":{\"prompt_id\":\"missing-kind\",\"usage\":{\"inputTokens\":9999,\"outputTokens\":9,\"cachedReadTokens\":0}}}}\n",
            // 显式标为非 turn_completed 却带 usage：防中途快照双算，不得导入
            "{\"timestamp\":\"2026-07-20T13:26:20Z\",\"method\":\"_x.ai/session/update\",\"params\":{\"update\":{\"sessionUpdate\":\"usage_snapshot\",\"prompt_id\":\"px\",\"usage\":{\"inputTokens\":9999,\"outputTokens\":9,\"cachedReadTokens\":0}}}}\n",
            "{\"timestamp\":\"2026-07-20T13:26:24Z\",\"method\":\"_x.ai/session/update\",\"params\":{\"update\":{\"sessionUpdate\":\"turn_completed\",\"prompt_id\":\"p1\",\"usage\":{\"inputTokens\":16632,\"outputTokens\":104,\"cachedReadTokens\":0,\"modelUsage\":{\"grok-4.5-build\":{\"inputTokens\":16632,\"outputTokens\":104,\"cachedReadTokens\":0,\"apiDurationMs\":5342,\"costUsdTicks\":338880000}}}}}}\n",
        );
        let events = parse_grok_usage_events(content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].prompt_id, "p1");
        assert_eq!(events[0].per_model.len(), 1);
        assert_eq!(events[0].per_model[0].0, "grok-4.5-build");
        assert_eq!(
            events[0].per_model[0].1,
            GrokCounters {
                input: 16632,
                output: 104,
                cached: 0,
                reasoning: None,
                api_ms: 5342,
                model_calls: 0,
                cost_ticks: 338_880_000,
                cost_reported: true,
                cost_partial: false,
                tokens_known: true,
            }
        );
    }

    #[test]
    fn top_level_usage_imports_as_unknown_model_without_fabricating_cost() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let line = format!(
            r#"{{"timestamp":{},"method":"_x.ai/session/update","params":{{"update":{{"sessionUpdate":"turn_completed","prompt_id":"unknown-model","usage":{{"inputTokens":100,"outputTokens":10,"cachedReadTokens":5}}}}}}}}"#,
            OLD_EPOCH
        );
        let path = write_session_file(temp.path(), "sess-unknown-model", &[line]);

        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 1);
        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].model, "unknown");
        assert_eq!(canonical[0].total_cost_usd, None);
        assert_eq!(canonical[0].reasoning_tokens, None);
        assert_eq!(canonical[0].cache_creation_tokens, None);
        Ok(())
    }

    #[test]
    fn two_turns_import_at_face_value_matching_reported_ticks() -> Result<(), AppError> {
        // `turn_completed` holds each prompt's face-value usage, not a
        // cumulative session counter. The reported CLI ticks are authoritative.
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters_with_ticks("grok-4.5-build", 17294, 28, 11136, 1, 158_248_000),
            ),
            usage_event_line(
                OLD_EPOCH + 60,
                "p2",
                &model_counters_with_ticks("grok-4.5-build", 17347, 56, 17280, 1, 56_540_000),
            ),
        ];
        let path = write_session_file(temp.path(), "sess-two-turns", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.deferred_files, 0);

        let expected1 = Decimal::from(158_248_000u64) / Decimal::from(10_000_000_000u64);
        let expected2 = Decimal::from(56_540_000u64) / Decimal::from(10_000_000_000u64);
        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].request_count, Some(2));
        let expected_total = (expected1 + expected2).to_string();
        assert_eq!(
            canonical[0].total_cost_usd.as_deref(),
            Some(expected_total.as_str())
        );
        assert_eq!(canonical[0].cost_status.as_deref(), Some("complete"));
        assert_eq!(canonical[0].cost_source.as_deref(), Some("grok_session"));
        assert_eq!(canonical[0].cache_creation_tokens, None);
        assert_eq!(
            canonical[0].input_token_semantics,
            INPUT_TOKEN_SEMANTICS_TOTAL
        );
        Ok(())
    }

    #[test]
    fn reasoning_tokens_are_preserved_without_inflating_output() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let path = write_session_file(
            temp.path(),
            "sess-reasoning",
            &[usage_event_line(
                OLD_EPOCH,
                "reasoning-turn",
                &model_counters_with_reasoning("grok-4.5-build", 100, 20, 3, 17, 4),
            )],
        );

        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 1);
        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].input_tokens, 100);
        assert_eq!(
            canonical[0].output_tokens, 20,
            "reasoning is an output subset"
        );
        assert_eq!(canonical[0].reasoning_tokens, Some(17));
        assert_eq!(canonical[0].cache_creation_tokens, None);
        assert_eq!(
            canonical[0].input_token_semantics,
            INPUT_TOKEN_SEMANTICS_TOTAL
        );
        Ok(())
    }

    #[test]
    fn settle_window_defers_recent_events_without_recording_sync_state() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("now")
            .as_secs() as i64;
        let lines = vec![
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            ),
            // 未沉降的新事件：本轮延后，且不落同步状态以便下一轮重读
            usage_event_line(now, "p2", &model_counters("grok-4.5-build", 250, 30, 0, 1)),
        ];
        let path = write_session_file(temp.path(), "sess-settle", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 1);
        assert_eq!(result.deferred_files, 1);
        assert_eq!(query_rows(&db)?.len(), 1);
        assert_eq!(query_coverage_markers(&db)?.len(), 1);

        let (last_modified, _) = get_sync_state(&db, &path.to_string_lossy())?;
        assert_eq!(last_modified, 0, "延后时不得记录同步状态");

        // 下一轮重读：旧事件 UPSERT 无变化，新事件仍未沉降继续延后
        let rerun = sync_single_grok_file(&db, &path)?;
        assert_eq!(rerun.imported, 0);
        assert_eq!(rerun.skipped, 1);
        assert_eq!(rerun.deferred_files, 1);
        assert_eq!(query_rows(&db)?.len(), 1);
        assert_eq!(query_coverage_markers(&db)?.len(), 1);
        Ok(())
    }

    #[test]
    fn takeover_guard_skips_events_near_proxy_activity() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "grok-proxy-req",
                    "some-provider",
                    "grokbuild",
                    "grok-4.5",
                    "grok-4.5",
                    999,
                    88,
                    0,
                    0,
                    "0.01",
                    100,
                    200,
                    OLD_EPOCH + 30,
                    "proxy"
                ],
            )?;
        }
        let temp = tempdir().expect("tempdir");
        let lines = vec![
            // 事件时刻落在代理行 ±窗口内 → 接管态，跳过（代理行权威）
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            ),
            // 远离接管窗口的后续事件按面值正常导入
            usage_event_line(
                OLD_EPOCH + SESSION_PROXY_DEDUP_WINDOW_SECONDS + 3600,
                "p2",
                &model_counters("grok-4.5-build", 250, 30, 0, 1),
            ),
        ];
        let path = write_session_file(temp.path(), "sess-guard", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.skipped, 1, "守卫跳过计入 skipped（未入账）");
        assert_eq!(result.imported, 1);

        let rows = query_rows(&db)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, 250, "守卫外事件按本轮面值入账");
        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].request_count, Some(1));
        assert_eq!(canonical[0].input_tokens, 250);
        let markers = query_coverage_markers(&db)?;
        assert_eq!(markers.len(), 1, "proxy-taken-over turn has no marker");
        assert!(markers[0].0.contains(":p2:"));
        Ok(())
    }

    #[test]
    fn rewind_truncation_does_not_double_count() -> Result<(), AppError> {
        // 回归（对比评审发现）：幂等键若含文件内序号，updates.jsonl 前缀被
        // 改写（rewind 截断）后幸存事件序号前移会生成新 request_id 造成双算。
        // prompt_id 锚定键下：幸存轮命中原行；被移除轮的行保留（rewind 不
        // 退还已消耗 token，留存即正确）。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let full = three_turn_fixture();
        let path = write_session_file(temp.path(), "sess-rewind", &full);
        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 3);

        // 模拟 rewind 截掉 p2：p3 从 idx2 前移到 idx1
        let truncated = vec![full[0].clone(), full[2].clone()];
        write_session_file(temp.path(), "sess-rewind", &truncated);
        clear_sync_cursor(&db)?;

        let rescan = sync_single_grok_file(&db, &path)?;
        assert_eq!(rescan.imported, 0, "幸存轮不得因序号前移重新入账");

        let rows = query_rows(&db)?;
        assert_eq!(rows.len(), 3, "被截掉轮次的行保留（token 已实际消耗）");
        let p3: Vec<_> = rows.iter().filter(|r| r.0.contains(":p3:")).collect();
        assert_eq!(p3.len(), 1);
        assert_eq!(p3[0].1, 300);
        assert_eq!(query_canonical_rows(&db)?[0].request_count, Some(3));
        assert_eq!(
            query_coverage_markers(&db)?.len(),
            3,
            "rewind skip adds no marker"
        );
        Ok(())
    }

    #[test]
    fn canonical_rebuild_retains_durable_rows_across_rewind_and_update() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let full = three_turn_fixture();
        let path = write_session_file(temp.path(), "sess-durable-rewind", &full);
        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 3);

        // The source rewinds away p2 while appending p4.  The current scan has
        // three prompts again, but durable raw rows prove four unique IDs.
        let rewritten = vec![
            full[0].clone(),
            full[2].clone(),
            usage_event_line(
                OLD_EPOCH + 180,
                "p4",
                &model_counters("grok-4.5-build", 400, 40, 0, 1),
            ),
        ];
        write_session_file(temp.path(), "sess-durable-rewind", &rewritten);
        clear_sync_cursor(&db)?;

        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 1);
        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].request_count, Some(4));
        assert_eq!(
            (
                canonical[0].input_tokens,
                canonical[0].output_tokens,
                canonical[0].cache_read_tokens
            ),
            (1000, 100, 0),
            "p2 remains counted and new p4 is included"
        );
        assert_eq!(
            canonical[0].reasoning_tokens, None,
            "raw fallback does not claim unsupported reasoning metadata"
        );
        assert_eq!(canonical[0].cost_status.as_deref(), Some("partial"));
        assert_eq!(query_coverage_markers(&db)?.len(), 4);

        // Updating p3 replaces its durable raw contribution; it must not add
        // a fifth request to the rebuilt canonical bucket.
        let updated = vec![
            full[0].clone(),
            usage_event_line(
                OLD_EPOCH + 120,
                "p3",
                &model_counters("grok-4.5-build", 350, 35, 0, 1),
            ),
            rewritten[2].clone(),
        ];
        write_session_file(temp.path(), "sess-durable-rewind", &updated);
        clear_sync_cursor(&db)?;
        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 1);

        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].request_count, Some(4));
        assert_eq!(
            (canonical[0].input_tokens, canonical[0].output_tokens),
            (1050, 105),
            "updating p3 replaces its contribution without increasing count"
        );
        assert_eq!(query_coverage_markers(&db)?.len(), 4);
        Ok(())
    }

    #[test]
    fn partial_reported_cost_prefers_local_pricing_when_priced() -> Result<(), AppError> {
        use std::str::FromStr;
        // costIsPartial=true：自报只是下界，不可作 total。token 数是完整的，
        // 有本地价时用本地全额复算（此处应得 338880000 ticks 等值）。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![usage_event_line_partial(
            OLD_EPOCH,
            "p1",
            &model_counters_with_ticks("grok-4.5-build", 16632, 104, 0, 1, 1_000),
        )];
        let path = write_session_file(temp.path(), "sess-partial", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 1);

        let conn = lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE data_source = 'grok_session'",
            [],
            |row| row.get(0),
        )?;
        let expected = Decimal::from(338_880_000u64) / Decimal::from(10_000_000_000u64);
        assert_eq!(Decimal::from_str(&total).expect("decimal"), expected);
        Ok(())
    }

    #[test]
    fn summary_id_mismatch_fails_closed_without_cross_cwd_merge() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let path = write_session_file(
            temp.path(),
            "folder-id",
            &[usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            )],
        );
        let summary_path = path.parent().expect("session dir").join("summary.json");
        std::fs::write(
            summary_path,
            r#"{"info":{"id":"different-id","cwd":"C:/fixture"}}"#,
        )
        .expect("rewrite mismatched summary");

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 0);
        assert!(query_rows(&db)?.is_empty());
        assert!(query_canonical_rows(&db)?.is_empty());
        assert!(query_coverage_markers(&db)?.is_empty());
        Ok(())
    }

    #[test]
    fn duplicate_prompt_turn_uses_latest_value_once() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let path = write_session_file(
            temp.path(),
            "sess-duplicate-prompt",
            &[
                usage_event_line(
                    OLD_EPOCH,
                    "same-prompt",
                    &model_counters("grok-4.5-build", 100, 10, 0, 1),
                ),
                usage_event_line(
                    OLD_EPOCH + 60,
                    "same-prompt",
                    &model_counters("grok-4.5-build", 250, 25, 5, 4),
                ),
            ],
        );

        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 2);
        assert_eq!(
            query_rows(&db)?.len(),
            1,
            "raw prompt UPSERT keeps latest value"
        );
        let canonical = query_canonical_rows(&db)?;
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].request_count, Some(1));
        assert_eq!(
            (
                canonical[0].input_tokens,
                canonical[0].output_tokens,
                canonical[0].cache_read_tokens
            ),
            (250, 25, 5)
        );
        Ok(())
    }
}
