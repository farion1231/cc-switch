# feat: Add AWS Bedrock Provider Support

## Summary

Add AWS Bedrock as a first-class cloud provider in cc-switch, supporting both Claude Code and OpenCode with two authentication methods (AKSK and API Key). Includes cross-region inference model support.

## Motivation

AWS Bedrock is a major cloud AI platform that provides access to Claude, Nova, Llama, DeepSeek, and other models through a unified API. Many enterprise users access AI models through Bedrock rather than direct API endpoints. Adding Bedrock support enables cc-switch users to manage their Bedrock-based AI CLI configurations alongside other providers.

## Changes

### New Provider Category

Added `cloud_provider` to `ProviderCategory` type to properly categorize cloud platform providers (distinct from `official`, `aggregator`, or `third_party`).

### Claude Code Presets (2 new presets)

**AWS Bedrock (AKSK)**
- Authentication via AWS Access Key ID + Secret Access Key
- Sets `CLAUDE_CODE_USE_BEDROCK=1` to enable Claude Code's native Bedrock support
- `ANTHROPIC_BASE_URL` templated with `${AWS_REGION}` for region flexibility
- Default region: `us-west-2` (user-configurable)

**AWS Bedrock (API Key)**
- Authentication via Bedrock API Key
- Same Bedrock URL templating and model defaults
- Simpler setup for users with Bedrock API Key access

### OpenCode Preset (1 new preset)

**AWS Bedrock**
- Added `@ai-sdk/amazon-bedrock` to `opencodeNpmPackages` list
- Added 6 model variants to `OPENCODE_PRESET_MODEL_VARIANTS`
- Uses `region`, `accessKeyId`, `secretAccessKey` options (matching `@ai-sdk/amazon-bedrock` API)

### Default Models

All presets include cross-region inference model IDs:

| Model | ID |
|-------|----|
| Claude Opus 4.6 | `global.anthropic.claude-opus-4-6-v1` |
| Claude Sonnet 4.5 | `global.anthropic.claude-sonnet-4-5-20250929-v1:0` |
| Claude Haiku 4.5 | `global.anthropic.claude-haiku-4-5-20251001-v1:0` |
| Amazon Nova Pro | `us.amazon.nova-pro-v1:0` |
| Meta Llama 4 Maverick | `us.meta.llama4-maverick-17b-instruct-v1:0` |
| DeepSeek R1 | `us.deepseek.r1-v1:0` |

Users can manually enter any Bedrock model ID, including `global.*`, `us.*`, `eu.*`, `apac.*` cross-region profiles.

## Files Changed

```
src/types.ts                                 |  +1  (add cloud_provider category)
src/config/claudeProviderPresets.ts          | +75  (2 Bedrock presets)
src/config/opencodeProviderPresets.ts        | +93  (npm pkg + model variants + preset)
tests/config/claudeProviderPresets.test.ts   | +79  (11 tests)
tests/config/opencodeProviderPresets.test.ts | +68  (8 tests)
```

**Total: 5 files, +316 lines, 0 deletions**

## Testing

- 19 new unit tests (11 Claude + 8 OpenCode)
- All 159/159 existing tests pass
- TypeScript typecheck passes
- No Rust backend changes required

## Commits

```
a1b518f feat: add cloud_provider category to ProviderCategory type
6381e2f feat: add AWS Bedrock (AKSK) Claude Code provider preset with tests
5e18fb8 feat: add AWS Bedrock (API Key) Claude Code provider preset with tests
9ff2939 feat: add AWS Bedrock OpenCode provider preset with @ai-sdk/amazon-bedrock
```

## Architecture Decision

**Preset-Only integration** (no Rust backend changes):
- Claude Code natively handles Bedrock SigV4 signing when `CLAUDE_CODE_USE_BEDROCK=1` is set
- OpenCode uses `@ai-sdk/amazon-bedrock` which handles AWS authentication natively
- Minimal risk, consistent with existing provider architecture
- Future: Codex and Gemini CLI support would require proxy-layer changes (out of scope)
