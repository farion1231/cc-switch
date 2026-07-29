# Codex Provider Switch Review Follow-up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the three P1 review gaps in proxy hot switching, credential
preservation, and Windows JSONL replacement without changing the approved provider
semantics.

**Architecture:** Extend the existing Codex switch journal into proxy takeover,
separate target-provider token preparation from preservation of an already-live
route, and replace the Windows delete-then-rename operation with a same-directory
atomic OS move. Each behavior is introduced with a focused failing regression test
before production code changes.

**Tech Stack:** Rust, Tauri, `toml_edit`, SQLite/rusqlite, `windows-sys`, Cargo
integration and unit tests.

---

### Task 1: Preserve an existing scoped third-party token

**Files:**
- Modify: `src-tauri/src/codex_config.rs:1798-1812`
- Test: `src-tauri/src/codex_config.rs` unit tests

- [ ] **Step 1: Write the failing scoped-token precedence test**

Add a unit test that passes an official global API key together with a live custom
provider containing `experimental_bearer_token = "third-party-key"`, calls the
wished-for preservation helper, and asserts that the output still contains
`third-party-key` rather than `official-openai-key`.

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```powershell
cargo test --lib preserve_live_route_prefers_existing_scoped_token_over_global_key -- --exact
```

Expected: compilation/test failure because the preservation helper does not exist.

- [ ] **Step 3: Add the scoped-token-first preservation helper**

Implement a private helper whose precedence is:

```rust
extract_codex_experimental_bearer_token(config_text)
    .or_else(|| extract_codex_auth_api_key(auth))
```

Use it only while constructing `safe_live_config`. Leave
`prepare_codex_provider_live_config` auth-first for target-provider writes.

- [ ] **Step 4: Run the focused test and related Codex config tests**

Run:

```powershell
cargo test --lib codex_config::tests
```

Expected: all Codex config tests pass.

### Task 2: Reconcile history in proxy takeover hot switches

**Files:**
- Modify: `src-tauri/src/services/proxy.rs:2324-2489`
- Test: `src-tauri/tests/provider_service.rs:1741-1891`

- [ ] **Step 1: Write the failing proxy history test**

Enable unified Codex history in the existing stopped-proxy takeover fixture, seed
JSONL and SQLite artifacts with the old route, switch to `new-provider`, and assert
that all persisted routes become `deepseek-new` with model
`deepseek-reasoner`.

- [ ] **Step 2: Run the focused integration test and verify RED**

Run:

```powershell
cargo test --test provider_service switch_codex_provider_with_takeover_live_but_stopped_proxy_keeps_proxy_live_config -- --exact
```

Expected: route assertion fails because the takeover branch returns without history
reconciliation.

- [ ] **Step 3: Extend the hot-switch transaction**

After live/backup preparation succeeds, call
`reconcile_history_for_provider(&provider)` for Codex when unified history is
enabled. Keep the returned `AppliedHistoryReconcile` until local and database
current-provider publication succeeds. On reconciliation, settings, or database
failure, roll history back before invoking the existing hot-switch preparation
rollback and include rollback failures in the returned error.

- [ ] **Step 4: Add and verify the rollback regression**

Seed an invalid nested `state_*.sqlite`, attempt the same takeover switch, and assert
that the previous current provider, live config/backup, and JSONL/SQLite routes are
unchanged. Run both focused takeover tests and expect them to pass.

### Task 3: Make Windows replacement safe and restore absent JSONL files

**Files:**
- Modify: `src-tauri/Cargo.toml:93-95`
- Modify: `src-tauri/src/config.rs:290-337`
- Modify: `src-tauri/src/codex_history_reconcile.rs:575-584`
- Test: `src-tauri/src/config.rs` unit tests
- Test: `src-tauri/src/codex_history_reconcile.rs` unit tests

- [ ] **Step 1: Write the failing absent-destination rollback test**

Create a journaled `JsonlChange` whose destination has been removed, call
`rollback_jsonl_change`, and assert that the original contents are recreated.

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```powershell
cargo test --lib rollback_jsonl_change_recreates_missing_destination -- --exact
```

Expected: failure with a not-found I/O error.

- [ ] **Step 3: Implement atomic Windows replacement and rollback fallback**

Enable `Win32_Storage_FileSystem` for `windows-sys`. Replace the Windows
remove-then-rename branch with `MoveFileExW` using
`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` on the same-directory temporary
file. If rollback reads `NotFound`, write the journaled original and restore its
timestamps; other read errors remain fatal.

- [ ] **Step 4: Add the Windows replacement test and verify GREEN**

Add a Windows-only test that replaces an existing file through the new helper and
asserts the new complete contents. Run the config and history reconciliation unit
tests and expect all to pass.

### Task 4: Verify, publish, and answer review threads

**Files:**
- Update: `docs/superpowers/plans/2026-07-27-codex-provider-switch-consistency.md`

- [ ] **Step 1: Run focused and full validation**

Run Rust formatting, Clippy with warnings denied, all runnable Rust targets,
TypeScript typecheck/format checks, the 522-test frontend suite, release compilation,
and Windows MSI packaging.

- [ ] **Step 2: Commit and push the review fixes**

Commit only the intended source, tests, Cargo feature, and plan status updates; push
the branch and verify the fork and PR head SHA match.

- [ ] **Step 3: Reply in the three inline threads**

Reply with `OK.` followed by the concrete fix and regression test for that thread.
Resolve a thread only after GitHub shows the pushed commit and the matching reply.

- [ ] **Step 4: Re-read PR checks and package metadata**

Confirm all new GitHub checks, the clean working tree, the MSI path, size, and SHA256
before reporting completion.
