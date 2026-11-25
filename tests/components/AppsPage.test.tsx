import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DndContextProps, DragEndEvent } from "@dnd-kit/core";
import { APP_IDS } from "@/config/appSpecs";
import {
  card,
  createToolState,
  deferred,
  isReady,
  mountApps,
  registerScrollIntoViewStub,
  renderApps,
  report,
  resetAppsPageMocks,
  updateAllButton,
  updateButton,
} from "./appsPageTestHelpers";

const mocks = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  getConfigStatus: vi.fn(async () => ({
    exists: true,
    path: "/home/me/.workbuddy",
  })),
  probeToolInstallations: vi.fn(),
  listToolInstallations: vi.fn(),
  runToolLifecycleAction: vi.fn(),
  prepareToolUninstall: vi.fn(),
  copyText: vi.fn(),
  info: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  settings: {} as {
    visibleApps?: Record<string, boolean>;
    appOrder?: string[];
  },
  updateSettings: vi.fn(),
  autoSaveSettings: vi.fn(async () => null),
  dragEndHandlers: [] as ((event: DragEndEvent) => void)[],
  desktopStatus: { supported: true, installed: true, configured: true },
}));

vi.mock("@/lib/api", () => ({ settingsApi: mocks }));
vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopStatus: async () => mocks.desktopStatus,
  },
}));
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: mocks.settings,
    updateSettings: mocks.updateSettings,
    autoSaveSettings: mocks.autoSaveSettings,
  }),
}));
vi.mock("@dnd-kit/core", async () => {
  const actual =
    await vi.importActual<typeof import("@dnd-kit/core")>("@dnd-kit/core");
  return {
    ...actual,
    DndContext: (props: DndContextProps) => {
      if (props.onDragEnd) mocks.dragEndHandlers.push(props.onDragEnd);
      return <actual.DndContext {...props} />;
    },
  };
});
vi.mock("sonner", () => ({ toast: mocks }));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));

const state = createToolState();
const { outdated, missing } = state;

/** 某一组里各行的应用 ID，按显示顺序。 */
function rowApps(region: string) {
  return Array.from(
    screen
      .getByRole("region", { name: region })
      .querySelectorAll<HTMLElement>("[data-tool-row]"),
    (row) => row.dataset.toolRow,
  );
}

registerScrollIntoViewStub();

beforeEach(() => {
  resetAppsPageMocks(mocks, state);
});

describe("AppsPage layout, grouping and diagnostics", () => {
  it("waits for Codex loading even while the check-updates label is hidden in the DOM", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    const codexReady = deferred<void>();
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      if (tools.includes("codex")) await codexReady.promise;
      return getVersions(tools);
    });
    // Finish the first dynamic import before starting a DOM wait. Under the full
    // suite, transforming AppsPage can take longer than findByText's timeout.
    await import("@/components/apps/AppsPage");
    const loading = renderApps();
    expect(await screen.findByText("appsPage.checkUpdates")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(
      screen.queryByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "appsPage.checking" }),
    ).toBeDisabled();
    await act(async () => codexReady.resolve());
    await loading;
    expect(updateButton("Codex")).toBeEnabled();
  });

  it("offers uninstall for installed tools and keeps missing tools installable", async () => {
    missing.add("codex");
    await renderApps();
    expect(
      card("Claude Code").getByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeEnabled();
    expect(
      card("OpenCode").getByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeEnabled();
    expect(
      card("Codex").queryByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeNull();
    expect(
      card("Codex").getByRole("button", { name: "settings.toolInstall" }),
    ).toBeEnabled();
  });

  it("groups apps into installed and not installed sections", async () => {
    missing.add("codex");
    await renderApps();
    const installed = within(
      screen.getByRole("region", { name: "appsPage.sectionInstalled" }),
    );
    const notInstalled = within(
      screen.getByRole("region", { name: "appsPage.sectionNotInstalled" }),
    );
    expect(installed.getByText("Claude Code")).toBeInTheDocument();
    expect(installed.getByText("Claude Desktop")).toBeInTheDocument();
    expect(installed.queryByText("Codex")).toBeNull();
    expect(notInstalled.getByText("Codex")).toBeInTheDocument();
    expect(notInstalled.queryByText("Claude Code")).toBeNull();
  });

  it("groups Claude Desktop by whether it is installed, not whether it is connected", async () => {
    mocks.desktopStatus = {
      supported: true,
      installed: true,
      configured: false,
    };
    await renderApps();
    expect(rowApps("appsPage.sectionInstalled")).toContain("claude-desktop");
    expect(
      card("Claude Desktop").getByText("appsPage.desktopNotConnected"),
    ).toBeInTheDocument();
  });

  it("puts Claude Desktop under not installed when it is not on this machine", async () => {
    mocks.desktopStatus = {
      supported: true,
      installed: false,
      configured: false,
    };
    await renderApps();
    expect(rowApps("appsPage.sectionNotInstalled")).toEqual(["claude-desktop"]);
  });

  it("orders rows by the saved app order within each section", async () => {
    missing.add("codex").add("hermes");
    mocks.settings = {
      appOrder: [
        "hermes",
        "gemini",
        "claude",
        "claude-desktop",
        "codex",
        ...APP_IDS.filter(
          (app) =>
            !["hermes", "gemini", "claude", "claude-desktop", "codex"].includes(
              app,
            ),
        ),
      ],
    };
    await renderApps();
    expect(rowApps("appsPage.sectionInstalled").slice(0, 4)).toEqual([
      "gemini",
      "claude",
      "claude-desktop",
      "grokbuild",
    ]);
    expect(rowApps("appsPage.sectionNotInstalled")).toEqual([
      "hermes",
      "codex",
    ]);
    // 只有已安装的能拖
    expect(
      within(
        screen.getByRole("region", { name: "appsPage.sectionInstalled" }),
      ).getAllByRole("button", { name: "appsPage.dragHandle" }),
    ).toHaveLength(APP_IDS.length - 2);
    expect(
      within(
        screen.getByRole("region", { name: "appsPage.sectionNotInstalled" }),
      ).queryByRole("button", { name: "appsPage.dragHandle" }),
    ).toBeNull();
  });

  it("saves the new order when a row is dropped onto another in its section", async () => {
    await renderApps();
    // jsdom 不做布局，算不出拖动落点：直接调各组 DndContext 收到的 onDragEnd
    const onDragEnd = mocks.dragEndHandlers.at(-1);
    expect(onDragEnd).toBeDefined();
    act(() => {
      onDragEnd?.({
        active: { id: "claude" },
        over: { id: "codex" },
      } as unknown as DragEndEvent);
    });
    const next = [
      "claude-desktop",
      "codex",
      "claude",
      ...APP_IDS.filter(
        (app) => !["claude", "claude-desktop", "codex"].includes(app),
      ),
    ];
    expect(mocks.updateSettings).toHaveBeenCalledWith({ appOrder: next });
    expect(mocks.autoSaveSettings).toHaveBeenCalledWith({ appOrder: next });
  });

  it("keeps the header steady while checking for updates", async () => {
    await renderApps();
    expect(updateAllButton()).toBeEnabled();
    const pending = deferred<void>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      await pending.promise;
      return getVersions(tools);
    });
    fireEvent.click(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    );
    // 检查期间各工具都在加载：「全部升级」保持显示（禁用），不先消失再出现
    expect(
      await screen.findByRole("button", { name: "appsPage.checking" }),
    ).toBeDisabled();
    expect(updateAllButton()).toBeDisabled();
    await act(async () => pending.resolve());
    expect(updateAllButton()).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeEnabled();
  });

  it("lists CodeBuddy Code last, like the sidebar, and upgrades it on its own", async () => {
    outdated.add("codebuddy");
    const view = await renderApps();
    const names = Array.from(
      view.container.querySelectorAll("[data-tool-row]"),
      (row) => row.textContent ?? "",
    );
    const codebuddy = names.findIndex((name) =>
      name.includes("CodeBuddy Code"),
    );
    expect(codebuddy).toBe(names.length - 1);
    expect(names[codebuddy - 1]).toContain("WorkBuddy");
    expect(card("CodeBuddy Code").getByText("1.0.0")).toBeInTheDocument();

    fireEvent.click(updateButton("CodeBuddy Code"));
    await waitFor(() => expect(isReady("CodeBuddy Code")).toBe(true));
    expect(mocks.probeToolInstallations).toHaveBeenCalledWith(["codebuddy"]);
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["codebuddy"]]);
  });

  it("offers to install CodeBuddy Code when it is missing", async () => {
    missing.add("codebuddy");
    await renderApps();
    expect(
      card("CodeBuddy Code").queryByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeNull();
    fireEvent.click(
      card("CodeBuddy Code").getByRole("button", {
        name: "settings.toolInstall",
      }),
    );
    await waitFor(() =>
      expect(
        mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
      ).toEqual([["codebuddy"]]),
    );
  });

  it("withholds the upgrade when the new version needs a newer Node and leaves it out of update all", async () => {
    const defaultVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      (await defaultVersions(tools)).map(
        (tool: { name: string; version: string | null }) => ({
          ...tool,
          update_blocked_by_node:
            tool.name === "claude" && tool.version === "1.0.0"
              ? { required: ">=24.16.0 <25 || >=26.1.0", current: "22.23.2" }
              : null,
        }),
      ),
    );
    await renderApps();
    await waitFor(() =>
      expect(
        card("Claude Code").getByText("appsPage.updateNeedsNode"),
      ).toBeInTheDocument(),
    );
    expect(
      card("Claude Code").queryByRole("button", {
        name: "settings.toolUpdate",
      }),
    ).toBeNull();
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(isReady("Codex")).toBe(true));
    expect(isReady("Gemini CLI")).toBe(true);
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["codex"], ["gemini"]]);
  });

  it("shows diagnosed conflicts on the row, refreshes the install report and says so", async () => {
    vi.resetModules();
    const install = (path: string, isDefault: boolean) => ({
      path,
      version: "2.0.0",
      runnable: true,
      error: null,
      source: "npm",
      is_path_default: isDefault,
    });
    // 打开页面时只有一处安装；之后外部又装了一份，诊断才发现
    mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) =>
        tool === "claude"
          ? report(tool, {
              is_conflict: true,
              installs: [
                install("/usr/local/bin/claude", true),
                install("/opt/homebrew/bin/claude", false),
              ],
            })
          : report(tool),
      ),
    );
    await renderApps();
    expect(screen.queryByText("/opt/homebrew/bin/claude")).toBeNull();

    await userEvent.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "settings.toolDiagnose" }),
    );

    // 冲突列表直接在行里展开，入口「另有 1 处安装」也刷新出来，并有一条提示
    expect(
      await card("Claude Code").findByText("/opt/homebrew/bin/claude"),
    ).toBeInTheDocument();
    expect(
      card("Claude Code").getByRole("button", {
        name: /appsPage\.otherInstalls/,
      }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(mocks.warning).toHaveBeenCalledWith(
      "settings.toolDiagnoseConflicts",
      expect.anything(),
    );
    expect(mocks.info).not.toHaveBeenCalled();
  });

  it("reuses an in-flight background tool check without duplicate probes and renders completed cards immediately", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    const codexReady = deferred<void>();
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      if (tools.includes("codex")) await codexReady.promise;
      return getVersions(tools);
    });
    // 安装清单与版本结果一致（都装在 PATH 默认处），版本先到的卡片不会换组重挂
    mocks.listToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => ({
        tool,
        installs: [
          {
            path: `/usr/local/bin/${tool}`,
            source: "system",
            is_path_default: true,
          },
        ],
        is_wsl: false,
      })),
    );
    const [{ TOOL_NAMES, checkToolUpdatesInBackground }, { AppsPage }] =
      await Promise.all([
        import("@/components/apps/useToolManagement"),
        import("@/components/apps/AppsPage"),
      ]);

    const bgFirst = checkToolUpdatesInBackground();
    const bgSecond = checkToolUpdatesInBackground();
    expect(bgSecond).toBe(bgFirst);

    const view = mountApps(AppsPage);
    // 非阻塞工具各自返回后立刻渲染，不被慢工具（codex）拖住整页。
    // 安装清单到达后各行分组、重新挂载，所以每次重新查卡片
    await waitFor(() =>
      expect(card("Claude Code").getByText("1.0.0")).toBeInTheDocument(),
    );
    expect(card("Codex").getByRole("status")).toBeInTheDocument();
    expect(
      within(view.container).getByText("appsPage.checking"),
    ).toHaveAttribute("aria-hidden", "false");
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_NAMES.length);

    await act(async () => {
      codexReady.resolve();
      await bgFirst;
    });

    await waitFor(() =>
      expect(
        within(view.container).getByText("appsPage.checkUpdates"),
      ).toHaveAttribute("aria-hidden", "false"),
    );
    expect(card("Codex").getByText("1.0.0")).toBeInTheDocument();
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_NAMES.length);
  });

  it("falls back to a fresh page probe when the in-flight background check fails", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    const bgGate = deferred<void>();
    let failBackground = true;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      if (failBackground) {
        await bgGate.promise;
        throw new Error("background ipc failed");
      }
      return getVersions(tools);
    });
    const [{ TOOL_NAMES, checkToolUpdatesInBackground }, { AppsPage }] =
      await Promise.all([
        import("@/components/apps/useToolManagement"),
        import("@/components/apps/AppsPage"),
      ]);

    const bg = checkToolUpdatesInBackground();
    const view = mountApps(AppsPage);
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_NAMES.length);

    await act(async () => {
      failBackground = false;
      bgGate.resolve();
      await bg;
    });

    await waitFor(() =>
      expect(
        within(view.container).getByText("appsPage.checkUpdates"),
      ).toHaveAttribute("aria-hidden", "false"),
    );
    expect(await card("Claude Code").findByText("1.0.0")).toBeInTheDocument();
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_NAMES.length * 2);
  });

  it("groups rows from the local install list before version probes finish", async () => {
    const pending = deferred<void>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      await pending.promise;
      return getVersions(tools);
    });
    mocks.listToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) =>
        tool === "claude"
          ? {
              tool,
              installs: [
                {
                  path: "/usr/local/bin/claude",
                  source: "system",
                  is_path_default: true,
                },
              ],
              is_wsl: false,
            }
          : { tool, installs: [], is_wsl: false },
      ),
    );
    const { AppsPage } = await import("@/components/apps/AppsPage");
    mountApps(AppsPage);
    // 版本探测还挂着：清单里有安装的先进「已安装」，空清单的先进「未安装」，
    // 版本列仍在加载，不冒出「未安装」字样
    await waitFor(() => {
      expect(rowApps("appsPage.sectionInstalled")).toContain("claude");
      expect(rowApps("appsPage.sectionNotInstalled")).toContain("codex");
    });
    expect(card("Codex").queryByText("common.notInstalled")).toBeNull();
    await act(async () => {
      pending.resolve();
    });
    // 版本探到后按结果修正：codex 1.0.0 归回已安装并可升级
    await waitFor(() => expect(updateButton("Codex")).toBeEnabled());
    expect(rowApps("appsPage.sectionInstalled")).toContain("codex");
  });

  it("waits for the version probe before grouping a tool that runs in WSL", async () => {
    const pending = deferred<void>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      await pending.promise;
      return getVersions(tools);
    });
    mocks.listToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => ({ tool, installs: [], is_wsl: tool === "gemini" })),
    );
    const { AppsPage } = await import("@/components/apps/AppsPage");
    mountApps(AppsPage);
    // 本机清单说明不了 WSL 里装没装：不把 gemini 归进「未安装」，但 shell 选择先出来
    expect(
      await card("Gemini CLI").findByText("appsPage.wslFlag"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: "appsPage.sectionNotInstalled" }),
    ).toBeNull();
    await act(async () => {
      pending.resolve();
    });
    await waitFor(() =>
      expect(rowApps("appsPage.sectionInstalled")).toContain("gemini"),
    );
  });

  it("shares one install listing between quick remounts", async () => {
    const gate = deferred<void>();
    const list = mocks.listToolInstallations.getMockImplementation()!;
    mocks.listToolInstallations.mockImplementation(async (tools: string[]) => {
      await gate.promise;
      return list(tools);
    });
    const { AppsPage } = await import("@/components/apps/AppsPage");
    const first = mountApps(AppsPage);
    first.unmount();
    mountApps(AppsPage);
    await act(async () => {
      gate.resolve();
    });
    expect(mocks.listToolInstallations).toHaveBeenCalledTimes(1);
  });
});

describe("AppsPage visibility column", () => {
  it("renders the sidebar visibility switches small with the action color", async () => {
    vi.resetModules();
    await renderApps();
    const switches = screen.getAllByRole("switch", {
      name: /appsPage\.showInSidebar/,
    });
    expect(switches.length).toBeGreaterThan(0);
    for (const element of switches) {
      expect(element).toHaveAttribute("data-size", "sm");
      expect(element).not.toHaveAttribute("data-tone");
    }
  });

  it("gives WorkBuddy only the sidebar switch, like Claude Desktop", async () => {
    vi.resetModules();
    await renderApps();
    const row = screen
      .getByText("WorkBuddy", { selector: "span" })
      .closest("[data-tool-row]") as HTMLElement;
    expect(row).not.toBeNull();
    expect(within(row).getByRole("switch")).toBeInTheDocument();
    // 和 Claude Desktop 一样：接入状态 + 说明，没有操作按钮
    expect(
      await within(row).findByText("appsPage.desktopConnected"),
    ).toBeInTheDocument();
    expect(mocks.getConfigStatus).toHaveBeenCalledWith("workbuddy");
    // 除了「已安装」组的拖动把手，唯一的按钮是说明的问号
    expect(
      within(row)
        .getAllByRole("button")
        .filter(
          (button) =>
            button.getAttribute("aria-label") !== "appsPage.dragHandle",
        ),
    ).toEqual([within(row).getByRole("button", { name: "WorkBuddy" })]);
  });

  it("hides CodeBuddy Code from the sidebar with its own switch", async () => {
    vi.resetModules();
    mocks.updateSettings.mockReset();
    mocks.autoSaveSettings.mockReset().mockResolvedValue(null);
    await renderApps();
    const toggle = card("CodeBuddy Code").getByRole("switch", {
      name: /appsPage\.showInSidebar/,
    });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(mocks.updateSettings).toHaveBeenCalledWith({
      visibleApps: expect.objectContaining({ codebuddy: false, kimi: true }),
    });
    expect(mocks.autoSaveSettings).toHaveBeenCalledWith({
      visibleApps: expect.objectContaining({ codebuddy: false }),
    });
  });

  it("keeps CodeBuddy Code on when it is the last visible app", async () => {
    vi.resetModules();
    mocks.updateSettings.mockReset();
    mocks.settings = {
      visibleApps: Object.fromEntries(
        [
          "claude",
          "claude-desktop",
          "codex",
          "gemini",
          "grokbuild",
          "opencode",
          "openclaw",
          "hermes",
          "pi",
          "mcode",
          "kimi",
          "dsh",
          "agy",
          "omp",
          "copilot",
          "workbuddy",
        ].map((app) => [app, false]),
      ),
    };
    try {
      await renderApps();
      const toggle = card("CodeBuddy Code").getByRole("switch", {
        name: /appsPage\.showInSidebar/,
      });
      expect(toggle).toBeChecked();
      expect(toggle).toBeDisabled();
      fireEvent.click(toggle);
      expect(mocks.updateSettings).not.toHaveBeenCalled();
    } finally {
      mocks.settings = {};
    }
  });
});
