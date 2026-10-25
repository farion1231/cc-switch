import { describe, expect, it } from "vitest";
import { isUnpricedUsage } from "@/types/usage";
import { usageCostLabel } from "@/components/usage/format";
import { aggregateSummaries } from "@/components/usage/UsageHero";
import { buildUsageTrendChartData } from "@/components/usage/UsageTrendChart";

const usage = {
  inputTokens: 100,
  outputTokens: 50,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  totalCostUsd: "0",
  statusCode: 200,
};

describe("explicit cost knowledge", () => {
  it("does not confuse a free model with missing pricing", () => {
    expect(isUnpricedUsage({ ...usage, costKnown: true })).toBe(false);
    expect(isUnpricedUsage({ ...usage, costKnown: false })).toBe(true);
    expect(isUnpricedUsage(usage)).toBe(true); // old API compatibility
  });
  it("shows unknown costs and known subtotals without changing free amounts", () => {
    expect(usageCostLabel("0", true, "$0.00", "Unknown")).toBe("Unknown");
    expect(usageCostLabel("1.25", true, "$1.25", "Unknown")).toBe("≥ $1.25");
    expect(usageCostLabel("0", false, "$0.00", "Unknown")).toBe("$0.00");
  });
  it("retains uncertainty and tokens when combining apps and chart buckets", () => {
    const empty = aggregateSummaries([]);
    const result = aggregateSummaries([
      empty,
      { ...empty, totalCost: "2", totalInputTokens: 100, costIncomplete: true },
    ]);
    expect(result.costIncomplete).toBe(true);
    expect(result.totalCost).toBe("2.000000");
    expect(result.realTotalTokens).toBe(100);
    const points = buildUsageTrendChartData(
      [{ ...result, date: "2026-10-23T00:00:00Z" }],
      {
        isHourly: false,
        dateLocale: "en-US",
        startDate: 0,
        endDate: 1792800000,
      },
    );
    expect(points[0].costIncomplete).toBe(true);
    expect(points[0].cost).toBe(2);
  });
});
