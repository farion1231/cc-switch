# Tasks: Codex Anthropic API Format Transform

**Input**: Design documents from `/specs/001-codex-anthropic-transform/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Unit tests included — this is a proxy transform layer where correctness is critical.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup (Module Rename + Structure)

**Purpose**: Rename `transform_codex.rs` → `transform_compat.rs` and update all references

- [x] T001 Rename `src-tauri/src/proxy/providers/transform_codex.rs` → `src-tauri/src/proxy/providers/transform_compat.rs`
- [x] T002 Update `pub mod transform_codex` → `pub mod transform_compat` in `src-tauri/src/proxy/providers/mod.rs`
- [x] T003 Update all `transform_codex::` references to `transform_compat::` in `src-tauri/src/proxy/handlers.rs`
- [x] T004 Run `cargo test` in `src-tauri/` to verify rename doesn't break existing openai_chat transform

**Checkpoint**: Rename complete, all existing tests pass

---

## Phase 2: Foundational (Shared Utilities + Format Detection)

**Purpose**: Extract shared bidirectional mappers and enable `"anthropic"` format detection

**⚠️ CRITICAL**: Must complete before US1 implementation

- [x] T005 [P] Extract `map_anthropic_stop_reason_to_status()` as `pub(crate)` utility in `src-tauri/src/proxy/providers/transform_responses.rs` — maps Anthropic `stop_reason` ("end_turn", "tool_use", "max_tokens") → Responses API `status` ("completed", "incomplete")
- [x] T006 [P] Extract `map_anthropic_usage_to_responses()` as `pub(crate)` utility in `src-tauri/src/proxy/providers/transform_responses.rs` — maps Anthropic usage (input_tokens, output_tokens, cache_*) → Responses API usage format (adds total_tokens)
- [x] T007 [P] Extract `map_tool_choice_to_anthropic()` as `pub(crate)` utility in `src-tauri/src/proxy/providers/transform_responses.rs` — reverse of existing `map_tool_choice_to_responses()`: maps Responses "required" → Anthropic `{type: "any"}`, function selector → `{type: "tool", name}`
- [x] T008 [P] Extract `map_tool_schema_to_anthropic()` as `pub(crate)` utility in `src-tauri/src/proxy/providers/transform_responses.rs` — converts Responses flat tool `{name, parameters}` → Anthropic `{name, input_schema}`
- [x] T009 Update `get_codex_api_format()` to recognize `"anthropic"` and update `needs_transform()` to return `true` for both `"openai_chat"` and `"anthropic"` in `src-tauri/src/proxy/providers/codex.rs`
- [x] T010 Add unit tests for all shared utility functions (T005-T008) in `src-tauri/src/proxy/providers/transform_responses.rs`
- [x] T011 Run `cargo test` in `src-tauri/` to verify existing transform_responses tests still pass

**Checkpoint**: Shared utilities available, `get_codex_api_format()` recognizes "anthropic"

---

## Phase 3: User Story 1 - Core Anthropic Transform (Priority: P1) 🎯 MVP

**Goal**: Codex CLI can send Responses API requests through the proxy to an Anthropic-compatible backend, with bidirectional format conversion (non-streaming first, then streaming)

**Independent Test**: Configure a provider with `api_format: "anthropic"` + `base_url` pointing to `http://aigw.fx.ctripcorp.com/llm/100000667`, send a `/v1/responses` request, verify valid response

### Implementation for User Story 1

- [x] T012 [P] [US1] Implement `responses_to_anthropic_messages()` request conversion function in `src-tauri/src/proxy/providers/transform_compat.rs` — convert input items → messages, instructions → system, tools flat → input_schema, max_output_tokens → max_tokens, reasoning.effort → thinking parameter. Call shared utilities from T005-T008
- [x] T013 [P] [US1] Implement `anthropic_messages_to_responses()` response conversion function in `src-tauri/src/proxy/providers/transform_compat.rs` — convert content blocks → output items (text → message, tool_use → function_call, thinking → reasoning), stop_reason → status, usage mapping. Call shared utilities from T005-T006
- [x] T014 [P] [US1] Add unit tests for `responses_to_anthropic_messages()` in `src-tauri/src/proxy/providers/transform_compat.rs` — test string input, array input, tools conversion, function_call round-trip, max_output_tokens, instructions, reasoning.effort → thinking
- [x] T015 [P] [US1] Add unit tests for `anthropic_messages_to_responses()` in `src-tauri/src/proxy/providers/transform_compat.rs` — test text response, tool_use response, thinking blocks → reasoning, stop_reason mapping, usage mapping
- [x] T016 [US1] Add `api_format == "anthropic"` branch to `handle_responses()` in `src-tauri/src/proxy/handlers.rs` — convert request via `transform_compat::responses_to_anthropic_messages()`, forward to `/v1/messages`, convert response via `handle_codex_anthropic_transform()`
- [x] T017 [US1] Implement `handle_codex_anthropic_transform()` async function in `src-tauri/src/proxy/handlers.rs` — non-streaming: read full Anthropic response, call `anthropic_messages_to_responses()`, return JSON; streaming: use `build_responses_sse_events()` for initial simulated SSE (real streaming in Phase 5)
- [x] T018 [US1] Add `"anthropic"` endpoint routing branch in `src-tauri/src/proxy/forwarder.rs` — route Responses API endpoints to `/v1/messages` when api_format is anthropic; add `anthropic-version: 2023-06-01` header conditionally
- [x] T019 [US1] Run `cargo test` and verify all new + existing tests pass in `src-tauri/`

**Checkpoint**: US1 functional — non-streaming Codex → Anthropic round-trip works

---

## Phase 4: User Story 2 - Model Name Mapping (Priority: P2)

**Goal**: `upstream_model` setting remaps model name in forwarded Anthropic request

**Independent Test**: Configure provider with `upstream_model = "claude-opus-4-6-v1"`, send request with model `o3-mini`, verify forwarded request uses `claude-opus-4-6-v1`

### Implementation for User Story 2

- [ ] T020 [US2] Verify `upstream_model` mapping is wired in T016's handler code (already uses `ctx.provider.settings_config.get("upstream_model")`) — add unit test for model remapping in `src-tauri/src/proxy/providers/transform_compat.rs`
- [ ] T021 [US2] Add unit test for passthrough when `upstream_model` is not set in `src-tauri/src/proxy/providers/transform_compat.rs`

**Checkpoint**: Model name mapping verified

---

## Phase 5: User Story 3 - Responses Compact Endpoint (Priority: P2)

**Goal**: `/v1/responses/compact` endpoint uses the same Anthropic transform as `/v1/responses`

**Independent Test**: Send request to `/v1/responses/compact` with anthropic-format provider, verify same conversion

### Implementation for User Story 3

- [x] T022 [US3] Add `api_format == "anthropic"` branch to `handle_responses_compact()` in `src-tauri/src/proxy/handlers.rs` — identical pattern to T016, forward to `/v1/messages`, use same `handle_codex_anthropic_transform()`
- [ ] T023 [US3] Run `cargo test` to verify compact endpoint handles anthropic format in `src-tauri/`

**Checkpoint**: Both `/v1/responses` and `/v1/responses/compact` work with anthropic transform

---

## Phase 6: User Story 4 - Auth Flexibility (Priority: P3)

**Goal**: Bearer auth works for Anthropic-compatible gateways

**Independent Test**: Configure provider with `sk-` prefixed API key, verify `Authorization: Bearer` header sent

### Implementation for User Story 4

- [x] T024 [US4] Verify CodexAdapter `add_auth_headers()` sends Bearer auth for anthropic-format providers in `src-tauri/src/proxy/providers/codex.rs` — Bearer is already the default; add test confirming behavior
- [x] T025 [US4] Ensure `anthropic-version: 2023-06-01` header is added in forwarder for anthropic format (from T018) — add test in `src-tauri/src/proxy/forwarder.rs`

**Checkpoint**: Auth headers correct for gateway endpoints

---

## Phase 7: Streaming Support (Enhancement on US1)

**Purpose**: Real-time Anthropic SSE → Responses API SSE conversion

- [ ] T026 [P] [US1] Create `src-tauri/src/proxy/providers/streaming_compat.rs` — implement `create_responses_sse_stream_from_anthropic()` state machine: parse Anthropic SSE events (message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop) → emit Responses API SSE events (response.created, response.output_item.added, response.output_text.delta, response.function_call_arguments.delta, response.completed)
- [ ] T027 Register `pub mod streaming_compat;` in `src-tauri/src/proxy/providers/mod.rs`
- [ ] T028 Update `handle_codex_anthropic_transform()` in `src-tauri/src/proxy/handlers.rs` to use real streaming via `create_responses_sse_stream_from_anthropic()` when `is_stream == true` instead of simulated SSE
- [ ] T029 [P] [US1] Add unit tests for streaming converter in `src-tauri/src/proxy/providers/streaming_compat.rs` — test text streaming, tool_use streaming, thinking block streaming, multi-block responses, error events
- [ ] T030 Run `cargo test` to verify streaming + all existing tests in `src-tauri/`

**Checkpoint**: Real-time streaming works — first token latency matches direct Anthropic API

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Validation, regression testing, documentation

- [ ] T031 [P] Run full `cargo test` regression in `src-tauri/` — verify all existing openai_responses and openai_chat tests pass unchanged
- [ ] T032 [P] Validate against quickstart.md scenarios — manual E2E test with `http://aigw.fx.ctripcorp.com/llm/100000667`
- [ ] T033 [P] Verify error passthrough — send request that triggers 4xx/5xx from upstream, confirm Codex CLI displays meaningful error
- [ ] T034 Code cleanup — remove any TODO comments, ensure all `#[allow(dead_code)]` annotations are appropriate

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Phase 2 — core MVP
- **US2 (Phase 4)**: Depends on Phase 3 (uses handler code from US1)
- **US3 (Phase 5)**: Depends on Phase 3 (same transform pattern)
- **US4 (Phase 6)**: Depends on Phase 3 (uses forwarder from US1)
- **Streaming (Phase 7)**: Depends on Phase 3 (enhances US1)
- **Polish (Phase 8)**: Depends on all desired phases

### User Story Dependencies

- **US1 (P1)**: After Foundational — no dependencies on other stories
- **US2 (P2)**: After US1 — uses handler code, minimal additional work
- **US3 (P2)**: After US1 — identical pattern, different endpoint
- **US4 (P3)**: After US1 — verification of existing auth behavior

### Parallel Opportunities

Within Phase 2 (Foundational):
- T005, T006, T007, T008 — all shared utilities can be written in parallel (different functions)

Within Phase 3 (US1):
- T012, T013 — request and response conversion in same file but independent functions
- T014, T015 — tests for T012 and T013 respectively

Within Phase 7 (Streaming):
- T026, T029 — streaming implementation and tests

### Suggested Subagents for Implementation

| Phase | Subagent Type | Skill | Why |
|-------|-------------|-------|-----|
| Phase 1 (Rename) | `plan-implementer` | — | Simple mechanical rename |
| Phase 2 (Utilities) | `plan-implementer` | — | Focused extraction from existing code |
| Phase 3 (US1 Core) | `plan-implementer` | `everything-claude-code:rust-review` | Core transform logic, needs Rust review |
| Phase 7 (Streaming) | `plan-implementer` | `everything-claude-code:rust-review` | Complex SSE state machine |
| Phase 8 (Polish) | `code-reviewer` | `everything-claude-code:verification-loop` | Regression + quality validation |

---

## Parallel Example: User Story 1

```bash
# Launch foundational utilities in parallel (4 agents):
Task T005: "Extract map_anthropic_stop_reason_to_status() in transform_responses.rs"
Task T006: "Extract map_anthropic_usage_to_responses() in transform_responses.rs"
Task T007: "Extract map_tool_choice_to_anthropic() in transform_responses.rs"
Task T008: "Extract map_tool_schema_to_anthropic() in transform_responses.rs"

# After foundational, launch US1 conversion functions in parallel (2 agents):
Task T012: "Implement responses_to_anthropic_messages() in transform_compat.rs"
Task T013: "Implement anthropic_messages_to_responses() in transform_compat.rs"

# Then launch tests in parallel (2 agents):
Task T014: "Unit tests for responses_to_anthropic_messages()"
Task T015: "Unit tests for anthropic_messages_to_responses()"
```

---

## Implementation Strategy

### MVP First (US1 Only — Non-Streaming)

1. Complete Phase 1: Rename (T001-T004)
2. Complete Phase 2: Shared Utilities (T005-T011)
3. Complete Phase 3: US1 Core Transform (T012-T019)
4. **STOP and VALIDATE**: Test with `http://aigw.fx.ctripcorp.com/llm/100000667`
5. Working non-streaming Codex → Anthropic round-trip

### Incremental Delivery

1. Setup + Foundational → rename + utilities ready
2. US1 (non-streaming) → MVP! Test independently
3. US2 + US3 → model mapping + compact endpoint
4. US4 → auth verification
5. Streaming (Phase 7) → real-time SSE
6. Polish → regression + cleanup

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story
- Each user story independently testable after completion
- Commit after each task or logical group
- Total: 34 tasks across 8 phases
- MVP scope: Phases 1-3 (T001-T019, 19 tasks)
