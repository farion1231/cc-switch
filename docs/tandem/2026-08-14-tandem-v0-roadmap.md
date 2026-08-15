# Tandem v0 Implementation Roadmap

> **For agentic workers:** Each detailed plan derived from this roadmap must be executed with `subagent-driven-development` (recommended) or `executing-plans`. Create an isolated worktree with `using-git-worktrees` before changing the CC-Switch fork.

**Goal:** Deliver the approved Tandem v0 promise: find existing Code Agent work, resume it in the native CLI, and hand it to another supported Agent without manually recovering context.

**Architecture:** The existing `youxuanxue/cc-switch` fork becomes the Tandem code repository so CC-Switch ancestry remains intact and upstream changes can be merged selectively. Tandem adds an independent Rust domain core and React product shell around retained CC-Switch configuration capabilities, then incrementally replaces CC-Switch ownership with Tandem-owned tasks, adapters, presets, and handoffs.

**Tech Stack:** Tauri 2, Rust 1.85+, SQLite/rusqlite, React 18, TypeScript 5, TanStack Query, Vitest/Testing Library/MSW, Playwright, macOS 12+, iTerm2.

## Global Constraints

- The code repository is `/Users/feng/Codes/dev/cc-switch`; `/Users/feng/Codes/dev/Tandem` remains the product-discovery source until its approved documents are migrated.
- Preserve `origin` as `git@github.com:youxuanxue/cc-switch.git`; register `https://github.com/farion1231/cc-switch.git` as `upstream`.
- Develop Tandem from CC-Switch ancestry; do not combine unrelated Git histories.
- Preserve MIT attribution and record every imported `dev-rules`, `twin`, Skill, and CC-Switch source.
- Cursor Agent CLI, Codex CLI, and Claude Code CLI are the only v0 Agent adapters.
- Native CLI transcript and native resume remain owned by each CLI.
- Native resume within the same Agent never creates a Handoff or a new Run.
- Switching Agent always creates a new Run and a Handoff.
- Human-interactive Runs expose reliable facts only; they do not infer progress from transcript text.
- Only explicit user confirmation completes a Task.
- Shared project method belongs in `.tandem/`; machine paths, provider/model choices, sessions, and secrets do not.
- Plaintext secrets must not enter `.tandem/`, SQLite task records, logs, notifications, or Handoffs.
- Every product-visible workflow must exercise the production React components through Playwright; native Tauri/CLI/iTerm2 boundaries require automated Rust integration tests plus an isolated real-app smoke when macOS offers no supported WebDriver path.
- Keep CC-Switch upstream-facing modules recognizable; Tandem-specific code lives under explicit `tandem` modules.

---

## Plan Sequence

### Plan 1: Fork Baseline and Task Ledger Vertical Slice

**Detailed plan:** discovery source `docs/superpowers/plans/2026-08-14-tandem-foundation.md`; canonical code-repository path after Plan 1 migration `docs/tandem/2026-08-14-tandem-foundation.md`.

**Delivers:**

- Tandem product branch on the existing CC-Switch fork with `upstream` registered;
- approved Tandem product specification and provenance in the code repository;
- independent Rust Task/Run domain types and invariants;
- an independent local `tandem.db` and repository for Projects, Tasks, Runs, and Native Session references, excluded from inherited CC-Switch export/WebDAV/S3 paths;
- Tauri commands for creating, listing, and explicitly completing Tasks;
- React Tandem shell with action-ledger home and retained CC-Switch configuration route;
- task-aware menu-bar summary;
- unit, integration, migration, and Playwright visual smoke tests.

**Intentionally preserves:** `~/.cc-switch`, `com.ccswitch.desktop`, `ccswitch://`, current updater endpoints, existing provider menus, and existing CC-Switch data ownership. Identity and data migration are a separate reversible change.

**Exit conditions:**

- a Task created through the Tandem UI persists in SQLite and reappears after a backend restart;
- the ledger groups reliable Task statuses into attention, acceptance, active, and resumable sections;
- no state transition other than explicit `ConfirmTaskCompleted` can produce `Completed`;
- CC-Switch provider/configuration UI remains reachable;
- menu-bar summary derives from the same Task repository;
- Rust tests, focused Vitest suites, typecheck, renderer build, and Playwright smoke pass.

### Plan 2: Product Identity, Data Migration, and Single Writer

**Depends on:** Plan 1.

**Delivers:**

- Tandem product name, bundle identifier, application icons, deep-link scheme, log names, and updater configuration;
- `~/.tandem` as the new application data root;
- transactional import from the existing CC-Switch database and configuration directory;
- migration receipt, backup, rollback, and idempotent rerun behavior;
- macOS Keychain references for secrets selected for Tandem ownership;
- a single-writer cutover that prevents Tandem and CC-Switch from concurrently mutating live Agent configuration;
- first-run migration UI and recovery UI.

**Required spike:** Decide whether provider secrets can be moved immediately without breaking upstream mergeability. The spike must inventory every secret-bearing CC-Switch field and live file before the detailed plan is written.

**Exit conditions:**

- a copied real CC-Switch profile migrates without losing providers, Profiles, Skills, or current selections;
- interruption at every migration checkpoint leaves either the old source or the committed Tandem destination usable;
- rerunning migration is idempotent;
- secret scan finds no newly persisted plaintext Tandem secret;
- signed development build launches as Tandem and never writes through the old CC-Switch process.

### Plan 3: Three-Agent Session Inbox and Candidate Tasks

**Depends on:** Plans 1 and 2.

**Delivers:**

- versioned Cursor, Codex, and Claude session scanner contracts;
- full and incremental local indexing;
- searchable session inbox with source facts and resumability status;
- candidate Task title and grouping suggestions clearly marked as machine suggestions;
- claim, rename, merge, split, ignore, and batch archive flows;
- project discovery policy: metadata-only, managed/readable, or sensitive/off;
- no requirement to clear the inbox before using Tandem.

**Required spikes:** Capture current session formats and resume commands for all three installed CLI versions. Fixtures must be redacted copies of real formats, not invented JSON.

**Exit conditions:**

- one malformed session cannot block another Agent's scan;
- any indexed session is searchable by Agent, project, time, and source title/summary;
- no candidate becomes a formal Task without a user action;
- accepted user titles are never overwritten by rescans;
- each Agent scanner passes the same black-box fixture suite and one real local smoke.

### Plan 4: Managed Launch, iTerm2 Binding, and Native Resume

**Depends on:** Plan 3.

**Delivers:**

- quick Task creation from an original instruction;
- Execution Profile selection using migrated CC-Switch provider/model capabilities;
- Cursor, Codex, and Claude launch adapters;
- iTerm2 tab creation, cwd setup, stable reference capture, focus, and fallback tab creation;
- binding a launched native session to one Run;
- append-only Adapter receipts capturing adapter/CLI version, actual Agent, model, Profile, selected capability, result, and observed time;
- restart recovery ledger that lets the user select which unfinished Tasks to reopen;
- native resume in the correct Workspace without creating a Handoff or new Run.

**Required spikes:** Validate iTerm2 Python API versus AppleScript for stable identity and focus; validate launch/session-ID binding for each CLI version.

**Exit conditions:**

- each Agent can be launched from the real Tandem UI in a selected Git project;
- the resulting native session is bound exactly once;
- all three real CLIs persist launch/bind/resume receipts and tests compare the selected Agent/model/Profile with observed values and capability outcomes;
- existing tabs are focused when available and new tabs are created only when required;
- app restart and Mac-restart simulation can reopen a selected Task with native resume;
- resume failure remains an explicit failure and never silently switches Agent.

### Plan 5: Agent Presets and `.tandem/` Projection

**Depends on:** Plan 4.

**Delivers:**

- Tandem Host versus Agent Preset ownership model in code;
- migration of the default `dev-rules` method and dedicated research/review/managed-execution presets;
- immutable Preset versions and per-Run snapshots;
- `.tandem/` project schema for shared Rules, Skills, gates, and project Preset additions;
- import of existing `AGENTS.md`, `CLAUDE.md`, `.cursor/rules`, and project Skills;
- lightweight committed compatibility entrypoints;
- deterministic Cursor/Codex/Claude projections, drift detection, repair, and launch blocking on unresolved drift;
- projection receipts extending each Run's receipt chain with actual Preset version, project-config version, and per-capability verification results.

**Required spike:** Build the exact projection matrix for current versions of all three CLIs. Explicitly classify canonical source, committed compatibility entrypoint, ignored generated artifact, and unsupported feature.

**Exit conditions:**

- a clean clone plus `.tandem/` reconstructs the same project method on one configured Mac;
- all three adapters persist receipts proving which Preset/project version and capability result actually applied, and release tests compare them with the selected configuration;
- generated-file edits are detected and repaired, never silently imported as source;
- a running Run cannot switch Preset or Execution Profile;
- a machine without Tandem can still follow committed compatibility entrypoints.

### Plan 6: Cross-Agent Handoff and Optional Managed Execution

**Depends on:** Plan 5.

**Delivers:**

- user-facing “Hand to another Agent” flow;
- lightweight Handoff v2 schema used only for Agent switching;
- source-Agent Skill that emits bounded semantic context;
- deterministic Git/Workspace facts added by Tandem;
- target Run creation in the same Workspace with the same Preset/project configuration;
- delivery, acknowledgement, first-verifiable-action, append-only handoff receipts, and user-confirmed success states;
- degraded Handoff generation when the source Agent is unavailable;
- migration of useful `twin` supervisor/runtime behavior into optional managed execution.

**Exit conditions:**

- Cursor → Codex, Codex → Claude, and Claude → Cursor real handoffs complete without manual prompt/history copying;
- target Agent opens the correct Workspace, receives the intended Preset and Handoff, acknowledges them, and completes one verifiable action;
- no system event alone marks Handoff success; the user confirms it;
- source Run remains intact after target failure;
- managed-execution failure degrades to an understandable human-interactive state.

### Plan 7: Dogfood Hardening and v0 Release Gate

**Depends on:** Plans 1–6.

**Delivers:**

- local backup, restore, retention, and complete-deletion policies;
- crash recovery and durable background-job replay;
- performance budgets for startup, search, ledger refresh, and menu-bar refresh;
- signed/notarized macOS build and updater rollback;
- local-only diagnostic export with secret redaction;
- instrumentation for the approved two-week dogfood metrics.

**Exit conditions:**

- at least 80% of new Code Agent Tasks start in or are claimed by Tandem for two continuous weeks;
- any recent important Task is found and natively resumed within 30 seconds;
- no important Task is abandoned because of an unreadable session ID;
- multiple real cross-Agent handoffs complete without manual instruction recovery;
- Tandem adds no material startup friction to quick tasks;
- all v0 real UI journeys and three-Agent release smokes pass.

## Deferred Phase: Comparison and Fusion

Only after the v0 dogfood gate passes, write a new approved specification and implementation plan for:

- “continue”, “compare”, and “review” Run intents;
- isolated `wts` worktrees for independent attempts;
- evidence-based per-criterion comparison without an aggregate score;
- human selection and fusion instructions;
- simple adoption in the selected Workspace versus multi-source fusion in a new worktree;
- a fusion Agent that implements and verifies the final result while Tandem only orchestrates.

This phase must not leak independent-worktree or comparison abstractions into v0 code before a current consumer needs them.
