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
    expect(isGranularitySelectable(start, end, "1min", end)).toBe(false);
    expect(isGranularitySelectable(start, end, "15min", end)).toBe(true);
    expect(isGranularitySelectable(start, end, "auto", end)).toBe(true);
  });

  it("disables sub-day granularity when the range starts past the detail retention window", () => {
    const now = Math.floor(Date.parse("2026-09-01T00:00:00Z") / 1000);
    const start = now - 35 * 86_400;
    const end = now - 86_400;
    expect(isGranularitySelectable(start, end, "hour", now)).toBe(false);
    expect(isGranularitySelectable(start, end, "15min", now)).toBe(false);
    // 范围整体在保留期内：子天档可选
    expect(isGranularitySelectable(now - 2 * 86_400, now, "hour", now)).toBe(
      true,
    );
    // 天及以上档位与 auto 不受保留期限制
    expect(isGranularitySelectable(start, end, "day", now)).toBe(true);
    expect(isGranularitySelectable(start, end, "auto", now)).toBe(true);
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

    const { rows, seriesKeys, columns } = buildTrendSeriesChart(response, {
      groupBy: "token_type",
      metric: "tokens",
      dateLocale: "en-US",
    });
    const col = (key: string) => columns[seriesKeys.indexOf(key)];

    expect(seriesKeys).toEqual([
      "input",
      "output",
      "cacheCreation",
      "cacheRead",
    ]);
    expect(rows[0][col("input")]).toBe(100);
    expect(rows[0][col("output")]).toBe(50);
    expect(rows[1][col("input")]).toBe(0);
    expect(rows[1][col("cacheRead")]).toBe(0);
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

    const { rows, seriesKeys, columns } = buildTrendSeriesChart(response, {
      groupBy: "model",
      metric: "tokens",
      dateLocale: "en-US",
      topN: 3,
    });
    const col = (key: string) => columns[seriesKeys.indexOf(key)];

    expect(seriesKeys).toEqual(["a", "b", "c", OTHER_SERIES_KEY]);
    expect(rows[0][col(OTHER_SERIES_KEY)]).toBe(3);
  });

  it("isolates series values from row metadata via positional columns", () => {
    // 回归：模型/Provider 名恰为 xKey/label/tooltipLabel 或 __other__ 时，
    // 系列值不得覆盖行元数据，也不得与合成“其他”key 产生同名双系列
    const response: UsageTrendSeriesResponse = {
      granularity: "day",
      buckets: [
        {
          bucketStart: "2026-08-10T00:00:00+08:00",
          series: [
            point("xKey", 11),
            point("label", 22),
            point("tooltipLabel", 33),
            point(OTHER_SERIES_KEY, 44),
          ],
        },
      ],
    };

    const { rows, seriesKeys, columns } = buildTrendSeriesChart(response, {
      groupBy: "model",
      metric: "tokens",
      dateLocale: "en-US",
    });
    const col = (key: string) => columns[seriesKeys.indexOf(key)];

    expect(rows[0].xKey).toBe("2026-08-10T00:00:00+08:00");
    expect(rows[0].label).toBeTruthy();
    expect(rows[0].tooltipLabel).toBeTruthy();
    expect(rows[0][col("xKey")]).toBe(11);
    expect(rows[0][col("label")]).toBe(22);
    expect(rows[0][col("tooltipLabel")]).toBe(33);
    expect(seriesKeys.filter((k) => k === OTHER_SERIES_KEY)).toHaveLength(1);
    expect(rows[0][col(OTHER_SERIES_KEY)]).toBe(44);
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

    const { rows, seriesKeys, columns } = buildTrendSeriesChart(response, {
      groupBy: "provider",
      metric: "cost",
      dateLocale: "en-US",
    });

    expect(rows[0][columns[seriesKeys.indexOf("packy")]]).toBe(1.25);
  });

  it("returns empty model for empty response", () => {
    const { rows, seriesKeys, columns } = buildTrendSeriesChart(undefined, {
      groupBy: "model",
      metric: "tokens",
      dateLocale: "en-US",
    });
    expect(rows).toEqual([]);
    expect(seriesKeys).toEqual([]);
    expect(columns).toEqual([]);
  });
});

describe("formatTrendBucketLabel", () => {
  it("formats minute buckets with time, month buckets with year", () => {
    // 只断言形状/年份，避免测试环境时区影响
    expect(
      formatTrendBucketLabel(
        "2026-08-10T09:05:00+08:00",
        "15min",
        "en-US",
        false,
      ),
    ).toMatch(/\d{2}\/\d{2}, \d{2}:\d{2}/);
    expect(
      formatTrendBucketLabel(
        "2026-08-01T00:00:00+08:00",
        "month",
        "en-US",
        false,
      ),
    ).toContain("2026");
    expect(
      formatTrendBucketLabel("2026-08-10T00:00:00+08:00", "day", "en-US", true),
    ).toContain("26");
    expect(
      formatTrendBucketLabel(
        "2026-08-10T00:00:00+08:00",
        "day",
        "en-US",
        false,
      ),
    ).not.toContain("2026");
  });
});
