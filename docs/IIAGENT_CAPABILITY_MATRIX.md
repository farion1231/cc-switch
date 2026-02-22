# IIAgent Provider Capability Matrix

## Universal Provider Definition

A **universal provider** has formats that support both `anthropic` and `openai_chat`/`openai_responses`, enabling use across Claude, Codex, and OpenCode.

Derived automatically via `isUniversal(endpoint)` → checks if `deriveApps()` returns true for claude, codex, and opencode.

## Blocking Rules

| Missing Format         | Blocked From |
| ---------------------- | ------------ |
| `anthropic`            | claude       |
| `openai_responses`     | codex        |
| `google`               | gemini       |

## IIAgent Endpoints (11)

All endpoints support `supportsModelsEndpoint: true` for automatic model enumeration.

| Endpoint                    | Formats                        | Universal | Apps                                    |
| --------------------------- | ------------------------------ | --------- | --------------------------------------- |
| iiagent_anthropic           | anthropic                      | ❌        | claude, opencode, openclaw              |
| iiagent_openai              | openai_chat, openai_responses  | ❌        | codex, opencode, openclaw               |
| iiagent_deepseek            | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_zhipu               | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_zhipu_en            | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_bailian             | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_kimi                | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_minimax             | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_minimax_en          | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_custom_anthropic    | anthropic, openai_chat         | ✅        | claude, codex, opencode, openclaw       |
| iiagent_custom_openai       | openai_chat, openai_responses  | ❌        | codex, opencode, openclaw               |

## Universal IIAgent Endpoints (7)

These endpoints support both Anthropic and OpenAI formats:

- **iiagent_deepseek** - DeepSeek V3 with dual format support
- **iiagent_zhipu** - Zhipu GLM (Chinese)
- **iiagent_zhipu_en** - Zhipu GLM International
- **iiagent_bailian** - Alibaba Bailian
- **iiagent_kimi** - Moonshot Kimi
- **iiagent_minimax** - MiniMax (Chinese)
- **iiagent_minimax_en** - MiniMax International
- **iiagent_custom_anthropic** - Custom Anthropic-compatible endpoints

## API Usage

```typescript
import {
  ENDPOINTS,
  APP_SLOTS,
  FLOW,
  UNIVERSAL,
  fitsSlot,
  deriveApps,
  isUniversal,
  flowTo,
  getDefaultApps
} from "@/config/capabilities";

import {
  IIAGENT_ENDPOINTS,
  iiAgentFlowTo
} from "@/config/iiAgentEndpoints";

// Add IIAgent endpoints to main list
export const ENDPOINTS: ProviderEndpoint[] = [
  // ... existing endpoints
  ...IIAGENT_ENDPOINTS,
];

// Get IIAgent endpoints that flow to an app
const claudeEndpoints = iiAgentFlowTo("claude");

// Check if endpoint is universal
if (isUniversal(endpoint)) { 
  // Can be used as universal provider
}

// Derive apps from capability
const apps = deriveApps(endpoint); 
// Example: { claude: true, codex: true, gemini: false, opencode: true, openclaw: true }

// Filter universal IIAgent endpoints
const universalIIAgent = IIAGENT_ENDPOINTS.filter(isUniversal);
```

## IIAgent Slot Definition

IIAgent supports both Anthropic and OpenAI formats:

```typescript
export const IIAGENT_SLOT = {
  acceptsFormats: new Set(["anthropic", "openai_chat", "openai_responses"]),
};
```

This allows ii-agent to work with:
- **Claude** (via anthropic format)
- **Codex** (via openai_responses format)
- **OpenCode** (via anthropic or openai_chat format)
- **OpenClaw** (via anthropic or openai_chat format)

## Files

```
cc-switch-preset/
├── iiagent.svg                    # IIAgent icon
├── iiAgentProviderPresets.ts      # Provider presets
├── iiAgentEndpoints.ts            # Endpoint annotations
├── slots.ts                       # Slot definitions, fitsSlot(), deriveApps(), isUniversal()
└── IIAGENT_CAPABILITY_MATRIX.md   # This document
```

## Integration

Add IIAgent endpoints to cc-switch:

```typescript
// src/config/capabilities/endpoints.ts
import { IIAGENT_ENDPOINTS } from "./iiAgentEndpoints";

export const ENDPOINTS: ProviderEndpoint[] = [
  // ... existing endpoints
  ...IIAGENT_ENDPOINTS,
];
```

Add IIAgent presets to provider list:

```typescript
// src/config/claudeProviderPresets.ts or similar
import { iiAgentPresets } from "./iiAgentProviderPresets";

export const providerPresets: ProviderPreset[] = [
  // ... existing presets
  ...iiAgentPresets,
];
```

## IIAgent Model Proxy

IIAgent provides a model proxy API for provider management:

```bash
GET  /api/v1/providers              # List all providers
POST /api/v1/providers              # Add provider
POST /api/v1/providers/{id}/switch  # Switch provider for app
GET  /api/v1/providers/{id}/models  # Enumerate models
```

The model proxy supports:
- **Anthropic format** - Direct or compatible APIs
- **OpenAI format** - Chat Completions and Responses APIs
- **Model enumeration** - Automatic discovery via `/models` endpoint
- **Proxy support** - HTTP/HTTPS/SOCKS5 per provider

See ii-agent documentation for complete API specification.
