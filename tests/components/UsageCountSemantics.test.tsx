import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ModelStatsTable } from "@/components/usage/ModelStatsTable";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import { UsageHero } from "@/components/usage/UsageHero";

const labels = vi.hoisted(() => ({
  requests: "Requests",
  hermesApiCalls: "Aggregate API calls",
  mixedActivity: "Counted activity",
  perRequest: "Average Cost",
  perApiCall: "Average cost per API call",
  perActivity: "Average cost per counted activity",
  successRate: "Success Rate",
  avgLatency: "Average Latency",
}));
const useUsageSummaryByAppMock = vi.hoisted(() => vi.fn());
const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const translations: Record<string, string> = {
        "usage.countLabel.requests": labels.requests,
        "usage.countLabel.hermesApiCalls": labels.hermesApiCalls,
        "usage.countLabel.mixedActivity": labels.mixedActivity,
        "usage.averageCostLabel.perRequest": labels.perRequest,
        "usage.averageCostLabel.perApiCall": labels.perApiCall,
        "usage.averageCostLabel.perActivity": labels.perActivity,
        "usage.successRate": labels.successRate,
        "usage.avgLatency": labels.avgLatency,
      };
      return translations[key] ?? key;
    },
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: (...args: unknown[]) =>
    useUsageSummaryByAppMock(...args),
  useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
  useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
}));

const range = { preset: "today" as const };
const summary = {
  totalRequests: 2,
  totalCost: "1.000000",
  totalInputTokens: 10,
  totalOutputTokens: 5,
  totalCacheCreationTokens: 0,
  totalCacheReadTokens: 0,
  realTotalTokens: 15,
  cacheHitRate: 0,
  successRate: 100,
  statusAvailable: true,
};
const providerStats = [
  {
    providerId: "provider-1",
    providerName: "Provider",
    requestCount: 2,
    totalTokens: 15,
    totalInputTokens: 10,
    totalOutputTokens: 5,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
    totalCacheWriteTokens: 0,
    totalReasoningTokens: 0,
    totalCost: "1.000000",
    successRate: 100,
    statusAvailable: true,
    avgLatencyMs: 10,
    latencyAvailable: true,
  },
];
const modelStats = [
  {
    model: "model",
    requestCount: 2,
    totalTokens: 15,
    totalInputTokens: 10,
    totalOutputTokens: 5,
    totalCacheCreationTokens: 0,
    totalCacheReadTokens: 0,
    totalCacheWriteTokens: 0,
    totalReasoningTokens: 0,
    totalCost: "1.000000",
    avgCostPerRequest: "0.500000",
  },
];

function renderCountedStats(appType: string) {
  render(
    <>
      <UsageHero
        range={range}
        appType={appType === "all" ? undefined : appType}
        refreshIntervalMs={0}
      />
      <ProviderStatsTable
        range={range}
        appType={appType}
        refreshIntervalMs={0}
      />
      <ModelStatsTable range={range} appType={appType} refreshIntervalMs={0} />
    </>,
  );
}

describe("usage count semantics", () => {
  beforeEach(() => {
    useUsageSummaryByAppMock.mockReset();
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    useUsageSummaryByAppMock.mockReturnValue({
      data: [
        { appType: "claude", summary },
        { appType: "hermes", summary },
      ],
      isLoading: false,
    });
    useProviderStatsMock.mockReturnValue({
      data: providerStats,
      isLoading: false,
    });
    useModelStatsMock.mockReturnValue({ data: modelStats, isLoading: false });
  });

  afterEach(() => {
    cleanup();
  });

  it("sums legacy cache creation and Hermes cache writes in All mode", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [
        {
          appType: "claude",
          summary: {
            ...summary,
            totalCacheCreationTokens: 120,
            realTotalTokens: 135,
          },
        },
        {
          appType: "hermes",
          summary: {
            ...summary,
            totalCacheCreationTokens: 40,
            totalCacheWriteTokens: 30,
            realTotalTokens: 45,
          },
        },
      ],
      isLoading: false,
    });

    renderCountedStats("all");

    const cacheWriteLabel = screen.getByText("usage.cacheWrite");
    expect(
      within(cacheWriteLabel.parentElement!.parentElement!).getByText("150"),
    ).toBeInTheDocument();
  });

  it("preserves known legacy cache creation as partial when Hermes cache write is absent in All mode", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [
        {
          appType: "claude",
          summary: {
            ...summary,
            totalCacheCreationTokens: 120,
            realTotalTokens: 135,
          },
        },
        {
          appType: "hermes",
          summary: {
            ...summary,
            realTotalTokens: 15,
          },
        },
      ],
      isLoading: false,
    });

    renderCountedStats("all");

    const cacheWriteLabel = screen.getByText("usage.cacheWrite");
    const cacheWriteStat = cacheWriteLabel.parentElement!.parentElement!;
    expect(within(cacheWriteStat).getByText("120")).toHaveClass(
      "text-muted-foreground/70",
    );
    expect(cacheWriteStat).toHaveAttribute("title", "usage.cacheWritePartial");
  });

  it("keeps Hermes-only cache write separate from cache creation", () => {
    useUsageSummaryByAppMock.mockReturnValue({
      data: [
        {
          appType: "hermes",
          summary: {
            ...summary,
            totalCacheCreationTokens: 40,
            totalCacheWriteTokens: 30,
          },
        },
      ],
      isLoading: false,
    });

    render(<UsageHero range={range} appType="hermes" refreshIntervalMs={0} />);

    const cacheWriteLabel = screen.getByText("usage.cacheWrite");
    expect(
      within(cacheWriteLabel.parentElement!.parentElement!).getByText("30"),
    ).toBeInTheDocument();
  });

  it.each([
    ["hermes", labels.hermesApiCalls, labels.perApiCall],
    ["all", labels.mixedActivity, labels.perActivity],
    ["claude", labels.requests, labels.perRequest],
  ])(
    "uses source-aware count and average labels for %s",
    (appType, countLabel, averageLabel) => {
      renderCountedStats(appType);

      expect(screen.getAllByText(countLabel)).toHaveLength(3);
      expect(
        screen.getByRole("columnheader", { name: averageLabel }),
      ).toBeInTheDocument();
    },
  );

  it("removes status and latency columns and uses a four-column empty state for Hermes", () => {
    useProviderStatsMock.mockReturnValue({ data: [], isLoading: false });
    renderCountedStats("hermes");

    const table = screen.getAllByRole("table")[0]!;
    expect(within(table).getAllByRole("columnheader")).toHaveLength(4);
    expect(
      within(table).queryByRole("columnheader", { name: labels.successRate }),
    ).not.toBeInTheDocument();
    expect(
      within(table).queryByRole("columnheader", { name: labels.avgLatency }),
    ).not.toBeInTheDocument();
    expect(within(table).getByRole("cell")).toHaveAttribute("colspan", "4");
  });

  it.each(["all", "claude"])(
    "keeps status and latency columns for %s mode",
    (appType) => {
      renderCountedStats(appType);

      const table = screen.getAllByRole("table")[0]!;
      expect(within(table).getAllByRole("columnheader")).toHaveLength(6);
      expect(
        within(table).getByRole("columnheader", { name: labels.successRate }),
      ).toBeInTheDocument();
      expect(
        within(table).getByRole("columnheader", { name: labels.avgLatency }),
      ).toBeInTheDocument();
    },
  );
});
