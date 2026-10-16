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
    [undefined, "nav.today:$47.20"],
    ["tokens", "nav.today:120M"],
  ])("shows %s", (mode, label) => {
    status.todayCost = 47.2;
    status.todayTokens = 120_400_000;
    renderSidebar({ usageDisplay: { sidebarToday: mode } });
    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it("stacks only the numbers when showing both", async () => {
    // 带上「今日」一行放不下长币种，会挤掉「用量统计」；完整说法在读屏名字和悬停里
    status.todayCost = 47.2;
    status.todayTokens = 120_400_000;
    renderSidebar({ usageDisplay: { sidebarToday: "both" } });
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
    renderSidebar({ usageDisplay: { sidebarToday: "hidden" } });
    expect(screen.queryByText(/^nav\.today/)).not.toBeInTheDocument();
  });

  it("skips the parts that are zero", () => {
    // 没配定价时金额是 0：两者模式只剩 Token，金额模式整个不显示
    status.todayCost = 0;
    status.todayTokens = 5_000;
    renderSidebar({ usageDisplay: { sidebarToday: "both" } });
    expect(screen.getByText("nav.today:5,000")).toBeInTheDocument();
  });

  it("hides the cost-only label while nothing is priced", () => {
    status.todayCost = 0;
    status.todayTokens = 5_000;
    renderSidebar({});
    expect(screen.queryByText(/^nav\.today/)).not.toBeInTheDocument();
  });
});
