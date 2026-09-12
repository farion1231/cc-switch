import { describe, expect, it } from "vitest";

import type { ModelsDevModel, ModelsDevResponse } from "@/lib/modelsDevPricing";
import {
  applyModelsDevCapabilities,
  buildVariantForEffort,
  buildVariantsForModel,
  findModelsDevEntry,
  injectedEffortsForModel,
  nextNumberedKey,
  transformModelsDevEntry,
} from "@/lib/opencodeModelCapabilities";
import type { OpenCodeModel } from "@/types";

// 与 models.dev 真实条目同构的 grok-4.6 样本（2026-08 抓取）；
// as 断言：description/knowledge/structured_output 等本就是类型契约外的原始目录字段
const GROK_46 = {
  id: "grok-4.6",
  name: "Grok 4.6",
  description: "xAI's frontier model",
  family: "grok",
  attachment: true,
  reasoning: true,
  reasoning_options: [
    { type: "effort", values: ["low", "medium", "high", "xhigh"] },
  ],
  tool_call: true,
  structured_output: true,
  temperature: true,
  knowledge: "2026-02-01",
  release_date: "2026-08-12",
  last_updated: "2026-08-12",
  modalities: { input: ["text", "image", "pdf"], output: ["text"] },
  open_weights: false,
  limit: { context: 500000, output: 500000 },
  cost: {
    input: 2,
    output: 6,
    cache_read: 0.5,
    tiers: [
      {
        input: 4,
        output: 12,
        cache_read: 1,
        tier: { type: "context", size: 200000 },
      },
    ],
  },
};

// gpt-5.6-sol 样本：limit 含 input 字段、reasoning_options 含 max 档
const GPT_56_SOL: ModelsDevModel = {
  id: "gpt-5.6-sol",
  name: "GPT-5.6 Sol",
  family: "gpt-sol",
  release_date: "2026-07-09",
  attachment: true,
  reasoning: true,
  temperature: false,
  tool_call: true,
  reasoning_options: [
    {
      type: "effort",
      values: ["none", "low", "medium", "high", "xhigh", "max"],
    },
  ],
  modalities: { input: ["text", "image", "pdf"], output: ["text"] },
  limit: { context: 1050000, input: 922000, output: 128000 },
  cost: { input: 4, output: 20, cache_read: 0.4, cache_write: 5 },
};

const CLAUDE_OPUS_46: ModelsDevModel = {
  id: "claude-opus-4-6",
  name: "Claude Opus 4.6",
  reasoning: true,
  reasoning_options: [
    { type: "effort", values: ["low", "medium", "high", "max"] },
    { type: "budget_tokens", min: 1024 },
  ],
  limit: { context: 1000000, output: 128000 },
};

const CLAUDE_OPUS_45: ModelsDevModel = {
  id: "claude-opus-4-5-20251101",
  name: "Claude Opus 4.5",
  reasoning: true,
  reasoning_options: [
    { type: "effort", values: ["low", "medium", "high"] },
    { type: "budget_tokens", min: 1024 },
  ],
  limit: { context: 200000, output: 64000 },
};

const CLAUDE_SONNET_45: ModelsDevModel = {
  id: "claude-sonnet-4-5-20250929",
  name: "Claude Sonnet 4.5",
  reasoning: true,
  reasoning_options: { type: "budget_tokens", min: 1024 },
  limit: { context: 200000, output: 64000 },
};

const GEMINI_36_FLASH: ModelsDevModel = {
  id: "gemini-3.6-flash",
  name: "Gemini 3.6 Flash",
  reasoning: true,
  reasoning_options: {
    type: "effort",
    values: ["minimal", "low", "medium", "high"],
  },
  limit: { context: 1048576, output: 65536 },
};

const GEMINI_25_FLASH: ModelsDevModel = {
  id: "gemini-2.5-flash",
  name: "Gemini 2.5 Flash",
  reasoning: true,
  reasoning_options: [
    { type: "toggle" },
    { type: "budget_tokens", min: 0, max: 24576 },
  ],
  limit: { context: 1048576, output: 65536 },
};

const GEMINI_37_FLASH: ModelsDevModel = {
  id: "gemini-3.7-flash",
  name: "Gemini 3.7 Flash",
  reasoning: true,
  reasoning_options: {
    type: "effort",
    values: ["low", "medium", "high"],
  },
  limit: { context: 1048576, output: 65536 },
};

const EMPTY_REASONING_OPTIONS: ModelsDevModel = {
  id: "gemini-3.7-flash",
  name: "Gemini 3.7 Flash",
  reasoning: true,
  reasoning_options: [],
  limit: { context: 1048576, output: 65536 },
};

const BEDROCK_CLAUDE_47: ModelsDevModel = {
  id: "global.anthropic.claude-opus-4-7",
  name: "Claude Opus 4.7 (Global)",
  reasoning: true,
  reasoning_options: {
    type: "effort",
    values: ["low", "medium", "high", "xhigh", "max"],
  },
  limit: { context: 1000000, output: 128000 },
};

const BEDROCK_NOVA: ModelsDevModel = {
  id: "amazon.nova-2-lite-v1:0",
  name: "Nova 2 Lite",
  reasoning: true,
  reasoning_options: [
    { type: "toggle" },
    { type: "effort", values: ["low", "medium", "high"] },
  ],
  limit: { context: 128000, output: 4096 },
};

const GEMMA_TOGGLE_ONLY: ModelsDevModel = {
  id: "gemma-4-26b-a4b-it",
  name: "Gemma 4 26B",
  reasoning: true,
  reasoning_options: { type: "toggle" },
  limit: { context: 131072, output: 8192 },
};

const UNKNOWN_ANTHROPIC_EFFORT_MODEL: ModelsDevModel = {
  id: "gateway-reasoner",
  name: "Gateway Reasoner",
  reasoning: true,
  reasoning_options: { type: "effort", values: ["low", "high"] },
  limit: { context: 128000, output: 8192 },
};

describe("injectedEffortsForModel", () => {
  it("复刻当前 OpenCode：gpt-5.6 使用 none/xhigh，不再注入 minimal", () => {
    expect(injectedEffortsForModel("gpt-5.6-sol", "2026-07-09")).toEqual([
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
  });

  it("非 gpt-5 模型按发布日期得到 none/xhigh", () => {
    expect(injectedEffortsForModel("grok-4.6", "2026-08-12")).toEqual([
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
  });

  it("特殊系列只注入单一档位", () => {
    expect(injectedEffortsForModel("gpt-5.6-deep-research")).toEqual([
      "medium",
    ]);
    expect(injectedEffortsForModel("gpt-5.6-chat", "2026-07-09")).toEqual([
      "medium",
    ]);
    expect(injectedEffortsForModel("gpt-5-pro", "2026-07-09")).toEqual([
      "high",
    ]);
  });

  it("无日期的基础模型为 low/medium/high", () => {
    expect(injectedEffortsForModel("some-model")).toEqual([
      "low",
      "medium",
      "high",
    ]);
  });
});

describe("transformModelsDevEntry", () => {
  it("按 opencode schema 白名单裁剪，丢弃 schema 外字段", () => {
    const out = transformModelsDevEntry(GROK_46);
    expect(Object.keys(out).sort()).toEqual([
      "attachment",
      "cost",
      "family",
      "limit",
      "modalities",
      "name",
      "reasoning",
      "release_date",
      "temperature",
      "tool_call",
    ]);
    expect(out).not.toHaveProperty("description");
    expect(out).not.toHaveProperty("knowledge");
    expect(out).not.toHaveProperty("last_updated");
    expect(out).not.toHaveProperty("open_weights");
    expect(out).not.toHaveProperty("structured_output");
    expect(out).not.toHaveProperty("reasoning_options");
    expect(out).not.toHaveProperty("id");
  });

  it("cost 无 context_over_200k 时从 tiers 转换", () => {
    const out = transformModelsDevEntry(GROK_46);
    expect(out.cost).toEqual({
      input: 2,
      output: 6,
      cache_read: 0.5,
      context_over_200k: { input: 4, output: 12, cache_read: 1 },
    });
  });

  it("cost 已有 context_over_200k 时直接采用", () => {
    const entry: ModelsDevModel = {
      ...GROK_46,
      cost: {
        input: 2,
        output: 6,
        context_over_200k: { input: 9 },
        tiers: [
          { input: 4, output: 12, tier: { type: "context", size: 200000 } },
        ],
      },
    };
    const out = transformModelsDevEntry(entry);
    expect(out.cost).toEqual({
      input: 2,
      output: 6,
      context_over_200k: { input: 9 },
    });
  });

  it("limit 只保留 context/input/output", () => {
    const entry: ModelsDevModel = {
      ...GROK_46,
      limit: { context: 100, input: 50, output: 80 } as ModelsDevModel["limit"],
    };
    expect(transformModelsDevEntry(entry).limit).toEqual({
      context: 100,
      input: 50,
      output: 80,
    });
  });
});

describe("buildVariantsForModel", () => {
  it("@ai-sdk/openai 生成三件套 plain 键并屏蔽注入但不支持的档位", () => {
    const result = buildVariantsForModel(GROK_46, "@ai-sdk/openai", "grok-4.6");
    expect(result).not.toBeNull();
    // grok-4.6 release 2026-08-12 → 注入集 [none, low, medium, high, xhigh]；
    // 官方仅声明 low/medium/high/xhigh，none 被屏蔽
    expect(result?.efforts).toEqual(["low", "medium", "high", "xhigh"]);
    expect(result?.suppressed).toEqual(["none"]);
    expect(Object.keys(result?.variants ?? {})).toEqual([
      "low",
      "medium",
      "high",
      "xhigh",
      "none",
    ]);
    expect(result?.variants.low).toEqual({
      reasoningEffort: "low",
      reasoningSummary: "auto",
      include: ["reasoning.encrypted_content"],
    });
    expect(result?.variants.none).toEqual({ disabled: true });
  });

  it("@ai-sdk/openai-compatible 仅生成 reasoningEffort", () => {
    const result = buildVariantsForModel(
      GROK_46,
      "@ai-sdk/openai-compatible",
      "grok-4.6",
    );
    // 当前 OpenCode 对通用兼容接口为 low/medium/high；不额外猜测 max。
    expect(result?.suppressed).toEqual([]);
    expect(result?.variants).not.toHaveProperty("max");
    expect(result?.variants.low).toEqual({ reasoningEffort: "low" });
  });

  it("OpenAI Compatible 屏蔽默认生成但 models.dev 未声明的档位", () => {
    const result = buildVariantsForModel(
      {
        id: "gpt-5-pro",
        reasoning_options: { type: "effort", values: ["high"] },
      },
      "@ai-sdk/openai-compatible",
      "gpt-5-pro",
    );
    expect(result?.variants.high).toEqual({ reasoningEffort: "high" });
    expect(result?.variants.low).toEqual({ disabled: true });
    expect(result?.variants.medium).toEqual({ disabled: true });
    expect(result?.suppressed).toEqual(["low", "medium"]);
  });

  it("无 reasoning_options(effort) 时返回 null", () => {
    expect(
      buildVariantsForModel({ name: "m" }, "@ai-sdk/openai", "m"),
    ).toBeNull();
  });

  it("Anthropic effort 使用 effort + thinking，而不是 reasoningEffort", () => {
    const result = buildVariantsForModel(
      CLAUDE_OPUS_46,
      "@ai-sdk/anthropic",
      "claude-opus-4-6",
      128000,
    );
    expect(result?.efforts).toEqual(["low", "medium", "high", "max"]);
    expect(result?.variants.high).toEqual({
      thinking: { type: "adaptive" },
      effort: "high",
    });
    expect(result?.variants.high).not.toHaveProperty("reasoningEffort");
  });

  it("旧版 Anthropic effort 同时生成预算 thinking", () => {
    const result = buildVariantsForModel(
      CLAUDE_OPUS_45,
      "@ai-sdk/anthropic",
      "claude-opus-4-5-20251101",
      64000,
    );
    expect(result?.variants.high).toEqual({
      thinking: { type: "enabled", budgetTokens: 16000 },
      effort: "high",
    });
  });

  it("Anthropic budget_tokens 生成 high/max budgetTokens", () => {
    const result = buildVariantsForModel(
      CLAUDE_SONNET_45,
      "@ai-sdk/anthropic",
      "claude-sonnet-4-5-20250929",
      64000,
    );
    expect(result?.efforts).toEqual(["high", "max"]);
    expect(result?.variants.high).toEqual({
      thinking: { type: "enabled", budgetTokens: 16000 },
    });
    expect(result?.variants.max).toEqual({
      thinking: { type: "enabled", budgetTokens: 31999 },
    });
  });

  it("Amazon Bedrock effort 使用 reasoningConfig", () => {
    const result = buildVariantsForModel(
      BEDROCK_CLAUDE_47,
      "@ai-sdk/amazon-bedrock",
      "global.anthropic.claude-opus-4-7",
      128000,
    );
    expect(result?.variants.high).toEqual({
      reasoningConfig: {
        type: "adaptive",
        maxReasoningEffort: "high",
        display: "summarized",
      },
    });
  });

  it("Amazon Bedrock 非 Anthropic effort 使用 enabled reasoningConfig", () => {
    const result = buildVariantsForModel(
      BEDROCK_NOVA,
      "@ai-sdk/amazon-bedrock",
      "amazon.nova-2-lite-v1:0",
      4096,
    );
    expect(result?.variants.high).toEqual({
      reasoningConfig: {
        type: "enabled",
        maxReasoningEffort: "high",
      },
    });
  });

  it("Google Gemini effort 使用 thinkingConfig.thinkingLevel", () => {
    const result = buildVariantsForModel(
      GEMINI_36_FLASH,
      "@ai-sdk/google",
      "gemini-3.6-flash",
    );
    expect(result?.variants.high).toEqual({
      thinkingConfig: { includeThoughts: true, thinkingLevel: "high" },
    });
  });

  it("Google Gemini 屏蔽默认生成但 models.dev 未声明的 thinkingLevel", () => {
    const result = buildVariantsForModel(
      GEMINI_37_FLASH,
      "@ai-sdk/google",
      "gemini-3.7-flash",
    );
    expect(result?.variants.minimal).toEqual({ disabled: true });
    expect(result?.suppressed).toEqual(["minimal"]);
  });

  it("Google Gemini budget_tokens 使用 thinkingConfig.thinkingBudget", () => {
    const result = buildVariantsForModel(
      GEMINI_25_FLASH,
      "@ai-sdk/google",
      "gemini-2.5-flash",
      65536,
    );
    expect(result?.efforts).toEqual(["high", "max"]);
    expect(result?.variants.high).toEqual({
      thinkingConfig: { includeThoughts: true, thinkingBudget: 12288 },
    });
    expect(result?.variants.max).toEqual({
      thinkingConfig: { includeThoughts: true, thinkingBudget: 24576 },
    });
  });

  it("toggle-only 元数据不猜测参数，并在应用时保留已有 variants", () => {
    const existing: OpenCodeModel = {
      name: "Gemma",
      variants: { thinking: { thinkingConfig: { includeThoughts: true } } },
    };
    expect(
      buildVariantsForModel(
        GEMMA_TOGGLE_ONLY,
        "@ai-sdk/google",
        "gemma-4-26b-a4b-it",
      ),
    ).toBeNull();
    const result = applyModelsDevCapabilities(
      existing,
      "@ai-sdk/google",
      "gemma-4-26b-a4b-it",
      GEMMA_TOGGLE_ONLY,
    );
    expect(result.model.variants).toEqual(existing.variants);
  });

  it("显式空 reasoning_options 屏蔽 OpenCode 自动生成的所有档位", () => {
    const result = buildVariantsForModel(
      EMPTY_REASONING_OPTIONS,
      "@ai-sdk/google",
      "gemini-3.7-flash",
    );
    expect(result?.efforts).toEqual([]);
    expect(result?.variants).toEqual({
      minimal: { disabled: true },
      low: { disabled: true },
      medium: { disabled: true },
      high: { disabled: true },
    });
    expect(result?.suppressed).toEqual(["minimal", "low", "medium", "high"]);
  });

  it("未知 Anthropic effort 模型使用 SDK 的 effort fallback，而非 OpenAI 字段", () => {
    const result = buildVariantsForModel(
      UNKNOWN_ANTHROPIC_EFFORT_MODEL,
      "@ai-sdk/anthropic",
      "gateway-reasoner",
    );
    expect(result?.variants.low).toEqual({ effort: "low" });
    expect(result?.variants.low).not.toHaveProperty("reasoningEffort");
  });

  it("numbered 风格按声明顺序编号，并屏蔽注入的 plain 档位", () => {
    const result = buildVariantsForModel(
      GROK_46,
      "@ai-sdk/openai",
      "grok-4.6",
      undefined,
      { style: "numbered" },
    );
    expect(result?.efforts).toEqual([
      "01-low",
      "02-medium",
      "03-high",
      "04-xhigh",
    ]);
    // 注入的 plain 档（none 以及 low/medium/high/xhigh 的 plain 名）全部屏蔽
    expect(result?.suppressed).toEqual([
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    expect(Object.keys(result?.variants ?? {})).toEqual([
      "01-low",
      "02-medium",
      "03-high",
      "04-xhigh",
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    expect(result?.variants["01-low"]).toEqual({
      reasoningEffort: "low",
      reasoningSummary: "auto",
      include: ["reasoning.encrypted_content"],
    });
    expect(result?.variants.low).toEqual({ disabled: true });
  });

  it("numbered 风格 openai-compatible 生成带序号键并屏蔽注入档位", () => {
    const result = buildVariantsForModel(
      GROK_46,
      "@ai-sdk/openai-compatible",
      "grok-4.6",
      undefined,
      { style: "numbered" },
    );
    expect(result?.efforts).toEqual([
      "01-low",
      "02-medium",
      "03-high",
      "04-xhigh",
    ]);
    expect(result?.variants["01-low"]).toEqual({ reasoningEffort: "low" });
    expect(result?.variants.low).toEqual({ disabled: true });
    expect(result?.variants.medium).toEqual({ disabled: true });
  });

  it("numbered 风格 Anthropic budget_tokens 生成 01-high/02-max", () => {
    const result = buildVariantsForModel(
      CLAUDE_SONNET_45,
      "@ai-sdk/anthropic",
      "claude-sonnet-4-5-20250929",
      64000,
      { style: "numbered" },
    );
    expect(result?.efforts).toEqual(["01-high", "02-max"]);
    expect(result?.variants["01-high"]).toEqual({
      thinking: { type: "enabled", budgetTokens: 16000 },
    });
    expect(result?.variants["02-max"]).toEqual({
      thinking: { type: "enabled", budgetTokens: 31999 },
    });
  });

  it("numbered 风格重复填充幂等，不叠加序号前缀", () => {
    const first = applyModelsDevCapabilities(
      { name: "GPT-5.6 Sol" },
      "@ai-sdk/openai",
      "gpt-5.6-sol",
      GPT_56_SOL,
      { style: "numbered" },
    );
    // 6 个带序号档位 + 5 个被屏蔽的注入 plain 档
    expect(Object.keys(first.model.variants as object)).toEqual([
      "01-none",
      "02-low",
      "03-medium",
      "04-high",
      "05-xhigh",
      "06-max",
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    const second = applyModelsDevCapabilities(
      first.model,
      "@ai-sdk/openai",
      "gpt-5.6-sol",
      GPT_56_SOL,
      { style: "numbered" },
    );
    expect(second.model.variants).toEqual(first.model.variants);
  });

  it("nextNumberedKey 在现有最大序号后追加，无前缀键从 01 起", () => {
    expect(
      nextNumberedKey(
        { "01-low": {}, "02-high": {} },
        "max",
      ),
    ).toBe("03-max");
    expect(nextNumberedKey({ low: {}, high: {} }, "medium")).toBe("01-medium");
    expect(nextNumberedKey(undefined, "low")).toBe("01-low");
  });

  it("buildVariantForEffort 按 SDK 生成单个档位参数体", () => {
    expect(
      buildVariantForEffort("@ai-sdk/openai-compatible", "m", "high"),
    ).toEqual({ reasoningEffort: "high" });
    expect(
      buildVariantForEffort("@ai-sdk/anthropic", "claude-opus-4-6", "high", 128000),
    ).toEqual({ thinking: { type: "adaptive" }, effort: "high" });
    expect(buildVariantForEffort("unknown-sdk", "m", "high")).toBeUndefined();
  });
});

describe("findModelsDevEntry", () => {
  const catalog: ModelsDevResponse = {
    xai: { models: { "grok-4.6": { name: "Grok via xai" } } },
    openai: { models: { "grok-4.6": { name: "Grok via openai" } } },
    openrouter: { models: { "GROK-4.6": { name: "Grok via openrouter" } } },
  };

  it("按官方源优先级命中", () => {
    const hit = findModelsDevEntry(catalog, "grok-4.6");
    expect(hit?.providerId).toBe("openai");
  });

  it("forcedSource 覆盖优先级", () => {
    const hit = findModelsDevEntry(catalog, "grok-4.6", "xai");
    expect(hit?.providerId).toBe("xai");
  });

  it("官方源未命中时大小写不敏感全表兜底", () => {
    const hit = findModelsDevEntry(catalog, "Grok-4.6-X");
    expect(hit).toBeNull();
    const loose = findModelsDevEntry(
      { openrouter: { models: { "GROK-4.6": { name: "x" } } } },
      "grok-4.6",
    );
    expect(loose?.providerId).toBe("openrouter");
  });
});

describe("applyModelsDevCapabilities", () => {
  const existing: OpenCodeModel = {
    name: "我的显示名",
    limit: { context: 1, output: 2 },
    options: { provider: { order: ["baseten"] } },
    headers: { "X-Custom": "keep" },
    customKey: "keep-me",
    variants: { "01-low": { reasoningEffort: "low" } },
  };

  it("能力字段覆盖；已手填的 limit.context/output 保留；显示名/SDK 选项/人工键保留；旧 variants 被刷新", () => {
    const { model, summary } = applyModelsDevCapabilities(
      existing,
      "@ai-sdk/openai",
      "grok-4.6",
      GROK_46,
    );
    expect(model.name).toBe("我的显示名");
    // 中转站要求特定大小时用户手填的 context/output 不被覆盖
    expect(model.limit).toEqual({ context: 1, output: 2 });
    expect(model.options).toEqual({ provider: { order: ["baseten"] } });
    expect(model.headers).toEqual({ "X-Custom": "keep" });
    expect(model.customKey).toBe("keep-me");
    expect(model.family).toBe("grok");
    expect(model.variants).toHaveProperty("low");
    expect(model.variants).not.toHaveProperty("01-low");
    expect(summary.contextLimit).toBe(1);
    expect(summary.suppressed).toEqual(["none"]);
  });

  it("limit 仅非空字段保留，空缺字段补 models.dev 值", () => {
    const { model } = applyModelsDevCapabilities(
      { name: "", limit: { context: 123456 } },
      "@ai-sdk/openai",
      "gpt-5.6-sol",
      GPT_56_SOL,
    );
    // 用户只填了 context → 保留；output 空缺 → 补官方值
    expect(model.limit).toEqual({
      context: 123456,
      input: 922000,
      output: 128000,
    });
  });

  it("导入或原始 JSON 中手填的 limit.input 也不被 models.dev 覆盖", () => {
    const { model } = applyModelsDevCapabilities(
      { name: "", limit: { input: 654321 } },
      "@ai-sdk/openai",
      "gpt-5.6-sol",
      GPT_56_SOL,
    );
    expect(model.limit).toEqual({
      context: 1050000,
      input: 654321,
      output: 128000,
    });
  });

  it("models.dev 条目无 limit 时不清除用户已填的 limit", () => {
    const { model, summary } = applyModelsDevCapabilities(
      { name: "", limit: { context: 999, output: 888 } },
      "@ai-sdk/openai",
      "m",
      { name: "M" },
    );
    expect(model.limit).toEqual({ context: 999, output: 888 });
    expect(summary.contextLimit).toBe(999);
  });

  it("用户未填 limit 时整体采用 models.dev 值", () => {
    const { model } = applyModelsDevCapabilities(
      { name: "" },
      "@ai-sdk/openai",
      "grok-4.6",
      GROK_46,
    );
    expect(model.limit).toEqual({ context: 500000, output: 500000 });
  });

  it("显示名为空时填官方名", () => {
    const { model } = applyModelsDevCapabilities(
      { name: "" },
      "@ai-sdk/openai",
      "grok-4.6",
      GROK_46,
    );
    expect(model.name).toBe("Grok 4.6");
  });

  it("无 reasoning_options 时保留旧 variants", () => {
    const { model } = applyModelsDevCapabilities(
      existing,
      "@ai-sdk/openai",
      "m",
      { name: "M" },
    );
    expect(model.variants).toEqual(existing.variants);
  });
});
