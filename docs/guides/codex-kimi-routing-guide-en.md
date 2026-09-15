# Using Kimi in Codex: CC Switch Local Routing Guide

> Applies to CC Switch 3.16.5 and nearby versions. This guide is based on the repository documentation and code, and uses Kimi as an example of an OpenAI Chat Completions-compatible API. Screenshots are generated from the current frontend UI with de-identified sample data to avoid exposing a real API key or account balance.

## Why local routing is needed

The newer Codex CLI targets the OpenAI Responses API, while both the Kimi Open Platform and Kimi For Coding expose the OpenAI Chat Completions shape, `/chat/completions`. These two protocols use different request bodies, streaming events, and response structures. If you put a Kimi endpoint directly into Codex configuration, the usual result is a 404 on `/responses`, or streaming responses that Codex cannot parse correctly.

The third-party tools officially supported by Kimi For Coding are Anthropic-compatible coding agents such as Claude Code and Roo Code — Codex is not on the list. To use Kimi inside Codex, you need a protocol conversion layer, and that is exactly what CC Switch Local Routing does.

CC Switch solves this by making Codex always talk to a local route and continue sending Responses API requests. The route detects whether the active provider is Chat-format, rewrites the request into Chat Completions for the upstream provider, and finally converts the Chat response back into the Responses shape that Codex understands.

![Needs routing marker in the Codex provider list](../images/codex-kimi-routing/01-codex-providers-require-routing.png)

The chain has four main steps:

1. When Codex routing is enabled, the local configuration is written as `http://127.0.0.1:15721/v1`, while `wire_api = "responses"` is kept in place.
2. The provider's `meta.apiFormat = "openai_chat"` tells the route that the real upstream is Chat Completions.
3. The route rewrites `/responses` or `/v1/responses` to `/chat/completions`, and converts the Responses request body into a Chat request body.
4. After the upstream responds, the route converts the Chat JSON or SSE stream back into Responses JSON/SSE.

## Prerequisites

Prepare these three things first:

- CC Switch installed and able to start.
- Codex CLI installed and run at least once, so the `~/.codex/config.toml` directory structure exists.
- A Kimi API key.

Kimi API keys come from two different places, matching two different built-in presets in CC Switch:

- **Kimi Open Platform** (platform.kimi.com): pay-as-you-go API keys billed by token usage, matching the `Kimi` preset. The OpenAI-compatible base URL is `https://api.moonshot.cn/v1` and the default model is `kimi-k2.7-code`.
- **Kimi For Coding** (kimi.com/code): a dedicated key generated from the Kimi Code membership benefits, matching the `Kimi For Coding` preset. The base URL is `https://api.kimi.com/coding/v1` and the unified model is `kimi-for-coding`.

Both presets already contain the correct endpoint and model details, so prefer the presets and do not manually assemble the endpoint path.

## Step 1: Add a Codex provider

Open CC Switch, switch to the top-level `Codex` tab, and click the plus button in the upper-right corner to add a provider.

Depending on which kind of key you have, choose the built-in `Kimi` preset (Open Platform, pay-as-you-go) or `Kimi For Coding` preset (membership subscription). You only need to do two things:

- Enter the matching Kimi API key.
- Save the provider.

![Upstream format in the Kimi Codex provider form](../images/codex-kimi-routing/02-kimi-codex-routing-form.png)

The preset already includes Kimi's request base URL, default model, model menu, thinking/reasoning parameters, and presets `Upstream Format` under `Advanced Options` to `Chat Completions (routing required)`. You can adjust the default model or model display names if needed — for example, the Open Platform preset defaults to `kimi-k2.7-code`, and you can switch to `kimi-k2.7-code-highspeed` following the official documentation. The protocol conversion is handled by the routing layer.

## Step 2: Enable local routing and route Codex

Go to the `Routing` page in Settings, expand `Local Routing`, and complete two toggles:

1. Turn on the main routing switch to start the local service. The default address is `127.0.0.1:15721`.
2. Turn on `Codex` under `Routing Enabled`. If you only want Codex to use local routing, you can leave Claude and Gemini off.

![Enabling Codex routing on the local routing page](../images/codex-kimi-routing/03-local-route-codex-takeover.png)

After routing is enabled, CC Switch points Codex's live configuration to the local route and manages authentication with a placeholder. The real Kimi key stays in the CC Switch provider configuration and is injected by the local route while forwarding requests, so you do not need to expose the key in Codex's live configuration.

## Step 3: Switch providers and restart Codex

Return to the Codex provider list and click `Enable` on the Kimi provider. If you see the `Needs Routing` marker, that provider must be used while routing is running; when the route is not started, CC Switch shows a prompt saying the routing service is required.

After switching, restart the current Codex terminal session. This is recommended because:

- The Codex process may already have read the old `config.toml`.
- After `model_catalog_json` is generated, the `/model` menu usually needs a fresh process before it refreshes.

Inside Codex, use `/model` to check whether the current model comes from the Kimi preset, such as `Kimi K2.7 Code` or `Kimi For Coding`. The Codex app currently does not support multi-model selection, so it defaults to the first configured model. Then send a small test prompt and confirm that the request count increases in the routing panel, or that a Codex request appears in usage/request logs.

## How to handle other Chat providers

Kimi, DeepSeek, MiniMax, SiliconFlow, and other common Chat-format providers already have presets in CC Switch, so use presets first. Only choose custom configuration for providers that are not covered by presets; in that case, fill in the API key, base URL, and models according to the provider's documentation, and set `Upstream Format` under `Advanced Options` to `Chat Completions (routing required)`.

If the upstream provider directly supports the OpenAI Responses API, set `Upstream Format` to `Responses`; CC Switch then connects through Responses directly without Chat conversion.

## FAQ

**Codex reports 404 or cannot find `/responses`**

Usually Codex routing is not enabled, or the Kimi Chat base URL was written directly into Codex manually — the Kimi upstream has no `/responses` endpoint, so that always 404s. Check whether `~/.codex/config.toml` points to `http://127.0.0.1:15721/v1`.

**Kimi upstream reports 401 or 403**

First confirm the key matches the preset: Open Platform keys only work with the `Kimi` preset, and Kimi Code membership keys only work with the `Kimi For Coding` preset. The two key families are not interchangeable.

**Kimi upstream reports 404**

If you are using a built-in Kimi preset, first confirm that the active provider really comes from the preset and that Codex routing is enabled. Only custom providers require extra base URL checks: the base URL should be the service root, not the full endpoint path with `/chat/completions`.

**`/model` does not show Kimi models**

Restart Codex after saving the provider. CC Switch generates `cc-switch-model-catalog.json` and writes its path to `model_catalog_json`, but a running Codex process may not hot-load the model catalog.
The Codex app currently does not support multi-model selection, so it uses the first configured model by default.

**Routing is enabled, but requests still go to the wrong provider**

Confirm that all three states match: the current provider under the Codex tab is Kimi; the local routing service is running; and the Codex toggle is enabled under `Routing Enabled`.

**Can I use an official OpenAI Codex account through local routing?**

Not recommended. CC Switch blocks switching to official providers while local routing takeover is enabled, because accessing official APIs through a proxy may create account risk. Routing is mainly intended for third-party, aggregator, or protocol-conversion scenarios.

## References

- [CC Switch User Manual: Add Provider](../user-manual/en/2-providers/2.1-add.md)
- [CC Switch User Manual: Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [CC Switch User Manual: App Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [Kimi Open Platform: Using Kimi K2.7 Code in coding tools](https://platform.kimi.com/docs/guide/agent-support)
- [Kimi Code Docs: Overview](https://www.kimi.com/code/docs/)
- [Kimi Code Docs: Using with third-party coding agents](https://www.kimi.com/code/docs/third-party-tools/other-coding-agents.html)
