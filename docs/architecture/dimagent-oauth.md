# DimAgent managed account integration

DimAgent appears under Account sign-in for every configurable CC Switch client. No external bridge or business API key is required. Clients with a native routing mode use the existing protocol converters. Other clients use an account-bound endpoint in the same CC Switch local server.

```text
Add provider -> Account sign-in -> DimAgent
  -> CN / Global -> Browser authorization (PKCE)
  -> Account -> Model -> API format -> Optional effort -> Save
  -> Native routing clients: enable routing -> existing protocol converters
  -> Other clients: automatic local gateway -> native protocol passthrough
  -> Bound account issuer (CC Switch must remain running)
```

## Authentication boundary

Two public OAuth clients use S256 PKCE, state, and the registered loopback callback `http://127.0.0.1:54322/oauth/callback`. CN uses `https://dimagent.cn`; Global uses `https://dimagent.com`. The Rust manager owns refresh tokens and private atomic credential storage. IPC returns account summaries only. Cancellation and expiry release the listener. A corrupt credential file reports an actionable initialization error without preventing app startup or overwriting data.

Provider metadata records `providerType: dim_oauth` and `authBinding: { source: managed_account, authProvider: dim_oauth, accountId }`. Every request requires this explicit binding. Removing an account never switches an existing provider to another account. Before URL construction, the proxy resolves the account and pins the upstream origin to its issuer, ignoring editable base URLs and full-URL flags. Live client configurations use proxy placeholders, never managed tokens.

## Models and account data

The authenticated model directory is parsed once at the network boundary into `DimModel { id, name, api_formats, efforts, zdr }`. Missing endpoint metadata means OpenAI Chat Completions. Explicit formats take precedence. Full model IDs remain intact. Only top-level `zdr: true` declares ZDR. Model effort options are explicit server declarations; absence means no selector override. A selected effort is saved as `dimOauthEffort` and applied after host protocol conversion. Codex excludes Gemini-native models.

Usage displays actual returned subscription windows and remaining credits; remaining credits include subscription funds and are not an independent quota window. Subscription and pricing links always use the account issuer. This initial integration does not reset quotas or make billable requests during verification.

## Verification

Fixture flows cover PKCE callback validation and cancellation, account persistence, rotated refresh tokens under concurrent requests, CN/global isolation, model directory parsing, and usage data. Provider flows cover native presets, explicit account selection and model/format persistence. Existing routing and protocol conversion tests protect client forwarding. Independent review checks token origin binding, token isolation, listener lifecycle, and native UI behavior.

## Client coverage expansion

The initial preview exposed only Claude Code and Codex. The expanded delivery includes the other configurable clients; a preset is complete only when saving, live configuration, server lifecycle, and authenticated requests work together.

| Client | Allowed directory API formats | Connection path |
| --- | --- | --- |
| Claude Code / Claude Desktop | Chat, Responses, Anthropic, Gemini | Existing Claude routing / conversion; Desktop uses proxy profile |
| Codex / Grok Build | Chat, Responses, Anthropic | Existing Responses routing / conversion |
| Gemini CLI | Gemini | Existing Gemini routing; no invented Gemini-to-Chat conversion |
| MiniMax Code / DeepSeek Harness / Hermes | Chat, Responses, Anthropic | Managed local gateway |
| OpenCode / OpenClaw / Pi / Oh My Pi / Kimi Code | Chat, Responses, Anthropic, Gemini | Managed local gateway |
| Antigravity | Gemini | Managed gateway in the existing private terminal profile; global Gemini settings are untouched |

Gemini-only clients show only models explicitly declaring Gemini support. An empty compatible-model directory is reported as such. Grok Build keeps its existing client profile and maps to the selected upstream model through its established routing projection.

### Shared gateway contract

The existing server adds `/managed/dim/{app}/{provider}/*path`. It resolves exactly that database row, requires `providerType: dim_oauth` and an explicit account binding, and never falls back to another account or the native current-provider setting. The endpoint forwards only the client's selected native protocol; it does not add a second conversion engine or a second listener.

Client base paths follow their native SDK contracts: Chat/Responses use the gateway prefix plus `/v1`; Anthropic in OpenCode uses `/v1` (AI SDK appends `/messages`), while other Anthropic clients use the prefix root; Gemini in OpenCode/Pi/Oh My Pi/OpenClaw uses `/v1beta`, while Kimi and Antigravity use the root because their SDK adds the API version. These are producer rules, not duplicate-path recovery in the relay.

The server records the actual TCP peer. Gateway requests require a loopback peer and a per-app/provider local key, derived with HMAC-SHA256 from a persistent random database setting. Only this local key is written to client configuration; access/refresh tokens stay in the Rust OAuth manager. Authentication precedes body reads. The outbound issuer is resolved from the bound account, redirects are disabled, and inference/model paths are allowlisted. Incoming authentication query parameters and headers never become upstream credentials. Streaming bodies remain streamed.

Saved provider data retains the canonical business configuration with a non-secret `PROXY_MANAGED` placeholder. Existing client writers project a clone into loopback base URL and local key. MiniMax/DeepSeek native-save paths, Pi/OMP metadata preparation, additive writes, and Antigravity terminal/resume paths all preserve the explicit managed binding. No database schema version changes are needed.

Adding/switching to a gateway-backed Dim provider ensures the existing local server is running. Restart restores this need from managed live client rows. Leaving a native application's Routing mode must not stop a server still used by managed clients. Explicit global stop and application exit stop the server; global stop remains a user-controlled action. Gateway configuration requires a loopback or wildcard listen address and uses a literal loopback URL, never an editable LAN client-host override. Successful listen-address/port changes re-project managed live files; a failed change restores the prior server and configuration. Clients caching configuration may need to restart. Antigravity receives the endpoint in its process environment, so an already-open terminal must be reopened after a port change; new and resumed terminal profiles use the new endpoint.

### Directory limits and request options

The model DTO adds nullable `context_window` and `max_output_tokens`, parsed once from the directory's explicit context/output metadata. No guessed context limits are inserted. Kimi requires a positive context window, so models lacking it cannot be selected for Kimi. Other client configurations omit unknown optional limits. Protocol-specific effort overrides share one business transformation with the native forwarder; Gemini does not offer an effort override without a defined protocol mapping.

### Expansion verification

Cover MiniMax and the other specialized forms through their existing save flows, then verify generated live configurations contain only loopback URLs and local keys. Gateway fixture flows cover wrong peer/key, cross-provider key use, missing/deleted accounts, path allowlists, query/header replacement, streaming, restart, and coexisting native routing. Native adapter tests cover Gemini Bearer auth and managed Grok/Desktop projections. Use isolated desktop configuration for UI checks and rebuild the installable macOS preview only after the source is stable. No billable model or quota-reset requests are part of these checks.
