import type { QueryClient } from "@tanstack/react-query";
import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DesktopAppControls } from "@/components/shell/DesktopAppControls";
import { desktopAppKeys } from "@/hooks/useDesktopAppStatus";
import type { AppId, DesktopAppStatus } from "@/lib/api";
import { createTestQueryClient } from "../../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  launch: vi.fn(),
  restart: vi.fn(),
  isWindows: vi.fn(() => true),
  isMac: vi.fn(() => false),
  isLinux: vi.fn(() => false),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values ? `${key}:${JSON.stringify(values)}` : key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/lib/toast", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
  },
}));

vi.mock("@/lib/platform", () => ({
  isWindows: mocks.isWindows,
  isMac: mocks.isMac,
  isLinux: mocks.isLinux,
}));

vi.mock("@/lib/api/desktopApp", () => ({
  desktopAppApi: {
    getStatus: mocks.getStatus,
    launch: mocks.launch,
    restart: mocks.restart,
  },
}));

const START_LABEL = "appPage.desktopApp.startApp";
const RESTART_LABEL = "appPage.desktopApp.restartApp";

const renderControls = (app: AppId = "codex", keyed = false) => {
  const client = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={client}>
      <DesktopAppControls key={keyed ? app : undefined} app={app} />
    </QueryClientProvider>,
  );
  const rerenderApp = (nextApp: AppId) =>
    view.rerender(
      <QueryClientProvider client={client}>
        <DesktopAppControls key={keyed ? nextApp : undefined} app={nextApp} />
      </QueryClientProvider>,
    );
  return { ...view, client, rerenderApp };
};

const waitForStatus = (
  client: QueryClient,
  app: AppId,
  expected: Partial<DesktopAppStatus>,
) =>
  waitFor(() =>
    expect(client.getQueryData(desktopAppKeys.status(app))).toMatchObject(
      expected,
    ),
  );

describe("DesktopAppControls", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.isWindows.mockReturnValue(true);
    mocks.isMac.mockReturnValue(false);
    mocks.isLinux.mockReturnValue(false);
    mocks.getStatus.mockResolvedValue({
      supported: true,
      installed: true,
      running: false,
    });
    mocks.launch.mockResolvedValue(undefined);
    mocks.restart.mockResolvedValue(undefined);
  });

  it("未安装时两个按钮都灰着", async () => {
    mocks.getStatus.mockResolvedValue({
      supported: true,
      installed: false,
      running: false,
    });
    const { client } = renderControls();

    await waitForStatus(client, "codex", { installed: false });

    expect(screen.getByRole("button", { name: START_LABEL })).toBeDisabled();
    expect(screen.getByRole("button", { name: RESTART_LABEL })).toBeDisabled();
  });

  it("仅按需检测，不启动 5 秒轮询", async () => {
    vi.useFakeTimers();
    try {
      renderControls();

      expect(mocks.getStatus).toHaveBeenCalledTimes(1);
      await act(() => vi.advanceTimersByTimeAsync(10_000));

      expect(mocks.getStatus).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("像 App.tsx 一样按 app 设置 key，切页时不沿用启动和重启状态", async () => {
    mocks.launch.mockReturnValue(new Promise<void>(() => {}));
    mocks.restart.mockReturnValue(new Promise<void>(() => {}));
    const user = userEvent.setup();
    const { client, rerenderApp } = renderControls("codex", true);

    await waitForStatus(client, "codex", { installed: true });
    await user.click(screen.getByRole("button", { name: START_LABEL }));
    expect(screen.getByRole("button", { name: START_LABEL })).toBeDisabled();
    expect(screen.getByRole("button", { name: RESTART_LABEL })).toBeDisabled();

    rerenderApp("opencode");
    await waitForStatus(client, "opencode", { installed: true });
    expect(mocks.getStatus).toHaveBeenLastCalledWith("opencode");
    await user.click(screen.getByRole("button", { name: RESTART_LABEL }));
    expect(mocks.restart).toHaveBeenCalledWith("opencode");
    expect(screen.getByRole("button", { name: START_LABEL })).toBeDisabled();
    expect(screen.getByRole("button", { name: RESTART_LABEL })).toBeDisabled();

    rerenderApp("claude-desktop");
    await waitForStatus(client, "claude-desktop", { installed: true });
    expect(screen.getByRole("button", { name: START_LABEL })).toBeEnabled();
    expect(screen.getByRole("button", { name: RESTART_LABEL })).toBeEnabled();
  });

  it.each([
    {
      operation: "launch",
      button: START_LABEL,
      apiMock: () => mocks.launch,
      toastKey: "appPage.desktopApp.startFailed",
    },
    {
      operation: "restart",
      button: RESTART_LABEL,
      apiMock: () => mocks.restart,
      toastKey: "appPage.desktopApp.restartFailed",
    },
  ])(
    "$operation 失败后刷新当前 app 状态，并显示对应 app 的错误",
    async ({ button, apiMock, toastKey }) => {
      mocks.getStatus
        .mockResolvedValueOnce({
          supported: true,
          installed: true,
          running: false,
        })
        .mockResolvedValueOnce({
          supported: true,
          installed: true,
          running: true,
        });
      apiMock().mockRejectedValueOnce(new Error("operation failed"));
      const user = userEvent.setup();
      const { client } = renderControls();

      await waitForStatus(client, "codex", { installed: true });
      expect(mocks.getStatus).toHaveBeenCalledTimes(1);
      await user.click(screen.getByRole("button", { name: button }));

      await waitFor(() => expect(mocks.getStatus).toHaveBeenCalledTimes(2));
      expect(mocks.getStatus).toHaveBeenLastCalledWith("codex");
      await waitForStatus(client, "codex", { installed: true, running: true });
      expect(mocks.toastError).toHaveBeenCalledWith(
        `${toastKey}:${JSON.stringify({ name: "Codex", error: "operation failed" })}`,
      );
    },
  );

  it("已安装未运行：启动和重启都可用，点启动会调用后端", async () => {
    const user = userEvent.setup();
    const { client } = renderControls();

    await waitForStatus(client, "codex", { installed: true, running: false });

    const start = screen.getByRole("button", { name: START_LABEL });
    const restart = screen.getByRole("button", { name: RESTART_LABEL });
    expect(start).toBeEnabled();
    expect(restart).toBeEnabled();

    await user.click(start);
    expect(mocks.launch).toHaveBeenCalledWith("codex");
  });

  it("运行中：启动灰掉，重启可用并能调用后端", async () => {
    mocks.getStatus.mockResolvedValue({
      supported: true,
      installed: true,
      running: true,
    });
    const user = userEvent.setup();
    const { client } = renderControls();

    await waitForStatus(client, "codex", { running: true });

    expect(screen.getByRole("button", { name: START_LABEL })).toBeDisabled();
    const restart = screen.getByRole("button", { name: RESTART_LABEL });
    expect(restart).toBeEnabled();

    await user.click(restart);
    expect(mocks.restart).toHaveBeenCalledWith("codex");
  });

  it.each(["codex", "claude-desktop", "opencode"] as const)(
    "Linux 上为 %s 渲染按钮、请求后端，并在未安装时保持置灰",
    async (app) => {
      mocks.isWindows.mockReturnValue(false);
      mocks.isLinux.mockReturnValue(true);
      mocks.getStatus.mockResolvedValue({
        supported: true,
        installed: false,
        running: false,
      });
      const { client } = renderControls(app);

      await waitForStatus(client, app, { installed: false });

      expect(mocks.getStatus).toHaveBeenCalledWith(app);
      expect(screen.getByRole("button", { name: START_LABEL })).toBeDisabled();
      expect(
        screen.getByRole("button", { name: RESTART_LABEL }),
      ).toBeDisabled();
    },
  );

  it("未知平台不渲染按钮，也不发检测请求", () => {
    mocks.isWindows.mockReturnValue(false);
    const { container } = renderControls();

    expect(container).toBeEmptyDOMElement();
    expect(mocks.getStatus).not.toHaveBeenCalled();
  });

  it("macOS 上渲染启动和重启按钮", async () => {
    mocks.isWindows.mockReturnValue(false);
    mocks.isMac.mockReturnValue(true);
    const { client } = renderControls();

    await waitForStatus(client, "codex", { installed: true });

    expect(screen.getByRole("button", { name: START_LABEL })).toBeEnabled();
    expect(screen.getByRole("button", { name: RESTART_LABEL })).toBeEnabled();
  });

  it("不支持启动的 app 不渲染按钮", () => {
    const { container } = renderControls("claude");

    expect(container).toBeEmptyDOMElement();
    expect(mocks.getStatus).not.toHaveBeenCalled();
  });
});
