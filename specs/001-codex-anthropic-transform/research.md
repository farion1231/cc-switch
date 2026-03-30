# Research: Codex Anthropic API Format Transform

**Date**: 2026-03-27
**Branch**: 001-codex-anthropic-transform

## Research Questions & Findings

### RQ1: How does the existing `openai_chat` transform work?

**Decision**: Follow the same pattern for the `anthropic` transform.

**Rationale**: The `openai_chat` transform path is already working and well-tested. It follows a clean pattern:
1. `get_codex_api_format()` detects the mode from `provider.meta.api_format`
2. `handle_responses()` checks the format and branches
3. Request is converted via `transform_codex::responses_to_chat_completions()`
4. Forwarded to `/chat/completions` endpoint instead of `/responses`
5. Response is converted back via `transform_codex::chat_completions_to_responses()`
6. For streaming: forces `stream=false` upstream, simulates SSE from complete response

**Alternatives considered**:
- Creating a completely separate handler: Rejected — too much code duplication
- Using the Claude handler path: Rejected — wrong direction (Claude handler converts OpenAI→Anthropic, we need Anthropic→OpenAI Responses)

### RQ2: Can existing `transform_responses.rs` be reused?

**Decision**: Use as reference but write new functions. The existing module converts in the WRONG direction for our use case.

**Rationale**:
- `transform_responses.rs::anthropic_to_responses()` converts Anthropic REQUEST → Responses REQUEST (used by Claude handler for OpenRouter)
- `transform_responses.rs::responses_to_anthropic()` converts Responses RESPONSE → Anthropic RESPONSE
- We need: Responses REQUEST → Anthropic REQUEST (reverse of `anthropic_to_responses`)
- We need: Anthropic RESPONSE → Responses RESPONSE (reverse of `responses_to_anthropic`)
- The field mappings are well-documented in `transform_responses.rs` and can be reversed

### RQ3: Should streaming use real-time conversion or simulate from complete response?

**Decision**: Implement real-time streaming conversion (Anthropic SSE → Responses API SSE).

**Rationale**:
- The `openai_chat` path uses simulate-from-complete because Chat Completions doesn't guarantee the same event lifecycle as Responses API
- Anthropic SSE has a well-defined lifecycle (`message_start` → `content_block_start` → `content_block_delta` → `content_block_stop` → `message_delta` → `message_stop`) that maps cleanly to Responses API events
- Real-time streaming provides better UX (tokens appear as they're generated instead of all-at-once)
- The existing `streaming_responses.rs` already demonstrates Responses API SSE → Anthropic SSE conversion; the reverse is technically straightforward

**Alternatives considered**:
- Simulate SSE from complete response (like openai_chat): Simpler but poor UX for long responses
- Hybrid (try streaming, fall back to simulate): Over-complex for initial implementation

### RQ4: How should the forwarder route requests in `anthropic` mode?

**Decision**: Forward to `/v1/messages` with Bearer auth and `anthropic-version` header.

**Rationale**:
- The forwarder already has routing logic for Codex transforms (line 817-828 in diff)
- Adding another branch for `api_format == "anthropic"` follows the pattern
- The upstream endpoint is always `/v1/messages` for Anthropic-compatible APIs
- Bearer auth works via the existing `CodexAdapter::add_auth_headers()`
- `anthropic-version: 2023-06-01` header is needed but many gateways also accept it

### RQ5: How should auth headers be handled?

**Decision**: Use Bearer auth by default (existing CodexAdapter behavior), optionally add `anthropic-version` header.

**Rationale**:
- The user's gateway (`aigw.fx.ctripcorp.com`) uses `sk-` prefixed keys with Bearer auth
- The CodexAdapter already sends `Authorization: Bearer <key>`, which is correct
- Native Anthropic API would need `x-api-key` + `anthropic-version` headers, but gateway proxies typically accept Bearer
- If a provider needs native Anthropic headers, they should use the Claude app type instead

## Key Technical Decisions Summary

| Decision | Choice | Impact |
|----------|--------|--------|
| Transform location | Extend `transform_codex.rs` | Keep Codex transforms together |
| Streaming approach | Real-time SSE conversion | Better UX, new file `streaming_codex_anthropic.rs` |
| Auth strategy | Bearer auth (CodexAdapter default) | No auth changes needed |
| Endpoint routing | Forward to `/v1/messages` | Add branch in forwarder |
| Module structure | New streaming file, extend existing transform | Follows established patterns |
