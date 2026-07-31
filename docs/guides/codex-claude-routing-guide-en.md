# Using Claude Models in Codex with CC Switch

> Applies to CC Switch 3.17.0 and later (the Anthropic Messages upstream was introduced in 3.17.0). This guide is based on the repository documentation and code, and uses a Claude-family relay gateway as the example. Screenshots are generated from the current frontend UI with de-identified sample data to avoid exposing a real API key.

## Why local routing is needed

The newer Codex CLI targets the OpenAI Responses API, while the various Claude-family relay gateways and internal enterprise gateways expose the Anthropic Messages protocol — that is, `/v1/messages`. These two protocols use completely different request bodies, streaming events, and response structures, so putting such a gateway's endpoint directly into Codex configuration can only result in a request to `/responses` coming back 404.

This feature targets the scenario where all you have is a `/v1/messages` endpoint: you have a key for some Claude-family relay gateway and want to run Claude-family models with Codex's interaction style; or your company has banned the Claude Code client for compliance reasons and kept only an approved Claude-family gateway — the model itself is available, and all that's missing is a permitted client, which Codex can now fill.

CC Switch's approach is to keep Codex always talking to the local route and still sending Responses API requests; once the route detects that the active provider is Anthropic-format, it converts the request into Anthropic Messages for the upstream, then converts the response back into the Responses shape it returns to Codex.

![Needs routing marker in the Codex provider list](../images/codex-claude-routing/01-codex-providers-require-routing.png)

The chain has four main steps:

1. When Codex is taken over, the local configuration is written as `http://127.0.0.1:15721/v1`, and `wire_api = "responses"` is forcibly kept in place.
2. The provider's `anthropic` upstream format tells the route that the real upstream speaks the Anthropic Messages protocol.
3. The route rewrites `/responses` to `/v1/messages` and converts the Responses request body into an Anthropic request body.
4. After the upstream responds, the route converts the Anthropic JSON or SSE back into the Responses JSON/SSE that Codex understands — reasoning content, tool calls, and images are all within the conversion scope.

## Prerequisites

Prepare these three things first:

- CC Switch installed and able to start (3.17.0 or later).
- Codex CLI installed and run at least once, so the `~/.codex/` directory structure exists.
- An API key that can reach an Anthropic Messages protocol endpoint (`/v1/messages`) — from some Claude-family relay gateway, or an internal enterprise Claude gateway; follow the gateway's documentation for its endpoint and auth method. Note: some providers restrict their Claude API to Claude Code only, so such a key may error out when used through Codex — if you're unsure, check with your provider first.

The Codex tab currently has no built-in Anthropic preset, so the steps below use the `Custom Configuration` path — just four or five fields from start to finish.

## Step 1: Add a Codex provider

Open CC Switch, switch to the top-level `Codex` tab, click the plus button in the upper-right corner to add a provider, keep the default `Custom Configuration`, then fill in:

- **Provider Name**: anything you like, e.g. `Claude Gateway`.
- **API Key**: your gateway key. The real key is stored only in CC Switch and injected by the local route when forwarding, so it never enters Codex's live config.
- **API Request URL**: just the gateway's service root, e.g. `https://claude-gateway.example.com`. It works with or without a trailing `/v1` — the route sends requests to `/v1/messages` automatically; don't assemble `/v1/messages` yourself (if your gateway documentation gives you a complete messages URL, you can turn on the `Full URL` toggle next to it and paste it verbatim). The yellow hint below the address bar, "compatible with OpenAI Response format", is generic copy written for the direct Responses scenario; when you choose the Anthropic format, just fill it in as this guide describes.
- **Default Model**: enter a Claude model id the gateway recognizes, e.g. `claude-sonnet-5`; follow the model names in the gateway's documentation.

Then expand `Advanced Options` and change `Upstream Format` from the default `Responses (native)` to **`Anthropic Messages (routing required)`**.

![Codex provider form for a Claude gateway](../images/codex-claude-routing/02-claude-codex-provider-form.png)

After selecting Anthropic Messages, three supporting fields appear below:

![Advanced options for the Anthropic upstream](../images/codex-claude-routing/03-anthropic-advanced-options.png)

- **Auth field**: determines which header carries the API key to the upstream; only one of the two is sent — choose per your gateway's documentation.
  - `ANTHROPIC_AUTH_TOKEN (Authorization)`: sends `Authorization: Bearer <key>`. This is the default, and most Claude-family relay gateways use it.
  - `ANTHROPIC_API_KEY (x-api-key)`: sends `x-api-key: <key>`. Some gateways that follow Anthropic's native header convention require this. Picking the wrong one usually shows up as 401 / 403.
- **Emulate Claude Code client**: off by default. Turn it on only when the gateway or its upstream restricts usage to "Claude Code only"; when enabled, it spoofs the User-Agent, `anthropic-beta`, and `x-app` headers and injects the Claude Code identity as the first line of the system prompt. Ordinary gateways don't need it; if you're still rejected after enabling it, see "FAQ".
- **Max output tokens**: the Anthropic protocol's `max_tokens` is required, and when a Codex request carries no output ceiling the route falls back to a conservative 8192, which may truncate long answers or deep reasoning (showing up as an incomplete reply, `stop_reason=max_tokens`). If you hit truncation, raise this to the model's real ceiling here — but don't exceed it, or the upstream will 400 outright.

The `Model Mapping` in the same area is optional: add model ids like `claude-opus-4-8`, `claude-sonnet-5`, and `claude-haiku-4-5-20251001` (use the names your upstream recognizes) one per row, and CC Switch generates a model catalog so Codex's `/model` menu can list them; you can also leave it empty, in which case Codex just requests the default model.

After you save the provider, a `Needs Routing` marker appears on the card — providers like this only work while local routing is running.

## Step 2: Enable local routing and take over Codex

Go to the `Routing` page in Settings, expand `Local Routing`, and complete two toggles:

1. Turn on the `Routing Master Switch` to start the local service (the first time you enable it, an explanatory confirmation dialog appears). The default address is `127.0.0.1:15721`.
2. Turn on `Codex` under `Routing Enabled`. If you only want Codex to use routing, you can leave Claude and Gemini off.

![Enabling Codex takeover on the local routing page](../images/codex-claude-routing/04-local-route-codex-takeover.png)

After takeover, CC Switch points Codex's live config at the local route (`base_url = http://127.0.0.1:15721/v1`), with only a placeholder in `auth.json`. The real Claude key stays in the CC Switch provider config and is injected by the local route on forward, using the auth field you selected.

## Step 3: Switch providers and restart Codex

Return to the Codex provider list and click `Enable` on the Claude provider. If routing isn't running, CC Switch shows "This provider uses Anthropic Messages API format, requires the routing service to work properly. Start routing first." — just go back to Step 2 and turn it on.

After switching, restart the current Codex terminal session: `config.toml` and the model catalog are read when the Codex process starts, and a running process isn't guaranteed to hot-load them.

Inside Codex you can verify step by step:

- If you configured model mapping, use `/model` to check whether the Claude models now appear in the menu; without a mapping, Codex just uses the default model.
- Send a small question and watch the "Current Provider" on the Settings → Routing page change from "Waiting for first request..." to your Claude provider, with "Total Requests" starting to climb.
- In the usage dashboard, these requests show their model names faithfully as `claude-*`, and you can filter by provider to reconcile token usage.

## Capabilities and known limitations

- **Prompt caching is automatic**: the conversion bridge injects standard 5-minute prompt-cache markers (system prompt, tool definitions, and conversation history) per Anthropic's convention, so long conversations don't resend everything at full price each turn — no configuration needed.
- **Reasoning and tools are lossless**: extended thinking content round-trips across the bridge intact, and multi-turn tool calls, image inputs, and PDF inputs are all fully converted.
- **Supports the `[1m]` long-context marker**: when the default model or a model id in the mapping ends with `[1m]` (e.g. `claude-sonnet-5[1m]`), the route strips the marker and automatically adds the corresponding 1M-context beta header, provided the gateway supports that capability.
- **Web search is unavailable**: in Anthropic upstream mode, Codex's built-in `web_search` is deliberately disabled — the conversion layer can't translate it for the Anthropic endpoint, and disabling it avoids presenting the model with a tool that's guaranteed to fail.
- **Truncation is reported faithfully**: when the upstream stops at the output ceiling or the stream is cut off, Codex sees "incomplete" rather than a disguised success, making it easy to notice and raise the max output tokens.

## FAQ

**The upstream returns 401 or 403**

Nine times out of ten the auth field doesn't match what the gateway wants: switch between `ANTHROPIC_AUTH_TOKEN (Authorization)` and `ANTHROPIC_API_KEY (x-api-key)` per your gateway's documentation and try again (most gateways use the default Bearer). Also confirm the key itself is valid and has balance.

**Codex reports 404 or cannot find `/responses`**

Usually Codex routing takeover isn't enabled, or you manually wrote the gateway's address directly into Codex — an Anthropic-protocol upstream has no `/responses` endpoint, so that always 404s. Check whether the current provider's `base_url` in `~/.codex/config.toml` points to `http://127.0.0.1:15721/v1`.

**The upstream returns 404 (routing already enabled)**

Check the API Request URL: it should be the gateway's service root, not an address carrying another protocol's path such as `/chat/completions`. When the gateway path is unusual, use the `Full URL` toggle to paste the complete messages endpoint directly.

**Replies often get cut off mid-way**

This is the default 8192 output ceiling showing up. Raise it in `Max output tokens` under the provider form's Advanced Options (don't exceed the model's/gateway's real ceiling), save, and retry.

**`/model` doesn't show the Claude models**

Confirm you've added entries to the model mapping, then restart Codex after saving the provider — the model catalog isn't hot-loaded by a running process. When the default model isn't in the mapping, the menu won't list it, but a direct request still works.

**Web search doesn't work**

By design; see "Capabilities and known limitations". For tasks that need web search, switch back to a Responses/Chat-format provider.

**An error says usage is restricted to Claude Code**

Some providers restrict their Claude API to the Claude Code client, so it gets rejected when going through this guide's chain via Codex. Try turning on the `Emulate Claude Code client` toggle in Advanced Options; if it still errors after that, the restriction is enforced on the provider's side — check with your provider whether your key can be used outside Claude Code. Keep this toggle off for ordinary gateways.

## Compliance note

Before using this in the "company bans the client but keeps only the gateway" scenario, it's worth confirming that doing so complies with your organization's specific policy — whether what's banned is a specific client or a manner of use differs from one place to the next. When using a third-party relay gateway, read the target gateway's terms on billing, compliance, and data retention.

## References

- [CC Switch User Manual: Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [CC Switch User Manual: App Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [CC Switch v3.17.0 Release Notes](../release-notes/v3.17.0-en.md)
- This feature comes from community contribution [#5071](https://github.com/farion1231/cc-switch/pull/5071); thanks @yeeyzy.
