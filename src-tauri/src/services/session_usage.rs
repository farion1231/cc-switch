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
    effective_usage_log_filter, find_model_pricing, should_skip_session_insert, DedupKey,
};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
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
    notify_sync_result(&result);
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
    status_code: u16,
    error_message: Option<String>,
    is_api_error: bool,
    timestamp: Option<String>,
    session_id: Option<String>,
}

/// One-time cursor marker for the parser version that started importing
/// structured Claude Code API errors. Existing installations have already
/// advanced past those zero-token rows, so they need one idempotent full scan.
const CLAUDE_API_ERROR_BACKFILL_MARKER: &str = "__claude_api_error_backfill_v1__";
const CLAUDE_BACKFILL_ERROR_MARKER: &str = "__claude_api_error_backfill_error_v1__";
const CLAUDE_INCOMPLETE_TAIL_MARKER_PREFIX: &str = "__claude_incomplete_tail_v1__:";
const CLAUDE_FILE_STATE_MARKER_PREFIX: &str = "__claude_file_state_v1__:";
const CLAUDE_FILE_HASH_MARKER_PREFIX: &str = "__claude_file_hash_v1__:";
const CLAUDE_FILE_FULL_HASH_MARKER_PREFIX: &str = "__claude_file_full_hash_v1__:";
const CLAUDE_PROXY_ERROR_DEDUP_MARKER_PREFIX: &str = "__claude_proxy_error_dedup_v1__:";
const FNV1A_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV1A_PRIME: u64 = 0x100000001b3;
const FILE_FINGERPRINT_SAMPLE_SIZE: i64 = 4096;
const FILE_FULL_HASH_VERIFY_INTERVAL_SECONDS: i64 = 24 * 60 * 60;

/// 同步 Claude Code 会话日志到使用统计数据库
pub fn sync_claude_session_logs(db: &Database) -> Result<SessionSyncResult, AppError> {
    let projects_dir = get_claude_config_dir().join("projects");
    sync_claude_session_logs_from_projects_dir(db, &projects_dir)
}

fn sync_claude_session_logs_from_projects_dir(
    db: &Database,
    projects_dir: &Path,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        suspected_duplicates: 0,
        deferred_files: 0,
        errors: vec![],
    };

    match fs::metadata(projects_dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            result.errors.push(format!(
                "Claude projects 路径不是目录: {}",
                projects_dir.display()
            ));
            return Ok(result);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(error) => {
            result.errors.push(format!(
                "无法读取 Claude projects 目录 {}: {error}",
                projects_dir.display()
            ));
            return Ok(result);
        }
    }

    // 收集所有 .jsonl 文件
    let (jsonl_files, collection_errors) = collect_jsonl_files(projects_dir);
    result.errors.extend(collection_errors);

    let force_full_scan = get_sync_state(db, CLAUDE_API_ERROR_BACKFILL_MARKER)? == (0, 0);

    for file_path in &jsonl_files {
        result.files_scanned += 1;

        match sync_single_file_with_options(db, file_path, force_full_scan) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("{}: {e}", file_path.display());
                log::warn!("[SESSION-SYNC] 文件解析失败: {msg}");
                result.errors.push(msg);
            }
        }
    }

    complete_api_error_backfill_if_successful(db, force_full_scan, &result.errors)?;

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

fn complete_api_error_backfill_if_successful(
    db: &Database,
    force_full_scan: bool,
    errors: &[String],
) -> Result<(), AppError> {
    if !force_full_scan {
        return Ok(());
    }

    if errors.is_empty() {
        update_sync_state(db, CLAUDE_API_ERROR_BACKFILL_MARKER, 1, 1)?;
        update_sync_state(db, CLAUDE_BACKFILL_ERROR_MARKER, 0, 0)?;
        return Ok(());
    }

    // 同一组错误连续出现两次后停止每分钟强制全量回扫。普通增量同步仍会
    // 每分钟枚举并重试这些路径；而旧文件没有内容指纹，会在恢复可读后保守
    // 全量解析，因此不会因结束一次性 backfill 而永久漏记。
    let mut sorted_errors = errors.to_vec();
    sorted_errors.sort_unstable();
    let mut signature_hash = FNV1A_OFFSET_BASIS;
    for error in &sorted_errors {
        update_fnv1a(&mut signature_hash, error.as_bytes());
        update_fnv1a(&mut signature_hash, b"\0");
    }
    let signature = (
        signature_hash as i64,
        sorted_errors.len().min(i64::MAX as usize) as i64,
    );
    if get_sync_state(db, CLAUDE_BACKFILL_ERROR_MARKER)? == signature {
        log::warn!(
            "[SESSION-SYNC] backfill 连续遇到相同的 {} 个错误，结束强制全量扫描并保留普通重试",
            errors.len()
        );
        update_sync_state(db, CLAUDE_API_ERROR_BACKFILL_MARKER, 1, 1)?;
        update_sync_state(db, CLAUDE_BACKFILL_ERROR_MARKER, 0, 0)?;
    } else {
        update_sync_state(db, CLAUDE_BACKFILL_ERROR_MARKER, signature.0, signature.1)?;
    }

    Ok(())
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
fn collect_jsonl_files(projects_dir: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let mut errors = Vec::new();

    for entry in read_dir_entries(projects_dir, false, &mut errors) {
        let path = entry.path();
        if !path_is_dir(&path, &mut errors) {
            continue;
        }
        // 每个项目目录下的 .jsonl 文件
        for sub_entry in read_dir_entries(&path, false, &mut errors) {
            let sub_path = sub_entry.path();
            if sub_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                // 主会话 JSONL 文件
                files.push(sub_path);
            } else if path_is_dir(&sub_path, &mut errors) {
                // 扫描子 agent 目录: 项目/SESSION_ID/subagents/*.jsonl
                let subagents_dir = sub_path.join("subagents");
                push_jsonl_children(&subagents_dir, true, &mut files, &mut errors);

                // 额外下探 Workflow 子 agent:
                // 项目/SESSION_ID/subagents/workflows/wf_<ID>/*.jsonl
                let workflows_dir = subagents_dir.join("workflows");
                for wf_entry in read_dir_entries(&workflows_dir, true, &mut errors) {
                    let wf_path = wf_entry.path();
                    if path_is_dir(&wf_path, &mut errors) {
                        push_jsonl_children(&wf_path, false, &mut files, &mut errors);
                    }
                }
            }
        }
    }

    (files, errors)
}

/// 将 `dir` 下直接子层的所有 `.jsonl` 文件追加到 `files`（不递归）。
fn push_jsonl_children(
    dir: &Path,
    optional: bool,
    files: &mut Vec<PathBuf>,
    errors: &mut Vec<String>,
) {
    for entry in read_dir_entries(dir, optional, errors) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn read_dir_entries(dir: &Path, optional: bool, errors: &mut Vec<String>) -> Vec<fs::DirEntry> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => {
            return Vec::new();
        }
        Err(error) => {
            errors.push(format!("无法读取目录 {}: {error}", dir.display()));
            return Vec::new();
        }
    };

    entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(error) => {
                errors.push(format!("无法读取目录项 {}: {error}", dir.display()));
                None
            }
        })
        .collect()
}

fn path_is_dir(path: &Path, errors: &mut Vec<String>) -> bool {
    match fs::metadata(path) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) => {
            errors.push(format!("无法读取路径元数据 {}: {error}", path.display()));
            false
        }
    }
}

/// 同步单个 JSONL 文件，返回 (imported, skipped)
#[cfg(test)]
fn sync_single_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    sync_single_file_with_options(db, file_path, false)
}

fn sync_single_file_with_options(
    db: &Database,
    file_path: &Path,
    force_full_scan: bool,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos_checked(&metadata)?;
    let file_len = metadata.len().min(i64::MAX as u64) as i64;

    // 检查同步状态。mtime 不能单独代表文件内容：日志可能在时间戳不变时追加，
    // 也可能被截断或替换，因此额外保存 (mtime, length) 文件签名。
    let (_, saved_offset) = if force_full_scan {
        (0, 0)
    } else {
        get_sync_state(db, &file_path_str)?
    };
    let file_state_marker = format!("{CLAUDE_FILE_STATE_MARKER_PREFIX}{file_path_str}");
    let previous_file_state = get_sync_state(db, &file_state_marker)?;
    let file_hash_marker = format!("{CLAUDE_FILE_HASH_MARKER_PREFIX}{file_path_str}");
    let previous_file_hash = get_sync_state(db, &file_hash_marker)?;
    let file_full_hash_marker = format!("{CLAUDE_FILE_FULL_HASH_MARKER_PREFIX}{file_path_str}");
    let previous_full_hash = get_sync_state(db, &file_full_hash_marker)?;
    let now = unix_timestamp_seconds();

    // 稳态先用固定大小采样避免每分钟读取全部历史日志；每天再全量校验一次，
    // 让刻意保留 mtime/长度及采样窗口的替换也不会永久漏掉。
    let mut full_content_changed = false;
    if !force_full_scan
        && previous_file_state == (file_modified, file_len)
        && previous_file_hash.1 == file_len
        && hash_file_prefix(file_path, file_len)? == previous_file_hash.0
    {
        if previous_full_hash != (0, 0)
            && now.saturating_sub(previous_full_hash.1) < FILE_FULL_HASH_VERIFY_INTERVAL_SECONDS
        {
            return Ok((0, 0));
        }
        let current_full_hash = hash_entire_file_prefix(file_path, file_len)?;
        if previous_full_hash.0 == current_full_hash {
            update_sync_state(db, &file_full_hash_marker, current_full_hash, now)?;
            return Ok((0, 0));
        }
        full_content_changed = true;
    }

    let can_continue_from_cursor = !force_full_scan
        && !full_content_changed
        && previous_file_state != (0, 0)
        && previous_file_hash.1 == previous_file_state.1
        && file_len >= previous_file_state.1
        && hash_file_prefix(file_path, previous_file_state.1)? == previous_file_hash.0;
    let last_offset = if can_continue_from_cursor {
        saved_offset
    } else {
        // 文件被截断、替换，或尚无内容指纹时，旧行号不再可信。
        0
    };

    // 从上次偏移位置开始增量解析
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let mut reader = BufReader::new(file);
    let incomplete_tail_marker = format!("{CLAUDE_INCOMPLETE_TAIL_MARKER_PREFIX}{file_path_str}");
    let observed_incomplete_tail = get_sync_state(db, &incomplete_tail_marker)?;

    let mut line_offset: i64 = 0;
    let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();
    let mut current_session_id: Option<String> = None;
    let mut line = Vec::new();
    let mut full_hash = FNV1A_OFFSET_BASIS;
    let mut full_hashed_bytes = 0_i64;
    let mut previous_prefix_hash = FNV1A_OFFSET_BASIS;
    let mut previous_prefix_hashed_bytes = 0_i64;

    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line).map_err(|error| {
            AppError::Config(format!(
                "读取 JSONL 失败 ({} 第 {} 行): {error}",
                file_path.display(),
                line_offset + 1
            ))
        })?;
        if bytes_read == 0 {
            break;
        }

        let bytes_to_hash = (file_len - full_hashed_bytes).clamp(0, bytes_read as i64) as usize;
        update_fnv1a(&mut full_hash, &line[..bytes_to_hash]);
        full_hashed_bytes += bytes_to_hash as i64;
        if can_continue_from_cursor {
            let prefix_len = previous_file_state.1;
            let prefix_bytes =
                (prefix_len - previous_prefix_hashed_bytes).clamp(0, bytes_read as i64) as usize;
            update_fnv1a(&mut previous_prefix_hash, &line[..prefix_bytes]);
            previous_prefix_hashed_bytes += prefix_bytes as i64;
        }

        line_offset += 1;

        // 跳过已处理的行
        if line_offset <= last_offset {
            continue;
        }

        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(error) if !line.ends_with(b"\n") => {
                let tail_signature = (fnv1a_hash(&line), line.len().min(i64::MAX as usize) as i64);
                if observed_incomplete_tail == tail_signature {
                    log::warn!(
                        "[SESSION-SYNC] 跳过稳定未变化的损坏 JSONL 末行 ({} 第 {line_offset} 行): {error}",
                        file_path.display()
                    );
                    line_offset -= 1;
                    break;
                }

                update_sync_state(
                    db,
                    &incomplete_tail_marker,
                    tail_signature.0,
                    tail_signature.1,
                )?;
                return Err(AppError::Config(format!(
                    "解析可能尚未写完的 JSONL 末行失败 ({} 第 {line_offset} 行): {error}",
                    file_path.display()
                )));
            }
            Err(error) => {
                log::warn!(
                    "[SESSION-SYNC] 跳过损坏的 JSONL 记录 ({} 第 {line_offset} 行): {error}",
                    file_path.display()
                );
                continue;
            }
        };

        // 提取 session ID (从 system 或首条消息)
        if current_session_id.is_none() {
            if let Some(sid) = value
                .get("sessionId")
                .and_then(|v| v.as_str())
                .filter(|sid| !sid.trim().is_empty())
            {
                current_session_id = Some(sid.to_string());
            }
        }

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

        let usage = match message.get("usage") {
            Some(u) => u,
            None => continue,
        };

        let api_error_status = value
            .get("apiErrorStatus")
            .and_then(|v| v.as_u64())
            .filter(|status| (400..=599).contains(status))
            .map(|status| status as u16);
        let is_api_error = value
            .get("isApiErrorMessage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            && api_error_status.is_some();

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
            status_code: api_error_status.unwrap_or(200),
            error_message: if is_api_error {
                value
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            } else {
                None
            },
            is_api_error,
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

    if can_continue_from_cursor
        && (previous_prefix_hashed_bytes != previous_file_state.1
            || previous_prefix_hash as i64 != previous_full_hash.0)
    {
        // 追加期间旧前缀也被改写；尚未执行任何插入，安全回退为一次全量解析。
        return sync_single_file_with_options(db, file_path, true);
    }

    // 写入数据库
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

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
        if !has_billable_tokens && !msg.is_api_error {
            continue;
        }

        let request_id = format!(
            "{}{}",
            crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX,
            msg.message_id
        );

        match insert_session_log_entry(db, &request_id, msg) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[SESSION-SYNC] 插入失败 ({}): {e}", msg.message_id);
                return Err(e);
            }
        }
    }

    // 更新同步状态
    update_sync_state(db, &file_path_str, file_modified, line_offset)?;
    update_sync_state(db, &file_full_hash_marker, full_hash as i64, now)?;
    update_sync_state(
        db,
        &file_hash_marker,
        hash_file_prefix(file_path, file_len)?,
        file_len,
    )?;
    update_sync_state(db, &file_state_marker, file_modified, file_len)?;
    if observed_incomplete_tail != (0, 0) {
        update_sync_state(db, &incomplete_tail_marker, 0, 0)?;
    }

    Ok((imported, skipped))
}

fn update_fnv1a(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV1A_PRIME);
    }
}

fn fnv1a_hash(bytes: &[u8]) -> i64 {
    let mut hash = FNV1A_OFFSET_BASIS;
    update_fnv1a(&mut hash, bytes);
    hash as i64
}

fn hash_file_prefix(file_path: &Path, length: i64) -> Result<i64, AppError> {
    let mut file = fs::File::open(file_path)
        .map_err(|error| AppError::Config(format!("无法打开文件: {error}")))?;
    let length = length.max(0);
    let mut hash = FNV1A_OFFSET_BASIS;
    update_fnv1a(&mut hash, &length.to_le_bytes());

    let sample_size = FILE_FINGERPRINT_SAMPLE_SIZE.min(length);
    let offsets = if length <= FILE_FINGERPRINT_SAMPLE_SIZE * 3 {
        vec![0]
    } else {
        vec![0, length / 2 - sample_size / 2, length - sample_size]
    };

    for offset in offsets {
        file.seek(SeekFrom::Start(offset as u64))
            .map_err(|error| AppError::Config(format!("定位文件指纹失败: {error}")))?;
        let bytes_to_read = if length <= FILE_FINGERPRINT_SAMPLE_SIZE * 3 {
            length as usize
        } else {
            sample_size as usize
        };
        let mut buffer = vec![0_u8; bytes_to_read];
        file.read_exact(&mut buffer)
            .map_err(|error| AppError::Config(format!("读取文件指纹失败: {error}")))?;
        update_fnv1a(&mut hash, &offset.to_le_bytes());
        update_fnv1a(&mut hash, &buffer);
    }

    Ok(hash as i64)
}

fn hash_entire_file_prefix(file_path: &Path, length: i64) -> Result<i64, AppError> {
    let file = fs::File::open(file_path)
        .map_err(|error| AppError::Config(format!("无法打开文件: {error}")))?;
    let length = length.max(0);
    let mut reader = file.take(length as u64);
    let mut buffer = [0_u8; 8192];
    let mut hash = FNV1A_OFFSET_BASIS;
    let mut bytes_read = 0_i64;

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| AppError::Config(format!("读取完整文件指纹失败: {error}")))?;
        if read == 0 {
            break;
        }
        update_fnv1a(&mut hash, &buffer[..read]);
        bytes_read += read as i64;
    }
    if bytes_read != length {
        return Err(AppError::Config(format!(
            "读取完整文件指纹失败: 文件在同步期间被截断 ({bytes_read}/{length})"
        )));
    }
    Ok(hash as i64)
}

fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

/// 获取 session_log_sync 表中某条目的同步进度。
///
/// Shared by all session_usage_* parsers.
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
    metadata_modified_nanos_checked(metadata).unwrap_or(0)
}

fn metadata_modified_nanos_checked(metadata: &fs::Metadata) -> Result<i64, AppError> {
    metadata
        .modified()
        .map_err(|error| AppError::Config(format!("无法读取文件修改时间: {error}")))?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| AppError::Config(format!("文件修改时间早于 UNIX epoch: {error}")))
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
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

/// 插入单条会话日志到 proxy_request_logs，返回是否成功插入 (true=新插入, false=已存在)
fn insert_session_log_entry(
    db: &Database,
    request_id: &str,
    msg: &ParsedAssistantUsage,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

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
    let should_skip = if msg.is_api_error {
        should_skip_api_error_insert(&conn, request_id, msg, created_at)?
    } else {
        should_skip_session_insert(&conn, request_id, &dedup_key)?
    };
    if should_skip {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        model: Some(msg.model.clone()),
        message_id: None,
    };

    let pricing = find_model_pricing_for_session(&conn, &msg.model);
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate(&usage, &p, multiplier);
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            )
        }
        None => (
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
        ),
    };

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
                0i64,               // latency_ms: 会话日志无此数据
                Option::<i64>::None, // first_token_ms
                msg.status_code as i64,
                msg.error_message,
                msg.session_id,
                Some("session_log"), // provider_type
                1i64,               // is_streaming: Claude Code 通常使用流式
                "1.0",              // cost_multiplier
                created_at,
                "session_log",      // data_source
            ],
        )
        .map_err(|e| AppError::Database(format!("插入会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

fn should_skip_api_error_insert(
    conn: &rusqlite::Connection,
    request_id: &str,
    msg: &ParsedAssistantUsage,
    created_at: i64,
) -> Result<bool, AppError> {
    let session_id = msg
        .session_id
        .as_deref()
        .filter(|session_id| !session_id.trim().is_empty());
    let request_exists = conn
        .prepare_cached("SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = ?1)")
        .and_then(|mut stmt| stmt.query_row([request_id], |row| row.get::<_, bool>(0)))
        .map_err(|error| AppError::Database(format!("查询 API 错误 request_id 失败: {error}")))?;
    if request_exists {
        return Ok(true);
    }
    let Some(session_id) = session_id else {
        return Ok(false);
    };

    let marker_prefix = CLAUDE_PROXY_ERROR_DEDUP_MARKER_PREFIX;
    let proxy_request_id = conn
        .prepare_cached(
            "SELECT l.request_id
             FROM proxy_request_logs l
             WHERE COALESCE(l.data_source, 'proxy') = 'proxy'
               AND l.app_type = 'claude'
               AND l.status_code = ?1
               AND l.session_id = ?2
               AND l.created_at = ?3
               AND NOT EXISTS (
                   SELECT 1 FROM session_log_sync s
                   WHERE s.file_path = ?4 || l.request_id
                     AND s.last_modified = 1
               )
             ORDER BY l.request_id
             LIMIT 1",
        )
        .and_then(|mut stmt| {
            stmt.query_row(
                rusqlite::params![
                    msg.status_code as i64,
                    session_id,
                    created_at,
                    marker_prefix,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
        })
        .map_err(|error| AppError::Database(format!("查询重复代理 API 错误失败: {error}")))?;

    if let Some(proxy_request_id) = proxy_request_id {
        update_sync_state_on_conn(conn, &format!("{marker_prefix}{proxy_request_id}"), 1, 1)?;
        Ok(true)
    } else {
        Ok(false)
    }
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
            status_code: 200,
            error_message: None,
            is_api_error: false,
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
            status_code: 200,
            error_message: None,
            is_api_error: false,
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
            status_code: 200,
            error_message: None,
            is_api_error: false,
            timestamp: Some("1970-01-01T00:16:45Z".to_string()),
            session_id: Some("session-1".to_string()),
        };

        let inserted = insert_session_log_entry(&db, "session:msg_1", &msg)?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_insert_api_error_does_not_use_token_heuristic_dedup() -> Result<(), AppError> {
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
                    "proxy-zero-success",
                    "openai-compatible",
                    "claude",
                    "<synthetic>",
                    "<synthetic>",
                    0,
                    0,
                    0,
                    0,
                    "0",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let msg = ParsedAssistantUsage {
            message_id: "msg_zero_403".to_string(),
            model: "<synthetic>".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            stop_reason: None,
            status_code: 403,
            error_message: Some("authentication_failed".to_string()),
            is_api_error: true,
            timestamp: Some("1970-01-01T00:16:40Z".to_string()),
            session_id: Some("session-error".to_string()),
        };

        assert!(
            insert_session_log_entry(&db, "session:msg_zero_403", &msg)?,
            "API 错误不能被零 token 的成功代理日志近似去重"
        );
        Ok(())
    }

    #[test]
    fn test_insert_api_error_skips_matching_proxy_error() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, error_message, session_id,
                    created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "proxy-403",
                    "openai-compatible",
                    "claude",
                    "deepseek-v4-flash-0731",
                    "deepseek-v4-flash-0731",
                    0,
                    0,
                    0,
                    0,
                    "0",
                    100,
                    403,
                    "authentication_failed",
                    "session-error",
                    1000,
                    "proxy"
                ],
            )?;
        }

        let msg = ParsedAssistantUsage {
            message_id: "msg_duplicate_403".to_string(),
            model: "<synthetic>".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            stop_reason: None,
            status_code: 403,
            error_message: Some("authentication_failed".to_string()),
            is_api_error: true,
            timestamp: Some("1970-01-01T00:16:40Z".to_string()),
            session_id: Some("session-error".to_string()),
        };

        assert!(
            !insert_session_log_entry(&db, "session:msg_duplicate_403", &msg)?,
            "代理已经记录同一会话的 403 时不能重复导入 session 错误"
        );

        let mut second_error = msg;
        second_error.message_id = "msg_second_403".to_string();
        assert!(
            insert_session_log_entry(&db, "session:msg_second_403", &second_error)?,
            "一条代理错误只能抵消一条 session 错误"
        );

        let mut unidentified = second_error;
        unidentified.message_id = "msg_empty_session_403".to_string();
        unidentified.session_id = Some(String::new());
        assert!(
            insert_session_log_entry(&db, "session:msg_empty_session_403", &unidentified)?,
            "空 session_id 不是可靠身份，不能把无关的 403 合并"
        );
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

        let (files, errors) = collect_jsonl_files(&tmp);
        assert!(errors.is_empty());
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

        let (files, errors) = collect_jsonl_files(&tmp);
        assert!(errors.is_empty());
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

        let (imported, _skipped) = sync_single_file(&db, &file)?;
        assert_eq!(
            imported, 1,
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

    #[test]
    fn test_sync_imports_claude_api_error_without_tokens() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("claude-api-error.jsonl");

        let api_error = r#"{"type":"assistant","message":{"id":"msg_api_error","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"stop_reason":"stop_sequence","content":[{"type":"text","text":"Please run /login · API Error: 403 Access to model denied."}]},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        fs::write(&file, format!("{api_error}\n")).unwrap();

        let metadata = fs::metadata(&file).unwrap();
        let modified = metadata_modified_nanos(&metadata);
        let file_len = metadata.len() as i64;
        let file_path = file.to_string_lossy();
        update_sync_state(&db, &file_path, modified, 1)?;
        update_sync_state(
            &db,
            &format!("{CLAUDE_FILE_HASH_MARKER_PREFIX}{file_path}"),
            hash_file_prefix(&file, file_len)?,
            file_len,
        )?;
        update_sync_state(
            &db,
            &format!("{CLAUDE_FILE_FULL_HASH_MARKER_PREFIX}{file_path}"),
            hash_entire_file_prefix(&file, file_len)?,
            unix_timestamp_seconds(),
        )?;
        update_sync_state(
            &db,
            &format!("{CLAUDE_FILE_STATE_MARKER_PREFIX}{file_path}"),
            modified,
            file_len,
        )?;
        assert_eq!(
            sync_single_file(&db, &file)?,
            (0, 0),
            "普通增量同步不会重读已推进的游标"
        );

        let (imported, _skipped) = sync_single_file_with_options(&db, &file, true)?;
        assert_eq!(imported, 1, "结构化 API 错误即使没有 token 也必须被导入");

        let conn = lock_conn!(db.conn);
        let (status, error): (i64, Option<String>) = conn.query_row(
            "SELECT status_code, error_message FROM proxy_request_logs WHERE request_id = 'session:msg_api_error'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(status, 403);
        assert_eq!(error.as_deref(), Some("authentication_failed"));
        drop(conn);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_api_error_backfill_marker_waits_for_all_files() -> Result<(), AppError> {
        let db = Database::memory()?;

        complete_api_error_backfill_if_successful(
            &db,
            true,
            &["temporarily unreadable.jsonl".to_string()],
        )?;
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0),
            "存在文件错误时必须保留回填待重试状态"
        );

        complete_api_error_backfill_if_successful(&db, true, &[])?;
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (1, 1),
            "所有文件扫描成功后才应完成回填"
        );

        Ok(())
    }

    #[test]
    fn test_stable_backfill_error_stops_repeated_force_scans() -> Result<(), AppError> {
        let db = Database::memory()?;
        let errors = ["permanently unreadable.jsonl".to_string()];

        complete_api_error_backfill_if_successful(&db, true, &errors)?;
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0),
            "第一次错误仍应保留一次强制重试"
        );

        complete_api_error_backfill_if_successful(&db, true, &errors)?;
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (1, 1),
            "相同永久错误不能导致每分钟全量回扫所有历史日志"
        );
        Ok(())
    }

    #[test]
    fn test_api_error_backfill_marker_waits_for_directory_scan_errors() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let not_a_directory = tmp.join("projects");
        fs::write(&not_a_directory, "not a directory").unwrap();

        let result = sync_claude_session_logs_from_projects_dir(&db, &not_a_directory)?;
        assert!(
            !result.errors.is_empty(),
            "目录枚举失败必须进入同步错误列表"
        );

        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0),
            "目录扫描不完整时必须保留回填待重试状态"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_optional_scan_path_that_is_not_a_directory_is_an_error() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let session_dir = tmp.join("project").join("session");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("subagents"), "not a directory").unwrap();

        let result = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            !result.errors.is_empty(),
            "存在但不是目录的可选扫描路径必须被报告，不能视为目录缺失"
        );
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0),
            "嵌套目录扫描失败时必须保留回填待重试状态"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_sync_does_not_advance_cursor_when_insert_fails() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        let api_error = r#"{"type":"assistant","message":{"id":"msg_insert_error","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        fs::write(&file, format!("{api_error}\n")).unwrap();

        {
            let conn = lock_conn!(db.conn);
            conn.execute_batch("DROP TABLE proxy_request_logs")?;
        }

        let result = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            !result.errors.is_empty(),
            "数据库插入失败必须进入顶层同步错误列表"
        );
        assert_eq!(
            get_sync_state(&db, &file.to_string_lossy())?,
            (0, 0),
            "插入失败后不能推进文件游标，后续同步必须能够重试"
        );
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0),
            "数据库插入失败时必须保留回填待重试状态"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_incomplete_jsonl_keeps_backfill_pending_until_retry() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        fs::write(
            &file,
            r#"{"type":"assistant","message":{"id":"msg_partial""#,
        )
        .unwrap();

        let first = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            !first.errors.is_empty(),
            "未写完的 JSONL 行必须使本次回填失败"
        );
        assert_eq!(
            get_sync_state(&db, &file.to_string_lossy())?,
            (0, 0),
            "未写完的行不能被文件游标越过"
        );
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0),
            "未写完的行必须让全量回填保持待重试"
        );

        let api_error = r#"{"type":"assistant","message":{"id":"msg_partial","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        fs::write(&file, format!("{api_error}\n")).unwrap();

        let retry = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert_eq!(retry.imported, 1, "文件写完后重试必须导入原来的 403");
        assert!(retry.errors.is_empty());
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (1, 1),
            "完整重试成功后才能完成回填"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_corrupt_complete_line_does_not_block_later_api_errors() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        let api_error = r#"{"type":"assistant","message":{"id":"msg_after_corrupt","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        fs::write(&file, format!("{{corrupt-json}}\n{api_error}\n")).unwrap();

        let result = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            result.errors.is_empty(),
            "有换行边界的永久损坏记录应被跳过，不能卡住整个文件"
        );
        assert_eq!(result.imported, 1, "损坏记录后的有效 403 必须继续导入");
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (1, 1),
            "跳过确定损坏的记录后应能完成回填"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_invalid_utf8_complete_line_does_not_block_later_api_errors() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        let api_error = r#"{"type":"assistant","message":{"id":"msg_after_invalid_utf8","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        let mut content = vec![0xff, b'\n'];
        content.extend_from_slice(api_error.as_bytes());
        content.push(b'\n');
        fs::write(&file, content).unwrap();

        let result = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            result.errors.is_empty(),
            "有换行边界的非法 UTF-8 记录应被跳过"
        );
        assert_eq!(result.imported, 1, "非法 UTF-8 记录后的有效 403 必须导入");
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (1, 1)
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_stable_corrupt_tail_does_not_block_future_appends() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        let corrupt_tail = r#"{"type":"assistant","message":{"id":"abandoned""#;
        fs::write(&file, corrupt_tail).unwrap();

        let first = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(!first.errors.is_empty(), "首次观察坏尾行时必须等待重试");
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (0, 0)
        );

        let stable_retry = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            stable_retry.errors.is_empty(),
            "文件保持不变后应把坏尾行认定为永久损坏"
        );
        assert_eq!(
            get_sync_state(&db, CLAUDE_API_ERROR_BACKFILL_MARKER)?,
            (1, 1),
            "稳定的坏尾行不能让全局回填永久 pending"
        );

        let original_modified = fs::metadata(&file).unwrap().modified().unwrap();
        let api_error = r#"{"type":"assistant","message":{"id":"msg_after_stable_tail","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        fs::write(&file, format!("{corrupt_tail}\n{api_error}\n")).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original_modified))
            .unwrap();

        let appended = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(appended.errors.is_empty());
        assert_eq!(
            appended.imported, 1,
            "坏尾行稳定后仍必须保留游标，以导入未来追加的 403"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_truncated_file_restarts_before_importing_new_api_error() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        let old_one = r#"{"type":"assistant","message":{"id":"msg_before_truncate_1","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        let old_two = r#"{"type":"assistant","message":{"id":"msg_before_truncate_2","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:45Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        let corrupt_tail = r#"{"type":"assistant","message":{"id":"abandoned""#;
        fs::write(&file, format!("{old_one}\n{old_two}\n{corrupt_tail}")).unwrap();

        let first = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(!first.errors.is_empty());
        let stable_retry = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(stable_retry.errors.is_empty());
        assert_eq!(stable_retry.imported, 2);

        std::thread::sleep(std::time::Duration::from_millis(20));
        let replacement = r#"{"type":"assistant","message":{"id":"msg_after_truncate","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:46Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}"#;
        fs::write(&file, format!("{replacement}\n")).unwrap();

        let truncated = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(truncated.errors.is_empty());
        assert_eq!(
            truncated.imported, 1,
            "日志截断或替换后必须从文件开头导入新的 403"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_longer_replacement_restarts_before_importing_new_api_error() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("claude-api-error.jsonl");
        let old = r#"{"type":"assistant","message":{"id":"msg_old_file","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"old"}"#;
        fs::write(&file, format!("{old}\n")).unwrap();
        assert_eq!(sync_single_file(&db, &file)?.0, 1);

        std::thread::sleep(std::time::Duration::from_millis(20));
        let replacement = r#"{"type":"assistant","message":{"id":"msg_new_first_line","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:45Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"new"}"#;
        let padding = r#"{"type":"system","sessionId":"session-error","padding":"this replacement is intentionally longer than the previous file so length alone cannot identify an append"}"#;
        fs::write(&file, format!("{replacement}\n{padding}\n")).unwrap();

        let (imported, _) = sync_single_file(&db, &file)?;
        assert_eq!(
            imported, 1,
            "更长的替换文件也必须通过前缀指纹识别并从头扫描"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_legacy_cursor_does_not_hide_same_mtime_append() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("claude-api-error.jsonl");
        let old = r#"{"type":"assistant","message":{"id":"msg_legacy_old","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"old"}"#;
        fs::write(&file, format!("{old}\n")).unwrap();
        assert_eq!(sync_single_file(&db, &file)?.0, 1);
        let original_modified = fs::metadata(&file).unwrap().modified().unwrap();

        let file_path = file.to_string_lossy();
        update_sync_state(
            &db,
            &format!("{CLAUDE_FILE_STATE_MARKER_PREFIX}{file_path}"),
            0,
            0,
        )?;
        update_sync_state(
            &db,
            &format!("{CLAUDE_FILE_HASH_MARKER_PREFIX}{file_path}"),
            0,
            0,
        )?;

        let appended = r#"{"type":"assistant","message":{"id":"msg_legacy_new","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:45Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"new"}"#;
        fs::write(&file, format!("{old}\n{appended}\n")).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original_modified))
            .unwrap();

        assert_eq!(
            sync_single_file(&db, &file)?.0,
            1,
            "旧版游标没有文件指纹时必须保守重扫，不能吞掉相同 mtime 的追加"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_same_metadata_replacement_is_verified_by_content_hash() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("claude-api-error.jsonl");
        let old = r#"{"type":"assistant","message":{"id":"msg_equal_old","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"old"}"#;
        let replacement = r#"{"type":"assistant","message":{"id":"msg_equal_new","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"new"}"#;
        assert_eq!(old.len(), replacement.len());
        fs::write(&file, format!("{old}\n")).unwrap();
        assert_eq!(sync_single_file(&db, &file)?.0, 1);
        let original_modified = fs::metadata(&file).unwrap().modified().unwrap();

        fs::write(&file, format!("{replacement}\n")).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original_modified))
            .unwrap();

        assert_eq!(
            sync_single_file(&db, &file)?.0,
            1,
            "mtime 和长度都相同也必须校验内容指纹"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_equal_metadata_corrupt_tail_requires_new_observation() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project_dir = tmp.join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let file = project_dir.join("claude-api-error.jsonl");
        let first_tail = r#"{"type":"assistant","message":{"id":"tail_old_x""#;
        let second_tail = r#"{"type":"assistant","message":{"id":"tail_new_y""#;
        assert_eq!(first_tail.len(), second_tail.len());
        fs::write(&file, first_tail).unwrap();
        let original_modified = fs::metadata(&file).unwrap().modified().unwrap();

        let first = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(!first.errors.is_empty());

        fs::write(&file, second_tail).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original_modified))
            .unwrap();

        let changed = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(
            !changed.errors.is_empty(),
            "内容不同的坏尾即使 mtime 和长度相同，也应重新进行首次观察"
        );
        let stable = sync_claude_session_logs_from_projects_dir(&db, &tmp)?;
        assert!(stable.errors.is_empty());

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_periodic_full_hash_detects_change_outside_samples() -> Result<(), AppError> {
        fn large_api_error(message_id: &str) -> String {
            format!(
                r#"{{"type":"assistant","padding_a":"{}","message":{{"id":"{message_id}","model":"<synthetic>","usage":{{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}},"padding_b":"{}","timestamp":"2026-08-12T07:34:44Z","sessionId":"session-error","isApiErrorMessage":true,"apiErrorStatus":403,"error":"authentication_failed"}}"#,
                "a".repeat(6000),
                "z".repeat(24000)
            )
        }

        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("claude-api-error.jsonl");
        let old = large_api_error("msg_sample_old");
        let replacement = large_api_error("msg_sample_new");
        assert_eq!(old.len(), replacement.len());
        fs::write(&file, format!("{old}\n")).unwrap();
        assert_eq!(sync_single_file(&db, &file)?.0, 1);
        let original_modified = fs::metadata(&file).unwrap().modified().unwrap();

        fs::write(&file, format!("{replacement}\n")).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(original_modified))
            .unwrap();

        let full_hash_marker = format!(
            "{CLAUDE_FILE_FULL_HASH_MARKER_PREFIX}{}",
            file.to_string_lossy()
        );
        let (old_full_hash, _) = get_sync_state(&db, &full_hash_marker)?;
        update_sync_state(&db, &full_hash_marker, old_full_hash, 0)?;

        assert_eq!(
            sync_single_file(&db, &file)?.0,
            1,
            "采样窗口之外的等元数据替换也必须在周期全量校验时被发现"
        );

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }
}
