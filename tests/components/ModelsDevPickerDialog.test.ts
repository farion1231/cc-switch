import { describe, expect, it } from "vitest";

import {
  flattenModels,
  formatPrice,
  normalizeModelIdForPricing,
} from "@/components/usage/ModelsDevPickerDialog";
import {
  getCommonModelKeys,
  resolveModelsDevSelection,
  toModelPricing,
} from "@/lib/modelsDevPricing";

describe("normalizeModelIdForPricing", () => {
  it("keeps already-normalized ids unchanged", () => {
    expect(normalizeModelIdForPricing("claude-opus-4-5")).toBe(
      "claude-opus-4-5",
    );
  });

  it("strips the vendor prefix before the last slash", () => {
    expect(normalizeModelIdForPricing("z-ai/glm-4.7")).toBe("glm-4.7");
    expect(normalizeModelIdForPricing("clarifai/main/models/mm-poly-8b")).toBe(
      "mm-poly-8b",
    );
  });

  it("lowercases the id", () => {
    expect(normalizeModelIdForPricing("MiniMaxAI/MiniMax-M2.1")).toBe(
      "minimax-m2.1",
    );
  });

  it("truncates colon suffixes", () => {
    expect(normalizeModelIdForPricing("claude-sonnet-4-thinking:8192")).toBe(
      "claude-sonnet-4-thinking",
    );
  });

  it("maps @ to -", () => {
    expect(normalizeModelIdForPricing("claude-sonnet-4@20250514")).toBe(
      "claude-sonnet-4-20250514",
    );
  });

  it("strips the [1m] context marker", () => {
    expect(normalizeModelIdForPricing("claude-sonnet-4-5[1m]")).toBe(
      "claude-sonnet-4-5",
    );
  });

  it("combines all rules", () => {
    expect(normalizeModelIdForPricing("Vendor/Claude-Sonnet-4@2025:free")).toBe(
      "claude-sonnet-4-2025",
    );
  });
});

describe("formatPrice", () => {
  it("formats integers without a decimal point", () => {
    expect(formatPrice(5)).toBe("5");
    expect(formatPrice(25)).toBe("25");
  });

  it("trims trailing zeros", () => {
    expect(formatPrice(0.5)).toBe("0.5");
    expect(formatPrice(6.25)).toBe("6.25");
    expect(formatPrice(1.0395)).toBe("1.0395");
  });

  it("keeps up to six decimal places", () => {
    expect(formatPrice(0.000001)).toBe("0.000001");
    expect(formatPrice(0.0000004)).toBe("0");
  });

  it("returns 0 for zero, negative and non-finite values", () => {
    expect(formatPrice(0)).toBe("0");
    expect(formatPrice(-1)).toBe("0");
    expect(formatPrice(NaN)).toBe("0");
    expect(formatPrice(Infinity)).toBe("0");
  });

  it("never produces exponent notation", () => {
    // 后端 Decimal::from_str 不接受科学计数法
    expect(formatPrice(1e-8)).toBe("0");
    expect(formatPrice(1e21)).toBe("0");
    for (const value of [5, 0.5, 0.000123, 123456.789]) {
      expect(formatPrice(value)).toMatch(/^\d+(\.\d+)?$/);
    }
  });
});

describe("flattenModels", () => {
  it("flattens providers, fills defaults and sorts by release date desc", () => {
    const entries = flattenModels({
      acme: {
        id: "acme",
        name: "Acme AI",
        models: {
          "old-model": {
            id: "old-model",
            name: "Old Model",
            release_date: "2024-01-01",
            cost: { input: 1, output: 2 },
          },
          "new-model": {
            id: "new-model",
            name: "New Model",
            release_date: "2025-06-01",
            cost: { input: 3, output: 6, cache_read: 0.3, cache_write: 3.75 },
          },
          "free-model": {
            id: "free-model",
            name: "No Cost Model",
          },
        },
      },
      bare: {
        models: {
          "Vendor/Some-Model:free": {
            release_date: "2025-01",
            cost: { input: 0.1 },
          },
        },
      },
    });

    expect(entries.map((e) => e.key)).toEqual([
      "acme/new-model",
      "bare/Vendor/Some-Model:free",
      "acme/old-model",
    ]);

    const newModel = entries[0];
    expect(newModel.normalizedId).toBe("new-model");
    expect(newModel.cacheRead).toBe(0.3);
    expect(newModel.cacheWrite).toBe(3.75);

    // 没有 name 的 provider 用 id 兜底；缺失的成本字段补 0
    const bareModel = entries[1];
    expect(bareModel.providerName).toBe("bare");
    expect(bareModel.normalizedId).toBe("some-model");
    expect(bareModel.output).toBe(0);
    expect(bareModel.cacheRead).toBe(0);

    // 完全没有定价的模型被过滤
    expect(entries.some((e) => e.modelId === "free-model")).toBe(false);
  });

  it("selects a bounded canonical set of common model families", () => {
    const openAiModels = Object.fromEntries(
      Array.from({ length: 7 }, (_, index) => {
        const version = index + 1;
        return [
          `gpt-${version}`,
          {
            name: `GPT ${version}`,
            release_date: `2025-0${version}-01`,
            cost: { input: version, output: version * 2 },
          },
        ];
      }),
    );
    const entries = flattenModels({
      openai: {
        name: "OpenAI",
        models: {
          ...openAiModels,
          "gpt-image-1": {
            release_date: "2026-01-01",
            cost: { input: 1, output: 2 },
          },
        },
      },
      aggregator: {
        name: "Aggregator",
        models: {
          "gpt-7": {
            release_date: "2026-02-01",
            cost: { input: 9, output: 18 },
          },
        },
      },
      anthropic: {
        name: "Anthropic",
        models: {
          "claude-sonnet-5": {
            release_date: "2026-06-01",
            cost: { input: 3, output: 15 },
          },
        },
      },
      deepseek: {
        name: "DeepSeek",
        models: {
          "deepseek-chat": {
            release_date: "2025-12-01",
            cost: { input: 0.3, output: 1.2 },
          },
        },
      },
      xiaomi: {
        name: "Xiaomi",
        models: {
          "mimo-v2.5": {
            release_date: "2026-04-01",
            cost: { input: 0.2, output: 1 },
          },
          "mimo-v2.5-tts": {
            release_date: "2026-05-01",
            cost: { input: 0.1, output: 0.5 },
          },
        },
      },
      longcat: {
        name: "LongCat",
        models: {
          "LongCat-2.0": {
            release_date: "2026-03-01",
            cost: { input: 0.4, output: 1.6 },
          },
        },
      },
    });

    const common = getCommonModelKeys(entries);
    expect(common.has("openai/gpt-image-1")).toBe(false);
    expect(common.has("aggregator/gpt-7")).toBe(false);
    expect(common.has("openai/gpt-7")).toBe(true);
    expect(common.has("openai/gpt-1")).toBe(false);
    expect(common.has("anthropic/claude-sonnet-5")).toBe(true);
    expect(common.has("deepseek/deepseek-chat")).toBe(true);
    expect(common.has("xiaomi/mimo-v2.5")).toBe(true);
    expect(common.has("xiaomi/mimo-v2.5-tts")).toBe(false);
    expect(common.has("longcat/LongCat-2.0")).toBe(true);
  });

  it("combines common and explicit selections and deduplicates normalized ids", () => {
    const entries = flattenModels({
      openai: {
        models: {
          "gpt-5": {
            name: "GPT-5 Official",
            release_date: "2025-08-01",
            cost: { input: 1, output: 2 },
          },
        },
      },
      relay: {
        models: {
          "vendor/GPT-5": {
            name: "GPT-5 Relay",
            release_date: "2025-07-01",
            cost: { input: 9, output: 18 },
          },
          "custom-model": {
            name: "Custom",
            release_date: "2025-06-01",
            cost: { input: 0.5, output: 1 },
          },
        },
      },
    });
    const selected = resolveModelsDevSelection(entries, {
      autoSyncEnabled: true,
      includeCommonModels: true,
      selectedModelKeys: ["relay/vendor/GPT-5", "relay/custom-model"],
      excludedCommonModelKeys: ["openai/gpt-5"],
      lastSyncAt: null,
      lastSyncError: null,
    });

    expect(selected.map((entry) => entry.key)).toEqual([
      "relay/vendor/GPT-5",
      "relay/custom-model",
    ]);

    const pricing = toModelPricing([
      entries.find((entry) => entry.key === "openai/gpt-5")!,
      entries.find((entry) => entry.key === "relay/vendor/GPT-5")!,
    ]);
    expect(pricing).toHaveLength(1);
    expect(pricing[0]).toMatchObject({
      modelId: "gpt-5",
      displayName: "GPT-5 Official",
      inputCostPerMillion: "1",
    });
  });
});
