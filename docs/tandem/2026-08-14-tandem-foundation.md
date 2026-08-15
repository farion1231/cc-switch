# Tandem Foundation and Task Ledger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or `executing-plans` to implement this plan task by task. Before implementation, use `using-git-worktrees`; for every production change, use `test-driven-development`; before claiming completion, use `verification-before-completion` and `requesting-code-review`.

**Goal:** Establish Tandem on the existing CC-Switch fork and deliver one persistent, task-led vertical slice: create a Task, show it in an action ledger, explicitly confirm it complete, and reflect reliable Task counts in the macOS menu bar while retained CC-Switch configuration remains accessible.

**Architecture:** Keep CC-Switch ancestry and current runtime identity intact. Add a `tandem` Rust bounded context containing domain types, SQLite repository methods, and thin Tauri commands. Add a separate React shell whose primary surface is the Task ledger and whose secondary “Agent Configuration” route renders the existing CC-Switch app. React accesses Tasks through a typed gateway backed by Tauri in production and deterministic in-memory state only in demo/e2e mode.

**Tech Stack:** Tauri 2, Rust 1.85+, SQLite/rusqlite, React 18, TypeScript 5, TanStack Query, Tailwind, lucide-react, Vitest, Testing Library, MSW, Playwright.

## Global Constraints

- Execute in a worktree created from `/Users/feng/Codes/dev/cc-switch`; do not implement in `/Users/feng/Codes/dev/Tandem`.
- Preserve the untracked `prototype/` directory and all unrelated local changes in the original CC-Switch checkout.
- Keep `origin` as `git@github.com:youxuanxue/cc-switch.git`; add `upstream` as `https://github.com/farion1231/cc-switch.git` if absent.
- Use branch `feat/tandem-foundation` unless it already exists; if it exists, inspect it before choosing a collision-free suffix.
- Keep `~/.cc-switch`, `cc-switch.db`, `com.ccswitch.desktop`, `ccswitch://`, updater endpoints, and existing provider/configuration behavior unchanged in this plan; persist Tandem state in a separate `~/.cc-switch/tandem.db` that CC-Switch export/WebDAV/S3 code never receives.
- Do not globally replace “CC Switch” strings. Product identity and data migration belong to Roadmap Plan 2.
- New Rust production code lives under `src-tauri/src/tandem/`; new React production code lives under `src/tandem/`.
- `src/App.tsx` remains the retained CC-Switch application. Rename its default import at the new shell boundary to `LegacyConfigApp`; do not refactor its 1,665 lines in this plan.
- Generate UUID v4 identifiers in Rust with the repository's existing `uuid` dependency; timestamps are Unix milliseconds in signed `i64`.
- Persist enum values as stable snake_case strings. Unknown persisted values return a typed database error; they never silently map to a default.
- `TaskStatus::Completed` is reachable only through `TandemTask::confirm_completed(now)` and the `confirm_tandem_task_completed` command.
- A newly created Task starts as `Active`. This plan does not launch an Agent and does not invent a Native Session.
- Every SQL write is transactional. Foreign keys remain enabled.
- Reject known structured plaintext credential forms before persistence; never echo matched content in errors, logs, screenshots, or fixtures. `TandemDatabase` must remain outside every existing CC-Switch export/WebDAV/S3 code path. Full credential/keychain ownership remains Roadmap Plan 2.
- Browser demo code is reachable only through `tandem-demo.html`; the production entry always injects the Tauri gateway and never imports demo modules.
- Do not add Handoff, Preset projection, session scanning, iTerm2 launching, worktrees, comparison, or LLM-generated status inference in this plan.

## Fixed Interfaces

Create these Rust contracts in `src-tauri/src/tandem/domain.rs`:

```rust
pub type TimestampMs = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Cursor,
    Codex,
    Claude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Active,
    NeedsAttention,
    AwaitingAcceptance,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Active,
    AwaitingUser,
    AwaitingAcceptance,
    Paused,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TandemTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub original_instruction: String,
    pub status: TaskStatus,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
    pub completed_at: Option<TimestampMs>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: String,
    pub task_id: String,
    pub agent: AgentKind,
    pub status: RunStatus,
    pub native_session_id: Option<String>,
    pub native_session_ref: Option<String>,
    pub started_at: TimestampMs,
    pub updated_at: TimestampMs,
    pub ended_at: Option<TimestampMs>,
}

pub struct NewTask {
    pub project_name: String,
    pub project_root_path: String,
    pub title: String,
    pub original_instruction: String,
}
```

Validation and transition methods:

```rust
impl NewTask {
    pub fn validate(self) -> Result<Self, TandemDomainError>;
}

impl TandemTask {
    pub fn new(id: String, project_id: String, input: NewTask, now: TimestampMs) -> Self;
    pub fn mark_needs_attention(&mut self, now: TimestampMs);
    pub fn mark_awaiting_acceptance(&mut self, now: TimestampMs);
    pub fn pause(&mut self, now: TimestampMs);
    pub fn resume(&mut self, now: TimestampMs);
    pub fn confirm_completed(&mut self, now: TimestampMs) -> Result<(), TandemDomainError>;
}
```

Validation rules are exact: trim all four `NewTask` strings; reject an empty value; reject a title over 120 Unicode scalar values; reject an instruction over 20,000 Unicode scalar values. Scan `title` and `original_instruction` for PEM private-key headers; known token prefixes `sk-`, `sk_live_`, `ghp_`, `github_pat_`, `xox[baprs]-`, and `AKIA` followed by their documented minimum token lengths; and case-insensitive named assignments (`api_key`, `apikey`, `token`, `secret`, `password`) followed by `:` or `=` and at least 12 non-space characters. Return only `CredentialKind`, never the match. This is a high-confidence structured-credential guard, not a claim to detect arbitrary secrets in prose. `confirm_completed` sets status, `updated_at`, and `completed_at`; repeated confirmation returns `AlreadyCompleted` without changing timestamps.

Create these JSON-facing TypeScript contracts in `src/tandem/types.ts`:

```ts
export type TaskStatus =
  | "active"
  | "needs_attention"
  | "awaiting_acceptance"
  | "paused"
  | "completed";

export interface Project { id: string; name: string; rootPath: string; createdAt: number; updatedAt: number }
export interface TandemTask { id: string; projectId: string; title: string; originalInstruction: string; status: TaskStatus; createdAt: number; updatedAt: number; completedAt: number | null }
export interface TaskLedgerItem { task: TandemTask; project: Project }
export interface TaskLedger { needsAttention: TaskLedgerItem[]; awaitingAcceptance: TaskLedgerItem[]; active: TaskLedgerItem[]; recentResumable: TaskLedgerItem[] }
export interface CreateTaskInput { projectName: string; projectRootPath: string; title: string; originalInstruction: string }

export interface TaskGateway {
  listLedger(): Promise<TaskLedger>;
  createTask(input: CreateTaskInput): Promise<TaskLedgerItem>;
  confirmCompleted(taskId: string): Promise<TaskLedgerItem>;
}
```

Ledger classification is exact:

- `needsAttention`: `needs_attention`, newest `updatedAt` first;
- `awaitingAcceptance`: `awaiting_acceptance`, newest first;
- `active`: `active`, newest first;
- `recentResumable`: `paused`, newest first, at most 10;
- completed Tasks are omitted from all four sections.

---

### Task 1: Establish the Tandem Branch, Provenance, and Baseline

**Files:**

- Create: `docs/tandem/2026-08-14-tandem-product-design.md`
- Create: `docs/tandem/2026-08-14-tandem-v0-roadmap.md`
- Create: `docs/tandem/2026-08-14-tandem-foundation.md`
- Create: `docs/tandem/PROVENANCE.md`
- Modify: `README.md`

**Step 1: Inspect and isolate the repository**

Run from `/Users/feng/Codes/dev/cc-switch`:

```bash
git status --short --branch
git remote -v
git log -1 --oneline
```

Expected: `origin` points to `youxuanxue/cc-switch`; record, but do not alter, unrelated dirty files.

Load `using-git-worktrees` and create a worktree for `feat/tandem-foundation`. Let that Skill choose the collision-free physical directory. Export its path as `$WORKTREE` for every remaining command.

If `upstream` is absent:

```bash
git remote add upstream https://github.com/farion1231/cc-switch.git
git remote get-url upstream
```

Expected final line: `https://github.com/farion1231/cc-switch.git`.

**Step 2: Prove the source document is the approved version**

```bash
git -C /Users/feng/Codes/dev/Tandem show 1251e9c:docs/superpowers/specs/2026-08-14-tandem-product-design.md >/tmp/tandem-product-design.approved.md
cmp /tmp/tandem-product-design.approved.md /Users/feng/Codes/dev/Tandem/docs/superpowers/specs/2026-08-14-tandem-product-design.md
```

Expected: `cmp` exits 0. Stop if it differs; do not migrate an unapproved working copy.

**Step 3: Copy the three approved planning documents**

Copy, without editing their content:

```bash
mkdir -p "$WORKTREE/docs/tandem"
cp /tmp/tandem-product-design.approved.md "$WORKTREE/docs/tandem/2026-08-14-tandem-product-design.md"
cp /Users/feng/Codes/dev/Tandem/docs/superpowers/plans/2026-08-14-tandem-v0-roadmap.md "$WORKTREE/docs/tandem/2026-08-14-tandem-v0-roadmap.md"
cp /Users/feng/Codes/dev/Tandem/docs/superpowers/plans/2026-08-14-tandem-foundation.md "$WORKTREE/docs/tandem/2026-08-14-tandem-foundation.md"
```

Create `docs/tandem/PROVENANCE.md` with:

- CC-Switch origin, upstream URL, inherited license, and fork base commit;
- Tandem source commit `1251e9c` and the three copied paths;
- `dev-rules` commit `e235d91` as an inspected future migration source, with no imported code in this plan;
- a rule requiring future imported files to list source path, source commit, destination, and adaptation summary.

Add a concise “Tandem development” section near the top of `README.md` that links the product design and roadmap. Preserve the current CC-Switch introduction and usage documentation.

**Step 4: Verify the untouched baseline**

Run in `$WORKTREE`:

```bash
pnpm install --frozen-lockfile
pnpm test:unit
pnpm run build:renderer
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all existing tests pass and renderer build exits 0. If an existing test fails, save the exact command/output in `docs/tandem/baseline-failures.md` and stop this plan; do not normalize a red baseline.

**Step 5: Commit**

```bash
git add README.md docs/tandem
git commit -m "docs: establish Tandem product baseline"
```

---

### Task 2: Implement Domain Types and Completion Invariants

**Files:**

- Create: `src-tauri/src/tandem/mod.rs`
- Create: `src-tauri/src/tandem/domain.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write failing domain tests**

At the bottom of `domain.rs`, add unit tests for:

1. `NewTask::validate` trims all values;
2. each empty field is rejected with the field name;
3. 121-character title and 20,001-character instruction are rejected;
4. every fixed structured-credential form is rejected, near-miss prose remains accepted, and no error string contains the matched value;
5. `TandemTask::new` starts `Active`, uses equal created/updated timestamps, and has no completion time;
6. `mark_needs_attention`, `mark_awaiting_acceptance`, `pause`, and `resume` set exact states and timestamps;
7. only `confirm_completed` sets `Completed` and `completed_at`;
8. repeated confirmation returns `AlreadyCompleted` and preserves the first completion timestamp;
9. serde names for all status and Agent enums are stable snake_case.

The tests must use fixed IDs and timestamps; no wall clock in unit tests.

**Step 2: Run tests to verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml tandem::domain --lib
```

Expected: compile failure because `domain` types and methods do not exist.

**Step 3: Implement the minimal domain**

Implement the fixed interfaces above plus:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind { PrivateKey, ApiToken, NamedSecret }

pub fn detect_structured_credential(value: &str) -> Option<CredentialKind>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TandemDomainError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains a structured plaintext credential ({kind:?})")]
    StructuredCredential { field: &'static str, kind: CredentialKind },
    #[error("task is already completed")]
    AlreadyCompleted,
}
```

Add `pub mod tandem;` in `src-tauri/src/lib.rs`. Keep constructors and transitions free of SQLite, Tauri, clocks, and filesystem access.

**Step 4: Verify tests pass**

```bash
cargo test --manifest-path src-tauri/Cargo.toml tandem::domain --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings
```

Expected: domain tests and Clippy pass.

**Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/tandem
git commit -m "feat(tandem): add task and run domain model"
```

---

### Task 3: Persist Projects, Tasks, Runs, and Native Session References

**Files:**

- Create: `src-tauri/src/tandem/database.rs`
- Create: `src-tauri/src/tandem/repository.rs`
- Create: `src-tauri/tests/tandem_repository.rs`
- Modify: `src-tauri/src/tandem/mod.rs`

**Step 1: Write failing schema and repository tests**

In `src-tauri/tests/tandem_repository.rs`, use `TandemDatabase::memory()` to test the public API without filesystem or global-HOME coupling:

```rust
pub fn create_task(&self, input: NewTask, now: TimestampMs) -> Result<TaskLedgerItem, AppError>;
pub fn list_ledger(&self) -> Result<TaskLedger, AppError>;
pub fn confirm_task_completed(&self, task_id: &str, now: TimestampMs) -> Result<TaskLedgerItem, AppError>;
```

Add repository-owned transport structs in `repository.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLedgerItem { pub task: TandemTask, pub project: Project }
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLedger {
    pub needs_attention: Vec<TaskLedgerItem>,
    pub awaiting_acceptance: Vec<TaskLedgerItem>,
    pub active: Vec<TaskLedgerItem>,
    pub recent_resumable: Vec<TaskLedgerItem>,
}
```

The external repository test covers only public behavior:

- `UNIQUE(root_path)` behavior reuses one Project when two Tasks use the same trimmed root;
- create and list round-trip all Unicode-safe non-secret fields;
- completing a Task persists `completed_at` and removes it from the ledger;
- completing a missing Task returns a not-found error;
- repeated completion returns `AlreadyCompleted` and preserves timestamps.

In a `#[cfg(test)]` module inside `repository.rs`, use `TandemDatabase::memory()` plus private fixture helpers to insert fixed-ID Tasks in every status. Test ledger grouping/order, the 10-item paused limit, completed omission, invalid persisted enum errors, transaction rollback after an induced foreign-key failure, and Project deletion cascading to Tasks and Runs. Do not expose the connection and do not add production state-transition APIs solely for fixtures.

In `tandem/database.rs`, add unit tests for a brand-new schema version 1 database. Verify all three tables, fixed-interface columns, indexes, status checks, foreign keys, and that `tandem_runs.native_session_id`/`native_session_ref` are nullable without uniqueness constraints. Add a reopen test against a `tempfile::TempDir`: create one Task, drop the database, reopen the same `tandem.db`, and read it back. Keep existing sync service signatures unchanged as `Arc<Database>`; final review must inspect the call graph and confirm `TandemDatabase` is passed only to Tandem commands and tray summary code.

**Step 2: Run tests to verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml tandem_repository
cargo test --manifest-path src-tauri/Cargo.toml tandem::database --lib
```

Expected: missing `TandemDatabase` module and methods.

**Step 3: Implement the independent Tandem schema**

Implement `TandemDatabase::init(path: &Path)` and `TandemDatabase::memory()` with a private `Mutex<Connection>`, `PRAGMA foreign_keys = ON`, and Tandem schema version 1. The production caller supplies `get_app_config_dir().join("tandem.db")`; the type never derives or opens `cc-switch.db`:

```sql
PRAGMA user_version = 1;
CREATE TABLE tandem_projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  root_path TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE tandem_tasks (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES tandem_projects(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  original_instruction TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active','needs_attention','awaiting_acceptance','paused','completed')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  completed_at INTEGER
);
CREATE INDEX idx_tandem_tasks_ledger ON tandem_tasks(status, updated_at DESC);
CREATE TABLE tandem_runs (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL REFERENCES tandem_tasks(id) ON DELETE CASCADE,
  agent TEXT NOT NULL CHECK (agent IN ('cursor','codex','claude')),
  status TEXT NOT NULL CHECK (status IN ('active','awaiting_user','awaiting_acceptance','paused','ended')),
  native_session_id TEXT,
  native_session_ref TEXT,
  started_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  ended_at INTEGER
);
CREATE INDEX idx_tandem_runs_task ON tandem_runs(task_id, started_at DESC);
```

Do not add JSON blobs or speculative columns.

**Step 4: Implement repository methods transactionally**

- Generate Project/Task IDs with `Uuid::new_v4().to_string()`.
- Reuse a Project by exact trimmed `root_path`; update its name and `updated_at` on reuse.
- Convert persisted enum strings with `TryFrom<&str>` implemented in the domain module.
- Query only non-completed statuses required by the ledger.
- Load the Task, call its domain transition, then update it in the same transaction for completion.
- Map missing IDs to `AppError::Message("Tandem task not found: {id}")`.
- Map `TandemDomainError` without including task instruction text.

**Step 5: Verify migration and persistence**

```bash
cargo test --manifest-path src-tauri/Cargo.toml tandem_repository
cargo test --manifest-path src-tauri/Cargo.toml tandem::database --lib
cargo test --manifest-path src-tauri/Cargo.toml tandem::repository --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --lib --tests -- -D warnings
```

Expected: all focused tests and Clippy pass.

**Step 6: Commit**

```bash
git add src-tauri/src/tandem src-tauri/tests/tandem_repository.rs
git commit -m "feat(tandem): persist task ledger in SQLite"
```

---

### Task 4: Wire the Independent Database and Expose Thin Tauri Commands

**Files:**

- Create: `src-tauri/src/commands/tandem.rs`
- Create: `src-tauri/tests/tandem_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write failing command tests**

Expose plain async implementation functions callable without a Tauri runtime, and thin `#[tauri::command]` wrappers:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTandemTaskInput {
    pub project_name: String,
    pub project_root_path: String,
    pub title: String,
    pub original_instruction: String,
}

pub async fn create_tandem_task_impl(db: Arc<TandemDatabase>, input: CreateTandemTaskInput, now: i64) -> Result<TaskLedgerItem, String>;
pub async fn list_tandem_ledger_impl(db: Arc<TandemDatabase>) -> Result<TaskLedger, String>;
pub async fn confirm_tandem_task_completed_impl(db: Arc<TandemDatabase>, task_id: String, now: i64) -> Result<TaskLedgerItem, String>;
```

In `src-tauri/tests/tandem_commands.rs`, test camelCase deserialization, validation error stability, create/list/complete round-trip, missing ID, unavailable-database behavior, and that returned errors do not echo `originalInstruction`.

**Step 2: Run test to verify failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test tandem_commands
```

Expected: unresolved `commands::tandem` imports.

**Step 3: Wire startup and implement commands**

Add a Tandem-owned managed service in `tandem/mod.rs`:

```rust
pub struct TandemState {
    pub db: Option<Arc<TandemDatabase>>,
    pub init_error: Option<String>,
}
```

During setup, initialize `TandemDatabase` at `get_app_config_dir().join("tandem.db")` after the legacy database is ready, then register `app.manage(TandemState { ... })` separately from `AppState`. A Tandem initialization failure is logged and stored but does not abort startup, migration recovery, provider configuration, or tray creation.

Wrappers obtain `State<TandemState>`. If `db` is absent, return `Tandem unavailable: {stored reason}` without touching the legacy `AppState` database. Otherwise call `chrono::Utc::now().timestamp_millis()` exactly once and delegate to the implementation functions. Register exactly:

- `commands::create_tandem_task`
- `commands::list_tandem_ledger`
- `commands::confirm_tandem_task_completed`

in the existing `tauri::generate_handler!` list.

No command contains SQL or domain transition logic.

**Step 4: Verify commands**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test tandem_commands
cargo test --manifest-path src-tauri/Cargo.toml tandem
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: focused tests and Clippy pass.

**Step 5: Commit**

```bash
git add src-tauri/src/commands src-tauri/src/lib.rs src-tauri/tests/tandem_commands.rs
git commit -m "feat(tandem): expose task ledger commands"
```

---

### Task 5: Build the Tandem Action-Ledger Shell

**Files:**

- Create: `src/tandem/types.ts`
- Create: `src/tandem/api/taskGateway.ts`
- Create: `src/tandem/api/TaskGatewayProvider.tsx`
- Create: `src/tandem/api/tauriTaskGateway.ts`
- Create: `src/tandem/api/demoTaskGateway.ts`
- Create: `src/tandem/hooks/useTaskLedger.ts`
- Create: `src/tandem/components/TandemShell.tsx`
- Create: `src/tandem/components/TaskLedgerPage.tsx`
- Create: `src/tandem/components/NewTaskDialog.tsx`
- Create: `src/tandem/components/TaskSection.tsx`
- Create: `src/tandem/taskLedger.ts`
- Create: `tandem-demo.html`
- Create: `src/tandem/demo/DemoLegacyConfigApp.tsx`
- Create: `src/tandem/demo/main.tsx`
- Create: `tests/tandem/taskLedger.test.ts`
- Create: `tests/tandem/TaskLedgerPage.test.tsx`
- Create: `tests/tandem/TandemShell.test.tsx`
- Modify: `src/main.tsx`
- Modify: `tests/msw/state.ts`
- Modify: `tests/msw/handlers.ts`
- Modify: `tests/setupTests.ts`

**Step 1: Write failing pure ledger tests**

In `taskLedger.test.ts`, assert exact classification, newest-first ordering, completion omission, paused limit, and no input mutation. Export:

```ts
export function buildTaskLedger(items: TaskLedgerItem[]): TaskLedger;
```

Run:

```bash
pnpm vitest run tests/tandem/taskLedger.test.ts
```

Expected: module not found.

**Step 2: Implement pure classification**

Implement only the fixed rules. Use numeric `updatedAt` comparison with task ID as a deterministic ascending tie-breaker.

Run the test again; expected pass.

**Step 3: Write failing gateway and UI tests**

Extend MSW state with deterministic Task fixtures and handlers for the three command names. Reset Task state in the existing reset function.

`TaskLedgerPage.test.tsx` must cover:

- all four section headings render in this order: `需要你处理`, `待验收`, `正在推进`, `最近可继续`;
- empty sections remain visible with concise empty text, not decorative cards;
- clicking `新建任务` opens fields for project name, project path, title, and original instruction;
- submitting calls `create_tandem_task` with camelCase payload and inserts the returned Task into `正在推进`;
- blank fields, overlong title, and the same structured-credential patterns are blocked client-side; Rust remains authoritative and a command-level test proves a bypassed client cannot persist them;
- every non-completed Task row offers `确认完成`; on a `待验收` Task it is the primary row action, while other sections expose it through the compact row action menu;
- clicking `确认完成` opens an explicit confirmation dialog; confirming invokes the command and removes the Task from the ledger;
- API error keeps the Task visible and renders a toast without instruction content.

`TandemShell.test.tsx` must cover:

- ledger is the initial route and primary navigation item;
- `Agent 配置` renders the injected `LegacyConfigApp` component;
- returning to `任务` preserves the last successful query cache;
- no marketing hero or onboarding copy appears.

**Step 4: Run UI tests to verify failure**

```bash
pnpm vitest run tests/tandem/TaskLedgerPage.test.tsx tests/tandem/TandemShell.test.tsx
```

Expected: missing Tandem modules and handlers.

**Step 5: Implement gateways, query hooks, and UI**

Production `createTaskGateway()` always returns `tauriTaskGateway` using `invoke`; it must not contain a demo environment branch. `src/tandem/demo/main.tsx`, reached only through the separate `tandem-demo.html` Vite entry, provides `demoTaskGateway` through `TaskGatewayProvider` and passes `DemoLegacyConfigApp` to `TandemShell`. The demo legacy adapter renders a deterministic `Agent 配置测试台` root and never imports or mounts the real CC-Switch app, because its startup effects require Tauri IPC.

Use query key `['tandem', 'task-ledger']`. Mutations set returned data directly or invalidate that exact key. Do not invalidate provider/configuration queries.

UI requirements:

- a quiet work console with a fixed 52px title/navigation bar and scrollable ledger;
- no nested cards, gradient, glass effects, hero, feature explanation, or decorative illustration;
- compact section headers with count, followed by unframed rows separated by borders;
- each row shows title, project, reliable status, and updated time;
- use lucide `Plus`, `Check`, `Settings`, and `ListTodo` icons with tooltips/accessible names;
- card radius never exceeds the existing 8px token;
- controls remain usable at 900×600 and 390×844 browser viewports;
- show instruction only in the creation dialog in this plan; do not render it in ledger rows or errors.

Change production `main.tsx` to import the existing default `App` as `LegacyConfigApp` and render:

```tsx
<TaskGatewayProvider gateway={createTaskGateway()}>
  <TandemShell LegacyConfigApp={LegacyConfigApp} />
</TaskGatewayProvider>
```

Keep all existing production providers, boundaries, theme, toaster, startup sync, event listeners, and database-upgrade path unchanged. `tandem-demo.html` loads `src/tandem/demo/main.tsx`, which creates its own `QueryClient`, mounts the same `TaskGatewayProvider`, `ThemeProvider`, `TandemShell`, and `Toaster`, and injects only `demoTaskGateway` plus `DemoLegacyConfigApp`. The demo entry must not import production `src/main.tsx` or `src/App.tsx`.

**Step 6: Verify focused frontend behavior**

```bash
pnpm vitest run tests/tandem
pnpm exec tsc --noEmit
pnpm run build:renderer
```

Expected: Tandem tests, typecheck, and build pass.

**Step 7: Commit**

```bash
git add src/main.tsx src/tandem tests/tandem tests/msw tests/setupTests.ts
git commit -m "feat(tandem): add action-ledger application shell"
```

---

### Task 6: Add a Task-Aware Menu-Bar Summary

**Files:**

- Create: `src-tauri/src/tandem/tray_summary.rs`
- Modify: `src-tauri/src/tandem/mod.rs`
- Modify: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Write failing pure summary tests**

In `tray_summary.rs`, test:

```rust
pub struct TaskSummary { pub needs_attention: usize, pub awaiting_acceptance: usize, pub active: usize }
pub fn summarize(ledger: &TaskLedger) -> TaskSummary;
pub fn summary_label(summary: &TaskSummary, language: &str) -> String;
```

Exact labels:

- Chinese (`zh` and `zh-TW`): `任务 · {needs_attention} 需处理 · {awaiting_acceptance} 待验收 · {active} 推进中`
- English and Japanese fallback in this plan: `Tasks · {needs_attention} attention · {awaiting_acceptance} review · {active} active`

Test zero and nonzero counts and ensure paused/completed Tasks do not contribute.

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml tandem::tray_summary --lib
```

Expected: module missing.

**Step 2: Implement summary and integrate the existing tray**

In `create_tray_menu`, after “Open main window” and before the retained CC-Switch provider sections:

- load `list_tandem_ledger()`;
- add one disabled summary item with ID `tandem_task_summary`;
- if the query fails, log the error and use `Tasks unavailable`; tray creation must continue;
- keep every current provider submenu and event ID unchanged.

Change tray tooltip from `CC Switch` to `Tandem` only. Do not change icons, app identity, website item, provider switching, or close behavior.

After successful create and completion commands, call a new `tray::schedule_tandem_tray_refresh(&app)` from the Tauri wrapper. It uses its own atomic 50ms coalescer and ends in the existing full `refresh_tray_menu(&app)`, because the existing `schedule_tray_refresh` only mutates provider usage submenu labels and cannot update a newly added Task item. Add `AppHandle` only to the wrapper, not repository/domain functions.

**Step 3: Test the integration boundary without constructing native menus**

Extract a pure helper in `tray.rs` that receives `Result<TaskLedger, AppError>` and language, then returns the disabled item label. Extend `tray.rs` tests to prove successful counts and the `Tasks unavailable` fallback. Keep the item ID as a constant `TANDEM_TASK_SUMMARY_ID`; assert that constant in the test. Extract the coalescer's final action behind a tiny test seam and assert Tandem refresh calls full `refresh_tray_menu`, while the pre-existing usage refresh still calls `update_tray_usage_labels`. Do not introduce a native-menu mock layer. The isolated Tauri smoke in Task 7 verifies the actual summary item and retained provider menu behavior.

**Step 4: Verify tray behavior**

```bash
cargo test --manifest-path src-tauri/Cargo.toml tandem::tray_summary --lib
cargo test --manifest-path src-tauri/Cargo.toml tray --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: tests and Clippy pass.

**Step 5: Commit**

```bash
git add src-tauri/src/tandem src-tauri/src/tray.rs src-tauri/src/lib.rs src-tauri/src/commands/tandem.rs
git commit -m "feat(tandem): show task state in menu bar"
```

---

### Task 7: Add Production-Component UI Coverage and Real-App Smoke Verification

**Files:**

- Create: `playwright.config.ts`
- Create: `e2e/tandem-ledger.spec.ts`
- Create: `e2e/__screenshots__/.gitkeep`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `docs/tandem/2026-08-14-tandem-foundation.md` only if execution discoveries require an explicit amendment

**Step 1: Add the failing Playwright test first**

Install the test dependency:

```bash
pnpm add -D @playwright/test
pnpm exec playwright install chromium
```

Add scripts:

```json
"test:e2e": "playwright test",
"test:e2e:update": "playwright test --update-snapshots"
```

Configure `webServer.command` as:

```bash
pnpm run dev:renderer -- --host 127.0.0.1
```

Use base URL `http://127.0.0.1:3000/tandem-demo.html`, port 3000, `reuseExistingServer: false`, one worker, Chromium only, and retain trace/screenshot on failure. The demo gateway must start with deterministic Tasks in all four non-completed statuses. This is production-component browser coverage, not a Tauri IPC/SQLite e2e claim: the separate command/reopen tests and isolated real-app smoke below own those boundaries.

Test at desktop 1200×800 and mobile-like 390×844 viewports:

1. ledger is the first screen;
2. all section headings and deterministic fixture rows are visible without overlap;
3. create `修复恢复流程` in `/tmp/tandem-demo`, see it under `正在推进`;
4. confirm the fixture acceptance Task and see it removed;
5. open `Agent 配置`, assert the deterministic demo legacy root renders, then return to `任务`;
6. run an accessibility scan using Playwright role/name assertions for all interactive controls;
7. capture named screenshots `ledger-desktop.png` and `ledger-narrow.png`.

**Step 2: Run to verify failure**

```bash
pnpm test:e2e
```

Expected: failure until demo gateway and shell behavior satisfy the journey. A missing browser is an environment setup failure; install Chromium and rerun rather than weakening the test.

**Step 3: Make the minimum demo/test adjustments**

Fix only deterministic demo data, accessible names, responsive constraints, and genuine defects exposed by the journey. Demo code must remain reachable only from `tandem-demo.html`, must not be imported by the production entry, and must not be exported from the Tauri gateway.

Review both screenshots with an image viewer. Reject and fix any clipped text, overlap, nested card treatment, hidden command, or blank legacy route. Do not approve snapshots solely because the pixel test produced files.

**Step 4: Run the complete verification matrix**

From `$WORKTREE`:

```bash
pnpm test:unit
pnpm exec tsc --noEmit
pnpm run build:renderer
pnpm test:e2e
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: every command exits 0.

Then run a real Tauri development smoke:

```bash
CC_SWITCH_TEST_HOME="$(mktemp -d /tmp/tandem-foundation-smoke.XXXXXX)" pnpm run dev
```

Manually verify, using a disposable non-secret instruction:

- the Tandem ledger is the initial window;
- create a Task and restart the application; the Task remains;
- navigate to Agent Configuration and confirm the retained provider UI loads;
- menu-bar summary item exists, counts the persisted Task, and the retained provider submenus still open;
- explicitly completing the newly created active Task requires confirmation, removes it from the ledger, and updates the menu;
- closing the main window leaves the menu-bar app recoverable.

Stop the dev process cleanly after verification. Record the printed `/tmp/tandem-foundation-smoke.*` path, confirm it contains the disposable `.cc-switch/cc-switch.db`, then delete only that recorded temporary root. Never run this smoke without `CC_SWITCH_TEST_HOME`.

**Step 5: Review scope and security**

Run:

```bash
git diff --check
git status --short
git diff --stat upstream/main...HEAD
git grep -n -E '(api[_-]?key|secret|token)[[:space:]]*[:=][[:space:]]*["'"'][^"'"']+' -- src-tauri/src/tandem src/tandem e2e tests/tandem || true
```

Expected: only planned files, no whitespace errors, and no secret literals. Inspect every match if the grep produces output.

Use `requesting-code-review`. Required review questions:

- Can any code path complete a Task without explicit confirmation?
- Does the production module graph import any demo entry, gateway, fixture, or legacy adapter?
- Does any error or UI row expose original instructions?
- Did the migration alter existing CC-Switch tables or identity?
- Are retained provider/configuration paths still reachable and independently cached?
- Does the tray survive a Tandem repository error?

Resolve all high/medium findings and rerun the affected focused tests plus the full matrix.

**Step 6: Commit the UI smoke harness**

```bash
git add package.json pnpm-lock.yaml playwright.config.ts e2e docs/tandem
git commit -m "test(tandem): cover foundation user journey"
```

**Step 7: Finish the branch**

Load `finishing-a-development-branch`. Present exact test evidence, screenshot paths, commits, known non-blocking limitations, and integration options. Do not merge, push, or delete the worktree without the user's selected option.

## Plan Exit Evidence

The implementation is complete only when the final report contains:

- worktree path, branch, base commit, and all Tandem commit hashes;
- output summary for the full frontend/Rust/Playwright verification matrix;
- desktop and narrow screenshot paths with a human visual review statement;
- a persistence smoke using a disposable Task;
- proof the menu-bar count and ledger derive from the same SQLite repository;
- proof the retained Agent Configuration route works;
- explicit confirmation that app identity/data roots were not migrated;
- code-review findings and their resolutions;
- the next roadmap plan: Product Identity, Data Migration, and Single Writer.
