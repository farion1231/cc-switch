import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

/** 「关于」页时期的键名，沿用至今：只有字面量 "false" 关掉自动检查。 */
const AUTO_CHECK_KEY = "ccswitch:about:autoCheckToolVersions";
/** TOOL_NAMES 的长度：一次自动检查 = 每个工具一个 getToolVersions 请求。 */
const TOOL_COUNT = 9;

const mocks = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  probeToolInstallations: vi.fn(),
  listToolInstallations: vi.fn(),
  runToolLifecycleAction: vi.fn(),
  info: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api", () => ({ settingsApi: mocks }));
vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopStatus: async () => ({ supported: true, configured: true }),
  },
}));
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: { visibleApps: undefined },
    updateSettings: vi.fn(),
    autoSaveSettings: vi.fn(async () => null),
  }),
}));
vi.mock("sonner", () => ({ toast: mocks }));

/** 挂载「应用」页。模块级缓存跨同一用例内的重挂保留，所以不在这里 resetModules。 */
async function renderApps() {
  const { AppsPage } = await import("@/components/apps/AppsPage");
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <AppsPage />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeInTheDocument(),
  );
  return view;
}

const autoCheckSwitch = () =>
  screen.getByRole("switch", { name: "settings.autoCheckToolVersions" });

/** 某个工具那一行（与 AppsPage.test.tsx 的取法一致）。 */
function card(name: string) {
  return within(
    screen.getByText(name).closest("[data-tool-row]") as HTMLElement,
  );
}

/**
 * 断言「没有新增探测请求」：先让挂载 effect 与微任务跑完，再比对调用次数。
 * renderApps 返回后 React 的 effect 已经跑过，这里再 flush 一次微任务即可确认
 * 没有迟到的探测。
 */
async function expectNoNewProbes(expected: {
  versions: number;
  installs: number;
}) {
  await act(async () => {
    await Promise.resolve();
  });
  expect(mocks.getToolVersions).toHaveBeenCalledTimes(expected.versions);
  expect(mocks.listToolInstallations).toHaveBeenCalledTimes(expected.installs);
}

describe("AppsPage automatic tool version checks", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
    mocks.getToolVersions
      .mockReset()
      .mockImplementation(async (tools: string[]) =>
        tools.map((name) => ({
          name,
          version: "2.0.0",
          latest_version: "2.0.0",
          error: null,
          installed_but_broken: false,
          env_type: "macos",
          wsl_distro: null,
        })),
      );
    mocks.listToolInstallations.mockReset().mockResolvedValue([]);
    mocks.probeToolInstallations.mockReset().mockResolvedValue([]);
  });

  it("probes nothing while off, shows unknown instead of 'not installed', and stays off after remount", async () => {
    localStorage.setItem(AUTO_CHECK_KEY, "false");
    const { unmount } = await renderApps();

    expect(autoCheckSwitch()).not.toBeChecked();
    expect(
      screen.getByText("settings.toolVersionsNotChecked"),
    ).toBeInTheDocument();
    expect(screen.getByText("appsPage.neverChecked")).toBeInTheDocument();
    // 没查过就不该谎报「未安装」、也不该给出安装按钮（会诱导重装已经装好的工具）。
    expect(card("Claude Code").getByText("common.unknown")).toBeInTheDocument();
    expect(screen.queryByText("common.notInstalled")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /settings\.tool(Install|Update)/ }),
    ).not.toBeInTheDocument();
    // 手动检查与诊断的入口仍在。
    expect(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    ).toBeInTheDocument();
    await expectNoNewProbes({ versions: 0, installs: 0 });

    unmount();
    await renderApps();
    expect(autoCheckSwitch()).not.toBeChecked();
    expect(
      screen.getByText("settings.toolVersionsNotChecked"),
    ).toBeInTheDocument();
    await expectNoNewProbes({ versions: 0, installs: 0 });
  });

  it("checks every tool once by default and reuses the result after remount", async () => {
    const { unmount } = await renderApps();

    expect(autoCheckSwitch()).toBeChecked();
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_COUNT),
    );
    // 安装分布探测同样只在挂载时做一次。
    await waitFor(() =>
      expect(mocks.listToolInstallations).toHaveBeenCalledTimes(1),
    );
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();
    expect(
      screen.queryByText("settings.toolVersionsNotChecked"),
    ).not.toBeInTheDocument();

    unmount();
    await renderApps();
    expect(autoCheckSwitch()).toBeChecked();
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();
    // 新鲜缓存：重挂不重新探测（9 个 `--version` 子进程 + 9 个网络请求都省掉）。
    await expectNoNewProbes({ versions: TOOL_COUNT, installs: 1 });
  });

  it("only the literal false disables automatic checks", async () => {
    localStorage.setItem(AUTO_CHECK_KEY, "0");
    await renderApps();

    expect(autoCheckSwitch()).toBeChecked();
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_COUNT),
    );
  });

  it("keeps the manual check working while off, then does not re-probe on remount", async () => {
    localStorage.setItem(AUTO_CHECK_KEY, "false");
    const { unmount } = await renderApps();
    await expectNoNewProbes({ versions: 0, installs: 0 });

    fireEvent.click(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    );
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_COUNT),
    );
    expect(mocks.listToolInstallations).toHaveBeenCalledTimes(1);
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();
    // 手动查过了：不能再提示「尚未检查」。
    expect(
      screen.queryByText("settings.toolVersionsNotChecked"),
    ).not.toBeInTheDocument();

    unmount();
    await renderApps();
    expect(autoCheckSwitch()).not.toBeChecked();
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();
    await expectNoNewProbes({ versions: TOOL_COUNT, installs: 1 });
  });

  it("persists the toggle and triggers exactly one check when re-enabled", async () => {
    localStorage.setItem(AUTO_CHECK_KEY, "false");
    const { unmount } = await renderApps();
    await expectNoNewProbes({ versions: 0, installs: 0 });

    fireEvent.click(autoCheckSwitch());
    expect(autoCheckSwitch()).toBeChecked();
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("true");
    // 打开开关 = 补查一次（每个工具一个请求），不因为状态变化再叠加一遍。
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_COUNT),
    );
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();

    expect(mocks.listToolInstallations).toHaveBeenCalledTimes(1);

    unmount();
    await renderApps();
    expect(autoCheckSwitch()).toBeChecked();
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();
    // 重挂复用版本和安装分布缓存。
    await expectNoNewProbes({ versions: TOOL_COUNT, installs: 1 });
  });

  it("keeps the loaded versions visible when switched off", async () => {
    await renderApps();
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_COUNT),
    );

    fireEvent.click(autoCheckSwitch());
    expect(autoCheckSwitch()).not.toBeChecked();
    expect(localStorage.getItem(AUTO_CHECK_KEY)).toBe("false");
    // 关闭不触发新探测，也不清掉已经加载的数据。
    await expectNoNewProbes({ versions: TOOL_COUNT, installs: 1 });
    expect(card("Claude Code").getByText("2.0.0")).toBeInTheDocument();
    expect(
      screen.queryByText("settings.toolVersionsNotChecked"),
    ).not.toBeInTheDocument();
  });
});
