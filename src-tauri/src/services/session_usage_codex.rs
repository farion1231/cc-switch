//! Codex 会话日志使用追踪
//!
//! 从 ~/.codex/sessions/ 下的 JSONL 会话文件中提取精确 token 使用数据，
//! 替代原有的 state_5.sqlite 估算方案。
//!
//! ## 数据流
//! ```text
//! ~/.codex/sessions/YYYY/MM/DD/*.jsonl → 增量解析 → delta 计算 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 解析的事件类型
//! - `session_meta` → 提取唯一 thread_id（子代理的 session_id 指向父线程）
//! - `turn_context` / `event_msg` (type=thread_settings_applied) → 提取当前 model
//! - `event_msg` (type=token_count) → 提取累计 token 用量，计算 delta
//! - `token_usage_record` → 随后的 token_count last 为 0 时补记这次用量

use crate::codex_config::get_codex_config_dir;
use crate::codex_rollout_file;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::ModelPricing;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    cost_columns, estimated_first_token_ms, estimated_latency_ms, metadata_modified_nanos,
    parse_timestamp_millis, update_sync_state, update_sync_state_on_conn, EstimatedTiming,
    SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::{
    find_model_pricing, has_suspected_codex_session_duplicate, should_skip_session_insert, DedupKey,
};
use crate::sync_utils::MutexExt;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
};

const CODEX_THREAD_REQUEST_ID_PREFIX: &str = "codex_session:thread-v1";

/// 累计 token 用量（跟踪 total_token_usage 字段）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CumulativeTokens {
    input: u64,
    cached_input: u64,
    cache_write_input: u64,
    output: u64,
}

/// 单次 API 调用的 token 增量
#[derive(Debug)]
struct DeltaTokens {
    input: u32,
    cached_input: u32,
    cache_write_input: u32,
    output: u32,
}

impl DeltaTokens {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.cached_input == 0 && self.output == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenCountersSignature {
    input: Option<u64>,
    cached_input: Option<u64>,
    output: Option<u64>,
    reasoning_output: Option<u64>,
    total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenUsageSignature {
    total: Option<TokenCountersSignature>,
    last: Option<TokenCountersSignature>,
}

#[derive(Debug, Clone)]
struct TimestampedTokenSignature {
    timestamp: DateTime<Utc>,
    signature: TokenUsageSignature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParentFileStamp {
    modified_nanos: i64,
    size: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    /// 卷序列号 + 文件 ID。`None`：文件系统不提供文件 ID（Windows 访问
    /// `\\wsl.localhost\...` 的 9P 共享时返回 ERROR_NOT_SUPPORTED），只凭
    /// 修改时间和大小判断文件变没变，否则 WSL 上的父 rollout 永远验证不了。
    #[cfg(windows)]
    identity: Option<(u64, [u8; 16])>,
}

impl ParentFileStamp {
    fn from_file(file: &fs::File) -> Option<Self> {
        let metadata = file.metadata().ok()?;
        #[cfg(windows)]
        let identity = windows_file_identity(file).ok()?;
        Some(Self {
            modified_nanos: metadata_modified_nanos(&metadata),
            size: metadata.len(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(windows)]
            identity,
        })
    }
}

/// `Ok(None)`：文件系统不支持查询文件 ID；其他失败照常报错。
#[cfg(windows)]
fn windows_file_identity(file: &fs::File) -> std::io::Result<Option<(u64, [u8; 16])>> {
    let mut information = FILE_ID_INFO::default();
    // SAFETY: `file` owns a live handle for this call, and `information` is a
    // valid writable FILE_ID_INFO buffer of the size passed to Windows.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            std::ptr::addr_of_mut!(information).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } != 0;
    if succeeded {
        return Ok(Some((
            information.VolumeSerialNumber,
            information.FileId.Identifier,
        )));
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error().map(|code| code as u32) {
        Some(ERROR_NOT_SUPPORTED | ERROR_INVALID_PARAMETER) => Ok(None),
        _ => Err(error),
    }
}

#[derive(Debug)]
struct ParentTokenTimeline {
    events: Vec<TimestampedTokenSignature>,
    max_timestamp: Option<DateTime<Utc>>,
    has_token_without_timestamp: bool,
    high_water: Option<CumulativeTokens>,
}

impl ParentTokenTimeline {
    fn signatures_before(
        &self,
        parent_path: &Path,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<TokenUsageSignature>, String> {
        if self.has_token_without_timestamp {
            return Err(format!(
                "父 rollout {} 的 token_count 缺少有效 timestamp",
                parent_path.display()
            ));
        }
        if self
            .max_timestamp
            .is_none_or(|timestamp| timestamp < cutoff)
        {
            return Err(format!(
                "父 rollout {} 尚未写到 child fork 时刻",
                parent_path.display()
            ));
        }
        Ok(self.signatures_at(cutoff))
    }

    fn signatures_at(&self, cutoff: DateTime<Utc>) -> Vec<TokenUsageSignature> {
        self.events
            .iter()
            .filter(|event| event.timestamp <= cutoff)
            .map(|event| event.signature.clone())
            .collect()
    }
}

#[derive(Debug)]
struct CachedParentTimeline {
    stamp: ParentFileStamp,
    timeline: Arc<ParentTokenTimeline>,
}

#[derive(Debug)]
struct CachedReplayPrefix {
    modified: i64,
    size: u64,
    prefix: usize,
}

#[derive(Debug)]
struct ParsedTokenEvent {
    line_offset: i64,
    signature: TokenUsageSignature,
    delta: DeltaTokens,
    event_index: Option<u32>,
    /// 不占用 `token_count` 序号的用量。远程压缩只写 `token_usage_record` 时用它，
    /// 避免把后面每次请求的 `event_index` 都往后挤。
    request_suffix: Option<String>,
    model: String,
    timestamp: Option<String>,
    /// 按事件时间戳估出来的请求耗时（含首字等待）和首字时间；估不出来为 None。
    timing: Option<EstimatedTiming>,
}

/// 只看每行开头这么多字节来判断行的种类：时间戳、类型、角色都在行头，
/// 后面的正文（工具输出可能上百 KB）不用解析。
const LINE_HEAD_BYTES: usize = 512;

fn is_usage_line(line: &str) -> bool {
    if line.contains("\"event_msg\"") {
        return line.contains("\"token_count\"") || line.contains("\"thread_settings_applied\"");
    }
    line.contains("\"turn_context\"")
        || line.contains("\"session_meta\"")
        || line.contains("\"token_usage_record\"")
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 从行头里取 `"key":"value"` 的值，needle 是静态的 `"key":"` 字节串。
/// 只用于时间戳、类型名、角色这类不含转义的短值，避免逐行分配搜索串。
fn head_str<'a>(head: &'a [u8], needle: &[u8]) -> Option<&'a str> {
    let start = find_bytes(head, needle)? + needle.len();
    let len = head[start..].iter().position(|b| *b == b'"')?;
    std::str::from_utf8(&head[start..start + len]).ok()
}

#[derive(Debug, PartialEq, Eq)]
enum TimingLine {
    /// 下一次请求只会在它之后发出：用户消息、`turn_context`
    Boundary,
    /// 工具结果，同样是下一次请求的起点
    ToolOutput,
    /// 模型输出的一项（思考、回复、工具调用），写在这一项生成完的时刻
    ModelOutput,
    /// `token_usage_record`：响应结束时写的用量记录（新版 Codex 才有）
    UsageRecord,
    /// 一轮开始（`task_started`）或被中断（`turn_aborted`）：没等到 `token_count`
    /// 的那次请求到此作废
    TurnReset,
    /// 新版 Codex 的 `item_completed`，而且是模型输出的思考或回复：带这一项开始
    /// 生成的时刻（`started_at_ms`，即上游开始输出这一项），一次响应里最早的那项就是首字
    OutputItemStarted(i64),
}

fn classify_timing_line(line: &str) -> Option<(i64, TimingLine)> {
    let bytes = line.as_bytes();
    let head = &bytes[..bytes.len().min(LINE_HEAD_BYTES)];
    let (envelope, payload) = match find_bytes(head, b"\"payload\":{") {
        Some(at) => head.split_at(at),
        None => (head, &[][..]),
    };
    let kind = match head_str(envelope, b"\"type\":\"")? {
        "turn_context" => TimingLine::Boundary,
        "token_usage_record" => TimingLine::UsageRecord,
        "response_item" => match head_str(payload, b"\"type\":\"")? {
            "message" => match head_str(payload, b"\"role\":\"")? {
                "assistant" => TimingLine::ModelOutput,
                _ => TimingLine::Boundary,
            },
            "reasoning" => TimingLine::ModelOutput,
            item if item.ends_with("_output") => TimingLine::ToolOutput,
            item if item.ends_with("_call") => TimingLine::ModelOutput,
            _ => return None,
        },
        "event_msg" => match head_str(payload, b"\"type\":\"")? {
            "task_started" | "turn_aborted" => TimingLine::TurnReset,
            "item_completed" => output_item_started_ms(line, payload)?,
            _ => return None,
        },
        _ => return None,
    };
    let timestamp_ms = head_str(envelope, b"\"timestamp\":\"").and_then(parse_timestamp_millis)?;
    Some((timestamp_ms, kind))
}

/// `item_completed` 行里模型输出项（思考、回复）的开始时刻。工具执行、用户消息
/// 这类项不是模型输出，不看。`started_at_ms` 写在 `item` 之后、可能超出行头，
/// 要解析整行；这类行很短（实测中位数约 400 字节）。
fn output_item_started_ms(line: &str, payload_head: &[u8]) -> Option<TimingLine> {
    let item_at = find_bytes(payload_head, b"\"item\":{")?;
    match head_str(&payload_head[item_at..], b"\"type\":\"")? {
        "Reasoning" | "AgentMessage" => {}
        _ => return None,
    }
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let started_at_ms = value.get("payload")?.get("started_at_ms")?.as_i64()?;
    Some(TimingLine::OutputItemStarted(started_at_ms))
}

/// 按事件顺序估每次请求的耗时。Codex 日志没有请求级计时，而且各版本的
/// 事件顺序不一样：
///
/// - 结束时刻：新版在响应结束时写 `token_usage_record`，直接用它。旧版只有
///   `token_count`，有的版本在响应结束时写，有的要等工具跑完才写；后一种
///   表现为「最后一个输出项之后先出现工具结果，`token_count` 紧跟着工具结果
///   写出」，这时改用最后一个输出项的时刻（略早于真正结束，速度会略偏高）。
/// - 开始时刻：看到这次请求第一个输出项时，在它之前最近的一个起点行
///   （上一次的 `token_count`、工具结果、用户消息、`turn_context`）。不直接取
///   结束前最近的起点行，是因为工具结果可能在响应还没结束时就写进来了。
///
/// 2025 年的旧布局把上一次响应的输出项补写在它的 `token_count` 之后（时间戳
/// 和那个 `token_count` 一样）。这些输出项不属于下一次请求，直接忽略；否则
/// 下一次请求的起点会被钉在上一次响应结束的时刻，用户隔很久才发下一条消息时
/// 算出来的耗时就长得离谱。
///
/// 首字时间：新版 Codex 给每个模型输出项记了开始生成的时刻（`item_completed` 的
/// `started_at_ms`），取这次请求里最早的一项减去起点。响应一开头就是工具调用、
/// 没有思考和回复项时推不出来。
///
/// 请求被中断或出错时等不到 `token_count`，它留下的起点要在下一轮开始时清掉，
/// 否则会被下一次请求沿用。只在 `task_started` / `turn_aborted` 上清，不在用户
/// 消息上清：Codex 会在响应中途插入 developer 消息，在那里清会把起点挪晚。
#[derive(Debug, Default)]
struct RequestTimer {
    last_boundary_ms: Option<i64>,
    last_token_count_ms: Option<i64>,
    request_start_ms: Option<i64>,
    last_model_output_ms: Option<i64>,
    /// 最后一个输出项之后出现的工具结果里最晚的一个；之后再有输出项就清空。
    tool_output_after_model_output_ms: Option<i64>,
    usage_record: Option<(i64, CumulativeTokens)>,
    /// 这次请求里最早开始生成的输出项的时刻，即首字。
    first_output_ms: Option<i64>,
}

/// 两行的时间戳相差不超过这个毫秒数，就算是同一批写出的（实测相差 0–1 毫秒，
/// 而一次真实的请求不可能这么快）。
const SAME_FLUSH_SLACK_MS: i64 = 100;

impl RequestTimer {
    fn observe_line(&mut self, line: &str) {
        let Some((timestamp_ms, kind)) = classify_timing_line(line) else {
            return;
        };
        match kind {
            TimingLine::Boundary => {
                self.last_boundary_ms = self.last_boundary_ms.max(Some(timestamp_ms));
            }
            TimingLine::ToolOutput => {
                self.last_boundary_ms = self.last_boundary_ms.max(Some(timestamp_ms));
                if self.last_model_output_ms.is_some() {
                    self.tool_output_after_model_output_ms = Some(timestamp_ms);
                }
            }
            TimingLine::ModelOutput => {
                let trails_token_count = self.request_start_ms.is_none()
                    && self
                        .last_token_count_ms
                        .is_some_and(|at| timestamp_ms - at <= SAME_FLUSH_SLACK_MS);
                if trails_token_count {
                    return;
                }
                if self.request_start_ms.is_none() {
                    self.request_start_ms = self.last_boundary_ms;
                }
                self.last_model_output_ms = Some(timestamp_ms);
                self.tool_output_after_model_output_ms = None;
            }
            TimingLine::UsageRecord => {
                // 行头只够判断种类，用量要解析整行；这类行很短
                let usage = serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("payload")
                            .and_then(|payload| payload.get("usage"))
                            .and_then(parse_cumulative_tokens)
                    });
                self.usage_record = usage.map(|usage| (timestamp_ms, usage));
            }
            TimingLine::OutputItemStarted(started_at_ms) => {
                self.first_output_ms = Some(
                    self.first_output_ms
                        .map_or(started_at_ms, |at| at.min(started_at_ms)),
                );
            }
            TimingLine::TurnReset => {
                *self = RequestTimer {
                    last_boundary_ms: self.last_boundary_ms.max(Some(timestamp_ms)),
                    last_token_count_ms: self.last_token_count_ms,
                    ..RequestTimer::default()
                };
            }
        }
    }

    /// 遇到一次有用量的 `token_count`：结算这次请求的耗时和首字，并把它记成下一次
    /// 请求的起点。`last` 是这次请求自己的用量，用来确认 `token_usage_record`
    /// 说的是同一次请求。
    fn finish_request(
        &mut self,
        token_count_ms: Option<i64>,
        last: Option<&CumulativeTokens>,
    ) -> Option<EstimatedTiming> {
        let start_ms = self.request_start_ms.or(self.last_boundary_ms);
        let first_output_ms = self.first_output_ms;
        let record_end_ms = self
            .usage_record
            .take()
            .filter(|(_, usage)| {
                last.is_none_or(|last| {
                    usage.input == last.input
                        && usage.cached_input == last.cached_input
                        && usage.output == last.output
                })
            })
            .map(|(timestamp_ms, _)| timestamp_ms);
        let token_count_waited_for_tools =
            match (self.tool_output_after_model_output_ms, token_count_ms) {
                (Some(tool_output_ms), Some(token_count_ms)) => {
                    token_count_ms - tool_output_ms <= SAME_FLUSH_SLACK_MS
                }
                (Some(_), None) => true,
                (None, _) => false,
            };
        let end_ms = record_end_ms.or(if token_count_waited_for_tools {
            self.last_model_output_ms
        } else {
            token_count_ms
        });

        *self = RequestTimer {
            last_boundary_ms: self.last_boundary_ms.max(token_count_ms),
            last_token_count_ms: token_count_ms,
            ..RequestTimer::default()
        };
        let start_ms = start_ms?;
        let latency_ms = estimated_latency_ms(start_ms, end_ms?)?;
        Some(EstimatedTiming {
            latency_ms,
            first_token_ms: first_output_ms.and_then(|first_output_ms| {
                estimated_first_token_ms(start_ms, first_output_ms, latency_ms)
            }),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParentResolution {
    None,
    Parent(String),
    Continuation {
        ancestor_parent_id: Option<String>,
        history_base_id: Option<String>,
    },
    Deferred(String),
}

#[derive(Debug)]
struct ParsedCodexFile {
    root_thread_id: Option<String>,
    /// root `session_meta` 的线程 ID（稳定逻辑线程）：单段文件名与文件名
    /// UUID 一致，revert/resume 的双段文件名对应前置 UUID。
    meta_thread_id: Option<String>,
    root_meta_seen: bool,
    bounded_history: Option<serde_json::Value>,
    /// root meta 的 `ordinal` 正好等于 `forked_from_ordinal_exclusive`：分叉子
    /// rollout 从父线程的截止序号接着编号，文件里只有分叉之后的记录。
    starts_at_fork_ordinal: bool,
    root_timestamp: Option<DateTime<Utc>>,
    parent: ParentResolution,
    token_events: Vec<ParsedTokenEvent>,
    /// `token_usage_record` 后面紧跟 last 全 0 的 `token_count` 时补记的用量。
    usage_record_events: Vec<ParsedTokenEvent>,
    line_offset: i64,
    /// Bytes actually read, including an incomplete final record. Persisted in
    /// `last_byte_offset` only to detect file changes, never used as a seek
    /// position: parsing restarts at the beginning and `line_offset` tracks
    /// consumed records so an incomplete tail can be retried after an append.
    observed_bytes: i64,
    has_billable_tokens: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingReason {
    MissingParent(String),
    Stable(String),
    Retryable {
        reason: String,
        parent_id: String,
        child_file: Option<ParentFileStamp>,
        // Sample before resolving the parent: a write during resolution must
        // invalidate this entry on the next pass, not be mistaken for the
        // already-observed state. None keeps metadata/open errors retryable.
        parent_files: Option<Vec<(PathBuf, ParentFileStamp)>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingEntry {
    modified: i64,
    size: u64,
    reason: PendingReason,
}

#[derive(Debug, Default)]
struct CodexReplayCaches {
    parent_timelines: HashMap<PathBuf, CachedParentTimeline>,
    replay_prefixes: HashMap<PathBuf, CachedReplayPrefix>,
    pending: HashMap<PathBuf, PendingEntry>,
}

static CODEX_REPLAY_CACHES: OnceLock<Mutex<CodexReplayCaches>> = OnceLock::new();

fn replay_caches() -> &'static Mutex<CodexReplayCaches> {
    CODEX_REPLAY_CACHES.get_or_init(|| Mutex::new(CodexReplayCaches::default()))
}

pub(crate) fn clear_codex_replay_caches() {
    // 清空即丢弃持锁 panic 时可能写了一半的内容，所以顺带解除中毒。
    let mut caches = replay_caches().lock_recover();
    *caches = CodexReplayCaches::default();
    replay_caches().clear_poison();
}

fn is_rollout_filename(file_name: &str) -> bool {
    let file_name = codex_rollout_file::logical_file_name(file_name);
    if !file_name.starts_with("rollout-") || !file_name.ends_with(".jsonl") {
        return false;
    }
    let stem = file_name.trim_end_matches(".jsonl");
    stem.get(stem.len().saturating_sub(36)..)
        .is_some_and(|candidate| uuid::Uuid::parse_str(candidate).is_ok())
}

fn is_codex_cursor_path(file_path: &str, codex_dir: &Path) -> bool {
    let path = Path::new(file_path);
    let file_name = file_path.rsplit(['/', '\\']).next().unwrap_or_default();
    if !is_rollout_filename(file_name) {
        return false;
    }

    if path.starts_with(codex_dir.join("sessions"))
        || path.starts_with(codex_dir.join("archived_sessions"))
    {
        return true;
    }

    // 兼容用户改过 CODEX_HOME 后遗留、且源文件已不存在的 cursor。只接受
    // 明确目录段 + Codex rollout UUID 文件名，避免宽 codex_dir 误删其他 importer。
    file_path
        .replace('\\', "/")
        .split('/')
        .any(|segment| matches!(segment, "sessions" | "archived_sessions"))
}

fn sqlite_table_exists(conn: &rusqlite::Connection, table: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("查询表 {table} 失败: {error}")))
}

fn sqlite_column_exists(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        rusqlite::params![table, column],
        |row| row.get(0),
    )
    .map_err(|error| AppError::Database(format!("查询列 {table}.{column} 失败: {error}")))
}

pub(crate) fn reset_codex_usage_on_conn(
    conn: &rusqlite::Connection,
    codex_dir: &Path,
) -> Result<(), AppError> {
    if sqlite_table_exists(conn, "proxy_request_logs")?
        && sqlite_column_exists(conn, "proxy_request_logs", "data_source")?
    {
        conn.execute(
            "DELETE FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 会话明细失败: {error}")))?;
    }
    if sqlite_table_exists(conn, "usage_daily_rollups")?
        && sqlite_column_exists(conn, "usage_daily_rollups", "provider_id")?
    {
        conn.execute(
            "DELETE FROM usage_daily_rollups WHERE provider_id = '_codex_session'",
            [],
        )
        .map_err(|error| AppError::Database(format!("清理 Codex 用量汇总失败: {error}")))?;
    }
    if sqlite_table_exists(conn, "session_log_sync")?
        && sqlite_column_exists(conn, "session_log_sync", "file_path")?
    {
        let paths = {
            let mut statement = conn
                .prepare("SELECT file_path FROM session_log_sync")
                .map_err(|error| {
                    AppError::Database(format!("读取会话同步 cursor 失败: {error}"))
                })?;
            let paths = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| AppError::Database(format!("查询会话同步 cursor 失败: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    AppError::Database(format!("解析会话同步 cursor 失败: {error}"))
                })?;
            paths
        };
        for file_path in paths
            .into_iter()
            .filter(|path| is_codex_cursor_path(path, codex_dir))
        {
            conn.execute(
                "DELETE FROM session_log_sync WHERE file_path = ?1",
                [file_path],
            )
            .map_err(|error| AppError::Database(format!("清理 Codex 同步 cursor 失败: {error}")))?;
        }
    }
    Ok(())
}

impl Database {
    pub(crate) fn reset_codex_usage(&self) -> Result<(), AppError> {
        let codex_dir = get_codex_config_dir();
        let conn = lock_conn!(self.conn);
        conn.execute("SAVEPOINT reset_codex_usage", [])
            .map_err(|error| AppError::Database(format!("开启 Codex 重建事务失败: {error}")))?;
        let result = reset_codex_usage_on_conn(&conn, &codex_dir);
        match result {
            Ok(()) => {
                conn.execute("RELEASE reset_codex_usage", [])
                    .map_err(|error| {
                        AppError::Database(format!("提交 Codex 重建事务失败: {error}"))
                    })?;
                drop(conn);
                clear_codex_replay_caches();
                Ok(())
            }
            Err(error) => {
                conn.execute("ROLLBACK TO reset_codex_usage", []).ok();
                conn.execute("RELEASE reset_codex_usage", []).ok();
                Err(error)
            }
        }
    }
}

fn non_empty_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn thread_id_from_filename(path: &Path) -> Option<String> {
    let logical = codex_rollout_file::logical_path(path);
    let stem = logical.file_stem()?.to_str()?;
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    uuid::Uuid::parse_str(candidate)
        .ok()
        .map(|value| value.hyphenated().to_string())
}

/// 双段文件名（`rollout-…-<threadId>_<rolloutId>.jsonl`）里下划线前的线程
/// 本体 UUID；单段文件名返回 `None`。
///
/// `thread/revert` 为同一线程新建替换 rollout 时产生这种文件名（见
/// openai/codex#38127）：末段是新生成的 rollout ID，其后的 resume 继续
/// 向该文件追加。root meta 的 `id` 始终是原线程 ID，一致性校验需同时
/// 接受两个 UUID。
fn leading_thread_id_from_filename(path: &Path) -> Option<String> {
    let logical = codex_rollout_file::logical_path(path);
    let stem = logical.file_stem()?.to_str()?;
    let len = stem.len();
    // 布局尾部：…<uuidA>_<uuidB>。uuidB 占 36 字符，其前是 '_'（共 37）
    if !stem.get(len.checked_sub(37)?..)?.starts_with('_') {
        return None;
    }
    let candidate = stem.get(len.checked_sub(73)?..len.checked_sub(37)?)?;
    uuid::Uuid::parse_str(candidate)
        .ok()
        .map(|value| value.hyphenated().to_string())
}

fn explicit_parent_from_meta(payload: &serde_json::Value) -> ParentResolution {
    let forked_from = non_empty_string(payload.get("forked_from_id"));
    let spawned_from = payload
        .get("source")
        .and_then(|source| source.get("subagent"))
        .and_then(|subagent| subagent.get("thread_spawn"))
        .and_then(|spawn| non_empty_string(spawn.get("parent_thread_id")));

    match (forked_from, spawned_from) {
        (None, None) => ParentResolution::None,
        (Some(parent), None) | (None, Some(parent)) => ParentResolution::Parent(parent),
        (Some(forked), Some(spawned)) if forked == spawned => ParentResolution::Parent(forked),
        (Some(forked), Some(spawned)) => ParentResolution::Deferred(format!(
            "forked_from_id ({forked}) 与 thread_spawn.parent_thread_id ({spawned}) 不一致"
        )),
    }
}

fn parse_timestamp(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(serde_json::Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn parse_signature_counters(value: Option<&serde_json::Value>) -> Option<TokenCountersSignature> {
    let value = value?.as_object()?;
    Some(TokenCountersSignature {
        input: value
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64),
        cached_input: value
            .get("cached_input_tokens")
            .or_else(|| value.get("cache_read_input_tokens"))
            .and_then(serde_json::Value::as_u64),
        output: value
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64),
        reasoning_output: value
            .get("reasoning_output_tokens")
            .and_then(serde_json::Value::as_u64),
        total: value
            .get("total_tokens")
            .and_then(serde_json::Value::as_u64),
    })
}

fn parse_token_signature(info: &serde_json::Value) -> Option<TokenUsageSignature> {
    let total = parse_signature_counters(info.get("total_token_usage"));
    let last = parse_signature_counters(info.get("last_token_usage"));
    (total.is_some() || last.is_some()).then_some(TokenUsageSignature { total, last })
}

fn token_snapshot_source(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("rate_limits")
        .and_then(|rate_limits| rate_limits.get("limit_id"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// 单个同步 pass 的共享状态。
///
/// - `cursors`：pass 开始时一次性预载的 `session_log_sync` 快照，替代逐文件
///   SELECT（尤其是游标继承要按文件名查同一 rollout 的其他路径，逐文件跑等于
///   每 pass 全表扫 N 次）。快照语义：同 pass 内其他文件刚写入的游标对后续
///   游标继承不可见——影响仅是多一轮由 request_id 去重兜底的重扫，
///   不丢数据、不双算。
/// - `pricing`：模型定价 pass 级缓存。定价表在 pass 进行中被修改时本 pass
///   仍用旧价，下一个同步 pass 生效。
struct CodexSyncPass<'a> {
    cursors: std::borrow::Cow<'a, crate::services::session_usage::SyncCursors>,
    /// rollout 逻辑文件名（去掉 `.zst`）→ 记过游标的路径，供游标继承按名查找
    rollout_cursor_paths: HashMap<String, Vec<String>>,
    pricing: HashMap<String, Option<ModelPricing>>,
}

impl<'a> CodexSyncPass<'a> {
    #[cfg(test)]
    fn load(db: &Database) -> Result<Self, AppError> {
        Ok(Self::from_cursors(std::borrow::Cow::Owned(
            crate::services::session_usage::load_sync_cursors(db)?,
        )))
    }

    fn from_cursors(
        cursors: std::borrow::Cow<'a, crate::services::session_usage::SyncCursors>,
    ) -> Self {
        let mut rollout_cursor_paths: HashMap<String, Vec<String>> = HashMap::new();
        for path in cursors.keys() {
            let name = codex_rollout_file::logical_file_name(path);
            if is_rollout_filename(name) {
                rollout_cursor_paths
                    .entry(name.to_string())
                    .or_default()
                    .push(path.clone());
            }
        }
        Self {
            cursors,
            rollout_cursor_paths,
            pricing: HashMap::new(),
        }
    }
}

/// 读游标；同一个 rollout 记在别的路径下的游标读得更远时继承它。
///
/// 路径会变的情况：Codex 归档 / 取消归档会移动文件，压缩成 `.jsonl.zst`
/// 或恢复对话时解压回 `.jsonl` 会换后缀。行号只看内容，各形态通用。
/// 🔴 这是压缩文件不被重复计费的唯一屏障：request_id 去重只覆盖明细保留的
/// 30 天，而被压缩的文件至少 7 天没动、往往更老。
fn get_codex_sync_state(
    db: &Database,
    file_path: &Path,
    pass: &CodexSyncPass,
) -> Result<(i64, i64), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let state = pass
        .cursors
        .get(&file_path_str)
        .map(|c| (c.last_modified, c.last_line_offset))
        .unwrap_or((0, 0));
    let logical_name = codex_rollout_file::logical_file_name(&file_path_str);
    let Some(paths) = pass.rollout_cursor_paths.get(logical_name) else {
        return Ok(state);
    };
    // 与原 SQL 等价：ORDER BY last_line_offset DESC, last_modified DESC LIMIT 1
    // → 在快照上按 (offset, modified) 取最大。
    let inherited = paths
        .iter()
        .filter(|path| path.as_str() != file_path_str)
        .filter_map(|path| pass.cursors.get(path))
        .map(|cursor| (cursor.last_line_offset, cursor.last_modified))
        .max();

    match inherited {
        Some((offset, modified)) if offset > state.1 => {
            update_sync_state(db, &file_path_str, modified, offset)?;
            Ok((modified, offset))
        }
        _ => Ok(state),
    }
}

/// 归一化 Codex 模型名
///
/// 处理规则（按顺序）：
/// 1. 转小写：`GLM-4.6` → `glm-4.6`
/// 2. 剥离 provider 前缀：`openai/gpt-5.4` → `gpt-5.4`
/// 3. 剥离 ISO 日期后缀：`gpt-5.4-2026-03-05` → `gpt-5.4`
/// 4. 剥离紧凑日期后缀：`gpt-5.4-20260305` → `gpt-5.4`
fn normalize_codex_model(raw: &str) -> String {
    // Step 1: 小写
    let mut name = raw.to_lowercase();

    // Step 2: 剥离 "provider/" 前缀（如 openai/, azure/）
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }

    // Step 3: 剥离 ISO 日期后缀 -YYYY-MM-DD（正好 11 字符）
    if name.len() > 11 && name.is_char_boundary(name.len() - 11) {
        let suffix = &name[name.len() - 11..];
        if suffix.is_ascii()
            && suffix.as_bytes()[0] == b'-'
            && suffix[1..5].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[5] == b'-'
            && suffix[6..8].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[8] == b'-'
            && suffix[9..11].chars().all(|c| c.is_ascii_digit())
        {
            name.truncate(name.len() - 11);
        }
    }

    // Step 4: 剥离紧凑日期后缀 -YYYYMMDD（正好 9 字符）
    if name.len() > 9 {
        let parts: Vec<&str> = name.rsplitn(2, '-').collect();
        if parts.len() == 2 {
            if let Some(suffix) = parts.first() {
                if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
                    name = parts[1].to_string();
                }
            }
        }
    }

    name
}

/// 计算两次累计值之间的 delta
fn compute_delta(prev: &Option<CumulativeTokens>, current: &CumulativeTokens) -> DeltaTokens {
    match prev {
        None => DeltaTokens {
            input: current.input as u32,
            cached_input: current.cached_input as u32,
            cache_write_input: current.cache_write_input as u32,
            output: current.output as u32,
        },
        Some(p) => DeltaTokens {
            input: current.input.saturating_sub(p.input) as u32,
            cached_input: current.cached_input.saturating_sub(p.cached_input) as u32,
            cache_write_input: current
                .cache_write_input
                .saturating_sub(p.cache_write_input) as u32,
            output: current.output.saturating_sub(p.output) as u32,
        },
    }
}

fn usage_matches(last: &CumulativeTokens, usage: &CumulativeTokens) -> bool {
    last.input == usage.input
        && last.cached_input == usage.cached_input
        && last.cache_write_input == usage.cache_write_input
        && last.output == usage.output
}

fn usage_is_zero(usage: &CumulativeTokens) -> bool {
    usage.input == 0 && usage.cached_input == 0 && usage.cache_write_input == 0 && usage.output == 0
}

fn delta_from_usage(usage: &CumulativeTokens) -> DeltaTokens {
    let input = usage.input.min(u32::MAX as u64) as u32;
    let cached_input = usage.cached_input.min(usage.input).min(u32::MAX as u64) as u32;
    let fresh_input = input.saturating_sub(cached_input);
    DeltaTokens {
        input,
        cached_input,
        cache_write_input: (usage.cache_write_input.min(u32::MAX as u64) as u32).min(fresh_input),
        output: usage.output.min(u32::MAX as u64) as u32,
    }
}

fn signature_for_last_usage(usage: &CumulativeTokens) -> TokenUsageSignature {
    TokenUsageSignature {
        total: None,
        last: Some(TokenCountersSignature {
            input: Some(usage.input),
            cached_input: Some(usage.cached_input),
            output: Some(usage.output),
            reasoning_output: None,
            total: Some(usage.input.saturating_add(usage.output)),
        }),
    }
}

struct PendingUsageRecord {
    key: String,
    usage: CumulativeTokens,
    timestamp: Option<String>,
    model: String,
}

fn push_unmatched_usage(
    events: &mut Vec<ParsedTokenEvent>,
    pending: PendingUsageRecord,
    flush_line_offset: i64,
    has_billable_tokens: &mut bool,
) {
    let delta = delta_from_usage(&pending.usage);
    if delta.is_zero() {
        return;
    }
    *has_billable_tokens = true;
    // 事件挂在补偿它的 token_count 行上而不是记录行：记录行可能在更早的
    // 同步轮就被游标越过（首轮无可计费内容也推进游标），挂回记录行会被
    // `line_offset <= last_offset` 门永久丢弃。配对 token_count 行必然晚于
    // 任何已有游标（每轮全量重解析、追加写文件），且 `usage:<key>` 请求 ID
    // 保证重解析不重复入账。
    events.push(ParsedTokenEvent {
        line_offset: flush_line_offset,
        signature: signature_for_last_usage(&pending.usage),
        delta,
        event_index: None,
        request_suffix: Some(format!("usage:{}", pending.key)),
        model: pending.model,
        timestamp: pending.timestamp,
        timing: None,
    });
}

fn update_high_water(high_water: &mut CumulativeTokens, current: &CumulativeTokens) {
    high_water.input = high_water.input.max(current.input);
    high_water.cached_input = high_water.cached_input.max(current.cached_input);
    high_water.cache_write_input = high_water.cache_write_input.max(current.cache_write_input);
    high_water.output = high_water.output.max(current.output);
}

/// 从 JSON Value 中提取累计 token 用量
fn parse_cumulative_tokens(total_usage: &serde_json::Value) -> Option<CumulativeTokens> {
    let fields = total_usage.as_object()?;
    if ![
        "input_tokens",
        "cached_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
    ]
    .iter()
    .any(|field| fields.contains_key(*field))
    {
        return None;
    }
    Some(CumulativeTokens {
        input: total_usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cached_input: total_usage
            .get("cached_input_tokens")
            .or_else(|| total_usage.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_write_input: total_usage
            .get("cache_write_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output: total_usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

type RolloutIndex = HashMap<String, Vec<PathBuf>>;

#[derive(Debug, Default)]
struct CodexFileSyncResult {
    imported: u32,
    skipped: u32,
    suspected_duplicates: u32,
    deferred: bool,
}

/// 同步 Codex 使用数据（从 JSONL 会话日志）
pub fn sync_codex_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_codex_usage_with_cursors(db, &crate::services::session_usage::load_sync_cursors(db)?)
}

pub(crate) fn sync_codex_usage_with_cursors(
    db: &Database,
    cursors: &crate::services::session_usage::SyncCursors,
) -> Result<SessionSyncResult, AppError> {
    let codex_dir = get_codex_config_dir();
    let files = collect_codex_session_files(&codex_dir);
    let rollout_index = build_rollout_index(&files);
    let mut pass = CodexSyncPass::from_cursors(std::borrow::Cow::Borrowed(cursors));

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    for file_path in &files {
        match sync_single_codex_file(db, file_path, &rollout_index, &mut pass) {
            Ok(file_result) => {
                result.imported = result.imported.saturating_add(file_result.imported);
                result.skipped = result.skipped.saturating_add(file_result.skipped);
                result.suspected_duplicates = result
                    .suspected_duplicates
                    .saturating_add(file_result.suspected_duplicates);
                if file_result.deferred {
                    result.deferred_files = result.deferred_files.saturating_add(1);
                }
            }
            Err(e) => {
                let msg = format!("Codex 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[CODEX-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 || result.deferred_files > 0 {
        log::info!(
            "[CODEX-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, deferred {} 个, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.deferred_files,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Codex 会话 JSONL 文件
fn collect_codex_session_files(codex_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // 1. 扫描 sessions/YYYY/MM/DD/*.jsonl（日期分区目录）
    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.is_dir() {
        collect_jsonl_recursive(&sessions_dir, &mut files, 0, 3);
    }

    // 2. 扫描 archived_sessions/*.jsonl（扁平归档目录）
    let archived_dir = codex_dir.join("archived_sessions");
    if archived_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&archived_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if codex_rollout_file::is_rollout_file(&path) {
                    files.push(path);
                }
            }
        }
    }

    files.sort();
    // 压缩 / 解压的瞬间两种形态并存时只读普通形态，避免同一线程出现两个候选
    codex_rollout_file::dedupe_siblings(files)
}

fn build_rollout_index(files: &[PathBuf]) -> RolloutIndex {
    let mut index = RolloutIndex::new();
    for path in files {
        if let Some(thread_id) = thread_id_from_filename(path) {
            index.entry(thread_id).or_default().push(path.clone());
        }
        // paginated / resume 的续写页（`<threadId>_<rolloutId>`）同时登记在线程
        // 本体 UUID 下：子代理的 forked_from_id 指向线程本体，解析父时间线时
        // 必须能看到首页之后的分段。
        if let Some(thread_id) = leading_thread_id_from_filename(path) {
            index.entry(thread_id).or_default().push(path.clone());
        }
    }
    for paths in index.values_mut() {
        paths.sort();
        paths.dedup();
    }
    index
}

/// 递归扫描目录下的 .jsonl 文件（限制最大深度）
fn collect_jsonl_recursive(dir: &Path, files: &mut Vec<PathBuf>, depth: u32, max_depth: u32) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            collect_jsonl_recursive(&path, files, depth + 1, max_depth);
        } else if codex_rollout_file::is_rollout_file(&path) {
            files.push(path);
        }
    }
}

/// 为翻页续写页解析其上一分段的截止累计高水位基线。
///
/// 当续写页中的 `token_count` 缺少有效的 `last_token_usage` 时，代码依赖
/// `compute_delta(&total_high_water, total)` 计算增量。若续写页不恢复前序分段高水位，
/// 首条记录会误将整段历史累计值作为增量计费。通过当前逻辑线程的候选文件列表（按时间排序）
/// 或 `history_base_id`，定位到紧邻前序分段并提取其最终高水位；若前序分段缺失或无法读取，
/// 必须返回错误以触发防御性暂缓导入（deferred），防止重复记账。
fn resolve_continuation_baseline(
    file_path: &Path,
    meta_thread_id: &str,
    history_base_id: Option<&str>,
    rollout_index: Option<&RolloutIndex>,
) -> Result<Option<CumulativeTokens>, String> {
    let Some(rollout_index) = rollout_index else {
        return Err("缺少 rollout_index 无法解析续写页基线".to_string());
    };

    let current_file_name = file_path.file_name();

    // 1. 优先在当前逻辑线程关联的候选文件中，按文件名时间自然序倒序寻找早于当前分段的前序页
    if let Some(candidates) = rollout_index.get(meta_thread_id) {
        for prev_path in candidates.iter().rev() {
            if prev_path.file_name() < current_file_name {
                let timeline = load_parent_timeline(prev_path).map_err(|e| {
                    format!(
                        "无法读取续写页前序分段 {}: {}",
                        prev_path.display(),
                        e.reason
                    )
                })?;
                if let Some(hw) = timeline.high_water.clone() {
                    return Ok(Some(hw));
                }
            }
        }
    }

    // 2. 若当前线程候选列表未找到且 history_base_id 指向独立 UUID，
    // 尝试从该独立候选列表中查找早于当前分段的文件
    if let Some(hb_id) = history_base_id {
        if hb_id != meta_thread_id {
            if let Some(candidates) = rollout_index.get(hb_id) {
                for prev_path in candidates.iter().rev() {
                    if prev_path.file_name() < current_file_name {
                        let timeline = load_parent_timeline(prev_path).map_err(|e| {
                            format!(
                                "无法读取续写页前序分段 {}: {}",
                                prev_path.display(),
                                e.reason
                            )
                        })?;
                        if let Some(hw) = timeline.high_water.clone() {
                            return Ok(Some(hw));
                        }
                    }
                }
            }
        }
    }

    if history_base_id.is_some() {
        return Err(format!(
            "找不到续写页 history_base 对应的前序分段文件 (meta: {meta_thread_id})"
        ));
    }

    Ok(None)
}

#[cfg(test)]
fn parse_codex_file(
    file_path: &Path,
    root_thread_id: Option<String>,
) -> Result<ParsedCodexFile, AppError> {
    parse_codex_file_with_index(file_path, root_thread_id, None)
}

fn parse_codex_file_with_index(
    file_path: &Path,
    root_thread_id: Option<String>,
    rollout_index: Option<&RolloutIndex>,
) -> Result<ParsedCodexFile, AppError> {
    #[cfg(test)]
    tests::record_codex_parse(file_path);
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    // 压缩文件（`.jsonl.zst`）的 observed_bytes 记磁盘上的压缩大小：
    // 它只用来和下轮的文件大小比对，记解压后长度会让每轮都重新解压。
    let compressed_size = if codex_rollout_file::is_compressed(file_path) {
        Some(
            file.metadata()
                .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?
                .len(),
        )
    } else {
        None
    };
    let mut reader = codex_rollout_file::reader_from_file(file_path, file)
        .map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let mut root_meta_seen = false;
    let mut bounded_history = None;
    let mut starts_at_fork_ordinal = false;
    let mut root_timestamp = None;
    let mut meta_thread_id = None;
    let mut parent = ParentResolution::None;
    let mut current_model = "unknown".to_string();
    // `total_token_usage` is session-cumulative, including across model and
    // rate-limit bucket changes. Divergent snapshots are handled by preferring
    // exact `last_token_usage`, not by splitting the cumulative baseline.
    let mut total_high_water = None;
    // Rate-limit refreshes can re-emit unchanged token info under another
    // `limit_id`. Same-source repeats are identified by that source's latest
    // full snapshot; cross-source repeats must match the immediately preceding
    // token event. Do not compare against other sources' older snapshots:
    // those stale signatures can legitimately recur after a counter reset.
    let mut last_signature_by_source: HashMap<Option<String>, TokenUsageSignature> = HashMap::new();
    let mut previous_token_signature = None;
    let mut is_continuation = false;
    let mut history_base_thread_id = None;
    let mut event_index = 0u32;
    let mut token_events = Vec::new();
    let mut usage_record_events = Vec::new();
    let mut pending_usage: Option<PendingUsageRecord> = None;
    let mut line_offset = 0i64;
    let mut observed_bytes = 0i64;
    let mut has_billable_tokens = false;
    let mut timer = RequestTimer::default();
    let mut bytes = Vec::new();

    loop {
        bytes.clear();
        let read = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|e| AppError::Config(format!("无法读取 Codex 日志: {e}")))?;
        // Count the incomplete suffix too, so an unchanged crashed/closed
        // rollout is skipped rather than fully reparsed on every sync pass.
        observed_bytes += read as i64;
        // A live writer may have only written part of the final JSON record.
        // Leave its line cursor unconsumed for the next file change, but retain
        // support for a complete final JSON record without a newline.
        // Validate it without allocating a Value tree for a potentially huge body.
        if read == 0
            || (bytes.last() != Some(&b'\n')
                && serde_json::from_slice::<IgnoredJsonValue>(&bytes).is_err())
        {
            break;
        }
        line_offset += 1;
        let line = match std::str::from_utf8(&bytes) {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        timer.observe_line(line);

        if !is_usage_line(line) {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(event_type) = value.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };

        match event_type {
            "session_meta" if !root_meta_seen => {
                root_meta_seen = true;
                root_timestamp = parse_timestamp(value.get("timestamp"));
                let payload = value.get("payload").unwrap_or(&serde_json::Value::Null);
                parent = explicit_parent_from_meta(payload);
                bounded_history = payload.get("history_base").cloned();
                starts_at_fork_ordinal = payload
                    .get("forked_from_ordinal_exclusive")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|boundary| {
                        boundary > 0
                            && value.get("ordinal").and_then(serde_json::Value::as_u64)
                                == Some(boundary)
                    });

                meta_thread_id = non_empty_string(
                    payload
                        .get("id")
                        .or_else(|| payload.get("thread_id"))
                        .or_else(|| payload.get("threadId")),
                )
                .map(|id| {
                    uuid::Uuid::parse_str(&id)
                        .map(|value| value.hyphenated().to_string())
                        .unwrap_or(id)
                });
                let leading_id = leading_thread_id_from_filename(file_path);
                if let (Some(filename_id), Some(meta_id)) =
                    (&root_thread_id, meta_thread_id.as_ref())
                {
                    let matches =
                        filename_id == meta_id || leading_id.as_deref() == Some(meta_id.as_str());
                    if !matches {
                        parent = ParentResolution::Deferred(format!(
                            "文件名线程 ID ({filename_id}) 与 root meta ID ({meta_id}) 不一致"
                        ));
                    }
                }

                // 分支线程翻页产生的续写页（`<branchId>_<pageId>`）以及带有指向本线程
                // history_base 的延续分段，其 session_meta 仍保留所属分支会话的
                // forked_from_id / parent_thread_id；但续写页是在本线程之后实时追加的，
                // 记录的是翻页后的新事件，新格式续写页不复制父会话用量。如果将其作为
                // 新 fork 处理，其 session_meta 时间戳是翻页时刻（晚于原始父线程末尾），
                // 会因父线程无法达到 child cutoff 而被永久 deferred。
                // 识别为本线程延续后，将其标为 Continuation，不剔除父重放（无需对齐父时间线）。
                history_base_thread_id = non_empty_string(
                    payload
                        .get("history_base")
                        .and_then(|hb| hb.get("thread_id")),
                )
                .map(|id| {
                    uuid::Uuid::parse_str(&id)
                        .map(|value| value.hyphenated().to_string())
                        .unwrap_or(id)
                });
                let is_paginated = payload
                    .get("history_mode")
                    .and_then(serde_json::Value::as_str)
                    == Some("paginated");
                let has_history_base = history_base_thread_id.is_some();
                let has_parent = matches!(parent, ParentResolution::Parent(_));
                is_continuation = match (leading_id.as_deref(), meta_thread_id.as_deref()) {
                    (Some(leading), Some(meta)) if leading == meta => {
                        is_paginated || has_history_base || has_parent
                    }
                    _ => {
                        has_history_base
                            && history_base_thread_id.as_deref() == meta_thread_id.as_deref()
                    }
                };

                if is_continuation {
                    let ancestor_parent_id = match &parent {
                        ParentResolution::Parent(p) => Some(p.clone()),
                        _ => None,
                    };
                    parent = ParentResolution::Continuation {
                        ancestor_parent_id,
                        history_base_id: history_base_thread_id.clone(),
                    };
                }

                if let ParentResolution::Parent(parent_id) = &mut parent {
                    match uuid::Uuid::parse_str(parent_id) {
                        Ok(value) => *parent_id = value.hyphenated().to_string(),
                        Err(_) => {
                            parent = ParentResolution::Deferred(format!(
                                "显式 parent_thread_id 不是有效 UUID: {parent_id}"
                            ));
                        }
                    }
                }
                if matches!((&root_thread_id, &parent), (Some(root), ParentResolution::Parent(parent_id)) if root == parent_id)
                {
                    parent = ParentResolution::Deferred(
                        "parent_thread_id 与 root_thread_id 相同".to_string(),
                    );
                }
            }
            "turn_context" => {
                if let Some(payload) = value.get("payload") {
                    if let Some(model) = payload
                        .get("model")
                        .or_else(|| payload.get("info").and_then(|info| info.get("model")))
                        .and_then(serde_json::Value::as_str)
                    {
                        current_model = normalize_codex_model(model);
                    }
                }
            }
            "event_msg" => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(serde_json::Value::as_str)
                    == Some("thread_settings_applied")
                {
                    // Resumed/Desktop threads may change models without another
                    // turn_context. Only update attribution: the cumulative token
                    // baseline and duplicate snapshots still belong to the session.
                    if let Some(model) = payload
                        .get("thread_settings")
                        .and_then(|settings| settings.get("model"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|model| !model.is_empty())
                    {
                        current_model = normalize_codex_model(model);
                    }
                    continue;
                }
                if payload.get("type").and_then(serde_json::Value::as_str) != Some("token_count") {
                    continue;
                }
                let Some(info) = payload.get("info").filter(|info| !info.is_null()) else {
                    continue;
                };
                let Some(signature) = parse_token_signature(info) else {
                    continue;
                };

                if let Some(model) = info
                    .get("model")
                    .or_else(|| info.get("model_name"))
                    .or_else(|| payload.get("model"))
                    .and_then(serde_json::Value::as_str)
                {
                    current_model = normalize_codex_model(model);
                }

                let snapshot_source = token_snapshot_source(payload);
                let total = info
                    .get("total_token_usage")
                    .and_then(parse_cumulative_tokens);
                let last = info
                    .get("last_token_usage")
                    .and_then(parse_cumulative_tokens);
                if let Some(pending) = pending_usage.take() {
                    // 对得上的 last 已经由这条 token_count 入账。last 为 0 是远程压缩
                    // 把本次用量只写在 token_usage_record 里之后的空快照。其它对不上的
                    // 记录只用于耗时，不能再记一行。
                    let covered = last
                        .as_ref()
                        .is_some_and(|last| usage_matches(last, &pending.usage));
                    if !covered && last.as_ref().is_some_and(usage_is_zero) {
                        push_unmatched_usage(
                            &mut usage_record_events,
                            pending,
                            line_offset,
                            &mut has_billable_tokens,
                        );
                    }
                }
                if total.is_none() && last.is_none() {
                    continue;
                }
                let has_total_snapshot = total.is_some();
                let duplicate_snapshot = has_total_snapshot
                    && (last_signature_by_source.get(&snapshot_source) == Some(&signature)
                        || previous_token_signature.as_ref() == Some(&signature));
                if has_total_snapshot {
                    last_signature_by_source.insert(snapshot_source, signature.clone());
                }
                previous_token_signature = Some(signature.clone());

                let request_usage = last.clone();
                let delta = if duplicate_snapshot {
                    DeltaTokens {
                        input: 0,
                        cached_input: 0,
                        cache_write_input: 0,
                        output: 0,
                    }
                } else if let Some(last) = last {
                    // Codex provides the exact per-request usage. Prefer it to
                    // subtracting cumulative snapshots, which may come from
                    // multiple independently advancing rate-limit lanes.
                    DeltaTokens {
                        input: last.input as u32,
                        cached_input: last.cached_input as u32,
                        cache_write_input: last.cache_write_input as u32,
                        output: last.output as u32,
                    }
                } else if let Some(total) = total.as_ref() {
                    if is_continuation && total_high_water.is_none() {
                        let meta_id = meta_thread_id.as_deref().unwrap_or_default();
                        match resolve_continuation_baseline(
                            file_path,
                            meta_id,
                            history_base_thread_id.as_deref(),
                            rollout_index,
                        ) {
                            Ok(Some(baseline)) => {
                                if total.input < baseline.input {
                                    // 续写页计数器重置（从 0 或较小值起算），以当前快照为首轮基线
                                    total_high_water = None;
                                } else {
                                    // 继承上一分段截止高水位基线
                                    total_high_water = Some(baseline);
                                }
                            }
                            Ok(None) => {
                                total_high_water = None;
                            }
                            Err(err) => {
                                // 防御机制：前序分段无法读取或基线无法可靠恢复，必须暂缓导入
                                parent = ParentResolution::Deferred(format!(
                                    "续写页缺少有效的前序累计用量基线: {err}"
                                ));
                                has_billable_tokens = true;
                                break;
                            }
                        }
                    }
                    compute_delta(&total_high_water, total)
                } else {
                    continue;
                };
                if let Some(total) = total {
                    if let Some(high_water) = total_high_water.as_mut() {
                        update_high_water(high_water, &total);
                    } else {
                        total_high_water = Some(total);
                    }
                }
                // Cache reads and writes are disjoint parts of input_tokens.
                let cached_input = delta.cached_input.min(delta.input);
                let delta = DeltaTokens {
                    cached_input,
                    cache_write_input: delta.cache_write_input.min(delta.input - cached_input),
                    ..delta
                };
                let nonzero_index = if delta.is_zero() {
                    None
                } else {
                    has_billable_tokens = true;
                    event_index = event_index.saturating_add(1);
                    Some(event_index)
                };

                let timestamp = value
                    .get("timestamp")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                // 重复快照（限额刷新时重发的）不是一次请求，不参与计时
                let timing = nonzero_index.and_then(|_| {
                    timer.finish_request(
                        timestamp.as_deref().and_then(parse_timestamp_millis),
                        request_usage.as_ref(),
                    )
                });

                token_events.push(ParsedTokenEvent {
                    line_offset,
                    signature,
                    delta,
                    event_index: nonzero_index,
                    request_suffix: None,
                    model: current_model.clone(),
                    timestamp,
                    timing,
                });
            }
            "token_usage_record" => {
                pending_usage = None;
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                let Some(usage) = payload.get("usage").and_then(parse_cumulative_tokens) else {
                    continue;
                };
                if usage_is_zero(&usage) {
                    continue;
                }
                let response_id = payload
                    .get("response_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let key = if response_id.is_empty() {
                    format!("line{line_offset}")
                } else {
                    response_id.to_string()
                };
                pending_usage = Some(PendingUsageRecord {
                    key,
                    usage,
                    timestamp: value
                        .get("timestamp")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    model: current_model.clone(),
                });
            }
            _ => {}
        }
    }

    Ok(ParsedCodexFile {
        root_thread_id,
        meta_thread_id,
        root_meta_seen,
        bounded_history,
        starts_at_fork_ordinal,
        root_timestamp,
        parent,
        token_events,
        usage_record_events,
        line_offset,
        observed_bytes: compressed_size
            .and_then(|size| i64::try_from(size).ok())
            .unwrap_or(observed_bytes),
        has_billable_tokens,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct ParentSignatureError {
    reason: String,
    cacheable: bool,
}

impl ParentSignatureError {
    fn content(reason: String) -> Self {
        Self {
            reason,
            cacheable: true,
        }
    }

    fn transient(reason: String) -> Self {
        Self {
            reason,
            cacheable: false,
        }
    }
}

#[cfg(test)]
fn parent_signatures_before(
    parent_path: &Path,
    cutoff: DateTime<Utc>,
) -> Result<Vec<TokenUsageSignature>, ParentSignatureError> {
    load_parent_timeline(parent_path)?
        .signatures_before(parent_path, cutoff)
        .map_err(ParentSignatureError::content)
}

#[derive(Default)]
struct ParentRecord {
    timestamp: Option<serde_json::Value>,
    event_type: Option<serde_json::Value>,
    payload: Option<ParentPayload>,
}

#[derive(Default)]
struct ParentPayload {
    event_type: Option<serde_json::Value>,
    info: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ParentField {
    Timestamp,
    #[serde(rename = "type")]
    EventType,
    Payload,
    Info,
    #[serde(other)]
    Other,
}

// Deserialize ignored values normally so invalid Unicode escapes, numeric
// overflow, and excessive nesting retain Value's validation without its allocations.
struct IgnoredJsonValue;

impl<'de> Deserialize<'de> for IgnoredJsonValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ObjectOrIgnored(Self))
    }
}

// Preserve Value::get's permissive handling of non-object records/payloads.
// Validate and discard them in one pass instead of rebuilding a full Value.
struct ObjectOrIgnored<T>(T);

impl<'de, T: serde::de::Visitor<'de, Value = T>> serde::de::Visitor<'de> for ObjectOrIgnored<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<T, E> {
        Ok(self.0)
    }

    fn visit_bool<E>(self, _: bool) -> Result<T, E> {
        Ok(self.0)
    }

    fn visit_i64<E>(self, _: i64) -> Result<T, E> {
        Ok(self.0)
    }

    fn visit_u64<E>(self, _: u64) -> Result<T, E> {
        Ok(self.0)
    }

    fn visit_f64<E>(self, _: f64) -> Result<T, E> {
        Ok(self.0)
    }

    fn visit_str<E>(self, _: &str) -> Result<T, E> {
        Ok(self.0)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<T, A::Error> {
        while sequence.next_element::<IgnoredJsonValue>()?.is_some() {}
        Ok(self.0)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
        self.0.visit_map(map)
    }
}

impl<'de> serde::de::Visitor<'de> for IgnoredJsonValue {
    type Value = Self;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self, A::Error> {
        while map.next_key::<Self>()?.is_some() {
            map.next_value::<Self>()?;
        }
        Ok(self)
    }
}

impl<'de> Deserialize<'de> for ParentRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ObjectOrIgnored(Self::default()))
    }
}

impl<'de> serde::de::Visitor<'de> for ParentRecord {
    type Value = Self;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a rollout record")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(mut self, mut map: A) -> Result<Self, A::Error> {
        while let Some(field) = map.next_key::<ParentField>()? {
            match field {
                ParentField::Timestamp => self.timestamp = map.next_value()?,
                ParentField::EventType => self.event_type = map.next_value()?,
                ParentField::Payload => self.payload = map.next_value()?,
                _ => {
                    map.next_value::<IgnoredJsonValue>()?;
                }
            }
        }
        Ok(self)
    }
}

impl<'de> Deserialize<'de> for ParentPayload {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ObjectOrIgnored(Self::default()))
    }
}

impl<'de> serde::de::Visitor<'de> for ParentPayload {
    type Value = Self;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a rollout payload")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(mut self, mut map: A) -> Result<Self, A::Error> {
        while let Some(field) = map.next_key::<ParentField>()? {
            match field {
                ParentField::EventType => self.event_type = map.next_value()?,
                ParentField::Info => self.info = map.next_value()?,
                _ => {
                    map.next_value::<IgnoredJsonValue>()?;
                }
            }
        }
        Ok(self)
    }
}

fn parse_parent_record(line: &str) -> Result<ParentRecord, serde_json::Error> {
    serde_json::from_str(line)
}

/// 读取（或命中缓存）父 rollout 单个文件的完整 token 时间线。
fn load_parent_timeline(
    parent_path: &Path,
) -> Result<Arc<ParentTokenTimeline>, ParentSignatureError> {
    let file = fs::File::open(parent_path).map_err(|error| {
        ParentSignatureError::transient(format!(
            "无法打开父 rollout {}: {error}",
            parent_path.display()
        ))
    })?;
    let stamp = ParentFileStamp::from_file(&file);
    let cached_timeline = stamp.and_then(|stamp| {
        replay_caches().lock().ok().and_then(|caches| {
            caches
                .parent_timelines
                .get(parent_path)
                .filter(|entry| entry.stamp == stamp)
                .map(|entry| Arc::clone(&entry.timeline))
        })
    });
    if let Some(timeline) = cached_timeline {
        return Ok(timeline);
    }

    let mut events = Vec::new();
    let mut max_timestamp: Option<DateTime<Utc>> = None;
    let mut has_token_without_timestamp = false;
    let mut high_water: Option<CumulativeTokens> = None;

    // 必须扫描完整父文件，不能在首个未来时间戳处 break：rollout 写入顺序
    // 不承诺时间戳严格单调。缓存完整时间线后，不同 child cutoff 只需内存过滤。
    let mut reader = codex_rollout_file::reader_from_file(parent_path, file).map_err(|error| {
        ParentSignatureError::transient(format!(
            "无法打开父 rollout {}: {error}",
            parent_path.display()
        ))
    })?;
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        // I/O/decompression failures are not evidence of stable content. Do
        // not cache a partial timeline or suppress its next resolution attempt.
        let read = reader.read_until(b'\n', &mut bytes).map_err(|error| {
            ParentSignatureError::transient(format!(
                "无法读取父 rollout {}: {error}",
                parent_path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        // Decode separately from reading: malformed UTF-8 is a skipped record,
        // just like malformed JSON, rather than a retryable reader failure.
        let Ok(line) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let Ok(record) = parse_parent_record(line) else {
            continue;
        };
        let timestamp = parse_timestamp(record.timestamp.as_ref());
        if let Some(timestamp) = timestamp {
            max_timestamp = Some(max_timestamp.map_or(timestamp, |current| current.max(timestamp)));
        }
        if record
            .event_type
            .as_ref()
            .and_then(serde_json::Value::as_str)
            != Some("event_msg")
            || record
                .payload
                .as_ref()
                .and_then(|payload| payload.event_type.as_ref())
                .and_then(serde_json::Value::as_str)
                != Some("token_count")
        {
            continue;
        }
        let Some(info) = record
            .payload
            .as_ref()
            .and_then(|payload| payload.info.as_ref())
            .filter(|info| !info.is_null())
        else {
            continue;
        };
        let Some(signature) = parse_token_signature(info) else {
            continue;
        };
        let Some(timestamp) = timestamp else {
            has_token_without_timestamp = true;
            continue;
        };
        events.push(TimestampedTokenSignature {
            timestamp,
            signature,
        });

        if let Some(total) = info
            .get("total_token_usage")
            .and_then(parse_cumulative_tokens)
        {
            if let Some(hw) = high_water.as_mut() {
                update_high_water(hw, &total);
            } else {
                high_water = Some(total);
            }
        }
    }

    let timeline = Arc::new(ParentTokenTimeline {
        events,
        max_timestamp,
        has_token_without_timestamp,
        high_water,
    });
    if let (Some(stamp), Ok(mut caches)) = (stamp, replay_caches().lock()) {
        caches.parent_timelines.insert(
            parent_path.to_path_buf(),
            CachedParentTimeline {
                stamp,
                timeline: Arc::clone(&timeline),
            },
        );
    }
    Ok(timeline)
}

/// 父线程的一个分段（首页或某个续写页），可能有多份内容一致的副本。
struct ParentSegment {
    rollout_id: Option<String>,
    timeline: Arc<ParentTokenTimeline>,
    /// 各副本中最早的末尾时间。
    max_timestamp: Option<DateTime<Utc>>,
}

fn resolve_parent_signatures(
    parent_id: &str,
    cutoff: DateTime<Utc>,
    rollout_index: &RolloutIndex,
) -> Result<Vec<TokenUsageSignature>, ParentSignatureError> {
    let Some(candidates) = rollout_index.get(parent_id) else {
        return Err(ParentSignatureError::transient(format!(
            "找不到父 rollout: {parent_id}"
        )));
    };

    // 同一线程 UUID 可能对应多个文件：同一 rollout 的副本（如 sessions/ 与
    // archived_sessions/ 各一份）必须内容一致；paginated / resume 的续写页
    // （`<parent_id>_<rolloutId>`）则是该线程的后续分段——翻页后首页不再增长，
    // 必须按文件名时间前缀顺序拼成一条时间线，再判断是否已写到 child fork 时刻。
    let mut ordered = candidates.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let mut merged = ParentTokenTimeline {
        events: Vec::new(),
        max_timestamp: None,
        has_token_without_timestamp: false,
        high_water: None,
    };
    let mut segments: Vec<ParentSegment> = Vec::new();
    let mut latest_path: Option<&Path> = None;
    for candidate in ordered {
        let timeline = load_parent_timeline(candidate)?;
        // 无论是否为重复副本，只要任一候选存在无时间戳错误标志，必须合入以保留防御校验
        merged.has_token_without_timestamp |= timeline.has_token_without_timestamp;
        let rollout_id = thread_id_from_filename(candidate);
        if let Some(segment) = segments
            .iter_mut()
            .find(|segment| segment.rollout_id == rollout_id)
        {
            if segment.timeline.signatures_at(cutoff) != timeline.signatures_at(cutoff) {
                return Err(ParentSignatureError::content(format!(
                    "父 rollout UUID {parent_id} 对应多个内容不一致的文件"
                )));
            }
            // 同一 rollout 的每份副本都必须写到 fork 时刻：该段取各副本中最早的
            // 末尾时间，结果不随副本顺序变化。
            segment.max_timestamp = match (segment.max_timestamp, timeline.max_timestamp) {
                (Some(current), Some(other)) => Some(current.min(other)),
                _ => None,
            };
            continue;
        }
        merged.events.extend(timeline.events.iter().cloned());
        segments.push(ParentSegment {
            rollout_id,
            max_timestamp: timeline.max_timestamp,
            timeline,
        });
        latest_path = Some(candidate.as_path());
    }
    merged.max_timestamp = segments
        .iter()
        .filter_map(|segment| segment.max_timestamp)
        .max();
    let Some(latest_path) = latest_path else {
        return Err(ParentSignatureError::transient(format!(
            "找不到父 rollout: {parent_id}"
        )));
    };
    merged
        .signatures_before(latest_path, cutoff)
        .map_err(ParentSignatureError::content)
}

/// Metadata-only dependency snapshot; opening a file checks accessibility and
/// gives the same identity stamp used by the parent timeline cache. Include all
/// candidates so archive moves, replacements and duplicate candidates retry.
/// This must be called without holding the replay-cache mutex.
fn parent_dependency_stamp(
    parent_id: &str,
    rollout_index: &RolloutIndex,
) -> Option<Vec<(PathBuf, ParentFileStamp)>> {
    let candidates = rollout_index.get(parent_id)?;
    let mut files = Vec::with_capacity(candidates.len());
    for path in candidates {
        let file = fs::File::open(path).ok()?;
        files.push((path.clone(), ParentFileStamp::from_file(&file)?));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Some(files)
}

fn matching_replay_prefix(child: &[ParsedTokenEvent], parent: &[TokenUsageSignature]) -> usize {
    let mut parent_offset = 0usize;
    let mut matched = 0usize;
    for event in child {
        let Some(relative_match) = parent[parent_offset..]
            .iter()
            .position(|signature| signature == &event.signature)
        else {
            break;
        };
        parent_offset += relative_match + 1;
        matched += 1;
    }
    matched
}

fn mark_deferred(
    file_path: &Path,
    modified: i64,
    size: u64,
    reason: PendingReason,
) -> CodexFileSyncResult {
    let entry = PendingEntry {
        modified,
        size,
        reason,
    };
    let should_warn = replay_caches()
        .lock()
        .ok()
        .and_then(|mut caches| {
            caches
                .pending
                .insert(file_path.to_path_buf(), entry.clone())
        })
        .as_ref()
        != Some(&entry);
    if should_warn {
        let reason = match &entry.reason {
            PendingReason::MissingParent(parent) => format!("找不到父 rollout {parent}"),
            PendingReason::Stable(reason) | PendingReason::Retryable { reason, .. } => {
                reason.clone()
            }
        };
        log::warn!("[CODEX-SYNC] deferred {}: {reason}", file_path.display());
    }
    CodexFileSyncResult {
        deferred: true,
        ..CodexFileSyncResult::default()
    }
}

/// 单文件批量插入的事务粒度。批内 UI 查询会被连接互斥锁挡住约几毫秒，
/// 批间释放锁让读侧插队——兼顾吞吐（避免逐行 autocommit 的每行 fsync）
/// 与大文件重导期间面板的响应性。
const CODEX_INSERT_BATCH_SIZE: usize = 1000;

fn update_codex_sync_state_on_conn(
    conn: &rusqlite::Connection,
    file_path: &str,
    modified: i64,
    parsed: &ParsedCodexFile,
) -> Result<(), AppError> {
    update_sync_state_on_conn(conn, file_path, modified, parsed.line_offset)?;
    conn.execute(
        "UPDATE session_log_sync SET last_byte_offset = ?1 WHERE file_path = ?2",
        rusqlite::params![parsed.observed_bytes, file_path],
    )?;
    Ok(())
}

fn update_codex_sync_state(
    db: &Database,
    file_path: &str,
    modified: i64,
    parsed: &ParsedCodexFile,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    let tx = conn.unchecked_transaction()?;
    update_codex_sync_state_on_conn(&tx, file_path, modified, parsed)?;
    tx.commit()?;
    Ok(())
}

/// Every billable event is the child's own request: exact `last_token_usage`
/// and a timestamp after the fork. Copied parent history fails the time check.
fn child_events_after_fork(parsed: &ParsedCodexFile) -> Option<()> {
    let cutoff = parsed.root_timestamp?;
    for event in parsed
        .token_events
        .iter()
        .chain(&parsed.usage_record_events)
    {
        let last = event.signature.last.as_ref()?;
        if last.input.is_none()
            || last.output.is_none()
            || DateTime::parse_from_rfc3339(event.timestamp.as_deref()?)
                .ok()?
                .with_timezone(&Utc)
                <= cutoff
        {
            return None;
        }
    }
    Some(())
}

// Codex 0.167+ forks/resumes of a finished thread number the child's records
// from the parent's cutoff (`ordinal == forked_from_ordinal_exclusive`) and do
// not copy the parent history, so there is nothing to replay. Accept them
// without waiting for the parent to reach the fork time, which an idle parent
// never does.
fn verified_ordinal_fork(parsed: &ParsedCodexFile) -> bool {
    matches!(parsed.parent, ParentResolution::Parent(_))
        && parsed.starts_at_fork_ordinal
        && !parsed.token_events.is_empty()
        && child_events_after_fork(parsed).is_some()
}

// A bounded-reference fork stores only its own requests. Accept this narrow case
// only with a complete physical prefix and an exact inherited counter baseline.
// Otherwise retain the existing replay/deferred path. Never rewrite rollout data.
fn verified_bounded_fork(parsed: &ParsedCodexFile, index: &RolloutIndex) -> bool {
    use std::io::Read;

    (|| {
        let ParentResolution::Parent(parent_id) = &parsed.parent else {
            return None;
        };
        let hb = parsed.bounded_history.as_ref()?;
        if hb.get("thread_id")?.as_str()? != parent_id {
            return None;
        }
        let end = hb.get("end_byte_offset")?.as_u64()?;
        let count = hb.get("end_ordinal_exclusive")?.as_u64()?;
        if end == 0 || end > 64 * 1024 * 1024 || count == 0 {
            return None;
        }
        let first = parsed.token_events.first()?;
        // Reject copied/replayed history, cumulative-only accounting and invalid timestamps.
        // Fallback usage records also belong to this child only when their own
        // timestamps are after the fork. Keep their native response-based IDs.
        child_events_after_fork(parsed)?;
        let total = first.signature.total.as_ref()?;
        let last = first.signature.last.as_ref()?;
        let baseline = TokenCountersSignature {
            input: Some(total.input?.checked_sub(last.input?)?),
            cached_input: Some(total.cached_input?.checked_sub(last.cached_input?)?),
            output: Some(total.output?.checked_sub(last.output?)?),
            reasoning_output: Some(
                total
                    .reasoning_output?
                    .checked_sub(last.reasoning_output?)?,
            ),
            total: Some(total.total?.checked_sub(last.total?)?),
        };
        let mut found = false;
        for path in index.get(parent_id)? {
            // The reference identifies one physical rollout, not all logical pages.
            if thread_id_from_filename(path).as_deref() != Some(parent_id) {
                continue;
            }
            let file = fs::File::open(path).ok()?;
            let stamp = ParentFileStamp::from_file(&file)?;
            let mut reader = codex_rollout_file::reader_from_file(path, file).ok()?;
            let mut used = 0u64;
            let mut ordinal = 0u64;
            let mut first_record = true;
            let mut parent_total = None;
            while used < end {
                let mut bytes = Vec::new();
                let n = reader
                    .by_ref()
                    .take(end - used)
                    .read_until(b'\n', &mut bytes)
                    .ok()?;
                if n == 0 || bytes.last() != Some(&b'\n') {
                    return None;
                }
                used += n as u64;
                let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                if first_record {
                    ordinal = value.get("ordinal")?.as_u64()?;
                    let inherited = value
                        .pointer("/payload/history_base/end_ordinal_exclusive")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    if ordinal != inherited
                        || value.get("type")?.as_str()? != "session_meta"
                        || value.pointer("/payload/id")?.as_str()? != parent_id
                    {
                        return None;
                    }
                    first_record = false;
                }
                if value.get("ordinal")?.as_u64()? != ordinal {
                    return None;
                }
                ordinal = ordinal.checked_add(1)?;
                if value.get("type").and_then(|v| v.as_str()) == Some("event_msg")
                    && value.pointer("/payload/type").and_then(|v| v.as_str())
                        == Some("token_count")
                {
                    if let Some(v) = value.pointer("/payload/info/total_token_usage") {
                        parent_total = parse_signature_counters(Some(v));
                    }
                }
            }
            let after = fs::File::open(path).ok()?;
            if ParentFileStamp::from_file(&after)? != stamp {
                return None;
            }
            if used != end || ordinal != count || parent_total.as_ref() != Some(&baseline) {
                return None;
            }
            found = true;
        }
        found.then_some(())
    })()
    .is_some()
}

/// 同步单个 Codex JSONL 文件。
fn sync_single_codex_file(
    db: &Database,
    file_path: &Path,
    rollout_index: &RolloutIndex,
    pass: &mut CodexSyncPass,
) -> Result<CodexFileSyncResult, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let file_size = metadata.len();

    // 检查同步状态
    let (last_modified, last_offset) = get_codex_sync_state(db, file_path, pass)?;

    // Windows may keep mtime unchanged while Codex holds its write handle open.
    // Legacy cursors have no byte offset: rescan once to catch up and persist it.
    let last_byte_offset = pass
        .cursors
        .get(&file_path_str)
        .and_then(|c| c.last_byte_offset);
    if file_modified == last_modified && last_byte_offset == i64::try_from(file_size).ok() {
        return Ok(CodexFileSyncResult::default());
    }

    // Capture child identity before parsing as well as before checking a
    // pending entry: replacing a deferred child can preserve mtime and size.
    // Unknown/inaccessible identity must not suppress the parser's retry/error.
    let child_file = fs::File::open(file_path)
        .ok()
        .and_then(|file| ParentFileStamp::from_file(&file));
    let pending = replay_caches()
        .lock()
        .ok()
        .and_then(|caches| caches.pending.get(file_path).cloned());
    if let Some(pending) = pending {
        if pending.modified == file_modified && pending.size == file_size {
            let unchanged = match &pending.reason {
                PendingReason::MissingParent(parent) => !rollout_index.contains_key(parent),
                PendingReason::Stable(_) => true,
                PendingReason::Retryable {
                    parent_id,
                    child_file: Some(previous_child),
                    parent_files: Some(parent_files),
                    ..
                } => {
                    child_file.as_ref() == Some(previous_child)
                        && parent_dependency_stamp(parent_id, rollout_index).as_ref()
                            == Some(parent_files)
                }
                PendingReason::Retryable { .. } => false,
            };
            if unchanged {
                return Ok(CodexFileSyncResult {
                    deferred: true,
                    ..CodexFileSyncResult::default()
                });
            }
        }
    }

    let parsed = parse_codex_file_with_index(
        file_path,
        thread_id_from_filename(file_path),
        Some(rollout_index),
    )?;
    if !parsed.has_billable_tokens {
        update_codex_sync_state(db, &file_path_str, file_modified, &parsed)?;
        return Ok(CodexFileSyncResult::default());
    }
    let Some(root_thread_id) = parsed.root_thread_id.as_deref() else {
        return Ok(mark_deferred(
            file_path,
            file_modified,
            file_size,
            PendingReason::Stable("文件名缺少有效的尾部 UUID".to_string()),
        ));
    };
    if !parsed.root_meta_seen {
        return Ok(mark_deferred(
            file_path,
            file_modified,
            file_size,
            PendingReason::Stable("含计费 token 但尚无 session_meta".to_string()),
        ));
    }

    let replay_prefix = match &parsed.parent {
        ParentResolution::None | ParentResolution::Continuation { .. } => 0,
        ParentResolution::Deferred(reason) => {
            let pending_reason = if let Some(meta_id) = parsed.meta_thread_id.as_deref() {
                if rollout_index.contains_key(meta_id) {
                    PendingReason::Retryable {
                        reason: reason.clone(),
                        parent_id: meta_id.to_string(),
                        child_file,
                        parent_files: parent_dependency_stamp(meta_id, rollout_index),
                    }
                } else {
                    PendingReason::Stable(reason.clone())
                }
            } else {
                PendingReason::Stable(reason.clone())
            };
            return Ok(mark_deferred(
                file_path,
                file_modified,
                file_size,
                pending_reason,
            ));
        }
        ParentResolution::Parent(_)
            if verified_ordinal_fork(&parsed) || verified_bounded_fork(&parsed, rollout_index) =>
        {
            0
        }
        ParentResolution::Parent(parent_id) => {
            let Some(cutoff) = parsed.root_timestamp else {
                return Ok(mark_deferred(
                    file_path,
                    file_modified,
                    file_size,
                    PendingReason::Stable(
                        "parented rollout 的 root meta 缺少有效 timestamp".to_string(),
                    ),
                ));
            };
            // 先把查询结果拷出来再释放锁：Rust 2021 下 `if let .. else` 的临时值活到
            // else 分支结束，锁中毒时 Err 里仍攥着 guard，else 里再加锁会自锁。
            // None = 锁已中毒（不走缓存），Some(None) = 未命中。
            let cached_prefix = replay_caches().lock().ok().map(|caches| {
                caches
                    .replay_prefixes
                    .get(file_path)
                    .filter(|cached| cached.modified == file_modified && cached.size == file_size)
                    .map(|cached| cached.prefix)
            });
            match cached_prefix {
                Some(Some(prefix)) => prefix,
                Some(None) => {
                    let parent_files = parent_dependency_stamp(parent_id, rollout_index);
                    let parent_signatures =
                        match resolve_parent_signatures(parent_id, cutoff, rollout_index) {
                            Ok(signatures) => signatures,
                            Err(error) => {
                                let pending_reason = if rollout_index.contains_key(parent_id) {
                                    PendingReason::Retryable {
                                        reason: error.reason,
                                        parent_id: parent_id.clone(),
                                        child_file,
                                        parent_files: if error.cacheable {
                                            parent_files
                                        } else {
                                            None
                                        },
                                    }
                                } else {
                                    PendingReason::MissingParent(parent_id.clone())
                                };
                                return Ok(mark_deferred(
                                    file_path,
                                    file_modified,
                                    file_size,
                                    pending_reason,
                                ));
                            }
                        };
                    let prefix = matching_replay_prefix(&parsed.token_events, &parent_signatures);
                    if let Ok(mut caches) = replay_caches().lock() {
                        caches.replay_prefixes.insert(
                            file_path.to_path_buf(),
                            CachedReplayPrefix {
                                modified: file_modified,
                                size: file_size,
                                prefix,
                            },
                        );
                    }
                    prefix
                }
                None => {
                    let parent_signatures =
                        resolve_parent_signatures(parent_id, cutoff, rollout_index)
                            .map_err(|error| AppError::Config(error.reason))?;
                    matching_replay_prefix(&parsed.token_events, &parent_signatures)
                }
            }
        }
    };

    if let Ok(mut caches) = replay_caches().lock() {
        caches.pending.remove(file_path);
    }

    let mut result = CodexFileSyncResult::default();
    let mut to_insert: Vec<(&ParsedTokenEvent, String)> = Vec::new();
    for (token_offset, event) in parsed.token_events.iter().enumerate() {
        let Some(event_index) = event.event_index else {
            continue;
        };
        if token_offset < replay_prefix {
            if event.line_offset > last_offset {
                result.skipped = result.skipped.saturating_add(1);
            }
            continue;
        }
        if event.line_offset <= last_offset {
            continue;
        }
        to_insert.push((event, event_index.to_string()));
    }
    for event in &parsed.usage_record_events {
        let Some(suffix) = event.request_suffix.as_deref() else {
            continue;
        };
        if event.line_offset <= last_offset {
            continue;
        }
        to_insert.push((event, suffix.to_string()));
    }

    // 分批事务写库：逐行 autocommit（journal_mode=delete 下每行一整套
    // journal 建立/fsync/删除）是全量重导的最大耗时项。批内单条插入失败
    // 沿用旧行为跳过该条继续；某批 commit 失败则该批整体回滚且游标不推进，
    // 下一 pass 重扫时由 request_id 主键 + 指纹去重兜底，不会双算。
    //
    // session_id 记 root meta 的线程 ID：双段文件名（thread/revert 的替换
    // rollout）下是前置 UUID，与会话管理器侧的会话身份同口径；尾部 rollout
    // ID 只承担 request_id 去重键（event_index 按物理文件计数，不能改用
    // 前置 ID）。
    let session_thread_id = parsed.meta_thread_id.as_deref().unwrap_or(root_thread_id);
    let batch_count = to_insert.len().div_ceil(CODEX_INSERT_BATCH_SIZE);
    for (batch_index, batch) in to_insert.chunks(CODEX_INSERT_BATCH_SIZE).enumerate() {
        let is_last_batch = batch_index + 1 == batch_count;
        let conn = lock_conn!(db.conn);
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::Database(format!("开启 Codex 会话写入事务失败: {e}")))?;

        let mut batch_imported = 0u32;
        let mut batch_skipped = 0u32;
        let mut batch_suspected = 0u32;
        for (event, suffix) in batch {
            let request_id = format!("{CODEX_THREAD_REQUEST_ID_PREFIX}:{root_thread_id}:{suffix}");
            match insert_codex_session_entry_on_conn(
                &tx,
                &request_id,
                &event.delta,
                &event.model,
                Some(session_thread_id),
                event.timestamp.as_deref(),
                event.timing,
                &mut batch_suspected,
                &mut pass.pricing,
            ) {
                Ok(true) => batch_imported += 1,
                Ok(false) => batch_skipped += 1,
                Err(e) => {
                    log::warn!("[CODEX-SYNC] 插入失败 ({request_id}): {e}");
                    batch_skipped += 1;
                }
            }
        }
        if is_last_batch {
            // 游标推进与最后一批数据同事务提交：中途崩溃时两者一起回滚，
            // 不会出现"游标已推进但数据缺失"的丢数据窗口。
            update_codex_sync_state_on_conn(&tx, &file_path_str, file_modified, &parsed)?;
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Codex 会话写入事务失败: {e}")))?;

        result.imported = result.imported.saturating_add(batch_imported);
        result.skipped = result.skipped.saturating_add(batch_skipped);
        result.suspected_duplicates = result.suspected_duplicates.saturating_add(batch_suspected);
    }

    if to_insert.is_empty() {
        update_codex_sync_state(db, &file_path_str, file_modified, &parsed)?;
    }
    Ok(result)
}

/// 插入单条 Codex 会话记录到 proxy_request_logs（自取锁的便捷包装，测试专用；
/// 生产路径走 [`insert_codex_session_entry_on_conn`] 以复用批量事务与定价缓存）
#[cfg(test)]
fn insert_codex_session_entry(
    db: &Database,
    request_id: &str,
    delta: &DeltaTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
    suspected_duplicates: &mut u32,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    insert_codex_session_entry_on_conn(
        &conn,
        request_id,
        delta,
        model,
        session_id,
        timestamp,
        None,
        suspected_duplicates,
        &mut HashMap::new(),
    )
}

/// 插入单条 Codex 会话记录到 proxy_request_logs。
///
/// 调用方负责持锁/事务；`pricing_cache` 按原始 model 字符串键控（
/// `find_codex_pricing` 是纯函数式查找，同串必同结果），全量重导时把
/// 每事件一次的定价 SELECT 降为每模型一次。`timing` 是按事件时间戳估出来的
/// 请求耗时和首字时间（见 [`RequestTimer`]），估不出来传 None。
#[allow(clippy::too_many_arguments)]
fn insert_codex_session_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    delta: &DeltaTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
    timing: Option<EstimatedTiming>,
    suspected_duplicates: &mut u32,
    pricing_cache: &mut HashMap<String, Option<ModelPricing>>,
) -> Result<bool, AppError> {
    let created_at = timestamp
        .and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.timestamp())
        })
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    let dedup_key = DedupKey {
        app_type: "codex",
        data_source: "codex_session",
        model,
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: delta.cache_write_input,
        created_at,
    };
    if should_skip_session_insert(conn, request_id, &dedup_key)? {
        return Ok(false);
    }
    if has_suspected_codex_session_duplicate(conn, request_id, &dedup_key)? {
        *suspected_duplicates = suspected_duplicates.saturating_add(1);
        log::warn!(
            "[CODEX-SYNC] 疑似重复会话用量: request_id={request_id}, model={model}, input={}, output={}, cache_read={}",
            delta.input,
            delta.output,
            delta.cached_input
        );
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: delta.cache_write_input,
        cache_creation_1h_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = pricing_cache
        .entry(model.to_string())
        .or_insert_with(|| find_codex_pricing(conn, model));
    let [input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost] =
        cost_columns("codex", &usage, pricing.as_ref(), Decimal::ONE);

    let inserted_rows = conn
        .prepare_cached(
            "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
        )
        .and_then(|mut stmt| stmt.execute(rusqlite::params![
                request_id,
                "_codex_session",    // provider_id
                "codex",             // app_type
                model,
                model,               // request_model = model
                delta.input,
                delta.output,
                delta.cached_input,
                delta.cache_write_input,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                timing.map_or(0, |t| t.latency_ms), // latency_ms: 按时间戳估算，0 = 没有计时
                timing.and_then(|t| t.first_token_ms), // first_token_ms: 按时间戳估算，推不出来为 NULL
                200i64,              // status_code
                Option::<String>::None, // error_message
                session_id.map(|s| s.to_string()),
                Some("codex_session"), // provider_type
                1i64,                // is_streaming
                "1.0",               // cost_multiplier
                created_at,
                "codex_session",     // data_source
                INPUT_TOKEN_SEMANTICS_TOTAL, // input_tokens 含缓存读写
            ]))
        .map_err(|e| AppError::Database(format!("插入 Codex 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

/// 查找 Codex 模型定价（带归一化）
fn find_codex_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, &normalize_codex_model(model_id))
}

#[cfg(test)]
mod tests;
