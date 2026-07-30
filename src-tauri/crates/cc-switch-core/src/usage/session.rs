//! 无界面会话用量导入器。
//!
//! 所有路径都从 `HeadlessState::home()` 派生，确保 Agent 只读取所连接主机的 CLI 数据；
//! 解析器不依赖 Tauri、全局 HOME 或桌面配置覆盖，适合作为远程临时进程的稳定边界。

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use rust_decimal::Decimal;

use crate::{CoreError, HeadlessState};

use super::model::{OperationCancellation, SessionSyncResult};
use super::sql::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};

const SESSION_PROXY_DEDUP_WINDOW_SECONDS: i64 = 10 * 60;
const GROK_SETTLE_WINDOW_SECONDS: i64 = SESSION_PROXY_DEDUP_WINDOW_SECONDS;

struct SessionRow {
    request_id: String,
    provider_id: &'static str,
    app_type: &'static str,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    input_token_semantics: i64,
    total_cost: String,
    latency_ms: u64,
    session_id: Option<String>,
    created_at: i64,
    data_source: &'static str,
}

struct GrokTurnContext<'a> {
    session_id: &'a str,
    turn_key: &'a str,
    usage_cost_partial: bool,
    created_at: i64,
}

/// 跨 proxy/session 去重只比较一次请求可稳定观测的字段；provider_id 不参与，
/// 因为 session 日志无法知道当时由哪个 Provider 实际处理。
pub(super) struct SessionFingerprint<'a> {
    pub app_type: &'a str,
    pub model: &'a str,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub created_at: i64,
}

pub(super) fn sync_non_codex_sessions(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let mut aggregate = SessionSyncResult::default();
    merge_source(&mut aggregate, "Claude", sync_claude(state, cancellation));
    cancellation.check()?;
    merge_source(&mut aggregate, "Gemini", sync_gemini(state, cancellation));
    cancellation.check()?;
    merge_source(
        &mut aggregate,
        "OpenCode",
        sync_opencode(state, cancellation),
    );
    cancellation.check()?;
    merge_source(
        &mut aggregate,
        "Grok Build",
        sync_grokbuild(state, cancellation),
    );
    cancellation.check()?;
    Ok(aggregate)
}

fn merge_source(
    aggregate: &mut SessionSyncResult,
    source: &str,
    result: Result<SessionSyncResult, CoreError>,
) {
    match result {
        Ok(result) => aggregate.merge(result),
        Err(error) if error.code() == "REMOTE_OPERATION_CANCELLED" => {
            aggregate.errors.push(error.to_string());
        }
        Err(error) => aggregate.errors.push(format!("{source} 同步失败: {error}")),
    }
}

fn sync_claude(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let root = state.home().join(".claude").join("projects");
    let mut files = Vec::new();
    // 深度 8 覆盖 projects/<project>/<session>/subagents/workflows/<workflow>/*.jsonl，
    // 同时保持固定上限，防止异常目录树拖垮临时 Agent。
    collect_named_files(&root, None, Some("jsonl"), 8, cancellation, &mut files)?;
    files.sort();
    let mut result = scanned_result(&files);
    for path in files {
        cancellation.check()?;
        let (should_scan, modified) = file_requires_scan(state, &path)?;
        if !should_scan {
            continue;
        }
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                result.errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let mut current_session = None;
        let mut line_count = 0_i64;
        for line in BufReader::new(file).lines() {
            cancellation.check()?;
            line_count = line_count.saturating_add(1);
            let Ok(line) = line else { continue };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if current_session.is_none() {
                current_session = string_at(&value, &["sessionId"]);
            }
            if string_at(&value, &["type"]).as_deref() != Some("assistant") {
                continue;
            }
            let Some(message) = value.get("message") else {
                continue;
            };
            let Some(message_id) = string_at(message, &["id"]) else {
                continue;
            };
            let Some(usage) = message.get("usage") else {
                continue;
            };
            let input = u64_at(usage, &["input_tokens"]);
            let output = u64_at(usage, &["output_tokens"]);
            let cache_read = u64_at(usage, &["cache_read_input_tokens"]);
            let cache_creation = u64_at(usage, &["cache_creation_input_tokens"]);
            if input + output + cache_read + cache_creation == 0 {
                continue;
            }
            let session_id = current_session
                .clone()
                .or_else(|| string_at(&value, &["sessionId"]));
            let row = SessionRow {
                request_id: format!(
                    "claude_session:{}:{message_id}",
                    session_id.as_deref().unwrap_or("unknown")
                ),
                provider_id: "_claude_session",
                app_type: "claude",
                model: string_at(message, &["model"]).unwrap_or_else(|| "unknown".to_string()),
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_creation_tokens: cache_creation,
                input_token_semantics: INPUT_TOKEN_SEMANTICS_FRESH,
                total_cost: "0".to_string(),
                latency_ms: 0,
                session_id,
                created_at: timestamp_at(&value, &["timestamp"]).unwrap_or_else(now_epoch),
                data_source: "claude_session",
            };
            record_row(state, row, &mut result)?;
        }
        update_sync_state(state, &path, modified, line_count)?;
    }
    Ok(result)
}

fn sync_gemini(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let root = state.home().join(".gemini").join("tmp");
    let mut files = Vec::new();
    collect_named_files(
        &root,
        Some("session-"),
        Some("json"),
        3,
        cancellation,
        &mut files,
    )?;
    files.sort();
    let mut result = scanned_result(&files);
    for path in files {
        cancellation.check()?;
        let (should_scan, modified) = file_requires_scan(state, &path)?;
        if !should_scan {
            continue;
        }
        let value = match read_json(&path) {
            Ok(value) => value,
            Err(error) => {
                result.errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let session_id = string_at(&value, &["sessionId"]);
        let Some(messages) = value.get("messages").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for message in messages {
            cancellation.check()?;
            if string_at(message, &["type"]).as_deref() != Some("gemini") {
                continue;
            }
            let Some(tokens) = message.get("tokens") else {
                continue;
            };
            let input = u64_at(tokens, &["input"]);
            let output = u64_at(tokens, &["output"]).saturating_add(u64_at(tokens, &["thoughts"]));
            let cache_read = u64_at(tokens, &["cached"]);
            if input + output + cache_read == 0 {
                continue;
            }
            let message_id = string_at(message, &["id"]).unwrap_or_else(|| "unknown".to_string());
            let row = SessionRow {
                request_id: format!(
                    "gemini_session:{}:{message_id}",
                    session_id.as_deref().unwrap_or("unknown")
                ),
                provider_id: "_gemini_session",
                app_type: "gemini",
                model: string_at(message, &["model"]).unwrap_or_else(|| "unknown".to_string()),
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_creation_tokens: 0,
                input_token_semantics: INPUT_TOKEN_SEMANTICS_TOTAL,
                total_cost: "0".to_string(),
                latency_ms: 0,
                session_id: session_id.clone(),
                created_at: timestamp_at(message, &["timestamp"]).unwrap_or_else(now_epoch),
                data_source: "gemini_session",
            };
            record_row(state, row, &mut result)?;
        }
        update_sync_state(state, &path, modified, messages.len() as i64)?;
    }
    Ok(result)
}

fn sync_opencode(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let path = state
        .home()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db");
    if !path.is_file() {
        return Ok(SessionSyncResult::default());
    }
    cancellation.check()?;
    let modified = opencode_modified_nanos(&path)?;
    if !watermark_requires_scan(state, &path, modified)? {
        return Ok(SessionSyncResult {
            files_scanned: 1,
            ..SessionSyncResult::default()
        });
    }
    let source = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = source.prepare(
        "SELECT m.id, m.session_id, m.data
         FROM message m
         WHERE json_extract(m.data, '$.role') = 'assistant'
           AND json_extract(m.data, '$.tokens') IS NOT NULL
           AND json_extract(m.data, '$.time.completed') IS NOT NULL
         ORDER BY m.time_created, m.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..SessionSyncResult::default()
    };
    for (message_id, session_id, data) in rows {
        cancellation.check()?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) else {
            result.skipped = result.skipped.saturating_add(1);
            continue;
        };
        let Some(tokens) = value.get("tokens") else {
            continue;
        };
        let input = u64_at(tokens, &["input"]);
        let output = u64_at(tokens, &["output"]).saturating_add(u64_at(tokens, &["reasoning"]));
        let cache_read = u64_at(tokens, &["cache", "read"]);
        let cache_creation = u64_at(tokens, &["cache", "write"]);
        if input + output + cache_read + cache_creation == 0 {
            continue;
        }
        let total_cost = value
            .get("cost")
            .and_then(serde_json::Value::as_f64)
            .filter(|cost| cost.is_finite() && *cost > 0.0)
            .map(|cost| cost.to_string())
            .unwrap_or_else(|| "0".to_string());
        let row = SessionRow {
            request_id: format!("opencode_session:{session_id}:{message_id}"),
            provider_id: "_opencode_session",
            app_type: "opencode",
            model: string_at(&value, &["modelID"]).unwrap_or_else(|| "unknown".to_string()),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            input_token_semantics: INPUT_TOKEN_SEMANTICS_FRESH,
            total_cost,
            latency_ms: 0,
            session_id: Some(session_id),
            created_at: i64_at(&value, &["time", "created"])
                .map(|value| value / 1_000)
                .unwrap_or_else(now_epoch),
            data_source: "opencode_session",
        };
        record_row(state, row, &mut result)?;
    }
    update_sync_state(state, &path, modified, result.imported as i64)?;
    Ok(result)
}

fn sync_grokbuild(
    state: &HeadlessState,
    cancellation: &OperationCancellation,
) -> Result<SessionSyncResult, CoreError> {
    let mut files = Vec::new();
    for root in ["sessions", "archived_sessions"] {
        collect_named_files(
            &state.home().join(".grok").join(root),
            Some("updates.jsonl"),
            None,
            4,
            cancellation,
            &mut files,
        )?;
    }
    files.sort();
    let mut result = scanned_result(&files);
    for path in files {
        cancellation.check()?;
        let (should_scan, modified) = file_requires_scan(state, &path)?;
        if !should_scan {
            continue;
        }
        let session_id = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                result.errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let now = now_epoch();
        let mut deferred = false;
        for (index, line) in BufReader::new(file).lines().enumerate() {
            cancellation.check()?;
            let Ok(line) = line else { continue };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if string_at(&value, &["method"]).as_deref() != Some("_x.ai/session/update") {
                continue;
            }
            let Some(update) = value.pointer("/params/update") else {
                continue;
            };
            if let Some(kind) = string_at(update, &["sessionUpdate"]) {
                if kind != "turn_completed" {
                    continue;
                }
            }
            let Some(usage) = update.get("usage") else {
                continue;
            };
            let created_at = timestamp_at(&value, &["timestamp"])
                .or_else(|| i64_at(&value, &["timestamp"]).map(normalize_epoch))
                .unwrap_or_else(now_epoch);
            // 活跃文件最后一轮可能仍在追加成本/modelUsage；延后整条及其后事件，下一轮
            // 轮询会重读文件，避免先写半截数据后因稳定 request ID 无法纠正。
            if now.saturating_sub(created_at) < GROK_SETTLE_WINDOW_SECONDS {
                deferred = true;
                break;
            }
            let turn_key = string_at(update, &["prompt_id"])
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("idx{index}"));
            let usage_cost_partial = usage
                .get("costIsPartial")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let models = usage
                .get("modelUsage")
                .and_then(serde_json::Value::as_object)
                .map(|models| models.iter().collect::<Vec<_>>())
                .unwrap_or_default();
            if models.is_empty() {
                record_grok_row(
                    state,
                    "unknown",
                    usage,
                    &GrokTurnContext {
                        session_id: &session_id,
                        turn_key: &turn_key,
                        usage_cost_partial,
                        created_at,
                    },
                    &mut result,
                )?;
            } else {
                for (model, counters) in models {
                    record_grok_row(
                        state,
                        model,
                        counters,
                        &GrokTurnContext {
                            session_id: &session_id,
                            turn_key: &turn_key,
                            usage_cost_partial,
                            created_at,
                        },
                        &mut result,
                    )?;
                }
            }
        }
        if deferred {
            result.deferred_files = result.deferred_files.saturating_add(1);
        } else {
            update_sync_state(state, &path, modified, 0)?;
        }
    }
    Ok(result)
}

fn record_grok_row(
    state: &HeadlessState,
    model: &str,
    counters: &serde_json::Value,
    context: &GrokTurnContext<'_>,
    result: &mut SessionSyncResult,
) -> Result<(), CoreError> {
    let input = u64_at(counters, &["inputTokens"]);
    let output = u64_at(counters, &["outputTokens"]);
    let cache_read = u64_at(counters, &["cachedReadTokens"]);
    if input + output + cache_read == 0 {
        return Ok(());
    }
    if state.with_connection(|connection| {
        has_recent_proxy_activity(connection, "grokbuild", context.created_at)
    })? {
        // Grok 会话是逐轮聚合，无法与逐请求 proxy 行做 token 指纹一一对应；同一
        // 时间窗口出现任何代理活动就以 proxy 为权威，方向保守地避免双算。
        result.skipped = result.skipped.saturating_add(1);
        return Ok(());
    }
    let reported_ticks = u64_at(counters, &["costUsdTicks"]);
    let counter_cost_partial = counters
        .get("costIsPartial")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let has_pricing = state
        .with_connection(|connection| super::mutation::model_has_pricing(connection, model))?;
    let total_cost = if reported_ticks == 0
        || ((context.usage_cost_partial || counter_cost_partial) && has_pricing)
    {
        "0".to_string()
    } else {
        (Decimal::from(reported_ticks) / Decimal::from(10_000_000_000_u64)).to_string()
    };
    record_row(
        state,
        SessionRow {
            request_id: format!(
                "grok_session:{}:{}:{model}",
                context.session_id, context.turn_key
            ),
            provider_id: "_grok_session",
            app_type: "grokbuild",
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: 0,
            input_token_semantics: INPUT_TOKEN_SEMANTICS_TOTAL,
            total_cost,
            latency_ms: u64_at(counters, &["apiDurationMs"]),
            session_id: Some(context.session_id.to_string()),
            created_at: context.created_at,
            data_source: "grok_session",
        },
        result,
    )
}

fn record_row(
    state: &HeadlessState,
    row: SessionRow,
    result: &mut SessionSyncResult,
) -> Result<(), CoreError> {
    let changed = state.with_connection(|connection| {
        if has_matching_proxy_row(connection, &row)? {
            return Ok(0);
        }
        Ok(connection.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, pricing_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics, input_cost_usd, output_cost_usd,
                cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, status_code, session_id, provider_type, is_streaming,
                cost_multiplier, created_at, data_source
             ) VALUES (
                ?1, ?2, ?3, ?4, ?4, ?4, ?5, ?6, ?7, ?8, ?9,
                '0', '0', '0', '0', ?10, ?11, 200, ?12, ?13, 1, '1', ?14, ?15
             )
             ON CONFLICT(request_id) DO UPDATE SET
                model = excluded.model,
                request_model = excluded.request_model,
                pricing_model = excluded.pricing_model,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_creation_tokens = excluded.cache_creation_tokens,
                input_token_semantics = excluded.input_token_semantics,
                input_cost_usd = '0',
                output_cost_usd = '0',
                cache_read_cost_usd = '0',
                cache_creation_cost_usd = '0',
                total_cost_usd = excluded.total_cost_usd,
                latency_ms = excluded.latency_ms,
                session_id = excluded.session_id,
                created_at = excluded.created_at
             WHERE proxy_request_logs.data_source = excluded.data_source
               AND (
                    proxy_request_logs.model != excluded.model
                 OR proxy_request_logs.input_tokens != excluded.input_tokens
                 OR proxy_request_logs.output_tokens != excluded.output_tokens
                 OR proxy_request_logs.cache_read_tokens != excluded.cache_read_tokens
                 OR proxy_request_logs.cache_creation_tokens != excluded.cache_creation_tokens
                 OR proxy_request_logs.input_token_semantics != excluded.input_token_semantics
                 OR proxy_request_logs.total_cost_usd != excluded.total_cost_usd
                 OR proxy_request_logs.created_at != excluded.created_at
               )",
            params![
                row.request_id,
                row.provider_id,
                row.app_type,
                row.model,
                row.input_tokens as i64,
                row.output_tokens as i64,
                row.cache_read_tokens as i64,
                row.cache_creation_tokens as i64,
                row.input_token_semantics,
                row.total_cost,
                row.latency_ms as i64,
                row.session_id,
                row.data_source,
                row.created_at,
                row.data_source,
            ],
        )?)
    })?;
    if changed == 1 {
        result.imported = result.imported.saturating_add(1);
    } else {
        result.skipped = result.skipped.saturating_add(1);
    }
    Ok(())
}

fn has_matching_proxy_row(connection: &Connection, row: &SessionRow) -> Result<bool, CoreError> {
    has_matching_proxy_usage(
        connection,
        &SessionFingerprint {
            app_type: row.app_type,
            model: &row.model,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_creation_tokens: row.cache_creation_tokens,
            created_at: row.created_at,
        },
    )
}

pub(super) fn has_matching_proxy_usage(
    connection: &Connection,
    fingerprint: &SessionFingerprint<'_>,
) -> Result<bool, CoreError> {
    let allow_unknown_cache_creation =
        matches!(fingerprint.app_type, "codex" | "gemini" | "opencode")
            && fingerprint.cache_creation_tokens == 0;
    connection
        .query_row(
            "SELECT EXISTS (
                SELECT 1 FROM proxy_request_logs
                WHERE data_source = 'proxy'
                  AND app_type = ?1
                  AND status_code BETWEEN 200 AND 299
                  AND input_tokens = ?2
                  AND output_tokens = ?3
                  AND cache_read_tokens = ?4
                  AND (cache_creation_tokens = ?5 OR ?9 = 1)
                  AND created_at BETWEEN ?6 - ?7 AND ?6 + ?7
                  AND (
                       LOWER(model) = LOWER(?8)
                    OR LOWER(model) = 'unknown'
                    OR LOWER(?8) = 'unknown'
                  )
             )",
            params![
                fingerprint.app_type,
                fingerprint.input_tokens as i64,
                fingerprint.output_tokens as i64,
                fingerprint.cache_read_tokens as i64,
                fingerprint.cache_creation_tokens as i64,
                fingerprint.created_at,
                SESSION_PROXY_DEDUP_WINDOW_SECONDS,
                fingerprint.model,
                allow_unknown_cache_creation as i64,
            ],
            |db_row| db_row.get(0),
        )
        .map_err(CoreError::from)
}

fn has_recent_proxy_activity(
    connection: &Connection,
    app_type: &str,
    created_at: i64,
) -> Result<bool, CoreError> {
    connection
        .query_row(
            "SELECT EXISTS (
                SELECT 1 FROM proxy_request_logs
                WHERE data_source = 'proxy'
                  AND app_type = ?1
                  AND created_at BETWEEN ?2 - ?3 AND ?2 + ?3
             )",
            params![app_type, created_at, SESSION_PROXY_DEDUP_WINDOW_SECONDS],
            |row| row.get(0),
        )
        .map_err(CoreError::from)
}

fn collect_named_files(
    directory: &Path,
    name_or_prefix: Option<&str>,
    extension: Option<&str>,
    depth: usize,
    cancellation: &OperationCancellation,
    files: &mut Vec<PathBuf>,
) -> Result<(), CoreError> {
    cancellation.check()?;
    if depth == 0 || !directory.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(directory).map_err(|source| CoreError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        cancellation.check()?;
        let entry = entry.map_err(|source| CoreError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|source| CoreError::Io {
                path: path.clone(),
                source,
            })?
            .is_dir()
        {
            collect_named_files(
                &path,
                name_or_prefix,
                extension,
                depth - 1,
                cancellation,
                files,
            )?;
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let name_matches = name_or_prefix
            .is_none_or(|expected| file_name == expected || file_name.starts_with(expected));
        let extension_matches = extension.is_none_or(|expected| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(expected))
        });
        if name_matches && extension_matches {
            files.push(path);
        }
    }
    Ok(())
}

fn scanned_result(files: &[PathBuf]) -> SessionSyncResult {
    SessionSyncResult {
        files_scanned: files.len() as u32,
        ..SessionSyncResult::default()
    }
}

fn file_requires_scan(state: &HeadlessState, path: &Path) -> Result<(bool, i64), CoreError> {
    let metadata = fs::metadata(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let modified = metadata_modified_nanos(&metadata);
    Ok((watermark_requires_scan(state, path, modified)?, modified))
}

fn opencode_modified_nanos(path: &Path) -> Result<i64, CoreError> {
    let metadata = fs::metadata(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut modified = metadata_modified_nanos(&metadata);
    let wal_path = path.with_extension("db-wal");
    if let Ok(metadata) = fs::metadata(&wal_path) {
        modified = modified.max(metadata_modified_nanos(&metadata));
    }
    Ok(modified)
}

pub(super) fn metadata_modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub(super) fn watermark_requires_scan(
    state: &HeadlessState,
    path: &Path,
    modified: i64,
) -> Result<bool, CoreError> {
    let key = path.to_string_lossy();
    let last_modified = state.with_connection(|connection| {
        Ok(connection
            .query_row(
                "SELECT last_modified FROM session_log_sync WHERE file_path = ?1",
                [key.as_ref()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0))
    })?;
    Ok(modified > last_modified)
}

pub(super) fn update_sync_state(
    state: &HeadlessState,
    path: &Path,
    modified: i64,
    offset: i64,
) -> Result<(), CoreError> {
    let key = path.to_string_lossy();
    state.with_connection(|connection| {
        connection.execute(
            "INSERT INTO session_log_sync (
                file_path, last_modified, last_line_offset, last_synced_at
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(file_path) DO UPDATE SET
                last_modified = excluded.last_modified,
                last_line_offset = excluded.last_line_offset,
                last_synced_at = excluded.last_synced_at",
            params![key.as_ref(), modified, offset, now_epoch()],
        )?;
        Ok(())
    })
}

fn read_json(path: &Path) -> Result<serde_json::Value, CoreError> {
    let content = fs::read_to_string(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&content).map_err(|error| {
        CoreError::RemoteBusiness(format!("JSON 解析失败 {}: {error}", path.display()))
    })
}

fn value_at<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

fn string_at(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    value_at(value, path)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn u64_at(value: &serde_json::Value, path: &[&str]) -> u64 {
    value_at(value, path)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}

fn i64_at(value: &serde_json::Value, path: &[&str]) -> Option<i64> {
    value_at(value, path).and_then(serde_json::Value::as_i64)
}

fn timestamp_at(value: &serde_json::Value, path: &[&str]) -> Option<i64> {
    value_at(value, path)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
}

fn normalize_epoch(value: i64) -> i64 {
    if value > 100_000_000_000 {
        value / 1_000
    } else {
        value
    }
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
