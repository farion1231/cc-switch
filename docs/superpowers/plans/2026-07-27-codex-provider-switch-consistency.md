# Codex Provider Switch Consistency Implementation Plan

**Goal:** Make official/third-party Codex switching credential-safe and make saved
threads follow the selected provider when unified history is enabled.

## Completed implementation

- [x] Discover every `state_*.sqlite` in the Codex root, nested `sqlite/`, TOML
  `sqlite_home`, and `CODEX_SQLITE_HOME`; canonicalize and deduplicate paths.
- [x] Keep third-party keys provider-scoped, preserve official OAuth separately,
  retain complete inactive provider definitions, and replace incomplete placeholders.
- [x] Reconcile `session_meta`, `thread_settings_applied`, and every discovered
  SQLite `threads` table to the selected provider/model.
- [x] Preserve unknown JSONL events and JSONL timestamps.
- [x] Prevalidate all history artifacts and roll back applied files/databases on any
  failure or concurrent modification.
- [x] Publish Codex live/history state before local and database current-provider
  state; restore live/history/local state when the commit fails.
- [x] Remove official-as-`custom` injection and its one-way migration path while
  retaining legacy restore-backup compatibility.
- [x] Update the existing unified-history UI copy to describe bidirectional
  current-provider-follow behavior and the reopen requirement.

## Verification

- [x] Focused state discovery, credential isolation, history reconciliation,
  bidirectional switch, and rollback tests.
- [x] Rust formatting and Clippy with warnings denied.
- [x] TypeScript typecheck and Prettier check.
- [x] Frontend suite (522 tests) and Rust all-target suite, excluding only seven
  environment-bound upstream cases (occupied fixed port, real Claude config path,
  and Windows symlink privilege).
- [x] Windows release compile and MSI installer packaging.
- [x] Final diff review.

## Review follow-up

- [ ] Add failing coverage proving proxy takeover hot switches reconcile Codex
  history and roll it back before provider publication on failure.
- [ ] Add failing coverage proving a preserved scoped third-party token wins over a
  retained global official API key.
- [ ] Add failing coverage for Windows atomic replacement and recovery when a
  journaled JSONL destination is absent.
- [ ] Implement the three minimal fixes and run focused tests after each change.
- [ ] Run formatting, Clippy, full Rust/frontend tests, release compilation, and
  Windows MSI packaging.
- [ ] Push the review commit, reply in each inline thread with the approved `OK.`
  wording, and resolve only after GitHub reflects the pushed fix.
