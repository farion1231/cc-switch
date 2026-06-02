use std::path::Path;

use chrono::Utc;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single orchestration execution record stored in SQLite.
///
/// PII constraint: raw prompts are never stored.  Instead we keep a SHA-256
/// hash (for deduplication) and a short redacted summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationRecord {
    pub id: String,
    pub timestamp: String,

    // Request features (PII-safe)
    pub task_type: String,
    pub complexity: f64,
    pub risk: String,
    pub prompt_hash: String,
    pub prompt_summary: String,

    // Execution
    pub strategy_used: String,
    pub models_called: Vec<ModelCall>,
    pub escalation_count: u32,
    pub total_latency_ms: u64,

    // Quality
    pub quality_scores: Vec<QualityScore>,
    pub final_quality: f64,
    pub passed: bool,

    // Cost
    pub total_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,

    // Feedback (optional)
    pub user_rating: Option<String>,
}

/// Details of a single model invocation within an orchestration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCall {
    pub model_key: String,
    pub provider: String,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub quality_score: f64,
    pub was_selected: bool,
}

/// A quality score produced by one verification tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScore {
    pub tool_name: String,
    pub score: f64,
}

// ---------------------------------------------------------------------------
// HistoryStore — SQLite-backed persistence
// ---------------------------------------------------------------------------

/// SQLite-backed store for orchestration records plus supporting tables.
pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    /// Open (or create) the SQLite database at `db_path` and run migrations.
    pub fn new(db_path: &Path) -> Result<Self, String> {
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create db directory: {}", e))?;
            }
        }

        let conn =
            Connection::open(db_path).map_err(|e| format!("failed to open SQLite db: {}", e))?;

        // Enable WAL for concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| format!("failed to set WAL mode: {}", e))?;

        let store = Self { conn };
        store.ensure_tables()?;
        Ok(store)
    }

    /// Create all required tables if they do not already exist.
    pub fn ensure_tables(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS orchestration_records (
                    id TEXT PRIMARY KEY,
                    timestamp TEXT NOT NULL,
                    task_type TEXT NOT NULL,
                    complexity REAL NOT NULL,
                    risk TEXT NOT NULL,
                    strategy TEXT NOT NULL,
                    prompt_hash TEXT NOT NULL,
                    prompt_summary TEXT NOT NULL,
                    models_called TEXT NOT NULL,
                    quality_scores TEXT NOT NULL,
                    final_quality REAL NOT NULL,
                    passed INTEGER NOT NULL,
                    total_cost_usd REAL NOT NULL,
                    total_latency_ms INTEGER NOT NULL,
                    total_input_tokens INTEGER NOT NULL,
                    total_output_tokens INTEGER NOT NULL,
                    escalation_count INTEGER NOT NULL DEFAULT 0,
                    user_rating TEXT
                );

                CREATE TABLE IF NOT EXISTS threshold_adjustments (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    strategy TEXT NOT NULL,
                    old_threshold REAL NOT NULL,
                    new_threshold REAL NOT NULL,
                    reason TEXT NOT NULL,
                    sample_size INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS model_performance_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    period_start TEXT NOT NULL,
                    model_key TEXT NOT NULL,
                    task_type TEXT NOT NULL,
                    avg_quality REAL NOT NULL,
                    avg_cost REAL NOT NULL,
                    avg_latency_ms INTEGER NOT NULL,
                    total_requests INTEGER NOT NULL,
                    pass_rate REAL NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_orch_records_task_type
                    ON orchestration_records(task_type);

                CREATE INDEX IF NOT EXISTS idx_orch_records_timestamp
                    ON orchestration_records(timestamp);

                CREATE INDEX IF NOT EXISTS idx_orch_records_prompt_hash
                    ON orchestration_records(prompt_hash);
                ",
            )
            .map_err(|e| format!("failed to create tables: {}", e))?;

        Ok(())
    }

    /// Insert an orchestration record into the database.
    pub fn record(&self, entry: &OrchestrationRecord) -> Result<(), String> {
        let models_json = serde_json::to_string(&entry.models_called)
            .map_err(|e| format!("failed to serialize models_called: {}", e))?;
        let quality_json = serde_json::to_string(&entry.quality_scores)
            .map_err(|e| format!("failed to serialize quality_scores: {}", e))?;

        self.conn
            .execute(
                "
                INSERT INTO orchestration_records (
                    id, timestamp, task_type, complexity, risk, strategy,
                    prompt_hash, prompt_summary, models_called, quality_scores,
                    final_quality, passed, total_cost_usd, total_latency_ms,
                    total_input_tokens, total_output_tokens, escalation_count,
                    user_rating
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                          ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                ",
                params![
                    entry.id,
                    entry.timestamp,
                    entry.task_type,
                    entry.complexity,
                    entry.risk,
                    entry.strategy_used,
                    entry.prompt_hash,
                    entry.prompt_summary,
                    models_json,
                    quality_json,
                    entry.final_quality,
                    entry.passed as i32,
                    entry.total_cost_usd,
                    entry.total_latency_ms as i64,
                    entry.total_input_tokens as i64,
                    entry.total_output_tokens as i64,
                    entry.escalation_count as i32,
                    entry.user_rating,
                ],
            )
            .map_err(|e| format!("failed to insert record: {}", e))?;

        Ok(())
    }

    /// Retrieve an orchestration record by its ID.
    pub fn get_by_id(&self, id: &str) -> Result<Option<OrchestrationRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "
                SELECT id, timestamp, task_type, complexity, risk, strategy,
                       prompt_hash, prompt_summary, models_called, quality_scores,
                       final_quality, passed, total_cost_usd, total_latency_ms,
                       total_input_tokens, total_output_tokens, escalation_count,
                       user_rating
                FROM orchestration_records
                WHERE id = ?1
                ",
            )
            .map_err(|e| format!("failed to prepare select: {}", e))?;

        let result = stmt
            .query_row(params![id], |row| {
                let passed: i32 = row.get(11)?;
                let escalation_count: i32 = row.get(16)?;
                let total_latency_ms: i64 = row.get(13)?;
                let total_input_tokens: i64 = row.get(14)?;
                let total_output_tokens: i64 = row.get(15)?;
                let user_rating: Option<String> = row.get(17)?;

                Ok(OrchestrationRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    task_type: row.get(2)?,
                    complexity: row.get(3)?,
                    risk: row.get(4)?,
                    strategy_used: row.get(5)?,
                    prompt_hash: row.get(6)?,
                    prompt_summary: row.get(7)?,
                    models_called: serde_json::from_str(&row.get::<_, String>(8)?)
                        .unwrap_or_default(),
                    quality_scores: serde_json::from_str(&row.get::<_, String>(9)?)
                        .unwrap_or_default(),
                    final_quality: row.get(10)?,
                    passed: passed != 0,
                    total_cost_usd: row.get(12)?,
                    total_latency_ms: total_latency_ms as u64,
                    total_input_tokens: total_input_tokens as u64,
                    total_output_tokens: total_output_tokens as u64,
                    escalation_count: escalation_count as u32,
                    user_rating,
                })
            })
            .optional()
            .map_err(|e| format!("failed to query record: {}", e))?;

        Ok(result)
    }

    /// Find records with the same task_type and complexity within the given range.
    pub fn find_similar(
        &self,
        task_type: &str,
        complexity_range: (f64, f64),
    ) -> Result<Vec<OrchestrationRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "
                SELECT id, timestamp, task_type, complexity, risk, strategy,
                       prompt_hash, prompt_summary, models_called, quality_scores,
                       final_quality, passed, total_cost_usd, total_latency_ms,
                       total_input_tokens, total_output_tokens, escalation_count,
                       user_rating
                FROM orchestration_records
                WHERE task_type = ?1
                  AND complexity >= ?2
                  AND complexity <= ?3
                ORDER BY timestamp DESC
                ",
            )
            .map_err(|e| format!("failed to prepare find_similar: {}", e))?;

        let rows = stmt
            .query_map(
                params![task_type, complexity_range.0, complexity_range.1],
                |row| {
                    let passed: i32 = row.get(11)?;
                    let escalation_count: i32 = row.get(16)?;
                    let total_latency_ms: i64 = row.get(13)?;
                    let total_input_tokens: i64 = row.get(14)?;
                    let total_output_tokens: i64 = row.get(15)?;
                    let user_rating: Option<String> = row.get(17)?;

                    Ok(OrchestrationRecord {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        task_type: row.get(2)?,
                        complexity: row.get(3)?,
                        risk: row.get(4)?,
                        strategy_used: row.get(5)?,
                        prompt_hash: row.get(6)?,
                        prompt_summary: row.get(7)?,
                        models_called: serde_json::from_str(&row.get::<_, String>(8)?)
                            .unwrap_or_default(),
                        quality_scores: serde_json::from_str(&row.get::<_, String>(9)?)
                            .unwrap_or_default(),
                        final_quality: row.get(10)?,
                        passed: passed != 0,
                        total_cost_usd: row.get(12)?,
                        total_latency_ms: total_latency_ms as u64,
                        total_input_tokens: total_input_tokens as u64,
                        total_output_tokens: total_output_tokens as u64,
                        escalation_count: escalation_count as u32,
                        user_rating,
                    })
                },
            )
            .map_err(|e| format!("failed to execute find_similar: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("failed to read row: {}", e))?);
        }
        Ok(results)
    }

    /// Return all records in timestamp order (helper for stats_engine).
    pub fn query_records_since(&self, since: &str) -> Result<Vec<OrchestrationRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "
                SELECT id, timestamp, task_type, complexity, risk, strategy,
                       prompt_hash, prompt_summary, models_called, quality_scores,
                       final_quality, passed, total_cost_usd, total_latency_ms,
                       total_input_tokens, total_output_tokens, escalation_count,
                       user_rating
                FROM orchestration_records
                WHERE timestamp >= ?1
                ORDER BY timestamp DESC
                ",
            )
            .map_err(|e| format!("failed to prepare query_records_since: {}", e))?;

        let rows = stmt
            .query_map(params![since], |row| {
                let passed: i32 = row.get(11)?;
                let escalation_count: i32 = row.get(16)?;
                let total_latency_ms: i64 = row.get(13)?;
                let total_input_tokens: i64 = row.get(14)?;
                let total_output_tokens: i64 = row.get(15)?;
                let user_rating: Option<String> = row.get(17)?;

                Ok(OrchestrationRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    task_type: row.get(2)?,
                    complexity: row.get(3)?,
                    risk: row.get(4)?,
                    strategy_used: row.get(5)?,
                    prompt_hash: row.get(6)?,
                    prompt_summary: row.get(7)?,
                    models_called: serde_json::from_str(&row.get::<_, String>(8)?)
                        .unwrap_or_default(),
                    quality_scores: serde_json::from_str(&row.get::<_, String>(9)?)
                        .unwrap_or_default(),
                    final_quality: row.get(10)?,
                    passed: passed != 0,
                    total_cost_usd: row.get(12)?,
                    total_latency_ms: total_latency_ms as u64,
                    total_input_tokens: total_input_tokens as u64,
                    total_output_tokens: total_output_tokens as u64,
                    escalation_count: escalation_count as u32,
                    user_rating,
                })
            })
            .map_err(|e| format!("failed to execute query_records_since: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("failed to read row: {}", e))?);
        }
        Ok(results)
    }

    // -----------------------------------------------------------------------
    // PII-safe prompt helpers
    // -----------------------------------------------------------------------

    /// Return first 100 characters of `raw` with API key / token patterns
    /// replaced by `[REDACTED]`.
    pub fn redact_prompt(raw: &str) -> String {
        let truncated: String = raw.chars().take(100).collect();

        // Pattern list covers common secret / token formats.
        let patterns = [
            // sk-... OpenAI-style keys
            r"(?i)sk-[a-zA-Z0-9]{20,}",
            // Generic API key assignments (match quoted values)
            r#"(?i)(?:api_?key|apikey|api_?secret)\s*[=:]\s*["'][^"']{8,}["']"#,
            // Bearer tokens
            r"(?i)bearer\s+[a-zA-Z0-9\-._~+/]+=*",
            // Generic hex/token blobs of 16+ chars after known labels (match quoted values)
            r#"(?i)(?:token|secret|key|password|credential)\s*[=:]\s*["'][a-zA-Z0-9\-._]{16,}["']"#,
            // AWS-style keys
            r"(?i)AKIA[A-Z0-9]{16}",
        ];

        let mut result = truncated;
        for pat in &patterns {
            if let Ok(re) = Regex::new(pat) {
                result = re.replace_all(&result, "[REDACTED]").to_string();
            }
        }

        result
    }

    /// SHA-256 hex digest of the raw prompt.
    pub fn hash_prompt(raw: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        let hash = hasher.finalize();
        format!("{:x}", hash)
    }
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl OrchestrationRecord {
    /// Create a new record with a generated UUID and current timestamp.
    pub fn new(
        task_type: &str,
        complexity: f64,
        risk: &str,
        raw_prompt: &str,
        strategy_used: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().to_rfc3339(),
            task_type: task_type.to_string(),
            complexity,
            risk: risk.to_string(),
            prompt_hash: HistoryStore::hash_prompt(raw_prompt),
            prompt_summary: HistoryStore::redact_prompt(raw_prompt),
            strategy_used: strategy_used.to_string(),
            models_called: Vec::new(),
            escalation_count: 0,
            total_latency_ms: 0,
            quality_scores: Vec::new(),
            final_quality: 0.0,
            passed: false,
            total_cost_usd: 0.0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            user_rating: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store(dir: &TempDir) -> HistoryStore {
        let path = dir.path().join("test_history.db");
        HistoryStore::new(&path).expect("HistoryStore::new should succeed")
    }

    fn sample_record(
        task_type: &str,
        complexity: f64,
        risk: &str,
        strategy: &str,
    ) -> OrchestrationRecord {
        let mut rec = OrchestrationRecord::new(
            task_type,
            complexity,
            risk,
            "Write a function to sort an array using merge sort in Rust",
            strategy,
        );
        rec.models_called = vec![ModelCall {
            model_key: "claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            latency_ms: 1200,
            cost_usd: 0.005,
            quality_score: 0.88,
            was_selected: true,
        }];
        rec.quality_scores = vec![QualityScore {
            tool_name: "structural_check".to_string(),
            score: 0.95,
        }];
        rec.final_quality = 0.88;
        rec.passed = true;
        rec.total_cost_usd = 0.005;
        rec.total_latency_ms = 1200;
        rec.total_input_tokens = 350;
        rec.total_output_tokens = 800;
        rec
    }

    // ---- Test: Create and insert a record, retrieve by ID ----

    #[test]
    fn insert_and_retrieve_by_id() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        let rec = sample_record("coding", 0.6, "medium", "cascade");
        let id = rec.id.clone();

        store.record(&rec).expect("record should succeed");
        let retrieved = store.get_by_id(&id).expect("get_by_id should succeed");

        assert!(retrieved.is_some(), "Record should be found");
        let got = retrieved.unwrap();
        assert_eq!(got.id, rec.id);
        assert_eq!(got.task_type, "coding");
        assert_eq!(got.complexity, 0.6);
        assert_eq!(got.risk, "medium");
        assert_eq!(got.strategy_used, "cascade");
        assert!(got.passed);
        assert_eq!(got.models_called.len(), 1);
        assert_eq!(got.models_called[0].model_key, "claude-sonnet");
        assert_eq!(got.quality_scores.len(), 1);
        assert_eq!(got.quality_scores[0].tool_name, "structural_check");
        assert!((got.final_quality - 0.88).abs() < f64::EPSILON);
    }

    // ---- Test: Find similar records by task_type and complexity range ----

    #[test]
    fn find_similar_returns_matching_records() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        let rec1 = sample_record("coding", 0.5, "low", "route");
        let rec2 = sample_record("coding", 0.7, "medium", "cascade");
        let rec3 = sample_record("chat", 0.3, "low", "route");

        store.record(&rec1).unwrap();
        store.record(&rec2).unwrap();
        store.record(&rec3).unwrap();

        // Search for coding tasks with complexity 0.4 to 0.6
        let results = store.find_similar("coding", (0.4, 0.6)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, rec1.id);

        // Search broader range
        let results = store.find_similar("coding", (0.4, 0.8)).unwrap();
        assert_eq!(results.len(), 2);
    }

    // ---- Test: PII redaction removes API key patterns ----

    #[test]
    fn redact_prompt_removes_api_keys() {
        let input = "Please use sk-abc123def456ghi789jkl012mno345 to access the API";
        let redacted = HistoryStore::redact_prompt(input);
        assert!(!redacted.contains("sk-abc123def456ghi789jkl012mno345"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn redact_prompt_removes_bearer_tokens() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.def";
        let redacted = HistoryStore::redact_prompt(input);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn redact_prompt_removes_key_assignments() {
        let input = r#"api_key = "super_secret_value_12345678""#;
        let redacted = HistoryStore::redact_prompt(input);
        assert!(!redacted.contains("super_secret_value_12345678"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn redact_prompt_truncates_to_100_chars() {
        let input = "A".repeat(200);
        let redacted = HistoryStore::redact_prompt(&input);
        assert_eq!(redacted.len(), 100);
    }

    #[test]
    fn redact_prompt_preserves_safe_content() {
        let input = "Write a simple function to add two numbers";
        let redacted = HistoryStore::redact_prompt(input);
        assert_eq!(redacted, input);
    }

    // ---- Test: SHA-256 hash is consistent for same input ----

    #[test]
    fn hash_prompt_is_consistent() {
        let input = "Write a merge sort in Rust";
        let hash1 = HistoryStore::hash_prompt(input);
        let hash2 = HistoryStore::hash_prompt(input);
        assert_eq!(hash1, hash2, "Same input should produce same hash");
    }

    #[test]
    fn hash_prompt_differs_for_different_input() {
        let hash1 = HistoryStore::hash_prompt("Write a sort function");
        let hash2 = HistoryStore::hash_prompt("Write a search function");
        assert_ne!(
            hash1, hash2,
            "Different inputs should produce different hashes"
        );
    }

    #[test]
    fn hash_prompt_is_64_char_hex() {
        let hash = HistoryStore::hash_prompt("test input");
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ---- Test: SQLite tables created correctly ----

    #[test]
    fn tables_created_successfully() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        // Verify tables exist by querying SQLite master
        let table_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                    'orchestration_records', 'threshold_adjustments', 'model_performance_snapshots'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 3, "All three tables should be created");
    }

    #[test]
    fn indexes_created_successfully() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        let index_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN (
                    'idx_orch_records_task_type',
                    'idx_orch_records_timestamp',
                    'idx_orch_records_prompt_hash'
                )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 3, "All three indexes should be created");
    }

    // ---- Test: Empty find_similar returns empty vec ----

    #[test]
    fn find_similar_empty_returns_empty_vec() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        let results = store.find_similar("nonexistent", (0.0, 1.0)).unwrap();
        assert!(results.is_empty(), "Empty DB should return empty vec");
    }

    // ---- Test: get_by_id returns None for unknown ID ----

    #[test]
    fn get_by_id_returns_none_for_unknown() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        let result = store.get_by_id("nonexistent-id").unwrap();
        assert!(result.is_none());
    }

    // ---- Test: OrchestrationRecord::new generates proper fields ----

    #[test]
    fn new_record_has_uuid_and_timestamp() {
        let rec = OrchestrationRecord::new("coding", 0.5, "low", "prompt text", "cascade");
        assert!(!rec.id.is_empty(), "ID should be generated");
        // UUID v4 format: 8-4-4-4-12
        assert_eq!(rec.id.len(), 36);
        assert!(!rec.timestamp.is_empty(), "Timestamp should be set");
        assert!(rec.timestamp.contains('T'), "Timestamp should be ISO 8601");
    }

    // ---- Test: Multiple records stored and queried ----

    #[test]
    fn store_multiple_records() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        for i in 0..5 {
            let rec = sample_record("coding", 0.1 * (i as f64 + 1.0), "low", "route");
            store.record(&rec).unwrap();
        }

        // All 5 should be in the coding range
        let results = store.find_similar("coding", (0.0, 1.0)).unwrap();
        assert_eq!(results.len(), 5);
    }
}
