# xAI Grok OAuth Provider Guide

> Applies to the xAI Grok OAuth provider added to CC Switch as a managed-auth Claude/Claude Desktop provider. The provider is intended for accounts with xAI OAuth access to Grok Build, such as eligible SuperGrok or X Premium+ accounts. Availability is controlled by xAI; a successful login does not guarantee inference access.

## What this provider does

The `xAI Grok OAuth (SuperGrok / X Premium+)` preset lets Claude Code or Claude Desktop route requests to xAI through CC Switch without storing a static xAI API key in the provider record.

The preset uses these defaults:

- Provider type: `xai_oauth`
- Base URL: `https://api.x.ai/v1`
- Upstream request path: `/v1/responses`
- API format: OpenAI Responses
- Default model: `grok-build-0.1`
- Managed auth store: `xai_oauth_auth.json` under the CC Switch app config directory

The real OAuth access token is resolved only when the local route forwards a request. Claude live config receives `PROXY_MANAGED` placeholders instead of real xAI tokens.

## Prerequisites

Prepare these before adding the provider:

- CC Switch running with the local routing service available.
- Claude Code or Claude Desktop configured in CC Switch.
- An xAI account that is eligible for Grok Build OAuth/API access.
- A local browser on the same machine as CC Switch.

This PR implements the browser OAuth/PKCE provider contract. It does not add a device-code flow. Headless or remote machines should use the copied authorization URL in a browser that can complete the local callback, or use the static xAI API-key provider path if OAuth cannot be completed on that machine.

## Add the provider

Open the Claude or Claude Desktop provider form and choose the built-in `xAI Grok OAuth (SuperGrok / X Premium+)` preset.

The preset already fills the xAI endpoint and default model. You do not need to enter an API key for this provider. Instead, use the OAuth section in the form:

1. Click the xAI login button.
2. Complete the browser login/consent flow with your xAI account.
3. Select the logged-in xAI account in the provider form.
4. Save the provider.
5. Enable local routing for the app and switch to the xAI OAuth provider.
6. Restart Claude Code or Claude Desktop if the client already had an older config loaded.

When saved, the provider records an auth binding with `authProvider = "xai_oauth"` and the selected managed account id. It does not save a bearer token into the provider settings.

## Live config and routing behavior

When the provider is enabled for Claude, CC Switch writes Claude-compatible live config with the xAI model/base URL fields and managed placeholders:

- `ANTHROPIC_BASE_URL` points to the configured xAI route/base URL.
- `ANTHROPIC_MODEL` defaults to `grok-build-0.1`.
- `ANTHROPIC_API_KEY` and, for non-Copilot managed auth, `ANTHROPIC_AUTH_TOKEN` use `PROXY_MANAGED`.
- Real OAuth tokens are read from the encrypted managed-auth store only at forwarding time.

The proxy only injects an xAI OAuth bearer token when the resolved upstream is `https://api.x.ai`. Requests to any other host keep the placeholder guard and fail before a managed token can be sent.

## Storage and refresh

The xAI OAuth manager stores account metadata and refresh material in `xai_oauth_auth.json` in the CC Switch app config directory. Debug output redacts access tokens, refresh tokens, ID tokens, auth codes, and token endpoint responses.

Before forwarding a request, CC Switch checks whether the selected account has a usable access token. If the token is close to expiry, the manager refreshes it with the stored refresh token. If no managed account exists, or the selected account has been removed, forwarding returns a managed-auth error instead of sending a placeholder upstream.

## 403 and entitlement errors

xAI may allow OAuth login while still rejecting Grok Build inference with a `403` or related entitlement error. This usually means the account does not have the required subscription, API entitlement, region access, or feature rollout.

If this happens:

- Confirm the account can access Grok Build through xAI's supported clients.
- Try a small request after re-login to rule out an expired session.
- Use the normal xAI API-key provider path if OAuth access is not enabled for the account.

CC Switch should treat these failures as xAI account/API eligibility problems, not as provider deletion, config corruption, or a missing local provider.

## Safety properties

The implementation adds these safeguards:

- Claude live config never stores real xAI OAuth tokens.
- Managed placeholders such as `PROXY_MANAGED`, `grok-oauth-placeholder`, and `xai-oauth-placeholder` are rejected before upstream forwarding.
- xAI token injection is host-pinned to `https://api.x.ai`.
- Existing providers are preserved when adding, saving, or switching the xAI OAuth provider.
- Frontend validation requires a logged-in xAI managed account before saving the preset.

## Manual validation checklist

Use this checklist before publishing a release or confirming access for a real account:

- Add the `xAI Grok OAuth (SuperGrok / X Premium+)` preset from the Claude provider form.
- Complete browser OAuth with a test xAI account that is expected to have Grok Build access.
- Save the provider without entering any static API key.
- Enable local routing for Claude and switch to the xAI OAuth provider.
- Confirm Claude live config contains `PROXY_MANAGED` placeholders and no real xAI bearer token.
- Send a small Claude Code request and confirm the proxy forwards to `https://api.x.ai/v1/responses`.
- Confirm a missing or removed xAI account returns a managed-auth error before any upstream request.
- If xAI returns `403`, verify the account entitlement before treating it as a CC Switch routing bug.
- Switch away from the provider and confirm other Claude providers are still present.

## References

- [xAI Grok Build 0.1 announcement](https://x.ai/news/grok-build-0-1)
- [Hermes Agent xAI Grok OAuth guide](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/guides/xai-grok-oauth.md)
- [OpenClaw xAI provider docs](https://docs.openclaw.ai/providers/xai)
- [OpenCode provider docs](https://opencode.ai/docs/providers/)
- [CC Switch xAI Grok OAuth implementation contract](../research/xai-grok-oauth-contract.md)
