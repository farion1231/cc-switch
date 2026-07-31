# Using DeepSeek V4 Pro in Codex: CC Switch Local Routing Guide

> **Important:** The built-in `DeepSeek` preset now contains only DeepSeek V4 Flash and connects directly through the native Responses API, so it **does not need local routing**. This guide applies only to the separate `DeepSeek V4 Pro` preset. V4 Pro currently uses Chat Completions and requires CC Switch to convert the protocol locally.

> Saved DeepSeek providers are not migrated automatically when a built-in preset changes. To use native Responses for Flash or the new Pro Chat configuration, select the corresponding preset again or create a new provider.

## Choose the right preset first

| Preset | Model | Upstream protocol | Local routing required |
|--------|-------|-------------------|------------------------|
| `DeepSeek` | `deepseek-v4-flash` | Native Responses | No |
| `DeepSeek V4 Pro` | `deepseek-v4-pro` | Chat Completions | Yes |

To use Flash, select `DeepSeek`, enter the API key, and save. Its Codex model catalog already declares function calling, freeform `apply_patch`, text Web Search, parallel tool calls, and `low` / `high` / `max` reasoning levels.

The remaining steps apply only to `DeepSeek V4 Pro`.

## Why V4 Pro needs local routing

Codex CLI uses the OpenAI Responses API, while the V4 Pro preset currently uses Chat Completions. CC Switch lets Codex keep sending Responses requests and converts the protocol in both directions:

1. After Codex takeover is enabled, the live configuration points to `http://127.0.0.1:15721/v1` and keeps `wire_api = "responses"`.
2. The `DeepSeek V4 Pro` preset is marked as Chat Completions format.
3. The local route converts each Responses request into Chat Completions and sends it to DeepSeek.
4. After DeepSeek responds, the route converts the JSON or SSE stream back into the Responses format Codex understands.

## Prerequisites

You need:

- CC Switch installed and able to start.
- Codex CLI installed and run at least once.
- A DeepSeek API key.

The preset already contains `https://api.deepseek.com` and the correct model name. Do not append `/chat/completions` to the base URL manually.

## Step 1: Add the V4 Pro provider

Open CC Switch, switch to the top-level `Codex` tab, and click the plus button:

1. Select the `DeepSeek V4 Pro` preset.
2. Enter the DeepSeek API key.
3. Save the provider.

The preset configures the Chat Completions format, the `deepseek-v4-pro` model catalog, and reasoning parameters automatically. You normally do not need to edit the advanced configuration.

## Step 2: Enable local routing and Codex takeover

Open the `Routing` page in Settings and expand `Local Routing`:

1. Turn on the main routing switch to start the local service. Its default address is `127.0.0.1:15721`.
2. Enable `Codex` under application routing.

After takeover is enabled, the Codex live configuration points to the local route. The real API key remains in the CC Switch provider configuration and is injected while forwarding.

## Step 3: Enable the provider and restart Codex

Return to the Codex provider list and enable `DeepSeek V4 Pro`. The preset is marked as requiring routing, so keep local routing running while it is in use.

Restart the Codex terminal session after switching. The process may have already loaded the old `config.toml`, and the `/model` menu usually reloads `model_catalog_json` only in a new process.

Inside Codex, use `/model` to confirm that the current model is `DeepSeek V4 Pro`. Then send a small request and verify that it appears in the CC Switch routing or request logs.

## Migrating an older DeepSeek configuration

Older versions put Flash and Pro in one Chat preset. Existing providers keep their saved values after upgrading and do not switch protocols automatically:

- For Flash, select the `DeepSeek` preset again or create a new provider. It connects directly through native Responses and does not need local routing.
- For Pro, select the `DeepSeek V4 Pro` preset again or create a new provider, and keep Codex local routing enabled.

Restart Codex after changing presets so the live configuration and model catalog are refreshed.

## FAQ

**Do I need Codex local routing if I only use V4 Flash?**

No. Select the main `DeepSeek` preset. Flash supports Responses natively, so CC Switch does not perform Chat protocol conversion.

**V4 Pro reports 404 or cannot find `/responses`**

Confirm that `DeepSeek V4 Pro` is selected, the local routing service is running, and Codex application routing is enabled. Do not write DeepSeek's Chat base URL directly into Codex configuration.

**`/model` does not show a DeepSeek model**

Restart Codex after saving and enabling the provider. A running Codex process may not hot-load the model catalog.

**Routing is enabled, but requests go to the wrong provider**

Confirm that `DeepSeek V4 Pro` is enabled on the Codex tab, the main routing switch is on, and Codex is enabled under application routing.

## References

- [CC Switch User Manual: Add Provider](../user-manual/en/2-providers/2.1-add.md)
- [CC Switch User Manual: Proxy Service](../user-manual/en/4-proxy/4.1-service.md)
- [CC Switch User Manual: App Routing](../user-manual/en/4-proxy/4.2-routing.md)
- [DeepSeek: Using the Responses API](https://api-docs.deepseek.com/guides/responses_api)
- [DeepSeek: Integrate with Codex](https://api-docs.deepseek.com/quick_start/agent_integrations/codex)
