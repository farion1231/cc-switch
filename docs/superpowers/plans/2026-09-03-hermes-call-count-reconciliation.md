# Hermes Call-Count Reconciliation Implementation Plan

> **For agentic workers:** Use the runtime's supported delegation or inline execution workflow. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Include main-loop calls present only in `sessions.api_call_count` in CC Switch's Hermes usage import without double-counting auxiliary calls or fabricating route attribution.

**Architecture:** Keep `session_model_usage` as the primary cumulative source. In the same read-only SQLite snapshot, compute each session's positive residual against only its `task = ''` rows and append a synthetic `unknown` / `unattributed_main` cumulative row that flows through the existing snapshot-and-delta pipeline. If the compatible `sessions.api_call_count` contract is absent, retain the existing detail-only behavior.

**Tech Stack:** Rust 2021, rusqlite, rust_decimal, Cargo test/fmt/Clippy, GitHub Actions.

---

### Task 1: Synchronize the PR with current upstream

**Files:**
- Inspect: `src-tauri/src/services/session_usage_hermes.rs`
- Inspect: files reported by Git as conflicted

- [ ] **Step 1: Verify live refs before integration**

Run:

```powershell
gh pr view 6120 --repo farion1231/cc-switch --json headRefOid,baseRefOid,mergeable,reviewDecision,statusCheckRollup
git -c safe.directory="D:/Projects/GitHub/farion1231/cc-switch/HermesUsage/live-pr-6120" status --short --branch
```

Expected: remote head is still the local pre-integration head, and the only local commit not pushed is the approved design documentation.

- [ ] **Step 2: Fetch and merge current upstream main**

Run:

```powershell
git -c safe.directory="D:/Projects/GitHub/farion1231/cc-switch/HermesUsage/live-pr-6120" -c http.sslBackend=openssl fetch origin main
git -c safe.directory="D:/Projects/GitHub/farion1231/cc-switch/HermesUsage/live-pr-6120" merge --no-edit FETCH_HEAD
```

Expected: either a clean merge or a finite conflict set. Resolve conflicts by preserving upstream behavior plus the existing Hermes importer, v19 convergent migration, and Linux `statx(STATX_BTIME)` replacement detection. Do not discard unrelated upstream changes.

- [ ] **Step 3: Verify the merged baseline**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml services::session_usage_hermes::tests --lib
```

Expected: formatting exits 0 and the existing Hermes importer tests pass before the new behavior is added.

### Task 2: Add RED tests for residual semantics

**Files:**
- Modify: `src-tauri/src/services/session_usage_hermes.rs:1025` (test module)

- [ ] **Step 1: Add a compatible sessions fixture**

Extend the test helpers with:

```rust
fn add_session_call_total(conn: &Connection, session_id: &str, calls: i64) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            api_call_count INTEGER DEFAULT 0
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO sessions (id, api_call_count) VALUES (?1, ?2)",
        params![session_id, calls],
    )
    .unwrap();
}
```

Add a helper that can insert a second usage row with explicit session, model, provider, and task dimensions so one test can represent both main and auxiliary calls.

- [ ] **Step 2: Add source-reader behavior tests**

Add tests equivalent to:

```rust
#[test]
fn session_total_adds_only_unattributed_main_residual() {
    // sessions total = 10, attributed main = 6, auxiliary = 3
    // expected synthetic residual = 4, not 1.
    let rows = read_hermes_database(&path).unwrap();
    let residual = rows.iter().find(|row| row.task == "unattributed_main").unwrap();
    assert_eq!(residual.model, "unknown");
    assert_eq!(residual.billing_provider, "");
    assert_eq!(residual.api_call_count, 4);
    assert_eq!(residual.input_tokens, 0);
    assert_eq!(residual.selected_cost_usd, Decimal::ZERO);
}

#[test]
fn covered_or_missing_session_totals_add_no_residual() {
    // One database has sessions total == attributed main.
    // A second fixture has no sessions table, matching the legacy test schema.
    assert!(rows.iter().all(|row| row.task != "unattributed_main"));
}
```

- [ ] **Step 3: Add sync-window/idempotence test**

Add a test that baselines a residual of 4, updates only `sessions.api_call_count` so the residual becomes 6, syncs again, and asserts exactly one delta with `task = 'unattributed_main'` and `api_call_count = 2`. A third unchanged sync must import zero rows.

- [ ] **Step 4: Run the new tests and verify RED**

Run each new test by fully qualified name:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml services::session_usage_hermes::tests::session_total_adds_only_unattributed_main_residual --lib -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml services::session_usage_hermes::tests::session_residual_increase_is_emitted_once --lib -- --exact --nocapture
```

Expected: FAIL because no `unattributed_main` row exists yet. A compile error or fixture error is not an acceptable RED result.

### Task 3: Implement the minimal source reconciliation

**Files:**
- Modify: `src-tauri/src/services/session_usage_hermes.rs:337-418`

- [ ] **Step 1: Add the reserved task constant and compatibility probe**

Add:

```rust
const HERMES_UNATTRIBUTED_MAIN_TASK: &str = "unattributed_main";

fn has_session_call_totals(conn: &Connection) -> Result<bool, AppError> {
    let mut columns = conn.prepare("PRAGMA table_info(sessions)")?;
    let names = columns.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == "api_call_count" {
            return Ok(true);
        }
    }
    Ok(false)
}
```

The empty `PRAGMA table_info` result handles a missing `sessions` table without turning a compatible detail-only database into an error.

- [ ] **Step 2: Read and construct positive residual rows**

Add a helper with this query:

```sql
SELECT s.id,
       s.api_call_count,
       COALESCE(SUM(CASE WHEN u.task = '' THEN u.api_call_count ELSE 0 END), 0)
FROM sessions s
LEFT JOIN session_model_usage u ON u.session_id = s.id
GROUP BY s.id, s.api_call_count
```

Parse both counters with `read_counter`. For each row, compute:

```rust
let residual = session_calls.saturating_sub(attributed_main_calls);
```

Append nothing when residual is zero. Otherwise construct one `HermesSourceRow` with the exact dimensions and zero token/cost fields defined in the approved design. Build its `row_key` from a reserved reconciliation marker plus the session ID so it cannot collide with a real Hermes row.

- [ ] **Step 3: Keep all source reads in one SQLite snapshot**

At the start of `read_hermes_database`, begin a deferred read transaction before the table/column probes and both row queries:

```rust
conn.execute_batch("BEGIN DEFERRED")?;
```

Use the same read-only connection for detail rows and residual rows. The connection is dropped after the function returns, so no write or explicit commit is required.

- [ ] **Step 4: Append residuals after validated detail rows**

After all `session_model_usage` rows have passed the existing parsers, call the residual helper and append its rows to the result. Do not alter tokens, cost, or API-call counters on the original rows.

- [ ] **Step 5: Run the RED tests and importer group**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml services::session_usage_hermes::tests::session_total_adds_only_unattributed_main_residual --lib -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml services::session_usage_hermes::tests::session_residual_increase_is_emitted_once --lib -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml services::session_usage_hermes::tests --lib
```

Expected: both focused tests pass, followed by the complete Hermes importer group with zero failures.

### Task 4: Verify and publish the implementation

**Files:**
- Verify: `src-tauri/src/services/session_usage_hermes.rs`
- Verify: `docs/superpowers/specs/2026-09-03-hermes-call-count-reconciliation-design.md`
- Verify: `docs/superpowers/plans/2026-09-03-hermes-call-count-reconciliation.md`

- [ ] **Step 1: Run repository checks matching backend CI**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
git -c safe.directory="D:/Projects/GitHub/farion1231/cc-switch/HermesUsage/live-pr-6120" diff --check
```

Expected: every command exits 0; the full test output reports zero failures.

- [ ] **Step 2: Inspect and commit only scoped changes**

Run:

```powershell
git -c safe.directory="D:/Projects/GitHub/farion1231/cc-switch/HermesUsage/live-pr-6120" status --short
git -c safe.directory="D:/Projects/GitHub/farion1231/cc-switch/HermesUsage/live-pr-6120" diff --stat HEAD
```

Expected: only the importer, its in-file tests, and the approved plan are new implementation changes. Commit with:

```text
fix(usage): reconcile Hermes session call totals
```

- [ ] **Step 3: Recheck live PR refs and push**

Query the live PR again. If its head changed, stop and reconcile that drift before pushing. Otherwise push `HEAD` to `xrbs00/cc-switch:feat/hermes-session-usage`.

- [ ] **Step 4: Verify GitHub Actions on the pushed SHA**

Wait for the CI workflow tied to the new head SHA. Success requires all backend matrix jobs, frontend checks, and changed-area detection to complete successfully. Report code-owner review or any remaining mergeability gate separately from test status.
