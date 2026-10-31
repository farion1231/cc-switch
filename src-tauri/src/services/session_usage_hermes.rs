//! Hermes 会话用量导入
//!
//! 从 Hermes Agent 的本地 `state.db`（SQLite，WAL 模式，时间戳为 epoch 秒
//! REAL）提取精确 token 使用数据。数据格式以 NousResearch/hermes-agent 上游
//! 实现为准（`hermes_state_schema.py` / `hermes_state_usage.py`，2026-10 核实）。
//!
//! ## 数据流
//! ```text
//! <hermes_home>/state.db
//!   → session_model_usage 表（schema v20+，按 (session, model, 计费路由, task) 聚合）
//!   → 去重 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 要点
//! - `session_model_usage` 的主键是 (session_id, model, billing_provider,
//!   billing_base_url, billing_mode, task)，行是累计值（Hermes 以 delta upsert）。
//!   同一行会随会话继续而增长，因此重导走「补全」而非 INSERT OR IGNORE。
//! - `task` 区分用量的用途：`''` 是主 agent 循环，辅助调用记任务名（vision、
//!   compression、title_generation、voice_chat…）。所有行都是真实计费，全部导入。
//! - `input_tokens` 是 Anthropic 风格的新鲜输入（OpenAI/Codex 形态的响应在
//!   Hermes 归一化时已减去 cache read/write），导入时显式写 FRESH 语义；
//!   reasoning token 单列存储且不计入 output，按 OpenCode 先例并入 output
//!   （按输出计费；Anthropic 形态该列为 0，并入无副作用）。
//! - 不做 schema v20 之前旧库（sessions 表聚合列）的回退导入：旧聚合把多个
//!   模型的用量混在会话行里，与 v20 迁移后的按模型拆分无法对齐，回退会在
//!   Hermes 升级后双算。v20 之前的库等 Hermes 自身迁移后再导入。
//! - `sessions/*.jsonl` 是对话导出（`hermes sessions export` / `/save`），不含
//!   用量字段，不参与导入。
//! - Hermes 无代理 handler，正常情况下不会有同请求的代理行；与 OpenCode 相同
//!   保留指纹去重，兜住用户把 Hermes 手动指到本软件代理端口的场景。
//! - 已知限制：只读主 profile 的 state.db（`HERMES_HOME` / 设置覆盖后的目录），
//!   不遍历 `profiles/<name>/` 子目录，与会话管理器的 Hermes 会话列表同口径。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::hermes_config::get_hermes_dir;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, update_sync_state, SessionSyncResult, SyncCursor,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rusqlite::OptionalExtension;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::time::SystemTime;

const APP_TYPE: &str = "hermes";
/// proxy_request_logs.provider_id / provider_type 的占位值
const PROVIDER_ID: &str = "_hermes_session";
const DATA_SOURCE: &str = "hermes_session";
const REQUEST_ID_PREFIX: &str = "hermes_session";

/// 同步 Hermes 使用数据
pub fn sync_hermes_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let db_path = get_hermes_dir().join("state.db");
    if !db_path.exists() {
        return Ok(SessionSyncResult::default());
    }
    let db_path_str = db_path.to_string_lossy().to_string();

    // state.db 运行在 WAL 模式：新提交先落在 -wal 文件里，主库文件只有在
    // checkpoint 时才更新，因此 mtime 门必须同时考虑 -wal（同 OpenCode）。
    let metadata = fs::metadata(&db_path)
        .map_err(|e| AppError::Config(format!("无法读取 Hermes state.db 元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);
    if let Ok(wal_meta) = fs::metadata(db_path.with_extension("db-wal")) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }

    let cursors = load_sync_cursors(db)?;
    if file_modified <= cursors.get(&db_path_str).map_or(0, |c| c.last_modified) {
        return Ok(SessionSyncResult {
            files_scanned: 1,
            ..SessionSyncResult::default()
        });
    }

    let source =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| AppError::Database(format!("无法打开 Hermes state.db: {e}")))?;

    if !table_exists(&source, "session_model_usage")? {
        // v20 之前的库没有按模型的用量归因表；不回退 sessions 聚合列（见模块文档）
        return advance_file_cursor(
            db,
            &db_path_str,
            file_modified,
            SessionSyncResult::default(),
        );
    }

    let mut result = sync_modern(db, &source, &db_path_str, file_modified, &cursors)?;
    result.files_scanned = 1;
    if result.imported > 0 {
        log::info!(
            "[HERMES-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个会话",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }
    Ok(result)
}

// ── 现代路径：session_model_usage（schema v20+）─────────────────────

/// 每会话增量：会话的同步水位是它所有用量行的 MAX(last_seen)。
/// 水位存微秒（不是毫秒）：last_seen 是 sub-second 精度的 REAL 秒，取毫秒
/// 会让同一毫秒内的两次 delta 永久漏掉后一次；微秒在 f64 精度内无损。
fn sync_modern(
    db: &Database,
    source: &rusqlite::Connection,
    db_path_str: &str,
    file_modified: i64,
    cursors: &HashMap<String, SyncCursor>,
) -> Result<SessionSyncResult, AppError> {
    let sessions: Vec<(String, f64)> = {
        let mut stmt = source.prepare(
            "SELECT session_id, MAX(last_seen) FROM session_model_usage \
             WHERE last_seen IS NOT NULL GROUP BY session_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut result = SessionSyncResult::default();
    let mut has_sync_errors = false;
    for (session_id, watermark_secs) in &sessions {
        let watermark = epoch_secs_to_micros(*watermark_secs);
        let sync_key = format!("{db_path_str}:{session_id}");
        if watermark <= cursors.get(&sync_key).map_or(0, |c| c.last_modified) {
            continue; // 会话用量未更新，跳过
        }

        match sync_session_usage_rows(db, source, session_id) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Hermes 会话用量导入失败 {session_id}: {e}");
                log::warn!("[HERMES-SYNC] {msg}");
                result.errors.push(msg);
                result.skipped += 1;
                has_sync_errors = true;
                continue;
            }
        }

        // 会话级游标：导入失败时不推进，确保下次可重试该会话
        if let Err(e) = update_sync_state(db, &sync_key, watermark, 0) {
            let msg = format!("Hermes 会话同步状态更新失败 {session_id}: {e}");
            log::warn!("[HERMES-SYNC] {msg}");
            result.errors.push(msg);
            has_sync_errors = true;
        }
    }

    // 仅在本轮完全成功时推进文件级状态；否则保留下次重试入口
    if !has_sync_errors {
        update_sync_state(db, db_path_str, file_modified, 0)?;
    }
    Ok(result)
}

/// 读取并导入一个会话的全部 session_model_usage 行（主循环 + 辅助 task）。
fn sync_session_usage_rows(
    db: &Database,
    source: &rusqlite::Connection,
    session_id: &str,
) -> Result<(u32, u32), AppError> {
    // v20/v21 的表没有 task 列（v22 才加入主键），弹性补空串；
    // sessions 的边缘时间列跨版本存在差异，同样弹性读取
    let usage_columns = table_columns(source, "session_model_usage")?;
    let session_columns = table_columns(source, "sessions")?;
    let has = |columns: &[String], name: &str| columns.iter().any(|c| c == name);
    let usage_col = |name: &str| {
        if has(&usage_columns, name) {
            name.to_string()
        } else {
            format!("NULL AS {name}")
        }
    };
    let session_col = |name: &str| {
        if has(&session_columns, name) {
            format!("s.{name}")
        } else {
            "NULL".to_string()
        }
    };
    let sql = format!(
        "SELECT u.model, {billing_provider}, {billing_base_url}, {billing_mode}, {task}, \
                u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_write_tokens, \
                u.reasoning_tokens, u.estimated_cost_usd, u.actual_cost_usd, \
                u.first_seen, u.last_seen, \
                {last_activity_at}, {ended_at}, {started_at} \
         FROM session_model_usage u LEFT JOIN sessions s ON s.id = u.session_id \
         WHERE u.session_id = ?1",
        billing_provider = usage_col("billing_provider"),
        billing_base_url = usage_col("billing_base_url"),
        billing_mode = usage_col("billing_mode"),
        task = usage_col("task"),
        last_activity_at = session_col("last_activity_at"),
        ended_at = session_col("ended_at"),
        started_at = session_col("started_at"),
    );

    let mut rows: Vec<HermesUsageRow> = {
        let mut stmt = source.prepare(&sql)?;
        let rows = stmt.query_map([session_id], |row| {
            Ok(HermesUsageRow {
                model: row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "unknown".into()),
                pk_tail: (
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                ),
                input_tokens: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                output_tokens: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                cache_read_tokens: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                cache_write_tokens: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                reasoning_tokens: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                estimated_cost: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
                actual_cost: row.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
                first_seen_secs: row.get::<_, Option<f64>>(12)?,
                last_seen_secs: row.get::<_, Option<f64>>(13)?,
                session_edge_secs: row
                    .get::<_, Option<f64>>(14)?
                    .or(row.get::<_, Option<f64>>(15)?)
                    .or(row.get::<_, Option<f64>>(16)?),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    rows.sort_by(|a, b| {
        a.last_seen_secs
            .partial_cmp(&b.last_seen_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| AppError::Database(format!("启动 Hermes 用量导入事务失败: {e}")))?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    for row in &rows {
        if !row.has_billable_tokens() {
            skipped += 1;
            continue;
        }
        let request_id = format!("{REQUEST_ID_PREFIX}:{session_id}:{}", row.pk_suffix());
        match upsert_usage_row(&tx, &request_id, row, session_id) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                let msg = format!("Hermes 用量写入失败 {request_id}: {e}");
                log::warn!("[HERMES-SYNC] {msg}");
                return Err(e);
            }
        }
    }
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Hermes 用量导入事务失败: {e}")))?;
    Ok((imported, skipped))
}

// ── 行结构与写入 ────────────────────────────────────────────────────

/// session_model_usage 的一行，token 为累计值。
struct HermesUsageRow {
    model: String,
    /// (billing_provider, billing_base_url, billing_mode, task)，主键去掉 session_id 的部分
    pk_tail: (String, String, String, String),
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    estimated_cost: f64,
    actual_cost: f64,
    first_seen_secs: Option<f64>,
    last_seen_secs: Option<f64>,
    session_edge_secs: Option<f64>,
}

impl HermesUsageRow {
    /// 只要产生了真实计费 token 就导入（与 Claude 路径的判定同口径）
    fn has_billable_tokens(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_write_tokens > 0
            || self.reasoning_tokens > 0
    }

    /// request_id 后缀：PK 去掉 session_id 后的域分隔哈希，同会话内唯一且稳定
    fn pk_suffix(&self) -> String {
        let (provider, base_url, mode, task) = &self.pk_tail;
        let mut hasher = Sha256::new();
        hasher.update(b"hermes-usage-pk-v1");
        for part in [
            self.model.as_str(),
            provider.as_str(),
            base_url.as_str(),
            mode.as_str(),
            task.as_str(),
        ] {
            hasher.update([0x1f]);
            hasher.update(part.as_bytes());
        }
        let digest = hasher.finalize();
        digest[..8].iter().map(|b| format!("{b:02x}")).collect()
    }

    /// created_at（秒）：优先 last_seen（该组最后一次计费时刻），回退首见与
    /// 会话边缘时间
    fn created_at_secs(&self) -> i64 {
        self.last_seen_secs
            .or(self.first_seen_secs)
            .or(self.session_edge_secs)
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(|s| s.round() as i64)
            .unwrap_or_else(now_secs)
    }

    /// native 费用：Hermes 自己的计费优先实际值，其次估算值（同 Hermes
    /// usage_totals 的偏好）
    fn native_cost(&self) -> Option<f64> {
        [self.actual_cost, self.estimated_cost]
            .into_iter()
            .find(|c| c.is_finite() && *c > 0.0)
    }
}

/// 插入或补全一行用量。返回 true 表示入库（新增或补全），false 表示跳过。
fn upsert_usage_row(
    conn: &rusqlite::Connection,
    request_id: &str,
    row: &HermesUsageRow,
    session_id: &str,
) -> Result<bool, AppError> {
    let created_at = row.created_at_secs();
    // reasoning 不在 output 里（OpenAI 形态单列），按输出计费并入 output
    let output_with_reasoning = row.output_tokens + row.reasoning_tokens;

    let stored: Option<(String, i64, i64, i64, i64, f64)> = conn
        .prepare_cached(
            "SELECT COALESCE(data_source, 'proxy'), input_tokens, output_tokens, \
                    cache_read_tokens, cache_creation_tokens, total_cost_usd \
             FROM proxy_request_logs WHERE request_id = ?1",
        )?
        .query_row([request_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get::<_, String>(5)?.parse::<f64>().unwrap_or(0.0),
            ))
        })
        .optional()
        .map_err(|e| AppError::Database(format!("查询已入库的 Hermes 用量失败: {e}")))?;

    if let Some((data_source, s_in, s_out, s_read, s_write, s_cost)) = stored {
        if data_source != DATA_SOURCE {
            return Ok(false);
        }
        // Hermes 的行是累计值：任一 token 维度增长或费用上调就补全
        let native_cost = row.native_cost();
        let grew = row.input_tokens > s_in
            || output_with_reasoning > s_out
            || row.cache_read_tokens > s_read
            || row.cache_write_tokens > s_write
            || native_cost.is_some_and(|c| c > s_cost + f64::EPSILON);
        if !grew {
            return Ok(false);
        }
        let [input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost] =
            cost_split(conn, row, output_with_reasoning);
        conn.execute(
            "UPDATE proxy_request_logs SET \
                input_tokens = ?1, output_tokens = ?2, cache_read_tokens = ?3, \
                cache_creation_tokens = ?4, input_cost_usd = ?5, output_cost_usd = ?6, \
                cache_read_cost_usd = ?7, cache_creation_cost_usd = ?8, total_cost_usd = ?9 \
             WHERE request_id = ?10",
            rusqlite::params![
                row.input_tokens,
                output_with_reasoning,
                row.cache_read_tokens,
                row.cache_write_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                request_id,
            ],
        )
        .map_err(|e| AppError::Database(format!("补全 Hermes 用量失败: {e}")))?;
        return Ok(true);
    }

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model: &row.model,
        input_tokens: row.input_tokens.clamp(0, u32::MAX as i64) as u32,
        output_tokens: output_with_reasoning.clamp(0, u32::MAX as i64) as u32,
        cache_read_tokens: row.cache_read_tokens.clamp(0, u32::MAX as i64) as u32,
        cache_creation_tokens: row.cache_write_tokens.clamp(0, u32::MAX as i64) as u32,
        created_at,
    };
    if should_skip_session_insert(conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let [input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost] =
        cost_split(conn, row, output_with_reasoning);

    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, first_token_ms, status_code, error_message,
                session_id, provider_type, is_streaming, cost_multiplier, created_at,
                data_source, input_token_semantics
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            rusqlite::params![
                request_id,
                PROVIDER_ID,
                APP_TYPE,
                row.model,
                row.model,             // request_model = model
                row.input_tokens,
                output_with_reasoning,
                row.cache_read_tokens,
                row.cache_write_tokens,
                input_cost,
                output_cost,
                cache_read_cost,
                cache_creation_cost,
                total_cost,
                0i64,                  // 聚合行没有请求计时
                Option::<i64>::None,   // first_token_ms
                200i64,
                Option::<String>::None,
                Some(session_id),
                Some(PROVIDER_ID),
                1i64,                  // is_streaming
                "1.0",                 // cost_multiplier
                created_at,
                DATA_SOURCE,
                INPUT_TOKEN_SEMANTICS_FRESH,
            ],
        )
        .map_err(|e| AppError::Database(format!("插入 Hermes 用量失败: {e}")))?;
    Ok(inserted > 0)
}

/// 五项费用：有 native 费用全部计入 total（聚合值无法拆分，同 OpenCode）；
/// 没有则用本软件的模型定价计算。
fn cost_split(
    conn: &rusqlite::Connection,
    row: &HermesUsageRow,
    output_with_reasoning: i64,
) -> [String; 5] {
    if let Some(cost) = row.native_cost() {
        return [
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            cost.to_string(),
        ];
    }
    let Some(pricing) = find_model_pricing(conn, &row.model) else {
        return std::array::from_fn(|_| "0".to_string());
    };
    let usage = TokenUsage {
        input_tokens: row.input_tokens.clamp(0, u32::MAX as i64) as u32,
        output_tokens: output_with_reasoning.clamp(0, u32::MAX as i64) as u32,
        cache_read_tokens: row.cache_read_tokens.clamp(0, u32::MAX as i64) as u32,
        cache_creation_tokens: row.cache_write_tokens.clamp(0, u32::MAX as i64) as u32,
        cache_creation_1h_tokens: 0,
        model: Some(row.model.clone()),
        message_id: None,
    };
    let cost = CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, Decimal::from(1));
    [
        cost.input_cost.to_string(),
        cost.output_cost.to_string(),
        cost.cache_read_cost.to_string(),
        cost.cache_creation_cost.to_string(),
        cost.total_cost.to_string(),
    ]
}

// ── 小工具 ──────────────────────────────────────────────────────────

fn table_exists(conn: &rusqlite::Connection, table: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(|e| AppError::Database(format!("查询表 {table} 失败: {e}")))
}

/// PRAGMA table_info 的列名列表（Hermes 各版本的列集不定，弹性读取）
fn table_columns(conn: &rusqlite::Connection, table: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| AppError::Database(format!("读取 {table} 表结构失败: {e}")))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(format!("读取 {table} 表结构失败: {e}")))
}

/// Hermes 时间戳是 epoch 秒 REAL；同步水位用微秒整数，避免毫秒取整截断
fn epoch_secs_to_micros(secs: f64) -> i64 {
    if !secs.is_finite() || secs <= 0.0 {
        return 0;
    }
    (secs * 1_000_000.0).round() as i64
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 推进文件级游标（无可导入数据时的收尾）
fn advance_file_cursor(
    db: &Database,
    db_path_str: &str,
    file_modified: i64,
    mut result: SessionSyncResult,
) -> Result<SessionSyncResult, AppError> {
    result.files_scanned = 1;
    update_sync_state(db, db_path_str, file_modified, 0)?;
    Ok(result)
}
