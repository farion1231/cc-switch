# Provider Routing and Cross-Protocol Routing for Codex, Claude, and Gemini

> Applies to the `codex/cross-protocol-supergoal` branch after commit `aa1d7089`. This guide documents the fork changes prepared for upstream review, including the earlier `devin/1781828247-english-per-model-routing` work and the later cross-protocol routing work.

## What changed

This fork extends CC Switch in two related areas:

1. Provider management and routing UX: English-first UI defaults, a searchable provider preset selector, and per-model provider routing for Claude Code.
2. Local protocol conversion: selected Codex, Claude, and Gemini Native providers can be used even when the client and upstream provider do not speak the same API protocol.

The branch intentionally includes the prior Devin branch. The final PR branch contains:

- English-default UI and updated i18n coverage.
- Alphabetical searchable Add Provider dropdown replacing the previous preset button grid.
- Per-model provider and model routing for Claude Code, with frontend manager, API bindings, Rust commands, database schema, DAO, proxy router, and model mapper support.
- Reverse OpenAI-to-Anthropic transform foundation used by Codex-to-Claude routing.
- Gemini Google OAuth refresh support at request time.
- Cross-protocol Codex-to-Anthropic and Codex-to-Gemini Native routing.
- Streaming conversion for Anthropic SSE and Gemini SSE.
- Documentation, golden fixtures, and expanded test coverage.

## Provider routing and UX changes

### English-default UI and i18n

The frontend now defaults to English while keeping existing Chinese, Traditional Chinese, and Japanese locale files updated for the newly added user-facing strings. The README files were also updated to reflect the current feature set.

### Searchable Add Provider dropdown

The Add Provider experience was changed from a preset button grid into an alphabetical searchable dropdown. The selector supports normal provider search and preserves management actions when appropriate. Tests cover search behavior, unified-provider presets, and the hidden management actions during active searches.

### Per-model provider routing for Claude Code

The branch adds a model-route configuration path so Claude Code requests can be routed by requested model to a selected provider and target model. This includes:

- `src/components/proxy/ModelRoutingManager.tsx` for the settings UI.
- `src/lib/api/modelRoutes.ts` and `src/lib/query/modelRoutes.ts` for frontend data access.
- `src-tauri/src/commands/model_routes.rs` for Tauri commands.
- `src-tauri/src/database/dao/model_routes.rs` and schema updates for persistence.
- `src-tauri/src/proxy/provider_router.rs` and `src-tauri/src/proxy/model_mapper.rs` for proxy-time routing decisions.
- Handler context changes so the proxy can carry routing information through the request lifecycle.

This is separate from cross-protocol conversion. Per-model routing decides which provider/model should handle the request; the protocol adapters then convert the request if the selected provider needs conversion.

### Partner and provider metadata updates

The branch also carries upstream/fork updates for provider metadata and partner assets, including Kimi partner badge copy/logo changes, GLM 5.2 pricing, and the Volcengine Coding Plan link update.

## Cross-protocol routing changes

Supported routes include:

| Client | Upstream provider | Credential type | Local routing required |
|--------|-------------------|-----------------|------------------------|
| Claude Code | OpenAI Chat Completions | Provider API key | Yes |
| Claude Code | OpenAI Responses API | Provider API key | Yes |
| Claude Code | Gemini Native generateContent | Gemini API key, `ya29` token, or Gemini CLI OAuth JSON | Yes |
| Codex | OpenAI Chat Completions | Provider API key | Yes |
| Codex | Anthropic Messages | Anthropic-compatible API key | Yes |
| Codex | Gemini Native generateContent | Gemini API key, `ya29` token, or Gemini CLI OAuth JSON | Yes |

Direct mode is intentionally not used for these bridge routes. The conversion happens in the CC Switch local proxy, so the proxy and the matching app takeover must stay enabled while the CLI is running.

## User-facing features

### Codex presets

The Codex provider list now includes two cross-protocol presets:

- `Claude / Anthropic via Codex`
- `Gemini Native OAuth/API key via Codex`

These presets configure the provider metadata needed by the proxy:

- `apiFormat = "anthropic"` for Anthropic Messages providers.
- `apiFormat = "gemini_native"` for Gemini Native providers.
- Local routing is required because Codex still sends OpenAI Responses API traffic to CC Switch.

### Provider warnings and routing badges

When a Codex cross-protocol provider is configured, the form shows a bridge warning explaining that the provider must be used through local routing. Provider cards also show the local-routing badge for Anthropic and Gemini Native Codex bridges, not only OpenAI Chat providers.

### Gemini credential inputs

Gemini Native providers accept any of these credentials:

- An AI Studio Gemini API key.
- A bare `ya29...` OAuth access token.
- The full `oauth_creds.json` payload generated by Gemini CLI login.

OAuth refresh handling is serialized per refresh token, and debug output redacts access tokens, refresh tokens, and client secrets.

## OAuth and auth behavior

This branch covers two OAuth-adjacent paths:

- Gemini OAuth credentials can be used by Gemini Native providers through the local proxy. The proxy can parse Gemini CLI OAuth JSON, refresh access tokens when needed, and redact credential fields in debug output.
- Codex OAuth handling from the existing app remains separate. The cross-protocol work does not add Anthropic account OAuth as a Codex credential.

For Codex-to-Claude routing, users should provide an Anthropic-compatible API key in the `Claude / Anthropic via Codex` preset.

## How the proxy routes requests

### Codex to Anthropic/Claude

Codex sends OpenAI Responses API requests. CC Switch detects Anthropic-backed Codex providers and converts the request before forwarding it upstream:

```text
Codex
  -> OpenAI Responses request
  -> CC Switch local proxy
  -> OpenAI Chat-compatible intermediate request
  -> Anthropic Messages request
  -> Anthropic-compatible provider
```

Responses are rebuilt into a Codex-compatible OpenAI response shape. Streaming responses are normalized through the Anthropic SSE to OpenAI Chat SSE converter before the existing Codex streaming handler returns them to Codex.

### Codex to Gemini Native

For Gemini Native providers, the conversion chain is:

```text
Codex
  -> OpenAI Responses request
  -> CC Switch local proxy
  -> OpenAI Chat-compatible intermediate request
  -> Anthropic-like intermediate request
  -> Gemini Native generateContent request
  -> Gemini provider
```

Gemini responses are converted back toward the OpenAI Chat/Codex shape. Gemini streaming events are normalized through the Gemini stream bridge so Codex receives parseable streaming chunks.

### Claude Code to Gemini Native

Claude Code can also use a Gemini Native provider through local routing. Claude sends Anthropic-style traffic to CC Switch, and the proxy converts the upstream request into Gemini Native `generateContent` calls. This allows Gemini API keys or Gemini OAuth credentials to be used from Claude Code without writing those credentials into the Claude live config.

## Setup: Codex using Claude/Anthropic

1. Open CC Switch and switch to the `Codex` app.
2. Click add provider.
3. Select `Claude / Anthropic via Codex`.
4. Paste an Anthropic-compatible API key.
5. Save the provider.
6. Start the local proxy.
7. Enable Codex takeover/local routing.
8. Enable the new provider.
9. Restart the Codex terminal session.
10. Send a small prompt, then a longer streaming prompt.

Expected result: Codex receives a Responses-compatible answer while the upstream request goes to the Anthropic-compatible provider.

## Setup: Codex using Gemini Native OAuth or API key

1. Open CC Switch and switch to the `Codex` app.
2. Click add provider.
3. Select `Gemini Native OAuth/API key via Codex`.
4. Paste one credential:
   - Gemini API key.
   - Bare `ya29...` token.
   - Full Gemini CLI `oauth_creds.json` content.
5. Save the provider.
6. Start the local proxy.
7. Enable Codex takeover/local routing.
8. Enable the new provider.
9. Restart the Codex terminal session.
10. Send a small prompt, then a longer streaming prompt.

Expected result: Codex receives a Responses-compatible answer while Gemini Native handles the upstream request.

## Setup: Claude Code using Gemini Native OAuth or API key

1. Open CC Switch and switch to the `Claude` app.
2. Add or edit a provider that uses Gemini Native format.
3. Paste one credential:
   - Gemini API key.
   - Bare `ya29...` token.
   - Full Gemini CLI `oauth_creds.json` content.
4. Save the provider.
5. Start the local proxy.
6. Enable Claude takeover/local routing.
7. Enable the Gemini-backed provider.
8. Restart the Claude Code terminal session.
9. Send a small prompt, then a longer streaming prompt.

Expected result: Claude Code receives an Anthropic-compatible response while Gemini Native handles the upstream request.

## Implementation overview

The main backend changes are in the proxy provider layer:

- `src-tauri/src/proxy/provider_router.rs` adds provider routing decisions for per-model routes.
- `src-tauri/src/proxy/model_mapper.rs` maps requested models to configured target models.
- `src-tauri/src/proxy/providers/transform_reverse.rs` provides the reverse OpenAI-to-Anthropic transform foundation.
- `src-tauri/src/proxy/forwarder.rs` detects Codex Responses endpoints and selects the required adapter based on the active provider metadata.
- `src-tauri/src/proxy/handlers.rs` applies the intermediate Chat-to-Responses conversion for Anthropic and Gemini Native Codex providers.
- `src-tauri/src/proxy/providers/claude.rs` can read Codex-style cross-protocol auth/config fields when routing through the Claude/Anthropic adapter.
- `src-tauri/src/proxy/providers/gemini_oauth.rs` trims parsed credential fields, serializes refreshes, and redacts OAuth values in `Debug`.
- `src-tauri/src/proxy/providers/streaming.rs` converts Anthropic SSE into OpenAI Chat-compatible SSE.
- `src-tauri/src/proxy/providers/streaming_gemini.rs` converts Gemini SSE into OpenAI Chat-compatible SSE for the Codex bridge.
- `src-tauri/src/proxy/providers/fixture_harness.rs` validates golden protocol fixtures.

The main frontend changes are:

- `src/components/proxy/ModelRoutingManager.tsx` adds the per-model routing UI.
- `src/lib/api/modelRoutes.ts` and `src/lib/query/modelRoutes.ts` expose model-route APIs.
- `src/components/providers/forms/ProviderPresetSelector.tsx` implements the searchable Add Provider selector.
- `src/config/codexProviderPresets.ts` adds the Anthropic and Gemini Native Codex presets.
- `src/components/providers/forms/CodexFormFields.tsx` shows bridge-specific warnings and routing requirements.
- `src/components/providers/forms/ProviderForm.tsx` persists the new Codex API format values.
- `src/components/providers/ProviderCard.tsx` marks Codex Anthropic/Gemini Native providers as requiring local routing.
- `src/types.ts` includes the new Codex API format values.
- `src/i18n/locales/en.json` includes the new user-facing warning text.

## Test coverage

The branch adds fixture and unit coverage for the new conversion paths:

- Provider preset selector tests cover search and management-action behavior.
- Model-route command, DAO, and proxy routing paths are covered by the Rust test suite.
- Golden protocol fixtures under `src-tauri/tests/fixtures/protocol_matrix/`.
- Fixture harness validation in `src-tauri/src/proxy/providers/fixture_harness.rs`.
- Codex preset coverage in `tests/config/codexChatProviderPresets.test.ts`.
- Provider selector search coverage in `tests/components/ProviderPresetSelector.test.tsx`.
- Cross-protocol form warning coverage in `tests/components/CodexFormFields.crossProtocol.test.tsx`.
- Rust tests for Codex-to-Anthropic routing, Codex-to-Gemini routing, streaming bridges, OAuth redaction, and cross-protocol config extraction.

Final verification completed on the branch:

```powershell
pnpm typecheck
pnpm format:check
pnpm test:unit
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Known limits

- Protocol conversion is best-effort. Text, common tool calls, and streaming are covered, but provider-specific reasoning, image, system prompt, and tool semantics can still differ.
- Gemini OAuth depends on valid Google credentials and refresh permissions.
- Per-model routing depends on the requested model name matching a configured route; unmatched requests continue through the normal active-provider path.
- Live validation with real providers requires user-owned credentials and may be affected by upstream rate limits, billing, model availability, and safety filters.
- The branch does not add Anthropic account OAuth as a Codex credential. Codex-to-Claude uses an Anthropic-compatible API key.

## Related documentation

- [User manual: Cross-Protocol Routing](../user-manual/en/4-proxy/4.6-cross-protocol-routing.md)
- [User manual: Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [User manual: App Routing](../user-manual/en/4-proxy/4.2-routing.md)
