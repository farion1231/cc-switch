SUPERGOAL_PHASE_START
Phase: 5 of 8 — Implement Streaming Bridge
Task: Add streaming SSE conversion for Codex <-> Claude cross-protocol traffic.
Mandatory commands: pnpm typecheck; pnpm format:check; pnpm test:unit; cargo fmt --check; cargo clippy -- -D warnings; cargo test
Acceptance criteria: 10
Evidence required: SSE fixture outputs, cancellation/error tests, command summaries
Depends on phases: 1, 2, 3, 4

## Work

Extend the Codex -> Claude path from non-streaming to streaming. Implement bidirectional chunk conversion between Anthropic event streams and OpenAI/Codex Responses/Chat SSE shapes, including start/delta/final/usage/error events.

## Mandatory commands

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## Evidence required

- SSE fixture input/output snippets.
- Streaming test names and pass summary.
- Cancellation/error handling evidence.
- Command summary with exit codes.

## Deliverables

- Streaming parser/converter modules or functions with pure fixture tests.
- Forwarder integration that preserves streaming headers and disables incompatible compression when necessary.
- Error and cancellation behavior that does not leak active connection counters.
- Tests for text deltas, reasoning/thinking deltas, tool-call deltas, usage/final events, and upstream error chunks.

## Acceptance Criteria

- Streaming Codex -> Claude requests receive valid Codex/OpenAI-style SSE downstream.
- Anthropic `content_block_*`, `message_delta`, and `message_stop` events are mapped deterministically.
- Tool-use streaming reconstructs stable tool call IDs, names, and argument deltas.
- Thinking/reasoning is mapped or explicitly dropped with a documented limitation.
- Usage tokens are emitted in the final shape expected by the Codex client where supported.
- Upstream Anthropic error events become downstream error events with redacted details.
- Client cancellation releases active connection tracking and does not mark healthy providers as failed.
- Fixture tests include at least one multiline SSE stream and one tool-call stream.
- Existing Claude -> OpenAI/Codex streaming tests, if any, remain green.
- Mandatory commands pass.

[Agent will print SUPERGOAL_PHASE_VERIFY and SUPERGOAL_PHASE_DONE here during execution]
