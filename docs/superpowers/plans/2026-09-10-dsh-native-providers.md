# DSH Native Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display and manage DSH 2.0.5 native providers in CC Switch with Pi-style synchronization and safe source-aware writes.

**Architecture:** Add a structural DSH native adapter that owns settings and credentials, then a provider service layer that mirrors native routes into the existing provider database. Keep UI behavior in the shared provider page while adding only DSH-specific state and form components where its schema differs from Pi.

**Tech Stack:** Rust, Tauri 2, yaml-rust, SQLite, React, TypeScript, TanStack Query, Vitest.

---

### Task 1: Native DSH document model

**Files:**
- Modify: `src-tauri/src/deepseek_harness_config.rs`

- [ ] Add failing tests for version-1 credentials, `llm-deepseek`, `llm-pi-ai.providers`, and `agent-default-model` parsing.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness_config --lib` and confirm the new tests fail.
- [ ] Implement bounded structural readers and source-aware atomic writers that preserve unrelated YAML and credential records.
- [ ] Run the focused test and confirm all DSH config tests pass.
- [ ] Commit as `feat(deepseek): model native Harness providers`.

### Task 2: Pi-style DSH provider service

**Files:**
- Create: `src-tauri/src/services/provider/deepseek_harness.rs`
- Modify: `src-tauri/src/services/provider/mod.rs`
- Modify: `src-tauri/src/services/provider/live.rs`
- Modify: `src-tauri/src/provider.rs`

- [ ] Add failing service tests for native import/list, official/custom route ownership, current provider, switching model, add/update/delete, and rollback behavior.
- [ ] Run the focused service tests and confirm expected failures.
- [ ] Implement the dedicated service and route all DSH lifecycle operations through it.
- [ ] Run focused DSH service/config tests and confirm they pass.
- [ ] Commit as `feat(deepseek): sync Harness native providers`.

### Task 3: Commands and startup synchronization

**Files:**
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/api/providers.ts`

- [ ] Add failing command tests for explicit DSH import and current-state response.
- [ ] Implement `import_deepseek_harness_providers_from_live` and startup sync alongside Pi.
- [ ] Register the Tauri commands and TypeScript API.
- [ ] Run command and provider integration tests.
- [ ] Commit as `feat(deepseek): import Harness live providers`.

### Task 4: Provider page correctness

**Files:**
- Modify: `src/config/appConfig.tsx`
- Modify: `src/components/providers/ProviderList.tsx`
- Modify: `src/components/providers/ProviderCard.tsx`
- Modify: `src/components/providers/ProviderEmptyState.tsx`
- Modify: `tests/config/appConfig.test.tsx`
- Modify: `tests/components/ProviderList.test.tsx`
- Modify: `tests/components/ProviderCardLayout.test.ts`

- [ ] Add failing tests proving DSH is exclusive, imports through its dedicated API, displays `baseURL`, and marks the native current provider.
- [ ] Implement the minimal shared-page changes.
- [ ] Run the focused Vitest files and confirm they pass.
- [ ] Commit as `fix(deepseek): show native providers on the main page`.

### Task 5: DSH provider form

**Files:**
- Create: `src/components/providers/forms/DeepSeekHarnessProviderForm.tsx`
- Modify: `src/components/providers/forms/ProviderForm.tsx`
- Modify: `src/config/deepseekHarnessProviderPresets.ts`
- Modify: `tests/components/DeepSeekHarnessProviderForm.test.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/ja.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`

- [ ] Add failing form tests for route id, API format, base URL, credential reference, API key, model list, and edit round-trip.
- [ ] Implement the DSH form using Pi's stable structured-field layout patterns.
- [ ] Run focused form and locale tests.
- [ ] Commit as `feat(deepseek): add structured Harness provider form`.

### Task 6: Local migration and end-to-end verification

**Files:**
- Modify only through application/native adapters: `~/.cc-switch/cc-switch.db`, `~/.dsh/settings.yaml`, `~/.dsh/.credentials.yaml`
- Build output: `outputs/CC-Switch-3.20.2-DSH-arm64.dmg`

- [ ] Back up the three local state files without recording secret contents.
- [ ] Run the DSH importer and confirm four cards appear: `deepseek-official`, `k3`, `glm-5-3`, and `yuzuvalley`.
- [ ] Confirm `yuzuvalley/deepseek-v4-pro` is current and switching round-trips in DSH Desktop.
- [ ] Run `pnpm typecheck`, `pnpm format:check`, focused Vitest, full Vitest, `cargo fmt --all --check`, focused Rust tests, and full `cargo test`.
- [ ] Build with the local macOS strip workaround, install with backup/rollback, and visually verify the DSH page.
- [ ] Record exact evidence and any remaining upstream-only warnings.
