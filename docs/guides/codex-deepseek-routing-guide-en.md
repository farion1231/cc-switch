# Using DeepSeek-Style Chat APIs in Codex: CC Switch Local Routing Guide

> Applies to CC Switch 3.19.1 and later. This guide is based on the repository documentation and code. Screenshots are generated with de-identified sample data to avoid exposing a real API key or account balance.
>
> **Important change as of 3.19.1**: the DeepSeek preset now connects directly over native Responses and no longer needs local routing. The routing-conversion path is not obsolete, though — it remains the only way to reach `deepseek-v4-pro`, it still applies to providers saved before the upgrade, and it is how Chat-format providers such as Kimi and Zhipu GLM work. Read the next section first to find out which case you are in.

## First, check whether you still need this guide

There is exactly one way to tell: look for the `Needs Routing` badge on the Codex provider card.

![Needs routing marker in the Codex provider list](../images/codex-deepseek-routing/01-codex-providers-require-routing.png)

- **Has the `Needs Routing` badge** → this provider is Chat-format, and the whole guide applies.
- **No badge** → it already connects directly over native Responses, so the routing steps here are pointless for it. Just use it.
- **Has the `No Routing Support` badge** → this is an official provider, and CC Switch blocks it from going through local routing (see the FAQ at the end).

The badge is driven by the API format recorded when the provider was saved, so upgrading CC Switch **does not** change how an existing provider behaves. For DeepSeek specifically, there are three cases after upgrading to 3.19.1:

| Your situation | Routing needed? | Notes |
|---|---|---|
| A DeepSeek provider saved before 3.19.1 | **Yes**, badge still shown | Preset changes only affect newly created providers; a saved configuration is kept as-is. To move it to a direct connection, see the end of Step 1 |
| A DeepSeek provider created from the preset after 3.19.1 | No | Connects directly to `api.deepseek.com` and picks up DeepSeek's official model catalog |
| You want to use `deepseek-v4-pro` | **Yes** | DeepSeek has not opened its Codex integration for that model yet (early August 2026 by their own estimate), so a direct connection fails upstream; it has to go through Chat + routing |

Beyond DeepSeek, plenty of providers are still Chat-format — Kimi, Zhipu GLM, SiliconFlow, ModelScope and others — and this guide applies to them in full. Just substitute the relevant preset wherever DeepSeek appears below.

## Why local routing is needed

The newer Codex CLI targets the OpenAI Responses API, while many providers expose the OpenAI Chat Completions shape, usually `/chat/completions`. These two protocols use different request bodies, streaming events, and response structures. If you put a Chat endpoint directly into Codex configuration, common results include an incorrect model list, 404/400 requests, or streaming responses that Codex cannot parse correctly.

CC Switch solves this by making Codex always talk to a local route and continue sending Responses API requests. The route detects whether the active provider is Chat-format, rewrites the request into Chat Completions for the upstream provider, and finally converts the Chat response back into the Responses shape that Codex understands.

The chain has four main steps:

1. When Codex routing is enabled, the local configuration is written as `http://127.0.0.1:15721/v1`, while `wire_api = "responses"` is kept in place.
2. The provider's `meta.apiFormat = "openai_chat"` tells the route that the real upstream is Chat Completions.
3. The route rewrites `/responses` or `/v1/responses` to `/chat/completions`, and converts the Responses request body into a Chat request body.
4. After the upstream responds, the route converts the Chat JSON or SSE stream back into Responses JSON/SSE.

For a provider that is natively Responses — such as the current DeepSeek preset — steps 2 through 4 never happen: the request goes straight to the upstream with no format rewriting at all.

## Prerequisites

Prepare these three things first:

- CC Switch installed and able to start.
- Codex CLI installed and run at least once, so the `~/.codex/config.toml` directory structure exists.
- An API key for the provider you want to use.

Taking DeepSeek as the example, its official documentation lists the OpenAI-compatible base URL as `https://api.deepseek.com` (other providers commonly use a base URL with a `/v1` suffix or a longer path — Zhipu GLM, for instance, uses `https://open.bigmodel.cn/api/coding/paas/v4`), and the Chat API path as `/chat/completions`. CC Switch's presets already carry these details, so prefer a preset and do not manually assemble the endpoint path.

## Step 1: Add a Codex provider

Open CC Switch, switch to the top-level `Codex` tab, and click the plus button in the upper-right corner to add a provider.

**Using a preset** (recommended): select the provider in the preset list, enter your API key, and save. The preset already carries the request base URL, default model, and model menu, and sets the upstream format for you; once a Chat-format preset is saved, the `Needs Routing` badge appears on its card. Thinking/reasoning parameters are preconfigured by the preset — you do not need to fill them in.

**Using a custom configuration**: enter the API key and base URL from the provider's documentation, then expand `Advanced Options` at the bottom of the form and set `Upstream Format` to `Chat Completions (routing required)`. The dropdown offers three options:

- `Responses (native)` — the upstream natively supports the Responses API; connects directly with no conversion and no routing.
- `Chat Completions (routing required)` — the case this guide covers.
- `Anthropic Messages (routing required)` — the upstream only offers the native Anthropic protocol, which the route converts as well.

Only `Responses (native)` works without routing takeover; the other two require it. For custom providers, CC Switch infers the thinking parameters from the provider name and address; you only need to expand `Reasoning Capability` and override them manually when that inference is wrong.

> **Converting an existing DeepSeek provider**: change `Upstream Format` to `Responses (native)` — there is no need to delete and recreate it. The next time you switch to it, CC Switch recognises the `deepseek.com` address and applies DeepSeek's official model catalog, so freeform `apply_patch`, the GPT-5 harness, the low/high/max reasoning levels, and web_search all take effect as usual.
>
> The one small difference is the context window: a provider's own saved model rows take priority, so the `1000000` stored before 3.19.1 overrides the officially declared `1048576`, costing you a little over 40k tokens. If that bothers you, open `Advanced Options` → `Model Mapping` and change that row's `Context Window` to `1048576`, or simply create a fresh provider from the preset.
>
> Conversely, to use `deepseek-v4-pro`, change `Upstream Format` back to `Chat Completions`.
>
> One more thing: the official model catalog used by the direct connection requires Codex CLI **0.144.0 or newer** (the freeform `apply_patch` registration it carries needs that release), and CC Switch does not verify this for you. The generated catalog file also grows to roughly 75 KB, because it contains the full GPT-5 harness text.

## Step 2: Enable local routing and route Codex

Go to the `Routing` page in Settings, expand `Local Routing`, and complete two toggles:

1. Turn on the `Routing Master Switch` to start the local service. The default address is `127.0.0.1:15721`.
2. Turn on `Codex` under `Routing Enabled`. If you only want Codex to use local routing, you can leave Claude and Gemini off.

![Enabling Codex routing on the local routing page](../images/codex-deepseek-routing/03-local-route-codex-takeover.png)

After routing is enabled, CC Switch points Codex's live configuration to the local route and manages authentication with a placeholder. The real API key stays in the CC Switch provider configuration and is injected by the local route while forwarding requests, so you do not need to expose the key in Codex's live configuration.

## Step 3: Switch providers and restart Codex

Return to the Codex provider list and click `Enable` on the provider you want. If it carries the `Needs Routing` marker while routing is not running, CC Switch shows a prompt saying the routing service is required.

After switching, restart the current Codex terminal session. This is recommended because:

- The Codex process may already have read the old `config.toml`.
- After `model_catalog_json` is generated, the `/model` menu usually needs a fresh process before it refreshes.

Inside Codex, use `/model` to check whether the current model comes from the matching preset. Then send a small test prompt and confirm that the request count increases in the routing panel, or that a Codex request appears in usage/request logs.

## Usage attribution changes once you connect directly

This is worth calling out on its own: once a provider connects directly, its requests no longer pass through the local route, so the per-request proxy usage accounting cannot see them.

The usage itself is not lost — Codex session-log import still records it — but that path carries no provider identity: all Codex usage that did not go through the local proxy is grouped under a single entry named `Codex (Session)`. **To tell them apart, look at the model**: every usage row keeps its own model id, and the usage panel's `Model Stats` table lists them per model, with separate cost and token figures.

If you genuinely need to reconcile by provider — comparing the same model across several aggregators, for instance — keep `Upstream Format` on Chat and leave routing takeover enabled.

## FAQ

**Codex reports 404 or cannot find `/responses`**

Usually Codex routing is not enabled, or the upstream Chat base URL was written directly into Codex manually. Check whether `~/.codex/config.toml` points to `http://127.0.0.1:15721/v1`.

**The upstream reports 404**

If you are using a built-in preset, first confirm that the active provider really comes from the preset and that Codex routing is enabled. Only custom providers require extra base URL checks: it should be the service endpoint given in the provider's documentation, not the full endpoint path with `/chat/completions`.

**Switching to `deepseek-v4-pro` fails upstream**

DeepSeek has not opened its Codex integration for that model yet. Change that provider's `Upstream Format` back to `Chat Completions (routing required)` and enable routing takeover — that is exactly the path DeepSeek took before 3.19.1, and the route's Responses-to-Chat conversion serves pro as it always did. Alternatively use `deepseek-v4-flash`, which is the preset default and unaffected.

**`/model` does not show the provider's models**

Restart Codex after saving the provider. CC Switch generates `cc-switch-model-catalog.json` and writes its path to `model_catalog_json`, but a running Codex process may not hot-load the model catalog.
The Codex app currently does not support multi-model selection, so it uses the first configured model by default.

**Routing is enabled, but requests still go to the wrong provider**

Confirm that all three states match: the current provider under the Codex tab is the right one; the local routing service is running; and the Codex toggle is enabled under `Routing Enabled`.

**Can I use an official OpenAI Codex account through local routing?**

Not recommended. CC Switch blocks switching to official providers while local routing takeover is enabled, because accessing official APIs through a proxy may create account risk. Routing is mainly intended for third-party, aggregator, or protocol-conversion scenarios.

## References

- [CC Switch User Manual: Add Provider](../user-manual/en/2-providers/2.1-add.md)
- [CC Switch User Manual: Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [CC Switch User Manual: App Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [DeepSeek API Docs: Integrate with Codex](https://api-docs.deepseek.com/quick_start/agent_integrations/codex/) (official Codex integration notes, including `wire_api = "responses"` and model coverage)
- [DeepSeek API Docs: Using the Responses API](https://api-docs.deepseek.com/guides/responses_api/)
- [DeepSeek API Docs: Your First API Call](https://api-docs.deepseek.com/)
- [DeepSeek API Docs: Create Chat Completion](https://api-docs.deepseek.com/api/create-chat-completion)
- [DeepSeek API Docs: Multi-round Conversation](https://api-docs.deepseek.com/guides/multi_round_chat)
