import { render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { UsageHero } from "./UsageHero";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import type { UsageSummary, UsageSummaryByApp } from "@/types/usage";

vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: vi.fn(),
}));

const useUsageSummaryByAppMock = vi.mocked(useUsageSummaryByApp);

function summary(overrides: Partial<UsageSummary> = {}): UsageSummary {
  return {
    totalRequests: 1,
    totalCost: "0",
    totalInputTokens: 100,
    totalOutputTokens: 10,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
    cacheObservedRequests: 0,
    cacheObservedInputTokens: 0,
    cacheObservedCreationTokens: 0,
    cacheObservedReadTokens: 0,
    successRate: 100,
    realTotalTokens: 110,
    cacheHitRate: null,
    ...overrides,
  };
}

function renderHero(data: UsageSummaryByApp[]) {
  useUsageSummaryByAppMock.mockReturnValue({
    data,
    isLoading: false,
  } as ReturnType<typeof useUsageSummaryByApp>);

  render(<UsageHero range={{ preset: "today" }} refreshIntervalMs={0} />);
}

describe("UsageHero cache hit observability", () => {
  it("shows N/A when no request has observable cache fields", () => {
    renderHero([{ appType: "claude", summary: summary() }]);

    expect(screen.getByText("N/A")).toBeInTheDocument();
    expect(screen.queryByText("仅基于部分请求")).not.toBeInTheDocument();
  });

  it("shows the hit rate without a caveat when every request is observable", () => {
    renderHero([
      {
        appType: "claude",
        summary: summary({
          totalRequests: 2,
          totalInputTokens: 150,
          totalCacheReadTokens: 50,
          cacheObservedRequests: 2,
          cacheObservedInputTokens: 150,
          cacheObservedReadTokens: 50,
          realTotalTokens: 210,
          cacheHitRate: 0.25,
        }),
      },
    ]);

    expect(screen.getByText("25.0%")).toBeInTheDocument();
    expect(screen.queryByText("仅基于部分请求")).not.toBeInTheDocument();
  });

  it("aggregates only the observable subset and marks partial coverage", () => {
    renderHero([
      {
        appType: "claude",
        summary: summary({
          totalInputTokens: 50,
          totalCacheReadTokens: 50,
          cacheObservedRequests: 1,
          cacheObservedInputTokens: 50,
          cacheObservedReadTokens: 50,
          realTotalTokens: 110,
          cacheHitRate: 0.5,
        }),
      },
      {
        appType: "cursor",
        summary: summary({
          totalInputTokens: 900,
          realTotalTokens: 910,
        }),
      },
    ]);

    expect(screen.getByText("50.0%")).toBeInTheDocument();
    expect(screen.getByText("仅基于部分请求")).toBeInTheDocument();
  });
});
