SUPERGOAL_PHASE_START
Phase: 1 of 8 — Normalize Base
Task: Establish the correct working branch, preserve PR #1, and produce a green baseline.
Mandatory commands: git status --short --branch; pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 8
Evidence required: branch/ref summary, diff summary, command summaries, baseline decision
Depends on phases: none

## Work

Start by making the repository state explicit. The planning recon found that local `HEAD` is upstream `origin/main`, while the previously merged PR lives on fetched `garcia/main`. Create or switch to a safe work branch that contains PR #1 behavior. If upstream reconciliation is attempted, do it intentionally and preserve all PR #1 features with tests.

## Mandatory commands

- `git status --short --branch`
- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- Branch/ref summary for `HEAD`, `origin/main`, and `garcia/main`.
- Diff summary for PR #1 files and upstream divergence.
- Command summary with exit codes.
- Baseline decision and any pre-existing failure classification.

## Deliverables

- A named git branch for this work.
- A written baseline note in the transcript showing `HEAD`, `origin/main`, and `garcia/main`.
- A decision: continue from `garcia/main` now, or reapply/rebase it onto upstream before new feature work.

## Acceptance Criteria

- `git status --short --branch` is shown before edits.
- `git rev-parse HEAD origin/main garcia/main` is shown.
- The branch used for implementation contains PR #1 behavior or explicitly reapplies it.
- No unrelated dirty files are overwritten or reverted.
- Existing PR #1 files are identified: model routes, provider router, forwarder, Gemini OAuth refresh, Add Provider selector.
- Frontend baseline commands are run and summarized.
- Rust baseline commands are run and summarized.
- Any baseline failures are classified as pre-existing or fixed before Phase 2.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
