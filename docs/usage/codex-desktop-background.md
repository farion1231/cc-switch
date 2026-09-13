# Codex Desktop background usage (macOS)

Codex Desktop can report `ephemeral_generation_token_usage` events for background
features such as `thread_title`, `ambient_suggestions`, and
`ambient_suggestion_safety`. These generations can be absent from the session
JSONL files used by the existing Codex importer.

With session auto-sync enabled, CC Switch reads retained `.log` files beneath
`~/Library/Logs/com.openai.codex/YYYY/MM/DD/`. Manual session sync uses the same
importer. Files with unchanged size and modification time are skipped; changed
files are scanned again, and duplicate event fingerprints are ignored. Complete
newline-terminated records are required. Truncated or invalid records are not
estimated. An absent log directory imports nothing. Windows and Linux log
locations are not implemented.

The usage dashboard shows a separate **Codex Desktop background usage** table
when the application filter is All or Codex and no provider is selected. It
respects the date range and model filter, groups by model, feature and status,
and hides when no matching records are present. The model picker includes models
found only in background events, even when no request/session records use them. Events with reported usage are
included even when the generation status is not `success`.

## Accounting boundaries

- These are generation observations, not HTTP request counts. One event may
  aggregate several model calls.
- They are stored separately and **excluded from existing request totals,
  charts, model/provider statistics and request logs**. Desktop events lack a
  stable upstream request ID, so overlap with proxy traffic cannot be resolved
  safely. Do not add both sources together as a reconciled total.
- Fingerprints use the normalized timestamp and sorted event fields, not file
  paths. Copies and rotated copies of identical records therefore collapse.
  Two actual generations with completely identical timestamps and fields
  cannot be distinguished and would also collapse. This is record-level
  deduplication, not proof of request identity.
- Input tokens already include cached input. Reasoning output is already part
  of output. Neither is added a second time.
- Costs use the current configured model prices and the existing Codex cache
  semantics, with a multiplier of one. Unknown prices remain unknown. These
  are standard-rate estimates, not actual ChatGPT subscription charges or
  service-tier/long-context adjustments.
- Coverage is local to this machine and retained logs, not account-wide.
  Imported observations survive source log deletion. This first version does
  not provide a background-ledger purge/rebuild control.
- WebDAV/S3 exports exclude background rows and file metadata. Sync imports
  preserve the local ledger, including when an older snapshot has no background
  tables. Full manual backups still include and restore these records.
- Only structured usage fields and hashes are stored, plus local file metadata
  for scan caching. Prompts and unrelated log text are not stored.

## Validation

Synthetic Rust tests cover count validation, overflow, repeated sync, copied
logs, appends, partial lines, truncation, invalid UTF-8, oversized lines, unknown
prices, status grouping, date/model filtering, and isolation from request totals.
A schema test exercises the v18-to-v19 migration. Component tests check filtering,
unknown prices, and query failures.

For an explicitly supplied local fixture directory (never the installed CC Switch
DB), an optional test imports into an in-memory database, repeats the import,
and prints only grouped usage:

```sh
CC_SWITCH_BACKGROUND_LOG_FIXTURE=/absolute/path/to/fixture \
  cargo test --manifest-path src-tauri/Cargo.toml --lib \
  codex_background_usage::tests::import_local_log_fixture -- --ignored --nocapture
```

Do not commit personal logs, credentials, or configuration files with a report.
