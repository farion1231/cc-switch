# Codex Local Model Routing Design Notes

## Goal

Codex should keep using the CC Switch Rust local proxy as its single configured endpoint. CC Switch then resolves the real upstream provider per request by reading `body.model`. This makes model switching in the chat window sufficient; the user should not need to first switch the active upstream provider in the GUI.

## Canonical Schema

The canonical configuration entry is `settings_config.codexRouting`.

```json
{
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
```

Legacy `settings_config.codexModelRoutes` and `settings_config.modelRoutes` are compatibility-only read paths. New writes should use `codexRouting`.

## Auth Scope

First version supports:

- `provider_config`: use the configured provider or route API key.
- `managed_codex_oauth`: use CC Switch managed Codex OAuth.
- `managed_account`: managed account binding, currently mapped to Codex OAuth.

`reuse_provider:<id>` is deliberately deferred. Supporting it later should include explicit resolver tests and UI copy so users know which provider owns auth and endpoint state.

## Backend Integration Points

- `src-tauri/src/proxy/providers/codex.rs`
  - Resolves `codexRouting`.
  - Creates effective routed providers.
  - Normalizes route API format to `openai_responses`, `openai_chat`, or `openai_messages`.
- `src-tauri/src/proxy/forwarder.rs`
  - Reuses the existing forwarder after effective provider resolution.
  - Handles endpoint rewrite and protocol conversion.
- `src-tauri/src/proxy/providers/transform_codex_chat.rs`
  - Uses text-only route capability to avoid sending Chat `image_url` content to text-only upstreams.
- `src-tauri/src/codex_config.rs`
  - Generates model catalog capabilities from route capability first, with hardcoded model-name fallbacks only for legacy behavior.

## Frontend Integration Points

- `src/types.ts`: route config and capability types.
- `src/components/providers/forms/hooks/useCodexConfigState.ts`: load new schema and migrate legacy route arrays into UI state.
- `src/components/providers/forms/ProviderForm.tsx`: save `settings_config.codexRouting`.
- `src/components/providers/forms/CodexFormFields.tsx`: Local model routing editor.
- `src/i18n/locales/*.json`: localized labels under `codexConfig`.

## PR Split

1. Rust backend route resolver, effective provider construction, forwarder integration, and focused tests.
2. Model catalog capabilities plus text-only image regression protection.
3. UI, i18n, docs, and `memory.md`.

## Validation

- `cargo fmt`
- `cargo test codex --lib`
- `pnpm run typecheck`

Known unrelated warning during Rust tests: `src/settings.rs` imports `std::io::Write` without using it.
