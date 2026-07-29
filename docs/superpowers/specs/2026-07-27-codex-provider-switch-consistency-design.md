# Codex Provider Switch Consistency Design

## Goal

Make official and third-party Codex provider switches safe in both directions. When
"Unified Codex session history" is enabled, every persisted thread resumes through
the provider currently selected in CC Switch. When it is disabled, legacy per-thread
provider ownership remains usable.

## Root cause

Codex persists `model_provider` and `model` independently from its live
`config.toml`. On resume, state SQLite metadata overrides the current live defaults.
CC Switch currently writes provider selection, live config, credentials, JSONL, and
SQLite through separate one-way operations. A partial switch can therefore combine:

- an `openai` thread with a third-party key in global `auth.json`, sending that key to
  `api.openai.com` and receiving 401;
- a `custom` thread with no `[model_providers.custom]` definition;
- a `custom` thread with official OAuth, risking an official credential being sent to
  a third-party endpoint;
- UI/database state for the new provider with live files for the old provider.

The existing official-to-`custom` migration is incomplete because it only rewrites
`session_meta`, discovers a fixed `state_5.sqlite` subset, changes JSONL mtimes, and
uses the `custom` route for official traffic.

## Required invariants

1. Unified mode keeps one stable `custom` history identity in every live route.
2. Official traffic keeps native OpenAI auth/capabilities behind an OpenAI-shaped
   `custom` provider; third-party traffic replaces only that provider's route.
3. Global `auth.json` never receives a third-party API key when resumable `openai`
   threads may exist. Third-party credentials are scoped to their provider table.
4. Valid inactive provider definitions remain available while persisted or
   already-loaded threads may still reference them.
5. With unified history enabled, persisted provider and model metadata equal the
   selected target before CC Switch publishes the new current-provider state.
6. A failed switch restores live files, history metadata, and current-provider state.
7. JSONL content outside provider/model fields and original file timestamps are
   preserved.

## Selected approach

Use the existing unified-history toggle as an explicit current-provider-follow mode.
Normalize official, third-party, direct, and proxy routes to one stable `custom`
identity. Reconcile legacy history into that bucket once; later switches change only
the route and credentials behind it, so restarting Codex cannot re-filter the same
thread into an obsolete provider bucket.

Alternatives rejected:

- Per-switch provider rewrites cannot safely replace an append-only JSONL that Codex
  keeps open; future appends would continue on the replaced file handle.
- Proxy-only interception does not cover direct switching, cannot safely represent
  every provider format, and still leaves persisted metadata inconsistent.

## Components

### Safe live configuration

`codex_config` prepares a target snapshot before any state is committed:

- official target: `model_provider = "custom"`, an OpenAI-shaped provider table, and
  native official auth in `auth.json`;
- third-party target: `model_provider = "custom"`, with the selected provider table
  projected into that bucket and its API key stored only as a scoped bearer token;
- complete inactive provider tables are retained when switching;
- an incomplete placeholder such as only `name = "OpenAI"` is replaced by the last
  complete custom route instead of blocking preservation;
- proxy takeover routes and committed restore configurations use the same stable
  bucket; the initial takeover backup remains a verbatim pre-transaction rollback
  snapshot.

### History reconciliation

A focused history reconciler operates on both `sessions` and `archived_sessions`.
It updates only recognized fields needed for discovery and resume:

- `session_meta.payload.model_provider`;
- the latest `event_msg` / `thread_settings_applied` provider and model settings;
- `threads.model_provider` and `threads.model` in every
  discovered state database.

Database discovery scans every `state_*.sqlite` in:

- the Codex config directory;
- `<CODEX_HOME>/sqlite`;
- `sqlite_home` from `config.toml`;
- `CODEX_SQLITE_HOME`.

Paths are canonicalized and deduplicated. Unknown JSONL event shapes and unrelated
SQLite tables are left untouched.

### Transaction journal

Before writing, the reconciler parses every target JSONL file and state database and
builds an in-memory change journal containing the exact original fields/content. No
artifact is touched until all inputs validate. Equal-length provider tokens such as
`openai` → `custom` are patched at fixed offsets, preserving an already-open append
handle. Provider/model changes for the latest thread settings are recorded by
appending a new valid settings event, matching Codex's append-only rollout format and
avoiding a variable-length rewrite during ordinary switches. Legacy variable-length
session metadata uses atomic replacement and fails closed on Windows when the file is
open and on Unix/macOS while a Codex process is running. Later failures roll back in
reverse order; if Codex appends a target-route event concurrently, rollback appends
the original route again so the final effective event matches the restored Live state.

### Coordinated switch

Codex switches run in this order, with the history lock held from reconciliation
through current-provider publication:

1. validate the target provider and build its complete live snapshot in memory;
2. capture current-provider settings/database values and live files;
3. write a safe live state whose stable route contains only the target credentials;
4. if unified mode is enabled, reconcile legacy JSONL and all state databases to the
   stable bucket;
5. publish the target as current in local settings and the CC Switch database;
6. re-project Codex MCP configuration and return success.

If steps 3-5 fail, restore changed history from the in-memory journal, restore live
files, and restore the local current-provider value. The database current-provider
update is itself transactional and is published last. Rollback failure is reported
together with the original error.

Backfill of the outgoing provider remains best-effort but occurs before the switch
snapshot. It cannot change routing, credentials, or the transaction commit point.

## Toggle behavior

- Enabling can immediately reconcile existing history to the currently selected
  Codex provider when the user selects that option.
- While enabled, every successful switch verifies all resumable threads remain in the
  stable bucket while replacing that bucket's target route.
- Disabling transactionally rebuilds the current direct or takeover route without the
  stable bucket before the setting save returns. It stops future rebinding; current
  tags remain usable. Exact-restore backups created by older releases remain
  available for backward compatibility.
- Legacy settings fields are migrated without silently enabling the mode.

## Active Codex processes

The safe route snapshot prevents credential crossover during the switch. Common
`openai`/`custom` history is patched without replacing the file held by Codex; a rare
variable-length legacy migration may require closing Codex once. The UI reports that
an already-open thread may need to be reopened; CC Switch never terminates Codex.

## Error handling

- Malformed target TOML fails before any mutation.
- Concurrent JSONL modification aborts that switch and triggers rollback.
- Locked or incompatible SQLite databases fail without a completion marker.
- Missing history directories or databases are valid no-op inputs.
- No API key, token, or full auth object is written to logs or the ledger.

## Verification

Test-driven coverage must include:

- official to third-party and third-party to official;
- stale `openai`, stale `custom`, and legacy provider IDs;
- provider plus model reconciliation in both JSONL event forms and SQLite;
- root, `sqlite/`, configured, environment, and multiple-version state databases;
- incomplete custom placeholders and complete inactive custom preservation;
- official OAuth and third-party key isolation;
- JSONL timestamp preservation;
- switch failure rollback at live, JSONL, SQLite, settings, and database boundaries;
- enable, repeated switches, disable, and transactional rollback;
- formatting, Clippy with warnings denied, Rust tests, frontend typecheck/tests, release
  compilation, and Windows installer generation.

## Review amendments (2026-07-29)

The coordinated-switch guarantees apply equally to direct switches and proxy
takeover hot switches. A Codex hot switch must reconcile history after its live and
backup preparation succeeds but before publishing local or database
current-provider state. The applied history journal participates in every later
rollback path.

When retaining the currently live third-party route during an upgrade, an existing
provider-scoped `experimental_bearer_token` is authoritative. A global
`OPENAI_API_KEY` may populate the scoped token only when the active provider table
does not already have one. Target-provider writes retain their existing
stored-auth-first behavior.

JSONL replacement on Windows must not remove the destination before the replacement
is ready. The shared atomic-write primitive will use a same-directory Windows atomic
replace operation. History rollback also recreates the journaled original if an
interrupted external or legacy write leaves the destination absent.

Proxy activation failure is distinct from normal proxy stop or crash recovery. If
takeover configuration or server startup fails before activation commits, every app
is restored directly from its raw pre-transaction backup and no unified Codex
projection or history reconciliation is performed. Cleanup and rollback failures are
reported to the caller while backups remain available for recovery. A proxy that was
prestarted for an ephemeral port remains running when rollback fails, so a remaining
localhost Live route stays usable. Bulk takeover holds every per-app switch lock for
the full transaction and rejects an already active per-app takeover without mutation.
Normal stop and crash recovery continue to use the logical unified-history restore
path.

Regression coverage must include proxy takeover in both directions and rollback,
upgrade state containing an official global API key plus a third-party scoped token,
Windows replacement/absent-destination recovery, and verbatim rollback after bulk
takeover activation failure, including existing per-app ownership and prestarted
proxy liveness.

## Non-goals

- Decrypting backend-specific `encrypted_content` across providers.
- Mutating in-memory threads inside a running Codex process.
- Changing proxy routing semantics unrelated to provider switching.
