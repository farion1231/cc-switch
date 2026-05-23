# Interface Contracts: Pi CLI 配置管理

**Feature**: specs/001-pi-cli-support

## Contract 1: models.json Output Format

CC Switch commits to producing `~/.pi/agent/models.json` in the following schema.
All CC Switch-managed providers use the `cc-switch-` prefix namespace.

### Schema

```jsonc
{
  "providers": {
    "<provider_id>": {
      "baseUrl": "string",           // REQUIRED: API endpoint URL
      "api": "string",               // REQUIRED: one of "openai-completions" | "anthropic-messages" | "google-generative-ai" | "openai-responses"
      "apiKey": "string",            // REQUIRED: env var name (e.g. "ANTHROPIC_API_KEY") or literal key
      "authHeader": true,            // REQUIRED: always true for CC Switch providers
      "headers": {},                 // OPTIONAL: custom HTTP headers
      "compat": {                    // OPTIONAL: compatibility flags
        "supportsDeveloperRole": false,
        "supportsReasoningEffort": false
      },
      "models": [                    // REQUIRED: at least one model entry
        {
          "id": "string",            // REQUIRED: model identifier
          "name": "string",          // REQUIRED: display name
          "reasoning": false,        // REQUIRED: extended thinking support
          "input": ["text"],         // REQUIRED: supported input modalities
          "contextWindow": 128000,   // REQUIRED: context window in tokens
          "maxTokens": 16384,        // REQUIRED: max output tokens
          "cost": {                  // REQUIRED: pricing per million tokens
            "input": 0,
            "output": 0,
            "cacheRead": 0,
            "cacheWrite": 0
          }
        }
      ]
    }
  }
}
```

### Merge Contract

- CC Switch reads existing `models.json` before writing
- Providers with IDs NOT starting with `"cc-switch-"` are preserved as-is
- Providers with IDs starting with `"cc-switch-"` are replaced entirely
- If `models.json` does not exist, CC Switch creates it with only CC Switch providers
- Write is atomic: temporary file → validate → rename

## Contract 2: settings.json Fields Managed by CC Switch

CC Switch writes/reads the following fields in `~/.pi/agent/settings.json`:

```jsonc
{
  "defaultProvider": "cc-switch-<name>",   // Managed: set on provider switch
  "defaultModel": "<model_id>",             // Managed: set on provider switch
  "defaultThinkingLevel": "medium",         // Managed: user-editable in UI
  "hideThinkingBlock": false,               // Managed: user-editable toggle
  "theme": "dark",                          // Managed: user-editable dropdown
  "quietStartup": false,                    // Managed: user-editable toggle
  "compaction": {                           // Managed: user-editable
    "enabled": true
  },
  "retry": {                                // Managed: user-editable
    "enabled": true,
    "maxRetries": 3
  }
  // All other fields in settings.json are preserved untouched
}
```

### Merge Contract

- CC Switch reads existing `settings.json` before writing
- CC Switch only modifies the fields listed above
- All other fields in the file are preserved unchanged
- If `settings.json` does not exist, CC Switch creates it with only the managed fields
- Write is atomic: temporary file → validate → rename

## Contract 3: Skills Sync Path

```
CC Switch SSOT: ~/.cc-switch/skills/<owner>--<repo>--<dir>/
Pi target:       ~/.pi/agent/skills/<dir>/
```

When a Skill is enabled for `AppType::Pi`, CC Switch syncs the Skill files to the Pi target directory using the configured sync method (symlink or copy).
