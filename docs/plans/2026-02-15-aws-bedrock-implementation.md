# AWS Bedrock Provider Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add AWS Bedrock as a provider in cc-switch for both Claude Code and OpenCode, with AKSK and API Key authentication support.

**Architecture:** Preset-Only integration — add provider presets to Claude and OpenCode preset arrays, add `@ai-sdk/amazon-bedrock` to OpenCode npm packages, and add a new "cloud_provider" category to ProviderCategory. No Rust backend changes.

**Tech Stack:** TypeScript, Vitest, React (existing cc-switch stack)

---

### Task 1: Add "cloud_provider" category to ProviderCategory type

**Files:**
- Modify: `src/types.ts:1-7`

**Step 1: Add the new category**

In `src/types.ts`, add `"cloud_provider"` to the `ProviderCategory` union type:

```typescript
export type ProviderCategory =
  | "official" // 官方
  | "cn_official" // 开源官方（原"国产官方"）
  | "cloud_provider" // 云服务商（AWS Bedrock 等）
  | "aggregator" // 聚合网站
  | "third_party" // 第三方供应商
  | "custom" // 自定义
  | "omo"; // Oh My OpenCode
```

**Step 2: Run typecheck to verify**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm typecheck`
Expected: PASS (no errors)

**Step 3: Commit**

```bash
git add src/types.ts
git commit -m "feat: add cloud_provider category to ProviderCategory type"
```

---

### Task 2: Add AWS Bedrock (AKSK) Claude Code preset

**Files:**
- Create: `tests/config/claudeProviderPresets.test.ts`
- Modify: `src/config/claudeProviderPresets.ts` (add to `providerPresets` array)

**Step 1: Write the failing test**

Create file `tests/config/claudeProviderPresets.test.ts`:

```typescript
import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";

describe("AWS Bedrock Provider Presets", () => {
  const bedrockAksk = providerPresets.find(
    (p) => p.name === "AWS Bedrock (AKSK)",
  );

  it("should include AWS Bedrock (AKSK) preset", () => {
    expect(bedrockAksk).toBeDefined();
  });

  it("AKSK preset should have required AWS env variables", () => {
    const env = (bedrockAksk!.settingsConfig as any).env;
    expect(env).toHaveProperty("AWS_ACCESS_KEY_ID");
    expect(env).toHaveProperty("AWS_SECRET_ACCESS_KEY");
    expect(env).toHaveProperty("AWS_REGION");
    expect(env).toHaveProperty("CLAUDE_CODE_USE_BEDROCK", "1");
  });

  it("AKSK preset should have template values for AWS credentials", () => {
    expect(bedrockAksk!.templateValues).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_ACCESS_KEY_ID).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_SECRET_ACCESS_KEY).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_REGION).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_REGION.editorValue).toBe(
      "us-west-2",
    );
  });

  it("AKSK preset should have correct base URL template", () => {
    const env = (bedrockAksk!.settingsConfig as any).env;
    expect(env.ANTHROPIC_BASE_URL).toContain("bedrock-runtime");
    expect(env.ANTHROPIC_BASE_URL).toContain("${AWS_REGION}");
  });

  it("AKSK preset should have cloud_provider category", () => {
    expect(bedrockAksk!.category).toBe("cloud_provider");
  });

  it("AKSK preset should have Bedrock model as default", () => {
    const env = (bedrockAksk!.settingsConfig as any).env;
    expect(env.ANTHROPIC_MODEL).toContain("anthropic.claude");
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit -- tests/config/claudeProviderPresets.test.ts`
Expected: FAIL — "AWS Bedrock (AKSK)" preset not found

**Step 3: Add the AKSK preset to the presets array**

In `src/config/claudeProviderPresets.ts`, add the following entry after the last preset (before the closing `];` of the `providerPresets` array):

```typescript
  {
    name: "AWS Bedrock (AKSK)",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        AWS_ACCESS_KEY_ID: "${AWS_ACCESS_KEY_ID}",
        AWS_SECRET_ACCESS_KEY: "${AWS_SECRET_ACCESS_KEY}",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL:
          "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-4-6-v1",
        CLAUDE_CODE_USE_BEDROCK: "1",
      },
    },
    category: "cloud_provider",
    templateValues: {
      AWS_REGION: {
        label: "AWS Region",
        placeholder: "us-west-2",
        editorValue: "us-west-2",
      },
      AWS_ACCESS_KEY_ID: {
        label: "Access Key ID",
        placeholder: "AKIA...",
        editorValue: "",
      },
      AWS_SECRET_ACCESS_KEY: {
        label: "Secret Access Key",
        placeholder: "your-secret-key",
        editorValue: "",
      },
    },
    icon: "aws",
    iconColor: "#FF9900",
  },
```

**Step 4: Run test to verify it passes**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit -- tests/config/claudeProviderPresets.test.ts`
Expected: PASS — all 6 tests pass

**Step 5: Commit**

```bash
git add src/config/claudeProviderPresets.ts tests/config/claudeProviderPresets.test.ts
git commit -m "feat: add AWS Bedrock (AKSK) Claude Code provider preset with tests"
```

---

### Task 3: Add AWS Bedrock (API Key) Claude Code preset

**Files:**
- Modify: `src/config/claudeProviderPresets.ts` (add to `providerPresets` array)
- Modify: `tests/config/claudeProviderPresets.test.ts` (add API Key tests)

**Step 1: Add tests for the API Key preset**

Append the following test block to `tests/config/claudeProviderPresets.test.ts`, inside the existing `describe` block:

```typescript
  const bedrockApiKey = providerPresets.find(
    (p) => p.name === "AWS Bedrock (API Key)",
  );

  it("should include AWS Bedrock (API Key) preset", () => {
    expect(bedrockApiKey).toBeDefined();
  });

  it("API Key preset should have apiKey template and AWS env variables", () => {
    const config = bedrockApiKey!.settingsConfig as any;
    expect(config).toHaveProperty("apiKey", "${BEDROCK_API_KEY}");
    expect(config.env).toHaveProperty("AWS_REGION");
    expect(config.env).toHaveProperty("CLAUDE_CODE_USE_BEDROCK", "1");
  });

  it("API Key preset should NOT have AKSK env variables", () => {
    const env = (bedrockApiKey!.settingsConfig as any).env;
    expect(env).not.toHaveProperty("AWS_ACCESS_KEY_ID");
    expect(env).not.toHaveProperty("AWS_SECRET_ACCESS_KEY");
  });

  it("API Key preset should have template values for API key and region", () => {
    expect(bedrockApiKey!.templateValues).toBeDefined();
    expect(bedrockApiKey!.templateValues!.BEDROCK_API_KEY).toBeDefined();
    expect(bedrockApiKey!.templateValues!.AWS_REGION).toBeDefined();
    expect(bedrockApiKey!.templateValues!.AWS_REGION.editorValue).toBe(
      "us-west-2",
    );
  });

  it("API Key preset should have cloud_provider category", () => {
    expect(bedrockApiKey!.category).toBe("cloud_provider");
  });
```

**Step 2: Run test to verify new tests fail**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit -- tests/config/claudeProviderPresets.test.ts`
Expected: FAIL — "AWS Bedrock (API Key)" preset not found

**Step 3: Add the API Key preset**

In `src/config/claudeProviderPresets.ts`, add the following entry right after the AKSK preset:

```typescript
  {
    name: "AWS Bedrock (API Key)",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      apiKey: "${BEDROCK_API_KEY}",
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL:
          "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-4-6-v1",
        CLAUDE_CODE_USE_BEDROCK: "1",
      },
    },
    category: "cloud_provider",
    templateValues: {
      AWS_REGION: {
        label: "AWS Region",
        placeholder: "us-west-2",
        editorValue: "us-west-2",
      },
      BEDROCK_API_KEY: {
        label: "Bedrock API Key",
        placeholder: "your-bedrock-api-key",
        editorValue: "",
      },
    },
    icon: "aws",
    iconColor: "#FF9900",
  },
```

**Step 4: Run test to verify all pass**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit -- tests/config/claudeProviderPresets.test.ts`
Expected: PASS — all 11 tests pass

**Step 5: Commit**

```bash
git add src/config/claudeProviderPresets.ts tests/config/claudeProviderPresets.test.ts
git commit -m "feat: add AWS Bedrock (API Key) Claude Code provider preset with tests"
```

---

### Task 4: Add AWS Bedrock OpenCode preset with @ai-sdk/amazon-bedrock

**Files:**
- Modify: `src/config/opencodeProviderPresets.ts` (add npm package + model variants + preset)
- Create: `tests/config/opencodeProviderPresets.test.ts`

**Step 1: Write the failing test**

Create file `tests/config/opencodeProviderPresets.test.ts`:

```typescript
import { describe, expect, it } from "vitest";
import {
  opencodeProviderPresets,
  opencodeNpmPackages,
  OPENCODE_PRESET_MODEL_VARIANTS,
} from "@/config/opencodeProviderPresets";

describe("AWS Bedrock OpenCode Provider Presets", () => {
  it("should include @ai-sdk/amazon-bedrock in npm packages", () => {
    const bedrockPkg = opencodeNpmPackages.find(
      (p) => p.value === "@ai-sdk/amazon-bedrock",
    );
    expect(bedrockPkg).toBeDefined();
    expect(bedrockPkg!.label).toBe("Amazon Bedrock");
  });

  it("should include Bedrock model variants", () => {
    const variants = OPENCODE_PRESET_MODEL_VARIANTS["@ai-sdk/amazon-bedrock"];
    expect(variants).toBeDefined();
    expect(variants.length).toBeGreaterThan(0);

    const opusModel = variants.find((v) =>
      v.id.includes("anthropic.claude-opus-4-6"),
    );
    expect(opusModel).toBeDefined();
  });

  const bedrockPreset = opencodeProviderPresets.find(
    (p) => p.name === "AWS Bedrock",
  );

  it("should include AWS Bedrock preset", () => {
    expect(bedrockPreset).toBeDefined();
  });

  it("Bedrock preset should use @ai-sdk/amazon-bedrock npm package", () => {
    expect(bedrockPreset!.settingsConfig.npm).toBe(
      "@ai-sdk/amazon-bedrock",
    );
  });

  it("Bedrock preset should have region in options", () => {
    expect(bedrockPreset!.settingsConfig.options).toHaveProperty("region");
  });

  it("Bedrock preset should have cloud_provider category", () => {
    expect(bedrockPreset!.category).toBe("cloud_provider");
  });

  it("Bedrock preset should have template values for AWS credentials", () => {
    expect(bedrockPreset!.templateValues).toBeDefined();
    expect(bedrockPreset!.templateValues!.region).toBeDefined();
    expect(bedrockPreset!.templateValues!.region.editorValue).toBe(
      "us-west-2",
    );
    expect(bedrockPreset!.templateValues!.accessKeyId).toBeDefined();
    expect(bedrockPreset!.templateValues!.secretAccessKey).toBeDefined();
  });

  it("Bedrock preset should include Claude models", () => {
    const models = bedrockPreset!.settingsConfig.models;
    expect(models).toBeDefined();
    const modelIds = Object.keys(models!);
    expect(
      modelIds.some((id) => id.includes("anthropic.claude")),
    ).toBe(true);
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit -- tests/config/opencodeProviderPresets.test.ts`
Expected: FAIL — @ai-sdk/amazon-bedrock not found

**Step 3: Add @ai-sdk/amazon-bedrock to npm packages list**

In `src/config/opencodeProviderPresets.ts`, update `opencodeNpmPackages`:

```typescript
export const opencodeNpmPackages = [
  { value: "@ai-sdk/openai", label: "OpenAI" },
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/anthropic", label: "Anthropic" },
  { value: "@ai-sdk/amazon-bedrock", label: "Amazon Bedrock" },
  { value: "@ai-sdk/google", label: "Google (Gemini)" },
] as const;
```

**Step 4: Add Bedrock model variants**

In `src/config/opencodeProviderPresets.ts`, add the following entry to `OPENCODE_PRESET_MODEL_VARIANTS`:

```typescript
  "@ai-sdk/amazon-bedrock": [
    {
      id: "global.anthropic.claude-opus-4-6-v1",
      name: "Claude Opus 4.6",
      contextLimit: 1000000,
      outputLimit: 128000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
    },
    {
      id: "global.anthropic.claude-sonnet-4-5-20250929-v1:0",
      name: "Claude Sonnet 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
    },
    {
      id: "global.anthropic.claude-haiku-4-5-20251001-v1:0",
      name: "Claude Haiku 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
    },
    {
      id: "us.amazon.nova-pro-v1:0",
      name: "Amazon Nova Pro",
      contextLimit: 300000,
      outputLimit: 5000,
      modalities: { input: ["text", "image"], output: ["text"] },
    },
    {
      id: "us.meta.llama4-maverick-17b-instruct-v1:0",
      name: "Meta Llama 4 Maverick",
      contextLimit: 131072,
      outputLimit: 131072,
      modalities: { input: ["text"], output: ["text"] },
    },
    {
      id: "us.deepseek.r1-v1:0",
      name: "DeepSeek R1",
      contextLimit: 131072,
      outputLimit: 131072,
      modalities: { input: ["text"], output: ["text"] },
    },
  ],
```

**Step 5: Add the Bedrock preset**

In `src/config/opencodeProviderPresets.ts`, add the following entry to `opencodeProviderPresets` array (before the "OpenAI Compatible" custom template entry):

```typescript
  {
    name: "AWS Bedrock",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      npm: "@ai-sdk/amazon-bedrock",
      name: "AWS Bedrock",
      options: {
        region: "${region}",
        accessKeyId: "${accessKeyId}",
        secretAccessKey: "${secretAccessKey}",
      },
      models: {
        "global.anthropic.claude-opus-4-6-v1": { name: "Claude Opus 4.6" },
        "global.anthropic.claude-sonnet-4-5-20250929-v1:0": {
          name: "Claude Sonnet 4.5",
        },
        "global.anthropic.claude-haiku-4-5-20251001-v1:0": {
          name: "Claude Haiku 4.5",
        },
        "us.amazon.nova-pro-v1:0": { name: "Amazon Nova Pro" },
        "us.meta.llama4-maverick-17b-instruct-v1:0": {
          name: "Meta Llama 4 Maverick",
        },
        "us.deepseek.r1-v1:0": { name: "DeepSeek R1" },
      },
    },
    category: "cloud_provider",
    icon: "aws",
    iconColor: "#FF9900",
    templateValues: {
      region: {
        label: "AWS Region",
        placeholder: "us-west-2",
        defaultValue: "us-west-2",
        editorValue: "us-west-2",
      },
      accessKeyId: {
        label: "Access Key ID",
        placeholder: "AKIA...",
        editorValue: "",
      },
      secretAccessKey: {
        label: "Secret Access Key",
        placeholder: "your-secret-key",
        editorValue: "",
      },
    },
  },
```

**Step 6: Run test to verify all pass**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit -- tests/config/opencodeProviderPresets.test.ts`
Expected: PASS — all 8 tests pass

**Step 7: Commit**

```bash
git add src/config/opencodeProviderPresets.ts tests/config/opencodeProviderPresets.test.ts
git commit -m "feat: add AWS Bedrock OpenCode provider preset with @ai-sdk/amazon-bedrock"
```

---

### Task 5: Run full test suite and verify

**Files:**
- None (verification only)

**Step 1: Run TypeScript type checking**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm typecheck`
Expected: PASS (no type errors)

**Step 2: Run code format check**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm format:check`
Expected: PASS (or fix with `pnpm format` then re-check)

**Step 3: Run full unit test suite**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit`
Expected: PASS — all existing tests pass, plus 19 new Bedrock tests

**Step 4: Fix any issues and commit**

If format check fails:
```bash
cd /root/keith-space/github-search/cc-switch && pnpm format
git add -A
git commit -m "style: format code"
```

---

### Task 6: Final verification

**Files:**
- None

**Step 1: Verify git status is clean**

Run: `cd /root/keith-space/github-search/cc-switch && git status`
Expected: Clean working directory

**Step 2: Review all changes**

Run: `cd /root/keith-space/github-search/cc-switch && git log --oneline -6`
Expected: See the 4-5 commits from this implementation

**Step 3: Run full test suite one final time**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit`
Expected: All tests PASS
