//! DSH 会话文件自带的精确计数（`assistant/message` 的最终 usage）。
//! 实测语义：`inputTokens` 不含 `cacheReadTokens`（fresh input），`outputTokens`
//! 已包含 `reasoningTokens`（DeepSeek completion 口径），两者都不做二次加减。
//! 找不到价格的模型记 0，绝不回退到当前供应商的价格。
use super::session_usage::SessionSyncResult;
use super::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::{
    database::{lock_conn, Database},
    error::AppError,
    session_manager::providers::deepseek_harness,
};
use rusqlite::params;
use serde_json::Value;

const APP_TYPE: &str = "deepseek-harness";
const DATA_SOURCE: &str = "dsh_session";
/// header 缺失（极旧会话）时的占位供应商，避免统计页出现空串。
const FALLBACK_PROVIDER: &str = "_dsh_session";

pub fn sync_deepseek_harness_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    for session in deepseek_harness::scan_sessions() {
        let Some(path) = session.source_path else {
            continue;
        };
        result.files_scanned += 1;
        match deepseek_harness::usage_records(std::path::Path::new(&path)) {
            Ok(rows) => match import_records(db, &session.session_id, &rows) {
                Ok((imported, skipped)) => {
                    result.imported += imported;
                    result.skipped += skipped;
                }
                Err(e) => result
                    .errors
                    .push(format!("DeepSeek Harness {}: {e}", session.session_id)),
            },
            Err(e) => result.errors.push(e),
        }
    }
    Ok(result)
}

/// 五个金额（输入、输出、缓存读、缓存写、合计）；查不到价格的记 0。
fn costs(conn: &rusqlite::Connection, model: &str, tokens: [i64; 4]) -> [String; 5] {
    let [input, output, cache_read, cache_write] =
        tokens.map(|v| u32::try_from(v).unwrap_or(u32::MAX));
    let usage = TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_write,
        cache_creation_1h_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };
    match crate::services::usage_stats::find_model_pricing(conn, model) {
        Some(pricing) => {
            let cost = CostCalculator::calculate_for_app(
                APP_TYPE,
                &usage,
                &pricing,
                rust_decimal::Decimal::from(1),
            );
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

fn import_records(db: &Database, session: &str, rows: &[Value]) -> Result<(u32, u32), AppError> {
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction()?;
    let (mut imported, mut skipped) = (0, 0);
    for row in rows {
        let usage = &row["usage"];
        let token = |name: &str| {
            usage[name]
                .as_u64()
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or(0)
        };
        let input = token("inputTokens");
        let output = token("outputTokens");
        let cache = token("cacheReadTokens");
        if input == 0 && output == 0 && cache == 0 {
            continue;
        }
        let Some(key) = row["id"].as_str() else {
            continue;
        };
        let model = row["model"].as_str().unwrap_or("unknown").to_string();
        let provider = row["provider"]
            .as_str()
            .unwrap_or(FALLBACK_PROVIDER)
            .to_string();
        let id = format!("dsh:{session}:{key}");
        // 与 agy 相同的墓碑去重：retention 清掉明细后也不允许重导被裁剪的历史。
        let fresh = tx.execute("INSERT OR IGNORE INTO session_usage_dedup(data_source,request_id,semantic_id,has_entry_id) VALUES (?1,?2,?2,1)", params![DATA_SOURCE, id])?;
        // DSH 会话不区分缓存写，write 金额恒为 0，直接以常量写入。
        let [input_cost, output_cost, cache_cost, _, total_cost] =
            costs(&tx, &model, [input, output, cache, 0]);
        if fresh == 0 {
            // 同一 message 被重写（重生成）时只修正保留中的明细；
            // 已被 retention 裁掉的行靠墓碑禁止复活。
            let changed = tx.execute("UPDATE proxy_request_logs SET model=?2,input_tokens=?3,output_tokens=?4,cache_read_tokens=?5,cache_creation_tokens=0,input_cost_usd=?6,output_cost_usd=?7,cache_read_cost_usd=?8,cache_creation_cost_usd=0,total_cost_usd=?9 WHERE request_id=?1 AND (model!=?2 OR input_tokens!=?3 OR output_tokens!=?4 OR cache_read_tokens!=?5 OR total_cost_usd!=?9)", params![id, model, input, output, cache, input_cost, output_cost, cache_cost, total_cost])?;
            imported += changed as u32;
            skipped += u32::from(changed == 0);
            continue;
        }
        let created_at = row["time"].as_i64().unwrap_or(0) / 1000;
        tx.execute(
            "INSERT INTO proxy_request_logs (
                request_id,provider_id,app_type,model,request_model,input_tokens,output_tokens,
                cache_read_tokens,cache_creation_tokens,input_cost_usd,output_cost_usd,
                cache_read_cost_usd,cache_creation_cost_usd,total_cost_usd,latency_ms,status_code,
                session_id,provider_type,is_streaming,cost_multiplier,created_at,data_source,input_token_semantics
            ) VALUES (?1,?10,?11,?2,?2,?3,?4,?5,0,?6,?7,?8,0,?9,0,200,?12,?13,1,'1',?14,?13,?15)",
            params![id, model, input, output, cache, input_cost, output_cost, cache_cost, total_cost, provider, APP_TYPE, session, DATA_SOURCE, created_at, INPUT_TOKEN_SEMANTICS_FRESH],
        )?;
        imported += 1;
    }
    tx.commit()?;
    Ok((imported, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usage_row(id: &str, model: &str, input: i64, output: i64, cache: i64) -> Value {
        json!({
            "id": id,
            "time": 1786945364644_i64,
            "provider": "deepseek-official",
            "model": model,
            "usage": {"inputTokens": input, "outputTokens": output, "cacheReadTokens": cache, "reasoningTokens": 59}
        })
    }

    fn query_row(db: &Database, sql: &str) -> (i64, i64, i64, String, String, i64) {
        db.conn
            .lock()
            .unwrap()
            .query_row(sql, [], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .unwrap()
    }

    #[test]
    fn dsh_native_fixture_imports_fresh_input_semantics() {
        let db = Database::memory().unwrap();
        let rows = vec![usage_row("m1", "k3", 3792, 107, 6912)];
        assert_eq!(import_records(&db, "fixture", &rows).unwrap(), (1, 0));
        let got = query_row(
            &db,
            "SELECT input_tokens,output_tokens,cache_read_tokens,model,provider_id,input_token_semantics FROM proxy_request_logs",
        );
        // input(3792) < cacheRead(6912)：原生口径即 fresh input，不做加减。
        assert_eq!(
            got,
            (
                3792,
                107,
                6912,
                "k3".into(),
                "deepseek-official".into(),
                INPUT_TOKEN_SEMANTICS_FRESH
            )
        );
    }

    #[test]
    fn dsh_usage_is_exact_deduplicated_and_survives_retention() {
        let db = Database::memory().unwrap();
        let rows = vec![usage_row("m1", "k3", 100, 10, 0)];
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (0, 1));
        db.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM proxy_request_logs", [])
            .unwrap();
        // retention 裁掉明细后墓碑仍禁止复活；另一会话的同名 message 不受影响。
        assert_eq!(import_records(&db, "session", &rows).unwrap(), (0, 1));
        assert_eq!(import_records(&db, "other", &rows).unwrap(), (1, 0));
    }

    #[test]
    fn dsh_rewritten_counters_update_retained_rows() {
        let db = Database::memory().unwrap();
        let mut row = usage_row("m1", "k3", 100, 10, 0);
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (1, 0));
        row["usage"]["outputTokens"] = json!(42);
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (1, 0));
        assert_eq!(import_records(&db, "s", &[row]).unwrap(), (0, 1));
        let output: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT output_tokens FROM proxy_request_logs", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(output, 42);
    }

    #[test]
    fn dsh_model_switch_attributes_rows_to_each_model() {
        let db = Database::memory().unwrap();
        let rows = vec![
            usage_row("m1", "k3", 10, 1, 0),
            json!({
                "id": "m2", "time": 1786945364644_i64,
                "provider": "yuzuvalley", "model": "deepseek-v4.1-flash",
                "usage": {"inputTokens": 20, "outputTokens": 2, "cacheReadTokens": 0}
            }),
        ];
        assert_eq!(import_records(&db, "switch", &rows).unwrap(), (2, 0));
        let models: Vec<String> = {
            let conn = db.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT model FROM proxy_request_logs ORDER BY created_at, model")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            models,
            vec!["deepseek-v4.1-flash".to_string(), "k3".to_string()]
        );
    }

    #[test]
    fn dsh_zero_token_rows_do_not_consume_dedup_ids() {
        let db = Database::memory().unwrap();
        let mut row = json!({
            "id": "m1", "time": 1_i64, "provider": "deepseek-official", "model": "k3",
            "usage": {"inputTokens": 0, "outputTokens": 0, "cacheReadTokens": 0}
        });
        assert_eq!(import_records(&db, "s", &[row.clone()]).unwrap(), (0, 0));
        row["usage"]["inputTokens"] = json!(7);
        assert_eq!(import_records(&db, "s", &[row]).unwrap(), (1, 0));
    }

    #[test]
    fn dsh_missing_header_falls_back_to_placeholder_provider() {
        let db = Database::memory().unwrap();
        let rows = vec![json!({
            "id": "m1", "time": 1_i64,
            "provider": null, "model": null,
            "usage": {"inputTokens": 5, "outputTokens": 1, "cacheReadTokens": 0}
        })];
        assert_eq!(import_records(&db, "old", &rows).unwrap(), (1, 0));
        let (model, provider): (String, String) = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT model,provider_id FROM proxy_request_logs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(model, "unknown");
        assert_eq!(provider, FALLBACK_PROVIDER);
    }
}
