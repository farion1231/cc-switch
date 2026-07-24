//! Grok Build (Grok CLI) 会话用量追踪
//!
//! 从 `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/updates.jsonl`
//! 的 `turn_completed` 事件中提取用量，写入 proxy_request_logs，实现官方
//! OAuth 直连态（无代理数据）下的用量统计。
//!
//! ## 数据流
//! ```text
//! updates.jsonl（逐轮 turn_completed） → 沉降窗/接管守卫 → 费用计算 → proxy_request_logs
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
//!   修复路径，所以定价漂移窗口不能押在本地价上）；本地定价负责分项成本与
//!   漂移告警。`costIsPartial` 标记自报为下界：有本地价回退本地全额复算并
//!   抑制漂移告警，无价才用下界入账（分项记 0）。
//! - 防接管态双算不用指纹去重：接管态下 CLI 照写 updates.jsonl，但轮事件是
//!   聚合值（多 loop 求和），与代理逐请求行结构性不相等。改用「沉降窗 +
//!   接管活动时间窗守卫」：只导入足够旧的事件（届时接管态的代理行必已
//!   落库），插入前按事件时刻查询附近是否存在代理直录行（见
//!   `has_recent_grokbuild_proxy_activity`）。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::{
    find_model_pricing, has_recent_grokbuild_proxy_activity, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use rust_decimal::Decimal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    api_ms: u64,
    model_calls: u64,
    /// CLI 自报本轮成本，1 tick = 1e-10 USD；0 = 上游未提供
    cost_ticks: u64,
    /// 上游标记 cost_ticks 只是部分费用（`costIsPartial`）：此时它是下界
    cost_partial: bool,
}

impl GrokCounters {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cached == 0
    }

    fn reported_cost_usd(&self) -> Option<Decimal> {
        (self.cost_ticks > 0)
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
        collect_files_named(&root, "updates.jsonl", &mut files);
    }
    files
}

/// 递归收集目录下指定文件名的文件（容忍布局深度变化，对齐会话浏览器的做法）
fn collect_files_named(root: &Path, name: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_named(&path, name, files);
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

    let (last_modified, _last_offset) = get_sync_state(db, &file_path_str)?;
    if file_modified <= last_modified {
        return Ok(SessionSyncResult::default());
    }

    // 文件变更时全量重读：UPSERT 幂等使重读无害，且沉降窗延后的事件本就
    // 依赖下一轮重读补入。事件已是逐轮独立值，改 offset 增量读在正确性上
    // 可行（无差分基线依赖），但需另行处理延后事件的 offset 回退，收益
    // （活跃会话每周期省一次 O(N) 解析）暂不值得该复杂度。
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let events = parse_grok_usage_events(&content);

    // 会话 ID = 会话目录名（与 summary.json 的 info.id 一致）。request_id
    // 唯一性押在该 UUIDv7 全局唯一上：同 ID 的归档/活跃副本经 UPSERT 幂等
    // 收敛（有意），不同 <enc-cwd> 下撞 ID 视为不可能。
    let session_id = file_path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut result = SessionSyncResult::default();
    let mut deferred = false;

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
            if turn.is_zero() {
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
            let request_id = format!("grok_session:{session_id}:{turn_key}:{model}");
            match insert_grok_session_entry(
                db,
                &request_id,
                turn,
                event.cost_is_partial || turn.cost_partial,
                model,
                &session_id,
                event.created_at,
            ) {
                Ok(true) => result.imported += 1,
                Ok(false) => result.skipped += 1,
                Err(e) => {
                    log::warn!("[GROK-SYNC] 插入失败 ({request_id}): {e}");
                    result.skipped += 1;
                }
            }
        }
    }

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
        // 只认 turn_completed（实测全体带 usage 的事件均为此类；判别字段是
        // sessionUpdate，serde internally-tagged）。字段缺失时向后兼容放行，
        // 但显式标为其它类型的事件即使带 usage 也不导入——中途快照若与轮末
        // 事件并存，双导会双算。
        let kind = update
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str());
        if kind.is_some() && kind != Some("turn_completed") {
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
                    .map(|(model, counters)| (model.clone(), parse_grok_counters(counters)))
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
    let get = |key: &str| value.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    GrokCounters {
        input: get("inputTokens"),
        output: get("outputTokens"),
        cached: get("cachedReadTokens"),
        api_ms: get("apiDurationMs"),
        model_calls: get("modelCalls"),
        cost_ticks: get("costUsdTicks"),
        cost_partial: value
            .get("costIsPartial")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
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
            let cost = CostCalculator::calculate_for_app("grokbuild", &usage, &p, multiplier);
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
           OR latency_ms != excluded.latency_ms
           OR model != excluded.model)",
        rusqlite::params![
            request_id,
            "_grok_session",     // provider_id
            "grokbuild",         // app_type
            model,
            model,               // request_model = model
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            0i64,                // cache_creation_tokens
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            turn.api_ms.min(i64::MAX as u64) as i64, // latency_ms（本轮 API 时长）
            Option::<i64>::None, // first_token_ms
            200i64,              // status_code
            Option::<String>::None, // error_message
            session_id,
            Some("grok_session"), // provider_type
            1i64,                // is_streaming
            "1.0",               // cost_multiplier
            created_at,
            "grok_session",      // data_source
            INPUT_TOKEN_SEMANTICS_TOTAL,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Grok Build 会话日志失败: {e}")))?;

    // changes() > 0 表示新插入或已更新，== 0 表示值完全相同（无实际变更）
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

    fn epoch_to_rfc3339(epoch: i64) -> String {
        chrono::DateTime::from_timestamp(epoch, 0)
            .expect("valid epoch")
            .to_rfc3339()
    }

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

    fn model_counters_with_ticks(
        model: &str,
        input: u64,
        output: u64,
        cached: u64,
        calls: u64,
        ticks: u64,
    ) -> String {
        format!(
            r#""{model}":{{"inputTokens":{input},"outputTokens":{output},"cachedReadTokens":{cached},"reasoningTokens":0,"modelCalls":{calls},"apiDurationMs":1000,"costUsdTicks":{ticks}}}"#
        )
    }

    fn write_session_file(dir: &Path, session_id: &str, lines: &[String]) -> PathBuf {
        let session_dir = dir.join("sessions").join("enc-project").join(session_id);
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("updates.jsonl");
        let mut file = std::fs::File::create(&path).expect("create updates.jsonl");
        for line in lines {
            writeln!(file, "{line}").expect("write line");
        }
        path
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

    fn query_costs(db: &Database) -> Result<Vec<(String, String)>, AppError> {
        let conn = lock_conn!(db.conn);
        let mut stmt = conn
            .prepare(
                "SELECT request_id, total_cost_usd FROM proxy_request_logs
                 WHERE data_source = 'grok_session' ORDER BY created_at, request_id",
            )
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    #[test]
    fn parses_turn_completed_and_ignores_noise_and_other_kinds() {
        let content = concat!(
            "{\"timestamp\":\"2026-07-20T13:26:10Z\",\"method\":\"session/update\",\"params\":{\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{}}}}\n",
            "not json at all\n",
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
                api_ms: 5342,
                model_calls: 0,
                cost_ticks: 338_880_000,
                cost_partial: false,
            }
        );
    }

    #[test]
    fn missing_model_usage_falls_back_to_top_level_counters() {
        // 同时覆盖：sessionUpdate 字段缺失时向后兼容放行
        let line = format!(
            r#"{{"timestamp":"{}","method":"_x.ai/session/update","params":{{"update":{{"prompt_id":"p1","usage":{{"inputTokens":100,"outputTokens":10,"cachedReadTokens":5}}}}}}}}"#,
            epoch_to_rfc3339(OLD_EPOCH)
        );
        let events = parse_grok_usage_events(&line);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].per_model[0].0, "unknown");
        assert_eq!(events[0].per_model[0].1.input, 100);
    }

    #[test]
    fn two_turns_import_at_face_value_matching_reported_ticks() -> Result<(), AppError> {
        use std::str::FromStr;
        // 2026-07-23 单进程双 prompt 实测原值：turn_completed 是逐轮独立总量。
        // 若误用相邻差分，第二轮会被记成 53/28/6144 的假增量（曾犯）。
        // 每轮 ticks 同时钉死逐轮口径与 2/6/0.30 定价：
        //   轮1 (17294-11136)×2 + 11136×0.30 + 28×6 = 15824.8 µUSD = 158248000 ticks
        //   轮2 (17347-17280)×2 + 17280×0.30 + 56×6 =  5654.0 µUSD =  56540000 ticks
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

        let rows = query_rows(&db)?;
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].1, rows[0].2, rows[0].3), (17294, 28, 11136));
        assert_eq!((rows[1].1, rows[1].2, rows[1].3), (17347, 56, 17280));
        // 语义列显式为 TOTAL，与代理路径一致
        assert!(rows.iter().all(|r| r.4 == INPUT_TOKEN_SEMANTICS_TOTAL));

        // 本地定价复算须与 CLI 自报 ticks 分毫不差（漂移告警在此阈值内静默）
        let costs = query_costs(&db)?;
        let expected1 = Decimal::from(158_248_000u64) / Decimal::from(10_000_000_000u64);
        let expected2 = Decimal::from(56_540_000u64) / Decimal::from(10_000_000_000u64);
        assert_eq!(Decimal::from_str(&costs[0].1).expect("decimal"), expected1);
        assert_eq!(Decimal::from_str(&costs[1].1).expect("decimal"), expected2);
        Ok(())
    }

    #[test]
    fn second_turn_with_smaller_counters_imports_at_face_value() -> Result<(), AppError> {
        // 2026-07-23 跨进程实测原值（进程 A 单轮 27386/74/15360，--resume 的
        // 进程 B 单轮 13793/21/13696）。逐轮口径下"第二轮更小"是常态，
        // 与是否跨进程无关，一律按面值入账。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 27386, 74, 15360, 2),
            ),
            usage_event_line(
                OLD_EPOCH + 15,
                "p2",
                &model_counters("grok-4.5-build", 13793, 21, 13696, 1),
            ),
        ];
        let path = write_session_file(temp.path(), "sess-resume", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 2);

        let rows = query_rows(&db)?;
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].1, rows[0].2, rows[0].3), (27386, 74, 15360));
        assert_eq!((rows[1].1, rows[1].2, rows[1].3), (13793, 21, 13696));
        Ok(())
    }

    #[test]
    fn identical_turns_both_import() -> Result<(), AppError> {
        // 回归（逐轮口径）：两轮数值完全相同 = 两笔真实用量，都必须入账。
        // 差分口径会把第二轮当零增量整轮跳过——那正是被证伪的旧行为。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            ),
            usage_event_line(
                OLD_EPOCH + 60,
                "p2",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            ),
        ];
        let path = write_session_file(temp.path(), "sess-identical", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 2, "相同数值的两轮都是真实用量");
        assert_eq!(query_rows(&db)?.len(), 2);
        Ok(())
    }

    #[test]
    fn multi_model_event_produces_row_per_model() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let both = format!(
            "{},{}",
            model_counters("grok-4.5-build", 100, 10, 0, 1),
            model_counters("grok-4.3", 30, 3, 0, 1)
        );
        let lines = vec![usage_event_line(OLD_EPOCH, "p1", &both)];
        let path = write_session_file(temp.path(), "sess-multi", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 2);
        let rows = query_rows(&db)?;
        assert!(rows[0].0.ends_with(":grok-4.3"));
        assert!(rows[1].0.ends_with(":grok-4.5-build"));
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

        let (last_modified, _) = get_sync_state(&db, &path.to_string_lossy())?;
        assert_eq!(last_modified, 0, "延后时不得记录同步状态");

        // 下一轮重读：旧事件 UPSERT 无变化，新事件仍未沉降继续延后
        let rerun = sync_single_grok_file(&db, &path)?;
        assert_eq!(rerun.imported, 0);
        assert_eq!(rerun.skipped, 1);
        assert_eq!(rerun.deferred_files, 1);
        assert_eq!(query_rows(&db)?.len(), 1);
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
        Ok(())
    }

    #[test]
    fn rescan_is_idempotent() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![
            usage_event_line(
                OLD_EPOCH,
                "p1",
                &model_counters("grok-4.5-build", 100, 10, 0, 1),
            ),
            usage_event_line(
                OLD_EPOCH + 60,
                "p2",
                &model_counters("grok-4.5-build", 250, 30, 50, 1),
            ),
        ];
        let path = write_session_file(temp.path(), "sess-idem", &lines);

        let first = sync_single_grok_file(&db, &path)?;
        assert_eq!(first.imported, 2);

        // mtime 未变 → 短路
        let second = sync_single_grok_file(&db, &path)?;
        assert_eq!(second.imported + second.skipped, 0);

        // 强制重读（清同步状态）→ UPSERT 全部无变化
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM session_log_sync", [])?;
        }
        let third = sync_single_grok_file(&db, &path)?;
        assert_eq!(third.imported, 0);
        assert_eq!(third.skipped, 2);
        assert_eq!(query_rows(&db)?.len(), 2);
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
        let full = vec![
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
        ];
        let path = write_session_file(temp.path(), "sess-rewind", &full);
        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 3);

        // 模拟 rewind 截掉 p2：p3 从 idx2 前移到 idx1
        let truncated = vec![full[0].clone(), full[2].clone()];
        write_session_file(temp.path(), "sess-rewind", &truncated);
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM session_log_sync", [])?;
        }

        let rescan = sync_single_grok_file(&db, &path)?;
        assert_eq!(rescan.imported, 0, "幸存轮不得因序号前移重新入账");

        let rows = query_rows(&db)?;
        assert_eq!(rows.len(), 3, "被截掉轮次的行保留（token 已实际消耗）");
        let p3: Vec<_> = rows.iter().filter(|r| r.0.contains(":p3:")).collect();
        assert_eq!(p3.len(), 1);
        assert_eq!(p3[0].1, 300);
        Ok(())
    }

    #[test]
    fn empty_prompt_id_falls_back_to_index_key() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![usage_event_line(
            OLD_EPOCH,
            "",
            &model_counters("grok-4.5-build", 100, 10, 0, 1),
        )];
        let path = write_session_file(temp.path(), "sess-noprompt", &lines);

        assert_eq!(sync_single_grok_file(&db, &path)?.imported, 1);
        let rows = query_rows(&db)?;
        assert!(rows[0].0.contains(":idx0:"), "空 prompt_id 回退序号键");
        Ok(())
    }

    #[test]
    fn cost_matches_cli_reported_ticks_for_seeded_grok45_build() -> Result<(), AppError> {
        use std::str::FromStr;
        // 真实样本：inputTokens=16632, outputTokens=104, cache=0,
        // costUsdTicks=338880000（1 tick = 1e-10 USD）。seed 的 grok-4.5-build
        // 定价（2/6）应精确复现 CLI 自报成本。fixture 故意不带 ticks，
        // 验证的是本地定价独立复算。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![usage_event_line(
            OLD_EPOCH,
            "p1",
            &model_counters("grok-4.5-build", 16632, 104, 0, 1),
        )];
        let path = write_session_file(temp.path(), "sess-ticks", &lines);

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
    fn cost_matches_cli_reported_ticks_with_cache_reads() -> Result<(), AppError> {
        use std::str::FromStr;
        // 2026-07-23 实测带缓存样本：13793/21/13696，costUsdTicks=44288000。
        // 钉死 cache read 实测单价 0.30：billable_input=(13793-13696)×2/1M
        // + 21×6/1M + 13696×0.30/1M = 0.0044288。seed 若改回 0.50 此测试即红。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![usage_event_line(
            OLD_EPOCH,
            "p1",
            &model_counters("grok-4.5-build", 13793, 21, 13696, 1),
        )];
        let path = write_session_file(temp.path(), "sess-ticks-cache", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 1);

        let conn = lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE data_source = 'grok_session'",
            [],
            |row| row.get(0),
        )?;
        let expected = Decimal::from(44_288_000u64) / Decimal::from(10_000_000_000u64);
        assert_eq!(Decimal::from_str(&total).expect("decimal"), expected);
        Ok(())
    }

    #[test]
    fn reported_ticks_override_stale_local_pricing() -> Result<(), AppError> {
        use std::str::FromStr;
        // 定价漂移窗口：CLI 自报为本地复算（338880000 ticks）的两倍，模拟
        // xAI 调价而 seed 未更新。total 必须以自报为准（回填不修正正值行，
        // 本地价错就永久错）；分项仍按本地价（暂不自洽，有漂移告警提示）。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![usage_event_line(
            OLD_EPOCH,
            "p1",
            &model_counters_with_ticks("grok-4.5-build", 16632, 104, 0, 1, 677_760_000),
        )];
        let path = write_session_file(temp.path(), "sess-drift", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, total): (String, String) = conn.query_row(
            "SELECT input_cost_usd, total_cost_usd FROM proxy_request_logs
             WHERE data_source = 'grok_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let expected_total = Decimal::from(677_760_000u64) / Decimal::from(10_000_000_000u64);
        assert_eq!(
            Decimal::from_str(&total).expect("decimal"),
            expected_total,
            "total 以自报为准"
        );
        assert!(
            Decimal::from_str(&input_cost).expect("decimal") > Decimal::ZERO,
            "分项仍按本地定价"
        );
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
    fn unpriced_model_falls_back_to_reported_ticks() -> Result<(), AppError> {
        use std::str::FromStr;
        // 未 seed 的新别名：total_cost 采用 CLI 自报 ticks（分项记 0），
        // 不再整单记 0。
        let db = Database::memory()?;
        let temp = tempdir().expect("tempdir");
        let lines = vec![usage_event_line(
            OLD_EPOCH,
            "p1",
            &model_counters_with_ticks("grok-6-future-alias", 1000, 100, 0, 1, 56_540_000),
        )];
        let path = write_session_file(temp.path(), "sess-unpriced", &lines);

        let result = sync_single_grok_file(&db, &path)?;
        assert_eq!(result.imported, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, total): (String, String) = conn.query_row(
            "SELECT input_cost_usd, total_cost_usd FROM proxy_request_logs
             WHERE data_source = 'grok_session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            Decimal::from_str(&input_cost).expect("decimal"),
            Decimal::ZERO
        );
        let expected = Decimal::from(56_540_000u64) / Decimal::from(10_000_000_000u64);
        assert_eq!(Decimal::from_str(&total).expect("decimal"), expected);
        Ok(())
    }
}
