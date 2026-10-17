import type { ComponentProps } from "react";
import { screen, within } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UsageTrendChart } from "@/components/usage/UsageTrendChart";
import { USAGE_CHOICE_KEYS } from "@/components/usage/rememberedChoice";

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

  it.each(["tokens", "requests", "cost"] as const)(
    "switches between bar and area charts while keeping the %s metric",
    async (metric) => {
      const user = userEvent.setup();
      const { container } = render(<UsageTrendChart {...props} />);
      await user.click(
        screen.getByRole("button", { name: `usage.trend.${metric}` }),
      );
      const controls = within(
        screen.getByRole("group", { name: "usage.trend.chartTypeLabel" }),
      );
      const bar = controls.getByRole("button", { name: "usage.trend.bar" });
      const area = controls.getByRole("button", { name: "usage.trend.area" });
      const legend = screen.getByTestId("usage-trend-legend").textContent;

      expect(bar).toHaveAttribute("aria-pressed", "true");
      expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();

      await user.click(area);
      expect(area).toHaveAttribute("aria-pressed", "true");
      expect(bar).toHaveAttribute("aria-pressed", "false");
      expect(container.querySelector(".recharts-area")).not.toBeNull();
      // A one-bucket range must still show a point in area mode.
      expect(container.querySelector(".recharts-area-dot")).not.toBeNull();
      expect(container.querySelector(".recharts-bar-rectangle")).toBeNull();
      expect(screen.getByTestId("usage-trend-legend").textContent).toBe(legend);

      await user.click(bar);
      expect(bar).toHaveAttribute("aria-pressed", "true");
      expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();
      expect(container.querySelector(".recharts-area")).toBeNull();
      expect(screen.getByTestId("usage-trend-legend").textContent).toBe(legend);
    },
  );

  it("remembers the chart style independently of the metric and filters", async () => {
    const user = userEvent.setup();
    const first = render(<UsageTrendChart {...props} />);
    await user.click(screen.getByRole("button", { name: "usage.trend.area" }));
    await user.click(screen.getByRole("button", { name: "usage.trend.cost" }));

    first.rerender(
      <UsageTrendChart
        {...props}
        range={{ preset: "1d" }}
        appType="codex"
        model="gpt-5"
      />,
    );
    expect(
      screen.getByRole("button", { name: "usage.trend.area" }),
    ).toHaveAttribute("aria-pressed", "true");
    first.unmount();

    render(<UsageTrendChart {...props} />);
    expect(
      screen.getByRole("button", { name: "usage.trend.area" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByTestId("usage-trend-legend")).toHaveTextContent(
      "usage.trend.cost",
    );
  });

  it("falls back to a bar chart for an unknown saved style", () => {
    localStorage.setItem(USAGE_CHOICE_KEYS.trendChartType, "unknown");
    const { container } = render(<UsageTrendChart {...props} />);

    expect(
      screen.getByRole("button", { name: "usage.trend.bar" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();
  });
});
