//! Claude Code 会话日志使用追踪
//!
//! 从 ~/.claude/projects/ 下的 JSONL 会话文件中提取 token 使用数据，
//! 实现无代理模式下的使用统计。
//!
//! ## 数据流
//! ```text
//! ~/.claude/projects/*/*.jsonl → 增量解析 → 去重 → 费用计算 → proxy_request_logs 表
//! ```

use crate::config::get_claude_config_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::usage_stats::{
    effective_usage_log_filter, find_model_pricing, has_matching_proxy_usage_log, DedupKey,
};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;
use std::time::SystemTime;

/// 同步结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncResult {
    pub imported: u32,
    pub skipped: u32,
    pub files_scanned: u32,
    pub suspected_duplicates: u32,
    pub deferred_files: u32,
    pub errors: Vec<String>,
}

impl SessionSyncResult {
    pub fn merge(&mut self, other: SessionSyncResult) {
        self.imported = self.imported.saturating_add(other.imported);
        self.skipped = self.skipped.saturating_add(other.skipped);
        self.files_scanned = self.files_scanned.saturating_add(other.files_scanned);
        self.suspected_duplicates = self
            .suspected_duplicates
            .saturating_add(other.suspected_duplicates);
        self.deferred_files = self.deferred_files.saturating_add(other.deferred_files);
        self.errors.extend(other.errors);
    }
}

pub fn session_sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 最近一次完整扫描（[`sync_all_unlocked`]，后台定时和手动同步都走它）完成的时间，
/// 毫秒时间戳；0 表示本次启动后还没扫过。只在内存里：启动时会立刻扫一轮。
static LAST_SYNC_COMPLETED_AT_MS: AtomicI64 = AtomicI64::new(0);

/// 记下一次扫描完成的时间；只会往后走（并发调用时不会被较早的时间覆盖）。
pub(crate) fn record_sync_completed(at_ms: i64) {
    LAST_SYNC_COMPLETED_AT_MS.fetch_max(at_ms, Ordering::Relaxed);
}

/// 最近一次完整扫描完成的时间（毫秒时间戳）；本次启动后还没扫过时为 `None`。
pub fn last_sync_completed_at() -> Option<i64> {
    let value = LAST_SYNC_COMPLETED_AT_MS.load(Ordering::Relaxed);
    (value > 0).then_some(value)
}

/// session_log_sync 表一行的内存快照。
///
/// 各解析器在一轮扫描开头用 [`load_sync_cursors`] 一次性预取全表，替代
/// 逐文件的单行查询（文件数随历史只增不减，逐文件查询意味着每轮上千次
/// 取锁）。`last_synced_at` 对 Pi 路径是编码后的 revision，其余路径是
/// 真实同步时间戳。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SyncCursor {
    pub last_modified: i64,
    pub last_line_offset: i64,
    pub last_byte_offset: Option<i64>,
    /// 游标边界前尾部字节的指纹（仅 Claude 路径写入），用于识别文件被
    /// 外部重写；NULL 表示无指纹可校验。
    pub last_tail_fingerprint: Option<i64>,
    pub last_synced_at: i64,
}

/// 一次性预取 session_log_sync 全表游标。
///
/// 失败必须中止本轮同步，不能回退空表当新库处理：`rollup_and_prune` 会把
/// 30 天前的明细汇总后删除，request_id 去重只查明细表，对已剪条目失明——
/// 空表回退意味着全量重导，被剪的旧条目会在下次 rollup 时再次累加进汇总，
/// 永久放大统计。
pub(crate) fn load_sync_cursors(db: &Database) -> Result<HashMap<String, SyncCursor>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn
        .prepare(
            "SELECT file_path, last_modified, last_line_offset, last_synced_at, last_byte_offset,
                    last_tail_fingerprint
             FROM session_log_sync",
        )
        .map_err(|e| AppError::Database(format!("预取同步游标失败: {e}")))?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            SyncCursor {
                last_modified: row.get(1)?,
                last_line_offset: row.get(2)?,
                last_synced_at: row.get(3)?,
                last_byte_offset: row.get(4)?,
                last_tail_fingerprint: row.get(5)?,
            },
        ))
    });
    rows.and_then(|rows| rows.collect::<Result<HashMap<_, _>, _>>())
        .map_err(|e| AppError::Database(format!("预取同步游标失败: {e}")))
}

fn merge_sync_step(
    aggregate: &mut SessionSyncResult,
    name: &str,
    step: Result<SessionSyncResult, AppError>,
) {
    match step {
        Ok(result) => aggregate.merge(result),
        Err(error) => aggregate.errors.push(format!("{name} 同步失败: {error}")),
    }
}

/// 调用方必须持有 [`session_sync_mutex`]。此函数是同步内核，供后台任务、
/// 手动同步和 Codex 重建共享，避免 tokio Mutex 重入。
pub fn sync_all_unlocked(db: &Database) -> SessionSyncResult {
    let mut result = SessionSyncResult::default();
    merge_sync_step(&mut result, "Claude", sync_claude_session_logs(db));
    merge_sync_step(
        &mut result,
        "Codex",
        crate::services::session_usage_codex::sync_codex_usage(db),
    );
    merge_sync_step(
        &mut result,
        "Gemini",
        crate::services::session_usage_gemini::sync_gemini_usage(db),
    );
    merge_sync_step(
        &mut result,
        "OpenCode",
        crate::services::session_usage_opencode::sync_opencode_usage(db),
    );
    merge_sync_step(
        &mut result,
        "Grok Build",
        crate::services::session_usage_grokbuild::sync_grokbuild_usage(db),
    );
    merge_sync_step(
        &mut result,
        "Pi",
        crate::services::session_usage_pi::sync_pi_usage(db),
    );
    merge_sync_step(
        &mut result,
        "MCode",
        crate::services::session_usage_mcode::sync_mcode_usage(db),
    );
    notify_sync_result(&result);
    record_sync_completed(chrono::Utc::now().timestamp_millis());
    result
}

pub(crate) fn notify_sync_result(result: &SessionSyncResult) {
    if result.imported > 0 {
        crate::usage_events::notify_log_recorded();
    }
}

/// 数据来源分布
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSourceSummary {
    pub data_source: String,
    pub request_count: u32,
    pub total_cost_usd: String,
}

/// 从 JSONL 中解析出的 assistant 消息使用数据
#[derive(Debug)]
struct ParsedAssistantUsage {
    message_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    stop_reason: Option<String>,
    timestamp: Option<String>,
    session_id: Option<String>,
}

/// 估算出的请求耗时短于这个毫秒数就不要：真实请求不可能这么快，多半是起点取错了。
const MIN_PLAUSIBLE_LATENCY_MS: i64 = 100;

/// 估算出的请求耗时长于这个毫秒数也不要：中间多半夹了休眠或长时间的重试等待。
const MAX_PLAUSIBLE_LATENCY_MS: i64 = 60 * 60 * 1000;

/// 会话日志没有请求计时，耗时只能拿两个时间戳相减估出来（含首字等待）。
/// 落在合理范围之外的返回 None，入库时写 0，表示没有计时。
///
/// Shared by the Claude and Codex session parsers.
pub(crate) fn estimated_latency_ms(start_ms: i64, end_ms: i64) -> Option<i64> {
    let latency = end_ms.checked_sub(start_ms)?;
    (MIN_PLAUSIBLE_LATENCY_MS..=MAX_PLAUSIBLE_LATENCY_MS)
        .contains(&latency)
        .then_some(latency)
}

pub(crate) fn parse_timestamp_millis(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// 一行日志在对话链上的位置，只留估算请求耗时用得到的三样。
struct ChainNode {
    parent: Option<uuid::Uuid>,
    timestamp_ms: Option<i64>,
    /// `attachment` 行的时间戳不可靠（有的是回复开始后才补记的），找请求起点时跳过。
    is_attachment: bool,
}

type ChainNodes = HashMap<uuid::Uuid, ChainNode>;

fn parse_line_uuid(value: &serde_json::Value, key: &str) -> Option<uuid::Uuid> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
}

fn record_chain_node(chain: &mut ChainNodes, value: &serde_json::Value) {
    let Some(uuid) = parse_line_uuid(value, "uuid") else {
        return;
    };
    chain.insert(
        uuid,
        ChainNode {
            parent: parse_line_uuid(value, "parentUuid"),
            timestamp_ms: value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(parse_timestamp_millis),
            is_attachment: value.get("type").and_then(|t| t.as_str()) == Some("attachment"),
        },
    );
}

/// 增量读取时往游标前回看的字节数。回复的第一块常常紧跟在游标后面，而它的
/// 起点行（用户消息、工具结果）在游标前面：工具结果先落盘，回复要等整条
/// 结束才一次写完。起点行比这个窗口还大（比如带图片的工具结果）时，那一条
/// 请求就不估耗时。
const CHAIN_SEED_BYTES: i64 = 256 * 1024;

/// 把游标前一小段里的行记进对话链和回复计时（只取这两样，不导入）。回复的
/// 前几块可能已经被上一轮读走，这一轮要靠它们才知道回复从哪一块开始。返回后
/// 文件位置停在 `offset`。回看读不出来不算错，只是少估几条耗时。
fn read_chain_seed(
    file: &mut fs::File,
    offset: i64,
    chain: &mut ChainNodes,
    timings: &mut HashMap<String, MessageTiming>,
) -> Result<(), AppError> {
    let len = offset.clamp(0, CHAIN_SEED_BYTES);
    if len == 0 {
        return Ok(());
    }
    let mut window = vec![0u8; len as usize];
    let read_ok = file
        .seek(SeekFrom::Start((offset - len) as u64))
        .and_then(|_| file.read_exact(&mut window))
        .is_ok();
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|e| AppError::Config(format!("无法定位文件偏移: {e}")))?;
    if !read_ok {
        return Ok(());
    }

    let mut lines = window.split(|b| *b == b'\n');
    if offset > len {
        // 窗口不是从文件头开始，第一段多半是半行
        lines.next();
    }
    for line in lines {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) {
            record_chain_node(chain, &value);
            if let Some(msg_id) = assistant_message_id(&value) {
                record_block_timing(timings, &value, msg_id);
            }
        }
    }
    Ok(())
}

/// assistant 行的 `message.id`；别的行返回 None。
fn assistant_message_id(value: &serde_json::Value) -> Option<&str> {
    if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    value.get("message")?.get("id")?.as_str()
}

/// 一条回复（同一个 message.id 的所有内容块）的计时线索。
struct MessageTiming {
    /// 文件里最先出现的那一块的父行，请求起点沿它往上找。
    first_parent: Option<uuid::Uuid>,
    /// 最先出现的那一块是不是这条回复的第一块。不是的话（前面的块在回看窗口
    /// 之外）起点会取晚、速度偏高，宁可不估。旧版日志没有 `apiBlockIndex`，按是处理。
    starts_at_first_block: bool,
    /// 各块时间戳（块写完的时刻）里最晚的一个，即回复结束。
    end_ms: Option<i64>,
}

/// 把一个内容块记进它所属回复的计时：最先出现的块定起点，各块里最晚的定结束。
fn record_block_timing(
    timings: &mut HashMap<String, MessageTiming>,
    value: &serde_json::Value,
    msg_id: &str,
) {
    let block_end_ms = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(parse_timestamp_millis);
    let timing = timings
        .entry(msg_id.to_string())
        .or_insert_with(|| MessageTiming {
            first_parent: parse_line_uuid(value, "parentUuid"),
            starts_at_first_block: value
                .get("apiBlockIndex")
                .is_none_or(|index| index.as_u64() == Some(0)),
            end_ms: None,
        });
    timing.end_ms = timing.end_ms.max(block_end_ms);
}

/// 沿父链往上最多走这么多行。
const MAX_CHAIN_HOPS: usize = 32;

/// 请求起点：从回复第一块的父行往上，跳过 `attachment`，第一个别的行（用户
/// 消息、工具结果、出错重试的 system 行）的时间戳。
fn resolve_request_start_ms(chain: &ChainNodes, first_parent: Option<uuid::Uuid>) -> Option<i64> {
    let mut cursor = first_parent?;
    for _ in 0..MAX_CHAIN_HOPS {
        let node = chain.get(&cursor)?;
        if !node.is_attachment {
            return node.timestamp_ms;
        }
        cursor = node.parent?;
    }
    None
}

/// 同步 Claude Code 会话日志到使用统计数据库
pub fn sync_claude_session_logs(db: &Database) -> Result<SessionSyncResult, AppError> {
    let projects_dir = get_claude_config_dir().join("projects");
    if !projects_dir.exists() {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            suspected_duplicates: 0,
            deferred_files: 0,
            errors: vec![],
        });
    }

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    // 收集所有 .jsonl 文件
    let jsonl_files = collect_jsonl_files(&projects_dir);
    let cursors = load_sync_cursors(db)?;

    for file_path in &jsonl_files {
        result.files_scanned += 1;

        let cursor = cursors.get(file_path.to_string_lossy().as_ref());
        match sync_single_file(db, file_path, cursor) {
            Ok(file_sync) => {
                result.imported += file_sync.imported;
                result.skipped += file_sync.skipped;
                if file_sync.incomplete_tail || file_sync.read_error.is_some() {
                    result.deferred_files += 1;
                }
                if let Some(err) = file_sync.read_error {
                    let msg = format!(
                        "{}: 读取中断，已入库部分保留、下轮从断点续读: {err}",
                        file_path.display()
                    );
                    log::warn!("[SESSION-SYNC] {msg}");
                    result.errors.push(msg);
                }
                if let Some(reason) = file_sync.pinned_rewrite {
                    // 永久跳过（防已剪明细双算），不会有下轮重试——须让
                    // 手动同步的用户看到，不能显示为无事发生的成功
                    result.errors.push(format!(
                        "{}: 检测到文件被外部{reason}，改写区间已跳过以防重复计数（不会再导入）",
                        file_path.display()
                    ));
                }
            }
            Err(e) => {
                let msg = format!("{}: {e}", file_path.display());
                log::warn!("[SESSION-SYNC] 文件解析失败: {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[SESSION-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集目录下所有 .jsonl 文件（含子 agent 文件）
///
/// 扫描固定深度，不使用递归，避免死循环：
///   projects_dir/项目目录/*.jsonl                                      (主会话)
///   projects_dir/项目目录/SESSION_ID/subagents/*.jsonl                  (Task/Agent 子 agent)
///   projects_dir/项目目录/SESSION_ID/subagents/workflows/wf_*/*.jsonl   (Workflow 子 agent)
///
/// 最后一层是 Claude Code Workflow 功能产生的子 agent transcript，比普通子
/// agent 多嵌套一层 `workflows/wf_<ID>/`。漏掉这一层会让 Workflow 的 token
/// 用量完全不计入统计；`journal.jsonl` 不含 `type=="assistant"` 行，解析时
/// 会被 `sync_single_file` 天然跳过，因此这里无需按文件名过滤。
fn collect_jsonl_files(projects_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let entries = match fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 每个项目目录下的 .jsonl 文件
        if let Ok(sub_entries) = fs::read_dir(&path) {
            for sub_entry in sub_entries.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    // 主会话 JSONL 文件
                    files.push(sub_path);
                } else if sub_path.is_dir() {
                    // 扫描子 agent 目录: 项目/SESSION_ID/subagents/*.jsonl
                    let subagents_dir = sub_path.join("subagents");
                    if subagents_dir.is_dir() {
                        push_jsonl_children(&subagents_dir, &mut files);

                        // 额外下探 Workflow 子 agent:
                        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/*.jsonl
                        let workflows_dir = subagents_dir.join("workflows");
                        if workflows_dir.is_dir() {
                            if let Ok(wf_entries) = fs::read_dir(&workflows_dir) {
                                for wf_entry in wf_entries.flatten() {
                                    let wf_path = wf_entry.path();
                                    if wf_path.is_dir() {
                                        push_jsonl_children(&wf_path, &mut files);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    files
}

/// 将 `dir` 下直接子层的所有 `.jsonl` 文件追加到 `files`（不递归）。
fn push_jsonl_children(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
}

/// 单文件同步结果
#[derive(Debug, Default)]
struct ClaudeFileSync {
    imported: u32,
    skipped: u32,
    /// 文件尾部有未以 `\n` 终结的残段（写入方可能正在追加）。残段若能
    /// 解析成功仍会导入（request_id 去重防重复），但游标不越过它，
    /// 待下轮补全后重新确认。
    incomplete_tail: bool,
    /// 解析中途 IO 读错误。已提交部分照常入库、游标盖旧 mtime 让下轮
    /// 续读，但必须向上层报告：手动同步没有"下一轮"，静默返回会让用户
    /// 把未读完的文件当成同步成功。
    read_error: Option<String>,
    /// 检测到外部截断/重写、游标已钉至当前 EOF（值为原因描述）。旧区间
    /// 被永久跳过以防已剪明细双算——这不是 deferred（下轮不会重试），
    /// 必须走 errors 上报让用户知道发生了不可恢复的跳过。
    pinned_rewrite: Option<&'static str>,
}

/// 游标边界指纹覆盖的尾部字节数（与 Pi 的 `REVISION_TAIL_BYTES` 同值）。
const TAIL_FINGERPRINT_BYTES: i64 = 4096;

/// 游标边界前尾部字节的指纹。域标签防止与其他用途的哈希混淆。
fn claude_tail_fingerprint(tail: &[u8]) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(b"claude-session-tail-v1");
    hasher.update(tail);
    let digest = hasher.finalize();
    i64::from(u32::from_be_bytes(
        digest[..4].try_into().unwrap_or_default(),
    ))
}

/// 读取 `end` 之前最多 [`TAIL_FINGERPRINT_BYTES`] 字节；返回后文件位置
/// 恰好停在 `end`，增量路径可直接从这里继续读。
fn read_tail_before(file: &mut fs::File, end: i64) -> Result<Vec<u8>, AppError> {
    let len = end.clamp(0, TAIL_FINGERPRINT_BYTES);
    let mut tail = vec![0u8; len as usize];
    file.seek(SeekFrom::Start((end - len) as u64))
        .map_err(|e| AppError::Config(format!("无法定位文件偏移: {e}")))?;
    if len > 0 {
        file.read_exact(&mut tail)
            .map_err(|e| AppError::Config(format!("无法读取游标边界尾部: {e}")))?;
    }
    Ok(tail)
}

/// 向滚动尾部缓冲追加已提交的行字节，只保留末尾指纹窗口大小。
fn push_committed_tail(tail_buf: &mut Vec<u8>, bytes: &[u8]) {
    tail_buf.extend_from_slice(bytes);
    let max = TAIL_FINGERPRINT_BYTES as usize;
    if tail_buf.len() > max {
        tail_buf.drain(..tail_buf.len() - max);
    }
}

/// 同步单个 JSONL 文件。
///
/// 增量语义：游标是字节偏移（`last_byte_offset`），文件追加时 seek 到
/// 游标只读新增尾部，避免活跃会话文件每轮被整文件重读。游标只推进到
/// 最后一个完整行（以 `\n` 结尾）之后——按行号计数的旧游标会把未写完
/// 的半行也计入，导致该行补全后被跳过、记录永久丢失。
///
/// `last_byte_offset` 为 NULL（升级前的行号游标）时按旧行号跳过前 L 行、
/// 转换为字节位置后继续增量。不能回退全量重读：`rollup_and_prune` 会把
/// 30 天前的明细汇总后删除，request_id 去重只查明细表、对已剪条目失明，
/// 重导会在下次 rollup 时二次累加、永久放大统计。代价是旧行号游标半行
/// bug 吞掉的历史行无法找回（无持久账本时与已剪条目不可区分，双算比
/// 丢行更糟）。
///
/// 同一双算风险决定了对非追加变化（Claude Code 正常路径纯追加，此情形
/// 只来自外部干预）的处理：截断（游标超过文件大小）与重写（游标边界前
/// 尾部指纹失配——同尺寸/更大的替换靠 size 检测不出来）都把游标钉到
/// 当前 EOF、不重放任何旧区间，之后的追加恢复正常增量。
fn sync_single_file(
    db: &Database,
    file_path: &Path,
    cursor: Option<&SyncCursor>,
) -> Result<ClaudeFileSync, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let file_size = metadata.len() as i64;

    let last_modified = cursor.map_or(0, |c| c.last_modified);
    let last_byte_offset = cursor.and_then(|c| c.last_byte_offset);
    let last_fingerprint = cursor.and_then(|c| c.last_tail_fingerprint);

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok(ClaudeFileSync::default());
    }

    let mut file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;

    // 非追加变化检测（仅字节游标路径）。检出后游标钉到当前 EOF、旧偏移
    // 一概不重放（见函数文档：重放会把已剪明细双算进汇总）。指纹为 NULL
    // （升级存量行转换后的首轮之前）时无从校验，按纯追加处理——这是旧
    // 行号游标本就存在的暴露面，首轮写入后即有指纹。
    let mut chain = ChainNodes::new();
    let mut timings: HashMap<String, MessageTiming> = HashMap::new();
    let (start_byte, legacy_lines, mut tail_buf) = match last_byte_offset {
        Some(offset) => {
            let truncated = !(0..=file_size).contains(&offset);
            // seed 同时用于指纹校验与滚动尾部缓冲；读取后文件位置恰好
            // 停在 offset，无需再显式 seek
            let seed = if truncated {
                None
            } else {
                Some(read_tail_before(&mut file, offset)?)
            };
            let rewritten = match (&seed, last_fingerprint) {
                (Some(seed), Some(expected)) => claude_tail_fingerprint(seed) != expected,
                _ => false,
            };
            if truncated || rewritten {
                let reason = if truncated { "截断" } else { "重写" };
                log::warn!(
                    "[SESSION-SYNC] 文件被外部{reason}，游标钉至 EOF、不重放旧区间: {}",
                    file_path.display()
                );
                let tail = read_tail_before(&mut file, file_size)?;
                let fingerprint = claude_tail_fingerprint(&tail);
                let conn = lock_conn!(db.conn);
                update_claude_sync_state_on_conn(
                    &conn,
                    &file_path_str,
                    file_modified,
                    file_size,
                    Some(fingerprint),
                )?;
                return Ok(ClaudeFileSync {
                    pinned_rewrite: Some(reason),
                    ..Default::default()
                });
            }
            read_chain_seed(&mut file, offset, &mut chain, &mut timings)?;
            (offset, 0, seed.unwrap_or_default())
        }
        // 旧行号游标：从头按行转换（下方转换段），tail 缓冲从空积累
        None => (
            0,
            cursor.map_or(0, |c| c.last_line_offset.max(0)),
            Vec::new(),
        ),
    };

    let mut reader = BufReader::new(file);

    // 游标只推进到最后一个完整行之后
    let mut committed_offset = start_byte;
    let mut incomplete_tail = false;
    let mut buf: Vec<u8> = Vec::new();

    // 旧行号 → 字节位置转换：只数字节不解析不导入。旧 `lines()` 把无换行
    // 的尾段也计入行号，这里 read_until 的每次返回同样计一行，字节位置与
    // 旧游标精确对齐。文件行数不足 L（截断）时停在 EOF，等价旧游标对
    // 截断文件的"新内容行号偏小被跳过"语义。转换必须完整走完才能写游标：
    // 中途读错误若照常落库，last_line_offset 会被清 0、剩余待跳行数丢失，
    // 下轮会把旧代码已导入过的行当新行重导——所以这里出错直接整文件失败。
    let mut skipped_legacy_lines: i64 = 0;
    while skipped_legacy_lines < legacy_lines {
        buf.clear();
        let read = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| AppError::Config(format!("转换旧行号游标失败: {e}")))?;
        if read == 0 {
            break;
        }
        push_committed_tail(&mut tail_buf, &buf);
        committed_offset += read as i64;
        skipped_legacy_lines += 1;
    }

    let mut read_error: Option<String> = None;
    let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();
    let mut current_session_id: Option<String> = None;

    loop {
        buf.clear();
        let read = match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                // IO 错误：已提交的部分照常入库；游标写旧 mtime 让下轮
                // mtime 门放行、从 committed_offset 续读（见写入处）。
                // 错误消息带回上层：手动同步没有下一轮，须让用户看到
                read_error = Some(e.to_string());
                break;
            }
        };
        if buf.ends_with(b"\n") {
            push_committed_tail(&mut tail_buf, &buf);
            committed_offset += read as i64;
        } else {
            incomplete_tail = true;
        }

        if buf.iter().all(u8::is_ascii_whitespace) {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_slice(&buf) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 提取 session ID (从 system 或首条消息)
        if current_session_id.is_none() {
            if let Some(sid) = value.get("sessionId").and_then(|v| v.as_str()) {
                current_session_id = Some(sid.to_string());
            }
        }

        // 每一行都记进对话链，估算请求耗时时沿它找起点
        record_chain_node(&mut chain, &value);

        // 只处理 assistant 类型的消息
        if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }

        let message = match value.get("message") {
            Some(m) => m,
            None => continue,
        };

        let msg_id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        record_block_timing(&mut timings, &value, &msg_id);

        let usage = match message.get("usage") {
            Some(u) => u,
            None => continue,
        };

        let parsed = ParsedAssistantUsage {
            message_id: msg_id.clone(),
            model: message
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            input_tokens: usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_read_tokens: usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            stop_reason: message
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            timestamp: value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            session_id: current_session_id.clone(),
        };

        // 按 message.id 去重：优先保留有 stop_reason 的条目，否则保留最新的
        let should_replace = match messages.get(&msg_id) {
            None => true,
            Some(existing) => {
                // 新条目有 stop_reason 而旧条目没有 → 替换
                if parsed.stop_reason.is_some() && existing.stop_reason.is_none() {
                    true
                }
                // 两个都有或都没有 stop_reason → 取 output_tokens 更大的
                else if parsed.stop_reason.is_some() == existing.stop_reason.is_some() {
                    parsed.output_tokens > existing.output_tokens
                } else {
                    false
                }
            }
        };

        if should_replace {
            messages.insert(msg_id, parsed);
        }
    }

    // 写入数据库：单次取锁 + 事务，插入与游标推进原子提交，
    // 避免逐条插入时反复抢占连接锁
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| AppError::Database(format!("启动会话用量导入事务失败: {e}")))?;

    for msg in messages.values() {
        // 只要产生了真实计费 token 就导入，不再强制要求 stop_reason 或 output>0。
        //
        // Anthropic 在受理请求时即对 input + cache_read + cache_creation 计费
        // （这些在请求开始就确定），output 按实际生成量计。Workflow / 子 agent 的
        // 并行短命请求经常只写了 message_start 快照（output=1、stop_reason=None）
        // 却没有写最终块，但其 cache/input 成本已被真实计费。旧逻辑用 stop_reason
        // 非空 + output>0 双重过滤，会把这类请求整条丢弃，实测系统性低估约 4.1%，
        // 且 92% 集中在 workflow/subagent。这里改为「任一计费维度 > 0 即导入」。
        //
        // 去重选择逻辑（上方按 message.id 取 stop_reason 优先 / output 最大者）保持
        // 不变：它选出的代表行的 input/cache 本就准确；request_id = session:msg_id
        // 主键 + INSERT OR IGNORE 保证一个 message 仍只落库一次，放宽 gate 不会双算。
        let has_billable_tokens = msg.input_tokens > 0
            || msg.output_tokens > 0
            || msg.cache_read_tokens > 0
            || msg.cache_creation_tokens > 0;
        if !has_billable_tokens {
            continue;
        }

        let request_id = format!(
            "{}{}",
            crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX,
            msg.message_id
        );

        // 耗时估算：只给写完整了的回复算（有 stop_reason），而且要看得到它的第一块
        // （这一批里，或游标前的回看窗口里），否则起点对不上
        let latency_ms = timings
            .get(&msg.message_id)
            .filter(|timing| msg.stop_reason.is_some() && timing.starts_at_first_block)
            .and_then(|timing| {
                let start_ms = resolve_request_start_ms(&chain, timing.first_parent)?;
                estimated_latency_ms(start_ms, timing.end_ms?)
            });

        match upsert_session_log_entry_on_conn(&tx, &request_id, msg, latency_ms) {
            // 补全已有的行也算进 imported：数据变了，界面要跟着刷新
            Ok(SessionRowOutcome::Inserted | SessionRowOutcome::Updated) => imported += 1,
            Ok(SessionRowOutcome::Skipped) => skipped += 1,
            Err(e) => {
                log::warn!("[SESSION-SYNC] 插入失败 ({}): {e}", msg.message_id);
                skipped += 1;
            }
        }
    }

    // 更新同步状态（字节游标）。读错误时盖旧 mtime 而非当前值：盖当前值
    // 会让下轮在 mtime 门被跳过，未读完的完整行要等文件再次变化才有机会。
    // incomplete_tail 无需此处理——半行补全必然伴随 append 抬高 mtime。
    let stamped_modified = if read_error.is_some() {
        last_modified
    } else {
        file_modified
    };
    // tail_buf 始终恰好是 committed_offset 前的末段字节：增量路径以边界
    // seed 起始，之后只在游标推进（完整行）时追加
    let fingerprint = claude_tail_fingerprint(&tail_buf);
    update_claude_sync_state_on_conn(
        &tx,
        &file_path_str,
        stamped_modified,
        committed_offset,
        Some(fingerprint),
    )?;
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交会话用量导入事务失败: {e}")))?;

    Ok(ClaudeFileSync {
        imported,
        skipped,
        incomplete_tail,
        read_error,
        pinned_rewrite: None,
    })
}

/// 写入 Claude 路径的字节游标。`last_line_offset` 固定写 0：字节游标语义
/// 下行号不再维护，置 0 明确表示"行号游标不可用"。
fn update_claude_sync_state_on_conn(
    conn: &rusqlite::Connection,
    file_path: &str,
    last_modified: i64,
    byte_offset: i64,
    tail_fingerprint: Option<i64>,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.prepare_cached(
        "INSERT OR REPLACE INTO session_log_sync
             (file_path, last_modified, last_line_offset, last_synced_at, last_byte_offset,
              last_tail_fingerprint)
         VALUES (?1, ?2, 0, ?3, ?4, ?5)",
    )
    .and_then(|mut stmt| {
        stmt.execute(rusqlite::params![
            file_path,
            last_modified,
            now,
            byte_offset,
            tail_fingerprint
        ])
    })
    .map_err(|e| AppError::Database(format!("更新同步状态失败: {e}")))?;
    Ok(())
}

/// 获取 session_log_sync 表中某条目的同步进度。
///
/// 生产路径已改为 [`load_sync_cursors`] 批量预取；保留此单行查询给测试
/// 断言游标状态用。
#[cfg(test)]
pub(crate) fn get_sync_state(db: &Database, file_path: &str) -> Result<(i64, i64), AppError> {
    let conn = lock_conn!(db.conn);
    let result = conn.query_row(
        "SELECT last_modified, last_line_offset FROM session_log_sync WHERE file_path = ?1",
        rusqlite::params![file_path],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    );
    Ok(result.unwrap_or((0, 0)))
}

/// 返回文件 mtime 的纳秒时间戳。
///
/// `session_log_sync.last_modified` 旧数据是秒级时间戳；新写入纳秒值不需要
/// schema 迁移，旧值会自然触发一次增量重扫，并继续依赖行 offset 避免重复导入。
pub(crate) fn metadata_modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// 更新 session_log_sync 表中某条目的同步进度。
///
/// Shared by all session_usage_* parsers.
pub(crate) fn update_sync_state(
    db: &Database,
    file_path: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    update_sync_state_on_conn(&conn, file_path, last_modified, last_offset)
}

/// [`update_sync_state`] 的免锁版本，供调用方在已持锁的事务内把游标推进
/// 与数据插入绑成原子提交。
pub(crate) fn update_sync_state_on_conn(
    conn: &rusqlite::Connection,
    file_path: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.prepare_cached(
        "INSERT OR REPLACE INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .and_then(|mut stmt| stmt.execute(rusqlite::params![file_path, last_modified, last_offset, now]))
    .map_err(|e| AppError::Database(format!("更新同步状态失败: {e}")))?;
    Ok(())
}

/// 一条回复写库的结果。
#[derive(Debug, PartialEq, Eq)]
enum SessionRowOutcome {
    Inserted,
    /// 这条回复早先按中间块入过库，这次补全了用量或耗时；或者补全后发现它和
    /// 路由服务记的是同一次请求，把先前那一行删了。
    Updated,
    Skipped,
}

/// 已经在库里的那一行里，判断要不要补全用得到的三样。
struct StoredSessionRow {
    data_source: String,
    output_tokens: i64,
    latency_ms: i64,
}

fn find_stored_session_row(
    conn: &rusqlite::Connection,
    request_id: &str,
) -> Result<Option<StoredSessionRow>, AppError> {
    conn.prepare_cached(
        "SELECT COALESCE(data_source, 'proxy'), output_tokens, latency_ms
         FROM proxy_request_logs WHERE request_id = ?1",
    )
    .and_then(|mut stmt| {
        stmt.query_row([request_id], |row| {
            Ok(StoredSessionRow {
                data_source: row.get(0)?,
                output_tokens: row.get(1)?,
                latency_ms: row.get(2)?,
            })
        })
        .optional()
    })
    .map_err(|e| AppError::Database(format!("查询已入库的会话日志失败: {e}")))
}

/// 一条回复按定价算出来的五项成本：输入、输出、缓存读、缓存写、合计。
fn session_costs(conn: &rusqlite::Connection, msg: &ParsedAssistantUsage) -> [String; 5] {
    let usage = TokenUsage {
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        model: Some(msg.model.clone()),
        message_id: None,
    };
    match find_model_pricing_for_session(conn, &msg.model) {
        Some(pricing) => {
            let cost = CostCalculator::calculate(&usage, &pricing, Decimal::from(1));
            [
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            ]
        }
        None => std::array::from_fn(|_| "0".to_string()),
    }
}

/// 补全一条已经入库的回复。
///
/// 子代理的日志是逐块写的，前面的块只带当时的中间用量（输出 token 只有个位数）。
/// 同步恰好落在一条回复写到一半的时候，就会按中间值入库；最终块到了以后要把
/// 输出 token、成本和耗时补上，否则这条回复的输出永远少记。输入和缓存 token
/// 在第一块里就是准的。
///
/// 只动会话日志导入的明细行。30 天前的明细已经汇总后删除，但一条回复的各块
/// 不会隔这么久才写完，所以不存在「补全一条已汇总的回复」的情况。
fn refresh_session_log_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    msg: &ParsedAssistantUsage,
    latency_ms: Option<i64>,
    stored: &StoredSessionRow,
    dedup_key: &DedupKey,
) -> Result<SessionRowOutcome, AppError> {
    if stored.data_source != "session_log" {
        return Ok(SessionRowOutcome::Skipped);
    }
    let output_grew = i64::from(msg.output_tokens) > stored.output_tokens;
    let latency_ms = latency_ms.map_or(stored.latency_ms, |ms| ms.max(stored.latency_ms));
    if !output_grew && latency_ms == stored.latency_ms {
        return Ok(SessionRowOutcome::Skipped);
    }

    if !output_grew {
        conn.execute(
            "UPDATE proxy_request_logs SET latency_ms = ?1 WHERE request_id = ?2",
            rusqlite::params![latency_ms, request_id],
        )
        .map_err(|e| AppError::Database(format!("更新会话日志耗时失败: {e}")))?;
        return Ok(SessionRowOutcome::Updated);
    }

    // 入库时用量还是中间值，对不上路由服务记的那一行；补全后对上了，说明这次
    // 请求路由服务已经记过，先前入库的这一行是重复的
    if has_matching_proxy_usage_log(conn, dedup_key)? {
        conn.execute(
            "DELETE FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
        )
        .map_err(|e| AppError::Database(format!("删除重复的会话日志失败: {e}")))?;
        return Ok(SessionRowOutcome::Updated);
    }

    let [input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost] =
        session_costs(conn, msg);
    conn.execute(
        "UPDATE proxy_request_logs SET
            input_tokens = ?1, output_tokens = ?2, cache_read_tokens = ?3,
            cache_creation_tokens = ?4, input_cost_usd = ?5, output_cost_usd = ?6,
            cache_read_cost_usd = ?7, cache_creation_cost_usd = ?8, total_cost_usd = ?9,
            latency_ms = ?10
         WHERE request_id = ?11",
        rusqlite::params![
            msg.input_tokens,
            msg.output_tokens,
            msg.cache_read_tokens,
            msg.cache_creation_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            latency_ms,
            request_id,
        ],
    )
    .map_err(|e| AppError::Database(format!("补全会话日志失败: {e}")))?;
    Ok(SessionRowOutcome::Updated)
}

/// 把一条回复写进 proxy_request_logs：没入过库就插入，入过库就按需补全
/// （见 [`refresh_session_log_entry_on_conn`]）。
///
/// 调用方持有连接锁（通常在事务内）。`latency_ms` 是按日志时间戳估出来的
/// 请求耗时（含首字等待），估不出来传 None。
fn upsert_session_log_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    msg: &ParsedAssistantUsage,
    latency_ms: Option<i64>,
) -> Result<SessionRowOutcome, AppError> {
    let created_at = msg
        .timestamp
        .as_ref()
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
        app_type: "claude",
        model: &msg.model,
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        created_at,
    };
    if let Some(stored) = find_stored_session_row(conn, request_id)? {
        return refresh_session_log_entry_on_conn(
            conn, request_id, msg, latency_ms, &stored, &dedup_key,
        );
    }
    if has_matching_proxy_usage_log(conn, &dedup_key)? {
        return Ok(SessionRowOutcome::Skipped);
    }

    let [input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost] =
        session_costs(conn, msg);

    let inserted_rows = conn
        .execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)",
            rusqlite::params![
                request_id,
                "_session",         // provider_id: 标记为会话来源
                "claude",           // app_type
                msg.model,
                msg.model,          // request_model = model
                msg.input_tokens,
                msg.output_tokens,
                msg.cache_read_tokens,
                msg.cache_creation_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                latency_ms.unwrap_or(0), // latency_ms: 按时间戳估算，0 = 没有计时
                Option::<i64>::None, // first_token_ms: 会话日志无此数据
                200i64,             // status_code: 会话日志中的请求只要产生计费 token 即视为成功
                Option::<String>::None, // error_message
                msg.session_id,
                Some("session_log"), // provider_type
                1i64,               // is_streaming: Claude Code 通常使用流式
                "1.0",              // cost_multiplier
                created_at,
                "session_log",      // data_source
            ],
        )
        .map_err(|e| AppError::Database(format!("插入会话日志失败: {e}")))?;

    Ok(if inserted_rows > 0 {
        SessionRowOutcome::Inserted
    } else {
        SessionRowOutcome::Skipped
    })
}

/// 从 model_pricing 表查找模型定价（支持模糊匹配）
fn find_model_pricing_for_session(
    conn: &rusqlite::Connection,
    model_id: &str,
) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

/// 查询数据来源分布统计
pub fn get_data_source_breakdown(db: &Database) -> Result<Vec<DataSourceSummary>, AppError> {
    let conn = lock_conn!(db.conn);

    let effective_filter = effective_usage_log_filter("l");
    let sql = format!(
        "SELECT COALESCE(l.data_source, 'proxy') as ds, COUNT(*) as cnt,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as cost
         FROM proxy_request_logs l
         WHERE {effective_filter}
         GROUP BY ds
         ORDER BY cnt DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map([], |row| {
        Ok(DataSourceSummary {
            data_source: row.get(0)?,
            request_count: row.get::<_, i64>(1)? as u32,
            total_cost_usd: format!("{:.6}", row.get::<_, f64>(2)?),
        })
    })?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }

    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_result_notification_is_coalesced_to_one_call() {
        crate::usage_events::take_test_notify_count();
        notify_sync_result(&SessionSyncResult::default());
        let result = SessionSyncResult {
            imported: 25,
            ..SessionSyncResult::default()
        };
        notify_sync_result(&result);
        assert_eq!(crate::usage_events::take_test_notify_count(), 1);
    }

    #[test]
    fn last_sync_completed_at_only_moves_forward() {
        // 别的测试会跑 sync_all_unlocked 记下「现在」，这里用一小时后的时间避开它们
        let later = chrono::Utc::now().timestamp_millis() + 3_600_000;
        record_sync_completed(later);
        assert_eq!(last_sync_completed_at(), Some(later));

        record_sync_completed(later - 1_000);
        assert_eq!(last_sync_completed_at(), Some(later));
    }

    #[tokio::test]
    async fn session_sync_mutex_serializes_callers() {
        let first = session_sync_mutex().lock().await;
        assert!(session_sync_mutex().try_lock().is_err());
        drop(first);
        assert!(session_sync_mutex().try_lock().is_ok());
    }

    #[test]
    fn test_parse_usage_from_jsonl_line() {
        let line = r#"{"type":"assistant","message":{"id":"msg_test123","model":"claude-opus-4-6","usage":{"input_tokens":3,"output_tokens":150,"cache_read_input_tokens":5000,"cache_creation_input_tokens":10000},"stop_reason":"end_turn"},"timestamp":"2026-04-05T12:00:00Z","sessionId":"session-abc"}"#;

        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            value.get("type").and_then(|t| t.as_str()),
            Some("assistant")
        );

        let message = value.get("message").unwrap();
        let usage = message.get("usage").unwrap();

        assert_eq!(usage.get("input_tokens").unwrap().as_u64().unwrap(), 3);
        assert_eq!(usage.get("output_tokens").unwrap().as_u64().unwrap(), 150);
        assert_eq!(
            usage
                .get("cache_read_input_tokens")
                .unwrap()
                .as_u64()
                .unwrap(),
            5000
        );
        assert_eq!(
            usage
                .get("cache_creation_input_tokens")
                .unwrap()
                .as_u64()
                .unwrap(),
            10000
        );
        assert_eq!(
            message.get("stop_reason").unwrap().as_str().unwrap(),
            "end_turn"
        );
    }

    #[test]
    fn test_dedup_by_message_id() {
        // 同一个 message.id 有多条，应该取 stop_reason 有值的那条
        let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();

        // 中间条目（无 stop_reason）
        let intermediate = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 3,
            output_tokens: 26,
            cache_read_tokens: 5000,
            cache_creation_tokens: 10000,
            stop_reason: None,
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };
        messages.insert("msg_1".to_string(), intermediate);

        // 最终条目（有 stop_reason）
        let final_entry = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 3,
            output_tokens: 1349,
            cache_read_tokens: 5000,
            cache_creation_tokens: 10000,
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };

        // 应该替换
        let should_replace = final_entry.stop_reason.is_some()
            && messages.get("msg_1").unwrap().stop_reason.is_none();
        assert!(should_replace);

        messages.insert("msg_1".to_string(), final_entry);
        assert_eq!(messages.get("msg_1").unwrap().output_tokens, 1349);
    }

    #[test]
    fn test_insert_claude_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "proxy-different-id",
                    "openai-compatible",
                    "claude",
                    "claude-sonnet-4-5",
                    "claude-sonnet-4-5",
                    100,
                    20,
                    10,
                    5,
                    "0.10",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let msg = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 10,
            cache_creation_tokens: 5,
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("1970-01-01T00:16:45Z".to_string()),
            session_id: Some("session-1".to_string()),
        };

        let outcome = {
            let conn = lock_conn!(db.conn);
            upsert_session_log_entry_on_conn(&conn, "session:msg_1", &msg, None)?
        };
        assert_eq!(outcome, SessionRowOutcome::Skipped);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_collect_jsonl_files_includes_subagents() {
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project = tmp.join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        fs::create_dir_all(&subagents_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-abc.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(&tmp);
        assert_eq!(files.len(), 2);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(paths.iter().any(|p| p.contains("main.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-abc.jsonl")));

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_collect_jsonl_files_includes_workflow_subagents() {
        // Claude Code Workflow 把子 agent transcript 嵌在
        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/ 下，比普通子 agent 深一层。
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project = tmp.join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        let wf_dir = subagents_dir.join("workflows").join("wf_test123");
        fs::create_dir_all(&wf_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-plain.jsonl"), "{}").unwrap();
        fs::write(wf_dir.join("agent-wf.jsonl"), "{}").unwrap();
        // journal.jsonl 也会被收集，但解析时因无 assistant 行而产出 0 条
        fs::write(wf_dir.join("journal.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(&tmp);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // 主会话 + 普通子 agent + Workflow 子 agent(agent-wf + journal) = 4
        assert_eq!(files.len(), 4);
        assert!(paths.iter().any(|p| p.contains("main.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-plain.jsonl")));
        assert!(
            paths.iter().any(|p| p.contains("agent-wf.jsonl")),
            "Workflow 子 agent transcript 必须被收集"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    /// 构造一行带计费 token 的 assistant JSONL
    fn assistant_line(msg_id: &str, output_tokens: u32) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"id":"{msg_id}","model":"claude-opus-4-8","usage":{{"input_tokens":10,"output_tokens":{output_tokens},"cache_read_input_tokens":100,"cache_creation_input_tokens":50}},"stop_reason":"end_turn"}},"timestamp":"2026-06-07T13:01:23Z","sessionId":"session-x"}}"#
        )
    }

    fn byte_cursor(db: &Database, path: &Path) -> Option<i64> {
        load_sync_cursors(db)
            .unwrap()
            .get(path.to_string_lossy().as_ref())
            .and_then(|c| c.last_byte_offset)
    }

    fn sync_with_cursor(db: &Database, path: &Path) -> Result<ClaudeFileSync, AppError> {
        let cursors = load_sync_cursors(db)?;
        let cursor = cursors.get(path.to_string_lossy().as_ref()).copied();
        sync_single_file(db, path, cursor.as_ref())
    }

    fn bump_mtime(path: &Path) {
        // 测试内两次写入可能落在文件系统 mtime 精度的同一 tick 里，
        // 显式后移 mtime 确保第二轮扫描不被 mtime 门挡住
        let later = SystemTime::now() + std::time::Duration::from_secs(2);
        let file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(later))
            .unwrap();
    }

    #[test]
    fn test_incremental_append_advances_byte_cursor() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");

        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                assistant_line("msg_a", 5),
                assistant_line("msg_b", 6)
            ),
        )
        .unwrap();
        let first = sync_with_cursor(&db, &file)?;
        assert_eq!(first.imported, 2);
        assert!(!first.incomplete_tail);
        let size_after_first = fs::metadata(&file).unwrap().len() as i64;
        assert_eq!(byte_cursor(&db, &file), Some(size_after_first));

        // 追加两行后只导入新增，游标推进到新末尾
        let mut content = fs::read(&file).unwrap();
        content.extend_from_slice(format!("{}\n", assistant_line("msg_c", 7)).as_bytes());
        fs::write(&file, &content).unwrap();
        bump_mtime(&file);
        let second = sync_with_cursor(&db, &file)?;
        assert_eq!((second.imported, second.skipped), (1, 0));
        let size_after_second = fs::metadata(&file).unwrap().len() as i64;
        assert_eq!(byte_cursor(&db, &file), Some(size_after_second));

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_complete_tail_without_newline_imports_but_holds_cursor() -> Result<(), AppError> {
        // 尾段是完整 JSON 但没有换行符：应当导入（不丢数据），但游标停在
        // 上一个完整行末尾；补全换行后重扫靠 request_id 去重不双算
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");

        let first_line = format!("{}\n", assistant_line("msg_a", 5));
        fs::write(
            &file,
            format!("{first_line}{}", assistant_line("msg_tail", 6)),
        )
        .unwrap();
        let first = sync_with_cursor(&db, &file)?;
        assert_eq!(first.imported, 2, "无换行的完整尾段也必须导入");
        assert!(first.incomplete_tail);
        assert_eq!(
            byte_cursor(&db, &file),
            Some(first_line.len() as i64),
            "游标不得越过未终结的尾段"
        );

        // 补全换行 + 追加新行：尾段被重扫但去重，只导入新行
        let mut content = fs::read(&file).unwrap();
        content.extend_from_slice(format!("\n{}\n", assistant_line("msg_c", 7)).as_bytes());
        fs::write(&file, &content).unwrap();
        bump_mtime(&file);
        let second = sync_with_cursor(&db, &file)?;
        assert_eq!((second.imported, second.skipped), (1, 1));
        assert!(!second.incomplete_tail);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_partial_line_completed_later_is_not_lost() -> Result<(), AppError> {
        // 回归（行号游标的数据丢失 bug）：写入方掉在半行时，旧实现把半行
        // 计入行号游标，补全后该行因 line_offset 已计数被永久跳过。字节
        // 游标只推进到最后一个完整行，补全后必须导入
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");

        let first_line = format!("{}\n", assistant_line("msg_a", 5));
        let full_second = assistant_line("msg_partial", 6);
        let (head, rest) = full_second.split_at(full_second.len() / 2);
        fs::write(&file, format!("{first_line}{head}")).unwrap();
        let first = sync_with_cursor(&db, &file)?;
        assert_eq!(first.imported, 1, "半行不可解析，只导入完整行");
        assert!(first.incomplete_tail);
        assert_eq!(byte_cursor(&db, &file), Some(first_line.len() as i64));

        let mut content = fs::read(&file).unwrap();
        content.extend_from_slice(format!("{rest}\n").as_bytes());
        fs::write(&file, &content).unwrap();
        bump_mtime(&file);
        let second = sync_with_cursor(&db, &file)?;
        assert_eq!(second.imported, 1, "补全后的行必须被导入，不得因游标丢失");

        let conn = lock_conn!(db.conn);
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:msg_partial')",
            [],
            |row| row.get(0),
        )?;
        assert!(exists);
        drop(conn);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_truncated_file_pins_cursor_at_eof_without_replay() -> Result<(), AppError> {
        // 文件被外部截断（游标超过新大小）：游标钉到当前 EOF、不重放旧
        // 区间。关键场景：明细已被 rollup_and_prune 剪掉（这里导入后删掉
        // 模拟），request_id 去重失明——若回退从头重扫，截断文件里残留的
        // msg_a 会重导并在下次 rollup 二次累加。代价（刻意）：msg_a 也
        // 不再重放，丢行优于双算
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");

        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                assistant_line("msg_a", 5),
                assistant_line("msg_b", 6)
            ),
        )
        .unwrap();
        assert_eq!(sync_with_cursor(&db, &file)?.imported, 2);

        // 模拟 rollup 剪掉明细
        {
            let conn = lock_conn!(db.conn);
            conn.execute("DELETE FROM proxy_request_logs", [])?;
        }

        fs::write(&file, format!("{}\n", assistant_line("msg_a", 5))).unwrap();
        bump_mtime(&file);
        let rescan = sync_with_cursor(&db, &file)?;
        assert_eq!(
            (rescan.imported, rescan.skipped),
            (0, 0),
            "截断后不重放任何旧区间"
        );
        assert_eq!(
            rescan.pinned_rewrite,
            Some("截断"),
            "永久跳过必须上报，不得静默成功"
        );
        let size = fs::metadata(&file).unwrap().len() as i64;
        assert_eq!(byte_cursor(&db, &file), Some(size), "游标钉在当前 EOF");
        {
            let conn = lock_conn!(db.conn);
            let rows: i64 =
                conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))?;
            assert_eq!(rows, 0, "已剪明细不得被重导");
        }

        // 钉住后追加恢复正常增量
        let mut content = fs::read(&file).unwrap();
        content.extend_from_slice(format!("{}\n", assistant_line("msg_c", 7)).as_bytes());
        fs::write(&file, &content).unwrap();
        bump_mtime(&file);
        let after = sync_with_cursor(&db, &file)?;
        assert_eq!((after.imported, after.skipped), (1, 0), "追加恢复增量");

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_same_size_rewrite_detected_by_tail_fingerprint() -> Result<(), AppError> {
        // 同尺寸重写靠 size 检测不出来（游标仍在范围内），只有游标边界前
        // 的尾部指纹能发现。检出后同样钉 EOF 不重放：重写内容里可能混着
        // 已剪的旧事件，从旧偏移切入或回头重扫都会双算
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");

        fs::write(&file, format!("{}\n", assistant_line("msg_a", 5))).unwrap();
        assert_eq!(sync_with_cursor(&db, &file)?.imported, 1);
        let original_size = fs::metadata(&file).unwrap().len() as i64;

        // 等长不同内容的重写（msg_x 与 msg_a 同字节数）
        fs::write(&file, format!("{}\n", assistant_line("msg_x", 5))).unwrap();
        bump_mtime(&file);
        assert_eq!(
            fs::metadata(&file).unwrap().len() as i64,
            original_size,
            "前置条件：重写后文件大小不变"
        );
        let rescan = sync_with_cursor(&db, &file)?;
        assert_eq!(
            (rescan.imported, rescan.skipped),
            (0, 0),
            "指纹失配后不重放重写内容"
        );
        assert_eq!(
            rescan.pinned_rewrite,
            Some("重写"),
            "永久跳过必须上报，不得静默成功"
        );
        {
            let conn = lock_conn!(db.conn);
            let msg_x_rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
                rusqlite::params![format!(
                    "{}msg_x",
                    crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX
                )],
                |row| row.get(0),
            )?;
            assert_eq!(msg_x_rows, 0, "重写内容不得入库");
        }
        assert_eq!(byte_cursor(&db, &file), Some(original_size));

        // 指纹已按新内容更新：之后的追加恢复正常增量
        let mut content = fs::read(&file).unwrap();
        content.extend_from_slice(format!("{}\n", assistant_line("msg_c", 7)).as_bytes());
        fs::write(&file, &content).unwrap();
        bump_mtime(&file);
        let after = sync_with_cursor(&db, &file)?;
        assert_eq!((after.imported, after.skipped), (1, 0), "追加恢复增量");

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_legacy_line_cursor_converts_without_reimport() -> Result<(), AppError> {
        // 升级路径：存量行只有行号游标（last_byte_offset 为 NULL）→ 按行号
        // 跳过前 L 行转换为字节位置，只导入其后的新行。关键场景：msg_a 的
        // 明细行已被 rollup_and_prune 剪掉（这里刻意不预插），request_id
        // 去重对它失明——若退化为全量重读，msg_a 会被重导并在下次 rollup
        // 时二次累加
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");
        let file_path_str = file.to_string_lossy().to_string();

        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                assistant_line("msg_a", 5),
                assistant_line("msg_b", 6)
            ),
        )
        .unwrap();

        // 旧版本游标：行号=1（msg_a 旧代码导入过、明细已剪），无字节游标，mtime 旧值
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES (?1, 1, 1, 1)",
                rusqlite::params![file_path_str],
            )?;
        }

        let result = sync_with_cursor(&db, &file)?;
        assert_eq!(
            (result.imported, result.skipped),
            (1, 0),
            "只导入行号游标之后的 msg_b，已剪的 msg_a 不重导"
        );
        let conn = lock_conn!(db.conn);
        let msg_a_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
            rusqlite::params![format!(
                "{}msg_a",
                crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX
            )],
            |row| row.get(0),
        )?;
        drop(conn);
        assert_eq!(msg_a_rows, 0, "msg_a 不得被重导");
        let size = fs::metadata(&file).unwrap().len() as i64;
        assert_eq!(byte_cursor(&db, &file), Some(size), "转换后写入字节游标");

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_legacy_line_cursor_beyond_eof_imports_nothing() -> Result<(), AppError> {
        // 行号游标超过文件行数（截断/重写后变短）：转换停在 EOF，不导入
        // 任何行——等价旧行号游标对截断文件的"新内容行号偏小被跳过"语义
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");
        let file_path_str = file.to_string_lossy().to_string();

        fs::write(&file, format!("{}\n", assistant_line("msg_a", 5))).unwrap();
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
                 VALUES (?1, 1, 5, 1)",
                rusqlite::params![file_path_str],
            )?;
        }

        let result = sync_with_cursor(&db, &file)?;
        assert_eq!((result.imported, result.skipped), (0, 0));
        let size = fs::metadata(&file).unwrap().len() as i64;
        assert_eq!(byte_cursor(&db, &file), Some(size), "游标停在 EOF");

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_sync_imports_billable_message_without_stop_reason() -> Result<(), AppError> {
        // 回归：stop_reason 缺失但有真实 cache/input 成本的 message（Workflow /
        // 子 agent 常见的「只有 message_start 快照、没写最终块」形态）必须被计入，
        // 不能因缺 stop_reason 或 output==0 而整条丢弃；全 0 token 的占位行仍应跳过。
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("agent-wf.jsonl");

        // 第一行：无 stop_reason、output=1，但 cache_read/cache_creation 很大 → 应导入
        // 第二行：全部 token 为 0 → 应跳过（无计费意义）
        let billable = r#"{"type":"assistant","message":{"id":"msg_nostop","model":"claude-opus-4-8","usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":48719,"cache_creation_input_tokens":2061}},"timestamp":"2026-06-07T13:01:23Z","sessionId":"session-wf"}"#;
        let empty = r#"{"type":"assistant","message":{"id":"msg_empty","model":"claude-opus-4-8","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-06-07T13:01:24Z","sessionId":"session-wf"}"#;
        fs::write(&file, format!("{billable}\n{empty}\n")).unwrap();

        let file_sync = sync_single_file(&db, &file, None)?;
        assert_eq!(
            file_sync.imported, 1,
            "有 cache 成本但无 stop_reason 的 message 必须被导入"
        );

        let conn = lock_conn!(db.conn);
        let cache_read: i64 = conn.query_row(
            "SELECT cache_read_tokens FROM proxy_request_logs WHERE request_id = 'session:msg_nostop'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cache_read, 48719, "cache_read 必须被完整记录");
        let empty_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:msg_empty')",
            [],
            |row| row.get(0),
        )?;
        assert!(!empty_exists, "全 0 token 的 message 应被跳过");
        drop(conn);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    fn chain_uuid(n: u8) -> String {
        format!("00000000-0000-4000-8000-0000000000{n:02x}")
    }

    /// 一行不带用量的日志（用户消息、工具结果、attachment），只有链信息
    fn chain_line(kind: &str, uuid: u8, parent: Option<u8>, timestamp: &str) -> String {
        let parent = parent.map_or("null".to_string(), |p| format!("\"{}\"", chain_uuid(p)));
        format!(
            r#"{{"parentUuid":{parent},"type":"{kind}","uuid":"{}","timestamp":"{timestamp}","sessionId":"session-t"}}"#,
            chain_uuid(uuid)
        )
    }

    /// 一条回复里的一个内容块；同一条回复的各块共用 message.id 和最终用量
    /// （主会话的写法：整条回复结束时一次写完）
    fn assistant_block(
        msg_id: &str,
        uuid: u8,
        parent: u8,
        block_index: u32,
        timestamp: &str,
    ) -> String {
        assistant_block_with(msg_id, uuid, parent, block_index, timestamp, 900, true)
    }

    /// 子代理的写法是逐块写：非末块只带当时的输出 token，也没有 stop_reason
    fn assistant_block_with(
        msg_id: &str,
        uuid: u8,
        parent: u8,
        block_index: u32,
        timestamp: &str,
        output_tokens: u32,
        finished: bool,
    ) -> String {
        let stop_reason = if finished { "\"tool_use\"" } else { "null" };
        format!(
            r#"{{"parentUuid":"{}","type":"assistant","uuid":"{}","apiBlockIndex":{block_index},"message":{{"id":"{msg_id}","model":"claude-opus-4-8","usage":{{"input_tokens":10,"output_tokens":{output_tokens},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"stop_reason":{stop_reason}}},"timestamp":"{timestamp}","sessionId":"session-t"}}"#,
            chain_uuid(parent),
            chain_uuid(uuid)
        )
    }

    fn append_lines(file: &Path, lines: &[String]) {
        let mut content = fs::read(file).unwrap_or_default();
        content.extend_from_slice((lines.join("\n") + "\n").as_bytes());
        fs::write(file, &content).unwrap();
        bump_mtime(file);
    }

    /// (输出 token, 合计成本, 耗时)
    fn stored_row(db: &Database, msg_id: &str) -> Option<(i64, String, i64)> {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT output_tokens, total_cost_usd, latency_ms FROM proxy_request_logs
             WHERE request_id = ?1",
            [format!("session:{msg_id}")],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .unwrap()
    }

    fn latency_of(db: &Database, msg_id: &str) -> i64 {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT latency_ms FROM proxy_request_logs WHERE request_id = ?1",
            [format!("session:{msg_id}")],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn temp_session_file() -> (PathBuf, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("session.jsonl");
        (tmp, file)
    }

    #[test]
    fn test_latency_spans_from_tool_result_to_last_block() -> Result<(), AppError> {
        let db = Database::memory()?;
        let (tmp, file) = temp_session_file();
        let lines = [
            // 起点：工具结果
            chain_line("user", 1, None, "2026-06-07T13:00:00.000Z"),
            chain_line("attachment", 2, Some(1), "2026-06-07T13:00:00.003Z"),
            // 回复开始后才补记的 attachment，时间戳比第一块还晚，不能当起点
            chain_line("attachment", 3, Some(2), "2026-06-07T13:00:05.001Z"),
            assistant_block("msg_t", 4, 3, 0, "2026-06-07T13:00:05.000Z"),
            assistant_block("msg_t", 5, 4, 1, "2026-06-07T13:00:07.000Z"),
            // 并行工具调用：前一个工具的结果插在同一条回复的两块中间
            chain_line("user", 6, Some(5), "2026-06-07T13:00:08.000Z"),
            assistant_block("msg_t", 7, 6, 2, "2026-06-07T13:00:12.500Z"),
        ];
        fs::write(&file, lines.join("\n") + "\n").unwrap();

        let file_sync = sync_single_file(&db, &file, None)?;
        assert_eq!(file_sync.imported, 1);
        assert_eq!(latency_of(&db, "msg_t"), 12_500);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_latency_start_is_found_before_the_cursor() -> Result<(), AppError> {
        // 工具结果先落盘、被上一轮同步读走；回复结束时才整条写出，
        // 它的起点行在游标前面
        let db = Database::memory()?;
        let (tmp, file) = temp_session_file();
        let before = [
            chain_line("user", 1, None, "2026-06-07T13:00:00.000Z"),
            chain_line("attachment", 2, Some(1), "2026-06-07T13:00:00.003Z"),
        ];
        fs::write(&file, before.join("\n") + "\n").unwrap();
        let first = sync_with_cursor(&db, &file)?;
        assert_eq!(first.imported, 0);

        let mut content = fs::read(&file).unwrap();
        content.extend_from_slice(
            format!(
                "{}\n",
                assistant_block("msg_t", 3, 2, 0, "2026-06-07T13:00:09.000Z")
            )
            .as_bytes(),
        );
        fs::write(&file, &content).unwrap();
        bump_mtime(&file);
        let second = sync_with_cursor(&db, &file)?;
        assert_eq!(second.imported, 1);
        assert_eq!(latency_of(&db, "msg_t"), 9_000);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_latency_is_not_estimated_without_a_reliable_start() -> Result<(), AppError> {
        let db = Database::memory()?;
        let (tmp, file) = temp_session_file();
        let lines = [
            chain_line("user", 1, None, "2026-06-07T13:00:00.000Z"),
            // 看不到第一块：最先出现的是第 2 块，它的父行是插进来的工具结果
            assistant_block("msg_no_first", 2, 1, 2, "2026-06-07T13:00:09.000Z"),
            // 父行不在文件里
            assistant_block("msg_orphan", 3, 9, 0, "2026-06-07T13:00:20.000Z"),
            // 起点和结束挨得太近，不像一次真实请求
            chain_line("user", 4, None, "2026-06-07T13:01:00.000Z"),
            assistant_block("msg_instant", 5, 4, 0, "2026-06-07T13:01:00.020Z"),
        ];
        fs::write(&file, lines.join("\n") + "\n").unwrap();

        let file_sync = sync_single_file(&db, &file, None)?;
        assert_eq!(file_sync.imported, 3);
        assert_eq!(latency_of(&db, "msg_no_first"), 0);
        assert_eq!(latency_of(&db, "msg_orphan"), 0);
        assert_eq!(latency_of(&db, "msg_instant"), 0);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_partial_subagent_reply_is_completed_by_its_final_block() -> Result<(), AppError> {
        let first_block = [
            chain_line("user", 1, None, "2026-06-07T13:00:00.000Z"),
            // 逐块写：第一块落盘时输出 token 还只是开头的几个
            assistant_block_with("msg_s", 2, 1, 0, "2026-06-07T13:00:02.000Z", 5, false),
        ];
        let final_block = [assistant_block_with(
            "msg_s",
            3,
            2,
            1,
            "2026-06-07T13:00:30.000Z",
            4_660,
            true,
        )];

        // 同步恰好落在两块之间
        let db = Database::memory()?;
        let (tmp, file) = temp_session_file();
        append_lines(&file, &first_block);
        assert_eq!(sync_with_cursor(&db, &file)?.imported, 1);
        assert_eq!(
            stored_row(&db, "msg_s").map(|row| (row.0, row.2)),
            Some((5, 0))
        );
        append_lines(&file, &final_block);
        let second = sync_with_cursor(&db, &file)?;
        assert_eq!((second.imported, second.skipped), (1, 0));

        // 对照：整条回复在一轮同步里读到
        let whole_db = Database::memory()?;
        let (whole_tmp, whole_file) = temp_session_file();
        append_lines(&whole_file, &first_block);
        append_lines(&whole_file, &final_block);
        sync_with_cursor(&whole_db, &whole_file)?;

        let completed = stored_row(&db, "msg_s");
        assert_eq!(completed, stored_row(&whole_db, "msg_s"));
        assert_eq!(completed.map(|row| (row.0, row.2)), Some((4_660, 30_000)));

        // 再同步一轮不会重复补
        bump_mtime(&file);
        let third = sync_with_cursor(&db, &file)?;
        assert_eq!((third.imported, third.skipped), (0, 0));

        fs::remove_dir_all(&tmp).ok();
        fs::remove_dir_all(&whole_tmp).ok();
        Ok(())
    }

    #[test]
    fn test_latency_is_extended_when_later_blocks_arrive() -> Result<(), AppError> {
        // 主会话的各块都带最终用量；同步读在一次写入的中途时，先按前几块算了
        // 较短的耗时，后面的块到了要补上
        let db = Database::memory()?;
        let (tmp, file) = temp_session_file();
        append_lines(
            &file,
            &[
                chain_line("user", 1, None, "2026-06-07T13:00:00.000Z"),
                assistant_block("msg_t", 2, 1, 0, "2026-06-07T13:00:05.000Z"),
            ],
        );
        sync_with_cursor(&db, &file)?;
        assert_eq!(latency_of(&db, "msg_t"), 5_000);

        append_lines(
            &file,
            &[
                chain_line("user", 3, Some(2), "2026-06-07T13:00:06.000Z"),
                assistant_block("msg_t", 4, 3, 1, "2026-06-07T13:00:12.500Z"),
            ],
        );
        let second = sync_with_cursor(&db, &file)?;
        assert_eq!(second.imported, 1);
        assert_eq!(latency_of(&db, "msg_t"), 12_500);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_completed_reply_matching_a_proxy_log_is_removed() -> Result<(), AppError> {
        let db = Database::memory()?;
        let (tmp, file) = temp_session_file();
        append_lines(
            &file,
            &[
                chain_line("user", 1, None, "2026-06-07T13:00:00.000Z"),
                assistant_block_with("msg_p", 2, 1, 0, "2026-06-07T13:00:02.000Z", 5, false),
            ],
        );
        // 中间值对不上路由服务记的那一行，先入了库
        sync_with_cursor(&db, &file)?;
        assert!(stored_row(&db, "msg_p").is_some());

        // 请求结束后路由服务记下了同一次请求
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "proxy-id",
                    "p1",
                    "claude",
                    "claude-opus-4-8",
                    "claude-opus-4-8",
                    10,
                    4_660,
                    0,
                    0,
                    "0.10",
                    30_000,
                    200,
                    parse_timestamp_millis("2026-06-07T13:00:30.000Z").unwrap() / 1000,
                    "proxy"
                ],
            )?;
        }
        append_lines(
            &file,
            &[assistant_block_with(
                "msg_p",
                3,
                2,
                1,
                "2026-06-07T13:00:30.000Z",
                4_660,
                true,
            )],
        );
        sync_with_cursor(&db, &file)?;

        assert_eq!(stored_row(&db, "msg_p"), None);
        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);
        drop(conn);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }
}
