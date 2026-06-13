# Codex Session Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add provider-scoped Codex conversation management, session-level token usage, and provider visibility sharing that also works in the Codex client.

**Architecture:** Keep original Codex JSONL files in place. Store CCS-owned provider/session links in SQLite, expose usage summaries by `session_id`, and materialize links into Codex-visible provider buckets through an explicit backed-up sync operation.

**Tech Stack:** Tauri 2, Rust, rusqlite, React 18, TypeScript, TanStack Query, shadcn/Radix UI, lucide-react.

---

## File Structure

Backend:

- Modify `src-tauri/src/database/mod.rs`: bump schema version.
- Modify `src-tauri/src/database/schema.rs`: create and migrate `codex_session_provider_links`.
- Modify `src-tauri/src/database/dao/mod.rs`: register the new DAO module.
- Create `src-tauri/src/database/dao/codex_sessions.rs`: CCS link-table CRUD.
- Modify `src-tauri/src/session_manager/mod.rs`: add optional Codex `model_provider` metadata to `SessionMeta`.
- Modify `src-tauri/src/session_manager/providers/codex.rs`: parse `session_meta.payload.model_provider`.
- Create `src-tauri/src/services/codex_session_sharing.rs`: provider-scoped session listing, link management, visibility sync, safe JSONL/state DB rewrites.
- Modify `src-tauri/src/services/mod.rs`: export the new service module if this file exists in the local checkout; otherwise add the module in `src-tauri/src/lib.rs`.
- Modify `src-tauri/src/services/usage_stats.rs`: expose `session_id`, add `session_id` filters, add Codex session summary query.
- Modify `src-tauri/src/commands/session_manager.rs`: add sharing commands.
- Modify `src-tauri/src/commands/usage.rs`: add session usage command.
- Modify `src-tauri/src/commands/mod.rs` and `src-tauri/src/lib.rs`: export/register the new commands.

Frontend:

- Modify `src/types.ts`: add `modelProvider` to `SessionMeta`; add Codex session sharing types if centralizing there.
- Modify `src/types/usage.ts`: add `sessionId`, `CodexSessionUsageSummary`, and usage filter support.
- Modify `src/lib/api/sessions.ts`: add Codex sharing API calls.
- Modify `src/lib/api/usage.ts`: add Codex session usage API call.
- Create `src/lib/query/codexSessions.ts`: React Query hooks for provider session window.
- Create `src/components/providers/CodexSessionsDialog.tsx`: provider-scoped Codex conversation manager.
- Modify `src/components/providers/ProviderActions.tsx`: add Codex sessions action.
- Modify `src/components/providers/ProviderCard.tsx`: pass Codex sessions action for Codex providers.
- Modify `src/components/providers/ProviderList.tsx`: hold selected provider and render the dialog.
- Modify locale files under `src/i18n/locales/*.json`: add labels and error strings.

Verification:

- Rust: `cargo test --lib` from `src-tauri`.
- Frontend: `npm run typecheck`; focused Vitest tests when added.

---

### Task 1: Add Codex Session Link Table And DAO

**Files:**
- Modify: `src-tauri/src/database/mod.rs`
- Modify: `src-tauri/src/database/schema.rs`
- Modify: `src-tauri/src/database/dao/mod.rs`
- Create: `src-tauri/src/database/dao/codex_sessions.rs`
- Test: `src-tauri/src/database/tests.rs`

- [ ] **Step 1: Write failing schema and DAO tests**

Add these tests to `src-tauri/src/database/tests.rs`:

```rust
#[test]
fn schema_creates_codex_session_provider_links_table() {
    let conn = Connection::open_in_memory().expect("open memory db");
    Database::create_tables_on_conn(&conn).expect("create tables");
    Database::apply_schema_migrations_on_conn(&conn).expect("apply migrations");

    assert!(Database::table_exists(&conn, "codex_session_provider_links").unwrap());
    for column in [
        "session_id",
        "source_path",
        "provider_id",
        "link_mode",
        "created_at",
        "updated_at",
    ] {
        assert!(
            Database::has_column(&conn, "codex_session_provider_links", column).unwrap(),
            "missing codex_session_provider_links.{column}"
        );
    }
}

#[test]
fn codex_session_provider_links_round_trip() -> Result<(), AppError> {
    let db = Database::memory()?;
    let links = db.replace_codex_session_provider_links(
        "session-1",
        "C:/Users/Test/.codex/sessions/2026/06/13/session-1.jsonl",
        &["provider-a".to_string(), "provider-b".to_string()],
        "manual",
    )?;

    assert_eq!(links.len(), 2);
    assert_eq!(
        db.get_codex_session_provider_links("session-1", "C:/Users/Test/.codex/sessions/2026/06/13/session-1.jsonl")?
            .into_iter()
            .map(|link| link.provider_id)
            .collect::<Vec<_>>(),
        vec!["provider-a".to_string(), "provider-b".to_string()]
    );

    db.delete_codex_session_provider_link(
        "session-1",
        "C:/Users/Test/.codex/sessions/2026/06/13/session-1.jsonl",
        "provider-a",
    )?;

    let remaining = db.get_codex_session_provider_links(
        "session-1",
        "C:/Users/Test/.codex/sessions/2026/06/13/session-1.jsonl",
    )?;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].provider_id, "provider-b");
    Ok(())
}
```

- [ ] **Step 2: Run the failing tests**

Run:

```powershell
cd src-tauri
cargo test codex_session_provider_links --lib
```

Expected: fails because `codex_session_provider_links` and DAO methods do not exist.

- [ ] **Step 3: Add the table to fresh schemas**

In `src-tauri/src/database/mod.rs`, change:

```rust
pub(crate) const SCHEMA_VERSION: i32 = 12;
```

In `Database::create_tables_on_conn` in `src-tauri/src/database/schema.rs`, after `session_log_sync`, add:

```rust
conn.execute(
    "CREATE TABLE IF NOT EXISTS codex_session_provider_links (
        session_id TEXT NOT NULL,
        source_path TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        link_mode TEXT NOT NULL DEFAULT 'manual',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (session_id, source_path, provider_id)
    )",
    [],
)
.map_err(|e| AppError::Database(e.to_string()))?;

conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_codex_session_provider_links_provider
     ON codex_session_provider_links(provider_id, updated_at DESC)",
    [],
)
.map_err(|e| AppError::Database(e.to_string()))?;
```

- [ ] **Step 4: Add v11 to v12 migration**

In `apply_schema_migrations_on_conn`, add:

```rust
11 => {
    log::info!("Migrating database from v11 to v12 (Codex session provider links)");
    Self::migrate_v11_to_v12(conn)?;
    Self::set_user_version(conn, 12)?;
}
```

Add this method in `impl Database` near the other migration methods:

```rust
fn migrate_v11_to_v12(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS codex_session_provider_links (
            session_id TEXT NOT NULL,
            source_path TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            link_mode TEXT NOT NULL DEFAULT 'manual',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (session_id, source_path, provider_id)
        )",
        [],
    )
    .map_err(|e| AppError::Database(format!("create codex_session_provider_links failed: {e}")))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_codex_session_provider_links_provider
         ON codex_session_provider_links(provider_id, updated_at DESC)",
        [],
    )
    .map_err(|e| AppError::Database(format!("create codex session link index failed: {e}")))?;

    Ok(())
}
```

- [ ] **Step 5: Add DAO module**

Add this to `src-tauri/src/database/dao/mod.rs`:

```rust
pub mod codex_sessions;
```

Create `src-tauri/src/database/dao/codex_sessions.rs`:

```rust
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionProviderLink {
    pub session_id: String,
    pub source_path: String,
    pub provider_id: String,
    pub link_mode: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

impl Database {
    pub fn replace_codex_session_provider_links(
        &self,
        session_id: &str,
        source_path: &str,
        provider_ids: &[String],
        link_mode: &str,
    ) -> Result<Vec<CodexSessionProviderLink>, AppError> {
        let conn = lock_conn!(self.conn);
        let now = now_secs();
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::Database(format!("begin codex session link transaction failed: {e}")))?;

        tx.execute(
            "DELETE FROM codex_session_provider_links WHERE session_id = ?1 AND source_path = ?2",
            params![session_id, source_path],
        )
        .map_err(|e| AppError::Database(format!("delete old codex session links failed: {e}")))?;

        for provider_id in provider_ids {
            tx.execute(
                "INSERT INTO codex_session_provider_links
                 (session_id, source_path, provider_id, link_mode, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![session_id, source_path, provider_id, link_mode, now, now],
            )
            .map_err(|e| AppError::Database(format!("insert codex session link failed: {e}")))?;
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("commit codex session links failed: {e}")))?;

        self.get_codex_session_provider_links(session_id, source_path)
    }

    pub fn get_codex_session_provider_links(
        &self,
        session_id: &str,
        source_path: &str,
    ) -> Result<Vec<CodexSessionProviderLink>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT session_id, source_path, provider_id, link_mode, created_at, updated_at
             FROM codex_session_provider_links
             WHERE session_id = ?1 AND source_path = ?2
             ORDER BY provider_id ASC",
        )?;
        let rows = stmt.query_map(params![session_id, source_path], |row| {
            Ok(CodexSessionProviderLink {
                session_id: row.get(0)?,
                source_path: row.get(1)?,
                provider_id: row.get(2)?,
                link_mode: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(format!("read codex session links failed: {e}")))
    }

    pub fn delete_codex_session_provider_link(
        &self,
        session_id: &str,
        source_path: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM codex_session_provider_links
             WHERE session_id = ?1 AND source_path = ?2 AND provider_id = ?3",
            params![session_id, source_path, provider_id],
        )
        .map_err(|e| AppError::Database(format!("delete codex session link failed: {e}")))?;
        Ok(())
    }
}
```

- [ ] **Step 6: Run the tests**

Run:

```powershell
cd src-tauri
cargo test codex_session_provider_links --lib
```

Expected: the new schema and DAO tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/database/mod.rs src-tauri/src/database/schema.rs src-tauri/src/database/dao/mod.rs src-tauri/src/database/dao/codex_sessions.rs src-tauri/src/database/tests.rs
git commit -m "feat: add codex session provider links"
```

---

### Task 2: Parse Codex Provider Bucket Metadata

**Files:**
- Modify: `src-tauri/src/session_manager/mod.rs`
- Modify: `src-tauri/src/session_manager/providers/codex.rs`
- Modify: `src/types.ts`

- [ ] **Step 1: Write failing Rust test**

Add this test to `src-tauri/src/session_manager/providers/codex.rs` tests:

```rust
#[test]
fn parse_session_extracts_model_provider() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("session.jsonl");
    std::fs::write(
        &path,
        concat!(
            "{\"timestamp\":\"2026-06-13T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\",\"cwd\":\"/tmp/project\",\"model_provider\":\"custom\"}}\n",
            "{\"timestamp\":\"2026-06-13T08:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}\n"
        ),
    )
    .expect("write session");

    let meta = parse_session(&path).expect("parse session");
    assert_eq!(meta.model_provider.as_deref(), Some("custom"));
}
```

- [ ] **Step 2: Run failing test**

```powershell
cd src-tauri
cargo test parse_session_extracts_model_provider --lib
```

Expected: fails because `SessionMeta` has no `model_provider`.

- [ ] **Step 3: Extend `SessionMeta`**

In `src-tauri/src/session_manager/mod.rs`, add this field after `resume_command`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub model_provider: Option<String>,
```

Update every `SessionMeta` literal in session providers/tests to set `model_provider: None` unless a provider can supply a value.

- [ ] **Step 4: Parse Codex metadata**

In `parse_session` in `src-tauri/src/session_manager/providers/codex.rs`, add:

```rust
let mut model_provider: Option<String> = None;
```

Inside the `session_meta` payload branch, add:

```rust
if model_provider.is_none() {
    model_provider = payload
        .get("model_provider")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
}
```

When constructing `SessionMeta`, include:

```rust
model_provider,
```

- [ ] **Step 5: Update frontend type**

In `src/types.ts`, add to `SessionMeta`:

```ts
modelProvider?: string;
```

- [ ] **Step 6: Run tests**

```powershell
cd src-tauri
cargo test parse_session_extracts_model_provider --lib
```

Expected: pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/session_manager/mod.rs src-tauri/src/session_manager/providers/codex.rs src/types.ts
git commit -m "feat: expose codex session provider bucket"
```

---

### Task 3: Add Session-Level Usage Backend

**Files:**
- Modify: `src-tauri/src/services/usage_stats.rs`
- Modify: `src-tauri/src/commands/usage.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types/usage.ts`
- Modify: `src/lib/api/usage.ts`

- [ ] **Step 1: Write failing backend tests**

Add tests to `src-tauri/src/services/usage_stats.rs` tests:

```rust
#[test]
fn get_request_logs_filters_by_session_id() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        insert_usage_log_for_test(
            &conn,
            "codex-session-a",
            "_codex_session",
            "codex",
            "gpt-5",
            100,
            20,
            10,
            0,
            200,
            "0.01",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET session_id = 'session-a', data_source = 'codex_session'
             WHERE request_id = 'codex-session-a'",
            [],
        )?;
        insert_usage_log_for_test(
            &conn,
            "codex-session-b",
            "_codex_session",
            "codex",
            "gpt-5",
            200,
            30,
            0,
            0,
            200,
            "0.02",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET session_id = 'session-b', data_source = 'codex_session'
             WHERE request_id = 'codex-session-b'",
            [],
        )?;
    }

    let logs = db.get_request_logs(
        &LogFilters {
            app_type: Some("codex".to_string()),
            session_id: Some("session-a".to_string()),
            ..Default::default()
        },
        0,
        10,
    )?;
    assert_eq!(logs.total, 1);
    assert_eq!(logs.data[0].session_id.as_deref(), Some("session-a"));
    Ok(())
}

#[test]
fn get_codex_session_usage_summaries_groups_by_session() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = lock_conn!(db.conn);
        insert_usage_log_for_test(
            &conn,
            "codex-session-a-1",
            "_codex_session",
            "codex",
            "gpt-5",
            100,
            20,
            10,
            0,
            200,
            "0.01",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET session_id = 'session-a', data_source = 'codex_session'
             WHERE request_id = 'codex-session-a-1'",
            [],
        )?;
        insert_usage_log_for_test(
            &conn,
            "codex-session-a-2",
            "_codex_session",
            "codex",
            "gpt-5",
            50,
            5,
            0,
            0,
            200,
            "0.02",
        )?;
        conn.execute(
            "UPDATE proxy_request_logs SET session_id = 'session-a', data_source = 'codex_session'
             WHERE request_id = 'codex-session-a-2'",
            [],
        )?;
    }

    let summaries = db.get_codex_session_usage_summaries(None, None)?;
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].session_id, "session-a");
    assert_eq!(summaries[0].request_count, 2);
    assert_eq!(summaries[0].total_input_tokens, 150);
    assert_eq!(summaries[0].total_output_tokens, 25);
    assert_eq!(summaries[0].total_cache_read_tokens, 10);
    Ok(())
}
```

- [ ] **Step 2: Run failing tests**

```powershell
cd src-tauri
cargo test session_usage_summaries --lib
```

Expected: fails because `session_id` filter and summaries are missing.

- [ ] **Step 3: Add backend DTOs and filters**

In `LogFilters`, add:

```rust
pub session_id: Option<String>,
```

In `RequestLogDetail`, add:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub session_id: Option<String>,
```

Define:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionUsageSummary {
    pub session_id: String,
    pub request_count: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cost_usd: String,
    pub first_used_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub models: Vec<String>,
}
```

- [ ] **Step 4: Include `session_id` in log queries**

Update the row mapper comment from 25 columns to 26 columns and map:

```rust
session_id: row.get(23)?,
data_source: row.get(24)?,
pricing_model: row.get(25)?,
```

In both `get_request_logs` and `get_request_detail`, add `l.session_id` before `l.data_source` in the SELECT list.

Add the filter in `get_request_logs`:

```rust
if let Some(ref session_id) = filters.session_id {
    conditions.push("l.session_id = ?".to_string());
    params.push(Box::new(session_id.clone()));
}
```

- [ ] **Step 5: Add grouped summary method**

Add to `impl Database` in `usage_stats.rs`:

```rust
pub fn get_codex_session_usage_summaries(
    &self,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<CodexSessionUsageSummary>, AppError> {
    let conn = lock_conn!(self.conn);
    let mut conditions = vec![
        effective_usage_log_filter("l"),
        "l.app_type = 'codex'".to_string(),
        "l.session_id IS NOT NULL".to_string(),
        "l.session_id <> ''".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_date {
        conditions.push("l.created_at >= ?".to_string());
        params.push(Box::new(start));
    }
    if let Some(end) = end_date {
        conditions.push("l.created_at <= ?".to_string());
        params.push(Box::new(end));
    }

    let where_clause = format!("WHERE {}", conditions.join(" AND "));
    let sql = format!(
        "SELECT
            l.session_id,
            COUNT(*) AS request_count,
            COALESCE(SUM(l.input_tokens), 0),
            COALESCE(SUM(l.output_tokens), 0),
            COALESCE(SUM(l.cache_read_tokens), 0),
            COALESCE(SUM(l.cache_creation_tokens), 0),
            COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0),
            MIN(l.created_at),
            MAX(l.created_at),
            GROUP_CONCAT(DISTINCT l.model)
         FROM proxy_request_logs l
         {where_clause}
         GROUP BY l.session_id
         ORDER BY MAX(l.created_at) DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let models_csv: Option<String> = row.get(9)?;
        Ok(CodexSessionUsageSummary {
            session_id: row.get(0)?,
            request_count: row.get::<_, i64>(1)? as u64,
            total_input_tokens: row.get::<_, i64>(2)? as u64,
            total_output_tokens: row.get::<_, i64>(3)? as u64,
            total_cache_read_tokens: row.get::<_, i64>(4)? as u64,
            total_cache_creation_tokens: row.get::<_, i64>(5)? as u64,
            total_cost_usd: format!("{:.6}", row.get::<_, f64>(6)?),
            first_used_at: row.get(7)?,
            last_used_at: row.get(8)?,
            models: models_csv
                .unwrap_or_default()
                .split(',')
                .filter(|model| !model.trim().is_empty())
                .map(|model| model.trim().to_string())
                .collect(),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(format!("read codex session usage summaries failed: {e}")))
}
```

- [ ] **Step 6: Add command and frontend API**

In `src-tauri/src/commands/usage.rs`:

```rust
#[tauri::command]
pub fn get_codex_session_usage_summaries(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<CodexSessionUsageSummary>, AppError> {
    state.db.get_codex_session_usage_summaries(start_date, end_date)
}
```

Register `commands::get_codex_session_usage_summaries` in `src-tauri/src/lib.rs`.

In `src/types/usage.ts`, add:

```ts
export interface CodexSessionUsageSummary {
  sessionId: string;
  requestCount: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheReadTokens: number;
  totalCacheCreationTokens: number;
  totalCostUsd: string;
  firstUsedAt?: number;
  lastUsedAt?: number;
  models: string[];
}
```

Add `sessionId?: string;` to `LogFilters` and `sessionId?: string;` to `RequestLog`.

In `src/lib/api/usage.ts`, import `CodexSessionUsageSummary` and add:

```ts
getCodexSessionUsageSummaries: async (
  startDate?: number,
  endDate?: number,
): Promise<CodexSessionUsageSummary[]> => {
  return invoke("get_codex_session_usage_summaries", { startDate, endDate });
},
```

- [ ] **Step 7: Run tests and typecheck**

```powershell
cd src-tauri
cargo test get_request_logs_filters_by_session_id get_codex_session_usage_summaries --lib
cd ..
npm run typecheck
```

Expected: tests and typecheck pass.

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/services/usage_stats.rs src-tauri/src/commands/usage.rs src-tauri/src/lib.rs src/types/usage.ts src/lib/api/usage.ts
git commit -m "feat: add codex session usage summaries"
```

---

### Task 4: Add Codex Session Sharing Service

**Files:**
- Create: `src-tauri/src/services/codex_session_sharing.rs`
- Modify: `src-tauri/src/services/mod.rs` or `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/session_manager.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/services/codex_session_sharing.rs`

- [ ] **Step 1: Write failing service tests**

Create module tests in `src-tauri/src/services/codex_session_sharing.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_codex_session(path: &std::path::Path, provider: &str) {
        std::fs::write(
            path,
            format!(
                "{{\"timestamp\":\"2026-06-13T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-1\",\"cwd\":\"/tmp/project\",\"model_provider\":\"{provider}\"}}}}\n\
                 {{\"timestamp\":\"2026-06-13T08:00:01Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}}}\n"
            ),
        )
        .expect("write session");
    }

    #[test]
    fn validates_source_path_under_codex_roots() {
        let root = tempdir().expect("root");
        let sessions = root.path().join("sessions");
        std::fs::create_dir_all(&sessions).expect("mkdir");
        let allowed = sessions.join("session.jsonl");
        write_codex_session(&allowed, "old-provider");

        assert!(validate_codex_session_source_path(root.path(), &allowed).is_ok());
        assert!(validate_codex_session_source_path(root.path(), &root.path().join("../outside.jsonl")).is_err());
    }

    #[test]
    fn rewrites_session_meta_provider_bucket_with_backup() {
        let root = tempdir().expect("root");
        let backup = tempdir().expect("backup");
        let sessions = root.path().join("sessions");
        std::fs::create_dir_all(&sessions).expect("mkdir");
        let path = sessions.join("session.jsonl");
        write_codex_session(&path, "old-provider");

        let result = rewrite_jsonl_session_provider_bucket(
            &path,
            root.path(),
            backup.path(),
            "custom",
        )
        .expect("rewrite");

        assert!(result.changed);
        let rewritten = std::fs::read_to_string(&path).expect("read");
        assert!(rewritten.contains("\"model_provider\":\"custom\""));
        assert!(backup.path().read_dir().expect("read backup dir").next().is_some());
    }
}
```

- [ ] **Step 2: Run failing tests**

```powershell
cd src-tauri
cargo test codex_session_sharing --lib
```

Expected: fails because service functions do not exist.

- [ ] **Step 3: Add service DTOs**

Create `src-tauri/src/services/codex_session_sharing.rs` with these public types:

```rust
use crate::codex_config::get_codex_config_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::session_manager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderVisibility {
    pub provider_id: String,
    pub linked: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCodexSession {
    pub session: session_manager::SessionMeta,
    pub linked_provider_ids: Vec<String>,
    pub visible_to_current_provider: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCodexSessionProvidersRequest {
    pub session_id: String,
    pub source_path: String,
    pub provider_ids: Vec<String>,
    pub link_mode: Option<String>,
    pub sync_to_codex: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexVisibilitySyncResult {
    pub changed_jsonl_files: u32,
    pub changed_state_rows: u32,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionProviderUpdateResult {
    pub provider_ids: Vec<String>,
    pub sync: Option<CodexVisibilitySyncResult>,
}
```

- [ ] **Step 4: Add path validation and JSONL rewrite**

Add helper functions:

```rust
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn validate_codex_session_source_path(codex_dir: &Path, source_path: &Path) -> Result<PathBuf, AppError> {
    let canonical_source = source_path
        .canonicalize()
        .map_err(|e| AppError::io(source_path, e))?;
    let roots = [
        codex_dir.join("sessions"),
        codex_dir.join("archived_sessions"),
    ];
    let allowed = roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| canonical_source.starts_with(canonical_root))
            .unwrap_or(false)
    });
    if !allowed {
        return Err(AppError::Message(format!(
            "Codex session path is outside configured session roots: {}",
            source_path.display()
        )));
    }
    Ok(canonical_source)
}

#[derive(Debug, Clone)]
struct JsonlRewriteResult {
    changed: bool,
}

fn rewrite_jsonl_session_provider_bucket(
    source_path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
    target_model_provider: &str,
) -> Result<JsonlRewriteResult, AppError> {
    let source_path = validate_codex_session_source_path(codex_dir, source_path)?;
    let content = fs::read_to_string(&source_path).map_err(|e| AppError::io(&source_path, e))?;
    let mut changed = false;
    let mut rewritten = String::with_capacity(content.len());

    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if line.contains("\"session_meta\"") {
            if let Ok(mut value) = serde_json::from_str::<Value>(line) {
                if value.get("type").and_then(Value::as_str) == Some("session_meta") {
                    if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
                        let old = payload.get("model_provider").and_then(Value::as_str);
                        if old != Some(target_model_provider) {
                            payload.insert(
                                "model_provider".to_string(),
                                Value::String(target_model_provider.to_string()),
                            );
                            rewritten.push_str(&serde_json::to_string(&value).map_err(|e| {
                                AppError::Config(format!("serialize Codex session_meta failed: {e}"))
                            })?);
                            rewritten.push_str(newline);
                            changed = true;
                            continue;
                        }
                    }
                }
            }
        }
        rewritten.push_str(line);
        rewritten.push_str(newline);
    }

    if changed {
        let backup_dir = backup_root.join(now_secs().to_string());
        fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
        fs::copy(&source_path, backup_dir.join("session.jsonl"))
            .map_err(|e| AppError::io(&source_path, e))?;
        fs::write(&source_path, rewritten).map_err(|e| AppError::io(&source_path, e))?;
    }

    Ok(JsonlRewriteResult { changed })
}
```

- [ ] **Step 5: Add public link update service**

Add:

```rust
pub fn set_codex_session_provider_links(
    db: &Database,
    request: SetCodexSessionProvidersRequest,
) -> Result<CodexSessionProviderUpdateResult, AppError> {
    let link_mode = request.link_mode.as_deref().unwrap_or("manual");
    db.replace_codex_session_provider_links(
        &request.session_id,
        &request.source_path,
        &request.provider_ids,
        link_mode,
    )?;

    let sync = if request.sync_to_codex {
        Some(sync_codex_session_visibility(
            &request.source_path,
            &request.provider_ids,
        )?)
    } else {
        None
    };

    Ok(CodexSessionProviderUpdateResult {
        provider_ids: request.provider_ids,
        sync,
    })
}
```

Add first-version `sync_codex_session_visibility` using shared `custom` bucket:

```rust
pub fn sync_codex_session_visibility(
    source_path: &str,
    provider_ids: &[String],
) -> Result<CodexVisibilitySyncResult, AppError> {
    let codex_dir = get_codex_config_dir();
    let backup_root = crate::config::get_app_config_dir()
        .join("backups")
        .join("codex-session-sharing");
    let source_path = PathBuf::from(source_path);
    let rewrite = rewrite_jsonl_session_provider_bucket(
        &source_path,
        &codex_dir,
        &backup_root,
        "custom",
    )?;

    Ok(CodexVisibilitySyncResult {
        changed_jsonl_files: u32::from(rewrite.changed),
        changed_state_rows: 0,
        skipped: Vec::new(),
        warnings: if provider_ids.is_empty() {
            vec!["No target providers were selected".to_string()]
        } else {
            Vec::new()
        },
    })
}
```

- [ ] **Step 6: Add commands**

In `src-tauri/src/commands/session_manager.rs`:

```rust
#[tauri::command]
pub fn set_codex_session_provider_links(
    state: tauri::State<'_, crate::store::AppState>,
    request: crate::services::codex_session_sharing::SetCodexSessionProvidersRequest,
) -> Result<crate::services::codex_session_sharing::CodexSessionProviderUpdateResult, crate::error::AppError> {
    crate::services::codex_session_sharing::set_codex_session_provider_links(&state.db, request)
}
```

Register `commands::set_codex_session_provider_links` in `src-tauri/src/lib.rs`.

- [ ] **Step 7: Run tests**

```powershell
cd src-tauri
cargo test codex_session_sharing --lib
```

Expected: pass.

- [ ] **Step 8: Commit**

```powershell
git add src-tauri/src/services/codex_session_sharing.rs src-tauri/src/services/mod.rs src-tauri/src/commands/session_manager.rs src-tauri/src/lib.rs
git commit -m "feat: add codex session sharing service"
```

---

### Task 5: Add Provider-Scoped Session Listing API

**Files:**
- Modify: `src-tauri/src/services/codex_session_sharing.rs`
- Modify: `src-tauri/src/commands/session_manager.rs`
- Modify: `src/lib/api/sessions.ts`
- Modify: `src/types.ts`

- [ ] **Step 1: Write failing service test**

Add to `codex_session_sharing.rs` tests:

```rust
#[test]
fn provider_session_listing_marks_linked_sessions() -> Result<(), AppError> {
    let db = Database::memory()?;
    db.replace_codex_session_provider_links(
        "session-1",
        "C:/Users/Test/.codex/sessions/session-1.jsonl",
        &["provider-a".to_string()],
        "manual",
    )?;

    let sessions = vec![session_manager::SessionMeta {
        provider_id: "codex".to_string(),
        session_id: "session-1".to_string(),
        title: Some("hello".to_string()),
        summary: None,
        project_dir: None,
        created_at: Some(1),
        last_active_at: Some(2),
        source_path: Some("C:/Users/Test/.codex/sessions/session-1.jsonl".to_string()),
        resume_command: Some("codex resume session-1".to_string()),
        model_provider: Some("custom".to_string()),
    }];

    let visible = merge_codex_sessions_with_links(&db, "provider-a", sessions)?;
    assert_eq!(visible.len(), 1);
    assert!(visible[0].visible_to_current_provider);
    assert_eq!(visible[0].linked_provider_ids, vec!["provider-a".to_string()]);
    Ok(())
}
```

- [ ] **Step 2: Run failing test**

```powershell
cd src-tauri
cargo test provider_session_listing_marks_linked_sessions --lib
```

Expected: fails because merge/list functions do not exist.

- [ ] **Step 3: Implement merge and listing**

Add:

```rust
fn merge_codex_sessions_with_links(
    db: &Database,
    current_provider_id: &str,
    sessions: Vec<session_manager::SessionMeta>,
) -> Result<Vec<ProviderCodexSession>, AppError> {
    let mut out = Vec::new();
    for session in sessions.into_iter().filter(|s| s.provider_id == "codex") {
        let Some(source_path) = session.source_path.clone() else {
            continue;
        };
        let links = db.get_codex_session_provider_links(&session.session_id, &source_path)?;
        let linked_provider_ids = links.into_iter().map(|link| link.provider_id).collect::<Vec<_>>();
        let visible_to_current_provider =
            linked_provider_ids.iter().any(|id| id == current_provider_id)
                || session.model_provider.as_deref() == Some("custom");
        out.push(ProviderCodexSession {
            session,
            linked_provider_ids,
            visible_to_current_provider,
        });
    }
    Ok(out)
}

pub fn list_provider_codex_sessions(
    db: &Database,
    provider_id: &str,
) -> Result<Vec<ProviderCodexSession>, AppError> {
    let sessions = session_manager::scan_sessions();
    merge_codex_sessions_with_links(db, provider_id, sessions)
}
```

- [ ] **Step 4: Add command**

In `commands/session_manager.rs`:

```rust
#[tauri::command]
pub fn list_provider_codex_sessions(
    state: tauri::State<'_, crate::store::AppState>,
    provider_id: String,
) -> Result<Vec<crate::services::codex_session_sharing::ProviderCodexSession>, crate::error::AppError> {
    crate::services::codex_session_sharing::list_provider_codex_sessions(&state.db, &provider_id)
}
```

Register in `src-tauri/src/lib.rs`.

- [ ] **Step 5: Add frontend types/API**

In `src/types.ts`:

```ts
export interface ProviderCodexSession {
  session: SessionMeta;
  linkedProviderIds: string[];
  visibleToCurrentProvider: boolean;
}

export interface SetCodexSessionProvidersRequest {
  sessionId: string;
  sourcePath: string;
  providerIds: string[];
  linkMode?: "manual" | "all" | "native";
  syncToCodex: boolean;
}

export interface CodexVisibilitySyncResult {
  changedJsonlFiles: number;
  changedStateRows: number;
  skipped: string[];
  warnings: string[];
}

export interface CodexSessionProviderUpdateResult {
  providerIds: string[];
  sync?: CodexVisibilitySyncResult;
}
```

In `src/lib/api/sessions.ts`:

```ts
async listProviderCodexSessions(providerId: string): Promise<ProviderCodexSession[]> {
  return await invoke("list_provider_codex_sessions", { providerId });
},

async setCodexSessionProviderLinks(
  request: SetCodexSessionProvidersRequest,
): Promise<CodexSessionProviderUpdateResult> {
  return await invoke("set_codex_session_provider_links", { request });
},
```

Import the new types from `@/types`.

- [ ] **Step 6: Verify**

```powershell
cd src-tauri
cargo test provider_session_listing_marks_linked_sessions --lib
cd ..
npm run typecheck
```

Expected: pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/services/codex_session_sharing.rs src-tauri/src/commands/session_manager.rs src-tauri/src/lib.rs src/types.ts src/lib/api/sessions.ts
git commit -m "feat: list codex sessions by provider"
```

---

### Task 6: Add Frontend Query Hooks

**Files:**
- Create: `src/lib/query/codexSessions.ts`

- [ ] **Step 1: Create hooks**

Create:

```ts
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { sessionsApi } from "@/lib/api/sessions";
import type { SetCodexSessionProvidersRequest } from "@/types";

export const codexSessionKeys = {
  provider: (providerId: string) => ["codex-sessions", providerId] as const,
};

export function useProviderCodexSessions(providerId?: string) {
  return useQuery({
    queryKey: providerId ? codexSessionKeys.provider(providerId) : ["codex-sessions", "none"],
    queryFn: () => sessionsApi.listProviderCodexSessions(providerId!),
    enabled: Boolean(providerId),
  });
}

export function useSetCodexSessionProviderLinks(providerId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: SetCodexSessionProvidersRequest) =>
      sessionsApi.setCodexSessionProviderLinks(request),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: codexSessionKeys.provider(providerId),
      });
    },
  });
}
```

- [ ] **Step 2: Verify typecheck**

```powershell
npm run typecheck
```

Expected: pass.

- [ ] **Step 3: Commit**

```powershell
git add src/lib/query/codexSessions.ts
git commit -m "feat: add codex session query hooks"
```

---

### Task 7: Add Provider Codex Sessions Dialog

**Files:**
- Create: `src/components/providers/CodexSessionsDialog.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`
- Modify: `src/i18n/locales/ja.json`

- [ ] **Step 1: Create the dialog component**

Create `src/components/providers/CodexSessionsDialog.tsx`:

```tsx
import { Copy, Loader2, RefreshCw, Share2 } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { Provider } from "@/types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Checkbox } from "@/components/ui/checkbox";
import { ScrollArea } from "@/components/ui/scroll-area";
import { usageApi } from "@/lib/api/usage";
import {
  useProviderCodexSessions,
  useSetCodexSessionProviderLinks,
} from "@/lib/query/codexSessions";

interface CodexSessionsDialogProps {
  open: boolean;
  provider: Provider | null;
  providers: Provider[];
  onOpenChange: (open: boolean) => void;
}

function formatTokens(value: number) {
  return new Intl.NumberFormat().format(value);
}

export function CodexSessionsDialog({
  open,
  provider,
  providers,
  onOpenChange,
}: CodexSessionsDialogProps) {
  const { t } = useTranslation();
  const providerId = provider?.id;
  const { data: sessions = [], isLoading, refetch } = useProviderCodexSessions(providerId);
  const mutation = useSetCodexSessionProviderLinks(providerId ?? "");

  const codexProviders = useMemo(
    () => providers.filter((candidate) => candidate.id),
    [providers],
  );

  const handleShareAll = async (sessionId: string, sourcePath?: string) => {
    if (!sourcePath) return;
    await mutation.mutateAsync({
      sessionId,
      sourcePath,
      providerIds: codexProviders.map((candidate) => candidate.id),
      linkMode: "all",
      syncToCodex: true,
    });
    toast.success(t("codexSessions.shareAllDone"));
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-5xl h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>
            {t("codexSessions.title", { name: provider?.name ?? "Codex" })}
          </DialogTitle>
        </DialogHeader>

        <div className="flex items-center justify-between gap-3 border-b pb-3">
          <p className="text-sm text-muted-foreground">
            {t("codexSessions.subtitle")}
          </p>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void refetch()}
            disabled={isLoading}
          >
            <RefreshCw className="size-4" />
            {t("common.refresh")}
          </Button>
        </div>

        <ScrollArea className="min-h-0 flex-1 pr-3">
          {isLoading ? (
            <div className="flex h-40 items-center justify-center text-muted-foreground">
              <Loader2 className="mr-2 size-4 animate-spin" />
              {t("common.loading")}
            </div>
          ) : sessions.length === 0 ? (
            <div className="flex h-40 items-center justify-center text-sm text-muted-foreground">
              {t("codexSessions.empty")}
            </div>
          ) : (
            <div className="space-y-2">
              {sessions.map((item) => {
                const session = item.session;
                const sourcePath = session.sourcePath;
                return (
                  <div
                    key={`${session.sessionId}:${sourcePath ?? ""}`}
                    className="rounded-lg border bg-card p-3"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="truncate font-medium">
                          {session.title || session.sessionId}
                        </div>
                        <div className="mt-1 truncate text-xs text-muted-foreground">
                          {session.projectDir || sourcePath || session.sessionId}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => handleShareAll(session.sessionId, sourcePath)}
                          disabled={!sourcePath || mutation.isPending}
                        >
                          <Share2 className="size-4" />
                          {t("codexSessions.shareAll")}
                        </Button>
                        {session.resumeCommand && (
                          <Button
                            size="icon"
                            variant="ghost"
                            onClick={() => {
                              void navigator.clipboard.writeText(session.resumeCommand!);
                              toast.success(t("sessionManager.resumeCommandCopied"));
                            }}
                            title={t("sessionManager.copyCommand")}
                          >
                            <Copy className="size-4" />
                          </Button>
                        )}
                      </div>
                    </div>

                    <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
                      <span>{t("codexSessions.modelProvider")}: {session.modelProvider || "unknown"}</span>
                      <span>{t("codexSessions.visible")}: {item.visibleToCurrentProvider ? t("common.yes") : t("common.no")}</span>
                      <span>{t("codexSessions.linkedProviders")}: {item.linkedProviderIds.length}</span>
                    </div>

                    <div className="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                      {codexProviders.map((candidate) => {
                        const checked = item.linkedProviderIds.includes(candidate.id);
                        return (
                          <label
                            key={candidate.id}
                            className="flex items-center gap-2 rounded-md border px-2 py-1.5 text-sm"
                          >
                            <Checkbox checked={checked} disabled />
                            <span className="truncate">{candidate.name}</span>
                          </label>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </ScrollArea>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 2: Remove unused import**

If `usageApi` is unused after the final component edit, remove this import:

```tsx
import { usageApi } from "@/lib/api/usage";
```

- [ ] **Step 3: Add locale keys**

Add the same key structure to each locale file:

```json
"codexSessions": {
  "title": "{{name}} Codex Sessions",
  "subtitle": "Manage Codex conversations visible to this provider.",
  "empty": "No Codex sessions found.",
  "shareAll": "Share all",
  "shareAllDone": "Session shared to all Codex providers.",
  "modelProvider": "Provider bucket",
  "visible": "Visible here",
  "linkedProviders": "Linked providers"
}
```

Use natural translations in `zh.json`, `zh-TW.json`, and `ja.json`.

- [ ] **Step 4: Typecheck**

```powershell
npm run typecheck
```

Expected: pass.

- [ ] **Step 5: Commit**

```powershell
git add src/components/providers/CodexSessionsDialog.tsx src/i18n/locales/en.json src/i18n/locales/zh.json src/i18n/locales/zh-TW.json src/i18n/locales/ja.json
git commit -m "feat: add codex sessions dialog"
```

---

### Task 8: Add Provider Entry Point

**Files:**
- Modify: `src/components/providers/ProviderActions.tsx`
- Modify: `src/components/providers/ProviderCard.tsx`
- Modify: `src/components/providers/ProviderList.tsx`

- [ ] **Step 1: Add action prop**

In `ProviderActions.tsx`, import `MessagesSquare`:

```tsx
import { MessagesSquare } from "lucide-react";
```

Add prop:

```ts
onOpenCodexSessions?: () => void;
```

Destructure it and add this button before the terminal button:

```tsx
{appId === "codex" && onOpenCodexSessions && (
  <Button
    size="icon"
    variant="ghost"
    onClick={onOpenCodexSessions}
    title={t("codexSessions.action", { defaultValue: "Codex sessions" })}
    className={cn(iconButtonClass, "hover:text-blue-600 dark:hover:text-blue-400")}
  >
    <MessagesSquare className="h-4 w-4" />
  </Button>
)}
```

- [ ] **Step 2: Thread prop through ProviderCard**

In `ProviderCardProps`, add:

```ts
onOpenCodexSessions?: (provider: Provider) => void;
```

Pass it into `ProviderCard` args and into `ProviderActions`:

```tsx
onOpenCodexSessions={
  appId === "codex" && onOpenCodexSessions
    ? () => onOpenCodexSessions(provider)
    : undefined
}
```

- [ ] **Step 3: Render dialog from ProviderList**

In `ProviderList.tsx`, import:

```tsx
import { CodexSessionsDialog } from "@/components/providers/CodexSessionsDialog";
```

Add state:

```tsx
const [codexSessionsProvider, setCodexSessionsProvider] = useState<Provider | null>(null);
```

Pass to each `ProviderCard`:

```tsx
onOpenCodexSessions={setCodexSessionsProvider}
```

Render near the existing dialogs:

```tsx
<CodexSessionsDialog
  open={Boolean(codexSessionsProvider)}
  provider={codexSessionsProvider}
  providers={providers}
  onOpenChange={(open) => {
    if (!open) setCodexSessionsProvider(null);
  }}
/>
```

- [ ] **Step 4: Add locale action key**

Add:

```json
"action": "Codex sessions"
```

under `codexSessions` in all locale files.

- [ ] **Step 5: Typecheck**

```powershell
npm run typecheck
```

Expected: pass.

- [ ] **Step 6: Commit**

```powershell
git add src/components/providers/ProviderActions.tsx src/components/providers/ProviderCard.tsx src/components/providers/ProviderList.tsx src/i18n/locales/en.json src/i18n/locales/zh.json src/i18n/locales/zh-TW.json src/i18n/locales/ja.json
git commit -m "feat: open codex sessions from providers"
```

---

### Task 9: Add Provider Checklist Editing

**Files:**
- Modify: `src/components/providers/CodexSessionsDialog.tsx`

- [ ] **Step 1: Add per-session provider toggles**

Inside each session card in `CodexSessionsDialog.tsx`, replace the disabled checkbox block with:

```tsx
<div className="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
  {codexProviders.map((candidate) => {
    const checked = item.linkedProviderIds.includes(candidate.id);
    return (
      <label
        key={candidate.id}
        className="flex items-center gap-2 rounded-md border px-2 py-1.5 text-sm"
      >
        <Checkbox
          checked={checked}
          onCheckedChange={(next) => {
            if (!sourcePath) return;
            const nextIds = new Set(item.linkedProviderIds);
            if (next === true) {
              nextIds.add(candidate.id);
            } else {
              nextIds.delete(candidate.id);
            }
            void mutation.mutateAsync({
              sessionId: session.sessionId,
              sourcePath,
              providerIds: Array.from(nextIds),
              linkMode: "manual",
              syncToCodex: false,
            });
          }}
        />
        <span className="truncate">{candidate.name}</span>
      </label>
    );
  })}
</div>
```

- [ ] **Step 2: Add explicit sync button**

Add this button next to `Share all`:

```tsx
<Button
  size="sm"
  variant="secondary"
  onClick={() => {
    if (!sourcePath) return;
    void mutation.mutateAsync({
      sessionId: session.sessionId,
      sourcePath,
      providerIds: item.linkedProviderIds,
      linkMode: "manual",
      syncToCodex: true,
    });
  }}
  disabled={!sourcePath || mutation.isPending}
>
  <RefreshCw className="size-4" />
  {t("codexSessions.syncVisibility")}
</Button>
```

- [ ] **Step 3: Add locale key**

```json
"syncVisibility": "Sync visibility"
```

Add natural translations in non-English locale files.

- [ ] **Step 4: Typecheck**

```powershell
npm run typecheck
```

Expected: pass.

- [ ] **Step 5: Commit**

```powershell
git add src/components/providers/CodexSessionsDialog.tsx src/i18n/locales/en.json src/i18n/locales/zh.json src/i18n/locales/zh-TW.json src/i18n/locales/ja.json
git commit -m "feat: manage codex session provider links"
```

---

### Task 10: Add Conversation Usage To Dialog

**Files:**
- Modify: `src/components/providers/CodexSessionsDialog.tsx`
- Modify: `src/lib/query/codexSessions.ts`

- [ ] **Step 1: Add usage query hook**

In `src/lib/query/codexSessions.ts`:

```ts
import { usageApi } from "@/lib/api/usage";

export function useCodexSessionUsageSummaries() {
  return useQuery({
    queryKey: ["codex-session-usage-summaries"],
    queryFn: () => usageApi.getCodexSessionUsageSummaries(),
  });
}
```

- [ ] **Step 2: Show usage totals**

In `CodexSessionsDialog.tsx`, import and call:

```tsx
import { useCodexSessionUsageSummaries } from "@/lib/query/codexSessions";

const { data: usageSummaries = [] } = useCodexSessionUsageSummaries();
const usageBySession = useMemo(
  () => new Map(usageSummaries.map((summary) => [summary.sessionId, summary])),
  [usageSummaries],
);
```

Inside each session card:

```tsx
const usage = usageBySession.get(session.sessionId);
```

Render below metadata:

```tsx
<div className="mt-3 grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
  <div className="rounded-md bg-muted/60 p-2">
    <div className="text-muted-foreground">{t("usage.inputTokens")}</div>
    <div className="font-medium">{formatTokens(usage?.totalInputTokens ?? 0)}</div>
  </div>
  <div className="rounded-md bg-muted/60 p-2">
    <div className="text-muted-foreground">{t("usage.outputTokens")}</div>
    <div className="font-medium">{formatTokens(usage?.totalOutputTokens ?? 0)}</div>
  </div>
  <div className="rounded-md bg-muted/60 p-2">
    <div className="text-muted-foreground">{t("usage.cacheReadTokens")}</div>
    <div className="font-medium">{formatTokens(usage?.totalCacheReadTokens ?? 0)}</div>
  </div>
  <div className="rounded-md bg-muted/60 p-2">
    <div className="text-muted-foreground">{t("usage.cost")}</div>
    <div className="font-medium">${usage?.totalCostUsd ?? "0.000000"}</div>
  </div>
</div>
```

- [ ] **Step 3: Typecheck**

```powershell
npm run typecheck
```

Expected: pass.

- [ ] **Step 4: Commit**

```powershell
git add src/components/providers/CodexSessionsDialog.tsx src/lib/query/codexSessions.ts
git commit -m "feat: show codex session usage in provider dialog"
```

---

### Task 11: Add State DB Visibility Sync

**Files:**
- Modify: `src-tauri/src/services/codex_session_sharing.rs`

- [ ] **Step 1: Write failing state DB sync test**

Add:

```rust
#[test]
fn updates_codex_state_db_thread_provider_bucket() -> Result<(), AppError> {
    let root = tempdir().expect("root");
    let db_path = root.path().join("state_5.sqlite");
    {
        let conn = rusqlite::Connection::open(&db_path).expect("open state");
        conn.execute(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL)",
            [],
        )
        .expect("create threads");
        conn.execute(
            "INSERT INTO threads (id, model_provider) VALUES ('session-1', 'old-provider')",
            [],
        )
        .expect("insert thread");
    }

    let changed = update_state_db_provider_bucket(&db_path, "session-1", "custom")?;
    assert_eq!(changed, 1);

    let conn = rusqlite::Connection::open(&db_path).expect("open state");
    let provider: String = conn.query_row(
        "SELECT model_provider FROM threads WHERE id = 'session-1'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(provider, "custom");
    Ok(())
}
```

- [ ] **Step 2: Run failing test**

```powershell
cd src-tauri
cargo test updates_codex_state_db_thread_provider_bucket --lib
```

Expected: fails because `update_state_db_provider_bucket` does not exist.

- [ ] **Step 3: Implement state DB update**

Add:

```rust
fn update_state_db_provider_bucket(
    db_path: &Path,
    session_id: &str,
    target_model_provider: &str,
) -> Result<u32, AppError> {
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| AppError::Database(format!("open Codex state DB failed: {e}")))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("set Codex state DB timeout failed: {e}")))?;

    if !Database::table_exists(&conn, "threads")?
        || !Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(0);
    }

    let changed = conn.execute(
        "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider <> ?1",
        rusqlite::params![target_model_provider, session_id],
    )
    .map_err(|e| AppError::Database(format!("update Codex state DB provider bucket failed: {e}")))?;

    Ok(changed as u32)
}
```

Call this from `sync_codex_session_visibility` after JSONL rewrite:

```rust
let state_db_path = codex_dir.join("state_5.sqlite");
let changed_state_rows = update_state_db_provider_bucket(&state_db_path, &session_id_from_jsonl(&source_path)?, "custom")?;
```

Add `session_id_from_jsonl`:

```rust
fn session_id_from_jsonl(path: &Path) -> Result<String, AppError> {
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    for line in content.lines() {
        if !line.contains("\"session_meta\"") {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|e| AppError::Config(format!("parse Codex session metadata failed: {e}")))?;
        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(id) = value
                .get("payload")
                .and_then(|payload| payload.get("id").or_else(|| payload.get("session_id")))
                .and_then(Value::as_str)
            {
                return Ok(id.to_string());
            }
        }
    }
    Err(AppError::Message(format!("Codex session id not found in {}", path.display())))
}
```

- [ ] **Step 4: Run test**

```powershell
cd src-tauri
cargo test updates_codex_state_db_thread_provider_bucket --lib
```

Expected: pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/services/codex_session_sharing.rs
git commit -m "feat: sync codex state db session visibility"
```

---

### Task 12: Final Verification

**Files:**
- No planned source edits unless verification reveals issues.

- [ ] **Step 1: Run Rust tests**

```powershell
cd src-tauri
cargo test --lib
```

Expected: all library tests pass.

- [ ] **Step 2: Run frontend typecheck**

```powershell
npm run typecheck
```

Expected: TypeScript exits successfully.

- [ ] **Step 3: Run unit tests**

```powershell
npm run test:unit
```

Expected: Vitest exits successfully.

- [ ] **Step 4: Inspect git diff**

```powershell
git status --short
git log --oneline -n 8
```

Expected: working tree is clean after task commits; recent commits match the tasks above.

---

## Self-Review

- Spec coverage: provider entry, provider-scoped session window, all-provider sharing, checked-provider sharing, CCS logical link table, Codex client visibility sync, backups, session usage summary, and `session_id` log filtering are each assigned to a task.
- Placeholder scan: plan contains concrete file paths, commands, snippets, and expected results.
- Type consistency: backend uses `session_id`/`provider_id`; frontend uses camelCase `sessionId`/`providerId` through serde and Tauri invoke conventions.
- Risk isolation: state DB mutation is delayed until Task 11, after JSONL rewrite and CCS link/index features are already testable.

