import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type {
  settingsApi,
  ToolInstallationReport,
  ToolUninstallCandidate,
  ToolUninstallPlan,
} from "@/lib/api/settings";
import type { DndContextProps, DragEndEvent } from "@dnd-kit/core";
import { APP_IDS } from "@/config/appConfig";

type ToolVersions = Awaited<ReturnType<typeof settingsApi.getToolVersions>>;

const mocks = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  probeToolInstallations: vi.fn(),
  listToolInstallations: vi.fn(),
  runToolLifecycleAction: vi.fn(),
  prepareToolUninstall: vi.fn(),
  copyText: vi.fn(),
  info: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  settings: {} as { visibleApps?: undefined; appOrder?: string[] },
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function report(
  tool: string,
  overrides: Partial<ToolInstallationReport> = {},
): ToolInstallationReport {
  return {
    tool,
    installs: [],
    is_conflict: false,
    needs_confirmation: false,
    command: `${tool} update`,
    anchored: true,
    unmanaged: false,
    ...overrides,
  };
}

function uninstallPlan(tool: string): ToolUninstallPlan {
  return {
    tool,
    path: `C:/tools/${tool}.cmd`,
    real_path: `C:/tools/${tool}.cmd`,
    command: `npm uninstall -g --prefix C:/tools ${tool}`,
    native: false,
    wsl_distro: null,
  };
}

function candidate(
  plan: ToolUninstallPlan | null,
  overrides: Partial<ToolUninstallCandidate> = {},
): ToolUninstallCandidate {
  return {
    path: plan?.path ?? "C:/tools/unknown.exe",
    version: "1.0.0",
    runnable: true,
    error: null,
    source: "system",
    is_path_default: true,
    plan,
    unavailable_reason: null,
    reinstalled_by_updater: false,
    ...overrides,
  };
}

const upgraded = new Set<string>();
const outdated = new Set<string>();
const missing = new Set<string>();

function card(name: string) {
  return within(
    screen
      .getByText(name, { selector: "span" })
      .closest("[data-tool-row]") as HTMLElement,
  );
}

/** 已是最新：没有升级 / 安装按钮，显示新版本号。 */
function isReady(name: string) {
  return (
    card(name).queryByRole("button", {
      name: /settings\.tool(Update|Install)/,
    }) === null && card(name).queryByText("2.0.0") !== null
  );
}

/** 某一组里各行的应用 ID，按显示顺序。 */
function rowApps(region: string) {
  return Array.from(
    screen
      .getByRole("region", { name: region })
      .querySelectorAll<HTMLElement>("[data-tool-row]"),
    (row) => row.dataset.toolRow,
  );
}

/** 行尾「⋯」菜单（卸载在里面）。 */
function rowMenu(name: string) {
  return card(name).getByRole("button", { name: "appPage.moreActions" });
}

async function openUninstall(name: string) {
  const user = userEvent.setup();
  await user.click(rowMenu(name));
  await user.click(
    await screen.findByRole("menuitem", { name: "settings.toolUninstall" }),
  );
}

function updateButton(name: string) {
  return card(name).getByRole("button", { name: "settings.toolUpdate" });
}

function mountApps(AppsPage: () => JSX.Element) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <AppsPage />
    </QueryClientProvider>,
  );
}

async function renderApps() {
  // 安装 / 升级的状态放在模块级 store 里，跨挂载保留。
  const { AppsPage } = await import("@/components/apps/AppsPage");
  const view = mountApps(AppsPage);
  // Both header labels stay mounted to reserve their width. Wait for the idle
  // label to be shown, including when a pending dialog hides the page itself.
  await waitFor(() =>
    expect(
      within(view.container).getByText("appsPage.checkUpdates"),
    ).toHaveAttribute("aria-hidden", "false"),
  );
  return view;
}

const updateAllButton = () =>
  screen.getByRole("button", { name: /settings\.updateAllTools/ });

// Radix Select scrolls the focused option; jsdom does not implement scrolling.
const originalScrollIntoView = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "scrollIntoView",
);
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});
afterAll(() => {
  if (originalScrollIntoView) {
    Object.defineProperty(
      HTMLElement.prototype,
      "scrollIntoView",
      originalScrollIntoView,
    );
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
});

describe("AppsPage concurrent CLI upgrades", () => {
  beforeEach(() => {
    vi.resetModules();
    upgraded.clear();
    outdated.clear();
    missing.clear();
    mocks.settings = {};
    mocks.dragEndHandlers = [];
    mocks.desktopStatus = {
      supported: true,
      installed: true,
      configured: true,
    };
    mocks.updateSettings.mockReset();
    mocks.autoSaveSettings.mockReset().mockResolvedValue(null);
    mocks.copyText.mockReset().mockResolvedValue(undefined);
    outdated.add("claude").add("codex").add("gemini");
    mocks.getToolVersions
      .mockReset()
      .mockImplementation(async (tools: string[]) =>
        tools.map((name) => ({
          name,
          version: missing.has(name)
            ? null
            : upgraded.has(name) || !outdated.has(name)
              ? "2.0.0"
              : "1.0.0",
          latest_version: "2.0.0",
          error: null,
          installed_but_broken: false,
          env_type: "windows",
          wsl_distro: null,
        })),
      );
    mocks.probeToolInstallations
      .mockReset()
      .mockImplementation(async (tools: string[]) =>
        tools.map((tool) => report(tool)),
      );
    mocks.listToolInstallations
      .mockReset()
      .mockImplementation(async (tools: string[]) =>
        tools.map((tool) => report(tool)),
      );
    mocks.prepareToolUninstall
      .mockReset()
      .mockImplementation(async (tool: string) => [
        candidate(uninstallPlan(tool)),
      ]);
    mocks.runToolLifecycleAction
      .mockReset()
      .mockImplementation(async ([tool]: string[]) => {
        upgraded.add(tool);
        missing.delete(tool);
      });
  });

  it("copies only the selected application's manual command and switches installation methods", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) => ({
        ...tool,
        env_type: "macos",
      })),
    );
    const user = userEvent.setup();
    await renderApps();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "settings.manualUninstallCommands",
      }),
    );
    const dialog = within(screen.getByRole("dialog"));
    const codex = () => within(dialog.getByText("Codex").closest("li")!);
    expect(
      codex().getByText("npm uninstall -g @openai/codex"),
    ).toBeInTheDocument();
    expect(dialog.queryByText("Hermes")).not.toBeInTheDocument();
    await user.click(codex().getByRole("button"));
    expect(mocks.copyText).toHaveBeenLastCalledWith(
      "npm uninstall -g @openai/codex",
    );

    expect(dialog.getByText("macOS")).toBeInTheDocument();
    expect(dialog.getAllByRole("combobox")).toHaveLength(2);
    await user.click(
      dialog.getByRole("combobox", { name: "settings.manualUninstallMethod" }),
    );
    await user.click(
      await screen.findByRole("option", { name: "Homebrew (cask)" }),
    );
    expect(
      dialog.queryByText("npm uninstall -g @openai/codex"),
    ).not.toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --cask claude-code"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --cask claude-code@latest"),
    ).toBeInTheDocument();
    await user.click(codex().getByRole("button"));
    expect(mocks.copyText).toHaveBeenLastCalledWith(
      "brew uninstall --cask codex",
    );
    expect(mocks.success).toHaveBeenCalledWith(
      "settings.uninstallCommandCopied",
      { closeButton: true },
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(dialog.getByRole("button", { name: "common.close" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "appsPage.manualCommands" }),
    );
    expect(
      within(screen.getByRole("dialog")).getByRole("heading", {
        name: "settings.manualInstallCommands",
      }),
    ).toBeInTheDocument();
  });

  it.each([["Windows NT 10.0"], ["X11; Linux x86_64"]])(
    "shows the official Hermes uninstall command for %s",
    async (userAgent) => {
      const platform = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      try {
        const user = userEvent.setup();
        await renderApps();
        await user.click(
          screen.getByRole("button", { name: "appsPage.moreActions" }),
        );
        await user.click(
          await screen.findByRole("menuitem", {
            name: "settings.manualUninstallCommands",
          }),
        );
        const dialog = within(screen.getByRole("dialog"));
        await user.click(
          dialog.getByRole("combobox", {
            name: "settings.manualUninstallMethod",
          }),
        );
        await user.click(await screen.findByRole("option", { name: "Hermes" }));
        expect(
          within(dialog.getByRole("list")).getByText("Hermes"),
        ).toBeInTheDocument();
        expect(dialog.queryByText("Codex")).not.toBeInTheDocument();
        await user.click(
          dialog.getByRole("button", {
            name: "settings.copyToolUninstallCommand",
          }),
        );
        expect(mocks.copyText).toHaveBeenCalledWith("hermes uninstall --yes");
        expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
      } finally {
        platform.mockRestore();
      }
    },
  );

  it("keeps manual commands visible when copying fails", async () => {
    mocks.copyText.mockRejectedValueOnce(new Error("clipboard unavailable"));
    const user = userEvent.setup();
    await renderApps();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "settings.manualUninstallCommands",
      }),
    );
    const dialog = within(screen.getByRole("dialog"));
    const codex = within(dialog.getByText("Codex").closest("li")!);
    await user.click(codex.getByRole("button"));
    expect(mocks.error).toHaveBeenCalledWith(
      "settings.uninstallCommandCopyFailed",
    );
    expect(
      codex.getByText("npm uninstall -g @openai/codex"),
    ).toBeInTheDocument();
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("automatically detects Windows and only shows documented pnpm and Bun clients", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const user = userEvent.setup();
      await renderApps();
      await user.click(
        screen.getByRole("button", { name: "appsPage.moreActions" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "settings.manualUninstallCommands",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(dialog.getByText("Windows")).toBeInTheDocument();
      expect(dialog.getAllByRole("combobox")).toHaveLength(2);
      const chooseMethod = async (name: string) => {
        await user.click(
          dialog.getByRole("combobox", {
            name: "settings.manualUninstallMethod",
          }),
        );
        expect(
          screen.queryByRole("option", { name: "Volta" }),
        ).not.toBeInTheDocument();
        await user.click(await screen.findByRole("option", { name }));
      };
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      expect(
        screen.queryByRole("option", { name: "Homebrew (cask)" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("option", { name: "Homebrew (formula)" }),
      ).not.toBeInTheDocument();
      await user.click(await screen.findByRole("option", { name: "pnpm" }));
      expect(dialog.getAllByRole("listitem")).toHaveLength(2);
      expect(
        dialog.getByText("pnpm remove -g opencode-ai"),
      ).toBeInTheDocument();
      expect(dialog.getByText("pnpm remove -g openclaw")).toBeInTheDocument();
      expect(dialog.queryByText("Codex")).not.toBeInTheDocument();
      await chooseMethod("bun");
      expect(dialog.getAllByRole("listitem")).toHaveLength(2);
      expect(dialog.getByText("bun remove -g openclaw")).toBeInTheDocument();
      expect(
        dialog.getByText("bun remove -g @oh-my-pi/pi-coding-agent"),
      ).toBeInTheDocument();
      expect(
        dialog.queryByText("bun remove -g opencode-ai"),
      ).not.toBeInTheDocument();
      expect(
        dialog.getByText("settings.manualUninstallOpenClawHint"),
      ).toBeInTheDocument();
      expect(
        dialog.getByText("settings.manualUninstallBunLauncherHint"),
      ).toBeInTheDocument();

      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it.each([
    ["updates the detected environments and per-client WSL commands", false],
    ["preserves a manual environment selection during detection", true],
  ] as const)("%s", async (_name, manualOverride) => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const pending = deferred<ToolVersions>();
      const getVersions = mocks.getToolVersions.getMockImplementation()!;
      const opencode = (await getVersions(["opencode"])) as ToolVersions;
      mocks.getToolVersions.mockImplementation((tools: string[]) =>
        tools.includes("opencode") ? pending.promise : getVersions(tools),
      );
      const user = userEvent.setup();
      const { AppsPage } = await import("@/components/apps/AppsPage");
      mountApps(AppsPage);
      await user.click(
        screen.getByRole("button", { name: "appsPage.moreActions" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "settings.manualUninstallCommands",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      const environmentSelect = dialog.getByRole("combobox", {
        name: "settings.manualUninstallEnvironment",
      });
      expect(environmentSelect).toHaveTextContent("Windows");
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      await user.click(await screen.findByRole("option", { name: "bun" }));
      expect(
        dialog.queryByText("bun remove -g opencode-ai"),
      ).not.toBeInTheDocument();

      const chooseEnvironment = async (name: string) => {
        await user.click(environmentSelect);
        expect(
          (await screen.findAllByRole("option")).map(
            (option) => option.textContent,
          ),
        ).toEqual(["Windows", "macOS", "Linux / WSL"]);
        await user.click(await screen.findByRole("option", { name }));
      };
      // Selecting the currently detected Windows environment explicitly makes
      // it an override; the pending WSL detection must not replace that choice.
      if (manualOverride) await chooseEnvironment("Windows");
      await act(async () =>
        pending.resolve(
          opencode.map((tool) => ({
            ...tool,
            env_type: "wsl",
            wsl_distro: "Ubuntu",
          })),
        ),
      );
      if (manualOverride) {
        expect(environmentSelect).toHaveTextContent(/^Windows$/);
        expect(
          dialog.queryByText("bun remove -g opencode-ai"),
        ).not.toBeInTheDocument();
      } else {
        expect(environmentSelect).toHaveTextContent("Windows / WSL · Ubuntu");
        const opencodeRow = within(
          dialog.getByText("bun remove -g opencode-ai").closest("li")!,
        );
        expect(opencodeRow.getByText("WSL · Ubuntu")).toBeInTheDocument();
        const openclawRow = within(
          dialog.getByText("bun remove -g openclaw").closest("li")!,
        );
        expect(openclawRow.getByText("Windows")).toBeInTheDocument();
        expect(dialog.getAllByRole("combobox")).toHaveLength(2);

        await user.click(
          dialog.getByRole("combobox", {
            name: "settings.manualUninstallMethod",
          }),
        );
        expect(
          screen.queryByRole("option", { name: "Homebrew (cask)" }),
        ).not.toBeInTheDocument();
        await user.click(
          await screen.findByRole("option", { name: "Homebrew (formula)" }),
        );
        expect(
          dialog.getByText("brew uninstall --formula opencode"),
        ).toBeInTheDocument();
        expect(
          dialog.getByText("brew uninstall --formula anomalyco/tap/opencode"),
        ).toBeInTheDocument();
        expect(
          dialog.queryByText("brew uninstall --formula gemini-cli"),
        ).not.toBeInTheDocument();
      }

      await chooseEnvironment("macOS");
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      await user.click(
        await screen.findByRole("option", { name: "Homebrew (cask)" }),
      );
      expect(
        dialog.getByText("brew uninstall --cask codex"),
      ).toBeInTheDocument();
      expect(dialog.queryByText("WSL · Ubuntu")).not.toBeInTheDocument();

      // A manual switch also drops methods unsupported in the new environment.
      await chooseEnvironment("Windows");
      expect(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      ).toHaveTextContent("npm");
      expect(
        dialog.queryByText("brew uninstall --cask codex"),
      ).not.toBeInTheDocument();
      await chooseEnvironment("Linux / WSL");
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      await user.click(await screen.findByRole("option", { name: "bun" }));
      expect(dialog.getByText("bun remove -g opencode-ai")).toBeInTheDocument();
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
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

  it("shows the selected path and command while retaining the resolved target for uninstall validation", async () => {
    const pending = deferred<ToolUninstallCandidate[]>();
    mocks.prepareToolUninstall.mockReturnValueOnce(pending.promise);
    await renderApps();
    await openUninstall("Claude Code");
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByRole("status")).toHaveTextContent(
      "settings.toolUninstallPreparing",
    );
    expect(
      dialog.queryByRole("button", { name: "settings.toolUninstall" }),
    ).toBeNull();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    const plan = {
      ...uninstallPlan("claude"),
      real_path: "C:/Users/me/.local/share/claude/versions/2.0.0",
      native: true,
    };
    await act(async () => pending.resolve([candidate(plan)]));
    expect(dialog.getByText(plan.path)).toBeInTheDocument();
    expect(dialog.queryByText(plan.real_path)).not.toBeInTheDocument();
    expect(dialog.getByText(plan.command)).toBeInTheDocument();
    expect(
      dialog.getByText("settings.toolUninstallNativeHint"),
    ).toBeInTheDocument();
    expect(mocks.prepareToolUninstall).toHaveBeenCalledWith("claude", {});
    fireEvent.click(
      dialog.getByRole("button", { name: "settings.toolUninstall" }),
    );
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["claude"],
        "uninstall",
        {},
        plan,
      ),
    );
  });

  it("offers only manual uninstall when a plan cannot be prepared", async () => {
    mocks.prepareToolUninstall.mockRejectedValueOnce(
      new Error("Unverified Homebrew owner: mise"),
    );
    await renderApps();
    await openUninstall("Codex");
    const dialog = within(screen.getByRole("dialog"));
    expect(await dialog.findByRole("alert")).toHaveTextContent(
      "settings.toolUninstallManualRequired",
    );
    expect(
      dialog.getByText("Unverified Homebrew owner: mise"),
    ).toBeInTheDocument();
    expect(
      dialog.queryByRole("button", { name: "settings.toolUninstall" }),
    ).toBeNull();
    fireEvent.click(
      dialog.getByRole("button", { name: "settings.manualUninstallCommands" }),
    );
    expect(
      screen.getByRole("heading", { name: "settings.manualUninstallCommands" }),
    ).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("lists every installation and uninstalls only the one the user picks", async () => {
    const native = {
      ...uninstallPlan("grok"),
      path: "/Users/me/.grok/bin/grok",
      real_path: "/Users/me/.grok/bin/grok-1.0.50",
      command:
        "rm -- '/Users/me/.grok/bin/grok' '/Users/me/.grok/bin/grok-1.0.50'",
      native: true,
    };
    const npm = {
      ...uninstallPlan("grok"),
      path: "/Users/me/.nvm/versions/node/v22/bin/grok",
      command:
        "npm uninstall -g --prefix /Users/me/.nvm/versions/node/v22 @xai-official/grok",
    };
    mocks.prepareToolUninstall.mockResolvedValueOnce([
      candidate(native, { version: "1.0.50" }),
      candidate(npm, {
        version: "1.0.50",
        source: "nvm",
        is_path_default: false,
        reinstalled_by_updater: true,
      }),
      candidate(null, {
        path: "/opt/tools/grok",
        version: "0.9.0",
        is_path_default: false,
        unavailable_reason:
          "Cannot automatically uninstall Grok Build at /opt/tools/grok.",
      }),
    ]);
    await renderApps();
    await openUninstall("Grok Build");
    const dialog = within(screen.getByRole("dialog"));
    const npmRadio = await dialog.findByRole("radio", { name: npm.path });
    const nativeRadio = dialog.getByRole("radio", { name: native.path });
    const manualRadio = dialog.getByRole("radio", { name: "/opt/tools/grok" });
    expect(nativeRadio).not.toBeChecked();
    expect(npmRadio).not.toBeChecked();
    expect(manualRadio).toBeDisabled();
    expect(
      dialog.getByText(
        "Cannot automatically uninstall Grok Build at /opt/tools/grok.",
      ),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("settings.toolUninstallReinstalledByUpdater"),
    ).toBeInTheDocument();
    expect(
      dialog.getByRole("button", { name: "settings.manualUninstallCommands" }),
    ).toBeInTheDocument();
    const confirm = dialog.getByRole("button", {
      name: "settings.toolUninstall",
    });
    expect(confirm).toBeDisabled();
    expect(dialog.queryByText(native.command)).toBeNull();

    fireEvent.click(npmRadio);
    expect(dialog.getByText(npm.command)).toBeInTheDocument();
    expect(dialog.queryByText("settings.toolUninstallNativeHint")).toBeNull();
    fireEvent.click(nativeRadio);
    expect(dialog.getByText(native.command)).toBeInTheDocument();
    expect(
      dialog.getByText("settings.toolUninstallNativeHint"),
    ).toBeInTheDocument();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["grok"],
        "uninstall",
        {},
        native,
      ),
    );
  });

  it("discards a cancelled preview instead of showing it for another tool", async () => {
    const first = deferred<ToolUninstallCandidate[]>();
    mocks.prepareToolUninstall.mockReturnValueOnce(first.promise);
    await renderApps();
    await openUninstall("Claude Code");
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    await openUninstall("Codex");
    const dialog = within(screen.getByRole("dialog"));
    expect(
      await dialog.findByText(uninstallPlan("codex").path),
    ).toBeInTheDocument();
    await act(async () => first.resolve([candidate(uninstallPlan("claude"))]));
    expect(dialog.queryByText(uninstallPlan("claude").path)).toBeNull();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("only offers valid Homebrew formula uninstall commands", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) => ({
        ...tool,
        env_type: "linux",
      })),
    );
    const user = userEvent.setup();
    await renderApps();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "settings.manualUninstallCommands",
      }),
    );
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByText("Linux")).toBeInTheDocument();
    await user.click(
      dialog.getByRole("combobox", { name: "settings.manualUninstallMethod" }),
    );
    await user.click(
      await screen.findByRole("option", { name: "Homebrew (formula)" }),
    );
    expect(
      dialog.getByText("brew uninstall --formula gemini-cli"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --formula opencode"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --formula anomalyco/tap/opencode"),
    ).toBeInTheDocument();
    expect(dialog.queryByText("Codex")).toBeNull();
    expect(dialog.queryByText("Claude Code")).toBeNull();
    expect(dialog.queryByText("Grok Build")).toBeNull();
  });

  it("confirms uninstall, preserves the running task across navigation and refreshes to install", async () => {
    const running = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await running.promise;
        missing.add(tool);
      },
    );
    const view = await renderApps();
    const uninstall = () => rowMenu("Claude Code");
    await openUninstall("Claude Code");
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await openUninstall("Claude Code");
    fireEvent.click(
      await within(screen.getByRole("dialog")).findByRole("button", {
        name: "settings.toolUninstall",
      }),
    );
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["claude"],
      "uninstall",
      {},
      uninstallPlan("claude"),
    );
    expect(uninstall()).toBeDisabled();
    expect(uninstall()).toHaveAttribute("aria-busy", "true");
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Codex")).toBeEnabled();
    view.unmount();
    await renderApps();
    expect(uninstall()).toBeDisabled();
    expect(uninstall()).toHaveAttribute("aria-busy", "true");
    await act(async () => running.resolve());
    expect(
      card("Claude Code").queryByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeNull();
    expect(
      card("Claude Code").getByRole("button", { name: "settings.toolInstall" }),
    ).toBeEnabled();
    expect(
      card("Claude Code").getByText("common.notInstalled"),
    ).toBeInTheDocument();
    expect(mocks.probeToolInstallations).toHaveBeenCalledWith(["claude"]);
    expect(mocks.success).toHaveBeenCalledOnce();
    expect(mocks.warning).not.toHaveBeenCalled();
  });

  it("keeps uninstall available for a broken installation", async () => {
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      tools.map((name) => ({
        name,
        version: null,
        latest_version: "2.0.0",
        error: "cannot run",
        installed_but_broken: true,
        env_type: "windows",
        wsl_distro: null,
      })),
    );
    await renderApps();
    expect(
      card("Claude Code").getByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeEnabled();
    expect(
      card("Claude Code").queryByRole("button", {
        name: "settings.toolInstall",
      }),
    ).toBeNull();
  });

  it("reports uninstall failure and unlocks the tool for retry", async () => {
    mocks.runToolLifecycleAction.mockRejectedValueOnce(
      new Error("permission denied"),
    );
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await renderApps();
      await openUninstall("Claude Code");
      fireEvent.click(
        await within(screen.getByRole("dialog")).findByRole("button", {
          name: "settings.toolUninstall",
        }),
      );
      await waitFor(() =>
        expect(mocks.error).toHaveBeenCalledWith(
          "settings.toolUninstallFailed",
          {
            description: "permission denied",
            closeButton: true,
          },
        ),
      );
      expect(
        card("Claude Code").getByRole("button", {
          name: "appPage.moreActions",
        }),
      ).toBeEnabled();
      expect(updateButton("Claude Code")).toBeEnabled();
      expect(mocks.success).not.toHaveBeenCalled();
    } finally {
      errorLog.mockRestore();
    }
  });

  it.each(["rejected", "empty"])(
    "warns when uninstall succeeds but installation diagnostics are %s",
    async (result) => {
      const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
      try {
        await renderApps();
        mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
          missing.add("claude");
          if (result === "rejected") {
            mocks.probeToolInstallations.mockRejectedValueOnce(
              new Error("diagnostics failed"),
            );
          } else {
            mocks.probeToolInstallations.mockResolvedValueOnce([]);
          }
        });
        await openUninstall("Claude Code");
        fireEvent.click(
          await within(screen.getByRole("dialog")).findByRole("button", {
            name: "settings.toolUninstall",
          }),
        );
        await waitFor(() =>
          expect(mocks.warning).toHaveBeenCalledWith(
            "settings.toolUninstallWarningTitle",
            {
              description: "settings.toolUninstallCheckFailed",
              closeButton: true,
            },
          ),
        );
        expect(mocks.error).not.toHaveBeenCalled();
        expect(mocks.success).not.toHaveBeenCalled();
        expect(
          card("Claude Code").getByRole("button", {
            name: "settings.toolInstall",
          }),
        ).toBeEnabled();
      } finally {
        errorLog.mockRestore();
      }
    },
  );

  it.each(["rejected", "empty"])(
    "warns when uninstall succeeds but the status refresh is %s",
    async (result) => {
      const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
      try {
        await renderApps();
        mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
          if (result === "rejected") {
            mocks.getToolVersions.mockRejectedValueOnce(
              new Error("refresh failed"),
            );
          } else {
            mocks.getToolVersions.mockResolvedValueOnce([]);
          }
        });
        await openUninstall("Claude Code");
        fireEvent.click(
          await within(screen.getByRole("dialog")).findByRole("button", {
            name: "settings.toolUninstall",
          }),
        );
        await waitFor(() =>
          expect(mocks.warning).toHaveBeenCalledWith(
            "settings.toolUninstallWarningTitle",
            {
              description: "settings.toolUninstallCheckFailed",
              closeButton: true,
            },
          ),
        );
        expect(mocks.error).not.toHaveBeenCalled();
        expect(mocks.success).not.toHaveBeenCalled();
        expect(
          card("Claude Code").getByRole("button", {
            name: "appPage.moreActions",
          }),
        ).toBeEnabled();
      } finally {
        errorLog.mockRestore();
      }
    },
  );

  it.each([
    ["Windows NT 10.0", null, true],
    ["Windows NT 10.0", "Ubuntu", false],
    ["X11; Linux x86_64", null, false],
  ])(
    "warns about Hermes leftovers only on native Windows (%s, %s)",
    async (userAgent, distro, warn) => {
      const platform = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      try {
        mocks.prepareToolUninstall.mockResolvedValueOnce([
          candidate({
            ...uninstallPlan("hermes"),
            command: "hermes uninstall --yes",
            wsl_distro: distro,
          }),
        ]);
        mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
          missing.add("hermes");
        });
        await renderApps();
        await openUninstall("Hermes");
        const dialog = within(screen.getByRole("dialog"));
        const confirm = await dialog.findByRole("button", {
          name: "settings.toolUninstall",
        });
        expect(
          dialog.queryByText("settings.toolUninstallHermesWindowsHint") !==
            null,
        ).toBe(warn);
        expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
        fireEvent.click(confirm);
        await waitFor(() =>
          expect(
            card("Hermes").getByRole("button", {
              name: "settings.toolInstall",
            }),
          ).toBeEnabled(),
        );
        if (warn) {
          expect(mocks.warning).toHaveBeenCalledWith(
            "settings.toolUninstallWarningTitle",
            {
              description: "settings.toolUninstallHermesWindowsHint",
              closeButton: true,
            },
          );
          expect(mocks.success).not.toHaveBeenCalled();
        } else {
          expect(mocks.warning).not.toHaveBeenCalled();
          expect(mocks.success).toHaveBeenCalledOnce();
        }
        expect(mocks.error).not.toHaveBeenCalled();
      } finally {
        platform.mockRestore();
      }
    },
  );

  it("refreshes the remaining installation after uninstall instead of reporting a broken install", async () => {
    await renderApps();
    mocks.probeToolInstallations.mockResolvedValueOnce([
      report("claude", {
        installs: [
          {
            path: "C:/remaining/claude.exe",
            version: "2.0.0",
            runnable: true,
            error: null,
            source: "system",
            is_path_default: true,
          },
        ],
      }),
    ]);
    await openUninstall("Claude Code");
    fireEvent.click(
      await within(screen.getByRole("dialog")).findByRole("button", {
        name: "settings.toolUninstall",
      }),
    );
    await waitFor(() =>
      expect(mocks.info).toHaveBeenCalledWith(
        "settings.toolUninstallRemaining",
        {
          description: "Claude Code",
          closeButton: true,
        },
      ),
    );
    expect(
      card("Claude Code").getByText("C:/remaining/claude.exe"),
    ).toBeInTheDocument();
    expect(
      card("Claude Code").getByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeEnabled();
    expect(mocks.warning).not.toHaveBeenCalled();
  });

  it("lets different tools preflight and submit together while blocking duplicate clicks", async () => {
    const claudeProbe = deferred<ToolInstallationReport[]>();
    const codexProbe = deferred<ToolInstallationReport[]>();
    const claudeRun = deferred<void>();
    const codexRun = deferred<void>();
    mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
      tool === "claude" ? claudeProbe.promise : codexProbe.promise,
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await (tool === "claude" ? claudeRun.promise : codexRun.promise);
        upgraded.add(tool);
      },
    );
    await renderApps();

    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Claude Code"));
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Codex")).toBeEnabled();
    fireEvent.click(updateButton("Codex"));
    expect(mocks.probeToolInstallations.mock.calls).toEqual([
      [["claude"]],
      [["codex"]],
    ]);

    await act(async () => {
      claudeProbe.resolve([report("claude")]);
      codexProbe.resolve([report("codex")]);
    });
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(2);
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Codex")).toBeDisabled();
    expect(updateButton("Gemini CLI")).toBeEnabled();

    await act(async () => codexRun.resolve());
    expect(isReady("Codex")).toBe(true);
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
    expect(isReady("Claude Code")).toBe(true);
    expect(mocks.success).toHaveBeenCalledTimes(2);
  });

  it("submits the remaining tools while a single upgrade is already running", async () => {
    const runs = new Map(
      ["claude", "codex", "gemini"].map((name) => [name, deferred<void>()]),
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await runs.get(tool)!.promise;
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    const updateAll = updateAllButton();
    expect(updateAll).toBeEnabled();
    fireEvent.click(updateAll);
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    expect(mocks.probeToolInstallations).toHaveBeenNthCalledWith(2, [
      "codex",
      "gemini",
    ]);
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["claude"], ["codex"], ["gemini"]]);

    await act(async () => {
      runs.get("gemini")!.resolve();
      runs.get("codex")!.resolve();
    });
    expect(isReady("Gemini CLI")).toBe(true);
    expect(isReady("Codex")).toBe(true);
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => runs.get("claude")!.resolve());
  });

  it.each(["install", "update"] as const)(
    "restores an ongoing %s after remount and receives its completion",
    async (action) => {
      if (action === "install") missing.add("claude");
      const running = deferred<void>();
      mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
        await running.promise;
        upgraded.add("claude");
        missing.delete("claude");
      });
      const actionButton = () =>
        card("Claude Code").getByRole("button", {
          name:
            action === "install"
              ? "settings.toolInstall"
              : "settings.toolUpdate",
        });
      const view = await renderApps();
      fireEvent.click(actionButton());
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
      );
      view.unmount();
      await renderApps();
      const versionChecks = mocks.getToolVersions.mock.calls.length;
      expect(actionButton()).toBeDisabled();
      expect(actionButton()).toHaveAttribute("aria-busy", "true");
      expect(updateButton("Codex")).toBeEnabled();
      fireEvent.click(actionButton());
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
      expect(mocks.info).not.toHaveBeenCalled();
      expect(mocks.error).not.toHaveBeenCalled();
      expect(mocks.warning).not.toHaveBeenCalled();
      expect(mocks.success).not.toHaveBeenCalled();

      await act(async () => running.resolve());
      expect(isReady("Claude Code")).toBe(true);
      expect(
        screen.getByRole("button", { name: "appsPage.checkUpdates" }),
      ).toBeEnabled();
      expect(mocks.success).toHaveBeenCalledTimes(1);
    },
  );

  it.each(["install", "update"] as const)(
    "shows an informational toast when the backend reports an ongoing %s",
    async (action) => {
      if (action === "install") missing.add("claude");
      mocks.runToolLifecycleAction.mockRejectedValueOnce(
        "TOOL_ACTION_IN_PROGRESS",
      );
      await renderApps();
      const versionChecks = mocks.getToolVersions.mock.calls.length;
      fireEvent.click(
        card("Claude Code").getByRole("button", {
          name:
            action === "install"
              ? "settings.toolInstall"
              : "settings.toolUpdate",
        }),
      );
      await waitFor(() =>
        expect(mocks.info).toHaveBeenCalledWith(
          "settings.toolActionInProgress",
          {
            description: "settings.toolActionInProgressDetail",
            closeButton: true,
          },
        ),
      );
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
      expect(mocks.error).not.toHaveBeenCalled();
      expect(mocks.warning).not.toHaveBeenCalled();
      expect(mocks.success).not.toHaveBeenCalled();
    },
  );

  it("ignores a late probe from the old page after the remounted page upgrades a tool", async () => {
    const stale = deferred<ToolVersions>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    let firstClaudeProbe = true;
    let oldResult: ToolVersions = [];
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      if (tools.includes("claude") && firstClaudeProbe) {
        firstClaudeProbe = false;
        oldResult = await getVersions(tools);
        return stale.promise;
      }
      return getVersions(tools);
    });
    const { AppsPage } = await import("@/components/apps/AppsPage");
    const { TOOL_NAMES } = await import("@/components/apps/useToolManagement");
    const view = mountApps(AppsPage);
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_NAMES.length),
    );
    view.unmount();
    const remounted = await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() => expect(isReady("Claude Code")).toBe(true));

    await act(async () => stale.resolve(oldResult));
    expect(isReady("Claude Code")).toBe(true);
    expect(card("Claude Code").queryByText("1.0.0")).not.toBeInTheDocument();
    remounted.unmount();
    await renderApps();
    expect(isReady("Claude Code")).toBe(true);
  });

  it("preserves WSL execution and refresh parameters when confirming a mixed batch after remount", async () => {
    const preflight = deferred<ToolInstallationReport[]>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) =>
        tool.name === "claude"
          ? { ...tool, env_type: "wsl", wsl_distro: "Ubuntu" }
          : tool,
      ),
    );
    mocks.probeToolInstallations.mockImplementationOnce(
      () => preflight.promise,
    );
    const scrollIntoView = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    try {
      const user = userEvent.setup();
      const view = await renderApps();
      await user.click(card("Claude Code").getAllByRole("combobox")[0]);
      await user.click(screen.getByRole("option", { name: "bash" }));
      await waitFor(() => expect(updateButton("Claude Code")).toBeEnabled());
      await user.click(card("Claude Code").getAllByRole("combobox")[1]);
      await user.click(screen.getByRole("option", { name: "-lic" }));
      await waitFor(() => expect(updateButton("Claude Code")).toBeEnabled());
      fireEvent.click(updateAllButton());
      view.unmount();
      // WSL tools do not request confirmation themselves; a native tool in
      // the same batch can hold the whole batch pending confirmation.
      await act(async () =>
        preflight.resolve([
          report("claude"),
          report("codex", { needs_confirmation: true }),
          report("gemini"),
        ]),
      );
      await renderApps();
      mocks.getToolVersions.mockClear();
      fireEvent.click(
        screen.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
      );
      await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
      const preferences = {
        claude: { wslShell: "bash", wslShellFlag: "-lic" },
      };
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["claude"],
        "update",
        preferences,
      );
      expect(mocks.getToolVersions).toHaveBeenCalledWith(
        ["claude"],
        preferences,
      );
    } finally {
      if (scrollIntoView) {
        Object.defineProperty(
          HTMLElement.prototype,
          "scrollIntoView",
          scrollIntoView,
        );
      } else {
        Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
      }
    }
  });

  it("preserves batch progress across remounts and updates each completed tool", async () => {
    const runs = new Map(
      ["claude", "codex", "gemini"].map((name) => [name, deferred<void>()]),
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await runs.get(tool)!.promise;
        upgraded.add(tool);
      },
    );
    const view = await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    view.unmount();
    await act(async () => runs.get("codex")!.resolve());
    await renderApps();
    const updateAll = updateAllButton();
    expect(updateAll).toBeDisabled();
    expect(updateAll).toHaveAttribute("aria-busy", "true");
    expect(isReady("Codex")).toBe(true);
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    expect(updateButton("Gemini CLI")).toBeDisabled();

    await act(async () => runs.get("gemini")!.resolve());
    expect(isReady("Gemini CLI")).toBe(true);
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateAll).toHaveAttribute("aria-busy", "true");
    await act(async () => runs.get("claude")!.resolve());
    expect(isReady("Claude Code")).toBe(true);
    // 全部升级完、没有可升级的了：按钮收起
    expect(
      screen.queryByRole("button", { name: /settings\.updateAllTools/ }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeEnabled();
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3);
    expect(mocks.success).toHaveBeenCalledTimes(1);
  });

  it("preserves preflight and its confirmation when navigating away and back", async () => {
    const preflight = deferred<ToolInstallationReport[]>();
    mocks.probeToolInstallations.mockImplementationOnce(
      () => preflight.promise,
    );
    const view = await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    view.unmount();
    const remounted = await renderApps();
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    expect(mocks.probeToolInstallations).toHaveBeenCalledTimes(1);
    await act(async () =>
      preflight.resolve([report("claude", { needs_confirmation: true })]),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    remounted.unmount();
    await renderApps();
    expect(
      within(screen.getByRole("dialog")).getByText("Claude Code"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() => expect(isReady("Claude Code")).toBe(true));
  });

  it("unlocks a failed background task after remount so it can be retried", async () => {
    const running = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementationOnce(() => running.promise);
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const view = await renderApps();
      fireEvent.click(updateButton("Claude Code"));
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
      );
      view.unmount();
      await renderApps();
      expect(updateButton("Claude Code")).toBeDisabled();
      await act(async () => running.reject(new Error("installer failed")));
      expect(updateButton("Claude Code")).toBeEnabled();
      expect(mocks.error).toHaveBeenCalledWith("settings.toolActionFailed", {
        description: "installer failed",
        closeButton: true,
      });
      fireEvent.click(updateButton("Claude Code"));
      await waitFor(() => expect(isReady("Claude Code")).toBe(true));
    } finally {
      errorLog.mockRestore();
    }
  });

  it("does not report success or failure when all requested tools are already running", async () => {
    mocks.runToolLifecycleAction.mockRejectedValue("TOOL_ACTION_IN_PROGRESS");
    await renderApps();
    const versionChecks = mocks.getToolVersions.mock.calls.length;
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(mocks.info).toHaveBeenCalledTimes(3));
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("completes other tools without counting an ongoing task as a failure", async () => {
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") throw "TOOL_ACTION_IN_PROGRESS";
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
    expect(mocks.info).toHaveBeenCalledTimes(1);
    expect(isReady("Codex")).toBe(true);
    expect(isReady("Gemini CLI")).toBe(true);
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("keeps genuine failures in the batch summary while excluding ongoing tasks", async () => {
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") throw "TOOL_ACTION_IN_PROGRESS";
        if (tool === "codex") throw new Error("installer failed");
        upgraded.add(tool);
      },
    );
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await renderApps();
      fireEvent.click(updateAllButton());
      await waitFor(() =>
        expect(mocks.warning).toHaveBeenCalledWith(
          "settings.toolActionPartial",
          { description: "Codex: installer failed", closeButton: true },
        ),
      );
      expect(mocks.info).toHaveBeenCalledTimes(1);
      expect(mocks.success).not.toHaveBeenCalled();
      expect(mocks.error).not.toHaveBeenCalled();
      expect(errorLog).toHaveBeenCalledTimes(1);
    } finally {
      errorLog.mockRestore();
    }
  });

  it("releases a failed tool for retry without unlocking another running tool", async () => {
    const firstRun = deferred<void>();
    const retryRun = deferred<void>();
    const codexRun = deferred<void>();
    const geminiRun = deferred<void>();
    let claudeAttempts = 0;
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        const run =
          tool === "claude"
            ? ++claudeAttempts === 1
              ? firstRun
              : retryRun
            : tool === "codex"
              ? codexRun
              : geminiRun;
        await run.promise;
        upgraded.add(tool);
      },
    );
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    await act(async () => firstRun.reject(new Error("upgrade failed")));
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(updateButton("Codex")).toBeDisabled();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(4),
    );
    await act(async () => {
      codexRun.resolve();
      geminiRun.resolve();
    });
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(mocks.warning).toHaveBeenCalledWith(
      "settings.toolActionPartial",
      expect.objectContaining({ description: "Claude Code: upgrade failed" }),
    );
    await act(async () => retryRun.resolve());
    expect(isReady("Claude Code")).toBe(true);
    errorLog.mockRestore();
  });

  it("queues concurrent confirmation requests and releases only the cancelled tool", async () => {
    const claudeProbe = deferred<ToolInstallationReport[]>();
    const codexProbe = deferred<ToolInstallationReport[]>();
    const claudeRun = deferred<void>();
    mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
      tool === "claude" ? claudeProbe.promise : codexProbe.promise,
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await claudeRun.promise;
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Codex"));
    await act(async () => {
      claudeProbe.resolve([report("claude", { needs_confirmation: true })]);
      codexProbe.resolve([report("codex", { needs_confirmation: true })]);
    });
    expect(
      within(screen.getByRole("dialog")).getByText("Claude Code"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("dialog")).queryByText("Codex"),
    ).not.toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
    );
    expect(
      within(screen.getByRole("dialog")).getByText("Codex"),
    ).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["claude"],
      "update",
      {},
    );
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(updateButton("Codex")).toBeEnabled();
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
  });

  it.each(["double click", "held Enter", "held Space"])(
    "does not confirm the next queued plan with a %s",
    async (gesture) => {
      const claudeProbe = deferred<ToolInstallationReport[]>();
      const codexProbe = deferred<ToolInstallationReport[]>();
      const run = deferred<void>();
      mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
        tool === "claude" ? claudeProbe.promise : codexProbe.promise,
      );
      mocks.runToolLifecycleAction.mockImplementation(() => run.promise);
      const user = userEvent.setup();
      await renderApps();
      fireEvent.click(updateButton("Claude Code"));
      fireEvent.click(updateButton("Codex"));
      await act(async () => {
        claudeProbe.resolve([report("claude", { needs_confirmation: true })]);
        codexProbe.resolve([report("codex", { needs_confirmation: true })]);
      });
      const confirm = screen.getByRole("button", {
        name: "settings.toolUpgradeConfirmBtn",
      });
      if (gesture === "double click") {
        await user.dblClick(confirm);
      } else {
        confirm.focus();
        await user.keyboard(
          gesture === "held Enter" ? "{Enter>3/}" : "[Space>3/]",
        );
      }
      expect(mocks.runToolLifecycleAction.mock.calls).toEqual([
        [["claude"], "update", {}],
      ]);
      expect(
        within(screen.getByRole("dialog")).getByText("Codex"),
      ).toBeInTheDocument();
      if (gesture !== "double click") {
        expect(
          screen.getByRole("heading", {
            name: "settings.toolUpgradeConfirmTitle",
          }),
        ).toHaveFocus();
      }
      // 第二项仍可通过一次新的、明确的操作正常确认。
      await user.click(confirm);
      expect(mocks.runToolLifecycleAction.mock.calls).toEqual([
        [["claude"], "update", {}],
        [["codex"], "update", {}],
      ]);
      await act(async () => run.resolve());
    },
  );

  it("does not cancel the next queued plan with a double click", async () => {
    const claudeProbe = deferred<ToolInstallationReport[]>();
    const codexProbe = deferred<ToolInstallationReport[]>();
    mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
      tool === "claude" ? claudeProbe.promise : codexProbe.promise,
    );
    const user = userEvent.setup();
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Codex"));
    await act(async () => {
      claudeProbe.resolve([report("claude", { needs_confirmation: true })]);
      codexProbe.resolve([report("codex", { needs_confirmation: true })]);
    });
    await user.dblClick(screen.getByRole("button", { name: "common.cancel" }));
    expect(
      within(screen.getByRole("dialog")).getByText("Codex"),
    ).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(updateButton("Codex")).toBeEnabled();
  });

  it("does not block another installation submission while an upgrade is running", async () => {
    const claudeRun = deferred<void>();
    missing.add("codex");
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") await claudeRun.promise;
        upgraded.add(tool);
        missing.delete(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    const install = card("Codex").getByRole("button", {
      name: "settings.toolInstall",
    });
    expect(install).toBeEnabled();
    fireEvent.click(install);
    await waitFor(() => expect(isReady("Codex")).toBe(true));
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["codex"],
      "install",
      {},
    );
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
  });

  it("keeps a tool locked through version refresh and remount with an expired cache", async () => {
    const refreshed =
      deferred<Awaited<ReturnType<typeof mocks.getToolVersions>>>();
    const view = await renderApps();
    mocks.getToolVersions.mockImplementationOnce(() => refreshed.promise);
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(
        card("Claude Code").getAllByLabelText("common.loading").length,
      ).toBeGreaterThan(0),
    );
    // 等原任务的版本刷新真正发出（初始探测 + 刷新共 2 次）再卸载，
    // 否则机器慢时刷新可能落在重新挂载之后，计数就会抖
    await waitFor(() =>
      expect(
        mocks.getToolVersions.mock.calls.filter(([tools]) =>
          tools.includes("claude"),
        ),
      ).toHaveLength(2),
    );
    view.unmount();
    const now = vi
      .spyOn(Date, "now")
      .mockReturnValue(Date.now() + 11 * 60 * 1000);
    try {
      await renderApps();
      // Initial probe and the original task's refresh only: remount must not
      // probe a tool again while its installation/version refresh is pending.
      expect(
        mocks.getToolVersions.mock.calls.filter(([tools]) =>
          tools.includes("claude"),
        ),
      ).toHaveLength(2);
    } finally {
      now.mockRestore();
    }
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    // 缓存过期，重新挂载时别的工具会重新探测版本；等探测完、按钮出来再点「全部更新」，
    // 不然机器慢时这几行还在加载
    await waitFor(() => {
      expect(updateButton("Codex")).toBeEnabled();
      expect(updateButton("Gemini CLI")).toBeEnabled();
    });
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    expect(
      mocks.runToolLifecycleAction.mock.calls.filter(([tools]) =>
        tools.includes("claude"),
      ),
    ).toHaveLength(1);
    await act(async () =>
      refreshed.resolve([
        {
          name: "claude",
          version: "2.0.0",
          latest_version: "2.0.0",
          error: null,
          installed_but_broken: false,
          env_type: "windows",
          wsl_distro: null,
        },
      ]),
    );
    expect(isReady("Claude Code")).toBe(true);
  });

  it("releases skipped unmanaged tools and runs the remaining batch", async () => {
    mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => report(tool, { unmanaged: tool === "claude" })),
    );
    await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(isReady("Codex")).toBe(true));
    expect(isReady("Gemini CLI")).toBe(true);
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["codex"], ["gemini"]]);
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeEnabled();
    expect(mocks.warning).toHaveBeenCalledWith(
      "settings.toolUpgradeUnmanagedTitle",
      expect.anything(),
    );
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
});
