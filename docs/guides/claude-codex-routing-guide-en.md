# Using GPT Models in Claude Code with CC Switch

> Applies to CC Switch 3.17.0 and later. (Both integration methods in this guide existed in earlier versions, but the gpt-5.6 preset and the client-identity fix landed in 3.17.0; on older versions, requesting new models like `gpt-5.6-luna` falsely returns 404.) This guide is compiled from the repository's documentation and code, and all sample data has been de-identified.

## Why local routing is needed

Claude Code targets the Anthropic Messages protocol — that is, `/v1/messages` — whereas the upstreams for Codex-family models, whether the OpenAI Responses API exposed by a third-party gateway or the Codex service behind a ChatGPT subscription, all speak the Responses protocol. The two protocols use completely different request bodies, streaming events, and response structures, so putting such an endpoint directly into Claude Code's config leaves the upstream receiving a `/v1/messages` request it doesn't recognize — which can only fail.

CC Switch's approach is to keep Claude Code always connected to the local route and still sending requests as Anthropic Messages; once the route detects that the active provider is Responses-format, it converts the request into Responses for the upstream, then converts the response back into the Messages shape it returns to Claude Code — tool calls, images, PDFs, and thinking configuration are all within the conversion scope.

This guide covers both integration methods:

- **Method 1 (API Key)**: you have a gateway endpoint and key compatible with the OpenAI Responses API, and you want to run the GPT-family models behind it inside Claude Code.
- **Method 2 (ChatGPT subscription)**: you have a ChatGPT Plus/Pro subscription and use its quota directly by signing in through Codex OAuth — no API key needed at any point.

The chain has four main steps:

1. When Claude Code is taken over, `ANTHROPIC_BASE_URL` in `~/.claude/settings.json` is written as the local route address (default `http://127.0.0.1:15721`), the auth entry keeps only a placeholder, and real credentials never enter the live config.
2. The provider's `API Format` is set to OpenAI Responses, telling the route that the real upstream speaks the Responses protocol.
3. The route converts the `/v1/messages` request into a Responses request body for the upstream; Method 2 additionally carries the OAuth token and the official client identity to reach ChatGPT's Codex service.
4. After the upstream responds, the route converts the Responses JSON/SSE back into the Messages shape Claude Code understands.

![The Needs Routing marker on a GPT provider in the Claude provider list](../images/claude-codex-routing/01-claude-providers-require-routing.png)

## Prerequisites

- CC Switch installed and able to start (3.17.0 or later; see the version note at the top for why).
- Claude Code installed and run at least once.
- For Method 1: a service endpoint compatible with the OpenAI Responses API and its API Key; follow the gateway's documentation for the endpoint and model names. Note it's the **Responses API**, not Chat Completions; a gateway that only offers the Chat format still works — see the `API Format` note in Step 1.
- For Method 2: a ChatGPT Plus/Pro subscription account.

## Step 1: Add a provider

### Method 1: Third-party Responses gateway (API Key)

Open CC Switch, switch to the top-level `Claude Code` tab, click the plus button in the upper-right corner to add a provider, keep the default `Custom Configuration`, then fill in:

- **Provider Name**: anything you like, e.g. `GPT Gateway`.
- **API Key**: your gateway key. The real key is stored only in CC Switch and injected by the local route when forwarding.
- **API Endpoint**: just the gateway's service root, e.g. `https://gpt-gateway.example.com`, without a trailing slash — the route sends requests to the gateway's Responses endpoint (`/v1/responses`) automatically. When the gateway path is unusual, turn on the `Full URL` toggle next to it and paste the complete endpoint verbatim.

Then expand `Advanced Options`:

- **API Format**: change from the default `Anthropic Messages (Native)` to **`OpenAI Responses API (Requires routing)`**. If the gateway only offers the Chat Completions protocol, choose `OpenAI Chat Completions (Requires routing)` here instead; every other step is identical.
- **Auth Field**: keep the default `ANTHROPIC_AUTH_TOKEN (Default)`; the route sends `Authorization: Bearer <key>` to the upstream — exactly the auth header an OpenAI-compatible gateway expects. Unless the gateway's documentation explicitly requires `x-api-key`, don't switch to `ANTHROPIC_API_KEY`; the wrong choice typically shows up as 401/403.
- **Model Mapping**: map Claude Code's model roles to the real models the gateway recognizes. **At minimum, fill in the `Default fallback model`** (e.g. `gpt-5.6`, per the gateway's documentation) — if left empty, unmatched requests pass through to the upstream under the original Claude model name and error out, while roles you haven't configured individually fall back to it. For finer control, specify per row: put your main model in `Sonnet`/`Opus`, and a cheap, fast model in `Haiku` (Claude Code's background sub-tasks use this tier). The `Display name` only affects what shows in the `/model` menu; leave it empty to show the real model name directly.
- **Declare 1M**: the `1M` checkbox on each model-mapping row declares to Claude Code that the tier supports a 1M context. Check it only when the gateway truly serves that model with a window of one million tokens or more (e.g. a gateway offering gpt-5.6 at its API spec); otherwise long conversations will error out at the upstream's real ceiling.

![Method 1: in Advanced Options, set API Format to OpenAI Responses, keep the default auth field, and map to the upstream's real models](../images/claude-codex-routing/02-responses-provider-form.png)

After saving, a `Needs Routing` marker appears on the card — providers like this only work while local routing is running.

### Method 2: ChatGPT subscription (Codex OAuth)

Again on the `Claude Code` tab, click the plus button and pick the **`Codex`** preset with the OpenAI icon from the preset list — it appearing under the Claude Code tab is not a mistake; this preset is built precisely for "using a ChatGPT subscription inside Claude Code":

- **No API Key and no address needed** — requests always go to ChatGPT's Codex service, so the address field in the form needs no changes.
- Click **`Sign in with ChatGPT`**. This is a device-code flow: CC Switch opens the browser automatically and copies the verification code to the clipboard; paste the code on the browser page to complete authorization, while the app shows `Waiting for authorization...`.
- After a successful sign-in, `Auth status` shows the signed-in account (email). Multiple accounts are supported: you can `Add another account`, `Set as default`, or pin a specific account to this provider; day-to-day management can also go through `Settings` → `OAuth Authentication Center`.
- **FAST mode**: an optional toggle; when on, requests carry `service_tier="priority"` for lower latency but consume ChatGPT quota at a higher rate. Keep it off by default.
- The model tiers are pre-filled: `Sonnet`/`Opus` map to `gpt-5.6`, and `Haiku` maps to `gpt-5.6-luna` (used for background sub-tasks — faster and lighter on quota).

![Method 2: ChatGPT login status and account management in the Codex preset](../images/claude-codex-routing/03-codex-oauth-form.png)

The login credentials are stored in `~/.cc-switch/codex_oauth_auth.json` (not `~/.codex/`), independent of the Codex CLI's own login; the token refreshes automatically before it expires.

## Step 2: Enable local routing and take over Claude Code

Go to the `Routing` page in Settings, expand `Local Routing`, and complete two toggles:

1. Turn on the `Routing Master Switch` to start the local service (the first time you enable it, an explanatory confirmation dialog appears). The default address is `127.0.0.1:15721`.
2. Turn on `Claude Code` under `Routing Enabled`. If you only want Claude Code to use routing, leave the other apps off.

After takeover, CC Switch points Claude Code's live config at the local route, with only a placeholder in the auth entry; both Method 1's gateway key and Method 2's OAuth token are injected by the local route on forward.

> **Note**: the live config is read when the Claude Code process starts. After you first enable takeover (or disable it to restore a direct connection), if Claude Code is already running, open a new terminal session. Afterward, switching providers in routing mode is a hot switch and needs no further restart.

## Step 3: Switch providers and verify

Return to the Claude Code provider list and click `Enable` on the target provider. If routing isn't running, CC Switch shows "This provider uses OpenAI Responses API format, requires the routing service to work properly. Start routing first." — this notice doesn't block the switch, but with routing off the request is bound to fail, so go back to Step 2 and turn it on.

Inside Claude Code you can verify step by step:

- Open a new session and use `/model` to view the model menu: each tier shows the display name from the model mapping (by default, Method 2 shows `gpt-5.6` and `gpt-5.6-luna`). A few spots in the UI may still show Claude-family model names — those are the internal role aliases the routing takeover uses; this is normal, and the `/model` menu and usage dashboard are authoritative.
- Send a small question and watch the `Current Provider` on the Settings → Routing page change to your provider and `Total Requests` start to climb.
- In the usage dashboard, these requests show under the upstream's real models: a tier mapped to `gpt-5.6` resolves to the Sol tier and displays as `GPT-5.6 Sol`, while `gpt-5.6-luna` displays as `GPT-5.6 Luna`; you can filter by provider to reconcile token usage.
- The Method 2 provider card also shows subscription quota: utilization and reset countdowns for the 5-hour and 7-day windows, drawn from the ChatGPT account itself and shared with the official Codex client.

## Capabilities and known limitations

- **Prompt caching is automatic**: the route injects a stable `prompt_cache_key` per session, and together with OpenAI's automatic prefix caching, long conversations don't resend everything at full price each turn — no configuration needed.
- **Thinking is mapped to reasoning effort**: Claude Code's thinking toggle and thinking level are mapped to GPT's `reasoning.effort` (low/medium/high); GPT's reasoning content round-trips across turns intact in encrypted form, so multi-turn reasoning coherence is unaffected by the conversion. Method 2 also accesses in stateless mode (`store:false`), leaving no conversation stored on OpenAI's servers.
- **Tools and multimodal are fully converted**: multi-turn tool calls, image inputs, and PDF inputs are all fully converted.
- **Context is managed against a 200K window**: Claude Code auto-compacts routed providers against a default 200K window. When the upstream's real window is larger (e.g. gpt-5.6 on ChatGPT's Codex service is 372K), anything beyond 200K currently goes unused — compaction triggers early, which is conservative but safe. The only switch to push past 200K today is the `1M` checkbox in the model mapping (a strict 1M declaration), for use only with Method 1 and only when the upstream truly serves the model at 1M or more; Method 2's upstream ceiling is 372K, short of 1M, so checking it would instead make long conversations error out at the upstream's real ceiling — keep it at the default.
- **Output ceiling**: for Method 2, the output ceiling is controlled by the ChatGPT server (the `max_tokens` in Claude Code's request is not sent downstream); for Method 1, Claude Code's `max_tokens` is passed through as-is — no configuration needed.
- **Web search is unavailable**: Claude Code's WebSearch relies on Anthropic's servers to run, which the GPT upstream can't take on, so for tasks involving web search, switch back to a Claude-family provider. Locally executed WebFetch is unaffected.
- **Dashboard dollar amounts are for reference**: token counts are accurate, but the dollar figures are estimates converted at public API prices — Method 2's subscription traffic is estimated at GPT-5.6's public price, and Method 1's third-party gateways bill at their own rates, so both may differ from what you're actually charged and serve only for comparison. For Method 2, treat the window utilization on the provider card as authoritative for quota consumption.

## FAQ

**The upstream returns 401 or 403 (Method 1)**

First confirm the `Auth Field` in Advanced Options is the default `ANTHROPIC_AUTH_TOKEN (Default)` — switching to `ANTHROPIC_API_KEY` sends `x-api-key`, which the vast majority of OpenAI-compatible gateways don't accept. Then confirm the key itself is valid and has balance.

**Requesting new models like `gpt-5.6-luna` returns 404 Model not found (Method 2)**

Upgrade to CC Switch 3.17.0 or later. On older versions the client identity wasn't aligned with the official Codex client, so the ChatGPT server resolves new models to a nonexistent engine.

**The switch didn't take effect, or the `/model` menu still shows old names**

Both the model menu and the route address are read when Claude Code starts: after first enabling takeover you must open a new terminal session; switching between providers is a hot switch, but the display names in the menu only refresh in a new session.

**Claude Code reports "Codex OAuth authentication failed", or the card shows "Session expired" (Method 2)**

The login credentials have expired. Go back to the provider form or `Settings` → `OAuth Authentication Center` and run through `Sign in with ChatGPT` again; no command-line steps are needed.

**The conversation auto-compacts partway through**

See "Capabilities and known limitations": routed providers are managed against a 200K window, so the compaction threshold comes early — this is expected behavior.

**Restoring the official Claude setup**

Switch back to an official provider, or turn off the `Claude Code` routing toggle on the Routing page — CC Switch restores the pre-takeover live config, and the official login credentials are unaffected throughout. After restoring, you'll again need to open a new terminal session.

**Should you enable FAST mode? (Method 2)**

Leaving it off is fine. Turn it on only if you're especially latency-sensitive and willing to accept faster quota consumption; if the ChatGPT server rejects the parameter, turning the toggle off restores things.

## Compliance note

Method 2 uses ChatGPT subscription quota outside the official Codex client, and this isn't a gray-area hack: Thibault Sottiaux (@thsottiaux), the OpenAI Codex lead, has publicly demonstrated and encouraged pointing Claude Code (the "orange crab", as he jokingly calls it) at GPT-5.6 Sol — using exactly a "local proxy + model alias" approach, the same category as Method 2 here. As the lead of the Codex product line, his active encouragement to use their own model inside a competitor's client shows that running GPT-family models in Claude Code on a subscription is a use the vendor welcomes and encourages people to try.

Two practical reminders are still worth noting: first, this traffic is counted against the same subscription quota as the official Codex client, so heavy use hits the cap sooner; second, CC Switch's authentication center keeps a compliance notice out of caution ("Use your other subscriptions in Claude Code — please be mindful of compliance risks."), and whether this fits the terms that apply to your account is for you to check. When using a third-party gateway in Method 1, separately read the target gateway's terms on billing, compliance, and data retention.

## References

- [CC Switch User Manual: Add a Provider (incl. Codex OAuth reverse proxy and API formats)](../user-manual/en/2-providers/2.1-add.md)
- [CC Switch User Manual: Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [CC Switch User Manual: App Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [CC Switch v3.17.0 Release Notes](../release-notes/v3.17.0-en.md)
- Reverse guide: [Using Claude Models in Codex](./codex-claude-routing-guide-en.md)
