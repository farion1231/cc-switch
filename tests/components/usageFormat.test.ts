import { describe, expect, it } from "vitest";
import {
  computeFloatingUsageSummary,
  formatTokensShort,
  getLocaleFromLanguage,
} from "@/components/usage/format";

describe("usage format helpers", () => {
  it("formats Traditional Chinese token units with Traditional characters", () => {
    expect(formatTokensShort(12_345, "zh-TW")).toBe("1.2 萬");
    expect(formatTokensShort(123_456_789, "zh-Hant", 2)).toBe("1.23 億");
  });

  it("resolves Traditional Chinese locale aliases", () => {
    expect(getLocaleFromLanguage("zh_TW")).toBe("zh-TW");
    expect(getLocaleFromLanguage("zh-HK")).toBe("zh-TW");
  });
});

describe("computeFloatingUsageSummary", () => {
  it("sums cache tokens into realTotalTokens and computes hit rate", () => {
    const summary = computeFloatingUsageSummary([
      {
        appType: "claude",
        summary: {
          totalRequests: 2,
          totalCost: "0.30",
          totalInputTokens: 100,
          totalOutputTokens: 50,
          totalCacheCreationTokens: 20,
          totalCacheReadTokens: 30,
          successRate: 100,
          realTotalTokens: 200,
          cacheHitRate: 0.2,
        },
      },
    ]);

    expect(summary.realTotalTokens).toBe(200); // 100 + 50 + 20 + 30
    expect(summary.inputTokens).toBe(100);
    expect(summary.outputTokens).toBe(50);
    expect(summary.cacheCreationTokens).toBe(20);
    expect(summary.cacheReadTokens).toBe(30);
    // 30 / (100 + 20 + 30) = 20%
    expect(summary.cacheHitRate).toBeCloseTo(20, 5);
    expect(summary.totalCost).toBeCloseTo(0.3, 5);
  });

  it("aggregates across multiple app summaries", () => {
    const summary = computeFloatingUsageSummary([
      {
        appType: "claude",
        summary: {
          totalRequests: 1,
          totalCost: "0.10",
          totalInputTokens: 100,
          totalOutputTokens: 50,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 0,
          successRate: 100,
          realTotalTokens: 150,
          cacheHitRate: 0,
        },
      },
      {
        appType: "codex",
        summary: {
          totalRequests: 1,
          totalCost: "0.20",
          totalInputTokens: 200,
          totalOutputTokens: 30,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 70,
          successRate: 100,
          realTotalTokens: 300,
          cacheHitRate: 0.7,
        },
      },
    ]);

    expect(summary.realTotalTokens).toBe(450); // (100+50) + (200+30+70)
    expect(summary.totalCost).toBeCloseTo(0.3, 5);
    // input 跨应用累计 = 300，cacheableInput = 300 + 70；70 / 370 = 18.9...
    expect(summary.cacheHitRate).toBeCloseTo(18.9189, 3);
  });

  it("returns zeros for empty data", () => {
    expect(computeFloatingUsageSummary([])).toEqual({
      totalCost: 0,
      realTotalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      cacheHitRate: 0,
    });
    expect(computeFloatingUsageSummary(undefined)).toEqual({
      totalCost: 0,
      realTotalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      cacheHitRate: 0,
    });
  });
});
