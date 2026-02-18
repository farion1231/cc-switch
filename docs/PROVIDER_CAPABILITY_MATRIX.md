# Capability-Based Preset Distribution

## Universal Provider Definition

A **universal provider** has formats `{anthropic, openai_responses, google}` and fits all 3 app slots.

Derived automatically via `isUniversal(endpoint)` → checks if `deriveApps()` returns all true.

## Blocking Rules

| Missing Format     | Blocked From |
| ------------------ | ------------ |
| `anthropic`        | claude       |
| `openai_responses` | codex        |
| `google`           | gemini       |

## Universal Endpoints (6)

Derived from `transport.formats`:

| Endpoint     | Formats                                          |
| ------------ | ------------------------------------------------ |
| opencode_zen | anthropic, openai_chat, openai_responses, google |
| openrouter   | anthropic, openai_chat, openai_responses, google |
| packycode    | anthropic, openai_responses, google              |
| cubence      | anthropic, openai_responses, google              |
| aigocode     | anthropic, openai_responses, google              |
| aicodemirror | anthropic, openai_responses, google              |

## API

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
  UNIVERSAL_ENDPOINTS,
  createUniversalProviderFromEndpoint
} from "@/config/universalProviderPresets";

// Get endpoints that flow to an app
const claudeEndpoints = FLOW.claude;

// Check if endpoint is universal
if (isUniversal(endpoint)) { ... }

// Derive apps from capability
const apps = deriveApps(endpoint); // { claude: true, codex: true, gemini: false }

// Create UniversalProvider from endpoint
const provider = createUniversalProviderFromEndpoint(endpoint, "api-key");
```

## Files

```
src/config/capabilities/
├── slots.ts      # Slot definitions, fitsSlot(), deriveApps(), isUniversal()
├── endpoints.ts  # All provider endpoints with transport metadata
└── index.ts      # FLOW, UNIVERSAL exports

src/config/universalProviderPresets.ts  # UNIVERSAL_ENDPOINTS, createUniversalProviderFromEndpoint()
```
