import { describe, expect, it } from "vitest";
import {
  buildUsageTrendChartData,
  createUsageTrendTokenTickFormatter,
  formatUsageTrendTokenTickLabel,
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

describe("formatUsageTrendTokenTickLabel", () => {
  it("uses localized compact units for large token axis ticks", () => {
    const zhFormatter = createUsageTrendTokenTickFormatter("zh-CN");
    const zhTwFormatter = createUsageTrendTokenTickFormatter("zh-TW");
    const enFormatter = createUsageTrendTokenTickFormatter("en-US");

    expect(formatUsageTrendTokenTickLabel(600_000_000, zhFormatter)).toBe(
      "6亿",
    );
    expect(formatUsageTrendTokenTickLabel(1_950_000_000, zhFormatter)).toBe(
      "19.5亿",
    );
    expect(formatUsageTrendTokenTickLabel(65_000_000, zhTwFormatter)).toBe(
      "6.5千萬",
    );
    expect(formatUsageTrendTokenTickLabel(600_000_000, enFormatter)).toBe(
      "600M",
    );
  });

  it("writes four-digit wan / yi ticks with a thousand unit in CJK", () => {
    const zh = createUsageTrendTokenTickFormatter("zh-CN");
    const zhTw = createUsageTrendTokenTickFormatter("zh-TW");
    const ja = createUsageTrendTokenTickFormatter("ja-JP");
    const en = createUsageTrendTokenTickFormatter("en-US");

    expect(formatUsageTrendTokenTickLabel(50_000_000, zh)).toBe("5千万");
    expect(formatUsageTrendTokenTickLabel(15_000_000, zh)).toBe("1.5千万");
    expect(formatUsageTrendTokenTickLabel(200_000_000_000, zh)).toBe("2千亿");
    expect(formatUsageTrendTokenTickLabel(50_000_000, zhTw)).toBe("5千萬");
    expect(formatUsageTrendTokenTickLabel(50_000_000, ja)).toBe("5千万");
    expect(formatUsageTrendTokenTickLabel(200_000_000_000, ja)).toBe("2千億");
    // 三位以内照旧，进位到 10 的交回原格式
    expect(formatUsageTrendTokenTickLabel(5_000_000, zh)).toBe("500万");
    expect(formatUsageTrendTokenTickLabel(100_000_000, zh)).toBe("1亿");
    expect(formatUsageTrendTokenTickLabel(99_600_000, zh)).toBe("9960万");
    expect(formatUsageTrendTokenTickLabel(50_000_000, en)).toBe("50M");
  });

  it("keeps zero and small-thousand token ticks readable", () => {
    expect(
      formatUsageTrendTokenTickLabel(
        0,
        createUsageTrendTokenTickFormatter("zh-CN"),
      ),
    ).toBe("0");
    expect(
      formatUsageTrendTokenTickLabel(
        1500,
        createUsageTrendTokenTickFormatter("en-US"),
      ),
    ).toBe("1.5K");
  });
});
