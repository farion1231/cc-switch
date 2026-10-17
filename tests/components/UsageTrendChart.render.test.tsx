import type { ComponentProps, ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { render as rtlRender, screen } from "@testing-library/react";
import {
  createTestQueryClient,
  renderWithQueryClient as render,
} from "../utils/testQueryClient";
import userEvent from "@testing-library/user-event";
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
  it("shows only the selected metric in the legend and follows the switch", async () => {
    const user = userEvent.setup();
    render(
      <UsageTrendChart
        range={{ preset: "7d" }}
        rangeLabel="7d"
        refreshIntervalMs={0}
      />,
    );

    const legend = screen.getByTestId("usage-trend-legend");
    // 默认「Tokens」（与热力图一致），且不再有第二条「成本（右轴）」图例
    expect(legend).toHaveTextContent("usage.trend.tokens");
    expect(screen.queryByText(/usage.trend.costLine/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "usage.trend.cost" }));
    expect(screen.getByTestId("usage-trend-legend")).toHaveTextContent(
      "usage.trend.cost",
    );

    await user.click(
      screen.getByRole("button", { name: "usage.trend.requests" }),
    );
    expect(screen.getByTestId("usage-trend-legend")).toHaveTextContent(
      "usage.trend.requestsLegend",
    );
  });

  it("reopens on the metric picked last time", async () => {
    const user = userEvent.setup();
    const props = {
      range: { preset: "7d" as const },
      rangeLabel: "7d",
      refreshIntervalMs: 0,
    };
    const first = render(<UsageTrendChart {...props} />);
    await user.click(screen.getByRole("button", { name: "usage.trend.cost" }));
    first.unmount();

    render(<UsageTrendChart {...props} />);
    expect(screen.getByTestId("usage-trend-legend")).toHaveTextContent(
      "usage.trend.cost",
    );
  });
});

describe("UsageTrendChart styles", () => {
  const props = {
    range: { preset: "7d" as const },
    rangeLabel: "7d",
    refreshIntervalMs: 0,
  };

  // 样式在用量「设置」里选，存在 settings.usageDisplay.trendChartType
  const renderWithChartType = (trendChartType?: string) => {
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

  it.each(["tokens", "requests", "cost"] as const)(
    "draws the saved area style for the %s metric",
    async (metric) => {
      const user = userEvent.setup();
      const { container } = renderWithChartType("area");
      await user.click(
        screen.getByRole("button", { name: `usage.trend.${metric}` }),
      );
      expect(container.querySelector(".recharts-area")).not.toBeNull();
      // A one-bucket range must still show a point in area mode.
      expect(container.querySelector(".recharts-area-dot")).not.toBeNull();
      expect(container.querySelector(".recharts-bar-rectangle")).toBeNull();
      expect(screen.getByTestId("usage-trend-legend")).toHaveTextContent(
        `usage.trend.${metric === "requests" ? "requestsLegend" : metric}`,
      );
    },
  );

  it("falls back to a bar chart for an unknown saved style", () => {
    const { container } = renderWithChartType("unknown");
    expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();
    expect(container.querySelector(".recharts-area")).toBeNull();
  });
});
