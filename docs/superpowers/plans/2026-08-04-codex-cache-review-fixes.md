# Codex Cache Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve the fourth-round Codex review findings by separating the official cache baseline from cc-switch output, gating refreshes on Codex takeover, and forwarding the ChatGPT account ID.

**Architecture:** Preserve the last clean official `models_cache.json` in a cc-switch sidecar and mark every rendered cache as cc-switch-owned. Keep the background writer aligned with the Codex per-app proxy flag, and pass a single parsed authentication context into official model discovery.

**Tech Stack:** Rust, serde_json, reqwest, rusqlite, Cargo unit tests

## Global Constraints

- Do not change the database schema.
- Keep personal ChatGPT accounts without an account ID supported.
- Preserve the synchronous cache refresh during a successful Codex takeover.
- Add no new network fetch to the synchronous cache writer.
- Every production behavior change must be preceded by a failing regression test.

---

### Task 1: Separate the official baseline from rendered cache ownership

**Files:**
- Modify: `src-tauri/src/codex_config.rs:23-24, 908-1010, 4702-5249`

**Interfaces:**
- Consumes: existing `write_json_file`, `write_codex_models_cache_for_official_login_at`, and `write_models_cache_json` behavior.
- Produces: `CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME`, a reliable-baseline loader/persister, and cc-switch-owned live cache writes.

- [ ] **Step 1: Add failing ownership tests**

Add assertions to the aggregate and regular-provider cache tests that an input `W/"official"` etag becomes a `W/"cc-switch-` etag. Add a new official-login test that performs two merges: the first starts from an official cache and persists the sidecar; the second starts from the cc-switch-rendered live cache and must still preserve the original official model by loading the sidecar.

- [ ] **Step 2: Run focused tests and verify RED**

Run `cargo test --manifest-path src-tauri/Cargo.toml write_codex_models_cache_ -- --nocapture`.

Expected: ownership assertions fail because `write_models_cache_json` preserves the official etag, and the repeated official merge cannot find a reliable baseline.

- [ ] **Step 3: Implement the baseline sidecar and ownership write**

Add `CC_SWITCH_CODEX_OFFICIAL_MODELS_CACHE_FILENAME` with value `cc-switch-official-models-cache.json`. Implement helpers that validate a non-empty, non-cc-switch-owned official cache, persist an established baseline to the sidecar, and load it when the live cache is cc-switch-owned. Quarantine a pre-sidecar legacy cache until Codex produces a different official snapshot, and observe an established official baseline before every aggregate or regular-provider overwrite. Change `write_models_cache_json` to always generate a new `W/"cc-switch-{timestamp}"` etag instead of preserving the existing etag.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run the Task 1 command again. Expected: all `write_codex_models_cache_` tests pass.

### Task 2: Gate the refresher on Codex takeover

**Files:**
- Modify: `src-tauri/src/services/proxy.rs:609-660, 3306+`

**Interfaces:**
- Consumes: `Database::get_proxy_flags_sync("codex")`.
- Produces: `fn codex_cache_refresh_enabled(db: &Database) -> bool` used by the background refresher.

- [ ] **Step 1: Add a failing per-app takeover test**

Using `Database::memory()`, enable only Claude and assert cache refresh is disabled; then enable Codex and assert it is enabled. The test must exercise `codex_cache_refresh_enabled` rather than duplicate the SQL query.

- [ ] **Step 2: Run the focused test and verify RED**

Run `cargo test --manifest-path src-tauri/Cargo.toml codex_cache_refresh_enabled -- --nocapture`.

Expected: compilation fails because the helper does not exist.

- [ ] **Step 3: Implement and apply the guard**

Implement `fn codex_cache_refresh_enabled(db: &Database) -> bool` by returning `db.get_proxy_flags_sync(AppType::Codex.as_str()).0`. Call it immediately after upgrading the weak database reference. If false, keep the refresher alive but skip the current cycle so enabling Codex later resumes refreshes without restarting a proxy used by another app. Before publishing, acquire the shared Codex switch lock and re-read both the flag and current provider under the lock to serialize publication with disable and hot-switch operations.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run the Task 2 command again. Expected: the per-app flag test passes.

### Task 3: Forward the auth.json account ID

**Files:**
- Modify: `src-tauri/src/codex_config.rs:188-205, test module`
- Modify: `src-tauri/src/services/codex_oauth_models.rs:15-70, test module`
- Modify: `src-tauri/src/commands/codex_oauth.rs:92-104`

**Interfaces:**
- Produces: `CodexAuthCredentials { access_token: String, account_id: Option<String> }` and `read_codex_auth_credentials()`.
- Changes: `fetch_official_models_with_token(token: &str, account_id: Option<&str>)`.

- [ ] **Step 1: Add failing credential and request-header tests**

Add a pure JSON parsing regression test using whitespace-padded token/account values and assert both are trimmed. Extract a request-builder helper in the test's desired interface and assert the built official models request contains `chatgpt-account-id: workspace-123` when supplied and omits it when `None`.

- [ ] **Step 2: Run focused tests and verify RED**

Run `cargo test --manifest-path src-tauri/Cargo.toml codex_auth_credentials -- --nocapture` and `cargo test --manifest-path src-tauri/Cargo.toml official_models_request -- --nocapture`.

Expected: compilation fails because the credentials parser and optional-account request builder do not exist.

- [ ] **Step 3: Implement unified credentials and optional header propagation**

Parse both fields from one `Value`, require a non-empty access token, and normalize an empty account ID to `None`. Update `get_codex_official_models` to pass `credentials.account_id.as_deref()`. Build the HTTP request through one helper that conditionally adds `chatgpt-account-id`.

- [ ] **Step 4: Run focused tests and verify GREEN**

Run both Task 3 commands again. Expected: credential and header tests pass.

### Task 4: Full verification and independent review

**Files:**
- Verify all modified Rust files and the design/plan documents.

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces: verification evidence and an independent reviewer assessment.

- [ ] **Step 1: Format and inspect the diff**

Run `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, `git diff --check`, and `git diff --stat`.

- [ ] **Step 2: Run Rust verification**

Run `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`.

Expected: zero test failures and zero clippy warnings.

- [ ] **Step 3: Dispatch an independent reviewer agent**

Provide the reviewer the original three findings, the approved design, the base SHA `435933f1`, the final head/diff, and ask it to inspect correctness, regressions, tests, lifecycle races, cache provenance, and account-header handling. Do not provide implementation reasoning beyond the requirements.

- [ ] **Step 4: Address all Critical and Important findings**

For every accepted finding, repeat RED to GREEN and rerun the relevant focused and full verification commands. Document any rejected finding with concrete code and test evidence.

### Task 5: Close the fifth-round routing and TTL findings

**Files:**
- Modify: `src-tauri/src/proxy/handler_context.rs`
- Modify: `src-tauri/src/proxy/providers/codex.rs`
- Modify: `src-tauri/src/codex_config.rs`

- [ ] **Step 1: Reproduce all findings with state-combination tests**

Prove that a mapped slot is misrouted when auto-failover omits the official provider, that official login can match a genuine official slug through another entry's `upstreamModel`, and that the 240-second refresher can keep an old sidecar baseline fresh indefinitely.

- [ ] **Step 2: Resolve mappings before failover selection**

When the configured current provider is Codex official, resolve exact custom slots before calling `ProviderRouter`. Route mapped slots only to their bound provider and reject unmapped aggregate-mode slots before unrelated failover providers are considered.

- [ ] **Step 3: Separate official and aggregate alias semantics**

Keep exact public-slot matching in both modes, but allow legacy `upstreamModel` alias matching only when official login is disabled.

- [ ] **Step 4: Give clean baselines a non-renewable TTL**

Record the capture time of a distinct official fingerprint. Do not extend it when the same snapshot is observed again. At 300 seconds, transition to `awaiting_official_refresh` and remove the rendered live cache; accept a later distinct official snapshot as the new baseline. Treat a missing time as a legacy sidecar that needs one-time initialization, while malformed/non-string/abnormally future times fail closed into refresh. Test 299/300/301-second boundaries with a fixed clock input.

- [ ] **Step 5: Run focused, full, and independent verification**

Run the routing/provider/cache test groups, the complete Rust suite, formatting, diff validation, and clippy with warnings denied. Then request a new independent white-box review across routing order, alias semantics, TTL state transitions, and the earlier cache/concurrency fixes.
