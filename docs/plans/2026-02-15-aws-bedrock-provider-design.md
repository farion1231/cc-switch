# AWS Bedrock Provider Integration Design

**Date**: 2026-02-15
**Author**: Keith (AWS AI Expert)
**Status**: Approved

## Overview

Add AWS Bedrock as a provider in cc-switch, supporting both AKSK (Access Key / Secret Key) and Bedrock API Key authentication methods. This is a Preset-Only integration (方案 A) that leverages Claude Code's native Bedrock support without modifying the Rust backend.

## Requirements

1. Support AWS Bedrock as a provider in cc-switch
2. Two authentication methods: AKSK and Bedrock API Key (equal priority)
3. Default region: `us-west-2`, user-configurable
4. Support cross-region inference model IDs (`global.*`, `us.*`, `eu.*`, `apac.*`)
5. All existing tests must pass after changes

## Approach: Preset-Only Integration

Add two provider presets to `src/config/claudeProviderPresets.ts`. No Rust backend changes required. Claude Code handles Bedrock SigV4 signing natively when `CLAUDE_CODE_USE_BEDROCK=1` is set.

### Why Preset-Only

- Minimal code changes, low risk
- Consistent with existing provider architecture
- Claude Code already supports Bedrock natively
- Tests remain simple and focused

## Provider Presets

### Preset 1: AWS Bedrock (AKSK)

Authentication via AWS Access Key ID and Secret Access Key.

```typescript
{
  name: "AWS Bedrock (AKSK)",
  icon: "aws",
  iconColor: "#FF9900",
  websiteUrl: "https://aws.amazon.com/bedrock/",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
      AWS_ACCESS_KEY_ID: "${AWS_ACCESS_KEY_ID}",
      AWS_SECRET_ACCESS_KEY: "${AWS_SECRET_ACCESS_KEY}",
      AWS_REGION: "${AWS_REGION}",
      ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
      CLAUDE_CODE_USE_BEDROCK: "1"
    }
  },
  templateValues: {
    AWS_REGION: { label: "AWS Region", placeholder: "us-west-2", editorValue: "us-west-2" },
    AWS_ACCESS_KEY_ID: { label: "Access Key ID", placeholder: "AKIA...", editorValue: "" },
    AWS_SECRET_ACCESS_KEY: { label: "Secret Access Key", placeholder: "your-secret-key", editorValue: "", secret: true }
  },
  models: {
    model: "global.anthropic.claude-opus-4-6-v1",
    haikuModel: "global.anthropic.claude-haiku-4-5-20251001-v1:0",
    sonnetModel: "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
    opusModel: "global.anthropic.claude-opus-4-6-v1"
  }
}
```

### Preset 2: AWS Bedrock (API Key)

Authentication via Bedrock API Key.

```typescript
{
  name: "AWS Bedrock (API Key)",
  icon: "aws",
  iconColor: "#FF9900",
  websiteUrl: "https://aws.amazon.com/bedrock/",
  settingsConfig: {
    apiKey: "${BEDROCK_API_KEY}",
    env: {
      ANTHROPIC_BASE_URL: "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
      AWS_REGION: "${AWS_REGION}",
      ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
      CLAUDE_CODE_USE_BEDROCK: "1"
    }
  },
  templateValues: {
    AWS_REGION: { label: "AWS Region", placeholder: "us-west-2", editorValue: "us-west-2" },
    BEDROCK_API_KEY: { label: "Bedrock API Key", placeholder: "your-bedrock-api-key", editorValue: "", secret: true }
  },
  models: {
    model: "global.anthropic.claude-opus-4-6-v1",
    haikuModel: "global.anthropic.claude-haiku-4-5-20251001-v1:0",
    sonnetModel: "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
    opusModel: "global.anthropic.claude-opus-4-6-v1"
  }
}
```

## Model Catalog

Default models available in presets:

| Model | Model ID |
|-------|----------|
| Claude Opus 4.6 | `global.anthropic.claude-opus-4-6-v1` |
| Claude Opus 4.6 (1M context) | `global.anthropic.claude-opus-4-6-v1[1m]` |
| Claude Sonnet 4.5 | `global.anthropic.claude-sonnet-4-5-20250929-v1:0` |
| Claude Haiku 4.5 | `global.anthropic.claude-haiku-4-5-20251001-v1:0` |
| Qwen3 Coder 480B | `qwen.qwen3-coder-480b-a35b-v1:0` |
| MiniMax M2.1 | `minimax.minimax-m2.1` |
| DeepSeek R1 | `us.deepseek.r1-v1:0` |
| Amazon Nova Pro | `us.amazon.nova-pro-v1:0` |
| Amazon Nova Lite | `us.amazon.nova-lite-v1:0` |
| Meta Llama 4 Maverick | `us.meta.llama4-maverick-17b-instruct-v1:0` |

Users can manually enter any Bedrock model ID, including cross-region inference profiles.

## Region Handling

- Default region: `us-west-2`
- User-configurable via template variable `${AWS_REGION}`
- Region injected into `ANTHROPIC_BASE_URL` and `AWS_REGION` env var
- Cross-region inference: use `global.*`, `us.*`, `eu.*`, `apac.*` prefixed model IDs

## Testing Strategy

### Must Pass

- `pnpm test:unit` — all existing unit tests
- `pnpm typecheck` — TypeScript type checking
- `pnpm format:check` — code formatting

### New Tests

- Validate Bedrock AKSK preset contains required template variables
- Validate Bedrock API Key preset contains required template variables
- Validate template variable substitution in URL
- Validate default model ID format

### Out of Scope

- E2E integration tests (requires live Bedrock environment)
- Rust backend tests (no backend changes)

## Files to Modify

1. `src/config/claudeProviderPresets.ts` — add two Bedrock presets
2. Existing test files for preset validation (if applicable)

## Non-Goals

- No Rust backend changes
- No new `BedrockAdapter` in proxy layer
- No SigV4 signing implementation (handled by Claude Code natively)
- No changes to MCP, Skills, or other cc-switch subsystems
