import { describe, expect, it } from "vitest";
import {
  buildUsageTrendChartData,
  formatUsageTrendTickLabel,
} from "@/components/usage/UsageTrendChart";

const day = (isoDate: string) =>
  ({
    date: `${isoDate}T12:00:00.000Z`,
    totalInputTokens: 100,
    totalOutputTokens: 50,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
    totalCost: "0.01",
  }) as const;

describe("buildUsageTrendChartData (#6302)", () => {
  it("keeps unique x-axis keys when the same MM/DD appears in multiple years", () => {
    // 2025-04-27 and 2026-04-27 share the same MM/DD tick text in single-year
    // formatting. Using that text as the Recharts category key made activeDots
    // jump to the earlier year's point while the tooltip followed the cursor.
    const startDate = Math.floor(Date.parse("2025-01-01T00:00:00Z") / 1000);
    const endDate = Math.floor(Date.parse("2026-08-10T00:00:00Z") / 1000);

    const points = buildUsageTrendChartData(
      [day("2025-04-27"), day("2026-04-27")],
      {
        isHourly: false,
        dateLocale: "en-US",
        startDate,
        endDate,
      },
    );

    expect(points).toHaveLength(2);
    expect(points[0].xKey).not.toBe(points[1].xKey);
    expect(points[0].xKey).toContain("2025-04-27");
    expect(points[1].xKey).toContain("2026-04-27");
    // Tooltip always carries a year so the user can tell which April it is.
    expect(points[0].tooltipLabel).toMatch(/2025/);
    expect(points[1].tooltipLabel).toMatch(/2026/);
  });

  it("includes a year in the axis tick when the selected range spans years", () => {
    const startDate = Math.floor(Date.parse("2025-01-01T00:00:00Z") / 1000);
    const endDate = Math.floor(Date.parse("2026-08-10T00:00:00Z") / 1000);

    const points = buildUsageTrendChartData([day("2026-04-27")], {
      isHourly: false,
      dateLocale: "en-US",
      startDate,
      endDate,
    });

    expect(points[0].label).toMatch(/26|2026/);
  });

  it("keeps short MM/DD ticks for single-year ranges", () => {
    const startDate = Math.floor(Date.parse("2026-01-01T00:00:00Z") / 1000);
    const endDate = Math.floor(Date.parse("2026-08-10T00:00:00Z") / 1000);

    const points = buildUsageTrendChartData([day("2026-04-27")], {
      isHourly: false,
      dateLocale: "en-US",
      startDate,
      endDate,
    });

    // en-US 2-digit month/day — should not need a year prefix inside one year.
    expect(points[0].label).not.toMatch(/2026/);
    expect(points[0].tooltipLabel).toMatch(/2026/);
  });
});

describe("formatUsageTrendTickLabel", () => {
  it("resolves labels by xKey even when the tick index is thinned", () => {
    const startDate = Math.floor(Date.parse("2025-01-01T00:00:00Z") / 1000);
    const endDate = Math.floor(Date.parse("2026-08-10T00:00:00Z") / 1000);
    const points = buildUsageTrendChartData(
      [
        {
          date: "2025-01-01T12:00:00.000Z",
          totalInputTokens: 1,
          totalOutputTokens: 1,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 0,
          totalCost: "0",
        },
        {
          date: "2025-04-27T12:00:00.000Z",
          totalInputTokens: 1,
          totalOutputTokens: 1,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 0,
          totalCost: "0",
        },
        {
          date: "2026-04-27T12:00:00.000Z",
          totalInputTokens: 1,
          totalOutputTokens: 1,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 0,
          totalCost: "0",
        },
      ],
      { isHourly: false, dateLocale: "en-US", startDate, endDate },
    );

    // Simulate Recharts passing a later category as the only visible tick
    // (filtered index 0 would wrongly map to the first chart row).
    const last = points[2];
    expect(formatUsageTrendTickLabel(last.xKey, points)).toBe(last.label);
    expect(formatUsageTrendTickLabel(last.xKey, points)).not.toBe(
      points[0].label,
    );
  });
});
