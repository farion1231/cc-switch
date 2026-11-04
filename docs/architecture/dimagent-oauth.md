# DimAgent managed account integration

DimAgent appears under Account sign-in for Claude Code and Codex. No external bridge or API key is required. Requests use CC Switch routing and existing protocol converters.

```text
Add provider -> Account sign-in -> DimAgent
  -> CN / Global -> Browser authorization (PKCE)
  -> Account -> Model -> API format -> Optional effort -> Save
  -> Enable routing -> Client request -> Bound account issuer
```

## Authentication boundary

Two public OAuth clients use S256 PKCE, state, and the registered loopback callback `http://127.0.0.1:54322/oauth/callback`. CN uses `https://dimagent.cn`; Global uses `https://dimagent.com`. The Rust manager owns refresh tokens and private atomic credential storage. IPC returns account summaries only. Cancellation and expiry release the listener. A corrupt credential file reports an actionable initialization error without preventing app startup or overwriting data.

Provider metadata records `providerType: dim_oauth` and `authBinding: { source: managed_account, authProvider: dim_oauth, accountId }`. Every request requires this explicit binding. Removing an account never switches an existing provider to another account. Before URL construction, the proxy resolves the account and pins the upstream origin to its issuer, ignoring editable base URLs and full-URL flags. Live client configurations use proxy placeholders, never managed tokens.

## Models and account data

The authenticated model directory is parsed once at the network boundary into `DimModel { id, name, api_formats, efforts, zdr }`. Missing endpoint metadata means OpenAI Chat Completions. Explicit formats take precedence. Full model IDs remain intact. Only top-level `zdr: true` declares ZDR. Model effort options are explicit server declarations; absence means no selector override. A selected effort is saved as `dimOauthEffort` and applied after host protocol conversion. Codex excludes Gemini-native models.

Usage displays actual returned subscription windows and remaining credits; remaining credits include subscription funds and are not an independent quota window. Subscription and pricing links always use the account issuer. This initial integration does not reset quotas or make billable requests during verification.

## Verification

Fixture flows cover PKCE callback validation and cancellation, account persistence, rotated refresh tokens under concurrent requests, CN/global isolation, model directory parsing, and usage data. Provider flows cover native presets, explicit account selection and model/format persistence. Existing routing and protocol conversion tests protect client forwarding. Independent review checks token origin binding, token isolation, listener lifecycle, and native UI behavior.
