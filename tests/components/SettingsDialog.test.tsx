import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import "@testing-library/jest-dom";
import { createContext, useContext } from "react";
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

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    status: null,
    isLoading: false,
    isRunning: false,
    isTakeoverActive: false,
    startWithTakeover: vi.fn(),
    stopWithRestore: vi.fn(),
    switchProxyProvider: vi.fn(),
    checkRunning: vi.fn(),
    checkTakeoverActive: vi.fn(),
    isStarting: false,
    isStopping: false,
    isPending: false,
  }),
}));

interface SettingsMock {
  settings: any;
  isLoading: boolean;
  isSaving: boolean;
  isPortable: boolean;
  appConfigDir?: string;
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

const createSettingsMock = (overrides: Partial<SettingsMock> = {}) => {
  const base: SettingsMock = {
    settings: {
      showInTray: true,
      minimizeToTrayOnClose: true,
      enableClaudePluginIntegration: false,
      language: "zh",
      claudeConfigDir: "/claude",
      codexConfigDir: "/codex",
    },
    isLoading: false,
    isSaving: false,
    isPortable: false,
    appConfigDir: "/app-config",
    resolvedDirs: {
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

interface ImportExportMock {
  selectedFile: string;
  status: string;
  errorMessage: string | null;
  backupId: string | null;
  isImporting: boolean;
  selectImportFile: ReturnType<typeof vi.fn>;
  importConfig: ReturnType<typeof vi.fn>;
  exportConfig: ReturnType<typeof vi.fn>;
  clearSelection: ReturnType<typeof vi.fn>;
  resetStatus: ReturnType<typeof vi.fn>;
}

const createImportExportMock = (overrides: Partial<ImportExportMock> = {}) => {
  const base: ImportExportMock = {
    selectedFile: "",
    status: "idle",
    errorMessage: null,
    backupId: null,
    isImporting: false,
    selectImportFile: vi.fn(),
    importConfig: vi.fn(),
    exportConfig: vi.fn(),
    clearSelection: vi.fn(),
    resetStatus: vi.fn(),
  };

  return { ...base, ...overrides };
};

let settingsMock = createSettingsMock();
let importExportMock = createImportExportMock();
const useImportExportSpy = vi.fn();
let lastUseImportExportOptions: Record<string, unknown> | undefined;

vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => settingsMock,
}));

vi.mock("@/hooks/useImportExport", () => ({
  useImportExport: (options?: Record<string, unknown>) =>
    useImportExportSpy(options),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    restart: vi.fn().mockResolvedValue(true),
  },
}));

const TabsContext = createContext<{
  value: string;
  onValueChange?: (value: string) => void;
}>({
  value: "general",
});

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="dialog-root">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

vi.mock("@/components/ui/tabs", () => {
  return {
    Tabs: ({ value, onValueChange, children }: any) => (
      <TabsContext.Provider value={{ value, onValueChange }}>
        <div data-testid="tabs">{children}</div>
      </TabsContext.Provider>
    ),
    TabsList: ({ children }: any) => <div>{children}</div>,
    TabsTrigger: ({ value, children }: any) => {
      const ctx = useContext(TabsContext);
      return (
        <button type="button" onClick={() => ctx.onValueChange?.(value)}>
          {children}
        </button>
      );
    },
    TabsContent: ({ value, children }: any) => {
      const ctx = useContext(TabsContext);
      if (ctx.value !== value) return null;
      return <div data-testid={`tab-${value}`}>{children}</div>;
    },
  };
});

vi.mock("@/components/settings/LanguageSettings", () => ({
  LanguageSettings: ({ value, onChange }: any) => (
    <div>
      <span>language:{value}</span>
      <button onClick={() => onChange("en")}>change-language</button>
    </div>
  ),
}));

vi.mock("@/components/settings/ThemeSettings", () => ({
  ThemeSettings: () => <div>theme-settings</div>,
}));

vi.mock("@/components/settings/WindowSettings", () => ({
  WindowSettings: ({ onChange }: any) => (
    <button onClick={() => onChange({ minimizeToTrayOnClose: false })}>
      window-settings
    </button>
  ),
}));

vi.mock("@/components/settings/DirectorySettings", () => ({
  DirectorySettings: ({
    onBrowseDirectory,
    onResetDirectory,
    onDirectoryChange,
    onBrowseAppConfig,
    onResetAppConfig,
    onAppConfigChange,
  }: any) => (
    <div>
      <button onClick={() => onBrowseDirectory("claude")}>
        browse-directory
      </button>
      <button onClick={() => onResetDirectory("claude")}>
        reset-directory
      </button>
      <button onClick={() => onDirectoryChange("codex", "/new/path")}>
        change-directory
      </button>
      <button onClick={() => onBrowseAppConfig()}>browse-app-config</button>
      <button onClick={() => onResetAppConfig()}>reset-app-config</button>
      <button onClick={() => onAppConfigChange("/app/new")}>
        change-app-config
      </button>
    </div>
  ),
}));

vi.mock("@/components/settings/AboutSection", () => ({
  AboutSection: ({ isPortable }: any) => <div>about:{String(isPortable)}</div>,
}));

let settingsApi: any;

describe("SettingsPage Component", () => {
  beforeEach(async () => {
    tMock.mockImplementation((key: string) => key);
    settingsMock = createSettingsMock();
    importExportMock = createImportExportMock();
    useImportExportSpy.mockReset();
    useImportExportSpy.mockImplementation(
      (options?: Record<string, unknown>) => {
        lastUseImportExportOptions = options;
        return importExportMock;
      },
    );
    lastUseImportExportOptions = undefined;
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    settingsApi = (await import("@/lib/api")).settingsApi;
    settingsApi.restart.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("should not render form content when loading", () => {
    settingsMock = createSettingsMock({ settings: null, isLoading: true });

    render(<SettingsPage open={true} onOpenChange={vi.fn()} />);

    expect(screen.queryByText("language:zh")).not.toBeInTheDocument();
    // 加载状态下显示 spinner 而不是表单内容
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("should reset import/export status when dialog transitions to open", () => {
    const { rerender } = render(
      <SettingsPage open={false} onOpenChange={vi.fn()} />,
    );

    importExportMock.resetStatus.mockClear();

    rerender(<SettingsPage open={true} onOpenChange={vi.fn()} />);

    expect(importExportMock.resetStatus).toHaveBeenCalledTimes(1);
  });

  it("should render general and advanced tabs and trigger child callbacks", () => {
    const onOpenChange = vi.fn();
    // 设置 selectedFile 后，按钮显示 settings.import（可执行导入）
    importExportMock = createImportExportMock({
      selectedFile: "/tmp/config.json",
    });

    render(<SettingsPage open={true} onOpenChange={onOpenChange} />);

    expect(screen.getByText("language:zh")).toBeInTheDocument();
    expect(screen.getByText("theme-settings")).toBeInTheDocument();

    fireEvent.click(screen.getByText("change-language"));
    expect(settingsMock.updateSettings).toHaveBeenCalledWith({
      language: "en",
    });

    fireEvent.click(screen.getByText("window-settings"));
    expect(settingsMock.updateSettings).toHaveBeenCalledWith({
      minimizeToTrayOnClose: false,
    });

    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.data.title"));

    // 有文件时，点击导入按钮执行 importConfig
    fireEvent.click(
      screen.getByRole("button", { name: /settings\.import/ }),
    );
    expect(importExportMock.importConfig).toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: "settings.exportConfig" }),
    );
    expect(importExportMock.exportConfig).toHaveBeenCalled();

    // 清除选择按钮
    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    expect(importExportMock.clearSelection).toHaveBeenCalled();
  });

  it("should pass onImportSuccess callback to useImportExport hook", async () => {
    const onImportSuccess = vi.fn();

    render(
      <SettingsPage
        open={true}
        onOpenChange={vi.fn()}
        onImportSuccess={onImportSuccess}
      />,
    );

    expect(useImportExportSpy).toHaveBeenCalledWith(
      expect.objectContaining({ onImportSuccess }),
    );
    expect(lastUseImportExportOptions?.onImportSuccess).toBe(onImportSuccess);

    if (typeof lastUseImportExportOptions?.onImportSuccess === "function") {
      await lastUseImportExportOptions.onImportSuccess();
    }
    expect(onImportSuccess).toHaveBeenCalledTimes(1);
  });

  it("should call saveSettings and close dialog when clicking save", async () => {
    const onOpenChange = vi.fn();
    importExportMock = createImportExportMock();

    render(<SettingsPage open={true} onOpenChange={onOpenChange} />);

    // 保存按钮在 advanced tab 中
    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByRole("button", { name: /common\.save/ }));

    await waitFor(() => {
      expect(settingsMock.saveSettings).toHaveBeenCalledTimes(1);
      expect(importExportMock.clearSelection).toHaveBeenCalledTimes(1);
      expect(importExportMock.resetStatus).toHaveBeenCalledTimes(2);
      expect(settingsMock.acknowledgeRestart).toHaveBeenCalledTimes(1);
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });

  it("should show restart prompt and allow immediate restart after save", async () => {
    settingsMock = createSettingsMock({
      requiresRestart: true,
      saveSettings: vi.fn().mockResolvedValue({ requiresRestart: true }),
    });

    render(<SettingsPage open={true} onOpenChange={vi.fn()} />);

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

  it("should allow postponing restart and close dialog without restarting", async () => {
    const onOpenChange = vi.fn();
    settingsMock = createSettingsMock({ requiresRestart: true });

    render(<SettingsPage open={true} onOpenChange={onOpenChange} />);

    expect(
      await screen.findByText("settings.restartRequired"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByText("settings.restartLater"));

    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(settingsMock.acknowledgeRestart).toHaveBeenCalledTimes(1);
    });

    expect(settingsApi.restart).not.toHaveBeenCalled();
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("should trigger directory management callbacks inside advanced tab", () => {
    render(<SettingsPage open={true} onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getByText("settings.tabAdvanced"));
    fireEvent.click(screen.getByText("settings.advanced.configDir.title"));

    fireEvent.click(screen.getByText("browse-directory"));
    expect(settingsMock.browseDirectory).toHaveBeenCalledWith("claude");

    fireEvent.click(screen.getByText("reset-directory"));
    expect(settingsMock.resetDirectory).toHaveBeenCalledWith("claude");

    fireEvent.click(screen.getByText("change-directory"));
    expect(settingsMock.updateDirectory).toHaveBeenCalledWith(
      "codex",
      "/new/path",
    );

    fireEvent.click(screen.getByText("browse-app-config"));
    expect(settingsMock.browseAppConfigDir).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByText("reset-app-config"));
    expect(settingsMock.resetAppConfigDir).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByText("change-app-config"));
    expect(settingsMock.updateAppConfigDir).toHaveBeenCalledWith("/app/new");
  });
});
