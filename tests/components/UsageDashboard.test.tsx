import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  UsageDashboard,
  resetUsageSyncClockForTests,
} from "@/components/usage/UsageDashboard";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
const useSummaryByAppMock = vi.hoisted(() => vi.fn());
const usageHeroMock = vi.hoisted(() => vi.fn());
const requestLogTableMock = vi.hoisted(() => vi.fn());
const detailPanelMock = vi.hoisted(() => vi.fn());
const usageApiMock = vi.hoisted(() => ({
  getUsageSummary: vi.fn(),
  syncSessionUsage: vi.fn(),
  getSessionUsageLastSync: vi.fn(),
  rebuildCodexUsage: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: unknown) => {
      if (typeof options === "string") return options;
      if (options && typeof options === "object") {
        const values = Object.entries(options as Record<string, unknown>)
          .filter(([name]) => name !== "defaultValue")
          .map(([, value]) => String(value));
        return values.length ? `${key}:${values.join(",")}` : key;
      }
      return key;
    },
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));

vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));

vi.mock("@/lib/api/usage", () => ({ usageApi: usageApiMock }));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
    useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
    useUsageSummaryByApp: (...args: unknown[]) => useSummaryByAppMock(...args),
  };
});

vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: (props: unknown) => {
    usageHeroMock(props);
    return <div data-testid="usage-hero" />;
  },
}));

vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: () => <div data-testid="usage-trend" />,
}));

vi.mock("@/components/usage/UsageHeatmap", () => ({
  UsageHeatmap: () => <div data-testid="usage-heatmap" />,
}));

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: (props: { onOpenDetail?: (id: string) => void }) => {
    requestLogTableMock(props);
    return (
      <button type="button" onClick={() => props.onOpenDetail?.("req-1")}>
        open-row
      </button>
    );
  },
}));

vi.mock("@/components/usage/RequestDetailPanel", () => ({
  RequestDetailPanel: (props: { requestId: string | null }) => {
    detailPanelMock(props);
    return props.requestId ? (
      <div data-testid="request-detail">{props.requestId}</div>
    ) : null;
  },
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: ({
    triggerLabel,
    onApply,
  }: {
    triggerLabel: string;
    onApply: (selection: { preset: "all" }) => void;
  }) => (
    <>
      <button type="button">{triggerLabel}</button>
      <button type="button" onClick={() => onApply({ preset: "all" })}>
        pick-all
      </button>
    </>
  ),
}));

vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({
    data: { enableLogging: true },
    isLoading: false,
  }),
}));

const renderDashboard = (props: ComponentProps<typeof UsageDashboard> = {}) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageDashboard {...props} />
    </QueryClientProvider>,
  );
};

describe("UsageDashboard", () => {
  beforeEach(() => {
    resetUsageSyncClockForTests();
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    useSummaryByAppMock.mockReset();
    usageHeroMock.mockReset();
    requestLogTableMock.mockReset();
    detailPanelMock.mockReset();
    useProviderStatsMock.mockReturnValue({
      data: [
        {
          providerId: "p1",
          appType: "claude",
          providerName: "DeepSeek",
          requestCount: 12,
          totalTokens: 100,
          totalCost: "1",
          successRate: 100,
          avgLatencyMs: 0,
        },
      ],
    });
    useModelStatsMock.mockReturnValue({ data: [] });
    useSummaryByAppMock.mockReturnValue({ data: [] });
    usageApiMock.getUsageSummary.mockResolvedValue({ totalRequests: 5 });
    usageApiMock.getSessionUsageLastSync.mockReset().mockResolvedValue(null);
    usageApiMock.syncSessionUsage.mockResolvedValue({
      imported: 2,
      skipped: 0,
      filesScanned: 3,
      suspectedDuplicates: 0,
      deferredFiles: 0,
      errors: [],
    });
  });

  it("swaps the trend chart for the heatmap on the all-time range", async () => {
    const user = userEvent.setup();
    renderDashboard();

    expect(await screen.findByTestId("usage-trend")).toBeInTheDocument();
    expect(screen.queryByTestId("usage-heatmap")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "pick-all" }));

    expect(await screen.findByTestId("usage-heatmap")).toBeInTheDocument();
    expect(screen.queryByTestId("usage-trend")).not.toBeInTheDocument();
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ range: { preset: "all" } }),
    );
  });

  it("shows the saved refresh interval in the header menu", () => {
    renderDashboard({ refreshIntervalMs: 5000 });

    expect(
      screen.getByRole("button", {
        name: "usage.refreshInterval: usage.refreshMenu.label:5",
      }),
    ).toBeInTheDocument();
  });

  it("defaults to today", () => {
    renderDashboard();

    expect(
      screen.getByRole("button", { name: "usage.presetToday" }),
    ).toBeInTheDocument();
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ range: { preset: "today" } }),
    );
  });

  it("filters usage queries to an app from its icon chip", async () => {
    const user = userEvent.setup();
    renderDashboard();

    await user.click(screen.getByRole("button", { name: "Pi" }));

    await waitFor(() =>
      expect(useProviderStatsMock).toHaveBeenLastCalledWith(
        expect.anything(),
        { appType: "pi" },
        expect.anything(),
      ),
    );
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      expect.anything(),
      { appType: "pi", providerName: undefined },
      expect.anything(),
    );
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ appType: "pi" }),
    );
    // 芯片只有图标，名字由 aria-label 提供
    expect(
      screen.getByRole("button", { name: "Pi", pressed: true }),
    ).toBeInTheDocument();
  });

  it("starts with the app filter passed in from an app page", () => {
    renderDashboard({ initialAppType: "codex" });

    expect(
      screen.getByRole("button", { name: /Codex/, pressed: true }),
    ).toBeInTheDocument();
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ appType: "codex" }),
    );
  });

  it("filters by provider and lists request counts", async () => {
    const user = userEvent.setup();
    renderDashboard();

    await user.click(
      screen.getByRole("button", { name: "usage.providerFilter.label" }),
    );
    const item = await screen.findByRole("menuitem", { name: /DeepSeek/ });
    expect(item).toHaveTextContent("12");
    await user.click(item);

    await waitFor(() =>
      expect(usageHeroMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ providerName: "DeepSeek" }),
      ),
    );
  });

  it("persists refresh interval changes", async () => {
    const user = userEvent.setup();
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(true);
    renderDashboard({ onRefreshIntervalChange });

    await user.click(
      screen.getByRole("button", {
        name: "usage.refreshInterval: usage.refreshMenu.label:30",
      }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "usage.refreshMenu.seconds:5",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    expect(
      screen.getByRole("button", {
        name: "usage.refreshInterval: usage.refreshMenu.label:5",
      }),
    ).toBeInTheDocument();
  });

  it("rolls back optimistic interval changes when persistence fails", async () => {
    const user = userEvent.setup();
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(false);
    renderDashboard({ onRefreshIntervalChange });

    await user.click(
      screen.getByRole("button", {
        name: "usage.refreshInterval: usage.refreshMenu.label:30",
      }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "usage.refreshMenu.seconds:5",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "usage.refreshInterval: usage.refreshMenu.label:30",
        }),
      ).toBeInTheDocument(),
    );
  });

  it("syncs session logs from the header and shows when it last synced", async () => {
    const user = userEvent.setup();
    renderDashboard();

    expect(screen.getByRole("status")).toHaveTextContent(
      "usage.syncStatus.auto",
    );
    await user.click(
      screen.getByRole("button", { name: "usage.sessionSync.syncNow" }),
    );

    await waitFor(() =>
      expect(usageApiMock.syncSessionUsage).toHaveBeenCalledTimes(1),
    );
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "usage.syncStatus.justNow",
      ),
    );
  });

  it("shows when the background scan last finished", async () => {
    usageApiMock.getSessionUsageLastSync.mockResolvedValue(
      Date.now() - 5 * 60_000 - 1_000,
    );
    renderDashboard();

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "usage.syncStatus.minutesAgo:5",
      ),
    );
  });

  it("prefers a manual sync that is newer than the last background scan", async () => {
    const user = userEvent.setup();
    usageApiMock.getSessionUsageLastSync.mockResolvedValue(
      Date.now() - 30 * 60_000,
    );
    renderDashboard();
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "usage.syncStatus.minutesAgo:30",
      ),
    );

    await user.click(
      screen.getByRole("button", { name: "usage.sessionSync.syncNow" }),
    );

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "usage.syncStatus.justNow",
      ),
    );
  });

  it("says when automatic scanning is off", () => {
    renderDashboard({ sessionAutoSyncEnabled: false });

    expect(screen.getByRole("status")).toHaveTextContent(
      "usage.syncStatus.off",
    );
  });

  it("opens the request detail drawer from a log row", async () => {
    const user = userEvent.setup();
    renderDashboard();

    await user.click(screen.getByRole("button", { name: "open-row" }));

    expect(await screen.findByTestId("request-detail")).toHaveTextContent(
      "req-1",
    );
  });

  it("switches tabs and filters logs by status code", async () => {
    const user = userEvent.setup();
    renderDashboard();

    await user.click(
      screen.getByRole("button", {
        name: "usage.statusCode: usage.statusFilter.all",
      }),
    );
    await user.click(await screen.findByRole("menuitem", { name: "429" }));
    await waitFor(() =>
      expect(requestLogTableMock).toHaveBeenLastCalledWith(
        expect.objectContaining({ statusCode: 429 }),
      ),
    );

    await user.click(screen.getByRole("tab", { name: "usage.tabs.pricing" }));
    expect(screen.getByTestId("pricing-config-panel")).toBeInTheDocument();
    expect(
      screen.getByRole("tab", { name: "usage.tabs.pricing" }),
    ).toHaveAttribute("aria-selected", "true");
  });

  it("wires the tabs to the tabpanel and supports arrow / Home / End keys", async () => {
    const user = userEvent.setup();
    renderDashboard();

    const logsTab = screen.getByRole("tab", { name: "usage.requestLogs" });
    expect(logsTab).toHaveAttribute("id", "usage-tab-logs");
    expect(logsTab).toHaveAttribute("aria-controls", "usage-tabpanel");
    expect(screen.getByRole("tabpanel")).toHaveAttribute(
      "aria-labelledby",
      "usage-tab-logs",
    );

    logsTab.focus();
    await user.keyboard("{ArrowRight}");
    const providersTab = screen.getByRole("tab", {
      name: "usage.tabs.providers",
    });
    expect(providersTab).toHaveAttribute("aria-selected", "true");
    expect(providersTab).toHaveFocus();
    expect(screen.getByRole("tabpanel")).toHaveAttribute(
      "aria-labelledby",
      "usage-tab-providers",
    );

    await user.keyboard("{End}");
    expect(
      screen.getByRole("tab", { name: "usage.tabs.pricing" }),
    ).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(logsTab).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowLeft}");
    expect(
      screen.getByRole("tab", { name: "usage.tabs.pricing" }),
    ).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Home}");
    expect(logsTab).toHaveFocus();
  });

  it("toggles session scanning and links to routing settings from the data sources drawer", async () => {
    const user = userEvent.setup();
    const onSessionAutoSyncEnabledChange = vi.fn();
    const onOpenRoutingSettings = vi.fn();
    renderDashboard({ onSessionAutoSyncEnabledChange, onOpenRoutingSettings });

    await user.click(screen.getByRole("button", { name: "usage.dataSources" }));
    await user.click(
      await screen.findByRole("switch", { name: "usage.sources.scanTitle" }),
    );
    expect(onSessionAutoSyncEnabledChange).toHaveBeenCalledWith(false);

    await user.click(
      screen.getByRole("button", { name: /usage.sources.editLogging/ }),
    );
    expect(onOpenRoutingSettings).toHaveBeenCalledTimes(1);
  });

  it("shows the empty state when there is no usage at all", async () => {
    usageApiMock.getUsageSummary.mockResolvedValue({ totalRequests: 0 });
    renderDashboard();

    expect(await screen.findByText("usage.empty.title")).toBeInTheDocument();
    expect(screen.queryByTestId("usage-hero")).not.toBeInTheDocument();
  });

  it("still lets the pricing tab be opened from the empty state", async () => {
    usageApiMock.getUsageSummary.mockResolvedValue({ totalRequests: 0 });
    const user = userEvent.setup();
    renderDashboard();

    await screen.findByText("usage.empty.title");
    expect(
      screen.queryByTestId("pricing-config-panel"),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "usage.empty.configurePricing" }),
    );
    expect(
      await screen.findByTestId("pricing-config-panel"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("tab", { name: "usage.tabs.pricing" }),
    ).toHaveAttribute("aria-selected", "true");
    // 空库里没有可看的概览，不画全是 0 的卡
    expect(screen.queryByTestId("usage-hero")).not.toBeInTheDocument();
  });
});
