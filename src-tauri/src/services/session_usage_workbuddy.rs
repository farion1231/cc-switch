//! Import WorkBuddy's per-request model usage from its project session logs.
//!
//! WorkBuddy 在本机可能双版本共存：国内版落在 `~/.workbuddy`，海外版
//! (WorkBuddy AI) 落在 `~/.workbuddy-ai`，目录结构与会话格式完全一致，合并
//! 统计到同一个 app 下。每个会话是 `projects/<项目>/<uuid>.jsonl` 的明文
//! JSONL，OpenAI Responses 风格事件流。token 用量挂在 `function_call` /
//! `message` 事件的 `providerData.usage`（归一化 camelCase；部分事件只有
//! `message.usage` 蛇形版本），一次 API 请求一条——带工具调用的中间轮在
//! function_call 上，最终回答在 message 上。
//!
//! 语义与 ZCode 一致：inputTokens 已含缓存读取（OpenAI prompt_tokens 风格），
//! cache read 从 inputTokensDetails 拆出，cache creation 单独上报；输出里的
//! reasoning_tokens 已含在 outputTokens 中，不要再加。文件按 uuid 命名、只
//! 追加，用「mtime 没变就跳过 + 字节偏移续读」做增量。

use crate::config::get_home_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
    SyncCursor,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::find_model_pricing;
use rusqlite::params;
use rust_decimal::Decimal;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 双版本的会话根目录：国内版 `~/.workbuddy`、海外版 `~/.workbuddy-ai`。
/// 哪个不存在就空转哪个，互不影响。
fn default_project_roots() -> Vec<PathBuf> {
    let home = get_home_dir();
    vec![
        home.join(".workbuddy").join("projects"),
        home.join(".workbuddy-ai").join("projects"),
    ]
}

pub fn sync_workbuddy_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    // 测试/验证用的单根覆盖：指向哪个安装就只扫哪个
    let roots = match std::env::var_os("WORKBUDDY_HOME") {
        Some(home) if !home.is_empty() => vec![PathBuf::from(home).join("projects")],
        _ => default_project_roots(),
    };
    sync_from_roots(db, &roots)
}

pub(super) fn sync_from_roots(
    db: &Database,
    roots: &[PathBuf],
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    let cursors = load_sync_cursors(db)?;
    let mut conn = crate::database::lock_conn!(db.conn);
    let tx = conn.transaction()?;
    for root in roots {
        let Ok(projects) = fs::read_dir(root) else {
            continue;
        };
        for project in projects.flatten() {
            let Ok(sessions) = fs::read_dir(project.path()) else {
                continue;
            };
            for session in sessions.flatten() {
                let path = session.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                sync_session_file(&tx, &path, &cursors, &mut result)?;
            }
        }
    }
    tx.commit()?;
    Ok(result)
}

fn sync_session_file(
    tx: &rusqlite::Transaction,
    path: &Path,
    cursors: &std::collections::HashMap<String, SyncCursor>,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    result.files_scanned += 1;
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let key = format!("workbuddy:{}", path.display());
    let modified_nanos = metadata_modified_nanos(&metadata);
    // 字节偏移游标：mtime 与上次一致就整文件跳过。偏移经
    // update_sync_state_on_conn 存在 last_line_offset 列（沿用 Pi 在该列
    // 编码 revision 的先例，它承载各解析器自定义的水位语义）。
    let start_offset = match cursors.get(&key) {
        Some(cursor) if cursor.last_modified == modified_nanos => return Ok(()),
        Some(cursor) => cursor.last_line_offset.min(metadata.len() as i64),
        None => 0,
    };
    let session_id = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let file = File::open(path).map_err(|source| AppError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(start_offset as u64)).is_err() {
        return Ok(());
    }
    let mut offset = start_offset;
    let mut line = String::new();
    loop {
        line.clear();
        let read = match reader.read_line(&mut line) {
            Ok(read) => read,
            Err(error) => {
                log::debug!(
                    "workbuddy session log read error ({}): {error}",
                    path.display()
                );
                break;
            }
        };
        if read == 0 {
            break;
        }
        // 尾部未写完的半行留给下一轮：WorkBuddy 正在追加时最后一段可能只落了
        // 半行 JSON，此时推进偏移会让这行永远丢失（与 Claude 导入器的
        // test_partial_line_completed_later_is_not_lost 同源问题）
        if !line.ends_with('\n') {
            break;
        }
        offset += read as i64;
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            import_event(tx, &event, &session_id, offset, result)?;
        }
    }
    update_sync_state_on_conn(tx, &key, modified_nanos, offset)?;
    Ok(())
}

fn import_event(
    tx: &rusqlite::Transaction,
    event: &Value,
    session_id: &str,
    end_offset: i64,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    let provider_data = event.get("providerData");
    let usage = provider_data
        .and_then(|pd| pd.get("usage"))
        .or_else(|| event.get("message").and_then(|m| m.get("usage")))
        .and_then(|u| u.as_object())
        .filter(|u| {
            u.get("inputTokens")
                .or_else(|| u.get("input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
                || u.get("outputTokens")
                    .or_else(|| u.get("output_tokens"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0
        });
    let Some(usage) = usage else {
        return Ok(());
    };
    let field = |camel: &str, snake: &str| -> u32 {
        usage
            .get(camel)
            .or_else(|| usage.get(snake))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    };
    let input_tokens = field("inputTokens", "input_tokens");
    let output_tokens = field("outputTokens", "output_tokens");
    // 缓存读取：归一化版藏在 inputTokensDetails[0].cached_tokens，蛇形版平铺
    let cache_read = usage
        .get("inputTokensDetails")
        .and_then(Value::as_array)
        .and_then(|details| details.first())
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| usage.get("cache_read_input_tokens").and_then(Value::as_u64))
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let cache_creation = field("cache_creation_input_tokens", "cache_creation_input_tokens");
    let model = provider_data
        .and_then(|pd| pd.get("model"))
        .or_else(|| event.get("message").and_then(|m| m.get("model")))
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or("unknown")
        .to_string();
    let request_id = match event.get("id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => format!("wb:{id}"),
        // 兜底用「会话 + 行末字节偏移」，追加式文件里偏移稳定
        _ => format!("wb:{session_id}@{end_offset}"),
    };
    let created_at_ms = event.get("timestamp").and_then(Value::as_i64).unwrap_or(0);
    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        cache_creation_1h_tokens: 0,
        model: Some(model.clone()),
        message_id: None,
    };
    let cost = find_model_pricing(tx, &model)
        .map(|pricing| {
            CostCalculator::calculate_for_app("workbuddy", &usage, &pricing, Decimal::ONE)
                .total_cost
                .to_string()
        })
        .unwrap_or_else(|| "0".into());
    let changed = tx.execute(
        "INSERT OR IGNORE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
            total_cost_usd, latency_ms, status_code, session_id, provider_type,
            is_streaming, cost_multiplier, created_at, data_source, input_token_semantics
         ) VALUES (?1, '_workbuddy_session', 'workbuddy', ?2, ?2, ?3, ?4, ?5, ?6,
                   '0', '0', '0', '0', ?7, 0, 200, ?8, 'workbuddy_session',
                   1, '1', ?9, 'workbuddy_session', ?10)",
        params![
            request_id,
            model,
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            usage.cache_creation_tokens,
            cost,
            session_id,
            created_at_ms / 1000,
            INPUT_TOKEN_SEMANTICS_TOTAL
        ],
    )?;
    result.imported += changed as u32;
    result.skipped += u32::from(changed == 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_project(root: &Path, session: &str, lines: &[Value]) -> PathBuf {
        // 真实布局：projects/<项目>/<uuid>.jsonl，文件直接在项目目录下
        let dir = root.join("proj");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{session}.jsonl"));
        let mut content = String::new();
        for line in lines {
            content.push_str(&line.to_string());
            content.push('\n');
        }
        fs::write(&path, content).unwrap();
        path
    }

    fn call(seq: i64, id: &str, model: &str, input: u64, output: u64, cached: u64) -> Value {
        serde_json::json!({
            "id": id,
            "timestamp": 1792155733222_i64 + seq * 1000,
            "type": "function_call",
            "providerData": {
                "model": model,
                "usage": {
                    "requests": 1,
                    "inputTokens": input,
                    "outputTokens": output,
                    "totalTokens": input + output,
                    "inputTokensDetails": [{"cached_tokens": cached}],
                    "outputTokensDetails": [{"reasoning_tokens": 0}],
                },
            },
        })
    }

    #[test]
    fn workbuddy_usage_imports_cache_inclusive_rows() {
        let root = std::env::temp_dir().join(format!("wb-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_project(
            &root,
            "sess-1",
            &[
                serde_json::json!({"id": "u1", "timestamp": 1, "type": "message", "message": {"role": "user"}}),
                call(1, "c1", "hy3", 39590, 293, 4096),
            ],
        );
        let db = Database::memory().unwrap();
        {
            // 无定价路径：先清掉内存库播种的定价
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM model_pricing", []).unwrap();
        }
        let result = sync_from_roots(&db, std::slice::from_ref(&root)).unwrap();
        assert_eq!(result.imported, 1);
        {
            let conn = db.conn.lock().unwrap();
            let row = conn
                .query_row(
                    "SELECT model, input_tokens, output_tokens, cache_read_tokens,
                            cache_creation_tokens, data_source, input_token_semantics,
                            session_id, created_at
                     FROM proxy_request_logs WHERE app_type='workbuddy'",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, i64>(8)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "hy3");
            assert_eq!((row.1, row.2, row.3, row.4), (39590, 293, 4096, 0));
            assert_eq!(row.5, "workbuddy_session");
            assert_eq!(row.6, INPUT_TOKEN_SEMANTICS_TOTAL);
            assert_eq!(row.7, "sess-1");
            assert_eq!(row.8, (1792155733222 + 1000) / 1000);
            // cache-inclusive：fresh input 应同时扣掉缓存读取
            let fresh: i64 = conn
                .query_row(
                    &format!(
                        "SELECT {} FROM proxy_request_logs WHERE app_type='workbuddy'",
                        crate::services::sql_helpers::fresh_input_sql("")
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(fresh, 39590 - 4096);
        }
        // mtime 未变：整文件跳过
        assert_eq!(
            sync_from_roots(&db, std::slice::from_ref(&root))
                .unwrap()
                .imported,
            0
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workbuddy_usage_reads_snake_case_message_usage() {
        let root = std::env::temp_dir().join(format!("wb-snake-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_project(
            &root,
            "sess-2",
            &[serde_json::json!({
                "id": "m1",
                "timestamp": 1792155733222_i64,
                "type": "message",
                "message": {
                    "role": "assistant",
                    "model": "glm-5.3",
                    "usage": {"input_tokens": 1000, "output_tokens": 50,
                              "total_tokens": 1050, "cache_read_input_tokens": 600,
                              "cache_creation_input_tokens": 100},
                },
            })],
        );
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_roots(&db, std::slice::from_ref(&root))
                .unwrap()
                .imported,
            1
        );
        let conn = db.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
                 FROM proxy_request_logs WHERE app_type='workbuddy'",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?, r.get::<_, i64>(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "glm-5.3");
        assert_eq!((row.1, row.2, row.3, row.4), (1000, 50, 600, 100));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workbuddy_usage_appends_incrementally_and_never_reimports_pruned_rows() {
        let root = std::env::temp_dir().join(format!("wb-inc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_project(&root, "sess-3", &[call(1, "c1", "hy3", 10, 10, 0)]);
        let db = Database::memory().unwrap();
        assert_eq!(
            sync_from_roots(&db, std::slice::from_ref(&root))
                .unwrap()
                .imported,
            1
        );
        // 模拟 rollup 剪明细 + 会话继续追加
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DELETE FROM proxy_request_logs", []).unwrap();
        }
        // 重写文件 = 旧内容 + 新事件（mtime 变化触发重读，但字节偏移只读新增部分）
        write_project(
            &root,
            "sess-3",
            &[
                call(1, "c1", "hy3", 10, 10, 0),
                call(2, "c2", "hy3", 20, 20, 0),
            ],
        );
        let result = sync_from_roots(&db, std::slice::from_ref(&root)).unwrap();
        assert_eq!(result.imported, 1, "只应导入新增行，被剪旧行不得重导");
        {
            let conn = db.conn.lock().unwrap();
            let inputs: Vec<i64> = {
                let mut stmt = conn
                    .prepare("SELECT input_tokens FROM proxy_request_logs ORDER BY input_tokens")
                    .unwrap();
                stmt.query_map([], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert_eq!(inputs, vec![20]);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workbuddy_usage_merges_both_installs_and_skips_missing_root() {
        // 国内版与海外版两个安装根合并统计；不存在的根直接跳过
        let cn = std::env::temp_dir().join(format!("wb-cn-{}", std::process::id()));
        let overseas = std::env::temp_dir().join(format!("wb-ai-{}", std::process::id()));
        let missing = std::env::temp_dir().join(format!("wb-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cn);
        let _ = fs::remove_dir_all(&overseas);
        let _ = fs::remove_dir_all(&missing);
        write_project(&cn, "cn-sess", &[call(1, "cn-1", "hy3", 11, 11, 0)]);
        write_project(
            &overseas,
            "ai-sess",
            &[call(1, "ai-1", "deepseek-v4.1-flash", 22, 22, 0)],
        );
        let db = Database::memory().unwrap();
        let result = sync_from_roots(&db, &[cn.clone(), missing, overseas.clone()]).unwrap();
        assert_eq!(result.imported, 2);
        {
            let conn = db.conn.lock().unwrap();
            let rows: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT model FROM proxy_request_logs WHERE app_type='workbuddy' ORDER BY model")
                    .unwrap();
                stmt.query_map([], |r| r.get(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            assert_eq!(rows, ["deepseek-v4.1-flash", "hy3"]);
        }
        let _ = fs::remove_dir_all(&cn);
        let _ = fs::remove_dir_all(&overseas);
    }

    #[test]
    fn workbuddy_usage_partial_tail_line_completed_later_is_not_lost() {
        // WorkBuddy 追加中途被扫描：最后一行只有半截 JSON，偏移必须停在
        // 半行行首，等下一轮补全后再导入（对应 Claude 导入器的同名回归）
        let root = std::env::temp_dir().join(format!("wb-tail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("proj");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sess-tail.jsonl");
        let full = call(1, "t1", "hy3", 10, 10, 0).to_string();
        let partial_second = r#"{"id":"t2","timestamp":1792155734222,"type":"fun"#;
        fs::write(&path, format!("{full}\n{partial_second}")).unwrap();
        let db = Database::memory().unwrap();
        let first = sync_from_roots(&db, std::slice::from_ref(&root)).unwrap();
        assert_eq!(first.imported, 1);
        // 半行随后写完（带换行）+ 追加第三行
        let second = call(2, "t2", "hy3", 20, 20, 0).to_string();
        let third = call(3, "t3", "hy3", 30, 30, 0).to_string();
        fs::write(&path, format!("{full}\n{second}\n{third}\n")).unwrap();
        let resumed = sync_from_roots(&db, std::slice::from_ref(&root)).unwrap();
        assert_eq!(resumed.imported, 2, "补全的半行和新增行都要能导入");
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod native_validation {
    use super::*;

    #[test]
    #[ignore = "requires the real WorkBuddy projects; set WORKBUDDY_HOME to run"]
    fn workbuddy_reads_real_projects() -> Result<(), AppError> {
        assert!(std::env::var("WORKBUDDY_HOME").is_ok());
        let db = Database::memory()?;
        let imported = sync_workbuddy_usage(&db)?;
        assert!(imported.imported > 0);
        assert_eq!(sync_workbuddy_usage(&db)?.imported, 0);
        let conn = crate::database::lock_conn!(db.conn);
        let (rows, models) = conn.query_row(
            "SELECT COUNT(*), COUNT(DISTINCT model) FROM proxy_request_logs WHERE app_type='workbuddy'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?;
        assert!(rows > 0 && models > 0);
        Ok(())
    }
}
