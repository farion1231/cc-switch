import { render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, beforeAll, expect, vi, type Mock } from "vitest";
import { QueryClientProvider } from "@tanstack/react-query";
import type {
  settingsApi,
  ToolInstallationReport,
  ToolUninstallCandidate,
  ToolUninstallPlan,
} from "@/lib/api/settings";
import type { DragEndEvent } from "@dnd-kit/core";
import { createTestQueryClient } from "../utils/testQueryClient";

export type ToolVersions = Awaited<
  ReturnType<typeof settingsApi.getToolVersions>
>;

export interface AppsPageMocks {
  getToolVersions: Mock;
  getConfigStatus: Mock;
  probeToolInstallations: Mock;
  listToolInstallations: Mock;
  runToolLifecycleAction: Mock;
  prepareToolUninstall: Mock;
  copyText: Mock;
  info: Mock;
  success: Mock;
  warning: Mock;
  error: Mock;
  settings: { visibleApps?: Record<string, boolean>; appOrder?: string[] };
  updateSettings: Mock;
  autoSaveSettings: Mock;
  dragEndHandlers: ((event: DragEndEvent) => void)[];
  desktopStatus: {
    supported: boolean;
    installed: boolean;
    configured: boolean;
  };
}

export interface AppsPageToolState {
  upgraded: Set<string>;
  outdated: Set<string>;
  missing: Set<string>;
}

export function createToolState(): AppsPageToolState {
  return {
    upgraded: new Set<string>(),
    outdated: new Set<string>(),
    missing: new Set<string>(),
  };
}

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

export function registerScrollIntoViewStub() {
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
}

export function resetAppsPageMocks(
  mocks: AppsPageMocks,
  state: AppsPageToolState,
) {
  vi.resetModules();
  state.upgraded.clear();
  state.outdated.clear();
  state.missing.clear();
  mocks.settings = {};
  mocks.dragEndHandlers = [];
  mocks.desktopStatus = {
    supported: true,
    installed: true,
    configured: true,
  };
  mocks.getConfigStatus.mockReset().mockResolvedValue({
    exists: true,
    path: "/home/me/.workbuddy",
  });
  mocks.updateSettings.mockReset();
  mocks.autoSaveSettings.mockReset().mockResolvedValue(null);
  mocks.copyText.mockReset().mockResolvedValue(undefined);
  state.outdated.add("claude").add("codex").add("gemini");
  mocks.getToolVersions
    .mockReset()
    .mockImplementation(async (tools: string[]) =>
      tools.map((name) => ({
        name,
        version: state.missing.has(name)
          ? null
          : state.upgraded.has(name) || !state.outdated.has(name)
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
      state.upgraded.add(tool);
      state.missing.delete(tool);
    });
}
