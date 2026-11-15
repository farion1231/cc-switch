import type { ComponentProps, ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UsageTab } from "@/tray-panel/UsageTab";
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
    getUsageSummary: vi.fn().mockResolvedValue({
      totalRequests: 3,
      totalCost: "1.5",
      totalInputTokens: 100,
      totalOutputTokens: 50,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
      successRate: 1,
      realTotalTokens: 150,
      cacheHitRate: 0,
    }),
    getUsageTrends: vi.fn().mockImplementation(async (start: number) => [
      {
        date: new Date((start + 60) * 1000).toISOString(),
        requestCount: 3,
        totalCost: "1.5",
        totalTokens: 150,
        totalInputTokens: 100,
        totalOutputTokens: 50,
        totalCacheCreationTokens: 0,
        totalCacheReadTokens: 0,
      },
    ]),
  },
}));

function renderTab(settings: Partial<Settings>) {
  const client = createTestQueryClient();
  // 不让挂载时的重新拉取用 mock 后端的默认设置覆盖掉这里给的值
  client.setDefaultOptions({ queries: { retry: false, staleTime: Infinity } });
  client.setQueryData(["settings"], settings);
  return render(<UsageTab refreshKey={1} visible />, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}

describe("tray panel UsageTab", () => {
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
});
