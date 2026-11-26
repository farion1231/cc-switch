import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import {
  SIDEBAR_EXPANDED_WIDTH,
  SIDEBAR_RAIL_WIDTH,
  sidebarRailWidth,
  sidebarWidth,
} from "@/hooks/useSidebarCollapsed";
import { setDensity } from "@/lib/interfacePrefs";
import type { VisibleApps } from "@/types";
import { createTestQueryClient } from "../../utils/testQueryClient";

const platform = vi.hoisted(() => ({ mac: false }));

vi.mock("@/lib/platform", () => ({
  isMac: () => platform.mac,
  DRAG_REGION_ATTR: {},
  DRAG_REGION_STYLE: {},
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));

vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: 0,
    todayTokens: 0,
    todayRequests: 0,
    todayHitRate: 0,
    authNeedsAttention: false,
  }),
}));

const renderSidebar = () => {
  const client = createTestQueryClient();
  client.setQueryDefaults(["settings"], { staleTime: Infinity });
  client.setQueryData(["settings"], {});
  return render(
    <QueryClientProvider client={client}>
      <Sidebar
        activeApp="claude"
        view="providers"
        visibleApps={{ claude: true } as VisibleApps}
        settingsSection="general"
        onSelectApp={vi.fn()}
        onSelectPage={vi.fn()}
        onSelectSettingsSection={vi.fn()}
        onExitSettings={vi.fn()}
      />
    </QueryClientProvider>,
  );
};

describe("collapsed sidebar width across densities", () => {
  beforeEach(() => {
    platform.mac = false;
    localStorage.setItem("cc-switch-sidebar-collapsed", "true");
    setDensity("compact");
  });

  afterEach(() => {
    localStorage.removeItem("cc-switch-sidebar-collapsed");
    setDensity("compact");
  });

  it("uses narrower rail widths than legacy 72px/84px with comfortable wider than compact", () => {
    expect(SIDEBAR_RAIL_WIDTH.compact.default).toBeLessThan(
      SIDEBAR_RAIL_WIDTH.comfortable.default,
    );
    expect(SIDEBAR_RAIL_WIDTH.comfortable.default).toBeLessThan(72);

    expect(SIDEBAR_RAIL_WIDTH.compact.mac).toBeLessThan(
      SIDEBAR_RAIL_WIDTH.comfortable.mac,
    );
    expect(SIDEBAR_RAIL_WIDTH.comfortable.mac).toBeLessThan(84);

    platform.mac = false;
    expect(sidebarRailWidth("compact")).toBe(52);
    expect(sidebarRailWidth("comfortable")).toBe(60);
    expect(sidebarWidth(true, "compact")).toBe(52);
    expect(sidebarWidth(true, "comfortable")).toBe(60);
    expect(sidebarWidth(false, "compact")).toBe(SIDEBAR_EXPANDED_WIDTH);
    expect(sidebarWidth(false, "comfortable")).toBe(SIDEBAR_EXPANDED_WIDTH);

    platform.mac = true;
    expect(sidebarRailWidth("compact")).toBe(76);
    expect(sidebarRailWidth("comfortable")).toBe(80);
    expect(sidebarWidth(true, "compact")).toBe(76);
    expect(sidebarWidth(true, "comfortable")).toBe(80);
  });

  it("updates collapsed sidebar width when switching between compact and comfortable", () => {
    renderSidebar();
    const nav = screen.getByRole("navigation", { name: "nav.mainLabel" });
    expect(nav).toHaveStyle({ width: "52px" });

    const appButton = screen.getByRole("button", { name: "Claude Code" });
    expect(appButton).toHaveClass("w-rail-item");
    expect(appButton).not.toHaveClass("w-12");

    act(() => {
      setDensity("comfortable");
    });
    expect(nav).toHaveStyle({ width: "60px" });

    act(() => {
      setDensity("compact");
    });
    expect(nav).toHaveStyle({ width: "52px" });
  });
});
