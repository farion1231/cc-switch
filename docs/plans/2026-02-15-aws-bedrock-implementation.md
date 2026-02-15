# AWS Bedrock Provider Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add AWS Bedrock as a provider in cc-switch with AKSK and API Key authentication support.

**Architecture:** Preset-Only integration — add two provider presets (AKSK and API Key) to the existing Claude provider presets array. Add a new "cloud_provider" category to the ProviderCategory type. No Rust backend changes.

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

### Task 2: Add AWS Bedrock (AKSK) preset

**Files:**
- Modify: `src/config/claudeProviderPresets.ts:53-536` (add to `providerPresets` array)

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
git commit -m "feat: add AWS Bedrock (AKSK) provider preset with tests"
```

---

### Task 3: Add AWS Bedrock (API Key) preset

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
git commit -m "feat: add AWS Bedrock (API Key) provider preset with tests"
```

---

### Task 4: Run full test suite and verify

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
Expected: PASS — all existing tests pass, plus 11 new Bedrock tests

**Step 4: Fix any issues and commit**

If format check fails:
```bash
cd /root/keith-space/github-search/cc-switch && pnpm format
git add -A
git commit -m "style: format code"
```

---

### Task 5: Final verification commit

**Files:**
- None

**Step 1: Verify git status is clean**

Run: `cd /root/keith-space/github-search/cc-switch && git status`
Expected: Clean working directory

**Step 2: Review all changes**

Run: `cd /root/keith-space/github-search/cc-switch && git log --oneline -5`
Expected: See the 3-4 commits from this implementation

**Step 3: Run full test suite one final time**

Run: `cd /root/keith-space/github-search/cc-switch && pnpm test:unit`
Expected: All tests PASS
