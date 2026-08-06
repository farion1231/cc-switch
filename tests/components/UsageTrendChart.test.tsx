import { cleanup, render, screen } from "@testing-library/react";
import { Children } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageTrendChart } from "@/components/usage/UsageTrendChart";

const useUsageTrendsMock = vi.hoisted(() => vi.fn());
const areaChartPropsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageTrends: (...args: unknown[]) => useUsageTrendsMock(...args),
}));

vi.mock("recharts", () => ({
  ResponsiveContainer: ({ children }: any) => <div>{children}</div>,
  AreaChart: ({ children, ...props }: any) => {
    areaChartPropsMock(props);
    return (
      <div>
        {Children.toArray(children).filter(
          (child: any) => child.type !== "defs",
        )}
      </div>
    );
  },
  Area: ({ dataKey, name }: any) => (
    <div data-testid={`area-${dataKey}`} data-name={name} />
  ),
  XAxis: () => null,
  YAxis: () => null,
  CartesianGrid: () => null,
  Tooltip: () => null,
  Legend: () => null,
}));

const range = { preset: "today" as const };
const baseTrend = {
  date: "2026-08-04T10:00:00.000Z",
  requestCount: 1,
  totalCost: "0.100000",
  totalTokens: 30,
  totalInputTokens: 10,
  totalOutputTokens: 5,
  totalCacheCreationTokens: 4,
  totalCacheReadTokens: 3,
  totalCacheWriteTokens: 0,
  totalReasoningTokens: 0,
};

function renderChart() {
  render(
    <UsageTrendChart range={range} rangeLabel="Today" refreshIntervalMs={0} />,
  );
}

describe("UsageTrendChart optional token dimensions", () => {
  beforeEach(() => {
    useUsageTrendsMock.mockReset();
    areaChartPropsMock.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("projects and renders cache-write and reasoning only when exposed", () => {
    useUsageTrendsMock.mockReturnValue({
      data: [
        {
          ...baseTrend,
          totalCacheWriteTokens: 6,
          totalReasoningTokens: 2,
        },
      ],
      isLoading: false,
    });

    renderChart();

    expect(screen.getByTestId("area-cacheWriteTokens")).toHaveAttribute(
      "data-name",
      "usage.cacheWrite",
    );
    expect(screen.getByTestId("area-reasoningTokens")).toHaveAttribute(
      "data-name",
      "usage.hermes.reasoningTokens",
    );
    expect(areaChartPropsMock.mock.calls.at(-1)?.[0].data[0]).toMatchObject({
      cacheWriteTokens: 6,
      reasoningTokens: 2,
    });

    cleanup();
    useUsageTrendsMock.mockReturnValue({
      data: [baseTrend],
      isLoading: false,
    });
    renderChart();

    expect(
      screen.queryByTestId("area-cacheWriteTokens"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("area-reasoningTokens"),
    ).not.toBeInTheDocument();
    expect(
      areaChartPropsMock.mock.calls.at(-1)?.[0].data[0],
    ).not.toHaveProperty("cacheWriteTokens");
    expect(
      areaChartPropsMock.mock.calls.at(-1)?.[0].data[0],
    ).not.toHaveProperty("reasoningTokens");
  });
});
