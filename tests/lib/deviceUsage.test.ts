import { describe, expect, it } from "vitest";
import {
  deviceUsagePresentation,
  selectDeviceUsageRows,
} from "@/lib/deviceUsage";
import { resolveUsageRange } from "@/lib/usageRange";
import { buildUsageTrendChartData } from "@/components/usage/UsageTrendChart";
import type { DeviceUsageReport } from "@/lib/api/deviceUsage";
const row = {
  date: "2026-10-11",
  appType: "codex",
  model: "shared",
  verified: true,
  input: 50,
  output: 10,
  cacheRead: 30,
  cacheWrite: 0,
  requests: 1,
  cost: "0.1",
};
const other = {
  ...row,
  appType: "claude",
  verified: false,
  cacheWrite: 20,
  requests: 2,
  cost: "0.2",
};
describe("snapshot adapters", () => {
  it("preserves cache-normalized totals across all existing component shapes", () => {
    const data = deviceUsagePresentation([row, other]);
    expect(data.apps.reduce((s, r) => s + r.summary.realTotalTokens, 0)).toBe(
      200,
    );
    expect(data.trends[0]).toMatchObject({
      totalTokens: 200,
      requestCount: 3,
      totalCacheReadTokens: 60,
      totalCacheCreationTokens: 20,
    });
    expect(data.models[0]).toMatchObject({
      totalTokens: 200,
      requestCount: 3,
      estimated: true,
    });
    expect(data.models[0].successRate).toBeUndefined();
    expect(data.daily).toHaveLength(2);
  });
  it("folds Desktop into Claude for filters and presentation without changing transport rows", () => {
    const desktop = { ...row, appType: "claude-desktop" };
    const report = { rows: [desktop, other, row] } as DeviceUsageReport;
    const selected = selectDeviceUsageRows(report, { preset: "all" }, "claude");
    expect(selected).toEqual([desktop, other]);
    const data = deviceUsagePresentation(selected);
    expect(data.apps).toHaveLength(1);
    expect(data.apps[0]).toMatchObject({
      appType: "claude",
      summary: { totalRequests: 3, realTotalTokens: 200 },
    });
    expect(Number(data.apps[0].summary.totalCost)).toBeCloseTo(0.3);
    expect(data.trends[0].requestCount).toBe(3);
    expect(data.models[0].requestCount).toBe(3);
    expect(data.daily.map((r) => r.appType)).toEqual(["claude", "claude"]);
    expect(desktop.appType).toBe("claude-desktop");
    expect(selectDeviceUsageRows(report, { preset: "all" }, "codex")).toEqual([
      row,
    ]);
    expect(
      selectDeviceUsageRows(
        report,
        { preset: "all" },
        "claude",
        "shared",
        "verified",
      ),
    ).toEqual([desktop]);
  });
  it("uses UTC date boundaries and provenance without altering the local range resolver", () => {
    const now = Date.parse("2026-10-11T01:00:00Z");
    expect(resolveUsageRange({ preset: "today" }, now, "UTC").startDate).toBe(
      Date.parse("2026-10-11T00:00:00Z") / 1000,
    );
    expect(resolveUsageRange({ preset: "7d" }, now, "UTC").startDate).toBe(
      Date.parse("2026-10-05T00:00:00Z") / 1000,
    );
    const report = {
      rows: [row, other, { ...row, date: "2026-10-10" }],
    } as DeviceUsageReport;
    expect(
      selectDeviceUsageRows(
        report,
        { preset: "today" },
        "all",
        undefined,
        "unverified",
        now,
      ),
    ).toEqual([other]);
    expect(
      selectDeviceUsageRows(
        report,
        { preset: "all" },
        "codex",
        undefined,
        "all",
        now,
      ),
    ).toHaveLength(2);
  });
  it("does not shift UTC snapshot dates on the shared trend chart", () => {
    const data = deviceUsagePresentation([row]);
    const points = buildUsageTrendChartData(data.trends, {
      isHourly: false,
      dateLocale: "en-US",
      startDate: 0,
      endDate: Date.parse("2026-10-11T12:00:00Z") / 1000,
      timeZone: "UTC",
    });
    expect(points[0].rawDate).toBe("2026-10-11");
    expect(points[0].tooltipLabel).toBe("10/11/2026");
    expect(points[0].tokens).toBe(90);
  });
});
