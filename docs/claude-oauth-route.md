# Routing Claude Code's subscription login (Claude OAuth)

## Problem

CC Switch routes Claude Code through the local proxy in **route** mode. Every
other app can route to a provider, but an official Claude subscription could not:
the proxy had no way to authenticate to `api.anthropic.com` without an API key.

Concretely, a Claude card aimed at `https://api.anthropic.com` with no key failed
with `配置错误: Claude Provider 缺少 base_url 配置` / `x-api-key header is
required`, the circuit breaker churned Open↔HalfOpen, and every request fell
through to the next provider in the failover queue.

The official docs describe this as a hard rule (「官方订阅不经过路由」). It is not
a rule, though — it is a **missing code path**: the proxy knows how to inject
managed credentials for `codex_oauth`, `github_copilot` and `xai_oauth`, but
nothing reads the Claude Code sign-in that already lives on the machine.

## What this adds

A fourth managed-credential kind, **`claude_oauth`**, mirroring `xai_oauth`:

| Layer | Addition |
| --- | --- |
| `provider_kind.rs` | `ProviderKind::ClaudeOAuth` ⇐ `meta.providerType == "claude_oauth"` |
| `provider.rs` | `Provider::is_claude_oauth()` (+ `is_anthropic_api_url` helper) |
| `proxy/providers/auth.rs` | `AuthStrategy::ClaudeOAuth` |
| `proxy/providers/mod.rs` | `ProviderType::ClaudeOAuth`, `ANTHROPIC_API_BASE_URL`, `pub mod claude_oauth_auth` |
| `proxy/providers/claude_oauth_auth.rs` | reads the access token from Claude Code's own credential store |
| `proxy/providers/claude.rs` | `provider_type` / `extract_base_url` / `extract_auth` / `get_auth_headers` wiring |
| `proxy/forwarder.rs` | injects the token per request; adds the `oauth-2025-04-20` beta |
| `src/config/constants.ts` | registers `claude_oauth` as a managed OAuth type |

### Which cards qualify

`Provider::is_claude_oauth()` is true when **any** of these holds:

1. the card declares `meta.providerType = "claude_oauth"`; or
2. the card is the official Claude card (`category: "official"`, no base URL); or
3. the card carries no credential of its own and its `ANTHROPIC_BASE_URL` host is
   `api.anthropic.com`.

`PROXY_MANAGED` — the placeholder CC Switch itself writes into the client config
while routing — is explicitly **not** treated as a credential.

Case 3 is what makes a plain `default` card work with no manual step. Case 2/3 are
guarded on the upstream host, so other apps' official cards (Codex →
`chatgpt.com`, Copilot → `githubcopilot.com`) never match.

### How the token is read

`claude_oauth_auth::access_token_for(provider)` reads
`claudeAiOauth.accessToken` through the credential store CC Switch already uses
for account switching:

- macOS: the `Claude Code-credentials` keychain item;
- other platforms: `.credentials.json`, with the keychain as the macOS fallback.

If the card is bound to a managed account (`meta.authBinding.authProvider ==
"claude_official"`), that account's slot is used; otherwise the machine's default
`~/.claude` sign-in is.

The forwarder replaces the client's placeholder `Authorization` header with
`Authorization: Bearer <token>` and folds `oauth-2025-04-20` into `anthropic-beta`
(alongside the existing `claude-code-20250219` marker). Anthropic accepts a
subscription token only for requests that look like Claude Code — in route mode
the client *is* Claude Code, so its system prompt already satisfies that.

## Limitations

- **No refresh.** CC Switch does not exchange the refresh token; Claude Code owns
  that cycle and writes back to the same store. An expired token surfaces upstream
  as a 401 and the existing provider failover takes over.
- **Route mode only.** The token is injected by the local proxy, so the card must
  be the route target or sit in the failover queue. In direct mode Claude Code uses
  its own login as before.
- Reading the token is best-effort per request; a missing sign-in is an
  authentication error, not a crash.

## Setting up a route that prefers Claude

1. Quit CC Switch (it owns `~/.cc-switch/cc-switch.db`).
2. Mark the card explicitly (optional — case 3 above already covers it):

   ```sql
   UPDATE providers
      SET meta = json_set(COALESCE(meta, '{}'), '$.providerType', 'claude_oauth')
    WHERE app_type = 'claude' AND id = 'default';
   ```

3. Reopen CC Switch, put the card first in the failover queue, and route the app
   to it. Requests hit real Anthropic first; a 429/5xx falls through to the next
   provider.
