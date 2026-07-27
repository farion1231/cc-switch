# Unified Codex Session History

## What it does

Codex stores a provider and model on each saved session. Those values can override
the current `config.toml` when an old session is resumed. Without coordination, an
old official session can still call OpenAI after CC Switch selects a third-party
provider, or an old third-party session can reference a route that is no longer in
the live configuration.

When **Unified Codex session history** is enabled, CC Switch rebinds saved sessions
to the provider you select on every switch. This works in both directions:

- official to third-party;
- third-party to official;
- one third-party provider to another.

Official traffic continues to use Codex's built-in `openai` provider. Third-party
traffic continues to use its configured provider ID. The feature does not disguise
official traffic as `custom`.

## Enable it

1. Open **Settings > Codex App Enhancements**.
2. Enable **Unified Codex session history**.
3. Optionally select **Update all existing saved sessions now**.

The optional checkbox immediately aligns existing sessions with the current
provider. Even when it is not selected, later provider switches rebind saved
sessions while the feature remains enabled.

After switching providers, reopen sessions that were already running. CC Switch can
update persisted files and indexes, but it cannot replace routing already held in a
running Codex process.

## Disable it

Disabling the option stops future provider switches from rebinding saved sessions.
Existing sessions stay assigned to the last selected provider.

Older CC Switch releases may have created an exact-restore backup under
`~/.cc-switch/backups/codex-official-history-unify-v1/`. When such a legacy backup
exists, the disable dialog offers a compatibility restore option. The current
transactional reconciler does not create migration backups.

## Credential safety

CC Switch keeps credentials attached to their own routes:

- ChatGPT OAuth remains in `auth.json`;
- a third-party API key is stored only as that provider table's scoped bearer token;
- a string API key is removed from global `auth.json` while a third-party route is
  active;
- valid inactive provider definitions are retained so an already-open session does
  not fail with `Model provider ... not found` during a switch.

This prevents a third-party key from being sent to `api.openai.com` and prevents an
official OAuth credential from being treated as a third-party key.

## Data safety

The reconciler updates only these routing fields:

- `session_meta.payload.model_provider` in active and archived JSONL sessions;
- `thread_settings_applied` provider and model fields;
- `threads.model_provider` and, when configured, `threads.model` in every discovered
  Codex state database.

Before writing, every affected file and database is validated and an in-memory
change journal is prepared. JSONL writes are atomic and retain their original
timestamps. SQLite updates use transactions and expected old values. If any step
fails or a concurrent change is detected, applied history changes and live files are
rolled back before the switch returns an error.

No conversation messages, response items, or encrypted reasoning payloads are
deleted or rewritten.

## State database locations

CC Switch scans every `state_*.sqlite` in:

- the Codex config directory;
- its nested `sqlite/` directory;
- `sqlite_home` configured in `config.toml`;
- `CODEX_SQLITE_HOME`.

Paths are canonicalized and deduplicated, so version changes such as
`state_5.sqlite` to `state_6.sqlite` do not hide part of the history.

## Known limitation

`encrypted_content` may be backend-specific. Rebinding makes a session visible and
routes its next request through the selected provider, but another backend may still
be unable to decrypt reasoning content created by the original backend. CC Switch
does not and cannot transform that encrypted content.
