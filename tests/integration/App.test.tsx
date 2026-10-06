import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  screen,
  waitFor,
  fireEvent,
  within,
} from "@testing-library/react";
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
  initialViews: [] as string[],
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

vi.mock("@/components/providers/EditProviderDialog", async () => {
  const { useUnsavedChangesTracker } = await vi.importActual<
    typeof import("@/lib/unsavedChanges")
  >("@/lib/unsavedChanges");
  // 和真的编辑页一样登记改动（真的由 FullScreenPanel 的 trackUnsavedChanges 负责）
  const Body = ({ provider, onSubmit, onOpenChange }: any) => (
    <div data-testid="edit-provider-dialog" {...useUnsavedChangesTracker()}>
      <input aria-label="edit-field" />
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
  );
  return {
    EditProviderDialog: (props: any) =>
      props.open ? <Body {...props} /> : null,
  };
});

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

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));

// 设置页要 ThemeProvider，这里只看导航进去时发生了什么
vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: () => <div data-testid="settings-page" />,
}));

vi.mock("@/components/skills/UnifiedSkillsPanel", () => ({
  // v7：Skills 的页头（添加 / 检查更新 / 存储与同步）在面板自己里面
  default: ({ initialView }: { initialView?: string }) => {
    skillsPanelMocks.initialViews.push(initialView ?? "installed");
    return <div data-testid="unified-skills-panel">{initialView}</div>;
  },
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

/** 侧栏里的应用行（v7 侧栏取代了原来页头的应用切换器） */
const sidebarApp = (name: string) =>
  // 首次启动提示是模态对话框，会把侧栏标成 aria-hidden，所以带上 hidden
  within(document.querySelector("nav") as HTMLElement).getByRole("button", {
    name,
    hidden: true,
  });

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
    skillsPanelMocks.initialViews = [];
    localStorage.removeItem("cc-switch-last-view");
    localStorage.removeItem("cc-switch-last-app");
  });

  it("covers basic provider flows via real hooks", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(sidebarApp("Codex"));
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

  it("resets provider view scroll when switching apps", async () => {
    const { default: App } = await import("@/App");
    const { container } = renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    const mainScrollContainer = container.querySelector("main") as HTMLElement;
    // 列表的滚动区在模式行下面（切换式应用每个应用一份）
    const providerScrollContainer = () =>
      container.querySelector<HTMLElement>("#main-content");

    expect(mainScrollContainer).not.toBeNull();
    expect(providerScrollContainer()).not.toBeNull();

    mainScrollContainer.scrollTop = 320;
    mainScrollContainer.scrollLeft = 12;
    providerScrollContainer()!.scrollTop = 640;
    providerScrollContainer()!.scrollLeft = 24;

    fireEvent.click(sidebarApp("Codex"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    expect(mainScrollContainer.scrollTop).toBe(0);
    expect(mainScrollContainer.scrollLeft).toBe(0);
    expect(providerScrollContainer()!.scrollTop).toBe(0);
    expect(providerScrollContainer()!.scrollLeft).toBe(0);
  }, 10_000);

  it("closes provider panels when navigating away from the app page", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    // 面板只盖住内容区，侧栏还能点：切应用时编辑面板必须关掉，否则 Claude 的
    // 供应商会以 appId=codex 保存进 Codex
    fireEvent.click(screen.getByText("edit"));
    expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
    fireEvent.click(sidebarApp("Codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
    expect(
      screen.queryByTestId("edit-provider-dialog"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("usage"));
    expect(screen.getByTestId("usage-modal")).toBeInTheDocument();
    fireEvent.click(sidebarApp("nav.usage"));
    await waitFor(() =>
      expect(screen.queryByTestId("usage-modal")).not.toBeInTheDocument(),
    );

    fireEvent.click(sidebarApp("Codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
    fireEvent.click(screen.getByText("create"));
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: ",", metaKey: true });
    expect(await screen.findByTestId("settings-page")).toBeInTheDocument();
    expect(screen.queryByTestId("add-provider-dialog")).not.toBeInTheDocument();
  }, 10_000);

  it("asks before leaving an editor page with unsaved changes", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    // 没改过：照常离开，不问
    fireEvent.click(screen.getByText("edit"));
    fireEvent.click(sidebarApp("Codex"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );
    expect(screen.queryByTestId("confirm-dialog")).not.toBeInTheDocument();

    // 改过：先问；继续编辑就留在原页
    // （ConfirmDialog 在这个文件里是 mock：只渲染消息和 confirm-delete / cancel-delete）
    fireEvent.click(screen.getByText("edit"));
    fireEvent.input(screen.getByLabelText("edit-field"), {
      target: { value: "draft" },
    });
    fireEvent.click(sidebarApp("Claude Code"));
    expect(await screen.findByTestId("confirm-message")).toHaveTextContent(
      "common.unsavedLeaveMessage",
    );
    fireEvent.click(screen.getByText("cancel-delete"));
    expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("provider-list").textContent).toContain(
      "codex-1",
    );

    // ⌘, 也问；放弃后才离开
    fireEvent.keyDown(window, { key: ",", metaKey: true });
    expect(await screen.findByTestId("confirm-message")).toHaveTextContent(
      "common.unsavedLeaveMessage",
    );
    fireEvent.click(screen.getByText("confirm-delete"));
    expect(await screen.findByTestId("settings-page")).toBeInTheDocument();
    expect(
      screen.queryByTestId("edit-provider-dialog"),
    ).not.toBeInTheDocument();
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

    fireEvent.click(sidebarApp("OpenClaw"));

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

  it.each([
    { options: { apiKey: "test-key" } },
    { npm: "@ai-sdk/openai-compatible", models: {} },
    { models: { "glm-5": { name: "GLM 5" } } },
  ])(
    "blocks incomplete OpenCode copies before saving or sorting: %j",
    async (settingsConfig) => {
      localStorage.setItem("cc-switch-last-app", "opencode");
      setProviders("opencode", {
        "opencode-go": {
          id: "opencode-go",
          name: "OpenCode Go",
          settingsConfig,
          sortIndex: 0,
        },
        other: { id: "other", name: "Other", settingsConfig: {}, sortIndex: 1 },
      });
      setCurrentProviderId("opencode", "opencode-go");
      setLiveProviderIds("opencode", ["opencode-go"]);
      const add = vi.spyOn(providersApi, "add");
      const sort = vi.spyOn(providersApi, "updateSortOrder");
      try {
        const { default: App } = await import("@/App");
        renderApp(App);
        await waitFor(() =>
          expect(screen.getByTestId("provider-list").textContent).toContain(
            "opencode-go",
          ),
        );
        fireEvent.click(screen.getByText("duplicate"));
        await waitFor(() =>
          expect(toastErrorMock).toHaveBeenCalledWith(
            "opencode.duplicateRequiresDefinition",
          ),
        );
        expect(add).not.toHaveBeenCalled();
        expect(sort).not.toHaveBeenCalled();
        expect(screen.getByTestId("provider-list").textContent).not.toContain(
          "opencode-go-copy",
        );
      } finally {
        add.mockRestore();
        sort.mockRestore();
      }
    },
  );

  it("duplicates complete OpenCode providers using an unused ID", async () => {
    localStorage.setItem("cc-switch-last-app", "opencode");
    setProviders("opencode", {
      custom: {
        id: "custom",
        name: "Custom",
        sortIndex: 0,
        settingsConfig: {
          npm: "@ai-sdk/openai-compatible",
          models: { "glm-5": { name: "GLM 5" } },
        },
      },
    });
    setCurrentProviderId("opencode", "custom");
    setLiveProviderIds("opencode", ["custom-copy"]);
    const { default: App } = await import("@/App");
    renderApp(App);
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "custom",
      ),
    );
    fireEvent.click(screen.getByText("duplicate"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "custom-copy-2",
      ),
    );
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("duplicates MiniMax Code providers under a generated unused key", async () => {
    localStorage.setItem("cc-switch-last-app", "mcode");
    const provider = (id: string, name: string) => ({
      id,
      name,
      settingsConfig: {},
      category: "custom" as const,
      sortIndex: 0,
      createdAt: Date.now(),
    });
    setProviders("mcode", {
      kimi: provider("kimi", "Kimi"),
      "kimi-copy": provider("kimi-copy", "Kimi copy"),
    });
    setCurrentProviderId("mcode", "kimi");

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "kimi-copy",
      ),
    );
    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "kimi-copy-2",
      ),
    );
    expect(toastErrorMock).not.toHaveBeenCalledWith(
      expect.stringContaining("Provider key is required for mcode"),
    );
  });

  it("refreshes MiniMax Code provider membership after removing it from live config", async () => {
    localStorage.setItem("cc-switch-last-app", "mcode");
    let liveConfigManaged = true;
    let providerRequests = 0;
    server.use(
      http.post("http://tauri.local/get_providers", async ({ request }) => {
        const { app } = (await request.json()) as { app: string };
        if (app !== "mcode") return;
        providerRequests += 1;
        return HttpResponse.json({
          custom: {
            id: "custom",
            name: "Custom MiniMax Code",
            settingsConfig: {},
            meta: { liveConfigManaged },
          },
        });
      }),
      http.post(
        "http://tauri.local/remove_provider_from_live_config",
        async ({ request }) => {
          expect(await request.json()).toEqual({ id: "custom", app: "mcode" });
          liveConfigManaged = false;
          return HttpResponse.json(true);
        },
      ),
    );

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list")).toHaveTextContent(
        '"liveConfigManaged":true',
      ),
    );
    const requestsBeforeRemoval = providerRequests;
    fireEvent.click(screen.getByText("remove"));
    fireEvent.click(screen.getByText("confirm-delete"));

    await waitFor(() =>
      expect(screen.queryByTestId("confirm-dialog")).not.toBeInTheDocument(),
    );
    expect(liveConfigManaged).toBe(false);
    await waitFor(() =>
      expect(screen.getByTestId("provider-list")).toHaveTextContent(
        '"liveConfigManaged":false',
      ),
    );
    expect(providerRequests).toBeGreaterThan(requestsBeforeRemoval);
    expect(screen.getByTestId("provider-list")).toHaveTextContent(
      "Custom MiniMax Code",
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

    fireEvent.click(sidebarApp("OpenClaw"));

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

  it("renders the Skills page with its own header", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(await screen.findByTestId("unified-skills-panel")).toHaveTextContent(
      "installed",
    );
  });

  it("navigates OpenClaw and Hermes pages with underline tabs", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(sidebarApp("OpenClaw"));
    // 首次启动提示是模态对话框，页面其余部分是 aria-hidden，所以带上 hidden
    const openclawTabs = await screen.findByRole("tablist", {
      name: "OpenClaw",
      hidden: true,
    });
    const pageTabs = within(openclawTabs).getAllByRole("tab", { hidden: true });
    expect(pageTabs.map((tab) => tab.textContent)).toEqual([
      "appPage.providers",
      "appPage.workspace",
      "appPage.openclawConfig",
    ]);
    expect(pageTabs[0]).toHaveAttribute("aria-selected", "true");
    expect(
      screen.queryByRole("group", { name: "OpenClaw", hidden: true }),
    ).not.toBeInTheDocument();

    fireEvent.click(pageTabs[2]);
    const configTabs = await screen.findByRole("tablist", {
      name: "appPage.openclawConfig",
      hidden: true,
    });
    const subTabs = within(configTabs).getAllByRole("tab", { hidden: true });
    expect(subTabs).toHaveLength(3);
    expect(subTabs[0]).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel", { hidden: true })).toHaveAttribute(
      "aria-labelledby",
      "openclaw-config-env",
    );

    fireEvent.click(sidebarApp("Hermes"));
    const hermesTabs = await screen.findByRole("tablist", {
      name: "Hermes",
      hidden: true,
    });
    const hermesPageTabs = within(hermesTabs).getAllByRole("tab", {
      hidden: true,
    });
    expect(hermesPageTabs.map((tab) => tab.textContent)).toEqual([
      "appPage.providers",
      "appPage.memory",
    ]);

    // 记忆页：页头的 solid 主操作换成「保存」，两份记忆用二级页签
    fireEvent.click(hermesPageTabs[1]);
    const header = document.querySelector("header") as HTMLElement;
    const save = await within(header).findByRole("button", {
      name: "common.save",
      hidden: true,
    });
    expect(save).toHaveAttribute("aria-disabled", "true");
    expect(
      within(header).queryByRole("button", {
        name: /provider.addProvider/,
        hidden: true,
      }),
    ).not.toBeInTheDocument();
    const memoryTabs = await screen.findByRole("tablist", {
      name: "hermes.memory.fileTabs",
      hidden: true,
    });
    expect(
      within(memoryTabs).getAllByRole("tab", { hidden: true }),
    ).toHaveLength(2);
    await waitFor(() =>
      expect(
        screen.getAllByRole("textbox", {
          name: "hermes.memory.editorLabel",
          hidden: true,
        })[0],
      ).toHaveValue("agent notes"),
    );
  });

  it("opens the old skillsDiscovery view as the Discover segment", async () => {
    localStorage.setItem("cc-switch-last-view", "skillsDiscovery");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(await screen.findByTestId("unified-skills-panel")).toHaveTextContent(
      "discover",
    );
  });
});
