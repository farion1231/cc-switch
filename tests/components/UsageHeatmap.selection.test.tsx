import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  HEATMAP_RAW_RETENTION_DAYS,
  UsageHeatmap,
  buildHeatmapUsageSelection,
  isSingleDayArchived,
  normalizeHeatmapRange,
  summarizeHeatmapRange,
} from "@/components/usage/UsageHeatmap";
import type { UsageTrendStatLike } from "@/components/usage/UsageTrendChart";

const hoisted = vi.hoisted(() => ({
  firstDate: "2026-09-01",
  trends: [] as UsageTrendStatLike[],
  lastTrendProps: null as null | {
    range: {
      preset: string;
      customStartDate?: number;
      customEndDate?: number;
      liveEndTime?: boolean;
    };
    metric: string;
    embedded?: boolean;
  },
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageFirstDate: () => ({
    isSuccess: true,
    isPending: false,
    data: hoisted.firstDate,
  }),
  useUsageTrends: () => ({
    isLoading: false,
    data: hoisted.trends,
  }),
}));

vi.mock("@/hooks/useMoneyFormat", () => ({
  useMoneyFormat: () => ({
    money: (value: unknown, digits = 2) =>
      `$${Number(value ?? 0).toFixed(digits)}`,
    moneyTick: (value: unknown) => `$${Number(value ?? 0)}`,
    rate: 1,
    currency: "USD",
  }),
}));

vi.mock("@/components/usage/UsageTrendChart", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/usage/UsageTrendChart")>();
  return {
    ...actual,
    UsageTrendChart: (props: {
      range: {
        preset: string;
        customStartDate?: number;
        customEndDate?: number;
        liveEndTime?: boolean;
      };
      metric: string;
      embedded?: boolean;
    }) => {
      hoisted.lastTrendProps = props;
      return (
        <div
          data-testid="mock-heatmap-trend-chart"
          data-start={String(props.range.customStartDate ?? "")}
          data-end={String(props.range.customEndDate ?? "")}
          data-live-end={String(Boolean(props.range.liveEndTime))}
          data-embedded={String(Boolean(props.embedded))}
          data-metric={props.metric}
        />
      );
    },
  };
});

function localDay(daysAgo: number): Date {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo);
}

function dayButtonKey(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

function localTimestampString(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}T00:00:00`;
}

function makeStat(
  date: Date,
  overrides: Partial<UsageTrendStatLike> = {},
): UsageTrendStatLike {
  return {
    date: localTimestampString(date),
    requestCount: 10,
    totalInputTokens: 1_000,
    totalOutputTokens: 500,
    totalCacheCreationTokens: 200,
    totalCacheReadTokens: 8_800,
    totalCost: "1.25",
    ...overrides,
  };
}

function getDayCell(date: Date): HTMLElement {
  const key = dayButtonKey(date);
  const el = document.querySelector<HTMLElement>(`[data-day-key="${key}"]`);
  if (!el) {
    throw new Error(`Day cell not found for key: ${key}`);
  }
  return el;
}

describe("UsageHeatmap selection & range helpers", () => {
  it("normalizes arbitrary endpoint order to [start, end] at local midnight", () => {
    const earlier = new Date(2026, 9, 10, 15, 30);
    const later = new Date(2026, 9, 14, 9, 15);
    const normalized = normalizeHeatmapRange(later, earlier);
    expect(normalized.start).toEqual(new Date(2026, 9, 10, 0, 0, 0, 0));
    expect(normalized.end).toEqual(new Date(2026, 9, 14, 0, 0, 0, 0));
  });

  it("builds local 00:00:00–23:59:59 custom range with liveEndTime false", () => {
    const start = new Date(2026, 10, 20);
    const end = new Date(2026, 10, 22);
    const selection = buildHeatmapUsageSelection({ start, end });
    expect(selection).toEqual({
      preset: "custom",
      customStartDate: Math.floor(
        new Date(2026, 10, 20, 0, 0, 0).getTime() / 1000,
      ),
      customEndDate: Math.floor(
        new Date(2026, 10, 22, 23, 59, 59).getTime() / 1000,
      ),
      liveEndTime: false,
    });
  });

  it("flags single-day selections at or beyond retention days as archived, but allows multi-day", () => {
    const today = new Date(2026, 10, 26);
    const recent = new Date(2026, 10, 26 - (HEATMAP_RAW_RETENTION_DAYS - 1));
    const archived = new Date(2026, 10, 26 - HEATMAP_RAW_RETENTION_DAYS);

    expect(isSingleDayArchived({ start: recent, end: recent }, today)).toBe(
      false,
    );
    expect(isSingleDayArchived({ start: archived, end: archived }, today)).toBe(
      true,
    );
    expect(isSingleDayArchived({ start: archived, end: recent }, today)).toBe(
      false,
    );
  });

  it("summarizes weighted cache hit rate and totals across days", () => {
    const d1 = new Date(2026, 10, 20);
    const d3 = new Date(2026, 10, 22);
    const today = new Date(2026, 10, 26);

    const byDay = new Map<string, UsageTrendStatLike>([
      [
        dayButtonKey(d1),
        makeStat(d1, {
          requestCount: 4,
          totalInputTokens: 100,
          totalOutputTokens: 50,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 900,
          totalCost: "1.5",
        }),
      ],
      // d2 故意留空，验证空日计入总天数但不算活跃天
      [
        dayButtonKey(d3),
        makeStat(d3, {
          requestCount: 6,
          totalInputTokens: 500,
          totalOutputTokens: 100,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 500,
          totalCost: "2.5",
        }),
      ],
    ]);

    const summary = summarizeHeatmapRange({ start: d1, end: d3 }, byDay, today);
    expect(summary.days).toBe(3);
    expect(summary.activeDays).toBe(2);
    expect(summary.requests).toBe(10);
    // d1: 1050, d3: 1100
    expect(summary.tokens).toBe(2150);
    expect(summary.cost).toBeCloseTo(4.0);
    // 加权命中率 = (900 + 500) / ((100 + 900) + (500 + 500)) = 1400 / 2000 = 0.7
    expect(summary.hitRate).toBeCloseTo(0.7);
  });
});

describe("UsageHeatmap inline drawer & contiguous multi-select", () => {
  const recentA = localDay(5);
  const recentMid = localDay(4);
  const recentB = localDay(3);
  const archivedDay = localDay(40);

  beforeEach(() => {
    hoisted.lastTrendProps = null;
    hoisted.firstDate = "2026-01-01";
    hoisted.trends = [
      makeStat(recentA, { requestCount: 12, totalCost: "2.00" }),
      makeStat(recentMid, { requestCount: 8, totalCost: "1.50" }),
      makeStat(recentB, { requestCount: 20, totalCost: "4.00" }),
      makeStat(archivedDay, { requestCount: 30, totalCost: "6.00" }),
    ];
  });

  it("expands inline drawer on day click and toggles closed on second click", () => {
    render(<UsageHeatmap metric="tokens" refreshIntervalMs={0} />);

    const drawer = screen.getByTestId("usage-heatmap-drawer");
    expect(drawer).toHaveAttribute("data-state", "closed");
    expect(drawer).toHaveAttribute("aria-hidden", "true");
    expect(
      screen.queryByTestId("mock-heatmap-trend-chart"),
    ).not.toBeInTheDocument();

    const cellA = getDayCell(recentA);
    fireEvent.click(cellA);

    expect(drawer).toHaveAttribute("data-state", "open");
    expect(drawer).toHaveAttribute("aria-hidden", "false");
    expect(cellA).toHaveAttribute("aria-pressed", "true");

    const chart = screen.getByTestId("mock-heatmap-trend-chart");
    expect(chart).toHaveAttribute("data-embedded", "true");
    expect(chart).toHaveAttribute("data-live-end", "false");
    expect(hoisted.lastTrendProps?.range).toEqual(
      buildHeatmapUsageSelection({ start: recentA, end: recentA }),
    );

    // 再次点击同一天：收起抽屉
    fireEvent.click(cellA);
    expect(drawer).toHaveAttribute("data-state", "closed");
    expect(cellA).toHaveAttribute("aria-pressed", "false");
  });

  it("supports contiguous multi-select in reverse order and forwards selection via onApplyRange", () => {
    const onApplyRange = vi.fn();
    render(
      <UsageHeatmap
        metric="cost"
        refreshIntervalMs={0}
        onApplyRange={onApplyRange}
      />,
    );

    // 开启「连续多选」开关
    fireEvent.click(
      screen.getByRole("switch", { name: "usage.heatmap.rangeSelect" }),
    );

    // 先点较晚的 B，再点较早的 A（验证逆序自动归一化为 [A, B]）
    fireEvent.click(getDayCell(recentB));
    expect(screen.getByText("usage.heatmap.pickRangeEnd")).toBeInTheDocument();

    fireEvent.click(getDayCell(recentA));
    expect(
      screen.queryByText("usage.heatmap.pickRangeEnd"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("usage.heatmap.dayCount")).toBeInTheDocument();

    // 起止与中间格子均处于选中态
    expect(getDayCell(recentA)).toHaveAttribute("aria-pressed", "true");
    expect(getDayCell(recentMid)).toHaveAttribute("aria-pressed", "true");
    expect(getDayCell(recentB)).toHaveAttribute("aria-pressed", "true");

    const expectedRange = buildHeatmapUsageSelection({
      start: recentA,
      end: recentB,
    });
    expect(hoisted.lastTrendProps?.range).toEqual(expectedRange);

    fireEvent.click(
      screen.getByRole("button", { name: "usage.heatmap.applyRange" }),
    );
    expect(onApplyRange).toHaveBeenCalledTimes(1);
    expect(onApplyRange).toHaveBeenCalledWith(expectedRange);
  });

  it("shows daily rollup notice for archived single day and allows extending into multi-day trend", () => {
    render(<UsageHeatmap metric="requests" refreshIntervalMs={0} />);

    fireEvent.click(getDayCell(archivedDay));
    expect(screen.getByText("usage.heatmap.archivedTitle")).toBeInTheDocument();
    expect(
      screen.queryByTestId("mock-heatmap-trend-chart"),
    ).not.toBeInTheDocument();

    // 开启连续多选后继续点前一天：原有归档单日自动延续为区间起点，切入多日趋势图
    fireEvent.click(
      screen.getByRole("switch", { name: "usage.heatmap.rangeSelect" }),
    );
    fireEvent.click(getDayCell(recentB));
    expect(
      screen.queryByText("usage.heatmap.archivedTitle"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("mock-heatmap-trend-chart")).toBeInTheDocument();
  });

  it("closes drawer via close button and Escape key", () => {
    render(<UsageHeatmap metric="tokens" refreshIntervalMs={0} />);

    fireEvent.click(getDayCell(recentA));
    const drawer = screen.getByTestId("usage-heatmap-drawer");
    expect(drawer).toHaveAttribute("data-state", "open");

    fireEvent.click(
      screen.getByRole("button", { name: "usage.heatmap.closeDetail" }),
    );
    expect(drawer).toHaveAttribute("data-state", "closed");

    fireEvent.click(getDayCell(recentB));
    expect(drawer).toHaveAttribute("data-state", "open");
    fireEvent.keyDown(getDayCell(recentB), { key: "Escape" });
    expect(drawer).toHaveAttribute("data-state", "closed");
  });
});
