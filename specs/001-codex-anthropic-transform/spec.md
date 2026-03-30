# Feature Specification: Codex Anthropic API Format Transform

**Feature Branch**: `001-codex-anthropic-transform`
**Created**: 2026-03-27
**Status**: Draft
**Input**: User description: "下面这个模型现在没法用cc-switch包装成gpt模型给codex使用 — model claude-opus-4-6-v1 at http://aigw.fx.ctripcorp.com/llm/100000667"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Use Anthropic-compatible endpoint as Codex provider (Priority: P1)

A user has an internal API gateway (or third-party Anthropic-compatible API relay) that serves Claude models using the Anthropic Messages API format. They want to configure this as a Codex provider in cc-switch so that Codex CLI can use the model through the local reverse proxy, with cc-switch automatically converting between OpenAI Responses API format (what Codex CLI speaks) and Anthropic Messages API format (what the upstream API expects).

**Why this priority**: This is the core value — without this, users with Anthropic-compatible endpoints cannot use them with Codex CLI at all.

**Independent Test**: Can be fully tested by configuring a provider with `api_format: "anthropic"` pointing to an Anthropic-compatible endpoint, then sending a Codex CLI request through the proxy and verifying a valid response is returned.

**Acceptance Scenarios**:

1. **Given** a Codex provider configured with `api_format = "anthropic"`, base_url pointing to an Anthropic-compatible API, and a valid API key, **When** Codex CLI sends a non-streaming `/v1/responses` request through the proxy, **Then** the proxy converts the request to Anthropic Messages format, forwards it, converts the response back to Responses API format, and returns it to Codex CLI.

2. **Given** a Codex provider configured with `api_format = "anthropic"`, **When** Codex CLI sends a streaming `/v1/responses` request, **Then** the proxy converts the request, receives Anthropic SSE events, converts them to Responses API SSE events, and streams them back to Codex CLI.

3. **Given** a Codex provider configured with `api_format = "anthropic"`, **When** Codex CLI sends a request with tool/function definitions, **Then** the proxy correctly converts tool definitions between Responses API format (flat `name`/`parameters`) and Anthropic format (nested `input_schema`), and correctly handles tool_use/tool_result round-trips.

---

### User Story 2 - Model name mapping for Anthropic backend (Priority: P2)

When a user configures a Codex provider with an Anthropic backend, they need the ability to map the model name. Codex CLI may send a model name like `gpt-4o` or `codex-mini`, but the upstream Anthropic API expects a model name like `claude-opus-4-6-v1`. The proxy should support an `upstream_model` setting to remap the model name in the forwarded request.

**Why this priority**: Essential for practical use since Codex CLI sends its own model names which don't match the upstream Anthropic model names.

**Independent Test**: Can be tested by configuring `upstream_model` in the provider settings, sending a request, and verifying the forwarded request uses the mapped model name.

**Acceptance Scenarios**:

1. **Given** a Codex provider with `api_format = "anthropic"` and `upstream_model = "claude-opus-4-6-v1"`, **When** Codex CLI sends a request with `model: "o3-mini"`, **Then** the forwarded Anthropic request uses `model: "claude-opus-4-6-v1"`.

2. **Given** a Codex provider with `api_format = "anthropic"` but no `upstream_model` configured, **When** a request is sent, **Then** the original model name from the request is passed through unchanged.

---

### User Story 3 - Responses Compact endpoint support (Priority: P2)

Codex CLI also uses the `/v1/responses/compact` endpoint. The Anthropic transform should apply equally to this endpoint, using the same conversion logic.

**Why this priority**: Codex CLI uses both `/v1/responses` and `/v1/responses/compact`; both must work.

**Independent Test**: Can be tested by sending a request to `/v1/responses/compact` with an Anthropic-format provider and verifying the same conversion behavior.

**Acceptance Scenarios**:

1. **Given** a Codex provider with `api_format = "anthropic"`, **When** Codex CLI sends a request to `/v1/responses/compact`, **Then** the same Responses-to-Anthropic conversion is applied, and the response is correctly converted back.

---

### User Story 4 - Auth flexibility for Anthropic-compatible gateways (Priority: P3)

Internal API gateways and third-party Anthropic relays may use Bearer token auth rather than Anthropic's native `x-api-key` header. The proxy should use the appropriate auth strategy when forwarding to the upstream Anthropic-compatible endpoint.

**Why this priority**: Many internal gateways use Bearer auth; the proxy must handle this correctly for the feature to work in real deployments.

**Independent Test**: Can be tested by configuring a provider with a Bearer-style API key and verifying the forwarded request uses the `Authorization: Bearer <key>` header.

**Acceptance Scenarios**:

1. **Given** a Codex provider with `api_format = "anthropic"` and an API key, **When** the proxy forwards the request, **Then** it uses `Authorization: Bearer <key>` header (standard Bearer auth, not Anthropic's `x-api-key`).

---

### Edge Cases

- What happens when the upstream Anthropic API returns an error (4xx/5xx)? The error should be passed through to Codex CLI in a format it can understand.
- How does the system handle thinking/reasoning blocks in Anthropic responses? Anthropic `thinking` content blocks MUST be mapped to Responses API `reasoning` output items. In the request direction, Responses API `reasoning.effort` parameters MUST be converted to Anthropic `thinking` parameters (verified: `claude-opus-4-6-v1` supports extended thinking).
- What happens when the request includes image content? Image blocks should be converted between Responses API `input_image` format and Anthropic `image` block format.
- How does the system handle `max_output_tokens` from Responses API vs `max_tokens` in Anthropic API? They should be correctly mapped.
- What happens when the Anthropic endpoint returns a streaming response with `content_block_delta` events? They should be correctly assembled into Responses API SSE events.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support a new `api_format` value of `"anthropic"` for Codex providers, triggering Responses API to Anthropic Messages API format conversion.
- **FR-002**: System MUST convert OpenAI Responses API request bodies to Anthropic Messages API request bodies, including: `input` to `messages`, `instructions` to `system`, `max_output_tokens` to `max_tokens`, tools (flat format to nested `input_schema` format).
- **FR-003**: System MUST convert Anthropic Messages API response bodies to OpenAI Responses API response bodies, including: `content` to `output`, `stop_reason` to `status`, `usage` field mapping.
- **FR-004**: System MUST support streaming mode by converting Anthropic SSE events (`message_start`, `content_block_start`, `content_block_delta`, `message_delta`, `message_stop`) into Responses API SSE events (`response.created`, `response.output_text.delta`, `response.completed`, etc.).
- **FR-005**: System MUST support model name remapping via the existing `upstream_model` provider setting when `api_format = "anthropic"`.
- **FR-006**: System MUST forward the Anthropic-format request to the endpoint `<base_url>/v1/messages` (appending the standard Anthropic API path).
- **FR-007**: System MUST use Bearer token authentication when forwarding to the upstream Anthropic-compatible endpoint.
- **FR-008**: System MUST handle tool_use/tool_result round-trips by converting between Responses API `function_call`/`function_call_output` items and Anthropic `tool_use`/`tool_result` content blocks.
- **FR-009**: System MUST pass through upstream error responses to the client in a format Codex CLI can interpret.
- **FR-010**: System MUST apply the same transform logic to both `/v1/responses` and `/v1/responses/compact` endpoints.
- **FR-011**: System MUST convert Responses API `reasoning.effort` parameters to Anthropic `thinking` parameters in the request direction, and convert Anthropic `thinking` content blocks to Responses API `reasoning` output items in the response direction.

### Key Entities

- **Codex Provider with Anthropic backend**: A provider configuration with `meta.api_format = "anthropic"`, containing a base_url pointing to an Anthropic-compatible API and a Bearer-style API key.
- **Compatibility Transform** (`transform_compat`): Bidirectional conversion between OpenAI Responses API and other API formats (Chat Completions, Anthropic Messages) for both request and response payloads, including streaming SSE events via `streaming_compat`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A Codex CLI session can successfully complete a multi-turn conversation (including tool use) through the proxy with an Anthropic-compatible backend, with all requests and responses correctly converted.
- **SC-002**: Streaming responses from the Anthropic backend are delivered to Codex CLI in real-time as Responses API SSE events, with the first token appearing within the same latency as a direct Anthropic API call (plus proxy overhead under 100ms).
- **SC-003**: All existing Codex proxy functionality (transparent passthrough and openai_chat transform) continues to work unchanged after the change.
- **SC-004**: Error responses from the upstream Anthropic API are correctly relayed to Codex CLI, allowing the CLI to display meaningful error messages to users.

## Clarifications

### Session 2026-03-27

- Q: Transform module naming convention — should `transform_codex.rs` be renamed to avoid tool-specific naming? → A: Rename to `transform_compat.rs` (compatibility transforms). Streaming counterpart: `streaming_compat.rs`. Follows the existing `transform_` prefix convention while using an abstract name that captures the concept of API format compatibility layers. The existing `transform_codex.rs` (Responses ↔ Chat Completions) and the new Anthropic transform will both live in `transform_compat.rs`.
- Q: How should thinking/reasoning blocks be handled? → A: Map `thinking` blocks → `reasoning` output items (Option A). Verified via API test: `claude-opus-4-6-v1` at the gateway supports extended thinking — when `thinking` parameter is sent, response includes `{"type": "thinking", "thinking": "..."}` content blocks. Must also handle the request direction: convert Responses API `reasoning.effort` parameter to Anthropic `thinking` parameter.
- Q: Code reuse strategy — should new transform functions share utilities with existing `transform_responses.rs`? → A: Option B — adapt + extract shared utilities. Extract stop_reason/usage/tool_choice mapping as bidirectional utility functions in `transform_responses.rs`, reusable by both directions. Extend `convert_input_items_to_messages()` in `transform_compat.rs` with Anthropic output mode. Core conversion functions remain in `transform_compat.rs` but call shared mappers.
- Q: Are there existing PRs implementing the same feature? → A: No. Searched farion1231/cc-switch PRs — #1058 (Claude openai_chat URL fix) and #924 (URL detection + Codex format selector) are related infrastructure but no one is implementing Codex + Anthropic backend transform.

## Assumptions

- The upstream Anthropic-compatible API follows the standard Anthropic Messages API contract (request/response schema, SSE event format).
- The API gateway uses Bearer token auth (not Anthropic's native `x-api-key` + `anthropic-version` header scheme). If the gateway requires Anthropic-native headers, the existing ClaudeAuth provider type can be used as reference.
- The `upstream_model` setting mechanism already exists in the codebase and can be reused for the new transform path.
- The existing `transform_responses.rs` module (which handles Anthropic-to-Responses conversion for the Claude handler) will be extended to export shared bidirectional utility functions (stop_reason mapping, usage mapping, tool_choice mapping) that both directions can call. Core conversion functions live in `transform_compat.rs`.
- Codex CLI sends requests to `/v1/responses` and `/v1/responses/compact` endpoints; both need to be handled identically.
