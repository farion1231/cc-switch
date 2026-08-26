import { describe, expect, it } from "vitest";
import {
  buildTrendSeriesChart,
  estimateTrendBucketCount,
  formatTrendBucketLabel,
  isGranularitySelectable,
  OTHER_SERIES_KEY,
  seriesColor,
  seriesColorIndex,
} from "@/components/usage/UsageStackedBarChart";
import type { UsageTrendSeriesResponse } from "@/types/usage";

const point = (key: string, tokens: number, cost = "0") => ({
  key,
  inputTokens: tokens,
  outputTokens: 0,
  cacheCreationTokens: 0,
  cacheReadTokens: 0,
  cost,
});

describe("granularity budget", () => {
  it("disables granularity options whose estimated bucket count exceeds the limit", () => {
    const start = Math.floor(Date.parse("2026-01-01T00:00:00Z") / 1000);
    const end = start + 2 * 86_400;
    expect(estimateTrendBucketCount(start, end, "1min")).toBe(2881);
    expect(isGranularitySelectable(start, end, "1min")).toBe(false);
    expect(isGranularitySelectable(start, end, "15min")).toBe(true);
    expect(isGranularitySelectable(start, end, "auto")).toBe(true);
  });
});

describe("seriesColor", () => {
  it("maps token types to the fixed palette and hashes names stably", () => {
    expect(seriesColor("input", "token_type")).toBe("#3b82f6");
    expect(seriesColor("cacheRead", "token_type")).toBe("#a855f7");
    expect(seriesColorIndex("claude-sonnet-4-6")).toBe(
      seriesColorIndex("claude-sonnet-4-6"),
    );
  });
});

describe("buildTrendSeriesChart", () => {
  it("keeps the four fixed token-type series and zero-fills missing buckets", () => {
    const response: UsageTrendSeriesResponse = {
      granularity: "hour",
      buckets: [
        {
          bucketStart: "2026-08-10T09:00:00+08:00",
          series: [point("input", 100), point("output", 50)],
        },
        { bucketStart: "2026-08-10T10:00:00+08:00", series: [] },
      ],
    };

    const { rows, seriesKeys } = buildTrendSeriesChart(response, {
      groupBy: "token_type",
      metric: "tokens",
      dateLocale: "en-US",
    });

    expect(seriesKeys).toEqual(["input", "output", "cacheCreation", "cacheRead"]);
    expect(rows[0].input).toBe(100);
    expect(rows[0].output).toBe(50);
    expect(rows[1].input).toBe(0);
    expect(rows[1].cacheRead).toBe(0);
  });

  it("aggregates series beyond topN into __other__ ranked by metric total", () => {
    const response: UsageTrendSeriesResponse = {
      granularity: "day",
      buckets: [
        {
          bucketStart: "2026-08-10T00:00:00+08:00",
          series: [point("a", 10), point("b", 9), point("c", 8), point("d", 3)],
        },
      ],
    };

    const { rows, seriesKeys } = buildTrendSeriesChart(response, {
      groupBy: "model",
      metric: "tokens",
      dateLocale: "en-US",
      topN: 3,
    });

    expect(seriesKeys).toEqual(["a", "b", "c", OTHER_SERIES_KEY]);
    expect(rows[0][OTHER_SERIES_KEY]).toBe(3);
  });

  it("derives cost values from the cost string per series", () => {
    const response: UsageTrendSeriesResponse = {
      granularity: "day",
      buckets: [
        {
          bucketStart: "2026-08-10T00:00:00+08:00",
          series: [
            {
              key: "packy",
              inputTokens: 1000,
              outputTokens: 0,
              cacheCreationTokens: 0,
              cacheReadTokens: 0,
              cost: "1.25",
            },
          ],
        },
      ],
    };

    const { rows } = buildTrendSeriesChart(response, {
      groupBy: "provider",
      metric: "cost",
      dateLocale: "en-US",
    });

    expect(rows[0].packy).toBe(1.25);
  });

  it("returns empty model for empty response", () => {
    const { rows, seriesKeys } = buildTrendSeriesChart(undefined, {
      groupBy: "model",
      metric: "tokens",
      dateLocale: "en-US",
    });
    expect(rows).toEqual([]);
    expect(seriesKeys).toEqual([]);
  });
});

describe("formatTrendBucketLabel", () => {
  it("formats minute buckets with time, month buckets with year", () => {
    // 只断言形状/年份，避免测试环境时区影响
    expect(
      formatTrendBucketLabel("2026-08-10T09:05:00+08:00", "15min", "en-US", false),
    ).toMatch(/\d{2}\/\d{2}, \d{2}:\d{2}/);
    expect(formatTrendBucketLabel("2026-08-01T00:00:00+08:00", "month", "en-US", false)).toContain(
      "2026",
    );
    expect(formatTrendBucketLabel("2026-08-10T00:00:00+08:00", "day", "en-US", true)).toContain(
      "26",
    );
    expect(
      formatTrendBucketLabel("2026-08-10T00:00:00+08:00", "day", "en-US", false),
    ).not.toContain("2026");
  });
});
