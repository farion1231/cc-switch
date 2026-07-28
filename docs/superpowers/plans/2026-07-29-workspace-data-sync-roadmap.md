# Workspace Data Sync Roadmap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver encrypted, incremental, lossless-first backup and multi-device merge for Claude Code, Codex, Grok Build, OpenCode, and Cursor without regressing the existing `db.sql + skills.zip` sync path.

**Architecture:** Build a new `workspace_sync` subsystem beside the existing sync services. Execute the work as six independently testable plans: core protocol, provider inventory and backup, file-provider merge, SQLite-provider merge, Cursor write-back, and productization/hardening.

**Tech Stack:** Rust 1.85, Tauri 2, rusqlite, reqwest, WebDAV, S3 SigV4, Argon2id, XChaCha20-Poly1305, React 18, TypeScript, TanStack Query, Vitest.

---

## Plan Set and Dependency Order

- [ ] **Plan 1: Core protocol and storage**
  - File: `docs/superpowers/plans/2026-07-29-workspace-sync-01-core-protocol.md`
  - Produces: core models, local sync database, object-storage abstraction, encryption, immutable snapshots, compare-and-swap Head updates.
  - Acceptance: an in-memory two-device test can upload encrypted blobs and reject a stale Head update.

- [ ] **Plan 2: Provider inventory and encrypted backup**
  - File: `docs/superpowers/plans/2026-07-29-workspace-sync-02-provider-backup.md`
  - Depends on Plan 1.
  - Produces: read-only adapters for Claude, Codex, Grok Build, OpenCode, and Cursor; backup preview; encrypted incremental upload; minimal commands and frontend API.
  - Acceptance: fixture data for all five providers produces a decryptable remote snapshot with credentials excluded.

- [ ] **Plan 3: File-provider merge**
  - File: `docs/superpowers/plans/2026-07-29-workspace-sync-03-file-merge.md`
  - Depends on Plans 1-2.
  - Produces: three-way merge, tombstones, restore points, Claude and Grok Build write-back, conflict copies.
  - Acceptance: two devices can branch the same Claude/Grok session and both branches survive locally and remotely.

- [ ] **Plan 4: SQLite-provider merge**
  - File: `docs/superpowers/plans/2026-07-29-workspace-sync-04-sqlite-merge.md`
  - Depends on Plans 1-3.
  - Produces: SQLite schema adapters, Codex and OpenCode logical record merge, ID rewriting, integrity checks, rollback.
  - Acceptance: Codex/OpenCode fixture databases merge without broken foreign keys or missing records.

- [ ] **Plan 5: Cursor Session Manager and write-back**
  - File: `docs/superpowers/plans/2026-07-29-workspace-sync-05-cursor.md`
  - Depends on Plans 1-4.
  - Produces: Cursor session parsing, schema registry, referenced-blob extraction, known-schema Composer fork/write-back, unknown-schema read-only fallback.
  - Acceptance: known Cursor fixtures can fork and restore a Composer; unknown fixtures remain viewable but cannot be written.

- [ ] **Plan 6: UI, automation, retention, and hardening**
  - File: `docs/superpowers/plans/2026-07-29-workspace-sync-06-productization.md`
  - Depends on Plans 1-5.
  - Produces: full settings page, conflict center, snapshot history, device manager, scheduled backup, exit backup, GC, security/performance tests, release flag.
  - Acceptance: all existing CI plus new cross-provider, multi-device, crash-recovery, and security suites pass.

## Release Gates

- [ ] **Alpha gate:** Plans 1-2 complete; backup-only feature behind `workspaceSync.experimental`.
- [ ] **Beta 1 gate:** Plan 3 complete; Claude/Grok write-back enabled per provider.
- [ ] **Beta 2 gate:** Plan 4 complete; Codex/OpenCode write-back enabled for recognized schemas.
- [ ] **RC gate:** Plan 5 complete; Cursor known-schema write-back and conflict center available.
- [ ] **Stable gate:** Plan 6 complete; scheduled backup, retention, GC, localization, and security review complete.

## Global Rules for Every Plan

- [ ] Start each implementation task with a failing unit or integration test.
- [ ] Run the narrow test and verify the expected failure before implementation.
- [ ] Add the minimal implementation needed for the test.
- [ ] Run the narrow test, then the affected package suite.
- [ ] Commit each coherent task separately.
- [ ] Never include credentials, raw sync passwords, session contents, or absolute project paths in logs.
- [ ] Never make unknown provider schemas writable by default.
- [ ] Never update remote Head without ETag/CAS protection or the documented compatibility lock fallback.
- [ ] Never delete data during merge unless a valid Tombstone is newer and no delete/modify conflict exists.
- [ ] Keep existing `src-tauri/src/services/webdav_sync.rs` and `src-tauri/src/services/s3_sync.rs` behavior backward compatible.
