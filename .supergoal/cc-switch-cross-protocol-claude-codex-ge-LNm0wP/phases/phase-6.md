SUPERGOAL_PHASE_START
Phase: 6 of 8 — Wire Gemini OAuth
Task: Make Gemini Google OAuth providers work from Claude and Codex in proxy mode.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 10
Evidence required: OAuth flow summary, transform tests, redaction tests, command summaries
Depends on phases: 1, 2, 3, 4, 5

## Work

Build on the existing Gemini OAuth refresh manager. Ensure Claude Code/Desktop can use Gemini OAuth through `gemini_native`, and add Codex -> Gemini routing through the Codex-consumer seam. Keep pasted/imported OAuth credentials working. Add full in-app Google login only if OAuth client details are available; otherwise provide a clean import path and explicit UX copy.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- OAuth credential flow summary.
- Redaction test output.
- Gemini transform/endpoint test names.
- Command summary with exit codes.

## Deliverables

- Gemini OAuth provider presets/states for Claude and Codex where appropriate.
- Codex -> Gemini request/response/streaming transforms or adapters.
- Token refresh integration that works for both Claude-originated and Codex-originated traffic.
- Redaction tests for refresh tokens, access tokens, OAuth JSON, and error bodies.
- Manual live validation checklist for Gemini OAuth.

## Acceptance Criteria

- Claude -> Gemini OAuth path uses refreshed Google access tokens at request time.
- Codex -> Gemini OAuth path selects Gemini providers through proxy/takeover routing.
- Token cache keys do not expose refresh tokens in logs or UI.
- Expired access tokens are refreshed before upstream calls.
- OAuth credential parsing accepts supported pasted token and `oauth_creds.json` forms.
- Missing/invalid OAuth credentials produce actionable local errors.
- Gemini Native endpoint construction handles model IDs without duplicated `models/` prefixes.
- Non-streaming and streaming Gemini fixture tests pass for Codex consumer traffic.
- CI tests require no real Google account.
- Mandatory commands pass.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
