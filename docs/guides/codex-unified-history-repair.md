# Codex unified history and official proxy takeover

The history bucket and the temporary proxy route are different concerns.
With **Unify Codex session history** enabled, both direct official routing and
official proxy takeover use the shared `custom` provider ID. With it disabled,
official takeover keeps the legacy `cc-switch-official` ID. Authentication still
uses native OpenAI login (`requires_openai_auth = true`); local takeover uses
Responses HTTP/SSE, not WebSockets.

## Existing sessions

The previous official-history migration only handled `openai` metadata. A
session created during official takeover could retain `cc-switch-official` in
both its metadata and persisted `thread_settings_applied` events. Removing the
temporary provider then caused resume to fail with "Model provider
`cc-switch-official` not found", even when the session appeared in history.

The version-2 migration covers both official IDs and updates:

- JSONL `session_meta.payload.model_provider`;
- matching `event_msg` / `thread_settings_applied` events' provider IDs;
- SQLite `threads.model_provider`.

It also repairs stale runtime settings in sessions whose metadata is already
`custom`. Runtime edits are scoped to an eligible session segment; an explicit
foreign thread ID is never rewritten. Message bodies and `encrypted_content`
are not modified. Rollout modification times are preserved.

Migration remains opt-in. Existing version-1 markers do not suppress this
upgrade; successful version-2 markers remain bound to the Codex directory.

1. Save work and quit Codex/ChatGPT desktop and all Codex CLI processes.
2. In CC Switch, enable unified history and opt in to migrating existing history.
3. Ensure the active Codex configuration uses the shared bucket. Select the
   official provider again if an old takeover configuration is still active.
4. Click the history migration/retry button in Settings. If writers were open
   during the automatic attempt, no files were changed by that attempt.
5. Reopen Codex and resume the old session.

Do not launch a Codex writer while migration or restore is running. The process
check is a conservative preflight, not an OS-level lock against new processes.
Renamed executables or remote writers cannot be identified by this check.

Backups and their source-directory metadata are recorded before mutations.
When disabling unification, optional restore changes only sessions identified
by the official backup ledger. Both original official IDs restore to the
usable built-in `openai` ID, **not** to a removed temporary proxy route. New
shared-bucket sessions without official ledger evidence are left unchanged.
Quit writers before restore too. If restore fails, retain the backups and retry
the enable/disable flow after resolving the error.

## Takeover ownership

`custom` alone is not evidence that a provider belongs to CC Switch. Official
takeover writes a TOML comment containing a hash of the provider ID and proxy
URL. Recognition additionally checks the exact official transport/auth table.
The comment is saved atomically with the config and its rollback snapshots;
it adds no unsupported Codex keys or credentials. An edited URL, extra
credential, or absent marker prevents shared-table ownership recognition.

History-setting changes reproject an active official takeover under the provider
switch lock and roll back the live configuration and backup on projection
failure. Re-enabling an existing legacy takeover also reconciles its bucket.

## Scope and limits

This fixes official takeover bucket divergence, including issue #5974. It is
not a general importer for arbitrary third-party provider IDs or other machines.
Seeing the same history does not guarantee cross-backend continuation: an
upstream may reject reasoning content encrypted by a different backend. This
change neither decrypts nor strips that content.

Tests use synthetic fixtures and temporary directories; no real credentials or
conversation contents are required. The regression coverage includes unified
and non-unified takeover, hot switching, toggle reconciliation, conservative
ownership detection, version-1 upgrades, runtime migration/ledger restore,
mtime preservation, and the explicit retry UI.
