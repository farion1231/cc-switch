import { describe, expect, it } from "vitest";
import { buildUsageTrendChartData } from "@/components/usage/UsageTrendChart";

describe("Hermes dimensions in the v4 single-metric chart", () => {
  it("includes cache-write and reasoning tokens without changing the upstream chart layout", () => {
    const base = {
      date: "2026-10-08T00:00:00Z",
      totalInputTokens: 10,
      totalOutputTokens: 5,
      totalCacheCreationTokens: 4,
      totalCacheReadTokens: 3,
      totalCost: "0.1",
    };
    const options = {
      isHourly: false,
      dateLocale: "en-US",
      startDate: 0,
      endDate: 2000000000,
    };
    const points = buildUsageTrendChartData(
      [{ ...base, totalCacheWriteTokens: 6, totalReasoningTokens: 2 }],
      options,
    );
    expect(points[0]).toMatchObject({
      tokens: 30,
      cacheWriteTokens: 6,
      reasoningTokens: 2,
    });
    const legacy = buildUsageTrendChartData([base], options)[0];
    expect(legacy.tokens).toBe(22);
    expect(legacy).not.toHaveProperty("cacheWriteTokens");
    expect(legacy).not.toHaveProperty("reasoningTokens");
  });
});
