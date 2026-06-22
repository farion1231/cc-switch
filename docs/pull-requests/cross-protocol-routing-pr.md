# Pull Request Body: Cross-Protocol Routing

Copy this body into the GitHub PR opened from `garciarsdiego:codex/cross-protocol-supergoal`.

## Summary / 概述

Adds local cross-protocol routing so Codex and Claude can use selected upstream providers even when the client and provider speak different API formats.

This PR adds:

- Codex -> Anthropic Messages routing for Claude/Anthropic-compatible providers.
- Codex -> Gemini Native routing with Gemini API key, `ya29` access token, or Gemini CLI `oauth_creds.json`.
- Claude Code -> Gemini Native routing using the existing local proxy/takeover flow.
- Streaming conversion for Anthropic SSE and Gemini SSE into OpenAI Chat/Codex-compatible streams.
- Gemini OAuth hardening: trimmed credential parsing, serialized refreshes, and redacted debug output.
- Codex provider presets for `Claude / Anthropic via Codex` and `Gemini Native OAuth/API key via Codex`.
- UI warnings and local-routing badges for Codex Anthropic/Gemini bridge providers.
- Golden protocol fixtures, fixture harness coverage, frontend tests, Rust tests, and user documentation.

Why:

Codex, Claude, and Gemini-compatible providers increasingly expose different request/response protocols. Without a local conversion layer, users must either avoid otherwise valid providers or manually configure endpoints in ways that fail for streaming, model routes, or response parsing. This keeps credentials in CC Switch provider config and lets the local proxy perform the conversion.

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
| Codex providers only covered OpenAI Responses/OpenAI Chat-style routing. | Codex can select Anthropic and Gemini Native bridge presets that require local routing. |
| Gemini OAuth debug output could expose credential-like values in derived debug output. | Gemini OAuth debug output redacts access tokens, refresh tokens, and client secrets. |
| Cross-protocol setup was not documented. | User manual and fork guide document setup, credentials, validation, and limits. |

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

- Codex preset search and provider-form warnings.
- Codex API format persistence for `anthropic` and `gemini_native`.
- Fixture-based protocol conversion coverage.
- Codex Responses endpoint detection and adapter selection.
- Anthropic SSE conversion.
- Gemini SSE conversion for the Codex bridge.
- Gemini OAuth credential parsing, refresh serialization, and debug redaction.
- Claude adapter support for Codex-style cross-protocol auth/config.

Manual live validation requires real provider credentials. Suggested smoke tests:

1. Codex -> Anthropic:
   - Add `Claude / Anthropic via Codex`.
   - Enter an Anthropic-compatible API key.
   - Start local proxy and enable Codex takeover.
   - Run a short Codex prompt and a longer streaming prompt.

2. Codex -> Gemini Native:
   - Add `Gemini Native OAuth/API key via Codex`.
   - Enter a Gemini API key, `ya29` token, or Gemini CLI `oauth_creds.json`.
   - Start local proxy and enable Codex takeover.
   - Run a short Codex prompt and a longer streaming prompt.

3. Claude Code -> Gemini Native:
   - Add or edit a Claude provider using Gemini Native format.
   - Enter a Gemini API key, `ya29` token, or Gemini CLI `oauth_creds.json`.
   - Start local proxy and enable Claude takeover.
   - Run a short Claude Code prompt and a longer streaming prompt.

## Compatibility and risk / 兼容性与风险

- Direct mode cannot perform these conversions; local proxy and app takeover are required.
- Protocol conversion is best-effort. Text, common tools, and streaming are covered, but provider-specific reasoning, images, system prompts, and unusual tool schemas can still differ.
- Gemini OAuth depends on valid Google credentials and refresh permissions.
- Upstream provider rate limits, billing behavior, safety filters, and model availability still apply.
- This PR does not add Anthropic account OAuth as a Codex credential. Codex -> Claude uses Anthropic-compatible API keys.
