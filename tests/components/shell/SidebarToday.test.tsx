import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import type { Settings, VisibleApps } from "@/types";
import { createTestQueryClient } from "../../utils/testQueryClient";

const status = vi.hoisted(() => ({
  todayCost: 47.2,
  todayTokens: 120_400_000,
  todayRequests: 663,
  todayHitRate: 0.986,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { value?: string }) =>
      options?.value != null ? `${key}:${options.value}` : key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));

vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: status.todayCost,
    todayTokens: status.todayTokens,
    todayRequests: status.todayRequests,
    todayHitRate: status.todayHitRate,
    authNeedsAttention: false,
  }),
}));

const renderSidebar = (settings: Partial<Settings>) => {
  const client = createTestQueryClient();
  // 不让设置查询回头去 mock 后端重取，盖掉这里给的值
  client.setQueryDefaults(["settings"], { staleTime: Infinity });
  client.setQueryData(["settings"], settings);
  render(
    <QueryClientProvider client={client}>
      <Sidebar
        activeApp="claude"
        view="usage"
        visibleApps={{} as VisibleApps}
        settingsSection="general"
        onSelectApp={vi.fn()}
        onSelectPage={vi.fn()}
        onSelectSettingsSection={vi.fn()}
        onExitSettings={vi.fn()}
      />
    </QueryClientProvider>,
  );
};

describe("sidebar Today label", () => {
  it.each([
    [undefined, "$47.20"],
    [["tokens"], "120M"],
    [["requests"], "nav.todayRequests:663"],
    [["hitRate"], "nav.todayHitRate:98.6%"],
  ])("shows %s", (items, value) => {
    // 行尾只写数，「今日」在读屏名字和悬停里
    status.todayCost = 47.2;
    status.todayTokens = 120_400_000;
    status.todayRequests = 663;
    renderSidebar({ usageDisplay: { sidebarTodayItems: items } });
    expect(screen.getByText(value)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `nav.usage · nav.today:${value}` }),
    ).toBeInTheDocument();
  });

  it("stacks only the numbers when showing two items", async () => {
    // 带上「今日」一行放不下长币种，会挤掉「用量统计」；完整说法在读屏名字和悬停里
    status.todayCost = 47.2;
    status.todayTokens = 120_400_000;
    renderSidebar({ usageDisplay: { sidebarTodayItems: ["cost", "tokens"] } });
    expect(screen.getByText("$47.20")).toBeInTheDocument();
    expect(screen.getByText("120M")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "nav.usage · nav.today:$47.20 · 120M",
      }),
    ).toBeInTheDocument();

    // 「今日」只在悬停提示里
    await userEvent.hover(screen.getByText("$47.20"));
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "nav.usage · nav.today:$47.20 · 120M",
    );
  });

  it("shows nothing when hidden", () => {
    renderSidebar({ usageDisplay: { sidebarTodayItems: [] } });
    expect(
      screen.getByRole("button", { name: "nav.usage" }),
    ).toBeInTheDocument();
  });

  it("skips the parts that are zero", () => {
    // 没配定价时金额是 0：金额 + Token 只剩 Token，只勾金额就整个不显示
    status.todayCost = 0;
    status.todayTokens = 5_000;
    renderSidebar({ usageDisplay: { sidebarTodayItems: ["cost", "tokens"] } });
    expect(screen.getByText("5,000")).toBeInTheDocument();
    expect(screen.queryByText("$0.00")).not.toBeInTheDocument();
  });

  it("leaves out the hit rate before the first request of the day", () => {
    status.todayRequests = 0;
    status.todayTokens = 0;
    status.todayCost = 0;
    renderSidebar({ usageDisplay: { sidebarTodayItems: ["hitRate"] } });
    expect(
      screen.getByRole("button", { name: "nav.usage" }),
    ).toBeInTheDocument();
  });

  it("hides the cost-only label while nothing is priced", () => {
    status.todayCost = 0;
    status.todayTokens = 5_000;
    renderSidebar({});
    expect(
      screen.getByRole("button", { name: "nav.usage" }),
    ).toBeInTheDocument();
  });
});
