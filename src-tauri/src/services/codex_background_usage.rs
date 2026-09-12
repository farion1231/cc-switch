//! Local Codex Desktop generation accounting, deliberately separate from request totals.
//! Desktop events have no request ID and may aggregate several model calls. Never
//! infer cross-source identity from timestamps, model names, or matching token counts.
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::{calculator::CostCalculator, parser::TokenUsage};
use crate::services::session_usage::SessionSyncResult;
use crate::services::usage_stats::find_model_pricing;
use chrono::DateTime;
use rusqlite::{params, Connection};
use rust_decimal::Decimal;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

pub(crate) fn create_tables(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS codex_background_usage (
            event_key TEXT PRIMARY KEY, created_at INTEGER NOT NULL,
            model TEXT NOT NULL, feature TEXT NOT NULL, status TEXT NOT NULL,
            input_tokens INTEGER NOT NULL, cached_input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_codex_background_time ON codex_background_usage(created_at);
        CREATE TABLE IF NOT EXISTS codex_background_files (
            path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified TEXT NOT NULL
        );",
    )
    .map_err(db_error)
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::Database(format!("Codex background usage: {error}"))
}

#[derive(Debug, PartialEq)]
struct Event {
    key: String,
    timestamp: i64,
    model: String,
    feature: String,
    status: String,
    input: u32,
    cached: u32,
    output: u32,
}

fn parse(line: &str) -> Option<Event> {
    let (timestamp, rest) = line.split_once(' ')?;
    let rest =
        rest.strip_prefix("info [ephemeral-generation] ephemeral_generation_token_usage ")?;
    let mut fields = HashMap::new();
    for field in rest.split_whitespace() {
        let (key, value) = field.split_once('=')?;
        if fields.insert(key, value).is_some() {
            return None;
        }
    }
    if fields.get("event")? != &"ephemeral_generation_token_usage" {
        return None;
    }
    let input: u32 = fields.get("inputTokens")?.parse().ok()?;
    let cached: u32 = fields.get("cachedInputTokens")?.parse().ok()?;
    let output: u32 = fields.get("outputTokens")?.parse().ok()?;
    let total: u64 = fields.get("totalTokens")?.parse().ok()?;
    if cached > input || total != u64::from(input) + u64::from(output) {
        return None;
    }
    if let Some(reasoning) = fields.get("reasoningOutputTokens") {
        if reasoning.parse::<u32>().ok()? > output {
            return None;
        }
    }
    let date = DateTime::parse_from_rfc3339(timestamp).ok()?;
    // Keep all reported statuses: a failed generation can still consume tokens.
    let model = fields.get("model")?.to_string();
    let feature = fields.get("feature")?.to_string();
    let status = fields.get("status")?.to_string();
    if [&model, &feature, &status]
        .iter()
        .any(|v| v.is_empty() || v.len() > 128)
    {
        return None;
    }
    // A source-record fingerprint, NOT an upstream request ID. Canonical ordering
    // also tolerates reordered fields. Exact copies of records collapse.
    let mut sorted_fields: Vec<_> = fields.into_iter().collect();
    sorted_fields.sort_unstable();
    let canonical = format!("{}|{sorted_fields:?}", date.to_rfc3339());
    Some(Event {
        key: format!("{:x}", Sha256::digest(canonical.as_bytes())),
        timestamp: date.timestamp(),
        model,
        feature,
        status,
        input,
        cached,
        output,
    })
}

pub fn sync(db: &Database) -> Result<SessionSyncResult, AppError> {
    #[cfg(target_os = "macos")]
    {
        sync_root(
            db,
            &crate::config::get_home_dir().join("Library/Logs/com.openai.codex"),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = db;
        Ok(SessionSyncResult::default())
    }
}

// Changed-file incremental scan. No raw log text, prompts, or conversation IDs
// are persisted. Re-scan changed files so truncation/rotation need no byte repair.
fn sync_root(db: &Database, root: &Path) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    if !root.exists() {
        return Ok(result);
    }
    visit(db, root, 0, &mut result)?;
    Ok(result)
}

fn visit(
    db: &Database,
    dir: &Path,
    depth: usize,
    result: &mut SessionSyncResult,
) -> Result<(), AppError> {
    for entry in fs::read_dir(dir).map_err(|e| AppError::io(dir, e))? {
        let entry = entry.map_err(|e| AppError::io(dir, e))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| AppError::io(&path, e))?;
        if kind.is_dir() && depth < 3 {
            visit(db, &path, depth + 1, result)?;
        } else if kind.is_file() && path.extension().is_some_and(|v| v == "log") {
            match sync_file(db, &path) {
                Ok(Some(imported)) => {
                    result.files_scanned += 1;
                    result.imported += imported;
                }
                Ok(None) => {}
                Err(error) => result.errors.push(error.to_string()),
            }
        }
    }
    Ok(())
}

fn sync_file(db: &Database, path: &Path) -> Result<Option<u32>, AppError> {
    let metadata = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    let modified = format!(
        "{:?}",
        metadata.modified().map_err(|e| AppError::io(path, e))?
    );
    let size = i64::try_from(metadata.len())
        .map_err(|_| AppError::Message("Codex desktop log too large".into()))?;
    let path_key = path.to_string_lossy();
    {
        let conn = lock_conn!(db.conn);
        let unchanged: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM codex_background_files WHERE path=?1 AND size=?2 AND modified=?3)", params![path_key, size, modified], |row| row.get(0)).map_err(db_error)?;
        if unchanged {
            return Ok(None);
        }
    }
    let file = fs::File::open(path).map_err(|e| AppError::io(path, e))?;
    let mut reader = BufReader::new(file.take(metadata.len()));
    let mut events = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        // Bound memory even when unrelated application log lines are huge.
        let n = reader
            .by_ref()
            .take(65537)
            .read_until(b'\n', &mut line)
            .map_err(|e| AppError::io(path, e))?;
        if n == 0 {
            break;
        }
        if line.last() != Some(&b'\n') {
            if n <= 65536 {
                break;
            } // Incomplete last line: reconsider on the next append.
            loop {
                line.clear();
                let n = reader
                    .by_ref()
                    .take(65537)
                    .read_until(b'\n', &mut line)
                    .map_err(|e| AppError::io(path, e))?;
                if n == 0 || line.last() == Some(&b'\n') {
                    break;
                }
            }
            continue;
        }
        if let Ok(line) = std::str::from_utf8(&line) {
            if let Some(event) = parse(line.trim_end()) {
                events.push(event);
            }
        }
    }
    let mut conn = lock_conn!(db.conn);
    let tx = conn.transaction().map_err(db_error)?;
    let mut imported = 0;
    for event in events {
        imported += tx
            .execute(
                "INSERT OR IGNORE INTO codex_background_usage VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    event.key,
                    event.timestamp,
                    event.model,
                    event.feature,
                    event.status,
                    event.input,
                    event.cached,
                    event.output
                ],
            )
            .map_err(db_error)? as u32;
    }
    tx.execute("INSERT INTO codex_background_files VALUES (?1,?2,?3) ON CONFLICT(path) DO UPDATE SET size=excluded.size,modified=excluded.modified", params![path_key,size,modified]).map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok(Some(imported))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundUsage {
    pub model: String,
    pub feature: String,
    pub status: String,
    pub generations: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// Current configured standard-rate estimate, never subscription billing.
    pub estimated_cost_usd: Option<String>,
}

pub fn summary(
    db: &Database,
    start: i64,
    end: i64,
    model: Option<&str>,
) -> Result<Vec<BackgroundUsage>, AppError> {
    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare("SELECT model,feature,status,input_tokens,cached_input_tokens,output_tokens FROM codex_background_usage WHERE created_at >= ?1 AND created_at <= ?2 AND (?3 IS NULL OR model=?3)").map_err(db_error)?;
    let rows = stmt
        .query_map(params![start, end, model], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, u32>(5)?,
            ))
        })
        .map_err(db_error)?;
    let mut groups: std::collections::BTreeMap<
        (String, String, String),
        (BackgroundUsage, Option<Decimal>),
    > = std::collections::BTreeMap::new();
    for row in rows {
        let (model, feature, status, input, cached, output) = row.map_err(db_error)?;
        let pricing = find_model_pricing(&conn, &model);
        let cost = pricing.map(|pricing| {
            CostCalculator::calculate_for_app(
                "codex",
                &TokenUsage {
                    input_tokens: input,
                    cache_read_tokens: cached,
                    output_tokens: output,
                    ..Default::default()
                },
                &pricing,
                Decimal::ONE,
            )
            .total_cost
        });
        let (group, sum) = groups
            .entry((model.clone(), feature.clone(), status.clone()))
            .or_insert_with(|| {
                (
                    BackgroundUsage {
                        model,
                        feature,
                        status,
                        generations: 0,
                        input_tokens: 0,
                        cached_input_tokens: 0,
                        output_tokens: 0,
                        estimated_cost_usd: None,
                    },
                    Some(Decimal::ZERO),
                )
            });
        group.generations += 1;
        group.input_tokens += u64::from(input);
        group.cached_input_tokens += u64::from(cached);
        group.output_tokens += u64::from(output);
        *sum = sum.and_then(|sum| cost.map(|cost| sum + cost));
    }
    Ok(groups
        .into_values()
        .map(|(mut group, cost)| {
            group.estimated_cost_usd = cost.map(|c| c.to_string());
            group
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    const LINE: &str = "2026-09-12T05:40:08.305Z info [ephemeral-generation] ephemeral_generation_token_usage cachedInputTokens=80 event=ephemeral_generation_token_usage feature=ambient_suggestions inputTokens=100 model=gpt-5.6-terra outputTokens=10 reasoningEffort=medium reasoningOutputTokens=5 serviceTier=null status=success totalTokens=110";

    #[test]
    fn validates_inclusive_counts_without_adding_reasoning_twice() {
        let event = parse(LINE).unwrap();
        assert_eq!((event.input, event.cached, event.output), (100, 80, 10));
        for (from, to) in [
            ("cachedInputTokens=80", "cachedInputTokens=101"),
            ("totalTokens=110", "totalTokens=111"),
            ("inputTokens=100", "inputTokens=4294967296"),
            ("reasoningOutputTokens=5", "reasoningOutputTokens=11"),
            ("outputTokens=10", "outputTokens=-1"),
            ("2026-09-12", "invalid"),
        ] {
            assert!(parse(&LINE.replace(from, to)).is_none());
        }
        assert!(parse(&format!("{LINE} inputTokens=100")).is_none());
        assert!(parse("unrelated log message").is_none());
        assert!(parse(&LINE.replace("status=success", "status=failed")).is_some());
    }

    #[test]
    fn repeated_scans_copies_append_and_truncation_do_not_inflate_usage() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let path = dir.path().join("desktop.log");
        fs::write(&path, format!("{LINE}\n")).unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 1);
        assert_eq!(sync_root(&db, dir.path()).unwrap().files_scanned, 0);
        fs::copy(&path, dir.path().join("rotated.log")).unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 0);
        let second = LINE.replace("08.305Z", "09.305Z");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{second}").unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 0);
        writeln!(file).unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 1);
        fs::write(&path, format!("{}\n", LINE.replace("08.305Z", "10.305Z"))).unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 1);
        let rows = summary(&db, 0, i64::MAX, None).unwrap();
        assert_eq!(rows[0].generations, 3);
        assert_eq!(rows[0].input_tokens, 300);
        assert_eq!(rows[0].cached_input_tokens, 240);
        assert_eq!(rows[0].output_tokens, 30);
        let cost: Decimal = rows[0]
            .estimated_cost_usd
            .as_ref()
            .unwrap()
            .parse()
            .unwrap();
        // Terra: 20 fresh * $2/M + 80 cached * $0.2/M + 10 output * $12/M.
        assert_eq!(cost, Decimal::new(528, 6));
        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn filters_unknown_prices_and_failed_generations_are_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let unknown = LINE
            .replace("gpt-5.6-terra", "unpriced-future-model")
            .replace("status=success", "status=failed");
        fs::write(dir.path().join("app.log"), format!("{LINE}\n{unknown}\n")).unwrap();
        sync_root(&db, dir.path()).unwrap();
        let rows = summary(&db, 0, i64::MAX, Some("unpriced-future-model")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "failed");
        assert!(rows[0].estimated_cost_usd.is_none());
        let timestamp = parse(LINE).unwrap().timestamp;
        assert!(summary(&db, 0, timestamp - 1, None).unwrap().is_empty());
        assert_eq!(summary(&db, timestamp, timestamp, None).unwrap().len(), 2);
    }

    #[test]
    fn oversized_and_invalid_utf8_lines_do_not_hide_following_usage() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::memory().unwrap();
        let mut bytes = vec![b'x'; 150_000];
        bytes.extend_from_slice(b"\n\xff\n");
        bytes.extend_from_slice(format!("{LINE}\n").as_bytes());
        fs::write(dir.path().join("app.log"), bytes).unwrap();
        assert_eq!(sync_root(&db, dir.path()).unwrap().imported, 1);
    }

    #[test]
    fn schema_creation_is_idempotent_and_preserves_existing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        conn.execute(
            "INSERT INTO codex_background_files VALUES ('fixture',1,'1')",
            [],
        )
        .unwrap();
        create_tables(&conn).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM codex_background_files", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    /// Opt-in local verification. Reads only the supplied directory; all writes
    /// go to a new in-memory database. Never accesses the installed CC Switch DB.
    #[test]
    #[ignore = "requires CC_SWITCH_BACKGROUND_LOG_FIXTURE directory"]
    fn import_local_log_fixture() {
        let root = std::env::var_os("CC_SWITCH_BACKGROUND_LOG_FIXTURE")
            .expect("explicit fixture directory required");
        let db = Database::memory().unwrap();
        let first = sync_root(&db, Path::new(&root)).unwrap();
        assert!(first.errors.is_empty(), "{:?}", first.errors);
        assert!(first.imported > 0);
        assert_eq!(sync_root(&db, Path::new(&root)).unwrap().imported, 0);
        println!(
            "{}",
            serde_json::to_string(&summary(&db, 0, i64::MAX, None).unwrap()).unwrap()
        );
    }
}
