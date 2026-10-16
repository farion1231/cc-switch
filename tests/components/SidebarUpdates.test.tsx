import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar, type SidebarProps } from "@/components/shell/Sidebar";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";

let collapsed = false;
let hasUpdate = true;
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("4.0.13"),
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate }),
}));
vi.mock("@/hooks/useSidebarCollapsed", () => ({
  sidebarWidth: () => (collapsed ? 72 : 200),
  useSidebarCollapsed: () => ({ collapsed, toggle: vi.fn() }),
}));
vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: 0,
    authNeedsAttention: false,
  }),
}));

function renderSidebar(overrides: Partial<SidebarProps> = {}) {
  const props: SidebarProps = {
    activeApp: "claude",
    view: "providers",
    visibleApps: DEFAULT_VISIBLE_APPS,
    settingsSection: "general",
    onSelectApp: vi.fn(),
    onSelectPage: vi.fn(),
    onSelectSettingsSection: vi.fn(),
    onExitSettings: vi.fn(),
    ...overrides,
  };
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <Sidebar {...props} />
    </QueryClientProvider>,
  );
  return props;
}

describe("sidebar update indicators", () => {
  beforeEach(() => {
    collapsed = false;
    hasUpdate = true;
  });

  it.each([false, true])(
    "shows a new-version badge on settings, collapsed=%s",
    (value) => {
      collapsed = value;
      const props = renderSidebar();
      const settings = screen.getByRole("button", {
        name: "nav.settingsHasUpdate",
      });
      expect(
        within(settings).getByText("settings.newVersion"),
      ).toBeInTheDocument();
      fireEvent.click(settings);
      expect(props.onSelectPage).toHaveBeenCalledWith("settings");
    },
  );

  it.each([false, true])(
    "shows the red dot at the top left of About, collapsed=%s",
    (value) => {
      collapsed = value;
      const props = renderSidebar({ view: "settings" });
      const about = screen.getByRole("button", {
        name: "settings.sections.about · settings.newVersion",
      });
      expect(
        about.querySelector(".bg-danger.absolute.left-1.top-1"),
      ).toBeInTheDocument();
      fireEvent.click(about);
      expect(props.onSelectSettingsSection).toHaveBeenCalledWith("about");
    },
  );

  it("hides update indicators when no new version is available", () => {
    hasUpdate = false;
    renderSidebar();
    expect(screen.queryByText("settings.newVersion")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "nav.settings" }),
    ).toBeInTheDocument();
  });
});
