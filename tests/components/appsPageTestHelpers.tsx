import { vi, expect, beforeEach, afterAll, beforeAll } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type {
  settingsApi,
  ToolInstallationReport,
  ToolUninstallCandidate,
  ToolUninstallPlan,
} from "@/lib/api/settings";
import type { DndContextProps, DragEndEvent } from "@dnd-kit/core";
import { createTestQueryClient } from "../utils/testQueryClient";
import userEvent from "@testing-library/user-event";

export type ToolVersions = Awaited<
  ReturnType<typeof settingsApi.getToolVersions>
>;

const mocks = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  getInstallManagerAvailability: vi.fn().mockResolvedValue({}),
  getToolInstallationUpdates: vi.fn(),
  getPreferredToolInstallation: vi.fn(),
  setPreferredToolInstallation: vi.fn(),
  getOfficialClaudeDesktopInstallation: vi.fn(),
  getOfficialWorkbuddyInstallation: vi.fn(),
  getConfigStatus: vi.fn(async () => ({
    exists: true,
    path: "/home/me/.workbuddy",
  })),
  probeToolInstallations: vi.fn(),
  listToolInstallations: vi.fn(),
  getToolInstallationReports: vi.fn(),
  runToolLifecycleAction: vi.fn(),
  prepareToolUninstall: vi.fn(),
  copyText: vi.fn(),
  openExternal: vi.fn(async () => {}),
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

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

export function report(
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

export function mockToolInstallations(
  tool: string,
  installs: ToolInstallationReport["installs"],
) {
  mocks.getToolInstallationReports.mockImplementation(async (tools: string[]) =>
    tools.map((name) =>
      report(name, { installs: name === tool ? installs : [] }),
    ),
  );
}

export async function openInstallCommands(
  user: ReturnType<typeof userEvent.setup>,
) {
  await user.click(
    screen.getByRole("button", { name: "appsPage.moreActions" }),
  );
  await user.click(
    await screen.findByRole("menuitem", { name: "appsPage.manualCommands" }),
  );
  return within(screen.getByRole("dialog"));
}

export function uninstallPlan(tool: string): ToolUninstallPlan {
  return {
    tool,
    path: `C:/tools/${tool}.cmd`,
    real_path: `C:/tools/${tool}.cmd`,
    command: `npm uninstall -g --prefix C:/tools ${tool}`,
    native: false,
    wsl_distro: null,
  };
}

export function candidate(
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

export const upgraded = new Set<string>();
export const outdated = new Set<string>();
export const missing = new Set<string>();

export function card(name: string) {
  return within(
    screen
      .getByText(name, { selector: "span" })
      .closest("[data-tool-row]") as HTMLElement,
  );
}

/** 已是最新：没有升级 / 安装按钮，显示新版本号。 */
export function isReady(name: string) {
  return (
    card(name).queryByRole("button", {
      name: /settings\.tool(Update|Install)/,
    }) === null && card(name).queryByText("2.0.0") !== null
  );
}

/** 某一组里各行的应用 ID，按显示顺序。 */
export function rowApps(region: string) {
  return Array.from(
    screen
      .getByRole("region", { name: region })
      .querySelectorAll<HTMLElement>("[data-tool-row]"),
    (row) => row.dataset.toolRow,
  );
}

/** 行尾「⋯」菜单（卸载在里面）。 */
export function rowMenu(name: string) {
  return card(name).getByRole("button", { name: "appPage.moreActions" });
}

export async function openUninstall(name: string) {
  const user = userEvent.setup();
  await user.click(rowMenu(name));
  await user.click(
    await screen.findByRole("menuitem", { name: "settings.toolUninstall" }),
  );
}

export function updateButton(name: string) {
  return card(name).getByRole("button", { name: "settings.toolUpdate" });
}

export function mountApps(AppsPage: () => JSX.Element) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <AppsPage />
    </QueryClientProvider>,
  );
}

export async function renderApps() {
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

export const updateAllButton = () =>
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

beforeEach(() => {
  mocks.getInstallManagerAvailability.mockReset().mockResolvedValue({});
  vi.resetModules();
  upgraded.clear();
  outdated.clear();
  missing.clear();
  mocks.settings = {};
  mocks.dragEndHandlers = [];
  mocks.getOfficialClaudeDesktopInstallation
    .mockReset()
    .mockResolvedValue(null);
  mocks.getOfficialWorkbuddyInstallation.mockReset().mockResolvedValue(null);
  mocks.desktopStatus = {
    supported: true,
    installed: true,
    configured: true,
  };
  mocks.updateSettings.mockReset();
  mocks.autoSaveSettings.mockReset().mockResolvedValue(null);
  mocks.copyText.mockReset().mockResolvedValue(undefined);
  outdated.add("claude").add("codex").add("gemini");
  mocks.getToolInstallationUpdates.mockReset().mockResolvedValue([]);
  mocks.getPreferredToolInstallation.mockReset().mockResolvedValue({
    supported: true,
    path: null,
    disabled_paths: [],
    selected_path: null,
    selected_real_path: null,
    selection: "unavailable",
  });
  mocks.setPreferredToolInstallation
    .mockReset()
    .mockImplementation(
      async (_tool: string, path: string | null, enabled?: boolean) => ({
        supported: true,
        path: enabled === undefined ? path : null,
        disabled_paths: enabled === false && path ? [path] : [],
      }),
    );
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
  mocks.listToolInstallations.mockReset().mockResolvedValue([]);
  mocks.getToolInstallationReports
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
export { mocks };
