# Implementation Plan: Codex Anthropic API Format Transform

**Branch**: `001-codex-anthropic-transform` | **Date**: 2026-03-27 | **Spec**: [spec.md](spec.md)

## Current Status: Streaming Tool Use Bug

### What Works
- Non-streaming: text, tools, thinking — all pass via curl
- Streaming: text responses work end-to-end with Codex CLI ("Say hello" succeeds)
- SSE format: `type` field + `response` wrapping matches OpenAI Responses API spec
- Transform: Responses API ↔ Anthropic Messages request/response conversion (95 unit tests pass)
- Handler/Forwarder routing: `/v1/messages` with `anthropic-version` header
- Developer role → system prompt extraction
- Consecutive same-role message merging
- Thinking blocks skipped in streaming output (Codex CLI can't handle reasoning items)

### Blocking Bug
**Gateway `tool_use` content_block_start missing `id` and `name`**

Standard Anthropic API returns:
```json
{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_xxx","name":"shell","input":{}}}
```

This gateway (`aigw.fx.ctripcorp.com`) returns:
```json
{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","input":{"command":"ls -la"}}}
```

Missing `id` and `name` → our streaming converter emits empty function_call → Codex CLI sends empty tool_use in follow-up → Anthropic rejects.

### Fix Required
In `streaming_compat.rs`, `content_block_start` → `tool_use` handler:
1. Generate `id` if missing: `format!("tool_{}", output_index)` or UUID
2. Infer `name` from request context (pass tools list to streaming converter) or use first tool name
3. If `input` is already complete in `content_block_start`, skip `content_block_delta` accumulation and use it directly

### Files Modified in This Session
| File | Changes |
|------|---------|
| `src-tauri/src/proxy/providers/transform_compat.rs` | RENAMED from transform_codex.rs + added `responses_to_anthropic_messages()`, `anthropic_messages_to_responses()`, developer role handling, message merging, default max_tokens |
| `src-tauri/src/proxy/providers/streaming_compat.rs` | NEW — Anthropic SSE → Responses API SSE real-time converter |
| `src-tauri/src/proxy/providers/transform_responses.rs` | Added 4 shared utility functions + 9 tests |
| `src-tauri/src/proxy/providers/codex.rs` | Updated `get_codex_api_format()` + `needs_transform()` for "anthropic" |
| `src-tauri/src/proxy/providers/mod.rs` | Renamed module + registered streaming_compat |
| `src-tauri/src/proxy/handlers.rs` | Added anthropic branch in handle_responses/compact, handle_codex_anthropic_transform(), real streaming wiring, debug logging |
| `src-tauri/src/proxy/forwarder.rs` | Added /v1/messages routing + anthropic-version header |
| Database | Provider meta.apiFormat changed to "anthropic" |
