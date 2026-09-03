//! CodeBuddy（腾讯 WorkBuddy / CodeBuddy Code CLI）会话日志使用追踪
//!
//! 从 `~/.codebuddy/projects/` 下的 JSONL 会话文件中提取 token 使用数据，
//! 实现代理关闭时的使用统计。
//!
//! ## 数据源格式
//! CodeBuddy 的会话转录按工作目录组织：
//! ```text
//! ~/.codebuddy/projects/<workspace-dir>/<sessionId>.jsonl
//! ```
//! 每行一个 JSON 对象。一次 API 调用产生多条行（reasoning / message /
//! function_call），其中只有**末尾事件**携带 `providerData.usage`：
//!
//! ```json
//! {
//!   "type": "function_call",
//!   "timestamp": 1788329200987,
//!   "sessionId": "2f8a7d30-...",
//!   "providerData": {
//!     "messageId": "fe2b7991de6744f3ac6e86f8c5b62864",
//!     "model": "glm-5.3",
//!     "requestModelId": "auto",
//!     "traceId": "77b7d14d...",
//!     "usage": {
//!       "requests": 1,
//!       "inputTokens": 21817,
//!       "outputTokens": 425,
//!       "totalTokens": 22242,
//!       "inputTokensDetails": [{ "cached_tokens": 12544 }],
//!       "outputTokensDetails": [{ "reasoning_tokens": 355 }]
//!     }
//!   }
//! }
//! ```
//!
//! `messageId` 每次调用唯一，作为去重主键；`inputTokens` 为 OpenAI 语义
//! （已包含 `cached_tokens`），因此 app 归入 [`CACHE_INCLUSIVE_APP_TYPES`]，
//! 行级写入 `input_token_semantics = TOTAL`。
//!
//! ## 数据流
//! ```text
//! ~/.codebuddy/projects/**/*.jsonl → 增量解析 → messageId 去重 → 费用计算 → proxy_request_logs 表
//! ```

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, SessionSyncResult, SyncCursor,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const APP_TYPE: &str = "codebuddy";
const DATA_SOURCE: &str = "codebuddy_session";
const PROVIDER_PLACEHOLDER: &str = "_codebuddy_session";
const UNKNOWN_MODEL: &str = "unknown";
/// projects/ 下的目录深度上限：projects/<workspace 编码目录>[/子目录]/session.jsonl。
/// 正常只有一层工作目录；多一层余量兼容未来的嵌套布局（如子 agent）。
const MAX_DIR_DEPTH: usize = 3;

/// 获取 CodeBuddy 配置目录（`$CODEBUDDY_HOME` 或 `~/.codebuddy`）。
fn get_codebuddy_config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("CODEBUDDY_HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home);
        }
    }
    crate::config::get_home_dir().join(".codebuddy")
}

/// 从 JSONL 行中解析出的单次 API 调用用量。
#[derive(Debug, Clone)]
struct ParsedCodeBuddyUsage {
    message_id: String,
    model: String,
    request_model: Option<String>,
    /// OpenAI 语义：已包含 cache_read_tokens。
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    /// OpenAI 协议不单独计量缓存写入，恒为 0。
    cache_creation_tokens: u32,
    /// 毫秒级 Unix 时间戳。
    timestamp_ms: i64,
    session_id: Option<String>,
}

/// 同步 CodeBuddy 会话日志到使用统计数据库。
pub fn sync_codebuddy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let projects_dir = get_codebuddy_config_dir().join("projects");
    if !projects_dir.exists() {
        return Ok(SessionSyncResult::default());
    }

    let mut result = SessionSyncResult::default();
    let jsonl_files = collect_codebuddy_jsonl_files(&projects_dir);
    let cursors = load_sync_cursors(db)?;

    for file_path in &jsonl_files {
        result.files_scanned += 1;
        let cursor = cursors.get(file_path.to_string_lossy().as_ref());
        match sync_single_file(db, file_path, cursor) {
            Ok(file_sync) => {
                result.imported += file_sync.imported;
                result.skipped += file_sync.skipped;
                if file_sync.incomplete_tail {
                    result.deferred_files += 1;
                }
            }
            Err(e) => {
                let msg = format!("{}: {e}", file_path.display());
                log::warn!("[CODEBUDDY-SYNC] 文件解析失败: {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[CODEBUDDY-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 递归收集 projects/ 下的 `.jsonl` 会话文件（限深防意外符号链接环）。
/// `tool-results/` 里只有 `.txt`，按扩展名自然排除。
fn collect_codebuddy_jsonl_files(projects_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_jsonl_dir(projects_dir, 0, &mut files);
    files.sort();
    files
}

fn walk_jsonl_dir(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > MAX_DIR_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_jsonl_dir(&path, depth + 1, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

/// 单文件同步结果。
#[derive(Debug, Default)]
struct CodeBuddyFileSync {
    imported: u32,
    skipped: u32,
    /// 尾部存在未以 `\n` 终结的残段（写入方可能正在追加）。残段能解析成功
    /// 仍会导入（request_id 主键去重防双算），但游标不越过它。
    incomplete_tail: bool,
}

/// 同步单个 JSONL 文件（增量字节游标）。
///
/// 游标语义与 Claude 路径一致：`last_byte_offset` 只推进到最后一个完整行
/// 之后，追加式增长时只读新增尾部；文件被外部截断（游标超出文件大小）时
/// 钉到当前 EOF、不重放旧区间（重放会在 `rollup_and_prune` 剪掉明细后
/// 二次累加统计）。同尺寸外部重写概率极低，且 `request_id` 主键 +
/// `INSERT OR IGNORE` 兜底保证同一 messageId 永远只落库一次。
fn sync_single_file(
    db: &Database,
    file_path: &Path,
    cursor: Option<&SyncCursor>,
) -> Result<CodeBuddyFileSync, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);
    let file_size = metadata.len() as i64;

    let last_modified = cursor.map_or(0, |c| c.last_modified);
    let last_byte_offset = cursor.and_then(|c| c.last_byte_offset);

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok(CodeBuddyFileSync::default());
    }

    let mut file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;

    // 截断检测：游标越过 EOF 说明文件被外部截断/重写。钉到当前 EOF、
    // 不重放旧区间，之后的追加恢复正常增量。
    let start_byte = match last_byte_offset {
        Some(offset) if !(0..=file_size).contains(&offset) => {
            log::warn!(
                "[CODEBUDDY-SYNC] 文件被外部截断，游标钉至 EOF、不重放旧区间: {}",
                file_path.display()
            );
            let conn = lock_conn!(db.conn);
            update_codebuddy_sync_state_on_conn(&conn, &file_path_str, file_modified, file_size)?;
            return Ok(CodeBuddyFileSync::default());
        }
        other => other.unwrap_or(0),
    };
    if start_byte > 0 {
        file.seek(std::io::SeekFrom::Start(start_byte as u64))
            .map_err(|e| AppError::Config(format!("无法定位游标偏移: {e}")))?;
    }

    let mut reader = BufReader::new(file);
    let mut committed_offset = start_byte;
    let mut incomplete_tail = false;
    let mut messages: HashMap<String, ParsedCodeBuddyUsage> = HashMap::new();
    let mut buf: Vec<u8> = Vec::new();

    loop {
        buf.clear();
        let read = match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break, // 读错误：已解析部分照常入库，游标停在完整行
        };
        if buf.ends_with(b"\n") {
            committed_offset += read as i64;
        } else {
            incomplete_tail = true;
        }

        if buf.iter().all(u8::is_ascii_whitespace) {
            continue;
        }

        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&buf) else {
            continue;
        };

        let Some(parsed) = parse_usage_line(&value) else {
            continue;
        };

        // 同 messageId 多条（防御流式多块）：保留 totalTokens 最大者
        let should_replace = messages.get(&parsed.message_id).is_none_or(|existing| {
            parsed.input_tokens + parsed.output_tokens
                > existing.input_tokens + existing.output_tokens
        });
        if should_replace {
            messages.insert(parsed.message_id.clone(), parsed);
        }
    }

    // 写入数据库：单次取锁 + 事务，插入与游标推进原子提交
    let mut imported: u32 = 0;
    let mut skipped: u32 = 0;

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| AppError::Database(format!("启动会话用量导入事务失败: {e}")))?;

    let mut ordered: Vec<&ParsedCodeBuddyUsage> = messages.values().collect();
    ordered.sort_by_key(|m| m.timestamp_ms);

    for msg in ordered {
        match insert_codebuddy_entry_on_conn(&tx, msg) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[CODEBUDDY-SYNC] 插入失败 ({}): {e}", msg.message_id);
                skipped += 1;
            }
        }
    }

    update_codebuddy_sync_state_on_conn(&tx, &file_path_str, file_modified, committed_offset)?;
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交会话用量导入事务失败: {e}")))?;

    Ok(CodeBuddyFileSync {
        imported,
        skipped,
        incomplete_tail,
    })
}

/// 解析单行 JSONL。只有携带 `providerData.usage` 的事件（一次 API 调用的
/// 末尾事件）才会返回 Some。
fn parse_usage_line(value: &serde_json::Value) -> Option<ParsedCodeBuddyUsage> {
    let provider_data = value.get("providerData")?;
    let usage = provider_data.get("usage")?;
    let message_id = provider_data
        .get("messageId")
        .and_then(|v| v.as_str())?
        .to_string();

    let input_tokens = usage
        .get("inputTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .get("outputTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    // OpenAI 风格：inputTokens 已包含 cached_tokens
    let cache_read_tokens = usage
        .get("inputTokensDetails")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // 任一计费维度 > 0 才有意义；全 0 占位行跳过
    if input_tokens == 0 && output_tokens == 0 && cache_read_tokens == 0 {
        return None;
    }

    let timestamp_ms = value
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0)
        });

    Some(ParsedCodeBuddyUsage {
        message_id,
        model: provider_data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(UNKNOWN_MODEL)
            .to_string(),
        request_model: provider_data
            .get("requestModelId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens: 0,
        timestamp_ms,
        session_id: value
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

/// 写入 CodeBuddy 路径的字节游标。
fn update_codebuddy_sync_state_on_conn(
    conn: &rusqlite::Connection,
    file_path: &str,
    last_modified: i64,
    byte_offset: i64,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.prepare_cached(
        "INSERT OR REPLACE INTO session_log_sync
             (file_path, last_modified, last_line_offset, last_synced_at, last_byte_offset)
         VALUES (?1, ?2, 0, ?3, ?4)",
    )
    .and_then(|mut stmt| {
        stmt.execute(rusqlite::params![
            file_path,
            last_modified,
            now,
            byte_offset
        ])
    })
    .map_err(|e| AppError::Database(format!("更新同步状态失败: {e}")))?;
    Ok(())
}

/// 插入单条 CodeBuddy 会话日志到 proxy_request_logs。
/// 返回是否为新插入（false = 已存在被去重跳过）。
fn insert_codebuddy_entry_on_conn(
    conn: &rusqlite::Connection,
    msg: &ParsedCodeBuddyUsage,
) -> Result<bool, AppError> {
    let created_at = (msg.timestamp_ms / 1000).clamp(0, i64::MAX);

    let request_id = format!("codebuddy_session:{}", msg.message_id);

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model: &msg.model,
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        created_at,
    };
    if should_skip_session_insert(conn, &request_id, &dedup_key)? {
        return Ok(false);
    }

    // 费用计算：calculate_for_app 按 CACHE_INCLUSIVE_APP_TYPES 语义
    // 自动从 input_tokens 中扣除 cache_read 部分再按输入价计费
    let usage = TokenUsage {
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        model: Some(msg.model.clone()),
        message_id: None,
    };

    let pricing: Option<ModelPricing> = find_model_pricing(conn, &msg.model);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app(APP_TYPE, &usage, &p, Decimal::ONE);
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
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            rusqlite::params![
                request_id,
                PROVIDER_PLACEHOLDER,  // provider_id: 标记为会话来源
                APP_TYPE,              // app_type: codebuddy
                msg.model,
                msg.request_model.as_deref().unwrap_or(&msg.model),
                msg.input_tokens,
                msg.output_tokens,
                msg.cache_read_tokens,
                msg.cache_creation_tokens,
                INPUT_TOKEN_SEMANTICS_TOTAL, // input 已含 cache read（OpenAI 语义）
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,                  // latency_ms: 会话日志无此数据
                Option::<i64>::None,   // first_token_ms
                200i64,                // status_code: 产生计费 token 即视为成功
                Option::<String>::None,
                msg.session_id,
                Some(DATA_SOURCE),     // provider_type
                1i64,                  // is_streaming
                "1.0",                 // cost_multiplier
                created_at,
                DATA_SOURCE,           // data_source: codebuddy_session
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 CodeBuddy 会话日志失败: {e}")))?;

    Ok(inserted_rows > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一行带 usage 的 CodeBuddy JSONL（模拟 function_call 末尾事件）
    fn usage_line(message_id: &str, input: u32, output: u32, cached: u32) -> String {
        format!(
            r#"{{"id":"evt-1","parentId":null,"timestamp":1788329200987,"type":"function_call","callId":"call_1","name":"Bash","arguments":"{{}}","sessionId":"sess-1","cwd":"/workspace","providerData":{{"messageId":"{message_id}","model":"glm-5.3","requestModelId":"auto","requestModelName":"Auto","traceId":"t1","usage":{{"requests":1,"inputTokens":{input},"outputTokens":{output},"totalTokens":{},"inputTokensDetails":[{{"cached_tokens":{cached}}}],"outputTokensDetails":[{{"reasoning_tokens":0}}]}}}}}}"#,
            input + output
        )
    }

    fn plain_message_line(message_id: &str, input: u32, output: u32) -> String {
        // 纯文本回复（无工具调用）时 usage 挂在 assistant message 行上
        format!(
            r#"{{"id":"evt-2","parentId":null,"timestamp":1788329209999,"type":"message","role":"assistant","status":"completed","content":[{{"type":"text","text":"ok"}}],"sessionId":"sess-1","cwd":"/workspace","providerData":{{"messageId":"{message_id}","model":"glm-5.3","usage":{{"requests":1,"inputTokens":{input},"outputTokens":{output},"totalTokens":{},"inputTokensDetails":[{{"cached_tokens":0}}],"outputTokensDetails":[{{"reasoning_tokens":0}}]}}}}}}"#,
            input + output
        )
    }

    fn no_usage_line(event_type: &str) -> String {
        format!(
            r#"{{"id":"evt-3","timestamp":1788329210000,"type":"{event_type}","sessionId":"sess-1","cwd":"/workspace","providerData":{{"messageId":"m0","model":"glm-5.3"}}}}"#
        )
    }

    #[test]
    fn test_parse_usage_line_extracts_tokens() {
        let line = usage_line("msg_a", 21817, 425, 12544);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        let parsed = parse_usage_line(&value).expect("usage 行必须被解析");
        assert_eq!(parsed.message_id, "msg_a");
        assert_eq!(parsed.model, "glm-5.3");
        assert_eq!(parsed.request_model.as_deref(), Some("auto"));
        assert_eq!(parsed.input_tokens, 21817);
        assert_eq!(parsed.output_tokens, 425);
        assert_eq!(parsed.cache_read_tokens, 12544);
        assert_eq!(parsed.cache_creation_tokens, 0);
        assert_eq!(parsed.timestamp_ms, 1788329200987);
        assert_eq!(parsed.session_id.as_deref(), Some("sess-1"));
    }

    #[test]
    fn test_parse_skips_lines_without_usage() {
        for t in [
            "message",
            "reasoning",
            "function_call",
            "file-history-snapshot",
        ] {
            let line = no_usage_line(t);
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert!(
                parse_usage_line(&value).is_none(),
                "无 usage 的 {t} 行应跳过"
            );
        }
    }

    #[test]
    fn test_parse_skips_all_zero_usage() {
        let line = usage_line("msg_zero", 0, 0, 0);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(parse_usage_line(&value).is_none(), "全 0 token 行应跳过");
    }

    #[test]
    fn test_parse_plain_message_usage() {
        let line = plain_message_line("msg_text", 100, 20);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        let parsed = parse_usage_line(&value).expect("纯文本回复的 usage 必须被解析");
        assert_eq!(parsed.input_tokens, 100);
        assert_eq!(parsed.output_tokens, 20);
    }

    #[test]
    fn test_sync_single_file_imports_and_dedups() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-cb-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("sess-1.jsonl");

        let content = format!(
            "{}\n{}\n{}\n",
            no_usage_line("reasoning"),
            usage_line("msg_a", 21817, 425, 12544),
            plain_message_line("msg_b", 100, 20)
        );
        fs::write(&file, content).unwrap();

        let first = sync_single_file(&db, &file, None)?;
        assert_eq!(first.imported, 2, "msg_a 与 msg_b 各导入一条");

        // 验证落库字段
        {
            let conn = lock_conn!(db.conn);
            let (input_tokens, cache_read, semantics, app_type, data_source): (
                i64, i64, i64, String, String,
            ) = conn.query_row(
                "SELECT input_tokens, cache_read_tokens, input_token_semantics, app_type, data_source
                 FROM proxy_request_logs WHERE request_id = 'codebuddy_session:msg_a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
            assert_eq!((input_tokens, cache_read), (21817, 12544));
            assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_TOTAL);
            assert_eq!(app_type, "codebuddy");
            assert_eq!(data_source, "codebuddy_session");
        }

        // 无变化的第二轮：mtime 门拦截，零导入
        let second = sync_with_cursor(&db, &file);
        assert_eq!((second.imported, second.skipped), (0, 0));

        // 追加新事件后再同步：只导入新增
        let mut new_content = fs::read_to_string(&file).unwrap();
        new_content.push_str(&format!("{}\n", usage_line("msg_c", 500, 30, 0)));
        fs::write(&file, new_content).unwrap();
        bump_mtime(&file);
        let third = sync_with_cursor(&db, &file);
        assert_eq!(third.imported, 1);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_incomplete_tail_imports_but_holds_cursor() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-cb-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("sess-2.jsonl");

        let first_line = format!("{}\n", usage_line("msg_a", 100, 10, 0));
        let tail = usage_line("msg_tail", 200, 20, 0); // 无换行的完整 JSON
        fs::write(&file, format!("{first_line}{tail}")).unwrap();

        let first = sync_single_file(&db, &file, None)?;
        assert_eq!(first.imported, 2, "无换行的完整尾段也必须导入");
        assert!(first.incomplete_tail);

        // 游标不越过未终结尾段；补全换行后重扫靠 request_id 去重
        {
            let cursors = load_sync_cursors(&db)?;
            let cursor = cursors.get(file.to_string_lossy().as_ref()).unwrap();
            assert_eq!(cursor.last_byte_offset, Some(first_line.len() as i64));
        }

        let mut content = fs::read_to_string(&file).unwrap();
        content.push_str(&format!("\n{}\n", usage_line("msg_c", 300, 30, 0)));
        fs::write(&file, content).unwrap();
        bump_mtime(&file);
        let second = sync_with_cursor(&db, &file);
        assert_eq!((second.imported, second.skipped), (1, 1));

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_truncated_file_pins_cursor_at_eof() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = std::env::temp_dir().join(format!("cc-switch-cb-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("sess-3.jsonl");

        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                usage_line("msg_a", 100, 10, 0),
                usage_line("msg_b", 200, 20, 0)
            ),
        )
        .unwrap();
        assert_eq!(sync_single_file(&db, &file, None)?.imported, 2);

        // 外部截断：只剩 msg_a
        fs::write(&file, format!("{}\n", usage_line("msg_a", 100, 10, 0))).unwrap();
        bump_mtime(&file);
        let rescan = sync_with_cursor(&db, &file);
        assert_eq!(
            (rescan.imported, rescan.skipped),
            (0, 0),
            "截断后不重放旧区间"
        );

        let size = fs::metadata(&file).unwrap().len() as i64;
        let cursors = load_sync_cursors(&db)?;
        let cursor = cursors.get(file.to_string_lossy().as_ref()).unwrap();
        assert_eq!(cursor.last_byte_offset, Some(size), "游标钉在当前 EOF");

        // 钉住后追加恢复正常增量
        let mut content = fs::read_to_string(&file).unwrap();
        content.push_str(&format!("{}\n", usage_line("msg_c", 300, 30, 0)));
        fs::write(&file, content).unwrap();
        bump_mtime(&file);
        let after = sync_with_cursor(&db, &file);
        assert_eq!(after.imported, 1, "追加恢复增量");

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_collect_jsonl_files_recursively() {
        let tmp = std::env::temp_dir().join(format!("cc-switch-cb-test-{}", uuid::Uuid::new_v4()));
        // projects/<workspace-dir>/<sessionId>.jsonl 及更深一层
        let deep = tmp.join("workspace-dir").join("nested");
        fs::create_dir_all(&deep).unwrap();
        fs::write(tmp.join("workspace-dir").join("sess-1.jsonl"), "{}").unwrap();
        fs::write(deep.join("sub-1.jsonl"), "{}").unwrap();
        // tool-results 里的 .txt 不应被收集
        let tool_results_dir = tmp.join("workspace-dir").join("tool-results");
        fs::create_dir_all(&tool_results_dir).unwrap();
        fs::write(tool_results_dir.join("call_1.txt"), "").unwrap();

        let files = collect_codebuddy_jsonl_files(&tmp);
        assert_eq!(files.len(), 2, "只收集 .jsonl，深度 2 内");

        fs::remove_dir_all(&tmp).ok();
    }

    fn sync_with_cursor(db: &Database, path: &Path) -> CodeBuddyFileSync {
        let cursors = load_sync_cursors(db).unwrap();
        let cursor = cursors.get(path.to_string_lossy().as_ref()).copied();
        sync_single_file(db, path, cursor.as_ref()).unwrap()
    }

    fn bump_mtime(path: &Path) {
        let later = SystemTime::now() + std::time::Duration::from_secs(2);
        let file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(later))
            .unwrap();
    }
}
