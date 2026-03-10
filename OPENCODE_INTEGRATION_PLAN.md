# OpenCode Integration Plan

## Overview

This document outlines the integration of cc-switch proxy mechanics with OpenCode, enabling provider failover, model picking, and centralized configuration management.

## Key Findings

### OpenCode Current State

1. **No Built-in Proxy Support**
   - Only supports standard `HTTP_PROXY`/`HTTPS_PROXY` environment variables
   - No provider failover mechanism
   - No model picker UI
   - Models defined statically in `opencode.json` or from models.dev

2. **Provider Configuration Format**
   ```jsonc
   // ~/.config/opencode/opencode.json
   {
     "$schema": "https://opencode.ai/config.json",
     "provider": {
       "openrouter": {
         "npm": "@openrouter/ai-sdk-provider",
         "options": {
           "baseURL": "https://openrouter.ai/api/v1",
           "apiKey": "sk-or-..."
         },
         "models": {
           "anthropic/claude-3.5-sonnet": { "name": "Claude 3.5 Sonnet" }
         }
       }
     }
   }
   ```

3. **AI SDK Based**
   - Uses Vercel AI SDK providers
   - Provider types: `@ai-sdk/openai-compatible`, `@ai-sdk/anthropic`, `@openrouter/ai-sdk-provider`, etc.
   - Model routing handled by AI SDK

### cc-switch Integration Approach

1. **-cc Suffix Pattern**
   - cc-switch managed providers use `-cc` suffix (e.g., `openrouter-cc`)
   - These providers are proxied through cc-switch's failover system
   - Original opencode providers remain untouched

2. **Proxy Architecture**
   ```
   OpenCode → cc-switch proxy (localhost:PORT) → Multiple upstream providers
                                           ↓
                                    Failover logic
                                           ↓
                              Best available provider
   ```

3. **Model Picker Catalog**
   - Predefined model selections (5 models per provider)
   - Similar to ii-agent's model picker
   - Stored in `~/.config/opencode/opencode.json` under `_models` field

## Implementation Status

### Completed (cc-switch)

✅ **OpenCode Proxy Adapter** (`src/proxy/providers/opencode.rs`)
- Handles AI SDK format providers
- Supports both `openai_chat` and `anthropic` API formats
- Transform OpenAI ↔ Anthropic message formats
- Environment variable reference support (`{env:API_KEY}`)

✅ **Provider Type Detection**
- `ProviderType::OpenCode` - Standard opencode provider
- `ProviderType::OpenCodeCC` - cc-switch managed provider (-cc suffix)

✅ **Model Picker Catalog** (`src/opencode_config.rs`)
- `OpenCodeModelInfo` - Model metadata structure
- `OpenCodeModelPicker` - Predefined 5-model selections
- Built-in catalogs for: OpenRouter, Anthropic, OpenAI, Google, DeepSeek

✅ **Tauri Commands**
- `get_opencode_provider_with_models` - Get provider with model array
- `get_all_opencode_providers_with_models` - List all providers with models
- `get_opencode_model_picker` - Get predefined model catalog
- `set_opencode_provider_models` - Set custom model selection

✅ **Proxy Support**
- `read_opencode_live()` - Read opencode.json config
- `write_opencode_live()` - Write opencode.json config
- `backup_live_config_strict()` - Backup for OpenCode
- `takeover_live_configs()` - Mark -cc providers for proxy routing

### Pending

⏳ **Proxy Server Routing**
- Add OpenCode endpoint handling in proxy handler
- Route `-cc` providers through failover logic
- Support AI SDK message format transformation

⏳ **ii-agent Integration**
- Borrow cc-switch proxy mechanics
- Implement similar model picker UI
- Add provider failover support

## Configuration Flow

### Adding a cc-switch Managed Provider

1. User selects provider in cc-switch UI
2. cc-switch creates provider with `-cc` suffix:
   ```json
   {
     "id": "openrouter-cc",
     "name": "OpenRouter (cc-switch)",
     "npm": "@openrouter/ai-sdk-provider",
     "options": {
       "baseURL": "http://localhost:PORT/v1",
       "apiKey": "PROXY_MANAGED"
     },
     "_models": [
       { "id": "anthropic/claude-3.5-sonnet", "name": "Claude 3.5 Sonnet" },
       { "id": "openai/gpt-4o", "name": "GPT-4o" },
       ...
     ]
   }
   ```

3. cc-switch proxy manages upstream providers
4. OpenCode uses the proxied provider transparently

### Model Selection

1. User selects from predefined model picker (5 models)
2. Models stored in `_models` field
3. cc-switch proxy uses models for routing decisions
4. OpenCode displays models in its UI

## Next Steps

### cc-switch (This Repository)

1. **Complete Proxy Handler** 
   - Add `/v1/chat/completions` endpoint for OpenCode
   - Support AI SDK message format
   - Integrate with failover router

2. **Testing**
   - Use Vite dev server for frontend testing
   - Test provider failover with OpenCode
   - Verify model picker integration

3. **Documentation**
   - Update README with OpenCode support
   - Add configuration examples

### ii-agent (Separate PR)

1. **Borrow Proxy Mechanics**
   - Implement similar adapter pattern
   - Add provider failover support
   - Create model picker catalog

2. **UI Integration**
   - Add model picker component
   - Provider management UI
   - Failover status display

## References

- OpenCode Config Schema: https://opencode.ai/config.json
- AI SDK Providers: https://sdk.vercel.ai/providers
- ii-agent Models: `/Users/jim/work/ii-agent/frontend/src/constants/models.tsx`
- cc-switch Provider Types: `src/provider.rs`
