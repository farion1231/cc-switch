//! 从 WorkBuddy 的会话日志导入用量：`<data dir>/projects/<cwd>/<sessionId>.jsonl`。
//!
//! 每次模型请求的 `providerData.rawUsage` 记在这次请求的最后一个事件上（assistant
//! `message` 或 `function_call`），按事件 `id` 去重；子代理
//! `<sessionId>/subagents/*.jsonl` 的请求算父会话的。`rawUsage` 是上游返回的 OpenAI 口径用量：`prompt_tokens` 已含缓存命中（`prompt_cache_hit_tokens +
//! prompt_cache_miss_tokens == prompt_tokens`），`completion_tokens` 已含思考 token。
//! 入库时把缓存读写从输入里扣掉，按「新输入」口径存（`input_token_semantics = FRESH`），
//! 计费时输入、缓存读、缓存写、输出各按各的单价算。`rawUsage.credit` 是 WorkBuddy 积分，
//! 不是美元，不当作费用。
//!
//! 日志只追加：按字节偏移增量读，只处理以换行结尾的完整行；文件变短说明被重写，从头再读，
//! `session_usage_dedup` 里的永久回执保证同一请求不会重复入库。
//!
//! 耗时与首字：日志只在响应结束时打一个时间戳，没有首字。耗时取「同一 `messageId` 分组
//! 之前最后一条记录」到本条记录的间隔（钳在 100ms–1h）；首字按同一模型的历史请求做
//! 最小二乘（截距 + 未命中前缀 + 缓存命中 + 输出 → 总耗时），扣掉解码项后回填。
//! 耗时样本优先取 `traces/<pid>/trace_*.jsonl` 里 `generation` span 的精确 duration
//! （trace 与日志没有共同 id，只能按时间窗匹配），否则退回上面的锚点差值。
//! 系数全为正且 R² 达标才写；样本不足的模型保持 NULL，界面显示「—」。
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::{
    session_usage::{
        cost_columns, estimated_latency_ms, load_sync_cursors, metadata_modified_nanos,
        update_sync_state_on_conn, SessionSyncResult,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use crate::session_manager::providers::workbuddy;
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
};

const DATA_SOURCE: &str = "workbuddy_session";
/// 游标键前缀。改了「哪些记录算请求」或派生字段的口径时递增，让已同步过的文件从头重读；
/// 已入库的请求由 `session_usage_dedup` 回执挡住，只补新认出的或缺失的字段。
const CURSOR_PREFIX: &str = "workbuddy:v3:";

/// 耗时回看窗口：游标停在文件中同时，回看这么多字节把时间线接上。
const TIMING_PREFETCH_BYTES: i64 = 64 * 1024;

pub(crate) fn sync_workbuddy_usage(
    db: &Database,
    cursors: &crate::services::session_usage::SyncCursors,
) -> Result<SessionSyncResult, AppError> {
    sync_all_with_cursors(
        db,
        &workbuddy::projects_dir(),
        &crate::workbuddy_config::data_dir().join("traces"),
        cursors,
    )
}

/// `tests` 用临时目录走这条路径，避免碰真实 `~/.workbuddy`。
#[cfg(test)]
fn sync_all(db: &Database, projects: &Path, traces: &Path) -> Result<SessionSyncResult, AppError> {
    let cursors = load_sync_cursors(db)?;
    sync_all_with_cursors(db, projects, traces, &cursors)
}

fn sync_all_with_cursors(
    db: &Database,
    projects: &Path,
    traces: &Path,
    cursors: &crate::services::session_usage::SyncCursors,
) -> Result<SessionSyncResult, AppError> {
    let result = sync_projects_with_cursors(db, projects, cursors)?;
    // 首字粗估的样本来自刚导入的耗时，同步后重拟合一次；失败只记日志，不影响已导入的用量。
    if let Err(error) = refine_first_token_estimates(db, traces) {
        log::warn!("WorkBuddy 首字估算更新失败: {error}");
    }
    Ok(result)
}

#[cfg(test)]
fn sync_projects(db: &Database, projects: &Path) -> Result<SessionSyncResult, AppError> {
    sync_projects_with_cursors(
        db,
        projects,
        &crate::services::session_usage::load_sync_cursors(db)?,
    )
}

fn sync_projects_with_cursors(
    db: &Database,
    projects: &Path,
    cursors: &crate::services::session_usage::SyncCursors,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let files = workbuddy::usage_files(projects);
    if files.is_empty() {
        return Ok(result);
    }
    for (path, parent_session) in files {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let modified = metadata_modified_nanos(&meta);
        let key = format!("{CURSOR_PREFIX}{}", path.to_string_lossy());
        let cursor = cursors.get(&key);
        if cursor
            .is_some_and(|c| c.last_modified == modified && c.last_line_offset == meta.len() as i64)
        {
            continue;
        }
        let offset = cursor
            .map(|c| c.last_line_offset)
            .filter(|offset| *offset >= 0 && *offset <= meta.len() as i64)
            .unwrap_or(0);
        match sync_file(db, &path, parent_session.as_deref(), &key, modified, offset) {
            Ok(part) => result.merge(part),
            Err(error) => result
                .errors
                .push(format!("WorkBuddy session {}: {error}", path.display())),
        }
    }
    Ok(result)
}

struct Entry {
    request_id: String,
    session_id: String,
    model: String,
    usage: TokenUsage,
    created_ms: i64,
    latency_ms: i64,
}

/// `parent_session`：子代理文件所属的父会话；记录里的 `sessionId` 是子代理自己的。
/// `anchor_ms`：同一 `messageId` 分组之前最后一条记录的时间戳，用来估耗时。
fn entry(record: &Value, parent_session: Option<&str>, anchor_ms: Option<i64>) -> Option<Entry> {
    let usage = workbuddy::assistant_usage(record)?;
    if usage.is_empty() {
        return None;
    }
    // 一次请求只有一个事件带 rawUsage，事件 id 就是请求 id（和 v1 入库的行一致，重读不会重复）。
    let id = record.get("id").and_then(Value::as_str)?;
    let session_id = parent_session
        .or_else(|| record.get("sessionId").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let model = usage.model.clone().unwrap_or_else(|| "unknown".into());
    let created_ms = record.get("timestamp").and_then(Value::as_i64)?;
    let latency_ms = anchor_ms
        .and_then(|anchor| estimated_latency_ms(anchor, created_ms))
        .unwrap_or(0);
    let clamp = |n: u64| n.min(u32::MAX as u64) as u32;
    Some(Entry {
        request_id: format!("workbuddy:{session_id}:{id}"),
        session_id,
        usage: TokenUsage {
            input_tokens: clamp(usage.fresh_input),
            output_tokens: clamp(usage.output),
            cache_read_tokens: clamp(usage.cache_read),
            cache_creation_tokens: clamp(usage.cache_write),
            cache_creation_1h_tokens: 0,
            model: Some(model.clone()),
            message_id: None,
        },
        model,
        created_ms,
        latency_ms,
    })
}

/// 耗时估算：以「上一条属于别的模型调用」的记录为起点。
///
/// WorkBuddy 只在响应结束时写一条带 `timestamp` 的记录，同一次调用会产生
/// `reasoning` / `message` / `function_call` 等多条同 `messageId` 的记录，它们的
/// 时间戳几乎相同。因此起点取该 `messageId` 分组之前最后一条记录——通常是上一次
/// 工具调用的结果或用户消息，差值即这一次模型往返（含首字等待）。
#[derive(Default)]
struct TimingTracker {
    last_ts: Option<i64>,
    last_message_id: Option<String>,
    group_anchor: Option<i64>,
}

impl TimingTracker {
    /// 喂一条记录，返回它自己的起点（若已有时序上下文）。
    fn observe(&mut self, record: &Value) -> Option<i64> {
        let Some(ts) = record.get("timestamp").and_then(Value::as_i64) else {
            return self.group_anchor;
        };
        let message_id = record
            .pointer("/providerData/messageId")
            .and_then(Value::as_str);
        let same_group = matches!(
            (self.last_message_id.as_deref(), message_id),
            (Some(previous), Some(current)) if previous == current
        );
        if !same_group {
            if let Some(previous) = self.last_ts {
                self.group_anchor = Some(previous);
            }
        }
        self.last_ts = Some(ts);
        self.last_message_id = message_id.map(str::to_owned);
        self.group_anchor
    }
}

fn sync_file(
    db: &Database,
    path: &Path,
    parent_session: Option<&str>,
    key: &str,
    modified: i64,
    offset: i64,
) -> Result<SessionSyncResult, AppError> {
    let mut file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let mut timing = TimingTracker::default();
    // 游标可能停在文件中间：回看一小段把时间线接上，只喂 tracker、不导入。
    if offset > 0 {
        let back = (offset - TIMING_PREFETCH_BYTES).max(0);
        file.seek(SeekFrom::Start(back as u64))
            .map_err(|error| AppError::io(path, error))?;
        {
            let mut pre = BufReader::new(&mut file);
            let mut line = Vec::new();
            let mut pos = back;
            // 从文件中间开始，第一行可能是半行，跳过。
            let mut skip_partial = back > 0;
            while pos < offset {
                line.clear();
                let size = pre
                    .read_until(b'\n', &mut line)
                    .map_err(|error| AppError::io(path, error))?;
                if size == 0 {
                    break;
                }
                pos += size as i64;
                if skip_partial {
                    skip_partial = false;
                    continue;
                }
                if line.last() != Some(&b'\n') {
                    break;
                }
                if let Ok(record) = serde_json::from_slice::<Value>(&line) {
                    timing.observe(&record);
                }
            }
        }
    }
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|error| AppError::io(path, error))?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut bytes = offset;
    let mut entries = Vec::new();
    loop {
        buffer.clear();
        let size = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|error| AppError::io(path, error))?;
        // 写入方可能只刷了最后一行的一部分，下一轮再读。
        if size == 0 || buffer.last() != Some(&b'\n') {
            break;
        }
        bytes += size as i64;
        let Ok(record) = serde_json::from_slice::<Value>(&buffer) else {
            continue;
        };
        let anchor = timing.observe(&record);
        if let Some(entry) = entry(&record, parent_session, anchor) {
            entries.push(entry);
        }
    }

    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for entry in entries {
        if tx.execute(
            "INSERT OR IGNORE INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES (?1, ?2, ?2, 1)",
            [DATA_SOURCE, &entry.request_id],
        )? == 0
        {
            result.skipped += 1;
            // 旧版本写过的行没有耗时：重读时补上，`latency_ms = 0` 让刷新天然幂等。
            refresh_legacy_latency(&tx, &entry.request_id, entry.latency_ms)?;
            continue;
        }
        let usage = &entry.usage;
        let amounts = cost_columns(
            "workbuddy",
            usage,
            find_model_pricing(&tx, &entry.model).as_ref(),
            Decimal::ONE,
        );
        tx.execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, status_code, session_id, provider_type, is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
            ) VALUES (?1, '_workbuddy_session', 'workbuddy', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?16, 200, ?12, ?13, 1, '1', ?14, ?13, ?15)",
            rusqlite::params![
                entry.request_id,
                entry.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
                amounts[0],
                amounts[1],
                amounts[2],
                amounts[3],
                amounts[4],
                entry.session_id,
                DATA_SOURCE,
                entry.created_ms / 1000,
                INPUT_TOKEN_SEMANTICS_FRESH,
                entry.latency_ms,
            ],
        )?;
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

/// 首字粗估的最小样本量：四参数回归，样本太少系数不稳。
const TTFT_MIN_SAMPLES: usize = 200;
/// 拟合优度门槛：解释不了总耗时的回归不写入。
const TTFT_MIN_R2: f64 = 0.5;
/// 行时间戳只有秒精度（created_at），匹配 trace span 时以秒中点为中心放宽容差。
const SPAN_MATCH_WINDOW_MS: i64 = 1500;

/// 一个耗时样本：`(新增输入, 缓存命中, 输出, 总耗时毫秒)`。
type LatencySample = (i64, i64, i64, i64);

/// 旧版本写过的行没有耗时；重读时补上。明细已被 30 天剪枝删除的行（只有收据）不重建。
fn refresh_legacy_latency(
    conn: &rusqlite::Connection,
    request_id: &str,
    latency_ms: i64,
) -> Result<(), AppError> {
    if latency_ms <= 0 {
        return Ok(());
    }
    conn.execute(
        "UPDATE proxy_request_logs SET latency_ms = ?2
         WHERE request_id = ?1 AND app_type = 'workbuddy' AND data_source = ?3 AND latency_ms = 0",
        rusqlite::params![request_id, latency_ms, DATA_SOURCE],
    )
    .map(|_| ())
    .map_err(|e| AppError::Database(format!("刷新 WorkBuddy 旧行耗时失败: {e}")))
}

/// 首字粗估：对同一模型的历史请求做最小二乘，把总耗时拆成
/// 「网络往返 + 前缀处理（未命中/缓存命中分开计） + 逐 token 解码」，
/// 扣掉解码项后剩下的就是首字的粗略估计。
///
/// 耗时样本优先取 trace 里的 `generation` span（精确起止），避免锚点耗时里
/// 混入「工具结果落盘 + 组装上下文」的应用开销（实测约 0.7–1.2s，会让首字
/// 系统性偏高）；trace 缺失或唯一匹配不足时退回锚点差值。
/// 系数全部为正且 R² 达标才写入，否则 `first_token_ms` 保持 NULL。
fn refine_first_token_estimates(db: &Database, traces_dir: &Path) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    let rows: Vec<(String, i64, i64, i64, i64, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT model, created_at, input_tokens, cache_read_tokens, output_tokens, latency_ms
                 FROM proxy_request_logs
                 WHERE app_type = 'workbuddy' AND data_source = ?1 AND latency_ms > 0",
            )
            .map_err(|e| AppError::Database(format!("查询 WorkBuddy 耗时样本失败: {e}")))?;
        let mapped = stmt
            .query_map(rusqlite::params![DATA_SOURCE], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| AppError::Database(format!("查询 WorkBuddy 耗时样本失败: {e}")))?;
        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("读取 WorkBuddy 耗时样本失败: {e}")))?
    };
    let spans = collect_generation_spans(traces_dir);

    // 按模型分组收集两种样本：trace 精确耗时（优先）与锚点差值（回退）。
    let mut span_samples: HashMap<String, Vec<LatencySample>> = HashMap::new();
    let mut anchor_samples: HashMap<String, Vec<LatencySample>> = HashMap::new();
    for (model, created_at, miss, hit, out, latency) in &rows {
        anchor_samples
            .entry(model.clone())
            .or_default()
            .push((*miss, *hit, *out, *latency));
        if spans.is_empty() {
            continue;
        }
        let mid_ms = created_at * 1000 + 500;
        let lo = spans.partition_point(|(ended, _)| *ended < mid_ms - SPAN_MATCH_WINDOW_MS);
        let hi = spans.partition_point(|(ended, _)| *ended <= mid_ms + SPAN_MATCH_WINDOW_MS);
        if hi - lo == 1 && spans[lo].1 > 0 {
            span_samples
                .entry(model.clone())
                .or_default()
                .push((*miss, *hit, *out, spans[lo].1));
        }
    }

    let models: BTreeSet<&String> = anchor_samples.keys().chain(span_samples.keys()).collect();
    for model in models {
        // trace 样本足够时优先，否则退回锚点样本
        let samples = match span_samples.get(model) {
            Some(samples) if samples.len() >= TTFT_MIN_SAMPLES => samples,
            _ => match anchor_samples.get(model) {
                Some(samples) if samples.len() >= TTFT_MIN_SAMPLES => samples,
                _ => continue,
            },
        };
        let Some([overhead, miss_ms, hit_ms, _decode_ms]) = fit_ols(samples) else {
            continue;
        };
        conn.execute(
            "UPDATE proxy_request_logs
             SET first_token_ms = MAX(0, MIN(latency_ms,
                 CAST(ROUND(?2 + ?3 * input_tokens + ?4 * cache_read_tokens) AS INTEGER)))
             WHERE app_type = 'workbuddy' AND data_source = ?5 AND model = ?1 AND latency_ms > 0",
            rusqlite::params![model, overhead, miss_ms, hit_ms, DATA_SOURCE],
        )
        .map(|_| ())
        .map_err(|e| AppError::Database(format!("写入 WorkBuddy 首字估算失败: {e}")))?;
    }
    Ok(())
}

/// 收集 trace 目录里 `generation` span 的 `(结束毫秒, duration 毫秒)`，
/// 只统计成功结束的 span；文件按行解析，坏行跳过。
fn collect_generation_spans(traces_dir: &Path) -> Vec<(i64, i64)> {
    let mut spans = Vec::new();
    let Ok(workers) = fs::read_dir(traces_dir) else {
        return spans;
    };
    for worker in workers.flatten() {
        let Ok(files) = fs::read_dir(worker.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(file) = fs::File::open(&path) else {
                continue;
            };
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else { continue };
                let Ok(record) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if record.get("type").and_then(Value::as_str) != Some("span_end") {
                    continue;
                }
                let Some(span) = record.get("span") else {
                    continue;
                };
                if span.get("name").and_then(Value::as_str) != Some("generation")
                    || span.get("status").and_then(Value::as_str) != Some("ok")
                {
                    continue;
                }
                let Some(ended) = span.get("endedAt").and_then(Value::as_str) else {
                    continue;
                };
                let Some(ended_ms) = crate::services::session_usage::parse_timestamp_millis(ended)
                else {
                    continue;
                };
                let Some(duration) = span.get("duration").and_then(Value::as_i64) else {
                    continue;
                };
                if duration > 0 {
                    spans.push((ended_ms, duration));
                }
            }
        }
    }
    spans.sort_unstable();
    spans
}

/// 四参数 OLS：`y = overhead + miss×k1 + hit×k2 + out×k3`。
/// 系数全部为正且 R² 达标才返回，否则 `None`（调用方保持现状）。
fn fit_ols(samples: &[LatencySample]) -> Option<[f64; 4]> {
    if samples.len() < TTFT_MIN_SAMPLES {
        return None;
    }
    let mut xtx = [[0.0f64; 4]; 4];
    let mut xty = [0.0f64; 4];
    let mut s_yy = 0.0f64;
    for &(miss, hit, out, latency) in samples {
        let xs = [1.0, miss as f64, hit as f64, out as f64];
        let y = latency as f64;
        s_yy += y * y;
        for i in 0..4 {
            xty[i] += y * xs[i];
            for j in 0..4 {
                xtx[i][j] += xs[i] * xs[j];
            }
        }
    }
    let coef = solve_linear_system(xtx, xty)?;
    // 物理约束：往返、两种前缀、解码的单 token 耗时都必须为正。
    if coef.iter().any(|value| !value.is_finite() || *value <= 0.0) {
        return None;
    }
    // R² = 1 − SS_res / SS_tot；SS_res 用 Σy² − 2·coefᵀXᵀy + coefᵀXᵀX·coef。
    let dot_xty: f64 = coef.iter().zip(xty).map(|(c, v)| c * v).sum();
    let quad: f64 = (0..4)
        .map(|i| coef[i] * (0..4).map(|j| coef[j] * xtx[i][j]).sum::<f64>())
        .sum();
    let mean = xty[0] / samples.len() as f64;
    let ss_res = s_yy - 2.0 * dot_xty + quad;
    let ss_tot = s_yy - samples.len() as f64 * mean * mean;
    if ss_tot <= 0.0 {
        return None;
    }
    let r2 = 1.0 - ss_res / ss_tot;
    // NaN 也视为不达标
    if r2.is_nan() || r2 < TTFT_MIN_R2 {
        return None;
    }
    Some(coef)
}

/// 高斯消元（列主元）；奇异矩阵返回 `None`。
fn solve_linear_system(mut matrix: [[f64; 4]; 4], mut rhs: [f64; 4]) -> Option<[f64; 4]> {
    let scale = matrix
        .iter()
        .flatten()
        .fold(0.0f64, |max, value| max.max(value.abs()));
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    for col in 0..4 {
        let mut pivot = col;
        for row in (col + 1)..4 {
            if matrix[row][col].abs() > matrix[pivot][col].abs() {
                pivot = row;
            }
        }
        if matrix[pivot][col].abs() <= scale * 1e-14 {
            return None;
        }
        matrix.swap(col, pivot);
        rhs.swap(col, pivot);
        let pivot_row = matrix[col];
        for row in (col + 1)..4 {
            let factor = matrix[row][col] / pivot_row[col];
            for (offset, value) in matrix[row][col..4].iter_mut().enumerate() {
                *value -= factor * pivot_row[col + offset];
            }
            rhs[row] -= factor * rhs[col];
        }
    }
    let mut result = [0.0; 4];
    for row in (0..4).rev() {
        let tail: f64 = (row + 1..4).map(|j| matrix[row][j] * result[j]).sum();
        result[row] = (rhs[row] - tail) / matrix[row][row];
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn assistant(
        id: &str,
        model: &str,
        prompt: u64,
        hit: u64,
        write: u64,
        completion: u64,
    ) -> String {
        serde_json::json!({
            "id": id, "parentId": "p", "timestamp": 1792764379226_i64, "type": "message",
            "role": "assistant", "status": "completed", "sessionId": "s1", "cwd": "/w",
            "content": [{"type": "output_text", "text": "ok"}],
            "providerData": {"model": model, "requestModelId": "alias", "rawUsage": {
                "prompt_tokens": prompt, "completion_tokens": completion,
                "total_tokens": prompt + completion,
                "prompt_cache_hit_tokens": hit, "prompt_cache_miss_tokens": prompt - hit,
                "cache_read_input_tokens": 0, "cache_creation_input_tokens": write,
                "prompt_cache_write_tokens": 0, "completion_thinking_tokens": 3,
                "credit": 0.5, "cached_tokens": 0
            }}
        })
        .to_string()
    }

    const USER: &str = r#"{"id":"u1","timestamp":1792764357348,"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}],"sessionId":"s1","cwd":"/w"}"#;
    const REASONING: &str = r#"{"id":"r1","timestamp":1792764379203,"type":"reasoning","rawContent":[{"type":"reasoning_text","text":"..."}],"providerData":{"model":"m"},"sessionId":"s1"}"#;

    fn rows(db: &Database) -> Vec<(String, String, i64, i64, i64, i64, i64)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT request_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, input_token_semantics FROM proxy_request_logs WHERE app_type = 'workbuddy' ORDER BY request_id")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn session_file(root: &Path) -> PathBuf {
        let project = root.join("Users-me-proj");
        fs::create_dir_all(&project).unwrap();
        project.join("s1.jsonl")
    }

    #[test]
    fn workbuddy_usage_imports_assistant_messages_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        fs::write(
            &path,
            format!(
                "{USER}\n{REASONING}\n{}\n",
                assistant("a1", "glm-test", 1200, 1000, 0, 34)
            ),
        )
        .unwrap();
        let db = Database::memory().unwrap();
        let first = sync_projects(&db, dir.path()).unwrap();
        assert_eq!(first.imported, 1);
        assert_eq!(
            rows(&db),
            vec![(
                "workbuddy:s1:a1".into(),
                "glm-test".into(),
                200,
                34,
                1000,
                0,
                INPUT_TOKEN_SEMANTICS_FRESH
            )]
        );

        // 文件没变就跳过；追加的请求只导一次；没写完的最后一行等下一轮。
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", assistant("a2", "glm-test", 500, 0, 100, 10)).unwrap();
        write!(file, "{{\"id\":\"a3\",").unwrap();
        drop(file);
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 1);
        let imported = rows(&db);
        assert_eq!(imported.len(), 2);
        assert_eq!((imported[1].2, imported[1].4, imported[1].5), (400, 0, 100));
    }

    #[test]
    fn workbuddy_usage_receipts_survive_file_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        let line = assistant("a1", "m", 300, 0, 0, 5);
        fs::write(&path, format!("{line}\n{line}\n")).unwrap();
        let db = Database::memory().unwrap();
        let first = sync_projects(&db, dir.path()).unwrap();
        assert_eq!((first.imported, first.skipped), (1, 1));
        // 文件被改短重写：从头再读，回执挡住重复。
        fs::write(&path, format!("{line}\n")).unwrap();
        let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 0);
        assert_eq!(rows(&db).len(), 1);
    }

    #[test]
    fn workbuddy_usage_counts_tool_calls_and_subagents_and_rereads_v1_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        let message = assistant("a1", "m", 300, 0, 0, 5);
        let call = |id: &str, session: &str| {
            serde_json::json!({
                "id": id, "timestamp": 1792764380000_i64, "type": "function_call",
                "name": "Read", "callId": "c", "sessionId": session,
                "providerData": {"model": "m", "messageId": format!("msg-{id}"), "rawUsage": {
                    "prompt_tokens": 1000, "completion_tokens": 20,
                    "prompt_cache_hit_tokens": 900, "prompt_cache_miss_tokens": 100
                }}
            })
            .to_string()
        };
        // 同一请求里先出现、不带 rawUsage 的 function_call 不算。
        let empty_call = r#"{"id":"c0","timestamp":1792764380000,"type":"function_call","name":"Glob","sessionId":"s1","providerData":{"model":"m"}}"#;
        fs::write(
            &path,
            format!("{message}\n{empty_call}\n{}\n", call("c1", "s1")),
        )
        .unwrap();
        let subagents = path.with_extension("").join("subagents");
        fs::create_dir_all(&subagents).unwrap();
        fs::write(
            subagents.join("agent-x.jsonl"),
            format!("{}\n", call("c2", "agent-own-id")),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        // 模拟 v1：只导过 assistant message，游标已到文件末尾。
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO session_usage_dedup (data_source, request_id, semantic_id, has_entry_id) VALUES (?1, 'workbuddy:s1:a1', 'workbuddy:s1:a1', 1)",
                [DATA_SOURCE],
            )
            .unwrap();
            let meta = fs::metadata(&path).unwrap();
            update_sync_state_on_conn(
                &conn,
                &format!("workbuddy:{}", path.to_string_lossy()),
                metadata_modified_nanos(&meta),
                meta.len() as i64,
            )
            .unwrap();
        }

        let result = sync_projects(&db, dir.path()).unwrap();
        assert_eq!((result.imported, result.skipped), (2, 1));
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT request_id, session_id, input_tokens, cache_read_tokens FROM proxy_request_logs WHERE app_type = 'workbuddy' ORDER BY request_id")
            .unwrap();
        let imported: Vec<(String, String, i64, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            imported,
            vec![
                ("workbuddy:s1:c1".into(), "s1".into(), 100, 900),
                // 子代理的请求记在父会话 s1 名下，不用子代理自己的 sessionId。
                ("workbuddy:s1:c2".into(), "s1".into(), 100, 900),
            ]
        );
    }

    #[test]
    fn workbuddy_usage_skips_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let result = sync_projects(&db, &dir.path().join("missing")).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
    }

    fn usage_call(
        id: &str,
        message_id: &str,
        timestamp: i64,
        prompt: u64,
        hit: u64,
        completion: u64,
    ) -> String {
        serde_json::json!({
            "id": id, "timestamp": timestamp, "type": "function_call", "name": "Read",
            "callId": "c", "sessionId": "s1",
            "providerData": {"model": "glm-test", "messageId": message_id, "rawUsage": {
                "prompt_tokens": prompt, "completion_tokens": completion,
                "prompt_cache_hit_tokens": hit, "prompt_cache_miss_tokens": prompt - hit
            }}
        })
        .to_string()
    }

    fn latency_of(db: &Database, request_id: &str) -> i64 {
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT latency_ms FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn workbuddy_usage_estimates_latency_from_previous_group() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        let base = 1_700_000_000_000i64;
        // 上一次工具调用结束是这次请求的起点
        let previous = serde_json::json!({
            "id": "r0", "timestamp": base, "type": "function_call_result",
            "sessionId": "s1", "providerData": {"messageId": "prev"}
        })
        .to_string();
        // 同一次调用先写 reasoning，再写带用量的 function_call，两条 messageId 相同
        let reasoning = serde_json::json!({
            "id": "r1", "timestamp": base + 5_000, "type": "reasoning",
            "sessionId": "s1", "providerData": {"messageId": "m1"}
        })
        .to_string();
        fs::write(
            &path,
            format!(
                "{previous}\n{reasoning}\n{}\n",
                usage_call("a1", "m1", base + 5_020, 1_000, 900, 20)
            ),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        assert_eq!(sync_projects(&db, dir.path()).unwrap().imported, 1);
        // 起点跳过同组记录，取上一条异 messageId 的记录
        assert_eq!(latency_of(&db, "workbuddy:s1:a1"), 5_020);
    }

    #[test]
    fn workbuddy_usage_ignores_implausibly_short_latency() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        let base = 1_700_000_000_000i64;
        let previous = serde_json::json!({
            "id": "r0", "timestamp": base, "type": "function_call_result",
            "sessionId": "s1", "providerData": {"messageId": "prev"}
        })
        .to_string();
        fs::write(
            &path,
            format!(
                "{previous}\n{}\n",
                usage_call("a1", "m1", base + 50, 1_000, 900, 20)
            ),
        )
        .unwrap();

        let db = Database::memory().unwrap();
        sync_projects(&db, dir.path()).unwrap();
        assert_eq!(latency_of(&db, "workbuddy:s1:a1"), 0);
    }

    #[test]
    fn workbuddy_usage_refreshes_legacy_rows_without_latency() {
        let dir = tempfile::tempdir().unwrap();
        let path = session_file(dir.path());
        let base = 1_700_000_000_000i64;
        let mut records = Vec::new();
        let mut ts = base;
        let mut expected = Vec::new();
        for i in 0..3 {
            // 第一条没有锚点；其余间隔分别是 5s、10s
            ts += if i == 0 { 100 } else { 5_000 * i };
            records.push(usage_call(
                &format!("a{i}"),
                &format!("m{i}"),
                ts,
                1_000,
                900,
                20,
            ));
            expected.push(if i == 0 { 0 } else { 5_000 * i });
        }
        fs::write(&path, format!("{}\n", records.join("\n"))).unwrap();
        let db = Database::memory().unwrap();
        sync_projects(&db, dir.path()).unwrap();

        // 模拟旧版本写出的行：只有用量，没有耗时
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE proxy_request_logs SET latency_ms = 0", [])
            .unwrap();

        // 模拟旧版本：游标是 v2 键、文件已读完，明细行没有耗时
        {
            let conn = db.conn.lock().unwrap();
            let meta = fs::metadata(&path).unwrap();
            conn.execute("DELETE FROM session_log_sync", []).unwrap();
            conn.execute(
                "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES (?1, ?2, ?3, 1)",
                rusqlite::params![
                    format!("workbuddy:v2:{}", path.to_string_lossy()),
                    metadata_modified_nanos(&meta),
                    meta.len() as i64,
                ],
            )
            .unwrap();
        }

        // 版本号变化后整体重读；收据命中时补耗时
        sync_projects(&db, dir.path()).unwrap();

        let restored: Vec<i64> = (0..3)
            .map(|i| latency_of(&db, &format!("workbuddy:s1:a{i}")))
            .collect();
        assert_eq!(restored, expected);
        let v3_cursors: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM session_log_sync WHERE file_path LIKE 'workbuddy:v3:%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v3_cursors, 1);
    }

    #[test]
    fn workbuddy_first_token_prefers_trace_calibration() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        let project = projects.join("proj");
        fs::create_dir_all(&project).unwrap();
        let path = project.join("s1.jsonl");
        let trace = dir
            .path()
            .join("traces")
            .join("worker")
            .join("trace_x.jsonl");

        let base = 1_700_000_000_000i64;
        let mut records = Vec::new();
        let mut spans = Vec::new();
        let mut ts = base;
        for i in 0..(TTFT_MIN_SAMPLES + 60) {
            // miss / hit 不能共线，否则正规方程奇异
            let miss = 8_000 + (i as i64 * 7_919) % 30_000;
            let hit = 40_000 + (i as i64 * 104_729) % 300_000;
            let out = 80 + (i as i64 * 613) % 2_000;
            // trace 里的精确耗时：span = 500 + miss×0.01 + hit×0.002 + out×3
            let span = (500.0 + 0.01 * miss as f64 + 0.002 * hit as f64 + 3.0 * out as f64) as i64;
            // 锚点耗时在 span 之上再加 3s 的应用开销；间隔足够大保证唯一匹配
            ts += span + 3_000;
            records.push(usage_call(
                &format!("a{i}"),
                &format!("m{i}"),
                ts,
                (miss + hit) as u64,
                hit as u64,
                out as u64,
            ));
            let ended = chrono::DateTime::from_timestamp_millis(ts).unwrap();
            spans.push(
                serde_json::json!({
                    "type": "span_end",
                    "span": {
                        "traceId": format!("trace_{i}"),
                        "name": "generation",
                        "endedAt": ended.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        "duration": span,
                        "status": "ok",
                    }
                })
                .to_string(),
            );
        }
        fs::write(&path, format!("{}\n", records.join("\n"))).unwrap();
        fs::create_dir_all(trace.parent().unwrap()).unwrap();
        fs::write(&trace, format!("{}\n", spans.join("\n"))).unwrap();

        let db = Database::memory().unwrap();
        sync_all(&db, &projects, &dir.path().join("traces")).unwrap();

        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT input_tokens, cache_read_tokens, latency_ms, first_token_ms FROM proxy_request_logs WHERE app_type = 'workbuddy'")
            .unwrap();
        let rows: Vec<(i64, i64, i64, Option<i64>)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows.len(), TTFT_MIN_SAMPLES + 60);
        let mut fitted = 0;
        for (miss, hit, latency, first_token) in rows {
            // 第一条没有锚点，耗时为 0，不参与估算
            if latency == 0 {
                assert!(first_token.is_none());
                continue;
            }
            let first_token = first_token.expect("first token should be calibrated from spans");
            // 必须用 span 模型而不是锚点模型：锚点口径会多出 3s 的应用开销
            let want = 500.0 + 0.01 * miss as f64 + 0.002 * hit as f64;
            assert!(
                (first_token as f64 - want).abs() < 2.0,
                "ttft {first_token} vs {want}"
            );
            assert!(first_token <= latency);
            fitted += 1;
        }
        assert!(fitted >= TTFT_MIN_SAMPLES);
    }
}
