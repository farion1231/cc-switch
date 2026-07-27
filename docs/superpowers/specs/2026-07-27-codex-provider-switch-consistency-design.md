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

1. Official live traffic uses Codex's built-in `openai` provider.
2. Third-party live traffic uses its configured model-provider ID.
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
Keep real route identities (`openai` and `custom`) and reconcile persisted history on
each switch. Do not disguise official traffic as `custom` and do not require proxy
takeover.

Alternatives rejected:

- A permanent shared `custom` identity breaks built-in official capabilities such as
  Responses Lite and reserved-provider behavior.
- Proxy-only interception does not cover direct switching, cannot safely represent
  every provider format, and still leaves persisted metadata inconsistent.

## Components

### Safe live configuration

`codex_config` prepares a target snapshot before any state is committed:

- official target: `model_provider = "openai"`, native official auth in `auth.json`;
- third-party target: its configured `model_provider`, with the API key stored only
  as that provider table's scoped bearer token;
- complete inactive provider tables are retained when switching;
- an incomplete placeholder such as only `name = "OpenAI"` is replaced by the last
  complete custom route instead of blocking preservation;
- official-as-custom unified-session injection is removed.

### History reconciliation

A focused history reconciler operates on both `sessions` and `archived_sessions`.
It updates only recognized fields:

- `session_meta.payload.model_provider`;
- `event_msg` / `thread_settings_applied` provider and model fields;
- `threads.model_provider` and, when the target defines one, `threads.model` in every
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
artifact is touched until all inputs validate. If a later write fails, applied
databases and files are restored in reverse order. Concurrent changes are detected
with file metadata and row-value predicates rather than overwritten.

### Coordinated switch

Codex switches run in this order, with the history lock held from reconciliation
through current-provider publication:

1. validate the target provider and build its complete live snapshot in memory;
2. capture current-provider settings/database values and live files;
3. write a dual-safe live state in which both `openai` and retained `custom` routes
   have only their own credentials;
4. if unified mode is enabled, reconcile JSONL and all state databases to the target;
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
- While enabled, every successful switch reconciles all resumable threads to the new
  target.
- Disabling stops future rebinding; current tags remain usable. Exact-restore backups
  created by older releases remain available for backward compatibility.
- Legacy settings fields are migrated without silently enabling the mode.

## Active Codex processes

CC Switch cannot replace a client already held in another process's memory. The safe
dual-route snapshot prevents credential crossover during the switch, while persisted
threads are correct on reopen. The UI reports that an already-open thread may need to
be reopened; CC Switch does not terminate Codex automatically.

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

## Non-goals

- Decrypting backend-specific `encrypted_content` across providers.
- Mutating in-memory threads inside a running Codex process.
- Changing proxy routing semantics unrelated to provider switching.
