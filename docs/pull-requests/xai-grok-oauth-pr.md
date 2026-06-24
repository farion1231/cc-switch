## Summary / 概述

Add an xAI Grok OAuth managed provider for Claude Code and Claude Desktop.

This PR adds a new `xai_oauth` provider type and preset named `xAI Grok OAuth (SuperGrok / X Premium+)`, routing Claude-compatible requests to xAI's OpenAI Responses endpoint with the default `grok-build-0.1` model. OAuth tokens are managed by CC Switch and injected only by the local proxy at request time, so Claude live config and provider settings do not store real xAI bearer tokens.

Main changes:

- Add an xAI OAuth manager with PKCE/browser OAuth contract, account storage, token refresh helpers, redacted debug output, and managed-auth command integration.
- Add provider metadata, Claude adapter routing, xAI Responses URL construction, and host-pinned token injection for `https://api.x.ai`.
- Add frontend presets and form UX for Claude/Claude Desktop, including managed account selection and xAI entitlement guidance.
- Add regression coverage for preset visibility, OAuth form behavior, provider preservation, placeholder live config, placeholder rejection, and xAI host pinning.
- Add user-facing documentation for setup, defaults, storage/refresh behavior, entitlement/403 risk, and security properties.

## Related Issue / 关联 Issue

Fixes #

## Screenshots / 截图

Not included. The change is mostly provider/backend behavior plus a small provider-form OAuth section that reuses existing managed-auth UI patterns.

| Before / 修改前 | After / 修改后 |
|-----------------|---------------|
| xAI Grok OAuth was not available as a managed provider. | xAI Grok OAuth is available from the Claude and Claude Desktop provider presets with managed OAuth account binding. |

## Validation / 验证

- `pnpm typecheck`
- `pnpm format:check`
- `pnpm test:unit -- tests/components/ClaudeFormFields.test.tsx tests/components/ClaudeDesktopProviderForm.test.tsx`
- `pnpm test:unit -- tests/components/ClaudeFormFields.test.tsx tests/components/ProviderPresetSelector.test.tsx`
- `cargo fmt --check`
- `cargo test --lib xai_oauth -j 1`
- `cargo test --lib managed_account -j 1`
- `cargo test --test provider_service xai_oauth -j 1`
- `cargo clippy -j 1 -- -D warnings`

## Security Notes / 安全说明

- Real xAI OAuth access tokens are not written to Claude live config.
- Claude live config uses `PROXY_MANAGED` placeholders for managed auth.
- Placeholder/sentinel auth values are rejected before upstream forwarding.
- xAI bearer injection is restricted to `https://api.x.ai`.
- Tests use fake tokens only and do not require live xAI network access.
- OAuth login may still fail at inference time with xAI-side `403` entitlement errors; the docs describe API-key fallback for accounts without OAuth/API access.

## Checklist / 检查清单

- [x] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [x] `pnpm format:check` passes / 通过代码格式检查
- [x] `cargo clippy` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [x] Updated i18n files if user-facing text changed / 如修改了用户可见文本，已更新国际化文件
