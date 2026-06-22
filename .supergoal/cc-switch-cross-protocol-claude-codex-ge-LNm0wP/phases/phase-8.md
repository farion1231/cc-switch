SUPERGOAL_PHASE_START
Phase: 8 of 8 — Polish & Harden
Task: Run the full regression, security, docs, and live-validation readiness pass.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 12
Evidence required: final command summaries, security checklist, docs diff, live validation checklist, final risk log
Depends on phases: 1, 2, 3, 4, 5, 6, 7

## Work

Perform the mandatory final hardening pass. Re-run all gates, review the full diff, check for secret leakage and debug leftovers, verify docs and user-facing copy, and produce a live-validation checklist for the user to run with real provider accounts.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- Final command summaries.
- Security/redaction checklist.
- Docs/live-validation checklist diff summary.
- Final known-limits and risk log.

## Deliverables

- Updated docs or PR notes explaining supported directions, limitations, and setup.
- Security/redaction checklist completed.
- Live validation checklist for Claude -> Codex, Codex -> Claude, Claude -> Gemini OAuth, and Codex -> Gemini OAuth.
- Final diff review summary with known limitations.

## Acceptance Criteria

- All mandatory commands pass in the final state.
- `git diff` is reviewed for unrelated changes, debug prints, session TODO/FIXME, commented-out code, and secrets.
- No fixture, screenshot, test output, or log contains raw access tokens, refresh tokens, API keys, or account identifiers.
- Docs explain proxy/takeover-mode requirement and direct-mode limitations.
- Docs explain lossy transform areas: thinking, tools, images, system prompts, and streaming differences.
- Manual live validation steps are concrete and ordered.
- At least one successful local smoke flow is documented for each UI surface changed.
- Existing provider flows still have regression coverage or manual smoke evidence.
- Schema migrations are forward-compatible for existing databases.
- Accessibility basics are checked for new controls: labels, keyboard access, focus visibility.
- Performance/regression review finds no obvious blocking hot path or unbounded buffering in streams.
- Remaining external blockers, if any, are stated as credential/account limitations rather than code completion gaps.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
