import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageHero } from "@/components/usage/UsageHero";
import { UsageHeatmap } from "@/components/usage/UsageHeatmap";
import { renderWithQueryClient } from "../utils/testQueryClient";

const useUsageSummaryByAppMock = vi.hoisted(() => vi.fn());
const useUsageFirstDateMock = vi.hoisted(() => vi.fn());
const useUsageTrendsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: undefined, isLoading: false }),
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: (...args: unknown[]) =>
    useUsageSummaryByAppMock(...args),
  useUsageFirstDate: (...args: unknown[]) => useUsageFirstDateMock(...args),
  useUsageTrends: (...args: unknown[]) => useUsageTrendsMock(...args),
}));

const summaryRows = [
  {
    appType: "codex",
    summary: {
      totalRequests: 9,
      totalCost: "1.25",
      totalInputTokens: 300,
      totalOutputTokens: 200,
      totalCacheCreationTokens: 100,
      totalCacheReadTokens: 400,
      successRate: 100,
      realTotalTokens: 1000,
      cacheHitRate: 0.5,
    },
  },
];

const trendRows = [
  {
    date: new Date().toISOString(),
    requestCount: 9,
    totalInputTokens: 300,
    totalOutputTokens: 200,
    totalCacheCreationTokens: 100,
    totalCacheReadTokens: 400,
    totalCost: "1.25",
  },
];

beforeEach(() => {
  useUsageSummaryByAppMock.mockReset().mockReturnValue({
    data: summaryRows,
    isLoading: false,
    isPlaceholderData: false,
  });
  useUsageFirstDateMock.mockReset().mockReturnValue({
    data: new Date().toISOString().slice(0, 10),
    isSuccess: true,
    isPending: false,
    isPlaceholderData: false,
  });
  useUsageTrendsMock.mockReset().mockReturnValue({
    data: trendRows,
    isLoading: false,
    isPlaceholderData: false,
  });
});

describe("usage overview scope placeholders", () => {
  const heroProps = {
    range: { preset: "7d" as const },
    appType: "codex",
    metric: "requests" as const,
    refreshIntervalMs: 0,
    onMetricChange: vi.fn(),
  };

  it("replaces the old hero values during a scope switch but keeps same-scope refresh data", () => {
    const { rerender } = renderWithQueryClient(<UsageHero {...heroProps} />);
    expect(
      screen.getByRole("button", { name: "usage.totalRequests 9" }),
    ).toBeInTheDocument();

    useUsageSummaryByAppMock.mockReturnValue({
      data: summaryRows,
      isLoading: false,
      isPlaceholderData: true,
      isFetching: true,
    });
    rerender(<UsageHero {...heroProps} projectKeys={["synthetic-project"]} />);
    expect(
      screen.getByRole("button", { name: "usage.totalRequests …" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "usage.totalRequests 9" }),
    ).not.toBeInTheDocument();

    useUsageSummaryByAppMock.mockReturnValue({
      data: summaryRows,
      isLoading: false,
      isPlaceholderData: false,
      isFetching: true,
    });
    rerender(<UsageHero {...heroProps} projectKeys={["synthetic-project"]} />);
    expect(
      screen.getByRole("button", { name: "usage.totalRequests 9" }),
    ).toBeInTheDocument();
  });

  it("waits for a non-placeholder first date before showing heatmap spans", () => {
    useUsageFirstDateMock.mockReturnValue({
      data: "2020-01-01",
      isSuccess: true,
      isPending: false,
      isPlaceholderData: true,
    });
    const { container } = renderWithQueryClient(
      <UsageHeatmap
        metric="requests"
        projectKeys={["synthetic-project"]}
        refreshIntervalMs={0}
      />,
    );

    expect(container.querySelector("svg.animate-spin")).not.toBeNull();
    expect(screen.queryByText("usage.heatmap.spanSummary")).toBeNull();
    expect(useUsageTrendsMock).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ projectKeys: ["synthetic-project"] }),
      expect.objectContaining({ enabled: false }),
    );
  });

  it("hides previous-scope heatmap trends while new trends are pending", () => {
    useUsageTrendsMock.mockReturnValue({
      data: trendRows,
      isLoading: false,
      isPlaceholderData: true,
      isFetching: true,
    });
    const { container } = renderWithQueryClient(
      <UsageHeatmap
        metric="requests"
        projectKeys={["synthetic-project"]}
        refreshIntervalMs={0}
      />,
    );

    expect(container.querySelector("svg.animate-spin")).not.toBeNull();
    expect(screen.queryByText("usage.heatmap.spanSummary")).toBeNull();
  });
});
