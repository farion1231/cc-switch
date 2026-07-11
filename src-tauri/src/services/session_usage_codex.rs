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
//! - `turn_context` → 提取当前 model
//! - `event_msg` (type=token_count) → 提取累计 token 用量，计算 delta

use crate::codex_config::get_codex_config_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const CODEX_SUBAGENT_USAGE_MIGRATION_KEY: &str = "codex_subagent_usage_thread_id_v1_migrated";
const CODEX_SUBAGENT_REBUILT_SESSION_KEY_PREFIX: &str =
    "codex_subagent_usage_thread_id_v1_rebuilt:";
const CODEX_SUBAGENT_SYNC_KEY_SUFFIX: &str = "#codex-thread-id-v1";

/// 累计 token 用量（跟踪 total_token_usage 字段）
#[derive(Debug, Clone, Default)]
struct CumulativeTokens {
    input: u64,
    cached_input: u64,
    output: u64,
}

/// 单次 API 调用的 token 增量
#[derive(Debug)]
struct DeltaTokens {
    input: u32,
    cached_input: u32,
    output: u32,
}

impl DeltaTokens {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.cached_input == 0 && self.output == 0
    }
}

/// 单文件解析时的运行状态
struct FileParseState {
    thread_id: Option<String>,
    current_model: String,
    prev_total: Option<CumulativeTokens>,
    event_index: u32,
    seen_session_meta: bool,
    history_replay_boundary: Option<i64>,
}

/// Codex 的现代子代理日志同时包含两个不同含义的 ID：
///
/// - `id`: 当前日志文件对应的唯一线程 ID；
/// - `session_id`: 父线程 ID（主线程中两者相同）。
///
/// request_id 必须使用 thread_id，否则同一父线程下各文件从 1 开始的
/// event_index 会互相碰撞，并被数据库的唯一约束当作重复记录丢弃。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexSessionIdentity {
    thread_id: String,
    session_id: String,
    carries_history_snapshot: bool,
}

impl CodexSessionIdentity {
    fn is_subagent(&self) -> bool {
        self.thread_id != self.session_id
    }
}

fn parse_codex_session_identity(payload: &serde_json::Value) -> Option<CodexSessionIdentity> {
    let thread_id = payload
        .get("id")
        .or_else(|| payload.get("thread_id"))
        .or_else(|| payload.get("threadId"))
        .or_else(|| payload.get("session_id"))
        .or_else(|| payload.get("sessionId"))
        .and_then(|value| value.as_str())?
        .to_string();

    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| thread_id.clone());
    let carries_history_snapshot = payload
        .get("forked_from_id")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty())
        || payload
            .get("source")
            .and_then(|source| source.get("subagent"))
            .is_some()
        || thread_id != session_id;

    Some(CodexSessionIdentity {
        thread_id,
        session_id,
        carries_history_snapshot,
    })
}

fn read_codex_session_identity(file_path: &Path) -> Option<CodexSessionIdentity> {
    let file = fs::File::open(file_path).ok()?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        if !line.contains("\"session_meta\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|value| value.as_str()) != Some("session_meta") {
            continue;
        }
        if let Some(identity) = value.get("payload").and_then(parse_codex_session_identity) {
            return Some(identity);
        }
    }

    None
}

fn codex_sync_state_key(file_path: &str, identity: Option<&CodexSessionIdentity>) -> String {
    if identity.is_some_and(CodexSessionIdentity::is_subagent) {
        // 旧版本用父 session_id 生成 request_id。为子代理使用新的同步键，
        // 可让升级后的首次同步重新读取已经到达 EOF 的历史子代理文件。
        format!("{file_path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}")
    } else {
        file_path.to_string()
    }
}

fn session_meta_carries_history_snapshot(payload: &serde_json::Value) -> bool {
    let has_fork_parent = payload
        .get("forked_from_id")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty());
    let is_subagent = payload
        .get("source")
        .and_then(|source| source.get("subagent"))
        .is_some()
        || parse_codex_session_identity(payload)
            .is_some_and(|identity| identity.carries_history_snapshot);

    has_fork_parent || is_subagent
}

/// fork/restore 日志会先重放父线程历史，再以明确的接管事件开始子线程。
/// 返回接管事件所在行；该行之前的 token_count 只用于恢复累计值基线。
fn codex_history_replay_boundary(
    file_path: &Path,
    identity: Option<&CodexSessionIdentity>,
) -> Option<i64> {
    if identity.is_some_and(|identity| !identity.carries_history_snapshot) {
        return None;
    }

    let file = fs::File::open(file_path).ok()?;
    let reader = BufReader::new(file);
    let mut carries_history_snapshot =
        identity.is_some_and(|identity| identity.carries_history_snapshot);

    for (index, line) in reader.lines().enumerate() {
        let Ok(line) = line else {
            continue;
        };
        if !line.contains("\"session_meta\"")
            && !line.contains("\"thread_settings_applied\"")
            && !line.contains("\"inter_agent_communication")
        {
            continue;
        }

        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(event_type) = value.get("type").and_then(|value| value.as_str()) else {
            continue;
        };

        if event_type == "session_meta" {
            if let Some(payload) = value.get("payload") {
                carries_history_snapshot |= session_meta_carries_history_snapshot(payload);
            }
            continue;
        }

        let is_replay_boundary = event_type.starts_with("inter_agent_communication")
            || (event_type == "event_msg"
                && value
                    .get("payload")
                    .and_then(|payload| payload.get("type"))
                    .and_then(|value| value.as_str())
                    == Some("thread_settings_applied"));
        if carries_history_snapshot && is_replay_boundary {
            return Some(index as i64 + 1);
        }
    }

    None
}

fn is_history_snapshot_event(state: &FileParseState, line_offset: i64) -> bool {
    state
        .history_replay_boundary
        .is_some_and(|boundary| line_offset < boundary)
}

struct CodexRollupDateCoverage {
    dates: HashSet<String>,
    unknown: bool,
}

impl CodexRollupDateCoverage {
    fn from_file(file_path: &Path) -> Self {
        match codex_file_rollup_date_candidates(file_path) {
            Some(dates) => Self {
                dates,
                unknown: false,
            },
            None => Self {
                dates: HashSet::new(),
                unknown: true,
            },
        }
    }

    fn may_overlap(&self, rollup_dates: &HashSet<String>) -> bool {
        !rollup_dates.is_empty()
            && (self.unknown || self.dates.iter().any(|date| rollup_dates.contains(date)))
    }
}

fn codex_file_rollup_date_candidates(file_path: &Path) -> Option<HashSet<String>> {
    let file = fs::File::open(file_path).ok()?;
    let mut dates = HashSet::new();

    for line in BufReader::new(file).lines() {
        let line = line.ok()?;
        if !line.contains("\"token_count\"") {
            continue;
        }

        let value = serde_json::from_str::<serde_json::Value>(&line).ok()?;
        let is_usage_event = value.get("type").and_then(|value| value.as_str())
            == Some("event_msg")
            && value
                .get("payload")
                .and_then(|payload| payload.get("type"))
                .and_then(|value| value.as_str())
                == Some("token_count")
            && value
                .get("payload")
                .and_then(|payload| payload.get("info"))
                .is_some_and(|info| !info.is_null());
        if !is_usage_event {
            continue;
        }

        let timestamp = value.get("timestamp").and_then(|value| value.as_str())?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
        // rollup 只保留当时的本地日期，无法得知当时的时区。任意合法时区下，
        // 一个时间点的本地日期只可能落在其 UTC 日期的前一天、当天或后一天。
        // 检查这三个日期可避免用户在 rollup 后切换时区时漏判并重复导入。
        let utc_date = timestamp.with_timezone(&chrono::Utc).date_naive();
        for date in [utc_date.pred_opt(), Some(utc_date), utc_date.succ_opt()]
            .into_iter()
            .flatten()
        {
            dates.insert(date.format("%Y-%m-%d").to_string());
        }
    }

    Some(dates)
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
            output: current.output as u32,
        },
        Some(p) => DeltaTokens {
            input: current.input.saturating_sub(p.input) as u32,
            cached_input: current.cached_input.saturating_sub(p.cached_input) as u32,
            output: current.output.saturating_sub(p.output) as u32,
        },
    }
}

/// 从 JSON Value 中提取累计 token 用量
fn parse_cumulative_tokens(total_usage: &serde_json::Value) -> Option<CumulativeTokens> {
    if total_usage.is_null() || !total_usage.is_object() {
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
        output: total_usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

/// 同步 Codex 使用数据（从 JSONL 会话日志）
pub fn sync_codex_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let codex_dir = get_codex_config_dir();

    let files = collect_codex_session_files(&codex_dir);
    let files_with_identity: Vec<(PathBuf, Option<CodexSessionIdentity>)> = files
        .into_iter()
        .map(|path| {
            let identity = read_codex_session_identity(&path);
            (path, identity)
        })
        .collect();

    repair_legacy_codex_subagent_usage(db, &files_with_identity)?;

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files_with_identity.len() as u32,
        errors: vec![],
    };

    if files_with_identity.is_empty() {
        return Ok(result);
    }

    for (file_path, identity) in &files_with_identity {
        match sync_single_codex_file(db, file_path, identity.as_ref()) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Codex 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[CODEX-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[CODEX-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 修复旧版以父 session_id 作为 request_id 命名空间造成的历史碰撞。
///
/// 仅在会话日志日期未与 Codex 汇总日期重叠时，处理已经被旧同步器读取过、
/// 且父线程日志仍可从磁盘重建的会话：
/// 1. 删除该父线程下无法区分主/子代理来源的旧 Codex session 行；
/// 2. 清除相关文件的同步游标；
/// 3. 当前同步轮次会用唯一 thread_id 重建准确记录。
///
/// `usage_daily_rollups` 没有 session 维度，已有汇总行时无法只撤销受影响会话。
/// 此时保留历史统计、迁移子代理游标并隔离旧 request_id，避免全量重导造成
/// 永久双计，也避免旧子代理记录阻塞父线程后续新增用量。
fn repair_legacy_codex_subagent_usage(
    db: &Database,
    files: &[(PathBuf, Option<CodexSessionIdentity>)],
) -> Result<(), AppError> {
    let (already_migrated, synced_subagent_paths) = {
        let conn = lock_conn!(db.conn);
        let already_migrated = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1 AND value = 'true')",
                rusqlite::params![CODEX_SUBAGENT_USAGE_MIGRATION_KEY],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|e| AppError::Database(format!("查询 Codex 子代理用量迁移状态失败: {e}")))?;
        let mut stmt = conn
            .prepare(
                "SELECT EXISTS(
                    SELECT 1 FROM session_log_sync
                    WHERE file_path = ?1 AND last_line_offset > 0
                )",
            )
            .map_err(|e| AppError::Database(format!("准备 Codex 子代理同步状态查询失败: {e}")))?;
        let mut synced_paths = Vec::new();

        for (path, identity) in files {
            let Some(identity) = identity else {
                continue;
            };
            if !identity.is_subagent() {
                continue;
            }

            let path = path.to_string_lossy();
            let was_synced = stmt
                .query_row(rusqlite::params![path.as_ref()], |row| {
                    row.get::<_, bool>(0)
                })
                .map_err(|e| AppError::Database(format!("查询 Codex 子代理旧同步状态失败: {e}")))?;
            if was_synced {
                synced_paths.push((path.into_owned(), identity.session_id.clone()));
            }
        }

        (already_migrated, synced_paths)
    };

    if synced_subagent_paths.is_empty() {
        // 空目录可能只是 CODEX_HOME 暂时不可用，不能永久关闭后续迁移。
        if !already_migrated && !files.is_empty() {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, 'true')",
                rusqlite::params![CODEX_SUBAGENT_USAGE_MIGRATION_KEY],
            )
            .map_err(|e| AppError::Database(format!("保存 Codex 子代理用量迁移状态失败: {e}")))?;
        }
        return Ok(());
    }

    let candidate_session_ids: HashSet<&str> = synced_subagent_paths
        .iter()
        .map(|(_, session_id)| session_id.as_str())
        .collect();
    let file_rollup_coverage: HashMap<PathBuf, CodexRollupDateCoverage> = files
        .iter()
        .filter(|(_, identity)| {
            identity.as_ref().is_some_and(|identity| {
                candidate_session_ids.contains(identity.thread_id.as_str())
                    || candidate_session_ids.contains(identity.session_id.as_str())
            })
        })
        .map(|(path, _)| (path.clone(), CodexRollupDateCoverage::from_file(path)))
        .collect();

    let rebuildable_thread_ids: HashSet<&str> = files
        .iter()
        .filter_map(|(_, identity)| {
            identity
                .as_ref()
                .map(|identity| identity.thread_id.as_str())
        })
        .collect();

    let mut conn = lock_conn!(db.conn);
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启 Codex 子代理用量修复事务失败: {e}")))?;

    // 文件扫描期间没有持有数据库锁。重新加锁后再确认旧游标仍存在，避免并发
    // 同步已经完成迁移时重复处理同一文件。
    let active_synced_subagent_paths = {
        let mut stmt = tx
            .prepare(
                "SELECT EXISTS(
                    SELECT 1 FROM session_log_sync
                    WHERE file_path = ?1 AND last_line_offset > 0
                )",
            )
            .map_err(|e| AppError::Database(format!("准备 Codex 子代理同步状态复查失败: {e}")))?;
        let mut active_paths = Vec::new();
        for (path, session_id) in synced_subagent_paths {
            let was_synced = stmt
                .query_row(rusqlite::params![path], |row| row.get::<_, bool>(0))
                .map_err(|e| AppError::Database(format!("复查 Codex 子代理旧同步状态失败: {e}")))?;
            if was_synced {
                active_paths.push((path, session_id));
            }
        }
        active_paths
    };

    if active_synced_subagent_paths.is_empty() {
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Codex 子代理用量修复失败: {e}")))?;
        return Ok(());
    }

    let migration_became_active = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1 AND value = 'true')",
            rusqlite::params![CODEX_SUBAGENT_USAGE_MIGRATION_KEY],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| AppError::Database(format!("复查 Codex 子代理迁移状态失败: {e}")))?;

    let codex_rollup_dates = tx
        .prepare("SELECT DISTINCT date FROM usage_daily_rollups WHERE app_type = 'codex'")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<HashSet<_>, _>>()
        })
        .map_err(|e| AppError::Database(format!("查询 Codex 历史汇总日期失败: {e}")))?;

    let synced_session_ids: HashSet<&str> = active_synced_subagent_paths
        .iter()
        .map(|(_, session_id)| session_id.as_str())
        .collect();
    let rebuilt_session_ids = {
        let mut stmt = tx
            .prepare("SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1 AND value = 'true')")
            .map_err(|e| AppError::Database(format!("准备 Codex 已重建会话查询失败: {e}")))?;
        let mut rebuilt = HashSet::new();
        for session_id in &synced_session_ids {
            let key = format!("{CODEX_SUBAGENT_REBUILT_SESSION_KEY_PREFIX}{session_id}");
            let was_rebuilt = stmt
                .query_row([key], |row| row.get::<_, bool>(0))
                .map_err(|e| AppError::Database(format!("查询 Codex 已重建会话失败: {e}")))?;
            if was_rebuilt {
                rebuilt.insert((*session_id).to_string());
            }
        }
        rebuilt
    };
    let mut affected_session_ids = HashSet::new();
    let mut late_rebuild_subagent_paths = HashSet::new();
    let mut preserved_session_ids = HashSet::new();
    let mut preserved_synced_subagent_paths = HashSet::new();

    if !migration_became_active {
        for session_id in synced_session_ids {
            if !rebuildable_thread_ids.contains(session_id) {
                continue;
            }

            let overlaps_rollup = files.iter().any(|(path, identity)| {
                identity.as_ref().is_some_and(|identity| {
                    (identity.thread_id == session_id || identity.session_id == session_id)
                        && file_rollup_coverage
                            .get(path)
                            .is_none_or(|coverage| coverage.may_overlap(&codex_rollup_dates))
                })
            });
            if !overlaps_rollup {
                affected_session_ids.insert(session_id.to_string());
            }
        }
    }

    for (path, session_id) in active_synced_subagent_paths {
        if affected_session_ids.contains(&session_id) {
            continue;
        }
        if rebuilt_session_ids.contains(&session_id) {
            let overlaps_rollup = file_rollup_coverage
                .get(Path::new(&path))
                .is_none_or(|coverage| coverage.may_overlap(&codex_rollup_dates));
            if overlaps_rollup {
                // 该文件的历史可能已经进入无 session 维度的日汇总，不能从头补回。
                // 只迁移旧游标，后续仍可安全导入 offset 之后的新事件。
                preserved_synced_subagent_paths.insert(path);
            } else {
                late_rebuild_subagent_paths.insert(path);
            }
        } else {
            preserved_session_ids.insert(session_id);
            preserved_synced_subagent_paths.insert(path);
        }
    }

    for session_id in &affected_session_ids {
        tx.execute(
            "DELETE FROM proxy_request_logs
             WHERE data_source = 'codex_session' AND session_id = ?1",
            rusqlite::params![session_id],
        )
        .map_err(|e| AppError::Database(format!("清理旧 Codex 子代理碰撞记录失败: {e}")))?;
    }

    // 保留历史统计时不能继续让旧记录占用父线程的 request_id 命名空间。
    // 旧子代理可能抢先写入 parent:N；父线程未来增长到同一个 N 时会被误判为
    // 重复记录。仅重命名主键，不改变任何用量字段或汇总结果。
    for session_id in &preserved_session_ids {
        tx.execute(
            "UPDATE proxy_request_logs
             SET request_id = 'codex_session:legacy:' || rowid || ':' || request_id
             WHERE data_source = 'codex_session' AND session_id = ?1
               AND request_id NOT LIKE 'codex_session:legacy:%'",
            rusqlite::params![session_id],
        )
        .map_err(|e| AppError::Database(format!("迁移保留的 Codex 旧请求 ID 失败: {e}")))?;
    }

    if !affected_session_ids.is_empty() {
        for (path, identity) in files {
            let Some(identity) = identity else {
                continue;
            };
            if !affected_session_ids.contains(&identity.thread_id)
                && !affected_session_ids.contains(&identity.session_id)
            {
                continue;
            }

            let path = path.to_string_lossy();
            let subagent_key = format!("{path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
            tx.execute(
                "DELETE FROM session_log_sync WHERE file_path = ?1 OR file_path = ?2",
                rusqlite::params![path.as_ref(), subagent_key],
            )
            .map_err(|e| AppError::Database(format!("重置 Codex 子代理同步游标失败: {e}")))?;
        }

        for session_id in &affected_session_ids {
            let key = format!("{CODEX_SUBAGENT_REBUILT_SESSION_KEY_PREFIX}{session_id}");
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, 'true')",
                [key],
            )
            .map_err(|e| AppError::Database(format!("记录 Codex 已重建会话失败: {e}")))?;
        }
    }

    // 同一父会话已完成精确重建时，迟到的兄弟子代理旧记录已随父命名空间
    // 一并删除。清除该文件的新旧游标，让它从头按唯一 thread_id 补回历史。
    for path in &late_rebuild_subagent_paths {
        let subagent_key = format!("{path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        tx.execute(
            "DELETE FROM session_log_sync WHERE file_path = ?1 OR file_path = ?2",
            rusqlite::params![path, subagent_key],
        )
        .map_err(|e| AppError::Database(format!("重置迟到 Codex 子代理同步游标失败: {e}")))?;
    }

    // 父日志已不存在，或历史已进入无 session 维度的 rollup 时，无法安全重建
    // 旧记录。保留旧统计，把旧游标复制到 thread_id 同步键，再删除 plain-path
    // 游标作为该文件已处理的标记。以后恢复的遗漏文件仍可独立触发同一迁移。
    for path in &preserved_synced_subagent_paths {
        let subagent_key = format!("{path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        tx.execute(
            "INSERT INTO session_log_sync (
                file_path, last_modified, last_line_offset, last_synced_at
             )
             SELECT ?2, last_modified, last_line_offset, last_synced_at
             FROM session_log_sync WHERE file_path = ?1
             ON CONFLICT(file_path) DO UPDATE SET
                last_modified = MAX(last_modified, excluded.last_modified),
                last_line_offset = MAX(last_line_offset, excluded.last_line_offset),
                last_synced_at = MAX(last_synced_at, excluded.last_synced_at)",
            rusqlite::params![path, subagent_key],
        )
        .map_err(|e| AppError::Database(format!("迁移保留的 Codex 子代理同步游标失败: {e}")))?;
        tx.execute("DELETE FROM session_log_sync WHERE file_path = ?1", [path])
            .map_err(|e| AppError::Database(format!("清理 Codex 子代理旧同步游标失败: {e}")))?;
    }

    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, 'true')",
        rusqlite::params![CODEX_SUBAGENT_USAGE_MIGRATION_KEY],
    )
    .map_err(|e| AppError::Database(format!("保存 Codex 子代理用量迁移状态失败: {e}")))?;

    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Codex 子代理用量修复失败: {e}")))?;

    if !affected_session_ids.is_empty() {
        log::info!(
            "[CODEX-SYNC] 已重置 {} 个存在子代理 request_id 碰撞的历史会话",
            affected_session_ids.len()
        );
    }
    if !codex_rollup_dates.is_empty() && !preserved_synced_subagent_paths.is_empty() {
        log::info!(
            "[CODEX-SYNC] 检测到历史汇总，已保留旧统计并迁移 {} 个子代理同步游标",
            preserved_synced_subagent_paths.len()
        );
    }
    if !late_rebuild_subagent_paths.is_empty() {
        log::info!(
            "[CODEX-SYNC] 已重置 {} 个迟到子代理游标以补回历史用量",
            late_rebuild_subagent_paths.len()
        );
    }

    Ok(())
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
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    files.push(path);
                }
            }
        }
    }

    files
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
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

/// 同步单个 Codex JSONL 文件，返回 (imported, skipped)
fn sync_single_codex_file(
    db: &Database,
    file_path: &Path,
    identity: Option<&CodexSessionIdentity>,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let sync_state_key = codex_sync_state_key(&file_path_str, identity);

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 检查同步状态
    let (mut last_modified, mut last_offset) = get_sync_state(db, &sync_state_key)?;
    if identity.is_some_and(CodexSessionIdentity::is_subagent) {
        let legacy_state = get_sync_state(db, &file_path_str)?;
        if legacy_state.1 > last_offset {
            // 迁移可能在日志文件暂时缺失时完成。suffix 游标不存在时始终继承
            // plain-path 旧游标，避免文件恢复后从头重导造成重复统计。
            update_sync_state(db, &sync_state_key, legacy_state.0, legacy_state.1)?;
            (last_modified, last_offset) = legacy_state;
        }
    }

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 打开文件逐行解析
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);
    let history_replay_boundary = codex_history_replay_boundary(file_path, identity);
    if identity.is_some_and(|identity| identity.carries_history_snapshot)
        && history_replay_boundary.is_none()
    {
        log::debug!(
            "[CODEX-SYNC] fork/子代理日志未发现历史接管边界: {}",
            file_path.display()
        );
    }

    let mut state = FileParseState {
        thread_id: identity.map(|identity| identity.thread_id.clone()),
        current_model: "unknown".to_string(),
        prev_total: None,
        event_index: 0,
        seen_session_meta: false,
        history_replay_boundary,
    };

    let mut line_offset: i64 = 0;
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    for line_result in reader.lines() {
        line_offset += 1;

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue, // 容忍不完整的最后一行
        };

        if line.trim().is_empty() {
            continue;
        }

        // 快速过滤：在 JSON 反序列化前跳过无关行
        let is_event_msg = line.contains("\"event_msg\"");
        let is_turn_context = line.contains("\"turn_context\"");
        let is_session_meta = line.contains("\"session_meta\"");

        if !is_event_msg && !is_turn_context && !is_session_meta {
            continue;
        }
        if is_event_msg && !line.contains("\"token_count\"") {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let event_type = match value.get("type").and_then(|t| t.as_str()) {
            Some(t) => t,
            None => continue,
        };

        match event_type {
            "session_meta" if !state.seen_session_meta => {
                state.seen_session_meta = true;
                if let Some(payload) = value.get("payload") {
                    if state.thread_id.is_none() {
                        state.thread_id = parse_codex_session_identity(payload)
                            .map(|identity| identity.thread_id);
                    }
                }
            }
            "turn_context" => {
                if let Some(payload) = value.get("payload") {
                    // model 可能在 payload.model 或 payload.info.model
                    if let Some(model) = payload
                        .get("model")
                        .or_else(|| payload.get("info").and_then(|info| info.get("model")))
                        .and_then(|v| v.as_str())
                    {
                        state.current_model = normalize_codex_model(model);
                    }
                }
            }
            "event_msg" => {
                let payload = match value.get("payload") {
                    Some(p) => p,
                    None => continue,
                };

                // 只处理 token_count 类型
                if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                    continue;
                }

                let info = match payload.get("info") {
                    Some(i) if !i.is_null() => i,
                    _ => continue, // 跳过 info 为 null 的首个事件
                };

                // 提取模型（token_count 事件也可能携带 model）
                if let Some(model) = info
                    .get("model")
                    .or_else(|| info.get("model_name"))
                    .or_else(|| payload.get("model"))
                    .and_then(|v| v.as_str())
                {
                    state.current_model = normalize_codex_model(model);
                }

                // 优先用 total_token_usage（累计值），fallback 到 last_token_usage（增量值）
                let (cumulative, is_total) = if let Some(total) = info.get("total_token_usage") {
                    (parse_cumulative_tokens(total), true)
                } else if let Some(last) = info.get("last_token_usage") {
                    (parse_cumulative_tokens(last), false)
                } else {
                    continue;
                };

                let cumulative = match cumulative {
                    Some(c) => c,
                    None => continue,
                };

                let delta = if is_total {
                    // 累计值模式：计算与上次的 delta
                    let d = compute_delta(&state.prev_total, &cumulative);
                    state.prev_total = Some(cumulative);
                    d
                } else {
                    // 增量值模式：直接使用 last_token_usage 的值
                    DeltaTokens {
                        input: cumulative.input as u32,
                        cached_input: cumulative.cached_input as u32,
                        output: cumulative.output as u32,
                    }
                };

                // 钳制：cached 不应超过 input（防护异常数据）
                let delta = DeltaTokens {
                    cached_input: delta.cached_input.min(delta.input),
                    ..delta
                };

                if delta.is_zero() {
                    continue; // 跳过 task 边界的零 delta 事件
                }

                // request_id 的序号沿用旧解析器定义：每个非零 token delta 都占一位。
                // replay 虽不计费，也必须占号，否则升级后追加事件会撞上旧主键。
                state.event_index += 1;

                let timestamp = value
                    .get("timestamp")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

                // 子代理/fork 文件开头可能重放父线程累计 token 历史。这里仍然
                // 保留上方 prev_total 的更新，用它作为后续真实调用的 delta 基线，
                // 但不为快照本身生成消费记录。
                if is_history_snapshot_event(&state, line_offset) {
                    if line_offset > last_offset {
                        skipped += 1;
                    }
                    continue;
                }

                // 跳过已处理的行（但仍需解析以恢复状态）
                if line_offset <= last_offset {
                    continue;
                }

                // 生成唯一 request_id
                let thread_id = state.thread_id.as_deref().unwrap_or("unknown");
                let request_id = format!("codex_session:{thread_id}:{}", state.event_index);

                match insert_codex_session_entry(
                    db,
                    &request_id,
                    &delta,
                    &state.current_model,
                    state.thread_id.as_deref(),
                    timestamp.as_deref(),
                ) {
                    Ok(true) => imported += 1,
                    Ok(false) => skipped += 1,
                    Err(e) => {
                        log::warn!("[CODEX-SYNC] 插入失败 ({}): {e}", request_id);
                        skipped += 1;
                    }
                }
            }
            _ => {}
        }
    }

    // 更新同步状态
    update_sync_state(db, &sync_state_key, file_modified, line_offset)?;

    Ok((imported, skipped))
}

/// 插入单条 Codex 会话记录到 proxy_request_logs
fn insert_codex_session_entry(
    db: &Database,
    request_id: &str,
    delta: &DeltaTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

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
        model,
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_codex_pricing(&conn, model);
    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app("codex", &usage, &p, multiplier);
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
                "_codex_session",    // provider_id
                "codex",             // app_type
                model,
                model,               // request_model = model
                delta.input,
                delta.output,
                delta.cached_input,
                0i64,                // cache_creation_tokens: Codex 日志无此数据
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,                // latency_ms
                Option::<i64>::None, // first_token_ms
                200i64,              // status_code
                Option::<String>::None, // error_message
                session_id.map(|s| s.to_string()),
                Some("codex_session"), // provider_type
                1i64,                // is_streaming
                "1.0",               // cost_multiplier
                created_at,
                "codex_session",     // data_source
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 Codex 会话日志失败: {e}")))?;

    if inserted_rows > 0 {
        crate::usage_events::notify_log_recorded();
    }

    Ok(true)
}

/// 查找 Codex 模型定价（带归一化）
fn find_codex_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, &normalize_codex_model(model_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_codex_usage_log(
        path: &Path,
        thread_id: &str,
        session_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": thread_id,
                    "session_id": session_id,
                    "parent_thread_id": (thread_id != session_id).then_some(session_id),
                    "source": if thread_id == session_id {
                        serde_json::Value::String("cli".to_string())
                    } else {
                        serde_json::json!({
                            "subagent": {
                                "thread_spawn": { "parent_thread_id": session_id, "depth": 1 }
                            }
                        })
                    }
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:01Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5.6-sol" }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": {
                            "input_tokens": input_tokens,
                            "cached_input_tokens": input_tokens / 2,
                            "output_tokens": output_tokens
                        }
                    }
                }
            }),
        ];
        let content = lines
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(path, content).unwrap();
    }

    fn insert_legacy_codex_usage_row(db: &Database, session_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd, latency_ms, status_code, session_id, created_at, data_source
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                format!("codex_session:{session_id}:1"),
                "_codex_session",
                "codex",
                "gpt-5.6-sol",
                "gpt-5.6-sol",
                200,
                20,
                100,
                0,
                "0",
                0,
                200,
                session_id,
                1_752_116_402i64,
                "codex_session"
            ],
        )?;
        Ok(())
    }

    #[test]
    fn test_delta_first_event() {
        let prev = None;
        let current = CumulativeTokens {
            input: 17934,
            cached_input: 9600,
            output: 454,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 17934);
        assert_eq!(delta.cached_input, 9600);
        assert_eq!(delta.output, 454);
        assert!(!delta.is_zero());
    }

    #[test]
    fn test_delta_subsequent_event() {
        let prev = Some(CumulativeTokens {
            input: 17934,
            cached_input: 9600,
            output: 454,
        });
        let current = CumulativeTokens {
            input: 36722,
            cached_input: 27904,
            output: 804,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 36722 - 17934);
        assert_eq!(delta.cached_input, 27904 - 9600);
        assert_eq!(delta.output, 804 - 454);
    }

    #[test]
    fn test_delta_zero_at_task_boundary() {
        let prev = Some(CumulativeTokens {
            input: 58346,
            cached_input: 46976,
            output: 1045,
        });
        // task 边界：相同的累计值
        let current = CumulativeTokens {
            input: 58346,
            cached_input: 46976,
            output: 1045,
        };
        let delta = compute_delta(&prev, &current);
        assert!(delta.is_zero());
    }

    #[test]
    fn test_delta_saturating_sub() {
        // 异常情况：当前值小于前值（不应发生，但需防护）
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 50,
            output: 30,
        });
        let current = CumulativeTokens {
            input: 80,
            cached_input: 40,
            output: 20,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 0);
        assert_eq!(delta.cached_input, 0);
        assert_eq!(delta.output, 0);
        assert!(delta.is_zero());
    }

    #[test]
    fn test_parse_cumulative_tokens_valid() {
        let json: serde_json::Value = serde_json::json!({
            "input_tokens": 17934,
            "cached_input_tokens": 9600,
            "output_tokens": 454,
            "reasoning_output_tokens": 233,
            "total_tokens": 18388
        });
        let tokens = parse_cumulative_tokens(&json).unwrap();
        assert_eq!(tokens.input, 17934);
        assert_eq!(tokens.cached_input, 9600);
        assert_eq!(tokens.output, 454);
    }

    #[test]
    fn test_parse_cumulative_tokens_null() {
        let json = serde_json::Value::Null;
        assert!(parse_cumulative_tokens(&json).is_none());
    }

    #[test]
    fn test_parse_cumulative_tokens_alt_field_names() {
        // 某些版本可能使用 cache_read_input_tokens 而非 cached_input_tokens
        let json: serde_json::Value = serde_json::json!({
            "input_tokens": 1000,
            "cache_read_input_tokens": 500,
            "output_tokens": 200
        });
        let tokens = parse_cumulative_tokens(&json).unwrap();
        assert_eq!(tokens.cached_input, 500);
    }

    #[test]
    fn test_collect_codex_session_files_nonexistent() {
        let files = collect_codex_session_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_subagent_identity_prefers_unique_thread_id() {
        let payload = serde_json::json!({
            "session_id": "parent-thread",
            "id": "child-thread",
            "parent_thread_id": "parent-thread",
            "source": {
                "subagent": {
                    "thread_spawn": { "parent_thread_id": "parent-thread", "depth": 1 }
                }
            }
        });

        let identity = parse_codex_session_identity(&payload).unwrap();
        assert_eq!(identity.thread_id, "child-thread");
        assert_eq!(identity.session_id, "parent-thread");
        assert!(identity.is_subagent());
        assert!(identity.carries_history_snapshot);
    }

    #[test]
    fn test_identity_and_replay_boundary_skip_malformed_matching_lines() {
        let temp = tempdir().unwrap();
        let child = temp.path().join("child-malformed.jsonl");
        let lines = [
            "not-json session_meta".to_string(),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "child",
                    "session_id": "parent",
                    "source": { "subagent": {} }
                }
            })
            .to_string(),
            "not-json thread_settings_applied".to_string(),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:01Z",
                "type": "event_msg",
                "payload": { "type": "thread_settings_applied" }
            })
            .to_string(),
        ];
        fs::write(&child, lines.join("\n") + "\n").unwrap();

        let identity = read_codex_session_identity(&child).unwrap();
        assert_eq!(identity.thread_id, "child");
        assert_eq!(
            codex_history_replay_boundary(&child, Some(&identity)),
            Some(4)
        );
    }

    #[test]
    fn test_subagent_history_snapshot_is_baseline_not_usage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = temp.path().join("child-history.jsonl");
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.000Z",
                "type": "session_meta",
                "payload": {
                    "id": "child",
                    "session_id": "parent",
                    "source": {
                        "subagent": {
                            "thread_spawn": { "parent_thread_id": "parent", "depth": 1 }
                        }
                    }
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.100Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 1000,
                        "cached_input_tokens": 900,
                        "output_tokens": 100
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.200Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 1200,
                        "cached_input_tokens": 1000,
                        "output_tokens": 120
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.200Z",
                "type": "event_msg",
                "payload": { "type": "thread_settings_applied" }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.300Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "model": "gpt-5.6-sol",
                        "total_token_usage": {
                            "input_tokens": 1300,
                            "cached_input_tokens": 1050,
                            "output_tokens": 150
                        }
                    }
                }
            }),
        ];
        fs::write(
            &child,
            lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let identity = read_codex_session_identity(&child).unwrap();
        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&identity))?,
            (1, 2)
        );

        let conn = lock_conn!(db.conn);
        let usage: (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs
             WHERE request_id = 'codex_session:child:3'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(usage, (100, 50, 30));

        Ok(())
    }

    #[test]
    fn test_fork_replay_keeps_legacy_request_id_sequence() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let fork = temp.path().join("fork.jsonl");
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "fork",
                    "session_id": "fork",
                    "forked_from_id": "parent",
                    "source": "cli"
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.100Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 100,
                        "cached_input_tokens": 50,
                        "output_tokens": 10
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.200Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 200,
                        "cached_input_tokens": 100,
                        "output_tokens": 20
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.300Z",
                "type": "event_msg",
                "payload": { "type": "thread_settings_applied" }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:01Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 300,
                        "cached_input_tokens": 150,
                        "output_tokens": 30
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 400,
                        "cached_input_tokens": 200,
                        "output_tokens": 40
                    }}
                }
            }),
        ];
        fs::write(
            &fork,
            lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let identity = read_codex_session_identity(&fork).unwrap();
        assert!(!identity.is_subagent());
        assert_eq!(
            codex_history_replay_boundary(&fork, Some(&identity)),
            Some(4)
        );

        {
            let conn = lock_conn!(db.conn);
            for event_index in 1..=3 {
                conn.execute(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, cache_read_tokens,
                        total_cost_usd, latency_ms, status_code, session_id,
                        created_at, data_source
                    ) VALUES (?1, '_codex_session', 'codex', 'gpt-5.6-sol',
                              'gpt-5.6-sol', 100, 10, 50, '0', 0, 200, 'fork',
                              1752116402, 'codex_session')",
                    [format!("codex_session:fork:{event_index}")],
                )?;
            }
        }

        let fork_path = fork.to_string_lossy().to_string();
        update_sync_state(&db, &fork_path, 1, 5)?;
        assert_eq!(sync_single_codex_file(&db, &fork, Some(&identity))?, (1, 0));

        let conn = lock_conn!(db.conn);
        let usage: (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs WHERE request_id = 'codex_session:fork:4'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(usage, (100, 50, 10));

        Ok(())
    }

    #[test]
    fn test_fast_subagent_without_replay_boundary_counts_first_usage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = temp.path().join("fast-child.jsonl");
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.000Z",
                "type": "session_meta",
                "payload": {
                    "id": "fast-child",
                    "session_id": "parent",
                    "source": {
                        "subagent": {
                            "thread_spawn": {
                                "parent_thread_id": "parent",
                                "depth": 1
                            }
                        }
                    }
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00.100Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 100,
                        "cached_input_tokens": 50,
                        "output_tokens": 10
                    }}
                }
            }),
        ];
        fs::write(
            &child,
            lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let identity = read_codex_session_identity(&child).unwrap();
        assert_eq!(codex_history_replay_boundary(&child, Some(&identity)), None);
        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let usage: (i64, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens FROM proxy_request_logs
             WHERE request_id = 'codex_session:fast-child:1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(usage, (100, 10));

        Ok(())
    }

    #[test]
    fn test_subagents_under_same_parent_get_unique_request_ids() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child_a = temp.path().join("child-a.jsonl");
        let child_b = temp.path().join("child-b.jsonl");
        write_codex_usage_log(&child_a, "child-a", "parent", 100, 10);
        write_codex_usage_log(&child_b, "child-b", "parent", 200, 20);

        let identity_a = read_codex_session_identity(&child_a).unwrap();
        let identity_b = read_codex_session_identity(&child_b).unwrap();
        assert_eq!(
            sync_single_codex_file(&db, &child_a, Some(&identity_a))?,
            (1, 0)
        );
        assert_eq!(
            sync_single_codex_file(&db, &child_b, Some(&identity_b))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            request_ids,
            vec![
                "codex_session:child-a:1".to_string(),
                "codex_session:child-b:1".to_string()
            ]
        );

        Ok(())
    }

    #[test]
    fn test_repair_resets_legacy_parent_collisions_and_reimports() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let main = temp.path().join("main.jsonl");
        let child = temp.path().join("child.jsonl");
        write_codex_usage_log(&main, "parent", "parent", 100, 10);
        write_codex_usage_log(&child, "child", "parent", 200, 20);

        let main_identity = read_codex_session_identity(&main).unwrap();
        let child_identity = read_codex_session_identity(&child).unwrap();
        let files = vec![
            (main.clone(), Some(main_identity.clone())),
            (child.clone(), Some(child_identity.clone())),
        ];

        insert_legacy_codex_usage_row(&db, "parent")?;
        let main_path = main.to_string_lossy().to_string();
        let child_path = child.to_string_lossy().to_string();
        update_sync_state(&db, &main_path, 1, 3)?;
        update_sync_state(&db, &child_path, 1, 3)?;

        repair_legacy_codex_subagent_usage(&db, &files)?;

        {
            let conn = lock_conn!(db.conn);
            let old_rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs
                 WHERE request_id = 'codex_session:parent:1'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(old_rows, 0);

            let old_sync_rows: i64 = conn.query_row(
                "SELECT COUNT(*) FROM session_log_sync WHERE file_path IN (?1, ?2)",
                rusqlite::params![main_path, child_path],
                |row| row.get(0),
            )?;
            assert_eq!(old_sync_rows, 0);
        }

        assert_eq!(
            sync_single_codex_file(&db, &main, Some(&main_identity))?,
            (1, 0)
        );
        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&child_identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);

        Ok(())
    }

    #[test]
    fn test_repair_reimports_late_sibling_after_parent_rebuild() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let main = temp.path().join("main.jsonl");
        let child_a = temp.path().join("child-a.jsonl");
        let child_b = temp.path().join("child-b.jsonl");
        write_codex_usage_log(&main, "parent", "parent", 100, 10);
        write_codex_usage_log(&child_a, "child-a", "parent", 200, 20);
        write_codex_usage_log(&child_b, "child-b", "parent", 300, 30);

        let main_identity = read_codex_session_identity(&main).unwrap();
        let child_a_identity = read_codex_session_identity(&child_a).unwrap();
        let child_b_identity = read_codex_session_identity(&child_b).unwrap();
        insert_legacy_codex_usage_row(&db, "parent")?;

        let main_path = main.to_string_lossy().to_string();
        let child_a_path = child_a.to_string_lossy().to_string();
        let child_b_path = child_b.to_string_lossy().to_string();
        update_sync_state(&db, &main_path, 1, 3)?;
        update_sync_state(&db, &child_a_path, 1, 3)?;
        update_sync_state(&db, &child_b_path, 1, 3)?;

        let first_files = vec![
            (main.clone(), Some(main_identity.clone())),
            (child_a.clone(), Some(child_a_identity.clone())),
        ];
        repair_legacy_codex_subagent_usage(&db, &first_files)?;
        assert_eq!(
            sync_single_codex_file(&db, &main, Some(&main_identity))?,
            (1, 0)
        );
        assert_eq!(
            sync_single_codex_file(&db, &child_a, Some(&child_a_identity))?,
            (1, 0)
        );

        let all_files = vec![
            (main, Some(main_identity)),
            (child_a, Some(child_a_identity)),
            (child_b.clone(), Some(child_b_identity.clone())),
        ];
        repair_legacy_codex_subagent_usage(&db, &all_files)?;
        assert_eq!(get_sync_state(&db, &child_b_path)?, (0, 0));
        assert_eq!(
            sync_single_codex_file(&db, &child_b, Some(&child_b_identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            request_ids,
            vec![
                "codex_session:child-a:1".to_string(),
                "codex_session:child-b:1".to_string(),
                "codex_session:parent:1".to_string(),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_late_sibling_with_rollup_only_imports_new_usage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let main = temp.path().join("main.jsonl");
        let child_a = temp.path().join("child-a.jsonl");
        let child_b = temp.path().join("child-b.jsonl");
        write_codex_usage_log(&main, "parent", "parent", 100, 10);
        write_codex_usage_log(&child_a, "child-a", "parent", 200, 20);

        let child_b_lines = [
            serde_json::json!({
                "timestamp": "2025-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "child-b",
                    "session_id": "parent",
                    "source": { "subagent": {} }
                }
            }),
            serde_json::json!({
                "timestamp": "2025-07-10T03:00:01Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5.6-sol" }
            }),
            serde_json::json!({
                "timestamp": "2025-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 300,
                        "cached_input_tokens": 150,
                        "output_tokens": 30
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:03Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 400,
                        "cached_input_tokens": 200,
                        "output_tokens": 40
                    }}
                }
            }),
        ];
        fs::write(
            &child_b,
            child_b_lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let main_identity = read_codex_session_identity(&main).unwrap();
        let child_a_identity = read_codex_session_identity(&child_a).unwrap();
        let child_b_identity = read_codex_session_identity(&child_b).unwrap();
        insert_legacy_codex_usage_row(&db, "parent")?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_request_logs
                 SET provider_id = 'openai', data_source = 'proxy'
                 WHERE request_id = 'codex_session:parent:1'",
                [],
            )?;
        }
        assert_eq!(db.rollup_and_prune(30)?, 1);

        let main_path = main.to_string_lossy().to_string();
        let child_a_path = child_a.to_string_lossy().to_string();
        let child_b_path = child_b.to_string_lossy().to_string();
        update_sync_state(&db, &main_path, 1, 3)?;
        update_sync_state(&db, &child_a_path, 1, 3)?;
        update_sync_state(&db, &child_b_path, 1, 3)?;

        let first_files = vec![
            (main.clone(), Some(main_identity.clone())),
            (child_a.clone(), Some(child_a_identity.clone())),
        ];
        repair_legacy_codex_subagent_usage(&db, &first_files)?;
        assert_eq!(
            sync_single_codex_file(&db, &main, Some(&main_identity))?,
            (1, 0)
        );
        assert_eq!(
            sync_single_codex_file(&db, &child_a, Some(&child_a_identity))?,
            (1, 0)
        );

        let all_files = vec![
            (main, Some(main_identity)),
            (child_a, Some(child_a_identity)),
            (child_b.clone(), Some(child_b_identity.clone())),
        ];
        repair_legacy_codex_subagent_usage(&db, &all_files)?;

        let subagent_key = format!("{child_b_path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        assert_eq!(get_sync_state(&db, &child_b_path)?, (0, 0));
        assert_eq!(get_sync_state(&db, &subagent_key)?, (1, 3));
        assert_eq!(
            sync_single_codex_file(&db, &child_b, Some(&child_b_identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let rollup_count: i64 = conn.query_row(
            "SELECT request_count FROM usage_daily_rollups
             WHERE app_type = 'codex' AND provider_id = 'openai'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(rollup_count, 1);

        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            request_ids,
            vec![
                "codex_session:child-a:1".to_string(),
                "codex_session:child-b:2".to_string(),
                "codex_session:parent:1".to_string(),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_repair_preserves_orphan_usage_and_migrates_cursor() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = temp.path().join("orphan-child.jsonl");
        let lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "child",
                    "session_id": "missing-parent",
                    "source": {
                        "subagent": {
                            "thread_spawn": {
                                "parent_thread_id": "missing-parent",
                                "depth": 1
                            }
                        }
                    }
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:01Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5.6-sol" }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 200,
                        "cached_input_tokens": 100,
                        "output_tokens": 20
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:03Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 300,
                        "cached_input_tokens": 150,
                        "output_tokens": 30
                    }}
                }
            }),
        ];
        fs::write(
            &child,
            lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let identity = read_codex_session_identity(&child).unwrap();
        let files = vec![(child.clone(), Some(identity.clone()))];
        insert_legacy_codex_usage_row(&db, "missing-parent")?;

        let child_path = child.to_string_lossy().to_string();
        update_sync_state(&db, &child_path, 1, 3)?;
        repair_legacy_codex_subagent_usage(&db, &files)?;

        let subagent_key = format!("{child_path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        assert_eq!(get_sync_state(&db, &child_path)?, (0, 0));
        assert_eq!(get_sync_state(&db, &subagent_key)?, (1, 3));
        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(request_ids.len(), 2);
        assert!(request_ids.contains(&"codex_session:child:2".to_string()));
        assert!(request_ids.iter().any(|request_id| {
            request_id.starts_with("codex_session:legacy:")
                && request_id.ends_with(":codex_session:missing-parent:1")
        }));

        Ok(())
    }

    #[test]
    fn test_late_subagent_after_migration_activation_preserves_legacy_cursor(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        repair_legacy_codex_subagent_usage(&db, &[])?;
        {
            let conn = lock_conn!(db.conn);
            let marker_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE key = ?1)",
                [CODEX_SUBAGENT_USAGE_MIGRATION_KEY],
                |row| row.get(0),
            )?;
            assert!(!marker_exists);
        }

        let temp = tempdir().unwrap();
        let unrelated_main = temp.path().join("unrelated-main.jsonl");
        write_codex_usage_log(&unrelated_main, "unrelated", "unrelated", 100, 10);
        let unrelated_identity = read_codex_session_identity(&unrelated_main).unwrap();
        repair_legacy_codex_subagent_usage(&db, &[(unrelated_main, Some(unrelated_identity))])?;

        let child = temp.path().join("late-child.jsonl");
        let child_lines = [
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "late-child",
                    "session_id": "missing-parent",
                    "source": { "subagent": {} }
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:01Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5.6-sol" }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 200,
                        "cached_input_tokens": 100,
                        "output_tokens": 20
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:03Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 300,
                        "cached_input_tokens": 150,
                        "output_tokens": 30
                    }}
                }
            }),
        ];
        fs::write(
            &child,
            child_lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let identity = read_codex_session_identity(&child).unwrap();
        let child_path = child.to_string_lossy().to_string();
        insert_legacy_codex_usage_row(&db, "missing-parent")?;
        update_sync_state(&db, &child_path, 1, 3)?;

        repair_legacy_codex_subagent_usage(&db, &[(child.clone(), Some(identity.clone()))])?;

        let subagent_key = format!("{child_path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        assert_eq!(get_sync_state(&db, &child_path)?, (0, 0));
        assert_eq!(get_sync_state(&db, &subagent_key)?, (1, 3));
        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'codex_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);

        Ok(())
    }

    #[test]
    fn test_subagent_sync_falls_back_to_newer_plain_cursor() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = temp.path().join("fallback-child.jsonl");
        write_codex_usage_log(&child, "fallback-child", "parent", 200, 20);

        let identity = read_codex_session_identity(&child).unwrap();
        let child_path = child.to_string_lossy().to_string();
        let subagent_key = format!("{child_path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        update_sync_state(&db, &subagent_key, 1, 1)?;
        update_sync_state(&db, &child_path, 2, 3)?;

        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&identity))?,
            (0, 0)
        );
        let (last_modified, last_offset) = get_sync_state(&db, &subagent_key)?;
        assert!(last_modified > 2);
        assert_eq!(last_offset, 3);

        Ok(())
    }

    #[test]
    fn test_repair_with_rollup_preserves_history_and_only_imports_new_usage() -> Result<(), AppError>
    {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let main = temp.path().join("main.jsonl");
        let child = temp.path().join("child.jsonl");
        write_codex_usage_log(&main, "parent", "parent", 100, 10);

        let child_lines = [
            serde_json::json!({
                "timestamp": "2025-07-10T03:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": "child",
                    "session_id": "parent",
                    "source": {
                        "subagent": {
                            "thread_spawn": { "parent_thread_id": "parent", "depth": 1 }
                        }
                    }
                }
            }),
            serde_json::json!({
                "timestamp": "2025-07-10T03:00:01Z",
                "type": "turn_context",
                "payload": { "model": "gpt-5.6-sol" }
            }),
            serde_json::json!({
                "timestamp": "2025-07-10T03:00:02Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 200,
                        "cached_input_tokens": 100,
                        "output_tokens": 20
                    }}
                }
            }),
            serde_json::json!({
                "timestamp": "2026-07-10T03:00:03Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "total_token_usage": {
                        "input_tokens": 300,
                        "cached_input_tokens": 150,
                        "output_tokens": 30
                    }}
                }
            }),
        ];
        fs::write(
            &child,
            child_lines
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let main_identity = read_codex_session_identity(&main).unwrap();
        let child_identity = read_codex_session_identity(&child).unwrap();
        let files = vec![
            (main.clone(), Some(main_identity)),
            (child.clone(), Some(child_identity.clone())),
        ];
        insert_legacy_codex_usage_row(&db, "parent")?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_request_logs
                 SET provider_id = 'openai', data_source = 'proxy'
                 WHERE request_id = 'codex_session:parent:1'",
                [],
            )?;
        }
        assert_eq!(db.rollup_and_prune(30)?, 1);

        // 模拟旧解析器中子代理抢占了父线程未来会使用的序号。
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens,
                    total_cost_usd, latency_ms, status_code, session_id,
                    created_at, data_source
                ) VALUES ('codex_session:parent:2', '_codex_session', 'codex',
                          'gpt-5.6-sol', 'gpt-5.6-sol', 50, 5, 25, '0', 0,
                          200, 'parent', 1783652402, 'codex_session')",
                [],
            )?;
        }

        let new_parent_usage = serde_json::json!({
            "timestamp": "2026-07-10T03:00:03Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": { "total_token_usage": {
                    "input_tokens": 200,
                    "cached_input_tokens": 100,
                    "output_tokens": 20
                }}
            }
        });
        let mut main_contents = fs::read_to_string(&main).unwrap();
        main_contents.push_str(&new_parent_usage.to_string());
        main_contents.push('\n');
        fs::write(&main, main_contents).unwrap();

        let main_path = main.to_string_lossy().to_string();
        let child_path = child.to_string_lossy().to_string();
        update_sync_state(&db, &main_path, 1, 3)?;
        update_sync_state(&db, &child_path, 1, 3)?;

        repair_legacy_codex_subagent_usage(&db, &files)?;

        let subagent_key = format!("{child_path}{CODEX_SUBAGENT_SYNC_KEY_SUFFIX}");
        assert_eq!(get_sync_state(&db, &main_path)?, (1, 3));
        assert_eq!(get_sync_state(&db, &subagent_key)?, (1, 3));
        assert_eq!(
            sync_single_codex_file(&db, &main, files[0].1.as_ref())?,
            (1, 0)
        );
        assert_eq!(
            sync_single_codex_file(&db, &child, Some(&child_identity))?,
            (1, 0)
        );

        let conn = lock_conn!(db.conn);
        let rollup_count: i64 = conn.query_row(
            "SELECT request_count FROM usage_daily_rollups
             WHERE app_type = 'codex' AND provider_id = 'openai'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(rollup_count, 1);

        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(request_ids.len(), 3);
        assert!(request_ids.contains(&"codex_session:child:2".to_string()));
        assert!(request_ids.contains(&"codex_session:parent:2".to_string()));
        assert!(request_ids.iter().any(|request_id| {
            request_id.starts_with("codex_session:legacy:")
                && request_id.ends_with(":codex_session:parent:2")
        }));

        Ok(())
    }

    #[test]
    fn test_insert_codex_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "codex-proxy",
                    "openai",
                    "codex",
                    "gpt-5.4",
                    "gpt-5.4",
                    10,
                    2,
                    1,
                    7,
                    "0.01",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let delta = DeltaTokens {
            input: 10,
            cached_input: 1,
            output: 2,
        };
        let inserted = insert_codex_session_entry(
            &db,
            "codex-session-dup",
            &delta,
            "gpt-5.4",
            Some("session-1"),
            Some("1970-01-01T00:16:45Z"),
        )?;
        assert!(!inserted);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    // ── 模型名归一化测试 ──

    #[test]
    fn test_normalize_codex_model_lowercase() {
        assert_eq!(normalize_codex_model("GLM-4.6"), "glm-4.6");
        assert_eq!(normalize_codex_model("DeepSeek-Chat"), "deepseek-chat");
        assert_eq!(normalize_codex_model("GPT-5.4"), "gpt-5.4");
    }

    #[test]
    fn test_normalize_codex_model_strip_prefix() {
        assert_eq!(normalize_codex_model("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("azure/gpt-5.2-codex"),
            "gpt-5.2-codex"
        );
        assert_eq!(normalize_codex_model("OPENAI/GPT-5.4"), "gpt-5.4");
    }

    #[test]
    fn test_normalize_codex_model_strip_iso_date() {
        assert_eq!(normalize_codex_model("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("gpt-5.4-pro-2026-03-05"),
            "gpt-5.4-pro"
        );
    }

    #[test]
    fn test_normalize_codex_model_strip_compact_date() {
        assert_eq!(normalize_codex_model("gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("claude-opus-4-6-20260206"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn test_normalize_codex_model_no_change() {
        assert_eq!(normalize_codex_model("gpt-5.4"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.2-codex"), "gpt-5.2-codex");
        assert_eq!(normalize_codex_model("o3"), "o3");
        assert_eq!(normalize_codex_model("deepseek-chat"), "deepseek-chat");
    }

    #[test]
    fn test_normalize_codex_model_combined() {
        // prefix + uppercase + ISO date
        assert_eq!(
            normalize_codex_model("openai/GPT-5.4-2026-03-05"),
            "gpt-5.4"
        );
        // prefix + compact date
        assert_eq!(normalize_codex_model("openai/gpt-5.4-20260305"), "gpt-5.4");
    }

    #[test]
    fn test_cached_clamped_to_input() {
        // cached > input 的异常场景应被 min() 钳制
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 0,
            output: 50,
        });
        let current = CumulativeTokens {
            input: 110,       // delta = 10
            cached_input: 80, // delta = 80（异常：大于 input delta）
            output: 60,
        };
        let delta = compute_delta(&prev, &current);
        // 钳制前：cached_input = 80, input = 10
        assert_eq!(delta.cached_input, 80);
        assert_eq!(delta.input, 10);
        // 实际钳制在调用侧：delta.cached_input.min(delta.input)
        let clamped = delta.cached_input.min(delta.input);
        assert_eq!(clamped, 10);
    }
}
