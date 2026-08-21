import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { providersApi } from "@/lib/api/providers";
import {
  resetProviderState,
  setCurrentProviderId,
  setLiveProviderIds,
  setProviders,
} from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const skillsPanelMocks = vi.hoisted(() => ({
  checkUpdates: vi.fn(),
  openDiscovery: vi.fn(),
}));
const copilotByokMocks = vi.hoisted(() => ({
  openAdd: vi.fn(),
}));
const copilotCliMocks = vi.hoisted(() => ({
  openAdd: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
    onEdit,
    onDuplicate,
    onConfigureUsage,
    onOpenWebsite,
    onCreate,
    onDelete,
    onRemoveFromConfig,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
      <button onClick={() => onEdit(providers[currentProviderId])}>edit</button>
      <button onClick={() => onDuplicate(providers[currentProviderId])}>
        duplicate
      </button>
      <button onClick={() => onConfigureUsage(providers[currentProviderId])}>
        usage
      </button>
      <button onClick={() => onOpenWebsite("https://example.com")}>
        open-website
      </button>
      <button onClick={() => onDelete(Object.values(providers)[0])}>
        delete
      </button>
      <button onClick={() => onRemoveFromConfig?.(Object.values(providers)[0])}>
        remove
      </button>
      <button onClick={() => onCreate?.()}>create</button>
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open, provider, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="edit-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              provider: {
                ...provider,
                name: `${provider.name}-edited`,
              },
              originalId: provider.id,
            })
          }
        >
          confirm-edit
        </button>
        <button onClick={() => onOpenChange(false)}>close-edit</button>
      </div>
    ) : null,
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen, provider, onSave, onClose }: any) =>
    isOpen ? (
      <div data-testid="usage-modal">
        <span data-testid="usage-provider">{provider?.id}</span>
        <button onClick={() => onSave("script-code")}>save-script</button>
        <button onClick={() => onClose()}>close-usage</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, message, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <div data-testid="confirm-message">{message}</div>
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
      <button onClick={() => onSwitch("copilot-byok")}>switch-copilot</button>
      <button onClick={() => onSwitch("copilot-cli")}>
        switch-copilot-cli
      </button>
    </div>
  ),
}));

vi.mock("@/components/settings/CopilotByokSettings", async () => {
  const React = await import("react");
  const MockCopilotByokSettings = React.forwardRef((props: any, ref) => {
    React.useImperativeHandle(ref, () => ({
      openAdd: copilotByokMocks.openAdd,
    }));
    return <div data-testid="copilot-byok-settings" data-mode={props.mode} />;
  });
  MockCopilotByokSettings.displayName = "MockCopilotByokSettings";
  return { CopilotByokSettings: MockCopilotByokSettings };
});

vi.mock("@/components/settings/CopilotCliSettings", async () => {
  const React = await import("react");
  const MockCopilotCliSettings = React.forwardRef((_props: any, ref) => {
    React.useImperativeHandle(ref, () => ({
      openAdd: copilotCliMocks.openAdd,
    }));
    return <div data-testid="copilot-cli-settings" />;
  });
  MockCopilotCliSettings.displayName = "MockCopilotCliSettings";
  return { CopilotCliSettings: MockCopilotCliSettings };
});

vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: ({ defaultTab, usageDefaultFilter, onOpenChange }: any) => (
    <div
      data-testid="settings-page"
      data-default-tab={defaultTab}
      data-usage-app={usageDefaultFilter?.appType}
      data-usage-provider={usageDefaultFilter?.providerName}
    >
      <button onClick={() => onOpenChange(false)}>close-settings</button>
    </div>
  ),
}));

vi.mock("@/components/sessions/SessionManagerPage", () => ({
  SessionManagerPage: ({ appId }: { appId: string }) => (
    <div data-testid="session-manager-page" data-app-id={appId} />
  ),
}));

vi.mock("@/components/skills/UnifiedSkillsPanel", async () => {
  const React = await import("react");
  const MockUnifiedSkillsPanel = React.forwardRef(
    ({ onCheckUpdatesStateChange }: any, ref) => {
      React.useEffect(() => {
        onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: true });
        return () =>
          onCheckUpdatesStateChange?.({
            isChecking: false,
            hasSkills: false,
          });
      }, [onCheckUpdatesStateChange]);
      React.useImperativeHandle(ref, () => ({
        openDiscovery: skillsPanelMocks.openDiscovery,
        openImport: vi.fn(),
        openInstallFromZip: vi.fn(),
        openRestoreFromBackup: vi.fn(),
        checkUpdates: skillsPanelMocks.checkUpdates,
      }));
      return <div data-testid="unified-skills-panel" />;
    },
  );
  MockUnifiedSkillsPanel.displayName = "MockUnifiedSkillsPanel";
  return { default: MockUnifiedSkillsPanel };
});

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
};

describe("App integration with MSW", () => {
  beforeEach(() => {
    resetProviderState();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    skillsPanelMocks.checkUpdates.mockReset();
    skillsPanelMocks.openDiscovery.mockReset();
    copilotByokMocks.openAdd.mockReset();
    copilotCliMocks.openAdd.mockReset();
    localStorage.removeItem("cc-switch-last-view");
    localStorage.removeItem("cc-switch-last-app");
  });

  it("opens VS Code Copilot as a primary page from the app switcher", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(await screen.findByText("switch-openclaw"));
    fireEvent.click(await screen.findByText("switch-copilot"));

    expect(
      await screen.findByTestId("copilot-byok-settings"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("copilot-byok-settings")).toHaveAttribute(
      "data-mode",
      "catalog",
    );
    expect(screen.getByTestId("app-switcher")).toHaveTextContent(
      "copilot-byok",
    );
    expect(screen.getByTestId("app-switcher")).toBeInTheDocument();
    expect(document.querySelector('button[title="使用统计"]')).toBeNull();
    const syncTargetsButton = await screen.findByTitle("copilotByok.targets");
    for (const title of [
      "skills.manage",
      "prompts.manage",
      "sessionManager.title",
      "mcp.title",
    ]) {
      expect(document.querySelector(`button[title="${title}"]`)).not.toBeNull();
    }
    expect(syncTargetsButton.querySelector(".lucide-cpu")).not.toBeNull();

    const addByok = document.querySelector<HTMLButtonElement>(
      'button[aria-label="provider.addNewProvider"]',
    );
    expect(addByok).not.toBeNull();
    fireEvent.click(addByok!);
    expect(copilotByokMocks.openAdd).toHaveBeenCalledTimes(1);

    fireEvent.click(syncTargetsButton);
    await waitFor(() =>
      expect(screen.getByTestId("copilot-byok-settings")).toHaveAttribute(
        "data-mode",
        "targets",
      ),
    );

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(screen.getByTestId("copilot-byok-settings")).toHaveAttribute(
        "data-mode",
        "catalog",
      ),
    );

    fireEvent.click(
      document.querySelector<HTMLButtonElement>(
        'button[title="sessionManager.title"]',
      )!,
    );
    expect(await screen.findByTestId("session-manager-page")).toHaveAttribute(
      "data-app-id",
      "copilot-byok",
    );
  }, 10_000);

  it("opens Copilot CLI as an independent primary page with first-class tools", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(await screen.findByText("switch-copilot-cli"));

    expect(
      await screen.findByTestId("copilot-cli-settings"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("app-switcher")).toHaveTextContent("copilot-cli");
    expect(document.querySelector('button[title="使用统计"]')).toBeNull();
    await waitFor(() =>
      expect(
        document.querySelector(
          'button[title="自定义指令"], button[title="Custom Instructions"], button[title="copilotByok.cli.instructions"]',
        ),
      ).not.toBeNull(),
    );
    expect(
      document.querySelector('button[title="skills.manage"]'),
    ).not.toBeNull();
    expect(
      document.querySelector('button[title="sessionManager.title"]'),
    ).not.toBeNull();
    expect(document.querySelector('button[title="mcp.title"]')).not.toBeNull();
    expect(
      document.querySelector('button[title="copilotByok.targets"]'),
    ).toBeNull();

    const addCli = document.querySelector<HTMLButtonElement>(
      'button[aria-label="provider.addNewProvider"]',
    );
    expect(addCli).not.toBeNull();
    fireEvent.click(addCli!);
    expect(copilotCliMocks.openAdd).toHaveBeenCalledTimes(1);

    fireEvent.click(
      document.querySelector<HTMLButtonElement>(
        'button[title="sessionManager.title"]',
      )!,
    );
    expect(await screen.findByTestId("session-manager-page")).toHaveAttribute(
      "data-app-id",
      "copilot-cli",
    );
  }, 10_000);

  it("covers basic provider flows via real hooks", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(screen.getByText("switch-codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByText("usage"));
    expect(screen.getByTestId("usage-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByText("save-script"));
    fireEvent.click(screen.getByText("close-usage"));

    fireEvent.click(screen.getByText("create"));
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-add"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /New codex Provider/,
      ),
    );

    fireEvent.click(screen.getByText("edit"));
    expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-edit"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /-edited/,
      ),
    );

    fireEvent.click(screen.getByText("switch"));
    fireEvent.click(screen.getByText("duplicate"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(/copy/),
    );

    fireEvent.click(screen.getByText("open-website"));

    emitTauriEvent("provider-switched", {
      appType: "codex",
      providerId: "codex-2",
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).toHaveBeenCalled();
  }, 10_000);

  it("shows toast when auto sync fails in background", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });

    toastErrorMock.mockReset();
    expect(() => {
      emitTauriEvent("s3-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });

  it("duplicates openclaw providers with a generated key that avoids live-only ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");
    setLiveProviderIds("openclaw", ["deepseek-copy"]);

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      const providerList = screen.getByTestId("provider-list").textContent;
      expect(providerList).toContain("deepseek-copy-2");
      expect(providerList).toContain("DeepSeek copy");
    });

    expect(toastErrorMock).not.toHaveBeenCalledWith(
      expect.stringContaining("Provider key is required for openclaw"),
    );
  });

  it("warns without blocking when removing Pi's global default provider", async () => {
    localStorage.setItem("cc-switch-last-app", "pi");
    setProviders("pi", {
      custom: {
        id: "custom",
        name: "Custom Pi",
        settingsConfig: {
          baseUrl: "https://api.example.com/v1",
          apiKey: "test-key",
          api: "openai-completions",
          models: [{ id: "model-a" }],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    server.use(
      http.post("http://tauri.local/get_pi_current_state", () =>
        HttpResponse.json({
          enabledProviderIds: ["custom"],
          defaultProviderId: "custom",
        }),
      ),
    );

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "Custom Pi",
      ),
    );
    fireEvent.click(screen.getByText("remove"));

    expect(screen.getByTestId("confirm-message")).toHaveTextContent(
      "confirm.piDefaultProviderWarning",
    );
    fireEvent.click(screen.getByText("confirm-delete"));
    await waitFor(() =>
      expect(screen.queryByTestId("confirm-dialog")).not.toBeInTheDocument(),
    );
  });

  it("shows toast when duplicate cannot load live provider ids", async () => {
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");

    const liveIdsSpy = vi
      .spyOn(providersApi, "getOpenClawLiveProviderIds")
      .mockRejectedValueOnce(new Error("broken config"));

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("读取配置中的供应商标识失败"),
      );
    });

    expect(screen.getByTestId("provider-list").textContent).not.toContain(
      "deepseek-copy",
    );

    liveIdsSpy.mockRestore();
  });

  it("hosts the Skills check-update action in the App toolbar", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    const checkUpdatesButton = await screen.findByRole("button", {
      name: "skills.checkUpdates",
    });
    await waitFor(() => expect(checkUpdatesButton).toBeEnabled());

    fireEvent.click(checkUpdatesButton);
    expect(skillsPanelMocks.checkUpdates).toHaveBeenCalledTimes(1);
  });

  it("routes the Skills discover toolbar action through the panel guard", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "skills.discover",
      }),
    );

    expect(skillsPanelMocks.openDiscovery).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("unified-skills-panel")).toBeInTheDocument();
  });
});
