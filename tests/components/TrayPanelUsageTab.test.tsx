import type { ComponentProps, ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageTab } from "@/tray-panel/UsageTab";
import { usageApi } from "@/lib/api";
import type { Settings } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("recharts", async (importOriginal) => {
  const recharts = await importOriginal<typeof import("recharts")>();
  return {
    ...recharts,
    // jsdom 不排版：给图表固定尺寸，才会真的画出柱子 / 面积
    ResponsiveContainer: (
      props: ComponentProps<typeof recharts.ResponsiveContainer>,
    ) => <recharts.ResponsiveContainer {...props} width={300} height={128} />,
  };
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  usageApi: {
    getUsageFirstDate: vi.fn().mockResolvedValue(null),
    getUsageSummary: vi.fn(),
    getUsageTrends: vi.fn(),
    getUsageSummaryByApp: vi.fn(),
    getProviderStats: vi.fn(),
    getModelStats: vi.fn(),
  },
}));

function seedDefaultMocks() {
  vi.mocked(usageApi.getUsageSummary).mockResolvedValue({
    totalRequests: 3,
    totalCost: "1.5",
    totalInputTokens: 100,
    totalOutputTokens: 50,
    totalCacheCreationTokens: 20,
    totalCacheReadTokens: 80,
    successRate: 100,
    realTotalTokens: 250,
    cacheHitRate: 0.4,
  });
  vi.mocked(usageApi.getUsageTrends).mockImplementation(
    async (start: number = 0) => [
      {
        date: new Date((start + 60) * 1000).toISOString(),
        requestCount: 1,
        totalCost: "0.5",
        totalTokens: 100,
        totalInputTokens: 40,
        totalOutputTokens: 20,
        totalCacheCreationTokens: 10,
        totalCacheReadTokens: 30,
      },
      {
        date: new Date((start + 3660) * 1000).toISOString(),
        requestCount: 2,
        totalCost: "1.0",
        totalTokens: 150,
        totalInputTokens: 60,
        totalOutputTokens: 30,
        totalCacheCreationTokens: 10,
        totalCacheReadTokens: 50,
      },
    ],
  );
  vi.mocked(usageApi.getUsageSummaryByApp).mockResolvedValue([
    {
      appType: "claude",
      summary: {
        totalRequests: 2,
        totalCost: "1.1",
        totalInputTokens: 70,
        totalOutputTokens: 30,
        totalCacheCreationTokens: 20,
        totalCacheReadTokens: 60,
        successRate: 100,
        realTotalTokens: 180,
        cacheHitRate: 0.4,
      },
    },
    {
      appType: "codex",
      summary: {
        totalRequests: 1,
        totalCost: "0.4",
        totalInputTokens: 30,
        totalOutputTokens: 20,
        totalCacheCreationTokens: 0,
        totalCacheReadTokens: 20,
        successRate: 100,
        realTotalTokens: 70,
        cacheHitRate: 0.4,
      },
    },
  ]);
  vi.mocked(usageApi.getProviderStats).mockResolvedValue([
    {
      providerId: "p1",
      appType: "claude",
      providerName: "Anthropic Official",
      requestCount: 2,
      totalTokens: 180,
      totalCost: "1.1",
      successRate: 100,
      avgLatencyMs: 500,
    },
    {
      providerId: "p2",
      appType: "codex",
      providerName: "OpenAI Direct",
      requestCount: 1,
      totalTokens: 70,
      totalCost: "0.4",
      successRate: 100,
      avgLatencyMs: 420,
    },
  ]);
  vi.mocked(usageApi.getModelStats).mockResolvedValue([
    {
      model: "claude-sonnet-4",
      requestCount: 2,
      totalTokens: 180,
      totalCost: "1.1",
      avgCostPerRequest: "0.55",
      successRate: 100,
    },
    {
      model: "gpt-5",
      requestCount: 1,
      totalTokens: 70,
      totalCost: "0.4",
      avgCostPerRequest: "0.4",
      successRate: 100,
    },
  ]);
}

function renderTab(settings: Partial<Settings>, platform?: string) {
  const client = createTestQueryClient();
  // 不让挂载时的重新拉取用 mock 后端的默认设置覆盖掉这里给的值
  client.setDefaultOptions({ queries: { retry: false, staleTime: Infinity } });
  client.setQueryData(["settings"], settings);
  return render(<UsageTab refreshKey={1} visible platform={platform} />, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

describe("tray panel UsageTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    seedDefaultMocks();
  });

  it("draws bars when no chart style is saved", async () => {
    const { container } = renderTab({});
    await screen.findByText("usage.totalCost");
    expect(container.querySelector(".recharts-bar-rectangle")).not.toBeNull();
    expect(container.querySelector(".recharts-area")).toBeNull();
  });

  it("follows the area style picked in the main window's usage settings", async () => {
    const { container } = renderTab({
      usageDisplay: { trendChartType: "area" },
    });
    await screen.findByText("usage.totalCost");
    expect(container.querySelector(".recharts-area")).not.toBeNull();
    expect(container.querySelector(".recharts-bar-rectangle")).toBeNull();
  });

  it("shows amounts in the currency picked in the main window", async () => {
    renderTab({
      usageDisplay: { currency: "CNY", exchangeRates: { CNY: 7 } },
    });
    expect(await screen.findByText("¥10.50")).toBeInTheDocument();
  });

  it("combines numbers with inline statistical graphics in the 4 KPI cards", async () => {
    const { container } = renderTab({});
    await screen.findByText("$1.50");
    expect(screen.getByText("40.0%")).toBeInTheDocument();
    // 环形进度条 + 迷你折线火花图（至少包含 SVG 圆环与路径）
    expect(container.querySelectorAll("svg circle").length).toBeGreaterThan(0);
    expect(container.querySelectorAll("svg path").length).toBeGreaterThan(0);
  });

  it("links card selection with the trend chart metric", async () => {
    renderTab({});
    const costLabel = await screen.findByText("usage.totalCost");
    fireEvent.click(costLabel);
    expect(window.localStorage.getItem("cc-switch-tray-usage-metric")).toBe(
      "cost",
    );
  });

  it("filters by app chip and provider/model filter popover", async () => {
    renderTab({});
    const claudeChip = await screen.findByRole("button", {
      name: "Claude Code",
    });
    fireEvent.click(claudeChip);

    await waitFor(() => {
      expect(usageApi.getUsageSummary).toHaveBeenCalledWith(
        expect.any(Number),
        expect.any(Number),
        "claude",
        undefined,
        undefined,
      );
    });

    const filterBtn = await screen.findByRole("button", {
      name: "usage.tray.filterBy",
    });
    fireEvent.click(filterBtn);

    const providerBtn = await screen.findByRole("button", {
      name: "Anthropic Official",
    });
    fireEvent.click(providerBtn);

    await waitFor(() => {
      expect(usageApi.getUsageSummary).toHaveBeenCalledWith(
        expect.any(Number),
        expect.any(Number),
        "claude",
        "Anthropic Official",
        undefined,
      );
    });
  });

  it("drills down into a single card, switches breakdown dimensions, cross-filters, and exits on Escape", async () => {
    renderTab({});
    const expandBtn = await screen.findByRole("button", {
      name: "usage.tray.viewDetails: usage.totalCost",
    });
    fireEvent.click(expandBtn);

    expect(
      await screen.findByRole("button", { name: "usage.tray.backToOverview" }),
    ).toBeInTheDocument();
    expect(
      (await screen.findAllByText("Claude Code")).length,
    ).toBeGreaterThanOrEqual(2);

    // 切换到按供应商细分，并点击供应商实现跨维度筛选联动
    fireEvent.click(screen.getByRole("tab", { name: "usage.tray.byProvider" }));
    const providerRow = await screen.findByRole("button", {
      name: /Anthropic Official/,
    });
    fireEvent.click(providerRow);

    await waitFor(() => {
      expect(usageApi.getUsageSummary).toHaveBeenCalledWith(
        expect.any(Number),
        expect.any(Number),
        undefined,
        "Anthropic Official",
        undefined,
      );
    });

    // 按 Esc 退出单卡下钻回到四格概览
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "usage.tray.backToOverview" }),
      ).toBeNull();
    });
  });

  it("pins a time bucket on chart bar click and updates KPI card values", async () => {
    const { container } = renderTab({});
    await screen.findByText("$1.50");

    const bars = container.querySelectorAll(".recharts-bar-rectangle");
    expect(bars.length).toBeGreaterThan(0);
    fireEvent.click(bars[0]);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /usage\.tray\.selectedBucket/ }),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("$0.50")).toBeInTheDocument();

    // 点击已选时段标签清除联动
    fireEvent.click(
      screen.getByRole("button", { name: /usage\.tray\.selectedBucket/ }),
    );
    await waitFor(() => {
      expect(screen.getByText("$1.50")).toBeInTheDocument();
    });
  });

  it("scrolls the app filter strip horizontally on vertical mouse wheel and arrow button click", async () => {
    renderTab({});
    const group = await screen.findByRole("group", {
      name: "usage.appFilter.label",
    });

    Object.defineProperty(group, "clientWidth", {
      configurable: true,
      value: 180,
    });
    Object.defineProperty(group, "scrollWidth", {
      configurable: true,
      value: 420,
    });
    // 触发 ResizeObserver / scroll 同步溢出箭头状态
    fireEvent.scroll(group);

    const rightBtn = await screen.findByRole("button", {
      name: "Scroll apps right",
    });
    fireEvent.wheel(group, { deltaY: 60 });
    expect(group.scrollLeft).toBe(60);

    fireEvent.click(rightBtn);
    expect(group.scrollLeft).toBeGreaterThan(60);
  });

  it("keeps the classic pre-change UsageTab style on Linux and enables enhanced style on macOS", async () => {
    const linuxView = renderTab({}, "linux");
    await screen.findByText("$1.50");
    expect(
      screen.queryByRole("group", { name: "usage.appFilter.label" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "usage.tray.viewDetails: usage.totalCost",
      }),
    ).toBeNull();
    expect(
      screen.queryByRole("tab", { name: "usage.tray.hitRateShort" }),
    ).toBeNull();
    linuxView.unmount();

    renderTab({}, "macos");
    await screen.findByText("$1.50");
    expect(
      await screen.findByRole("group", { name: "usage.appFilter.label" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "usage.tray.viewDetails: usage.totalCost",
      }),
    ).toBeInTheDocument();
  });
});
