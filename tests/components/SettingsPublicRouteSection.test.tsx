import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { Sidebar } from "@/components/shell/Sidebar";
import { SETTINGS_SECTIONS } from "@/lib/navigation";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: {},
    isLoading: false,
    isSaving: false,
    isPortable: false,
    appConfigDir: "",
    initialAppConfigDir: "",
    resolvedDirs: {},
    updateSettings: vi.fn(),
    updateDirectory: vi.fn(),
    updateAppConfigDir: vi.fn(),
    browseDirectory: vi.fn(),
    browseAppConfigDir: vi.fn(),
    resetDirectory: vi.fn(),
    resetAppConfigDir: vi.fn(),
    saveSettings: vi.fn(),
    autoSaveSettings: vi.fn(),
    requiresRestart: false,
    acknowledgeRestart: vi.fn(),
  }),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: undefined }),
}));

vi.mock("@/hooks/useImportExport", () => ({
  useImportExport: () => ({
    selectedFile: null,
    status: "idle",
    errorMessage: null,
    backupId: null,
    isImporting: false,
    selectImportFile: vi.fn(),
    importConfig: vi.fn(),
    exportConfig: vi.fn(),
    clearSelection: vi.fn(),
    resetStatus: vi.fn(),
  }),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: { restart: vi.fn() },
}));

// 面板本体（含自己的 hooks）stub 掉，只断言它被挂载在公网路由分组里
vi.mock("@/components/proxy/PublicRoutePanel", () => ({
  PublicRoutePanel: () => <div data-testid="public-route-panel" />,
}));

// Sidebar 侧栏（设置目录就长在它里面）
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("4.0.4"),
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false }),
}));
vi.mock("@/hooks/useSidebarCollapsed", () => ({
  useSidebarCollapsed: () => ({ collapsed: false, toggle: vi.fn() }),
  sidebarWidth: () => 240,
}));
vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: {},
    todayCost: 0,
    authNeedsAttention: false,
  }),
}));

const ALL_VISIBLE = Object.fromEntries(
  [
    "claude",
    "claude-desktop",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "hermes",
    "cursor",
    "pi",
    "mcode",
  ].map((id) => [id, true]),
) as never;

function renderSidebar() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <Sidebar
        activeApp="claude"
        view="settings"
        visibleApps={ALL_VISIBLE}
        settingsSection="routing"
        onSelectApp={() => {}}
        onSelectPage={() => {}}
        onSelectSettingsSection={() => {}}
        onExitSettings={() => {}}
      />
    </QueryClientProvider>,
  );
}

function renderSection(section: "publicRoute") {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <SettingsPage section={section} onOpenApps={() => {}} onOpenApp={() => {}} />
    </QueryClientProvider>,
  );
}

/**
 * 2026-10-09 回归（两轮）：
 *  ① 公网路由面板曾因上游重写设置页而变成孤儿（没人渲染）→「功能失效」；
 *  ② 随后被暂时塞进「本地路由」页面当栏目 —— 用户要求它与「本地路由」**同级**。
 * 故断言：它必须是 SETTINGS_SECTIONS 里的独立分组（紧邻 routing），且该分组渲染面板。
 */
describe("设置 → 公网路由（与本地路由同级）", () => {
  it("注册为独立分组，排在「本地路由」之后", () => {
    expect(SETTINGS_SECTIONS).toContain("publicRoute");
    expect(SETTINGS_SECTIONS[SETTINGS_SECTIONS.indexOf("routing") + 1]).toBe(
      "publicRoute",
    );
  });

  it("该分组渲染公网路由面板", () => {
    renderSection("publicRoute");
    expect(screen.getByTestId("public-route-panel")).toBeInTheDocument();
  });

  // 上一轮的漏点：只断言了 SETTINGS_SECTIONS 与 SettingsPage，**没断言侧栏**——
  // 而侧栏导航用的是另一份硬编码 SETTINGS_ITEMS，于是装出来"设置里看不到公网路由"。
  it("侧栏设置目录里出现「公网路由」导航项", () => {
    renderSidebar();
    expect(
      screen.getByText("settings.sections.publicRoute"),
    ).toBeInTheDocument();
  });
});
