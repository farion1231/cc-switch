import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UsageTrendChart } from "@/components/usage/UsageTrendChart";

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
});
