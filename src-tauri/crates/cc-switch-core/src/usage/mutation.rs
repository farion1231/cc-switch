use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::backup::Backup;
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;

use crate::{CoreError, HeadlessState};

use super::model::{OperationCancellation, PricingUpdate, ProviderLimitStatus, SessionSyncResult};
use super::sql::{
    is_cache_inclusive_app, INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL,
};

#[derive(Clone)]
struct PricingInfo {
    input: Decimal,
    output: Decimal,
    cache_read: Decimal,
    cache_creation: Decimal,
}

struct BackfillRow {
    request_id: String,
    app_type: String,
    model: String,
    request_model: Option<String>,
    pricing_model: Option<String>,
    cost_multiplier: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    input_token_semantics: i64,
}

pub(super) fn update_pricing(
    connection: &Connection,
    input: PricingUpdate,
) -> Result<(), CoreError> {
    update_pricing_batch(connection, vec![input]).map(|_| ())
}

/// models.dev 可能一次同步数百条定价；全部条目与历史成本回填共用一个事务，
/// 避免逐条回填造成 O(模型数 × 历史行数) 的放大，也避免远端断线留下部分批次。
pub(super) fn update_pricing_batch(
    connection: &Connection,
    inputs: Vec<PricingUpdate>,
) -> Result<usize, CoreError> {
    let mut normalized = std::collections::BTreeMap::new();
    for input in inputs {
        let input = validate_pricing(input)?;
        normalized.insert(input.model_id.clone(), input);
    }
    if normalized.is_empty() {
        return Ok(0);
    }

    let transaction = connection.unchecked_transaction()?;
    let mut changed = 0;
    for input in normalized.into_values() {
        changed += transaction.execute(
            "INSERT INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(model_id) DO UPDATE SET
                display_name = excluded.display_name,
                input_cost_per_million = excluded.input_cost_per_million,
                output_cost_per_million = excluded.output_cost_per_million,
                cache_read_cost_per_million = excluded.cache_read_cost_per_million,
                cache_creation_cost_per_million = excluded.cache_creation_cost_per_million
             WHERE display_name <> excluded.display_name
                OR input_cost_per_million <> excluded.input_cost_per_million
                OR output_cost_per_million <> excluded.output_cost_per_million
                OR cache_read_cost_per_million <> excluded.cache_read_cost_per_million
                OR cache_creation_cost_per_million <> excluded.cache_creation_cost_per_million",
            params![
                input.model_id,
                input.display_name,
                input.input_cost,
                input.output_cost,
                input.cache_read_cost,
                input.cache_creation_cost,
            ],
        )?;
    }
    // 与上游桌面写侧保持一致：定价内容未变化时不重复扫描全部零成本历史行。
    if changed > 0 {
        backfill_missing_costs(&transaction)?;
    }
    transaction.commit()?;
    Ok(changed)
}

pub(super) fn delete_pricing(connection: &Connection, model_id: &str) -> Result<(), CoreError> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err(CoreError::InvalidPricing("模型 ID 不能为空".to_string()));
    }
    connection.execute("DELETE FROM model_pricing WHERE model_id = ?1", [model_id])?;
    Ok(())
}

pub(super) fn limits(
    connection: &Connection,
    provider_id: &str,
    app_type: &str,
) -> Result<ProviderLimitStatus, CoreError> {
    let meta = connection
        .query_row(
            "SELECT meta FROM providers WHERE id = ?1 AND app_type = ?2",
            params![provider_id, app_type],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let (daily_limit, monthly_limit) = meta
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .map(|value| {
            (
                limit_value(&value, "limitDailyUsd"),
                limit_value(&value, "limitMonthlyUsd"),
            )
        })
        .unwrap_or((None, None));

    let daily_usage = usage_cost_for_period(connection, provider_id, app_type, true)?;
    let monthly_usage = usage_cost_for_period(connection, provider_id, app_type, false)?;
    Ok(ProviderLimitStatus {
        provider_id: provider_id.to_string(),
        daily_usage: format!("{daily_usage:.6}"),
        daily_limit: daily_limit.map(|value| format!("{value:.2}")),
        daily_exceeded: daily_limit.is_some_and(|limit| daily_usage >= limit),
        monthly_usage: format!("{monthly_usage:.6}"),
        monthly_limit: monthly_limit.map(|value| format!("{value:.2}")),
        monthly_exceeded: monthly_limit.is_some_and(|limit| monthly_usage >= limit),
    })
}

pub(super) fn rebuild_codex(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let _guard = session_sync_guard()?;
    cancellation.check()?;
    let backup_path = codex_backup_path(state.home())?;
    state.with_connection(|connection| backup_database(connection, &backup_path))?;

    cancellation.check()?;
    state.with_connection(reset_codex_usage)?;

    cancellation.check()?;
    sync_codex_sessions(state, cancellation)
}

pub(super) fn sync_sessions(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let _guard = session_sync_guard()?;
    cancellation.check()?;
    let mut result = super::session::sync_non_codex_sessions(state, cancellation)?;
    result.merge(sync_codex_sessions(state, cancellation)?);
    // 会话导入器优先保证 token 原始值可追溯；成本统一在全部来源完成后回填，
    // 避免五套解析器各自复制模型定价和 cache token 计费规则。
    state.with_connection(|connection| {
        backfill_missing_costs(connection)?;
        Ok(())
    })?;
    Ok(result)
}

/// 锁覆盖一次同步或重建的完整生命周期，而非单次 SQLite 写入；Task 9 引入并发 worker
/// 后也不能让普通同步插入到 Codex backup/reset/import 三阶段之间。
fn session_sync_guard() -> Result<MutexGuard<'static, ()>, CoreError> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| CoreError::StatePoisoned)
}

fn codex_backup_path(home: &Path) -> Result<std::path::PathBuf, CoreError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CoreError::RemoteBusiness(format!("系统时间无效: {error}")))?
        .as_nanos();
    Ok(home
        .join(".cc-switch")
        .join(format!("cc-switch.db.codex-rebuild-{timestamp}.bak")))
}

/// 使用 SQLite 在线备份 API 生成一致快照；不能在连接仍写入时直接复制数据库文件。
fn backup_database(connection: &Connection, path: &Path) -> Result<(), CoreError> {
    let mut destination = Connection::open(path)?;
    let backup = Backup::new(connection, &mut destination)?;
    backup.run_to_completion(128, Duration::from_millis(5), None)?;
    Ok(())
}

fn reset_codex_usage(connection: &Connection) -> Result<(), CoreError> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "DELETE FROM proxy_request_logs
         WHERE COALESCE(data_source, '') = 'codex_session'
            OR provider_id = '_codex_session'",
        [],
    )?;
    transaction.execute(
        "DELETE FROM usage_daily_rollups WHERE provider_id = '_codex_session'",
        [],
    )?;
    transaction.execute(
        "DELETE FROM session_log_sync
         WHERE REPLACE(file_path, char(92), '/') LIKE '%/.codex/sessions/%'
            OR REPLACE(file_path, char(92), '/') LIKE '%/.codex/archived_sessions/%'",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn sync_codex_sessions(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let codex = state.home().join(".codex");
    let sessions = codex.join("sessions");
    let archived = codex.join("archived_sessions");
    if !sessions.exists() && !archived.exists() {
        return Ok(SessionSyncResult::default());
    }
    let mut files = Vec::new();
    for directory in [&sessions, &archived] {
        if directory.exists() && !directory.is_dir() {
            return Err(CoreError::RemoteBusiness(format!(
                "Codex 会话路径不是目录: {}",
                directory.display()
            )));
        }
        if directory.is_dir() {
            if let Err(error) = collect_session_files(directory, cancellation, &mut files) {
                cancellation.check()?;
                return Err(CoreError::RemoteBusiness(format!(
                    "读取 Codex 会话目录失败 {}: {error}",
                    directory.display()
                )));
            }
        }
    }
    files.sort();
    let parsed = files
        .iter()
        .map(|file| parse_codex_file(file, cancellation))
        .collect::<Result<Vec<_>, _>>()?;
    let rollout_index = parsed
        .iter()
        .enumerate()
        .map(|(index, file)| (file.session_id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut result = SessionSyncResult {
        files_scanned: files.len() as u32,
        ..SessionSyncResult::default()
    };
    for file in &parsed {
        cancellation.check()?;
        if !super::session::watermark_requires_scan(state, &file.path, file.modified)? {
            continue;
        }
        let Some(replay_count) = codex_replay_count(file, &parsed, &rollout_index) else {
            // 父 rollout 尚未落盘或元数据冲突时不推进任何导入状态；下次同步可在
            // 子文件本身未变化的情况下重新解析，避免把重放历史先写成真实用量。
            result.deferred_files = result.deferred_files.saturating_add(1);
            continue;
        };
        result.skipped = result.skipped.saturating_add(replay_count as u32);
        let rows = codex_usage_rows(file, replay_count);
        state.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            for row in rows {
                if super::session::has_matching_proxy_usage(
                    &transaction,
                    &super::session::SessionFingerprint {
                        app_type: "codex",
                        model: &row.model,
                        input_tokens: row.input_tokens,
                        output_tokens: row.output_tokens,
                        cache_read_tokens: row.cache_read_tokens,
                        cache_creation_tokens: 0,
                        created_at: row.created_at,
                    },
                )? {
                    result.skipped = result.skipped.saturating_add(1);
                    continue;
                }
                let changed = transaction.execute(
                    "INSERT OR IGNORE INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, request_model, pricing_model,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        input_token_semantics, input_cost_usd, output_cost_usd,
                        cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                        latency_ms, status_code, created_at, data_source, cost_multiplier
                     ) VALUES (
                        ?1, '_codex_session', 'codex', ?2, ?2, ?2,
                        ?3, ?4, ?5, 0, 1, '0', '0', '0', '0', '0',
                        0, 200, ?6, 'codex_session', '1'
                     )",
                    params![
                        row.request_id,
                        row.model,
                        row.input_tokens as i64,
                        row.output_tokens as i64,
                        row.cache_read_tokens as i64,
                        row.created_at,
                    ],
                )?;
                if changed == 1 {
                    result.imported = result.imported.saturating_add(1);
                } else {
                    result.skipped = result.skipped.saturating_add(1);
                }
            }
            transaction.commit()?;
            Ok(())
        })?;
        super::session::update_sync_state(
            state,
            &file.path,
            file.modified,
            file.events.len() as i64,
        )?;
    }
    Ok(result)
}

fn collect_session_files(
    directory: &Path,
    cancellation: &OperationCancellation,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(directory)? {
        if cancellation.check().is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "operation cancelled",
            ));
        }
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_session_files(&entry.path(), cancellation, files)?;
        } else if entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(entry.path());
        }
    }
    Ok(())
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct CodexCumulative {
    input: u64,
    cached: u64,
    output: u64,
}

struct CodexUsageRow {
    request_id: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    created_at: i64,
}

struct CodexTokenEvent {
    counters: CodexCumulative,
    cumulative: bool,
    model: String,
    created_at: i64,
    event_index: u64,
}

struct ParsedCodexFile {
    path: std::path::PathBuf,
    modified: i64,
    session_id: String,
    meta_timestamp: Option<i64>,
    forked_from: Option<String>,
    spawned_from: Option<String>,
    events: Vec<CodexTokenEvent>,
}

/// 第一遍只解析 rollout 结构，不计算费用差值；父子文件必须全部建立索引后，才能
/// 区分 fork 重放历史与子线程真正新增的累计 token。
fn parse_codex_file(
    path: &Path,
    cancellation: &OperationCancellation,
) -> Result<ParsedCodexFile, CoreError> {
    let file = std::fs::File::open(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = std::fs::metadata(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut session_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut model = "unknown".to_string();
    let mut meta_timestamp = None;
    let mut forked_from = None;
    let mut spawned_from = None;
    let mut event_index = 0u64;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        cancellation.check()?;
        let line = line.map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("session_meta") => {
                let payload = value.get("payload");
                if let Some(id) = payload
                    .and_then(|payload| payload.get("id"))
                    .and_then(serde_json::Value::as_str)
                {
                    session_id = id.to_string();
                }
                meta_timestamp = parse_codex_timestamp(value.get("timestamp"));
                forked_from = payload
                    .and_then(|payload| payload.get("forked_from_id"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                spawned_from = payload
                    .and_then(|payload| {
                        payload.pointer("/source/subagent/thread_spawn/parent_thread_id")
                    })
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            Some("turn_context") => {
                if let Some(value) = value
                    .get("payload")
                    .and_then(|payload| payload.get("model"))
                    .and_then(serde_json::Value::as_str)
                {
                    model = value.to_string();
                }
            }
            Some("event_msg") => {
                let Some(payload) = value.get("payload") else {
                    continue;
                };
                if payload.get("type").and_then(serde_json::Value::as_str) != Some("token_count") {
                    continue;
                }
                let Some(info) = payload.get("info") else {
                    continue;
                };
                let (current, cumulative) = if let Some(total) = info.get("total_token_usage") {
                    (parse_codex_cumulative(total), true)
                } else if let Some(last) = info.get("last_token_usage") {
                    (parse_codex_cumulative(last), false)
                } else {
                    continue;
                };
                let Some(created_at) = parse_codex_timestamp(value.get("timestamp")) else {
                    continue;
                };
                event_index = event_index.saturating_add(1);
                events.push(CodexTokenEvent {
                    counters: current,
                    cumulative,
                    model: model.clone(),
                    created_at,
                    event_index,
                });
            }
            _ => {}
        }
    }
    Ok(ParsedCodexFile {
        path: path.to_path_buf(),
        modified: super::session::metadata_modified_nanos(&metadata),
        session_id,
        meta_timestamp,
        forked_from,
        spawned_from,
        events,
    })
}

fn parse_codex_timestamp(value: Option<&serde_json::Value>) -> Option<i64> {
    value
        .and_then(serde_json::Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
}

fn codex_replay_count(
    file: &ParsedCodexFile,
    files: &[ParsedCodexFile],
    index: &HashMap<String, usize>,
) -> Option<usize> {
    if file.events.is_empty() {
        return Some(0);
    }
    // 有计费用量却没有 session_meta 时无法确定它是主 rollout 还是归档/子线程副本；
    // 延后比按完整累计值记账更安全，后续 cursor 兼容由同一索引层扩展。
    let fork_time = file.meta_timestamp?;
    let parent_id = match (&file.forked_from, &file.spawned_from) {
        (Some(forked), Some(spawned)) if forked != spawned => return None,
        (Some(parent), _) | (_, Some(parent)) => Some(parent),
        (None, None) => None,
    };
    let Some(parent_id) = parent_id else {
        return Some(0);
    };
    if parent_id == &file.session_id {
        return None;
    }
    let parent = index
        .get(parent_id)
        .and_then(|position| files.get(*position))?;
    let parent_signatures = parent
        .events
        .iter()
        .filter(|event| event.created_at <= fork_time)
        .map(|event| event.counters)
        .collect::<Vec<_>>();
    Some(matching_codex_replay_prefix(
        &file.events,
        &parent_signatures,
    ))
}

/// Codex 可能过滤掉父历史中的中间 token_count；因此按有序子序列对齐，而不是要求
/// 子文件前缀与父文件逐项完全相等。第一个无法继续对齐的事件就是子线程实时增量起点。
fn matching_codex_replay_prefix(child: &[CodexTokenEvent], parent: &[CodexCumulative]) -> usize {
    let mut parent_cursor = 0;
    let mut matched = 0;
    for event in child {
        let Some(relative) = parent[parent_cursor..]
            .iter()
            .position(|candidate| candidate == &event.counters)
        else {
            break;
        };
        parent_cursor = parent_cursor.saturating_add(relative).saturating_add(1);
        matched += 1;
    }
    matched
}

/// Codex token_count 是累计计数；跳过 fork 重放后，以最后一个重放值作为新事件的
/// 差分基线。`last_token_usage` 是单轮值，不参与累计基线更新。
fn codex_usage_rows(file: &ParsedCodexFile, replay_count: usize) -> Vec<CodexUsageRow> {
    let mut previous = replay_count
        .checked_sub(1)
        .and_then(|index| file.events.get(index))
        .filter(|event| event.cumulative)
        .map(|event| event.counters)
        .unwrap_or_default();
    let mut rows = Vec::new();
    for event in file.events.iter().skip(replay_count) {
        let current = event.counters;
        let input = if event.cumulative {
            current.input.saturating_sub(previous.input)
        } else {
            current.input
        };
        let cached = if event.cumulative {
            current.cached.saturating_sub(previous.cached)
        } else {
            current.cached
        }
        .min(input);
        let output = if event.cumulative {
            current.output.saturating_sub(previous.output)
        } else {
            current.output
        };
        if event.cumulative {
            previous = current;
        }
        if input == 0 && output == 0 {
            continue;
        }
        rows.push(CodexUsageRow {
            request_id: format!("codex_session:{}:{}", file.session_id, event.event_index),
            model: event.model.clone(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cached,
            created_at: event.created_at,
        });
    }
    rows
}

fn parse_codex_cumulative(value: &serde_json::Value) -> CodexCumulative {
    CodexCumulative {
        input: value
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        cached: value
            .get("cached_input_tokens")
            .or_else(|| value.get("cache_read_input_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        output: value
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    }
}

fn validate_pricing(mut input: PricingUpdate) -> Result<PricingUpdate, CoreError> {
    input.model_id = input.model_id.trim().to_string();
    input.display_name = input.display_name.trim().to_string();
    if input.model_id.is_empty() {
        return Err(CoreError::InvalidPricing("模型 ID 不能为空".to_string()));
    }
    if input.display_name.is_empty() {
        return Err(CoreError::InvalidPricing("显示名称不能为空".to_string()));
    }
    for (name, value) in [
        ("input_cost", &mut input.input_cost),
        ("output_cost", &mut input.output_cost),
        ("cache_read_cost", &mut input.cache_read_cost),
        ("cache_creation_cost", &mut input.cache_creation_cost),
    ] {
        *value = value.trim().to_string();
        let decimal = Decimal::from_str(value).map_err(|error| {
            CoreError::InvalidPricing(format!("{name} 不是有效十进制数: {error}"))
        })?;
        if decimal < Decimal::ZERO {
            return Err(CoreError::InvalidPricing(format!("{name} 不能为负数")));
        }
    }
    Ok(input)
}

fn backfill_missing_costs(connection: &Connection) -> Result<u64, CoreError> {
    let mut statement = connection.prepare(
        "SELECT request_id, app_type, model, request_model, pricing_model, cost_multiplier,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics
         FROM proxy_request_logs
         WHERE CAST(total_cost_usd AS REAL) <= 0
           AND (input_tokens > 0 OR output_tokens > 0
                OR cache_read_tokens > 0 OR cache_creation_tokens > 0)",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(BackfillRow {
            request_id: row.get(0)?,
            app_type: row.get(1)?,
            model: row.get(2)?,
            request_model: row.get(3)?,
            pricing_model: row.get(4)?,
            cost_multiplier: row
                .get::<_, Option<String>>(5)?
                .unwrap_or_else(|| "1".to_string()),
            input_tokens: row.get::<_, i64>(6)? as u64,
            output_tokens: row.get::<_, i64>(7)? as u64,
            cache_read_tokens: row.get::<_, i64>(8)? as u64,
            cache_creation_tokens: row.get::<_, i64>(9)? as u64,
            input_token_semantics: row.get(10)?,
        })
    })?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let mut cache = HashMap::new();
    let mut updated = 0;
    for row in rows {
        let Some(pricing) = pricing_for_log(connection, &mut cache, &row)? else {
            continue;
        };
        let multiplier = Decimal::from_str(&row.cost_multiplier).unwrap_or(Decimal::ONE);
        let million = Decimal::from(1_000_000u64);
        let input_tokens = if !is_cache_inclusive_app(&row.app_type)
            || row.input_token_semantics == INPUT_TOKEN_SEMANTICS_FRESH
        {
            row.input_tokens
        } else if row.input_token_semantics == INPUT_TOKEN_SEMANTICS_TOTAL {
            row.input_tokens
                .saturating_sub(row.cache_read_tokens)
                .saturating_sub(row.cache_creation_tokens)
        } else {
            row.input_tokens.saturating_sub(row.cache_read_tokens)
        };
        let input_cost = Decimal::from(input_tokens) * pricing.input / million;
        let output_cost = Decimal::from(row.output_tokens) * pricing.output / million;
        let cache_read_cost = Decimal::from(row.cache_read_tokens) * pricing.cache_read / million;
        let cache_creation_cost =
            Decimal::from(row.cache_creation_tokens) * pricing.cache_creation / million;
        let total_cost =
            (input_cost + output_cost + cache_read_cost + cache_creation_cost) * multiplier;
        connection.execute(
            "UPDATE proxy_request_logs
             SET input_cost_usd = ?1, output_cost_usd = ?2, cache_read_cost_usd = ?3,
                 cache_creation_cost_usd = ?4, total_cost_usd = ?5
             WHERE request_id = ?6",
            params![
                format!("{input_cost:.6}"),
                format!("{output_cost:.6}"),
                format!("{cache_read_cost:.6}"),
                format!("{cache_creation_cost:.6}"),
                format!("{total_cost:.6}"),
                row.request_id,
            ],
        )?;
        updated += 1;
    }
    Ok(updated)
}

fn pricing_for_log(
    connection: &Connection,
    cache: &mut HashMap<String, Option<PricingInfo>>,
    row: &BackfillRow,
) -> Result<Option<PricingInfo>, CoreError> {
    if let Some(pricing_model) = row
        .pricing_model
        .as_deref()
        .filter(|value| !is_placeholder_model(value))
    {
        return pricing_for_model(connection, cache, pricing_model);
    }
    if let Some(pricing) = pricing_for_model(connection, cache, &row.model)? {
        return Ok(Some(pricing));
    }
    if !is_placeholder_model(&row.model) {
        return Ok(None);
    }
    match row.request_model.as_deref() {
        Some(model) if model != row.model => pricing_for_model(connection, cache, model),
        _ => Ok(None),
    }
}

fn pricing_for_model(
    connection: &Connection,
    cache: &mut HashMap<String, Option<PricingInfo>>,
    model: &str,
) -> Result<Option<PricingInfo>, CoreError> {
    if let Some(pricing) = cache.get(model) {
        return Ok(pricing.clone());
    }
    // 归一化只用于查价，不改写日志里的原始模型字段；原始值是排查供应商路由和
    // 后续修复定价 seed 的依据，不能为了命中价格而永久丢失。
    let candidates = model_pricing_candidates(model);
    let mut row = None;
    for candidate in &candidates {
        row = query_model_pricing_exact(connection, candidate)?;
        if row.is_some() {
            break;
        }
    }
    if row.is_none() {
        for candidate in &candidates {
            if should_try_pricing_prefix_match(candidate) {
                row = query_model_pricing_prefix(connection, candidate)?;
                if row.is_some() {
                    break;
                }
            }
        }
    }
    let pricing = row
        .map(
            |(input, output, cache_read, cache_creation)| -> Result<PricingInfo, CoreError> {
                Ok(PricingInfo {
                    input: parse_stored_price("输入", &input)?,
                    output: parse_stored_price("输出", &output)?,
                    cache_read: parse_stored_price("缓存读取", &cache_read)?,
                    cache_creation: parse_stored_price("缓存写入", &cache_creation)?,
                })
            },
        )
        .transpose()?;
    cache.insert(model.to_string(), pricing.clone());
    Ok(pricing)
}

pub(super) fn model_has_pricing(connection: &Connection, model: &str) -> Result<bool, CoreError> {
    pricing_for_model(connection, &mut HashMap::new(), model).map(|pricing| pricing.is_some())
}

fn query_model_pricing_exact(
    connection: &Connection,
    model: &str,
) -> Result<Option<(String, String, String, String)>, CoreError> {
    connection
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing WHERE model_id = ?1",
            [model],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(CoreError::from)
}

fn query_model_pricing_prefix(
    connection: &Connection,
    model: &str,
) -> Result<Option<(String, String, String, String)>, CoreError> {
    connection
        .query_row(
            "SELECT input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing WHERE model_id LIKE ?1
             ORDER BY LENGTH(model_id) ASC LIMIT 1",
            [format!("{model}-%")],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(CoreError::from)
}

fn model_pricing_candidates(model: &str) -> Vec<String> {
    let cleaned = clean_model_id_for_pricing(model);
    if is_placeholder_model(&cleaned) {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut queue = vec![cleaned];
    while let Some(candidate) = queue.pop() {
        if candidate.is_empty() || candidates.iter().any(|value| value == &candidate) {
            continue;
        }
        candidates.push(candidate.clone());
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

fn clean_model_id_for_pricing(model: &str) -> String {
    let normalized = model
        .rsplit_once('/')
        .map_or(model, |(_, value)| value)
        .split(':')
        .next()
        .unwrap_or(model)
        .trim()
        .replace('@', "-")
        .to_ascii_lowercase();
    normalized.trim_end_matches("[1m]").trim().to_string()
}

fn strip_known_model_namespace(model: &str) -> Option<String> {
    if let Some(position) = model.rfind("claude-") {
        if position > 0 {
            return Some(model[position..].to_string());
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
        if let Some(stripped) = model.strip_prefix(marker) {
            return Some(stripped.to_string());
        }
    }
    None
}

fn strip_claude_desktop_non_anthropic_prefix(model: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
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
    let rest = model.strip_prefix("claude-")?;
    MARKERS
        .iter()
        .any(|marker| rest.starts_with(marker))
        .then(|| rest.to_string())
}

fn strip_bedrock_model_version_suffix(model: &str) -> Option<String> {
    let (base, suffix) = model.rsplit_once("-v")?;
    (!base.is_empty() && !suffix.is_empty() && suffix.chars().all(|value| value.is_ascii_digit()))
        .then(|| base.to_string())
}

fn strip_model_date_suffix(model: &str) -> Option<String> {
    let bytes = model.as_bytes();
    if bytes.len() > 11 {
        let start = bytes.len() - 11;
        let suffix = &bytes[start..];
        let iso_date = suffix[0] == b'-'
            && suffix[1..5].iter().all(u8::is_ascii_digit)
            && suffix[5] == b'-'
            && suffix[6..8].iter().all(u8::is_ascii_digit)
            && suffix[8] == b'-'
            && suffix[9..11].iter().all(u8::is_ascii_digit);
        if iso_date {
            return Some(model[..start].to_string());
        }
    }
    let (base, suffix) = model.rsplit_once('-')?;
    if base.is_empty() || !suffix.chars().all(|value| value.is_ascii_digit()) {
        return None;
    }
    if suffix.len() == 8 {
        return Some(base.to_string());
    }
    if suffix.len() == 6 {
        let month = suffix[2..4].parse::<u32>().unwrap_or(0);
        let day = suffix[4..6].parse::<u32>().unwrap_or(0);
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            return Some(base.to_string());
        }
    }
    None
}

fn strip_reasoning_effort_suffix(model: &str) -> Option<String> {
    ["-minimal", "-low", "-medium", "-high", "-xhigh"]
        .into_iter()
        .find_map(|suffix| {
            model
                .strip_suffix(suffix)
                .filter(|stripped| !stripped.is_empty())
                .map(str::to_string)
        })
}

fn should_try_pricing_prefix_match(model: &str) -> bool {
    let dash_count = model.matches('-').count();
    if model.starts_with("claude-") {
        return dash_count >= 3;
    }
    if ["o1", "o3", "o4", "o5"]
        .iter()
        .any(|prefix| model.starts_with(prefix))
    {
        return dash_count >= 1;
    }
    [
        "gpt-",
        "gemini-",
        "deepseek-",
        "qwen-",
        "glm-",
        "kimi-",
        "minimax-",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
        && dash_count >= 2
}

fn parse_stored_price(name: &str, value: &str) -> Result<Decimal, CoreError> {
    Decimal::from_str(value)
        .map_err(|error| CoreError::InvalidPricing(format!("数据库中的{name}价格无效: {error}")))
}

fn is_placeholder_model(model: &str) -> bool {
    matches!(
        model.trim().to_ascii_lowercase().as_str(),
        "" | "unknown" | "null" | "none"
    )
}

fn limit_value(meta: &serde_json::Value, key: &str) -> Option<f64> {
    meta.get(key)
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse().ok())
}

fn usage_cost_for_period(
    connection: &Connection,
    provider_id: &str,
    app_type: &str,
    daily: bool,
) -> Result<f64, CoreError> {
    let (detail_period, rollup_period) = if daily {
        (
            "date(datetime(created_at, 'unixepoch', 'localtime')) = date('now', 'localtime')",
            "date = date('now', 'localtime')",
        )
    } else {
        (
            "strftime('%Y-%m', datetime(created_at, 'unixepoch', 'localtime')) = strftime('%Y-%m', 'now', 'localtime')",
            "strftime('%Y-%m', date) = strftime('%Y-%m', 'now', 'localtime')",
        )
    };
    let sql = format!(
        "SELECT COALESCE(SUM(cost), 0) FROM (
            SELECT CAST(total_cost_usd AS REAL) AS cost FROM proxy_request_logs
            WHERE provider_id = ?1 AND app_type = ?2 AND {detail_period}
            UNION ALL
            SELECT CAST(total_cost_usd AS REAL) FROM usage_daily_rollups
            WHERE provider_id = ?1 AND app_type = ?2 AND {rollup_period}
         )"
    );
    connection
        .query_row(&sql, params![provider_id, app_type], |row| row.get(0))
        .map_err(CoreError::from)
}
