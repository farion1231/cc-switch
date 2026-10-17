import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { emitTauriEvent } from "../msw/tauriMocks";

/**
 * 不 mock 子组件，把整页（指标、趋势图、四个页签）真渲染一遍，接口用假数据。
 */

const usageApiMock = vi.hoisted(() => ({
  getUsageSummary: vi.fn(),
  getUsageSummaryByApp: vi.fn(),
  getUsageTrends: vi.fn(),
  getProviderStats: vi.fn(),
  getModelStats: vi.fn(),
  getRequestLogs: vi.fn(),
  getRequestDetail: vi.fn(),
  getModelPricing: vi.fn(),
  getModelsDevSyncConfig: vi.fn(),
  saveModelsDevSyncConfig: vi.fn(),
  syncSessionUsage: vi.fn(),
  getSessionUsageLastSync: vi.fn().mockResolvedValue(null),
  rebuildCodexUsage: vi.fn(),
}));

// t 要稳定：真实的 i18next 也只在切语言时换 t，组件里有依赖 t 的 effect
const i18nMock = vi.hoisted(() => ({
  t: (key: string, options?: unknown) =>
    options && typeof options === "object" && "count" in options
      ? `${key}:${(options as { count: unknown }).count}`
      : key,
  i18n: { resolvedLanguage: "zh", language: "zh" },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => i18nMock,
}));

vi.mock("@/lib/api/usage", () => ({ usageApi: usageApiMock }));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getPricingModelSource: vi.fn().mockResolvedValue("response"),
    setPricingModelSource: vi.fn().mockResolvedValue(undefined),
  },
}));

const summary = {
  totalRequests: 3120,
  totalCost: "42.100000",
  totalInputTokens: 1_000_000,
  totalOutputTokens: 400_000,
  totalCacheCreationTokens: 200_000,
  totalCacheReadTokens: 16_600_000,
  successRate: 99.5,
  realTotalTokens: 18_200_000,
  cacheHitRate: 0.7161,
};

describe("UsageDashboard (smoke)", () => {
  beforeEach(() => {
    usageApiMock.syncSessionUsage.mockReset().mockResolvedValue({
      imported: 0,
      filesScanned: 0,
      errors: [],
    });
    usageApiMock.getUsageSummary.mockResolvedValue(summary);
    usageApiMock.getUsageSummaryByApp.mockResolvedValue([
      { appType: "claude", summary },
    ]);
    usageApiMock.getUsageTrends.mockResolvedValue([
      {
        date: "2026-09-30T00:00:00+08:00",
        requestCount: 290,
        totalCost: "4.1",
        totalTokens: 1_800_000,
        totalInputTokens: 100_000,
        totalOutputTokens: 50_000,
        totalCacheCreationTokens: 10_000,
        totalCacheReadTokens: 1_640_000,
      },
    ]);
    usageApiMock.getProviderStats.mockResolvedValue([
      {
        providerId: "p1",
        appType: "claude",
        providerName: "DeepSeek",
        requestCount: 720,
        totalTokens: 4_300_000,
        totalCost: "6.1",
        successRate: 99.4,
        avgLatencyMs: 9000,
        speedOutputTokens: 92_000,
        speedGenerationMs: 1_000_000,
      },
    ]);
    usageApiMock.getModelStats.mockResolvedValue([
      {
        model: "kimi-k2.6",
        requestCount: 980,
        totalTokens: 5_700_000,
        totalCost: "11.2",
        avgCostPerRequest: "0.0114",
        successRate: 100,
      },
    ]);
    usageApiMock.getRequestLogs.mockResolvedValue({
      data: [],
      total: 0,
      page: 0,
      pageSize: 20,
    });
    usageApiMock.getModelPricing.mockResolvedValue([
      {
        modelId: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        inputCostPerMillion: "1.32",
        outputCostPerMillion: "3.96",
        cacheReadCostPerMillion: "0.044",
        cacheCreationCostPerMillion: "0",
      },
    ]);
    usageApiMock.getModelsDevSyncConfig.mockResolvedValue({
      configPath: "/tmp/model-pricing.json",
      config: {
        autoSyncEnabled: true,
        includeCommonModels: true,
        selectedModelKeys: [],
        excludedCommonModelKeys: [],
        lastSyncAt: Date.now() - 2 * 60 * 60 * 1000,
        lastSyncError: null,
      },
    });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("scans session logs on open and shows imported usage via the backend event", async () => {
    vi.useFakeTimers();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    usageApiMock.syncSessionUsage.mockImplementation(async () => {
      usageApiMock.getUsageSummaryByApp.mockResolvedValue([
        { appType: "claude", summary: { ...summary, totalCost: "43.1" } },
      ]);
      // 真实后端扫完、防抖 200ms 后才发这个事件，那时监听早已挂上
      setTimeout(() => emitTauriEvent("usage-log-recorded", null), 200);
      return { imported: 1, filesScanned: 1, errors: [] };
    });
    render(
      <QueryClientProvider client={client}>
        <UsageDashboard refreshIntervalMs={5000} />
      </QueryClientProvider>,
    );
    await act(() => vi.advanceTimersByTimeAsync(10));
    expect(usageApiMock.syncSessionUsage).toHaveBeenCalledTimes(1);
    expect(screen.getByText("$42.10")).toBeInTheDocument();
    await act(() => vi.advanceTimersByTimeAsync(200));
    expect(screen.getByText("$43.10")).toBeInTheDocument();
    client.clear();
  });

  it("scans at the interval and re-runs statistics only once a minute for rolling ranges", async () => {
    vi.useFakeTimers();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    usageApiMock.syncSessionUsage.mockResolvedValue({
      imported: 0,
      filesScanned: 1,
      errors: [],
    });
    render(
      <QueryClientProvider client={client}>
        <UsageDashboard refreshIntervalMs={5000} />
      </QueryClientProvider>,
    );
    await act(() => vi.advanceTimersByTimeAsync(10));
    const queries = [
      usageApiMock.getUsageSummary,
      usageApiMock.getUsageSummaryByApp,
      usageApiMock.getUsageTrends,
      usageApiMock.getProviderStats,
      usageApiMock.getModelStats,
      usageApiMock.getRequestLogs,
    ];
    const counts = queries.map((query) => query.mock.calls.length);
    expect(counts.every((count) => count > 0)).toBe(true);
    // 没有新用量：扫描不触发重查，只有「今天」这类范围每分钟重算一次
    await act(() => vi.advanceTimersByTimeAsync(59000));
    expect(queries.map((query) => query.mock.calls.length)).toEqual(counts);
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(queries.map((query) => query.mock.calls.length)).toEqual(
      counts.map((count) => count + 1),
    );
    expect(usageApiMock.syncSessionUsage).toHaveBeenCalledTimes(13);
    client.clear();
  });

  it("hides the scan interval and never scans while auto scanning is off", async () => {
    vi.useFakeTimers();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <UsageDashboard
          refreshIntervalMs={5000}
          sessionAutoSyncEnabled={false}
        />
      </QueryClientProvider>,
    );
    await act(() => vi.advanceTimersByTimeAsync(60000));
    expect(usageApiMock.syncSessionUsage).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: /usage\.refreshInterval/ }),
    ).not.toBeInTheDocument();
    client.clear();
  });

  it("renders metrics, trend, and every tab with real children", async () => {
    const user = userEvent.setup();
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <UsageDashboard />
      </QueryClientProvider>,
    );

    // 指标卡
    expect(await screen.findByText("$42.10")).toBeInTheDocument();
    expect(screen.getByText("18.2M")).toBeInTheDocument();
    // 「全部」时由各应用的 token 重新算：16.6M ÷ (1M + 0.2M + 16.6M)
    expect(screen.getByText("93.3%")).toBeInTheDocument();
    expect(screen.getByText("usage.trend.title")).toBeInTheDocument();

    // 更多指标
    await user.click(
      screen.getByRole("button", { name: "usage.metrics.more" }),
    );
    expect(
      screen.getByRole("button", { name: "usage.metrics.more" }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("usage.cacheWrite")).toBeInTheDocument();

    // 供应商：汇总速度 92000 / 1000 s = 92 tok/s
    await user.click(screen.getByRole("tab", { name: "usage.tabs.providers" }));
    const providerRow = (await screen.findByText("DeepSeek")).closest("tr")!;
    expect(within(providerRow).getByText("92")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "usage.tabs.models" }));
    expect(await screen.findByText("kimi-k2.6")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "usage.tabs.settings" }));
    expect(await screen.findByText("DeepSeek V4 Pro")).toBeInTheDocument();
    expect(screen.getByText("$1.32")).toBeInTheDocument();
    expect(screen.getByText("$0.044")).toBeInTheDocument();
    expect(
      await screen.findByText("usage.pricing.modelsDevTitle"),
    ).toBeInTheDocument();
  });
});
