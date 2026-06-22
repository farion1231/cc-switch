SUPERGOAL_PHASE_START
Phase: 2 of 8 — Harden Prior Work
Task: Review and stabilize the work already merged in PR #1.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 9
Evidence required: code review findings, fixed bugs, regression tests, command summaries
Depends on phases: 1

## Work

Review PR #1 behavior with a bug-risk stance. Focus on per-model provider+target-model routing, Add Provider combobox behavior, Gemini OAuth refresh at request time, and the reverse Chat transform foundation. Fix small issues discovered during previous testing, especially the Add Provider typing race if reproducible.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- Code review findings with severity and file references.
- Regression test names and pass summary.
- Fixed bug summary or explicit deferral rationale.
- Command summary with exit codes.

## Deliverables

- Regression tests for model route provider+target-model persistence and model rewrite.
- Regression tests for Add Provider searchable combobox behavior.
- Regression tests for Gemini OAuth refresh parsing/cache/redaction where feasible.
- A short review note in the transcript distinguishing fixed issues from intentionally deferred cross-protocol work.

## Acceptance Criteria

- Per-model routing preserves `target_model` through DB migration, DAO, Tauri command, UI query hook, and proxy forwarder.
- Route target-model rewrite applies only to the pinned provider and not to failover providers.
- Add Provider combobox remains searchable, alphabetical, and accessible by keyboard.
- The prior fast-typing/input race is either fixed or documented with a failing reproduction and follow-up.
- Gemini OAuth refresh never logs raw tokens in normal success or error paths.
- Reverse Chat transform unit tests still pass.
- No schema migration version regression is introduced.
- Frontend tests cover changed UI behavior.
- All mandatory commands pass or pre-existing failures are documented with evidence.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
