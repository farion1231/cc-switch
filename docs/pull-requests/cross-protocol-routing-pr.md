# Pull Request Body: Provider Routing and Cross-Protocol Routing

Copy this body into the GitHub PR opened from `garciarsdiego:codex/cross-protocol-supergoal`.

## Summary / 概述

Adds provider-routing and cross-protocol routing improvements so CC Switch can route Claude Code requests by model and allow Codex/Claude to use selected upstream providers even when the client and provider speak different API formats.

This PR adds:

- English-default UI behavior and updated README/i18n coverage.
- A searchable alphabetical Add Provider dropdown replacing the previous preset button grid.
- Per-model provider and target-model routing for Claude Code, including settings UI, frontend API/query bindings, Tauri commands, database schema/DAO, and proxy-time provider/model routing.
- Reverse OpenAI -> Anthropic transform foundation for Codex -> Claude-style routing.
- Gemini Google OAuth refresh support at request time.
- Codex -> Anthropic Messages routing for Claude/Anthropic-compatible providers.
- Codex -> Gemini Native routing with Gemini API key, `ya29` access token, or Gemini CLI `oauth_creds.json`.
- Claude Code -> Gemini Native routing using the existing local proxy/takeover flow.
- Streaming conversion for Anthropic SSE and Gemini SSE into OpenAI Chat/Codex-compatible streams.
- Gemini OAuth hardening: trimmed credential parsing, serialized refreshes, and redacted debug output.
- Codex provider presets for `Claude / Anthropic via Codex` and `Gemini Native OAuth/API key via Codex`.
- UI warnings and local-routing badges for Codex Anthropic/Gemini bridge providers.
- Partner/provider metadata updates carried from the fork branch, including GLM 5.2 pricing and partner badge/link updates.
- Golden protocol fixtures, fixture harness coverage, frontend tests, Rust tests, and user documentation.

Why:

Claude Code users often need different providers or target models for different requested models, while Codex, Claude, and Gemini-compatible providers increasingly expose different request/response protocols. Without model-aware routing and a local conversion layer, users must either avoid otherwise valid providers or manually configure endpoints in ways that fail for streaming, model routes, or response parsing. This keeps credentials in CC Switch provider config and lets the local proxy choose the correct provider/model and perform the required conversion.

Additional documentation:

- `docs/user-manual/en/4-proxy/4.6-cross-protocol-routing.md`
- `docs/guides/cross-protocol-routing-guide-en.md`

## Related Issue / 关联 Issue

Fixes #

## Screenshots / 截图

No screenshots included in this PR body. The change is mostly proxy/backend behavior plus provider-form warnings. The new UX can be checked by opening the Codex provider add flow and searching for:

- `Claude / Anthropic via Codex`
- `Gemini Native OAuth/API key via Codex`

| Before / 修改前 | After / 修改后 |
|-----------------|---------------|
| Provider presets were shown as a large button grid. | Provider presets are searchable in an alphabetical dropdown. |
| Claude Code used the active provider path without per-model provider/model overrides. | Claude Code can route configured requested models to specific providers and target models. |
| Codex providers only covered OpenAI Responses/OpenAI Chat-style routing. | Codex can select Anthropic and Gemini Native bridge presets that require local routing. |
| Gemini OAuth debug output could expose credential-like values in derived debug output. | Gemini OAuth debug output redacts access tokens, refresh tokens, and client secrets. |
| Cross-protocol and per-model setup were not documented together. | User manual and fork guide document setup, credentials, validation, architecture, and limits. |

## Checklist / 检查清单

- [x] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [x] `pnpm format:check` passes / 通过代码格式检查
- [x] `cargo clippy` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [x] Updated i18n files if user-facing text changed / 如修改了用户可见文本，已更新国际化文件

Additional validation run:

- [x] `pnpm test:unit`
- [x] `cargo fmt --check`
- [x] `cargo test`
- [x] `git diff --check`
- [x] Security/debug grep over changed `src`, `src-tauri`, `tests`, and `docs` found no debug prints, TODO/FIXME markers, or real token-looking strings.

## Testing notes / 测试说明

Automated tests cover:

- Provider preset selector search and action visibility.
- Per-model provider/model routing persistence and proxy behavior through the Rust suite.
- Codex preset search and provider-form warnings.
- Codex API format persistence for `anthropic` and `gemini_native`.
- Reverse OpenAI -> Anthropic transforms.
- Fixture-based protocol conversion coverage.
- Codex Responses endpoint detection and adapter selection.
- Anthropic SSE conversion.
- Gemini SSE conversion for the Codex bridge.
- Gemini OAuth credential parsing, refresh serialization, and debug redaction.
- Claude adapter support for Codex-style cross-protocol auth/config.

Manual live validation requires real provider credentials. Suggested smoke tests:

1. Claude Code per-model routing:
   - Add at least two Claude-compatible providers.
   - Open the proxy/model routing settings.
   - Route one requested model to a different provider and target model.
   - Start local proxy and enable Claude takeover.
   - Run Claude Code with the routed model and confirm the selected upstream provider receives the request.

2. Codex -> Anthropic:
   - Add `Claude / Anthropic via Codex`.
   - Enter an Anthropic-compatible API key.
   - Start local proxy and enable Codex takeover.
   - Run a short Codex prompt and a longer streaming prompt.

3. Codex -> Gemini Native:
   - Add `Gemini Native OAuth/API key via Codex`.
   - Enter a Gemini API key, `ya29` token, or Gemini CLI `oauth_creds.json`.
   - Start local proxy and enable Codex takeover.
   - Run a short Codex prompt and a longer streaming prompt.

4. Claude Code -> Gemini Native:
   - Add or edit a Claude provider using Gemini Native format.
   - Enter a Gemini API key, `ya29` token, or Gemini CLI `oauth_creds.json`.
   - Start local proxy and enable Claude takeover.
   - Run a short Claude Code prompt and a longer streaming prompt.

## Compatibility and risk / 兼容性与风险

- Direct mode cannot perform these conversions; local proxy and app takeover are required.
- Per-model routing only applies when a requested model matches a configured route; otherwise requests use the normal active provider path.
- Protocol conversion is best-effort. Text, common tools, and streaming are covered, but provider-specific reasoning, images, system prompts, and unusual tool schemas can still differ.
- Gemini OAuth depends on valid Google credentials and refresh permissions.
- Upstream provider rate limits, billing behavior, safety filters, and model availability still apply.
- This PR does not add Anthropic account OAuth as a Codex credential. Codex -> Claude uses Anthropic-compatible API keys.
