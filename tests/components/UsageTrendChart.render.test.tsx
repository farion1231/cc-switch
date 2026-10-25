import type { ComponentProps, ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { render as rtlRender, screen } from "@testing-library/react";
import {
  createTestQueryClient,
  renderWithQueryClient as render,
} from "../utils/testQueryClient";
import { describe, expect, it, vi } from "vitest";
import { UsageTrendChart } from "@/components/usage/UsageTrendChart";

vi.mock("recharts", async (importOriginal) => {
  const recharts = await importOriginal<typeof import("recharts")>();
  return {
    ...recharts,
    // jsdom has no layout; keep the actual charts with fixed container dimensions.
    ResponsiveContainer: (
      props: ComponentProps<typeof recharts.ResponsiveContainer>,
    ) => <recharts.ResponsiveContainer {...props} width={640} height={132} />,
  };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageTrends: () => ({
    data: [
      {
        date: "2026-09-30T00:00:00Z",
        requestCount: 12,
        totalInputTokens: 100,
        totalOutputTokens: 50,
        totalCacheCreationTokens: 0,
        totalCacheReadTokens: 0,
        totalCost: "0.01",
      },
    ],
    isLoading: false,
  }),
}));

describe("UsageTrendChart (single metric)", () => {
  it("titles the chart with the metric picked on the metric strip", () => {
    const { rerender } = render(
      <UsageTrendChart
        range={{ preset: "7d" }}
        metric="tokens"
        refreshIntervalMs={0}
      />,
    );
    // 图表自己不再有指标切换和单系列图例：指标由上方指标条选
    expect(
      screen.getByRole("heading", { name: "usage.trend.titleMetric" }),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("usage-trend-legend")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "usage.trend.cost" }),
    ).not.toBeInTheDocument();

    rerender(
      <UsageTrendChart
        range={{ preset: "7d" }}
        metric="hitRate"
        refreshIntervalMs={0}
      />,
    );
    expect(
      screen.getByRole("heading", { name: "usage.trend.titleMetric" }),
    ).toBeInTheDocument();
  });
});

describe("UsageTrendChart styles", () => {
  // 样式在用量「设置」里选，存在 settings.usageDisplay.trendChartType
  const renderWithChartType = (
    trendChartType?: string,
    metric: "cost" | "requests" | "tokens" | "hitRate" = "tokens",
  ) => {
    const props = {
      range: { preset: "7d" as const },
      metric,
      refreshIntervalMs: 0,
    };
    const client = createTestQueryClient();
    // 不让挂载时的重新拉取用 mock 后端的默认设置覆盖掉这里给的值
    client.setDefaultOptions({
      queries: { retry: false, staleTime: Infinity },
    });
    client.setQueryData(["settings"], { usageDisplay: { trendChartType } });
    return rtlRender(<UsageTrendChart {...props} />, {
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
  };

  it("draws a bar chart by default without a style switch on the chart", () => {
    const { container } = renderWithChartType(undefined);
    expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();
    expect(container.querySelector(".recharts-area")).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "usage.display.trendChartTypes.area",
      }),
    ).not.toBeInTheDocument();
  });

  it.each(["tokens", "requests", "cost", "hitRate"] as const)(
    "draws the saved area style for the %s metric",
    (metric) => {
      const { container } = renderWithChartType("area", metric);
      expect(container.querySelector(".recharts-area")).not.toBeNull();
      // A one-bucket range must still show a point in area mode.
      expect(container.querySelector(".recharts-area-dot")).not.toBeNull();
      expect(container.querySelector(".recharts-bar-rectangle")).toBeNull();
    },
  );

  it("falls back to a bar chart for an unknown saved style", () => {
    const { container } = renderWithChartType("unknown");
    expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();
    expect(container.querySelector(".recharts-area")).toBeNull();
  });
});
