# xAI Grok OAuth Contract

This note records the local implementation contract for adding xAI/Grok OAuth
to CC Switch as a separate provider change.

## Reference Sources

- Hermes Agent xAI Grok OAuth guide:
  https://github.com/NousResearch/hermes-agent/blob/main/website/docs/guides/xai-grok-oauth.md
- OpenClaw xAI provider docs:
  https://docs.openclaw.ai/providers/xai
- OpenCode provider docs:
  https://opencode.ai/docs/providers/
- xAI Grok Build 0.1 announcement:
  https://x.ai/news/grok-build-0-1

## Provider Contract

- CC Switch provider type: `xai_oauth`.
- Display name: `xAI Grok OAuth (SuperGrok / X Premium+)`.
- Base URL: `https://api.x.ai/v1`.
- Upstream host allowlist for OAuth bearer injection: `api.x.ai` only.
- Request path for the default Claude proxy route: `/v1/responses`.
- API format: `openai_responses`.
- Default model: `grok-build-0.1`.
- Auth server family: xAI account OAuth, modeled as managed account auth in
  CC Switch rather than a static API key in provider config.
- Persistent store name: `xai_oauth_auth.json` under the app config directory.
- Static API-key fallback remains the normal xAI API-key provider path, not
  this managed OAuth provider.

## Auth Flow

The first CC Switch implementation should use browser OAuth with PKCE and a
local loopback callback, following the same user expectation as Hermes Agent
and OpenCode. The managed auth command layer should expose login/poll/status,
list, default-account, remove-account, and logout behavior in the same shape as
the existing managed auth providers.

Remote/headless behavior should be explicit in the UI and docs. The planned
fallback is to either print/copy the authorization URL for manual opening or
support a device-code flow if xAI exposes the required public endpoints in a
stable way. If the implementation does not include device-code in the first
PR, the docs must say so plainly and describe local-browser requirements.

Access tokens must be refreshed before expiry and only retrieved at forwarding
time. Provider records and Claude live config must not store real xAI bearer
tokens.

## Entitlement and 403 Risk

The reference tools document that OAuth login can succeed while inference later
fails with xAI-side eligibility or entitlement errors. CC Switch should surface
that as an account/subscription/API-access problem, not as a provider loss,
configuration corruption, or generic proxy failure. The docs should recommend
the API-key path when OAuth access is not enabled for the account.

## Code Surfaces To Change

- Backend managed auth:
  - `src-tauri/src/proxy/providers/xai_oauth_auth.rs`
  - `src-tauri/src/proxy/providers/mod.rs`
  - `src-tauri/src/commands/auth.rs`
  - `src-tauri/src/lib.rs`
- Provider metadata and safe routing:
  - `src-tauri/src/provider.rs`
  - `src-tauri/src/proxy/forwarder.rs`
  - `src-tauri/src/proxy/providers/claude.rs`
  - `src-tauri/src/proxy/providers/mod.rs`
  - `src-tauri/src/claude_desktop_config.rs`
  - `src-tauri/src/services/proxy.rs`
- Frontend provider UX:
  - `src/config/constants.ts`
  - `src/config/claudeProviderPresets.ts`
  - `src/config/claudeDesktopProviderPresets.ts`
  - `src/lib/api/auth.ts`
  - `src/types.ts`
  - `src/components/providers/forms/ProviderForm.tsx`
  - `src/components/providers/forms/ClaudeDesktopProviderForm.tsx`
  - related managed-auth hooks/tests as needed.
- Tests and docs:
  - targeted Rust unit/integration tests for auth storage and proxy injection
  - targeted frontend tests for presets and OAuth-required UI
  - `docs/guides/xai-grok-oauth-provider-guide-en.md`
  - `docs/pull-requests/xai-grok-oauth-pr.md`

## Surfaces Not To Change

- Do not merge this work with the cross-protocol PR branch.
- Do not rewrite existing `github_copilot` or `codex_oauth` behavior except to
  share small helper logic when tests prove byte-level compatibility.
- Do not alter existing user provider rows, live provider selection, or Claude
  Desktop config preservation logic except to permit the new `xai_oauth`
  managed-auth case.
- Do not add real tokens, auth codes, refresh tokens, client secrets, or
  account-specific values to source, tests, docs, logs, or snapshots.

## Security Invariants

- Only inject `Authorization: Bearer <access_token>` for `https://api.x.ai`
  upstreams resolved from an `xai_oauth` provider.
- Reject placeholders such as `grok-oauth-placeholder`, `xai-oauth-placeholder`,
  or proxy-managed sentinel values before forwarding upstream.
- Redact access tokens, refresh tokens, ID tokens, auth codes, client secrets,
  and token endpoint responses from debuggable structures and error messages.
- Automated tests must use fake token values only and must not require live
  xAI network access.
