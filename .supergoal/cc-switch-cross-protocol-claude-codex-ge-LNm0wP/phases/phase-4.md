SUPERGOAL_PHASE_START
Phase: 4 of 8 — Wire Codex To Claude
Task: Let Codex use Anthropic/Claude providers for non-streaming requests through the proxy.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 10
Evidence required: changed file summary, fixture test output, route/auth evidence, command summaries
Depends on phases: 1, 2, 3

## Work

Implement the first end-to-end missing direction: Codex app protocol targeting an Anthropic provider. Start non-streaming. Wire request routing, endpoint rewrite, auth header strategy, request transform, response transform, usage accounting, and provider selection without disrupting existing Codex/OpenAI-native behavior.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- Changed file summary for routing, forwarder, transforms, and auth.
- Fixture test output for non-streaming Codex -> Claude.
- Endpoint/auth rewrite evidence.
- Command summary with exit codes.

## Deliverables

- Codex-consumer routing path that can select Anthropic providers in proxy mode.
- OpenAI Responses or Codex request -> Anthropic Messages transform as needed by real Codex traffic.
- Anthropic Messages response -> OpenAI Responses/Codex response transform.
- Non-streaming fixture tests and forwarder/handler tests.
- Clear unsupported-feature behavior for direct mode and unsupported endpoints.

## Acceptance Criteria

- Codex native OpenAI/Codex providers still use the existing path.
- Codex -> Anthropic provider path rewrites endpoint to Anthropic Messages.
- Anthropic auth uses the configured Anthropic key/token and never forwards OpenAI bearer placeholders upstream.
- Request transform handles input/messages, system/developer instructions, max tokens, temperature/top_p, tools, and tool results.
- Response transform returns the response shape Codex expects for non-streaming calls.
- Usage accounting uses the actual outbound/upstream model where available.
- Unsupported direct mode produces a clear local error, not a malformed upstream request.
- Unit tests cover golden path and at least three edge cases.
- No real credentials are needed for CI.
- Mandatory commands pass.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
