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
//! 耗时：日志只在响应结束时打一个时间戳。耗时取「同一 `messageId` 分组之前最后一条
//! 记录」到本条记录的间隔（钳在 100ms–1h）；`traces/<pid>/trace_*.jsonl` 里的
//! `generation` span 有精确起止（与日志没有共同 id，只能按时间窗匹配），导入时唯一
//! 匹配上就用 span 的 duration 覆盖锚点值——锚点差值含「工具结果落盘 + 组装上下文」
//! 的应用开销（实测中位 0.12s、p75 0.92s），留在耗时里会让估算速度偏低（速度按
//! 输出 ÷ 整段耗时算，不减首字）。只处理本轮新入库的行，不回写历史。
//!
//! 首字不做逐行估算（日志没有这个时间戳，逐行数字给不出精度）；汇总表按模型用
//! 耗时回归给一个量级参考，见 [`estimated_model_first_token_ms`]。
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::{
    session_usage::{
        cost_columns, estimated_latency_ms, load_sync_cursors, metadata_modified_nanos,
        update_sync_state_on_conn, SessionSyncResult, MAX_PLAUSIBLE_LATENCY_MS,
        MIN_PLAUSIBLE_LATENCY_MS,
    },
    sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH,
    usage_stats::find_model_pricing,
};
use crate::session_manager::providers::workbuddy;
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const DATA_SOURCE: &str = "workbuddy_session";
/// 游标键前缀。改了「哪些记录算请求」或派生字段的口径时递增，让已同步过的文件从头重读；
/// 已入库的请求由 `session_usage_dedup` 回执挡住，只补新认出的或缺失的字段。
const CURSOR_PREFIX: &str = "workbuddy:v3:";

/// 耗时回看窗口：游标停在文件中同时，回看这么多字节把时间线接上。
const TIMING_PREFETCH_BYTES: i64 = 64 * 1024;

pub fn sync_workbuddy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_all(
        db,
        &workbuddy::projects_dir(),
        &crate::workbuddy_config::data_dir().join("traces"),
    )
}

/// `tests` 用临时目录走这条路径，避免碰真实 `~/.workbuddy`。
fn sync_all(db: &Database, projects: &Path, traces: &Path) -> Result<SessionSyncResult, AppError> {
    let mut pending = Vec::new();
    let result = sync_projects(db, projects, &mut pending)?;
    // 只给这一轮新入库的请求补精确耗时；失败只记日志，不影响已导入的用量。
    if let Err(error) = apply_trace_durations(db, traces, &pending) {
        log::warn!("WorkBuddy 耗时校准失败: {error}");
    }
    Ok(result)
}

/// `pending` 收集本轮真正入库的请求，同步结束后拿去 trace 里找精确耗时。
fn sync_projects(
    db: &Database,
    projects: &Path,
    pending: &mut Vec<PendingRequest>,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let files = workbuddy::usage_files(projects);
    if files.is_empty() {
        return Ok(result);
    }
    let cursors = load_sync_cursors(db)?;
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
        match sync_file(
            db,
            &path,
            parent_session.as_deref(),
            &key,
            modified,
            offset,
            pending,
        ) {
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
    pending: &mut Vec<PendingRequest>,
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
        pending.push(PendingRequest {
            request_id: entry.request_id,
            created_ms: entry.created_ms,
            latency_ms: entry.latency_ms,
        });
        result.imported += 1;
    }
    update_sync_state_on_conn(&tx, key, modified, bytes)?;
    tx.commit()?;
    Ok(result)
}

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

/// 行时间戳只有秒精度（`created_at` 是完成时刻的秒级截断），trace 的 `endedAt`
/// 是毫秒。实测 `span_end − created_at×1000` 落在 0–1000ms（p1=10、p99=974），
/// 两头各留一点余量覆盖写入延迟和时钟抖动；窗口再放宽只会捞到相邻请求的 span。
const SPAN_MATCH_EARLY_MS: i64 = 100;
const SPAN_MATCH_LATE_MS: i64 = 1100;
/// 匹配窗口比请求间隔宽（两条请求只差 1 秒时窗口重叠 200ms），比请求时间早太多的
/// trace 文件不可能含有能匹配上的 span，直接跳过不读。
const TRACE_FILE_MARGIN_MS: i64 = 60 * 1000;

/// 本轮新入库的请求：trace 里若有唯一对应的 `generation` span，用它的 duration
/// 覆盖锚点耗时。[`apply_trace_durations`] 只处理这些行，不回写历史。
struct PendingRequest {
    request_id: String,
    created_ms: i64,
    latency_ms: i64,
}

/// 给刚入库的请求补精确耗时。trace 只做只读访问，失败时调用方记日志、保留锚点值。
fn apply_trace_durations(
    db: &Database,
    traces_dir: &Path,
    pending: &[PendingRequest],
) -> Result<(), AppError> {
    if pending.is_empty() {
        return Ok(());
    }
    let since = pending
        .iter()
        .map(|request| request.created_ms)
        .min()
        .unwrap_or_default()
        - TRACE_FILE_MARGIN_MS;
    // trace 只追加，文件 mtime 就是它最后一条 span 的时间：早于本批最早请求的文件
    // 不可能命中，不用打开。活跃时通常只剩一两个正在写的文件。
    let files: Vec<TraceFile> = trace_file_stats(traces_dir)
        .into_iter()
        .filter(|file| file.modified >= since.saturating_mul(1_000_000))
        .collect();
    let spans = collect_generation_spans(&files);
    if spans.is_empty() {
        return Ok(());
    }

    // 先给每条请求找唯一候选，再剔掉被多条请求同时认领的 span：窗口有 200ms 重叠，
    // 靠得近的两条请求可能各自都「唯一」命中同一个 span。
    let claimed: Vec<Option<usize>> = pending
        .iter()
        .map(|request| {
            // 入库时 `created_at` 是秒级截断，匹配窗口必须锚在这个值上，
            // 否则同一秒内的两条请求会落到不同窗口。
            let start_ms = request.created_ms / 1000 * 1000;
            let lo = spans.partition_point(|(ended, _)| *ended < start_ms - SPAN_MATCH_EARLY_MS);
            let hi = spans.partition_point(|(ended, _)| *ended <= start_ms + SPAN_MATCH_LATE_MS);
            (hi - lo == 1).then_some(lo)
        })
        .collect();
    let mut claims = vec![0u8; spans.len()];
    for index in claimed.iter().flatten() {
        claims[*index] = claims[*index].saturating_add(1);
    }
    let updates: Vec<(&str, i64)> = pending
        .iter()
        .zip(claimed)
        .filter_map(|(request, index)| {
            let index = index?;
            let duration = spans[index].1;
            (claims[index] == 1 && duration != request.latency_ms)
                .then_some((request.request_id.as_str(), duration))
        })
        .collect();
    if updates.is_empty() {
        return Ok(());
    }

    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "UPDATE proxy_request_logs SET latency_ms = ?2
             WHERE request_id = ?1 AND app_type = 'workbuddy' AND data_source = ?3
               AND latency_ms IS NOT ?2",
        )?;
        for (request_id, duration) in updates {
            stmt.execute(rusqlite::params![request_id, duration, DATA_SOURCE])
                .map_err(|e| AppError::Database(format!("写入 WorkBuddy 精确耗时失败: {e}")))?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// 一个 trace 文件：路径 + 元数据。
struct TraceFile {
    path: PathBuf,
    modified: i64,
}

/// trace 目录下的 jsonl 文件清单。trace 只追加，文件 mtime 就是它最后一条 span
/// 的时间，用来筛掉不可能命中本批请求的文件——只 stat，不解析。
fn trace_file_stats(traces_dir: &Path) -> Vec<TraceFile> {
    let mut files = Vec::new();
    let Ok(workers) = fs::read_dir(traces_dir) else {
        return files;
    };
    for worker in workers.flatten() {
        let Ok(entries) = fs::read_dir(worker.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            files.push(TraceFile {
                path,
                modified: metadata_modified_nanos(&meta),
            });
        }
    }
    files
}

/// trace 记录的字段子集。`generation` 的 `span_end` 行会带上整段 prompt
/// （`toolInput`，实测平均 23KB、占 trace 总字节的 65%），用 `Value` 解析等于
/// 把这几十兆字符串全物化；按结构体反序列化，不认识的字段直接跳过、不分配。
#[derive(serde::Deserialize)]
struct TraceLine<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    span: Option<TraceSpan<'a>>,
}

#[derive(serde::Deserialize)]
struct TraceSpan<'a> {
    name: Option<&'a str>,
    status: Option<&'a str>,
    #[serde(rename = "endedAt")]
    ended_at: Option<&'a str>,
    duration: Option<i64>,
}

/// 收集 trace 里 `generation` span 的 `(结束毫秒, duration 毫秒)`，
/// 只统计成功结束的 span；坏行跳过。
fn collect_generation_spans(files: &[TraceFile]) -> Vec<(i64, i64)> {
    let mut spans = Vec::new();
    let mut buffer = Vec::new();
    for file in files {
        let Ok(handle) = fs::File::open(&file.path) else {
            continue;
        };
        let mut reader = BufReader::new(handle);
        loop {
            buffer.clear();
            // 读进复用的缓冲区：一行可能有几十 KB，没必要每行都新分配。
            let Ok(size) = reader.read_until(b'\n', &mut buffer) else {
                break;
            };
            if size == 0 {
                break;
            }
            let Ok(line) = std::str::from_utf8(&buffer) else {
                continue;
            };
            // 便宜的预过滤：绝大多数行不是 span_end，不必进 JSON 解析。
            if !line.contains("\"span_end\"") {
                continue;
            }
            let Ok(record) = serde_json::from_str::<TraceLine>(line) else {
                continue;
            };
            if record.kind != Some("span_end") {
                continue;
            }
            let Some(span) = record.span else {
                continue;
            };
            if span.name != Some("generation") || span.status != Some("ok") {
                continue;
            }
            let Some(ended) = span.ended_at else {
                continue;
            };
            let Some(ended_ms) = crate::services::session_usage::parse_timestamp_millis(ended)
            else {
                continue;
            };
            let Some(duration) = span.duration else {
                continue;
            };
            // 与锚点耗时同一把尺子：休眠跨夜的超长 span、毫秒级的脏数据都不参与。
            if (MIN_PLAUSIBLE_LATENCY_MS..=MAX_PLAUSIBLE_LATENCY_MS).contains(&duration) {
                spans.push((ended_ms, duration));
            }
        }
    }
    spans.sort_unstable();
    spans
}

/// 模型级首字估算的最小样本量和拟合门槛（与聚合口径无关，只决定「估不估」）。
const TTFT_ESTIMATE_MIN_SAMPLES: usize = 200;
const TTFT_ESTIMATE_MIN_R2: f64 = 0.5;
/// 只用最近这么多天的样本：模型的下游、批次、负载都会变，越久的数据越不代表现在。
const TTFT_ESTIMATE_WINDOW_SECS: i64 = 30 * 24 * 3600;
/// 估算结果缓存多久。看板刷新间隔最短 5s，没有缓存等于每次刷新都重算一遍。
const TTFT_ESTIMATE_TTL_MS: i64 = 10 * 60 * 1000;

/// 模型级首字估算：WorkBuddy 的会话日志和 trace 都没有首字时间戳，只能从耗时里拆。
///
/// 对同一模型最近的请求拟合 `耗时 = a + b × 输出 token`，再取
/// `median(耗时 − b × 输出)` 当典型首字。用中位数而不是截距：耗时分布右偏，
/// 截距会被长尾排队抬高，而汇总额本来就是中位数口径。
///
/// 只用于汇总表按模型展示，不写进明细行：系数会随批次和负载漂移，同一个请求
/// 换个窗口重算能差半秒以上，逐行数字给不出这个精度。样本不足、解码项非正或
/// R² 不达标时返回 `None`（界面显示「—」）。
pub(crate) fn estimated_model_first_token_ms(
    conn: &rusqlite::Connection,
    model: &str,
) -> Option<u64> {
    /// 模型 → (算出来的时刻, 估算值)
    type Cache = Mutex<HashMap<String, (i64, Option<u64>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let now = chrono::Utc::now().timestamp_millis();
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((computed_at, value)) = guard.get(model) {
            if now - computed_at < TTFT_ESTIMATE_TTL_MS {
                return *value;
            }
        }
    }

    let value = fit_first_token_ms(conn, model, now / 1000 - TTFT_ESTIMATE_WINDOW_SECS);
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(model.to_owned(), (now, value));
    value
}

fn fit_first_token_ms(conn: &rusqlite::Connection, model: &str, since_secs: i64) -> Option<u64> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT output_tokens, latency_ms FROM proxy_request_logs
             WHERE app_type = 'workbuddy' AND data_source = ?1 AND model = ?2
               AND latency_ms > 0 AND output_tokens > 0 AND created_at >= ?3",
        )
        .ok()?;
    let samples: Vec<(f64, f64)> = stmt
        .query_map(rusqlite::params![DATA_SOURCE, model, since_secs], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?))
        })
        .ok()?
        .filter_map(Result::ok)
        .collect();
    if samples.len() < TTFT_ESTIMATE_MIN_SAMPLES {
        return None;
    }

    let count = samples.len() as f64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for &(x, y) in &samples {
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let denominator = count * sxx - sx * sx;
    if !(denominator.is_finite() && denominator > 0.0) {
        return None;
    }
    let slope = (count * sxy - sx * sy) / denominator;
    let intercept = (sy - slope * sx) / count;
    // 解码速度必须为正，否则说明样本被少数长尾主导，估出来的「首字」没有意义。
    if !(slope.is_finite() && slope > 0.0) {
        return None;
    }

    let mut residuals = Vec::with_capacity(samples.len());
    let mut durations = Vec::with_capacity(samples.len());
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    let mean = sy / count;
    for &(x, y) in &samples {
        ss_res += (y - (intercept + slope * x)).powi(2);
        ss_tot += (y - mean).powi(2);
        residuals.push(y - slope * x);
        durations.push(y);
    }
    if !ss_tot.is_finite() || ss_tot <= 0.0 || 1.0 - ss_res / ss_tot < TTFT_ESTIMATE_MIN_R2 {
        return None;
    }

    residuals.sort_by(|left, right| left.total_cmp(right));
    durations.sort_by(|left, right| left.total_cmp(right));
    let median = residuals[residuals.len() / 2];
    // 首字不可能超过同一批请求的典型耗时，也不该落到不合理的区间外。
    let ceiling = durations[durations.len() / 2];
    let estimate = median.min(ceiling).clamp(
        MIN_PLAUSIBLE_LATENCY_MS as f64,
        MAX_PLAUSIBLE_LATENCY_MS as f64,
    );
    Some(estimate.round() as u64)
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
        let first = sync_projects(&db, dir.path(), &mut Vec::new()).unwrap();
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
        assert_eq!(
            sync_projects(&db, dir.path(), &mut Vec::new())
                .unwrap()
                .imported,
            0
        );
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{}", assistant("a2", "glm-test", 500, 0, 100, 10)).unwrap();
        write!(file, "{{\"id\":\"a3\",").unwrap();
        drop(file);
        assert_eq!(
            sync_projects(&db, dir.path(), &mut Vec::new())
                .unwrap()
                .imported,
            1
        );
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
        let first = sync_projects(&db, dir.path(), &mut Vec::new()).unwrap();
        assert_eq!((first.imported, first.skipped), (1, 1));
        // 文件被改短重写：从头再读，回执挡住重复。
        fs::write(&path, format!("{line}\n")).unwrap();
        let file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            sync_projects(&db, dir.path(), &mut Vec::new())
                .unwrap()
                .imported,
            0
        );
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

        let result = sync_projects(&db, dir.path(), &mut Vec::new()).unwrap();
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
        let result = sync_projects(&db, &dir.path().join("missing"), &mut Vec::new()).unwrap();
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
        assert_eq!(
            sync_projects(&db, dir.path(), &mut Vec::new())
                .unwrap()
                .imported,
            1
        );
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
        sync_projects(&db, dir.path(), &mut Vec::new()).unwrap();
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
        sync_projects(&db, dir.path(), &mut Vec::new()).unwrap();

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
        sync_projects(&db, dir.path(), &mut Vec::new()).unwrap();

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

    /// 造一条请求：用户消息在 `at − 5s`（锚点耗时 5s），请求记录在 `at`，
    /// trace 里放 `span_durations` 个都结束于 `at` 的 generation span。
    fn write_single_request(
        root: &Path,
        projects: &Path,
        at: i64,
        span_durations: &[i64],
    ) -> PathBuf {
        let path = session_file(projects);
        let user = serde_json::json!({
            "id": "u1", "timestamp": at - 5_000, "type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "hi"}], "sessionId": "s1", "cwd": "/w"
        })
        .to_string();
        let record = usage_call("a1", "m1", at, 1_000, 200, 30);
        fs::write(&path, format!("{user}\n{record}\n")).unwrap();
        write_trace(root, at, span_durations)
    }

    /// 在 `at` 这个时刻写下若干个 `generation` span（duration 由参数给）。
    fn write_trace(root: &Path, at: i64, span_durations: &[i64]) -> PathBuf {
        let trace = root.join("traces").join("worker").join("trace_x.jsonl");
        fs::create_dir_all(trace.parent().unwrap()).unwrap();
        let ended = chrono::DateTime::from_timestamp_millis(at).unwrap();
        let lines: Vec<String> = span_durations
            .iter()
            .enumerate()
            .map(|(index, duration)| {
                serde_json::json!({
                    "type": "span_end",
                    "span": {
                        "traceId": format!("trace_{index}"),
                        "name": "generation",
                        "endedAt": ended.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        "duration": duration,
                        "status": "ok",
                    }
                })
                .to_string()
            })
            .collect();
        fs::write(&trace, format!("{}\n", lines.join("\n"))).unwrap();
        trace
    }

    #[test]
    fn workbuddy_latency_backfills_rows_without_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        // 文件第一条就是请求：没有锚点，耗时算不出来
        let at = 1_792_764_379_226i64;
        fs::write(
            session_file(&projects),
            format!("{}\n", usage_call("a1", "m1", at, 1_000, 200, 30)),
        )
        .unwrap();
        write_trace(dir.path(), at, &[2_500]);

        let db = Database::memory().unwrap();
        sync_all(&db, &projects, &dir.path().join("traces")).unwrap();
        // trace 有精确值就用，不该继续显示「—」
        assert_eq!(latency_of(&db, "workbuddy:s1:a1"), 2_500);
    }

    #[test]
    fn workbuddy_latency_uses_trace_duration_when_unique() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        write_single_request(dir.path(), &projects, 1_792_764_379_226, &[3_000]);
        let db = Database::memory().unwrap();
        sync_all(&db, &projects, &dir.path().join("traces")).unwrap();

        // 锚点给的是 5s，trace 说只跑了 3s：以 trace 为准
        assert_eq!(latency_of(&db, "workbuddy:s1:a1"), 3_000);
    }

    #[test]
    fn workbuddy_latency_keeps_anchor_when_span_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        // 同一秒里两个 span（并发请求）：认不出是哪一个，保持锚点
        write_single_request(dir.path(), &projects, 1_792_764_379_226, &[3_000, 4_000]);
        let db = Database::memory().unwrap();
        sync_all(&db, &projects, &dir.path().join("traces")).unwrap();

        assert_eq!(latency_of(&db, "workbuddy:s1:a1"), 5_000);
    }

    /// 插一条最简的 WorkBuddy 明细：只给估算用得到的字段。
    fn insert_duration_sample(db: &Database, model: &str, output: i64, latency: i64) {
        let now = chrono::Utc::now().timestamp();
        let mut conn = db.conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, latency_ms, status_code,
                created_at, data_source, output_tokens
             ) VALUES (?1, '_workbuddy_session', 'workbuddy', ?2, ?3, 200, ?4, ?5, ?6)",
            rusqlite::params![
                format!("workbuddy:test:{}", uuid_like()),
                model,
                latency,
                now,
                DATA_SOURCE,
                output
            ],
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn uuid_like() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn workbuddy_model_first_token_estimation_follows_durations() {
        let db = Database::memory().unwrap();
        // 耗时 = 2000 + 6 × 输出（外加一点抖动），首字应该估到 2000 附近
        for i in 0..300i64 {
            let output = 100 + (i * 37) % 900;
            let jitter = (i * 7919) % 400 - 200;
            insert_duration_sample(&db, "estimate-ok", output, 2_000 + 6 * output + jitter);
        }
        let conn = db.conn.lock().unwrap();
        let estimate = estimated_model_first_token_ms(&conn, "estimate-ok").unwrap();
        assert!(
            (estimate as i64 - 2_000).abs() <= 120,
            "estimate {estimate} 偏离 2000 太多"
        );
    }

    #[test]
    fn workbuddy_model_first_token_estimation_rejects_no_signal() {
        let db = Database::memory().unwrap();
        // 耗时和输出无关：拆不出解码项，不估
        for i in 0..300i64 {
            insert_duration_sample(&db, "estimate-flat", 100 + i % 500, 5_000);
        }
        // 样本不足
        for i in 0..50i64 {
            insert_duration_sample(&db, "estimate-few", 100 + i, 1_000 + 6 * i);
        }
        let conn = db.conn.lock().unwrap();
        assert!(estimated_model_first_token_ms(&conn, "estimate-flat").is_none());
        assert!(estimated_model_first_token_ms(&conn, "estimate-few").is_none());
    }
    #[test]
    fn workbuddy_latency_ignores_span_claimed_by_two_requests() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        // 两条请求只差 1 秒，只有一个 span 落在重叠区（T×1000+900）：
        // 各自看都是「唯一」，但同一个 span 不可能属于两条请求，两边都不采信。
        let at = 1_792_764_379_900i64;
        let path = session_file(&projects);
        let user = serde_json::json!({
            "id": "u1", "timestamp": at - 5_000, "type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "hi"}], "sessionId": "s1", "cwd": "/w"
        })
        .to_string();
        fs::write(
            &path,
            format!(
                "{user}\n{}\n{}\n",
                usage_call("a1", "m1", at, 1_000, 200, 30),
                // 第二条的锚点是第一条的时间戳，耗时 1s
                usage_call("a2", "m2", at + 1_000, 1_000, 200, 30),
            ),
        )
        .unwrap();
        write_trace(dir.path(), at, &[2_500]);

        let db = Database::memory().unwrap();
        sync_all(&db, &projects, &dir.path().join("traces")).unwrap();

        assert_eq!(latency_of(&db, "workbuddy:s1:a1"), 5_000);
        assert_eq!(latency_of(&db, "workbuddy:s1:a2"), 1_000);
    }
}
