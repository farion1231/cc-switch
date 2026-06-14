//! Antigravity session DB usage tracking.
//!
//! Reads the newer SQLite trajectory DBs under ~/.gemini/antigravity*/conversations
//! and extracts token usage from gen_metadata using the same field mapping as
//! agentsview:
//! - f2 = uncached input
//! - f3 = total output, including thinking
//! - f5 = cache read input

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::usage_stats::{find_model_pricing, should_skip_session_insert, DedupKey};
use rusqlite::{params, Connection, OpenFlags};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const APP_TYPE: &str = "antigravity";
const DATA_SOURCE: &str = "antigravity_session";
const PROVIDER_ID: &str = "_antigravity_session";
const MAX_PLAUSIBLE_TOKENS: u64 = 2_000_000;
const PROTO_MAX_DEPTH: usize = 32;
const PROTO_MAX_FIELDS: usize = 1 << 20;
const GENERATION_STEP_TYPE: i64 = 15;

#[derive(Debug, Clone)]
struct AntigravityUsage {
    input: u32,
    output: u32,
    cache_read: u32,
}

impl AntigravityUsage {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0
    }
}

#[derive(Debug, Clone)]
struct UsageEvent {
    idx: i64,
    usage: AntigravityUsage,
    model: String,
    timestamp: Option<i64>,
}

#[derive(Debug, Clone)]
struct ProtoField {
    number: u32,
    wire: u8,
    varint: u64,
    bytes: Vec<u8>,
    nested: Vec<ProtoField>,
}

pub fn sync_antigravity_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = collect_antigravity_db_files();
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        errors: vec![],
    };

    for file_path in &files {
        match sync_single_antigravity_db(db, file_path) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Antigravity DB parse failed {}: {e}", file_path.display());
                log::warn!("[ANTIGRAVITY-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[ANTIGRAVITY-SYNC] Imported {} records, skipped {}, scanned {} DBs",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

fn collect_antigravity_db_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for rel in [
        ".gemini/antigravity/conversations",
        ".gemini/antigravity-ide/conversations",
        ".gemini/antigravity-cli/conversations",
    ] {
        let dir = home.join(rel);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("db") {
                files.push(path);
            }
        }
    }
    files
}

fn sync_single_antigravity_db(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let file_modified = composite_modified_nanos(file_path);
    let (last_modified, _last_offset) = get_sync_state(db, &file_path_str)?;
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let events = parse_antigravity_db_events(file_path)?;
    let fallback_created_at = file_modified_seconds(file_path);
    let session_id = session_id_for_path(file_path);

    let mut imported = 0;
    let mut skipped = 0;
    for event in &events {
        if event.usage.is_zero() {
            continue;
        }
        let request_id = format!("{DATA_SOURCE}:{session_id}:{}", event.idx);
        let created_at = event.timestamp.unwrap_or(fallback_created_at);
        match insert_antigravity_session_entry(
            db,
            &request_id,
            &event.usage,
            &event.model,
            &session_id,
            created_at,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                log::warn!("[ANTIGRAVITY-SYNC] insert failed ({request_id}): {e}");
                skipped += 1;
            }
        }
    }

    update_sync_state(db, &file_path_str, file_modified, events.len() as i64)?;
    Ok((imported, skipped))
}

fn parse_antigravity_db_events(file_path: &Path) -> Result<Vec<UsageEvent>, AppError> {
    let conn = Connection::open_with_flags(
        file_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::Database(format!("open Antigravity DB failed: {e}")))?;

    let timestamps = load_generation_timestamps(&conn)?;
    let mut stmt = match conn.prepare("SELECT idx, data FROM gen_metadata ORDER BY idx") {
        Ok(stmt) => stmt,
        Err(e) if is_missing_table_error(&e) => return Ok(Vec::new()),
        Err(e) => {
            return Err(AppError::Database(format!(
                "query gen_metadata failed: {e}"
            )))
        }
    };
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })
        .map_err(|e| AppError::Database(format!("read gen_metadata failed: {e}")))?;

    let mut events = Vec::new();
    for row in rows {
        let (idx, data) = row.map_err(|e| AppError::Database(format!("scan gen_metadata: {e}")))?;
        let Some(data) = data else {
            continue;
        };
        if let Some(usage) = extract_token_usage(&data) {
            events.push(UsageEvent {
                idx,
                usage,
                model: extract_model_name(&data).unwrap_or_else(|| "unknown".to_string()),
                timestamp: timestamps.get(&idx).copied(),
            });
        }
    }
    Ok(events)
}

fn load_generation_timestamps(conn: &Connection) -> Result<HashMap<i64, i64>, AppError> {
    let mut stmt = match conn.prepare("SELECT step_type, step_payload FROM steps ORDER BY idx") {
        Ok(stmt) => stmt,
        Err(e) if is_missing_table_error(&e) => return Ok(HashMap::new()),
        Err(e) => return Err(AppError::Database(format!("query steps failed: {e}"))),
    };
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })
        .map_err(|e| AppError::Database(format!("read steps failed: {e}")))?;

    let mut timestamps = HashMap::new();
    let mut generation_idx = 0i64;
    for row in rows {
        let (step_type, payload) =
            row.map_err(|e| AppError::Database(format!("scan step: {e}")))?;
        if step_type != GENERATION_STEP_TYPE {
            continue;
        }
        if let Some(payload) = payload {
            if let Some(ts) = earliest_timestamp_seconds(&payload) {
                timestamps.insert(generation_idx, ts);
            }
        }
        generation_idx += 1;
    }
    Ok(timestamps)
}

fn is_missing_table_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(_, Some(message))
            if message.contains("no such table")
    )
}

fn insert_antigravity_session_entry(
    db: &Database,
    request_id: &str,
    usage_tokens: &AntigravityUsage,
    model: &str,
    session_id: &str,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let dedup_key = DedupKey {
        app_type: APP_TYPE,
        model,
        input_tokens: usage_tokens.input,
        output_tokens: usage_tokens.output,
        cache_read_tokens: usage_tokens.cache_read,
        cache_creation_tokens: 0,
        created_at,
    };
    if should_skip_session_insert(&conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: usage_tokens.input,
        output_tokens: usage_tokens.output,
        cache_read_tokens: usage_tokens.cache_read,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };
    let pricing_model = normalize_antigravity_model(model);
    let pricing = find_antigravity_pricing(&conn, &pricing_model);
    let multiplier = Decimal::ONE;
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app(APP_TYPE, &usage, &p, multiplier);
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

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            request_model = excluded.request_model,
            pricing_model = excluded.pricing_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            created_at = excluded.created_at
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model
           OR COALESCE(pricing_model, '') != COALESCE(excluded.pricing_model, '')
           OR created_at != excluded.created_at",
        params![
            request_id,
            PROVIDER_ID,
            APP_TYPE,
            model,
            model,
            pricing_model,
            usage_tokens.input,
            usage_tokens.output,
            usage_tokens.cache_read,
            0i64,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            Some(session_id.to_string()),
            Some(DATA_SOURCE),
            1i64,
            "1.0",
            created_at,
            DATA_SOURCE,
        ],
    )
    .map_err(|e| AppError::Database(format!("insert Antigravity usage failed: {e}")))?;

    let changed = conn.changes() > 0;
    if changed {
        crate::usage_events::notify_log_recorded();
    }
    Ok(changed)
}

fn find_antigravity_pricing(conn: &Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

fn normalize_antigravity_model(raw: &str) -> String {
    let mut name = raw.trim().to_lowercase();
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }
    if let Some(stripped) = name.strip_suffix("-thinking") {
        name = stripped.to_string();
    }

    let loose = name
        .chars()
        .map(|ch| match ch {
            '-' | '_' | '(' | ')' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if loose == "gemini 3 flash a" || loose == "gemini 3 flash preview" {
        return "gemini-3-flash-preview".to_string();
    }
    if loose.starts_with("gemini 3.5 flash") {
        return "gemini-3.5-flash".to_string();
    }
    if loose == "gemini pro default" {
        return "gemini-3.1-pro-preview".to_string();
    }
    if loose == "gemini 3 pro preview" {
        return "gemini-3-pro-preview".to_string();
    }

    name
}

fn extract_token_usage(data: &[u8]) -> Option<AntigravityUsage> {
    let fields = parse_proto(data).ok()?;
    let mut found = None;
    walk_fields(&fields, &mut |fs| {
        if found.is_none() {
            found = token_block_from(fs);
        }
    });
    found
}

fn token_block_from(fields: &[ProtoField]) -> Option<AntigravityUsage> {
    let f1 = find_field(fields, 1)?;
    let f2 = find_field(fields, 2)?;
    let f3 = find_field(fields, 3)?;
    if f1.wire != 0 || f2.wire != 0 || f3.wire != 0 {
        return None;
    }
    if !(1000..5000).contains(&f1.varint) {
        return None;
    }
    if f2.varint > MAX_PLAUSIBLE_TOKENS || f3.varint > MAX_PLAUSIBLE_TOKENS {
        return None;
    }
    if f2.varint + f3.varint > MAX_PLAUSIBLE_TOKENS {
        return None;
    }
    if let Some(f4) = find_field(fields, 4) {
        if f4.wire != 0 || f4.varint > MAX_PLAUSIBLE_TOKENS {
            return None;
        }
    }

    let cache_read = match find_field(fields, 5) {
        Some(f5) if f5.wire == 0 && f5.varint <= MAX_PLAUSIBLE_TOKENS => f5.varint,
        Some(_) => return None,
        None => 0,
    };

    Some(AntigravityUsage {
        input: f2.varint as u32,
        output: f3.varint as u32,
        cache_read: cache_read as u32,
    })
}

fn extract_model_name(data: &[u8]) -> Option<String> {
    let fields = parse_proto(data).ok()?;
    let mut model = None;
    walk_fields(&fields, &mut |fs| {
        if model.is_some() {
            return;
        }
        for field_no in [21, 19] {
            if let Some(f) = find_field(fs, field_no) {
                if let Some(s) = proto_string(f) {
                    if is_plausible_model_name(&s) {
                        model = Some(s);
                        return;
                    }
                }
            }
        }
    });
    model
}

fn earliest_timestamp_seconds(data: &[u8]) -> Option<i64> {
    let fields = parse_proto(data).ok()?;
    let mut best: Option<i64> = None;
    walk_fields(&fields, &mut |fs| {
        for f in fs {
            if !f.nested.is_empty() {
                if let Some((seconds, _nanos)) = proto_timestamp(&f.nested) {
                    if seconds > 946_684_800 && seconds < 4_102_444_800 {
                        best = Some(best.map_or(seconds, |current| current.min(seconds)));
                    }
                }
            }
        }
    });
    best
}

fn parse_proto(data: &[u8]) -> Result<Vec<ProtoField>, ()> {
    let mut budget = PROTO_MAX_FIELDS;
    parse_proto_depth(data, 0, &mut budget)
}

fn parse_proto_depth(data: &[u8], depth: usize, budget: &mut usize) -> Result<Vec<ProtoField>, ()> {
    if depth > PROTO_MAX_DEPTH {
        return Err(());
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        if *budget == 0 {
            return Ok(out);
        }
        *budget -= 1;

        let (tag, n) = read_varint(&data[pos..]).ok_or(())?;
        pos += n;
        let number = (tag >> 3) as u32;
        let wire = (tag & 0x7) as u8;
        if number == 0 {
            return Err(());
        }

        let mut field = ProtoField {
            number,
            wire,
            varint: 0,
            bytes: Vec::new(),
            nested: Vec::new(),
        };

        match wire {
            0 => {
                let (value, n) = read_varint(&data[pos..]).ok_or(())?;
                field.varint = value;
                pos += n;
            }
            1 => {
                if pos + 8 > data.len() {
                    return Err(());
                }
                field.bytes = data[pos..pos + 8].to_vec();
                pos += 8;
            }
            2 => {
                let (len, n) = read_varint(&data[pos..]).ok_or(())?;
                pos += n;
                let len = len as usize;
                if len > data.len().saturating_sub(pos) {
                    return Err(());
                }
                field.bytes = data[pos..pos + len].to_vec();
                pos += len;
                if let Ok(nested) = parse_proto_depth(&field.bytes, depth + 1, budget) {
                    if looks_like_message(&nested) {
                        field.nested = nested;
                    }
                }
            }
            5 => {
                if pos + 4 > data.len() {
                    return Err(());
                }
                field.bytes = data[pos..pos + 4].to_vec();
                pos += 4;
            }
            3 | 4 => {}
            _ => return Err(()),
        }

        out.push(field);
    }
    Ok(out)
}

fn read_varint(data: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, byte) in data.iter().copied().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

fn looks_like_message(fields: &[ProtoField]) -> bool {
    !fields.is_empty() && fields.iter().all(|f| (1..=100_000).contains(&f.number))
}

fn walk_fields<F>(fields: &[ProtoField], visit: &mut F)
where
    F: FnMut(&[ProtoField]),
{
    visit(fields);
    for f in fields {
        if !f.nested.is_empty() {
            walk_fields(&f.nested, visit);
        }
    }
}

fn find_field(fields: &[ProtoField], number: u32) -> Option<&ProtoField> {
    fields.iter().find(|f| f.number == number)
}

fn proto_string(field: &ProtoField) -> Option<String> {
    if field.wire != 2 || !field.nested.is_empty() {
        return None;
    }
    std::str::from_utf8(&field.bytes).ok().map(str::to_string)
}

fn is_plausible_model_name(s: &str) -> bool {
    !s.is_empty() && s.chars().any(char::is_alphabetic) && s.chars().all(|c| !c.is_control())
}

fn proto_timestamp(fields: &[ProtoField]) -> Option<(i64, i32)> {
    let mut seconds = None;
    let mut nanos = 0i32;
    for f in fields {
        if f.wire != 0 {
            return None;
        }
        match f.number {
            1 => seconds = Some(f.varint as i64),
            2 => {
                if f.varint >= 1_000_000_000 {
                    return None;
                }
                nanos = f.varint as i32;
            }
            _ => return None,
        }
    }
    seconds.map(|s| (s, nanos))
}

fn session_id_for_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let source = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .find(|c| matches!(*c, "antigravity" | "antigravity-ide" | "antigravity-cli"))
        .unwrap_or("antigravity");
    format!("{source}:{stem}")
}

fn composite_modified_nanos(path: &Path) -> i64 {
    let mut best = fs::metadata(path)
        .ok()
        .map(|m| metadata_modified_nanos(&m))
        .unwrap_or(0);
    // WAL may contain uncheckpointed writes. SHM is SQLite lock/index state and
    // can be touched without content changes, so ignore it for incremental sync.
    let wal_path = PathBuf::from(format!("{}-wal", path.display()));
    if let Ok(metadata) = fs::metadata(wal_path) {
        best = best.max(metadata_modified_nanos(&metadata));
    }
    best
}

fn file_modified_seconds(path: &Path) -> i64 {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(now_seconds)
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                return out;
            }
        }
    }

    fn field_varint(number: u32, value: u64) -> Vec<u8> {
        let mut out = enc_varint((u64::from(number) << 3) | 0);
        out.extend(enc_varint(value));
        out
    }

    fn field_bytes(number: u32, bytes: &[u8]) -> Vec<u8> {
        let mut out = enc_varint((u64::from(number) << 3) | 2);
        out.extend(enc_varint(bytes.len() as u64));
        out.extend(bytes);
        out
    }

    fn test_gen_metadata(input: u64, output: u64, cache: u64, model: &str) -> Vec<u8> {
        let mut usage = Vec::new();
        usage.extend(field_varint(1, 1001));
        usage.extend(field_varint(2, input));
        usage.extend(field_varint(3, output));
        usage.extend(field_varint(5, cache));

        let mut data = Vec::new();
        data.extend(field_bytes(7, &usage));
        data.extend(field_bytes(21, model.as_bytes()));
        data
    }

    fn test_step_payload(seconds: u64) -> Vec<u8> {
        let ts = field_varint(1, seconds);
        field_bytes(3, &ts)
    }

    #[test]
    fn extracts_usage_model_and_timestamp() {
        let data = test_gen_metadata(1234, 567, 890, "gemini-3-pro-preview");
        let usage = extract_token_usage(&data).expect("usage");
        assert_eq!(usage.input, 1234);
        assert_eq!(usage.output, 567);
        assert_eq!(usage.cache_read, 890);
        assert_eq!(
            extract_model_name(&data).as_deref(),
            Some("gemini-3-pro-preview")
        );

        let ts = earliest_timestamp_seconds(&test_step_payload(1_779_237_991));
        assert_eq!(ts, Some(1_779_237_991));
    }

    #[test]
    fn maps_gen_metadata_idx_to_generation_step_order() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE steps (
                idx INTEGER PRIMARY KEY,
                step_type INTEGER NOT NULL,
                step_payload BLOB
            )",
            [],
        )?;
        conn.execute(
            "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, ?2, ?3)",
            params![10i64, 7i64, test_step_payload(1_779_200_000)],
        )?;
        conn.execute(
            "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, ?2, ?3)",
            params![
                20i64,
                GENERATION_STEP_TYPE,
                test_step_payload(1_779_237_991)
            ],
        )?;
        conn.execute(
            "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, ?2, ?3)",
            params![21i64, 8i64, test_step_payload(1_779_238_111)],
        )?;
        conn.execute(
            "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, ?2, ?3)",
            params![
                30i64,
                GENERATION_STEP_TYPE,
                test_step_payload(1_779_238_222)
            ],
        )?;

        let timestamps = load_generation_timestamps(&conn)?;
        assert_eq!(timestamps.get(&0).copied(), Some(1_779_237_991));
        assert_eq!(timestamps.get(&1).copied(), Some(1_779_238_222));
        assert_eq!(timestamps.get(&20), None);
        assert_eq!(timestamps.get(&30), None);
        Ok(())
    }

    #[test]
    fn normalizes_antigravity_model_aliases_for_pricing() {
        assert_eq!(
            normalize_antigravity_model("gemini-3-flash-a"),
            "gemini-3-flash-preview"
        );
        assert_eq!(
            normalize_antigravity_model("Gemini 3.5 Flash (High)"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_model("gemini-pro-default"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_model("gemini-3-flash-a-thinking"),
            "gemini-3-flash-preview"
        );
        assert_eq!(
            normalize_antigravity_model("gemini-pro-default-thinking"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_model("gemini-3-pro-preview"),
            "gemini-3-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_model("claude-opus-4-6-thinking"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn inserts_antigravity_usage_with_fresh_input_cost_semantics() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES ('antigravity-test-model', 'Antigravity Test', '1', '10', '0.1', '0')",
                [],
            )?;
        }

        let usage = AntigravityUsage {
            input: 1000,
            output: 100,
            cache_read: 500,
        };
        let inserted = insert_antigravity_session_entry(
            &db,
            "antigravity-test",
            &usage,
            "antigravity-test-model",
            "antigravity:test",
            1_779_237_991,
        )?;
        assert!(inserted);

        let conn = lock_conn!(db.conn);
        let (app_type, pricing_model, input_cost, cache_read_cost, total_cost): (
            String,
            Option<String>,
            String,
            String,
            String,
        ) = conn.query_row(
            "SELECT app_type, pricing_model, input_cost_usd, cache_read_cost_usd, total_cost_usd
                 FROM proxy_request_logs WHERE request_id = 'antigravity-test'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(app_type, APP_TYPE);
        assert_eq!(pricing_model.as_deref(), Some("antigravity-test-model"));
        assert_eq!(input_cost, "0.001");
        assert_eq!(cache_read_cost, "0.00005");
        assert_eq!(total_cost, "0.00205");

        Ok(())
    }
}
