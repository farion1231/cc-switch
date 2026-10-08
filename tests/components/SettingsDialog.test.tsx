import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import type { ComponentProps } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SettingsPage } from "@/components/settings/SettingsPage";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const tMock = vi.fn((key: string) => key);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "system", setTheme: vi.fn() }),
}));

interface SettingsMock {
  settings: any;
  isLoading: boolean;
  isSaving: boolean;
  isPortable: boolean;
  appConfigDir?: string;
  initialAppConfigDir?: string;
  resolvedDirs: Record<string, string>;
  requiresRestart: boolean;
  updateSettings: ReturnType<typeof vi.fn>;
  updateDirectory: ReturnType<typeof vi.fn>;
  updateAppConfigDir: ReturnType<typeof vi.fn>;
  browseDirectory: ReturnType<typeof vi.fn>;
  browseAppConfigDir: ReturnType<typeof vi.fn>;
  resetDirectory: ReturnType<typeof vi.fn>;
  resetAppConfigDir: ReturnType<typeof vi.fn>;
  saveSettings: ReturnType<typeof vi.fn>;
  autoSaveSettings: ReturnType<typeof vi.fn>;
  resetSettings: ReturnType<typeof vi.fn>;
  acknowledgeRestart: ReturnType<typeof vi.fn>;
}

const savedSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableClaudePluginIntegration: false,
  language: "zh",
  claudeConfigDir: "/claude",
  codexConfigDir: "/codex",
};

const createSettingsMock = (overrides: Partial<SettingsMock> = {}) => {
  const base: SettingsMock = {
    settings: { ...savedSettings },
    isLoading: false,
    isSaving: false,
    isPortable: false,
    appConfigDir: "/app-config",
    initialAppConfigDir: "/app-config",
    resolvedDirs: {
      appConfig: "/app-config",
      claude: "/claude",
      codex: "/codex",
    },
    requiresRestart: false,
    updateSettings: vi.fn(),
    updateDirectory: vi.fn(),
    updateAppConfigDir: vi.fn(),
    browseDirectory: vi.fn(),
    browseAppConfigDir: vi.fn(),
    resetDirectory: vi.fn(),
    resetAppConfigDir: vi.fn(),
    saveSettings: vi.fn().mockResolvedValue({ requiresRestart: false }),
    autoSaveSettings: vi.fn().mockResolvedValue({ requiresRestart: false }),
    resetSettings: vi.fn(),
    acknowledgeRestart: vi.fn(),
  };

  return { ...base, ...overrides };
};

const createImportExportMock = () => ({
  selectedFile: "/tmp/config.sql",
  status: "idle",
  errorMessage: null,
  backupId: null,
  isImporting: false,
  selectImportFile: vi.fn(),
  importConfig: vi.fn(),
  exportConfig: vi.fn(),
  clearSelection: vi.fn(),
  resetStatus: vi.fn(),
});

let settingsMock = createSettingsMock();
let importExportMock = createImportExportMock();
const useImportExportSpy = vi.fn();

vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => settingsMock,
}));

vi.mock("@/hooks/useImportExport", () => ({
  useImportExport: (options?: Record<string, unknown>) =>
    useImportExportSpy(options),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: savedSettings }),
}));

const restartMock = vi.fn().mockResolvedValue(true);
vi.mock("@/lib/api", () => ({
  settingsApi: {
    restart: (...args: unknown[]) => restartMock(...args),
  },
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="dialog-root">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/settings/AboutSection", () => ({
  AboutSection: ({ isPortable }: any) => <div>about:{String(isPortable)}</div>,
}));
vi.mock("@/components/settings/WebdavSyncSection", () => ({
  WebdavSyncSection: () => <div>webdav-sync-section</div>,
}));
vi.mock("@/components/settings/BackupListSection", () => ({
  BackupListSection: () => <div>backup-list-section</div>,
}));
vi.mock("@/components/settings/LogConfigPanel", () => ({
  LogConfigPanel: () => <div>log-config-panel</div>,
}));
vi.mock("@/components/settings/ProxyTabContent", () => ({
  ProxyTabContent: () => <div>proxy-tab-content</div>,
}));
vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div>global-proxy-settings</div>,
}));
vi.mock("@/components/settings/CodexAuthSettings", () => ({
  CodexAuthSettings: () => <div>codex-auth-settings</div>,
}));

const renderSettingsPage = (
  props?: Partial<ComponentProps<typeof SettingsPage>>,
) => {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  const allProps = {
    section: "general" as const,
    onOpenApps: vi.fn(),
    onOpenApp: vi.fn(),
    ...props,
  };
  const view = render(
    <QueryClientProvider client={client}>
      <SettingsPage {...allProps} />
    </QueryClientProvider>,
  );
  return {
    ...view,
    props: allProps,
    rerenderWith: (next: Partial<ComponentProps<typeof SettingsPage>>) =>
      view.rerender(
        <QueryClientProvider client={client}>
          <SettingsPage {...allProps} {...next} />
        </QueryClientProvider>,
      ),
  };
};

describe("SettingsPage", () => {
  beforeEach(() => {
    tMock.mockImplementation((key: string) => key);
    settingsMock = createSettingsMock();
    importExportMock = createImportExportMock();
    useImportExportSpy.mockReset();
    useImportExportSpy.mockImplementation(() => importExportMock);
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    restartMock.mockClear();
  });

  it("shows a spinner instead of the form while loading", () => {
    settingsMock = createSettingsMock({ settings: null, isLoading: true });

    renderSettingsPage();

    expect(
      screen.queryByText("settings.general.appearance"),
    ).not.toBeInTheDocument();
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("titles the page with the current section", () => {
    renderSettingsPage({ section: "network" });

    expect(
      screen.getByRole("heading", {
        level: 1,
        name: "settings.sections.network",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("global-proxy-settings")).toBeInTheDocument();
  });

  it("saves general switches immediately and links to the Apps page", async () => {
    const { props } = renderSettingsPage();

    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );
    expect(settingsMock.updateSettings).toHaveBeenCalledWith({
      minimizeToTrayOnClose: false,
    });
    await waitFor(() =>
      expect(settingsMock.autoSaveSettings).toHaveBeenCalledWith({
        minimizeToTrayOnClose: false,
      }),
    );

    fireEvent.click(
      screen.getByRole("switch", { name: "settings.general.checkToolUpdates" }),
    );
    await waitFor(() =>
      expect(settingsMock.autoSaveSettings).toHaveBeenCalledWith({
        checkToolUpdatesOnStartup: true,
      }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: /settings\.general\.goToApps/ }),
    );
    expect(props.onOpenApps).toHaveBeenCalledTimes(1);
  });

  it("rolls a failed autosave back and reports it", async () => {
    settingsMock = createSettingsMock({
      autoSaveSettings: vi.fn().mockRejectedValue(new Error("disk full")),
    });

    renderSettingsPage();
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    );

    await waitFor(() =>
      expect(settingsMock.updateSettings).toHaveBeenLastCalledWith({
        minimizeToTrayOnClose: true,
      }),
    );
    expect(toastErrorMock).toHaveBeenCalledWith("settings.saveFailedGeneric");
  });

  it("wires import, export and sync into the data section", () => {
    renderSettingsPage({ section: "data" });

    fireEvent.click(screen.getByRole("button", { name: /settings\.import/ }));
    expect(importExportMock.importConfig).toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.exportConfig" }),
    );
    expect(importExportMock.exportConfig).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    expect(importExportMock.clearSelection).toHaveBeenCalled();

    expect(screen.getByText("webdav-sync-section")).toBeInTheDocument();
    expect(screen.getByText("backup-list-section")).toBeInTheDocument();
    expect(screen.getByText("log-config-panel")).toBeInTheDocument();
  });

  it("passes onImportSuccess through to the import hook", () => {
    const onImportSuccess = vi.fn();

    renderSettingsPage({ onImportSuccess });

    expect(useImportExportSpy).toHaveBeenCalledWith(
      expect.objectContaining({ onImportSuccess }),
    );
  });

  it("drives per-app directories and saves only after a change", async () => {
    const { rerenderWith } = renderSettingsPage({ section: "appConfig" });

    expect(screen.getByText("codex-auth-settings")).toBeInTheDocument();
    // 没改过目录：没有保存按钮
    expect(
      screen.queryByRole("button", { name: "common.save" }),
    ).not.toBeInTheDocument();

    fireEvent.click(
      screen.getAllByRole("button", { name: "settings.browseDirectory" })[0],
    );
    expect(settingsMock.browseDirectory).toHaveBeenCalledWith("claude");
    fireEvent.click(
      screen.getAllByRole("button", { name: "settings.resetDefault" })[1],
    );
    expect(settingsMock.resetDirectory).toHaveBeenCalledWith("codex");

    settingsMock = createSettingsMock({
      settings: { ...savedSettings, codexConfigDir: "/new/codex" },
    });
    rerenderWith({ section: "appConfig" });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(settingsMock.saveSettings).toHaveBeenCalledTimes(1),
    );
    expect(settingsMock.acknowledgeRestart).toHaveBeenCalledTimes(1);
  });

  it("asks to restart after moving the data directory", async () => {
    settingsMock = createSettingsMock({
      appConfigDir: "/moved",
      saveSettings: vi.fn().mockResolvedValue({ requiresRestart: true }),
    });

    renderSettingsPage({ section: "data" });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    expect(
      await screen.findByText("settings.restartRequired"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText("settings.restartNow"));

    await waitFor(() => {
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "settings.devModeRestartHint",
        expect.objectContaining({ closeButton: true }),
      );
    });
  });

  it("lets the restart wait", async () => {
    settingsMock = createSettingsMock({ requiresRestart: true });

    renderSettingsPage();

    expect(
      await screen.findByText("settings.restartRequired"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText("settings.restartLater"));

    await waitFor(() =>
      expect(settingsMock.acknowledgeRestart).toHaveBeenCalledTimes(1),
    );
    expect(restartMock).not.toHaveBeenCalled();
    expect(screen.queryByText("settings.restartRequired")).toBeNull();
  });

  it("scrolls back to the top when the section changes", () => {
    const { container, rerenderWith } = renderSettingsPage();
    const scrollContainer = container.querySelector(
      "#main-content",
    ) as HTMLDivElement;

    scrollContainer.scrollTop = 640;
    rerenderWith({ section: "about" });

    expect(scrollContainer.scrollTop).toBe(0);
    expect(screen.getByText("about:false")).toBeInTheDocument();
  });
});
