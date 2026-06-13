# Codex Session Sharing And Usage Design

## Context

CC Switch already scans Codex conversation JSONL files from the active Codex config directory:

- `sessions`
- `archived_sessions`

It also imports token usage from Codex JSONL logs into `proxy_request_logs` with `data_source = "codex_session"` and a `session_id`. The missing pieces are UI/API surfaces for querying usage by conversation and a safe way to make Codex conversations visible across CCS-managed Codex providers.

Codex provider switching can make history appear to disappear because Codex buckets conversations by provider metadata such as `model_provider`, and in some versions also by records in `state_5.sqlite`. The files are usually still present.

## Goals

1. Add a lightweight Codex conversation window under each Codex provider.
2. Add a session-based usage query so users can see token/cost totals for each Codex conversation under the current provider.
3. Support sharing a conversation to all Codex providers or to a checked subset of providers.
4. Make shared conversations visible in the Codex client itself, not only inside CC Switch.
5. Avoid moving or copying the original JSONL conversation files.
6. Back up any Codex-owned file or state database before modifying it.

## Non-Goals

- Do not create OS-level symbolic links for JSONL files.
- Do not duplicate conversation JSONL files per provider.
- Do not rewrite API keys or auth data as part of conversation sharing.
- Do not attempt to support non-Codex providers in this feature.

## Recommended Approach

Use a hybrid design:

- CC Switch owns a lightweight logical binding index.
- A separate sync operation materializes those bindings into Codex-visible provider buckets.

The logical binding index gives CCS a stable UI and avoids unnecessary writes. The materialization step is what lets the Codex client see the same conversation after switching providers.

## User Experience

### Provider Entry

Each Codex provider gains a new action next to existing provider actions such as enable, edit, usage, and terminal:

- `Codex sessions`

Opening it shows a provider-scoped conversation window.

### Conversation Window

The window shows:

- Conversation title
- Project directory
- Last active time
- Source file path
- Current visibility status
- Token totals
- Cost total
- Model breakdown when available
- Resume command

Actions:

- `Share to all Codex providers`
- `Manage providers`
- `Sync visibility to Codex`
- `Copy resume command`
- `Open usage details`
- `Remove sharing link`

Deleting keeps the current Session Manager semantics separate:

- Removing a sharing link only removes the CCS binding and optionally re-syncs Codex visibility.
- Deleting the source conversation remains an explicit destructive action.

### Usage Query

Provider usage gets a new query mode:

- `By conversation`

For Codex providers, this mode lists sessions visible to the current provider and aggregates usage from `proxy_request_logs.session_id`.

## Data Model

Add a CCS-owned table:

```sql
CREATE TABLE codex_session_provider_links (
  session_id TEXT NOT NULL,
  source_path TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  link_mode TEXT NOT NULL DEFAULT 'manual',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, source_path, provider_id)
);
```

`link_mode` values:

- `manual`: user selected this provider.
- `all`: created by "share to all Codex providers".
- `native`: optional cached marker for provider visibility discovered from Codex metadata.

This table is CCS metadata only. It does not replace Codex JSONL or Codex state.

## Provider Bucket Rules

The first implementation should prefer a shared third-party bucket:

- Official OpenAI provider keeps its official bucket.
- CCS-managed third-party/aggregator providers share `model_provider = "custom"` where possible.

This aligns with existing migration code that already normalizes known third-party history into `custom`.

When the user shares a conversation:

1. CCS records links in `codex_session_provider_links`.
2. CCS determines each target provider's effective Codex model provider id.
3. CCS updates Codex visibility metadata so the conversation is visible from the target provider.

## Codex Visibility Sync

The sync operation should be explicit and safe.

Inputs:

- Session id
- Source JSONL path
- Target provider ids

Steps:

1. Validate that the JSONL path is under the configured Codex `sessions` or `archived_sessions` directory.
2. Read the session metadata and current provider bucket.
3. Back up the JSONL file before changing it.
4. Back up `state_5.sqlite` before changing it.
5. Update `session_meta.payload.model_provider` when needed.
6. Update matching `threads.model_provider` rows in `state_5.sqlite` when the table and column exist.
7. Store a sync result with changed files, changed rows, skipped files, and warnings.

If Codex does not support one thread belonging to multiple provider ids at the same time, the first version should use the shared `custom` bucket rather than attempting to duplicate rows.

## Token Usage Flow

Existing Codex usage import already parses JSONL token events and inserts rows into `proxy_request_logs`.

Changes:

1. Expose `session_id` in request log DTOs.
2. Add `session_id` to usage filters.
3. Add a session summary query grouped by `session_id`.
4. Join the session summary with Codex session metadata for titles and project paths.
5. Add UI for provider-scoped `By conversation` results.

The provider-scoped usage result should include:

- total input tokens
- total output tokens
- cache read tokens
- cache creation tokens when available
- total cost
- request count
- first/last usage timestamps
- model breakdown

## Error Handling

- If a JSONL file changed while CCS is rewriting it, abort and report a retryable conflict.
- If `state_5.sqlite` is locked, report that Codex may be running and ask the user to close it or retry later.
- If a provider has no resolvable Codex model provider id, keep the CCS binding but mark Codex visibility sync as skipped.
- If backup fails, do not modify Codex files.
- If a sync partially succeeds, show which target providers were updated and which need retry.

## Safety

- Never modify `auth.json` for this feature.
- Never write API keys.
- Never follow arbitrary source paths outside Codex session roots.
- Backups must be stored under the CC Switch app backup directory with timestamped names.
- All destructive source-session deletion remains opt-in and separate from unlinking.

## Tests

Backend tests:

- Parse Codex session metadata with and without `model_provider`.
- Link one session to all providers.
- Link one session to a selected provider subset.
- Reject source paths outside Codex session roots.
- Back up and rewrite JSONL `session_meta.model_provider`.
- Back up and update `state_5.sqlite.threads.model_provider`.
- Skip state DB update when table or column is missing.
- Add usage filtering by `session_id`.
- Aggregate usage by session.

Frontend tests:

- Provider action opens the Codex sessions window.
- Provider checklist creates expected link requests.
- "Share to all" selects all eligible Codex providers.
- By-conversation usage mode renders totals and empty states.
- Removing a link does not call source-session deletion.

## Rollout

Phase 1:

- Add backend usage filtering and session summary by `session_id`.
- Add provider-scoped Codex sessions window.
- Add CCS logical binding table.

Phase 2:

- Add "share to all" and provider checklist bindings.
- Add explicit Codex visibility sync with backups.

Phase 3:

- Add richer diagnostics for provider buckets and history repair.
- Consider automatic sync on provider switch if manual sync proves reliable.

