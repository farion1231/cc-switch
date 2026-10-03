# Codex fork usage fixtures

Synthetic, sanitized JSONL: fixed UUIDs, model identifiers, timestamps and
token counters only. No user messages, instructions, filesystem paths,
account identifiers, provider configuration or credentials are included.

The reference-backed format is derived from OpenAI Codex revision
`bcd6d9ab6b9f26f85d76d0c680b3f88b367bffa0`:

- [HistoryPosition and SessionMeta](https://github.com/openai/codex/blob/bcd6d9ab6b9f26f85d76d0c680b3f88b367bffa0/codex-rs/protocol/src/protocol.rs)
- [Fork reference preparation](https://github.com/openai/codex/blob/bcd6d9ab6b9f26f85d76d0c680b3f88b367bffa0/codex-rs/thread-store/src/local/paginated_fork.rs)
- [Revert creates a replacement rollout](https://github.com/openai/codex/blob/bcd6d9ab6b9f26f85d76d0c680b3f88b367bffa0/codex-rs/thread-store/src/local/revert_thread.rs)
- [Fork usage initialization and persistence](https://github.com/openai/codex/blob/bcd6d9ab6b9f26f85d76d0c680b3f88b367bffa0/codex-rs/core/src/session/mod.rs)

`history_base.thread_id` identifies a **physical rollout**, which can differ
from the logical `forked_from_id`. `end_byte_offset` and
`end_ordinal_exclusive` describe its inherited prefix. Ancestor references
must be followed rather than concatenating every file with the logical ID.

| File | Inherited context | New input / cached / output |
| --- | --- | --- |
| parent | none | A: 1000 / 400 / 100; B: 500 / 200 / 50 |
| replacement | A, not B | C: 300 / 100 / 30 |
| child-a | A and C | D: 200 / 100 / 20 |
| child-b | A | E: 400 / 100 / 40 |
| grandchild | A, C and D | G: 100 / 50 / 10 |

The parent's B request is still real consumption even though revert excludes
it from the replacement's context. Bootstrap snapshots in child files are
not new requests. The complete corpus contains six billable requests:
2500 inclusive input, 950 cached input, 250 output, 2750 real total tokens.
At synthetic prices of $2 fresh input, $1 cached input and $10 output per
million tokens, its cost is exactly `$0.006550`.

Tests also derive ordinary/legacy copied forks and invalid, incomplete,
conflicting, filtered and cumulative-only cases from these records. JSONL
must remain UTF-8 with LF endings: literal history byte offsets are part of
the reproduction, enforced by the adjacent `.gitattributes`.

Run in an isolated home (PowerShell):

```powershell
$env:CC_SWITCH_TEST_HOME = Join-Path $env:TEMP ('cc-switch-fork-' + [guid]::NewGuid())
New-Item -ItemType Directory -Force $env:CC_SWITCH_TEST_HOME | Out-Null
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib services::session_usage_codex::tests -- --test-threads=1
```

The fixture tests additionally install an RAII-scoped `CC_SWITCH_TEST_HOME`
and use `Database::init()` to verify a real temporary SQLite database and the
dashboard's `get_usage_summary` / `get_model_stats` query methods. The ignored
real-corpus harness is not run. No model requests or application startup are
needed.
