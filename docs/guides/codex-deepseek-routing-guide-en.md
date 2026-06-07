# Codex Local Model Routing Guide

> Applies to CC Switch 3.16.0+.

This is the generic replacement for the old DeepSeek-only routing guide. Codex keeps one CC Switch local proxy endpoint, while CC Switch dispatches each request to an upstream route by `body.model`.

## Why It Exists

Codex sends OpenAI Responses API requests. Many upstream providers expose Chat Completions or Messages-style APIs instead. If those upstream endpoints are written directly into `~/.codex/config.toml`, Codex can hit `/responses` errors, model catalog mismatches, or stream parsing failures.

Local model routing keeps Codex connected to the CC Switch Rust local proxy, then resolves the real upstream route inside CC Switch.

## Runtime Flow

1. Codex calls the local CC Switch proxy, usually `http://127.0.0.1:15721/v1/responses`.
2. CC Switch reads `body.model`.
3. The resolver matches `settings_config.codexRouting.routes[]` by exact model or prefix.
4. CC Switch builds an effective provider with route base URL, API format, auth, model mapping, and capabilities.
5. The existing forwarder performs the configured protocol conversion:
   - `openai_responses`: pass through Responses format.
   - `openai_chat`: convert Responses to Chat Completions and back.
   - `openai_messages`: convert to Messages format where supported.

## Configure Routes

In the Codex provider form, open **Local model routing** and configure:

- Match: `match.models` and `match.prefixes`.
- Upstream: `upstream.baseUrl` and `upstream.apiFormat`.
- Auth source:
  - `provider_config`: use the route/current provider API key.
  - `managed_codex_oauth`: use CC Switch managed Codex OAuth.
  - `managed_account`: managed-account auth binding, currently mapped to Codex OAuth.
- Model mapping: `upstream.modelMap`, for example `codex-model=upstream-model`.
- Capabilities: text-only, image, and reasoning.

`reuse_provider:<id>` is intentionally not part of the first version.

## Schema

```json
{
  "settings_config": {
    "codexRouting": {
      "enabled": true,
      "defaultRouteId": "openai",
      "routes": [
        {
          "id": "deepseek",
          "label": "DeepSeek",
          "enabled": true,
          "match": {
            "models": ["deepseek-v4-flash"],
            "prefixes": ["deepseek-"]
          },
          "upstream": {
            "baseUrl": "https://api.deepseek.com",
            "apiFormat": "openai_chat",
            "auth": { "source": "provider_config" },
            "modelMap": { "deepseek-v4-flash": "deepseek-v4-flash" }
          },
          "capabilities": {
            "textOnly": true,
            "inputModalities": ["text"],
            "supportsReasoning": true
          }
        }
      ]
    }
  }
}
```

`settings_config.codexRouting` is the canonical schema. `settings_config.codexModelRoutes` and `settings_config.modelRoutes` are read-only legacy fallbacks that the UI can migrate into the new schema on save.

## Notes

- A text-only route writes catalog models with `input_modalities=["text"]`.
- Responses-to-Chat conversion also uses route capability to avoid sending `image_url` blocks to text-only upstreams.
- Chat-window model switching should work without first switching the active provider in the GUI, because route resolution uses `body.model`.
