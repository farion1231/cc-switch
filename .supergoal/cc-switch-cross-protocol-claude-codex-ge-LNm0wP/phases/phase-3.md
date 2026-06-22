SUPERGOAL_PHASE_START
Phase: 3 of 8 — Build Fixture Harness
Task: Add golden fixtures and a capability matrix before wiring new protocol paths.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 8
Evidence required: fixture file list, test names, capability matrix summary, command summaries
Depends on phases: 1, 2

## Work

Create a test harness for protocol conversions and provider capability decisions. The goal is to make new translation work observable without live secrets. Include fixtures for Anthropic Messages, OpenAI Chat Completions, OpenAI Responses, Codex-style Responses, and Gemini Native/OAuth paths.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- Fixture file listing.
- Capability matrix summary.
- Test names covering request, response, and SSE fixture paths.
- Command summary with exit codes.

## Deliverables

- Golden fixture files for non-streaming text, system/developer messages, tool calls/tool results, images, thinking/reasoning, and usage accounting.
- Golden fixture files for streaming SSE chunks in both directions.
- A provider capability matrix that declares app protocol, provider protocol, supported transform path, streaming support, tool support, and known lossy features.
- Unit tests that can run in CI without provider secrets.

## Acceptance Criteria

- Fixtures are committed under an appropriate Rust test fixture directory.
- Tests assert exact transformed JSON for representative non-streaming requests/responses.
- Tests assert streaming chunk sequences for at least one text stream and one tool-call stream.
- Capability matrix rejects unsupported direct-mode cross-protocol paths.
- Capability matrix allows proxy/takeover mode cross-protocol paths that have transforms.
- Known lossy features are documented in test names or matrix comments.
- Fixtures contain no real API keys, refresh tokens, user messages, or account identifiers.
- Mandatory commands pass.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
