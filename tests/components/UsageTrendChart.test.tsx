import { describe, expect, it } from "vitest";
import {
  buildUsageTrendChartData,
  hitRateOf,
} from "@/components/usage/UsageTrendChart";

describe("Hermes dimensions in the v4 single-metric chart", () => {
  it.each([
    [100, 0, 100, 100, 1 / 3],
    [100, 50, 100, 50, 1 / 3],
    [0, 0, 0, 100, 0],
  ])(
    "includes Hermes cache writes in shared trend/heatmap hit rate (%s/%s/%s/%s)",
    (input, creation, read, write, expected) => {
      const stat = {
        date: "2026-10-23T00:00:00Z",
        totalInputTokens: input,
        totalOutputTokens: 0,
        totalCacheCreationTokens: creation,
        totalCacheReadTokens: read,
        totalCacheWriteTokens: write,
        totalReasoningTokens: 900,
        totalCost: "0",
      };
      expect(hitRateOf(stat)).toBe(expected);
      const point = buildUsageTrendChartData([stat], {
        isHourly: false,
        dateLocale: "en-US",
        startDate: 0,
        endDate: 2000000000,
      })[0];
      expect(point.hitRate).toBe(expected);
    },
  );
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
