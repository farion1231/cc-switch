# Built-in Codex identity for unified history

With unified history enabled, official OAuth sessions keep the built-in
`openai` provider. Third-party API routes use that identity with a loopback
`openai_base_url`; the proxy replaces upstream credentials and removes official
account headers and internal request metadata. The native login stays available
to desktop features such as Dot. Strict API direct mode remains outside this
compatibility path and enabling unified history there is rejected.

Existing shared `custom` history can be migrated to `openai` after explicit
opt-in. JSONL and SQLite originals are backed up under a new v2 generation;
legacy v1 ledgers and unrelated provider histories remain intact. Migration
markers include the directory and target provider identity. Active profiles
and malformed provider selectors are checked before migration.

This fixes client identity and history visibility. Backend-scoped native tool
handles can still need a separate portability adapter when changing upstreams.
Dot visibility and a completed task require real desktop acceptance; backend
tests alone do not establish them.
