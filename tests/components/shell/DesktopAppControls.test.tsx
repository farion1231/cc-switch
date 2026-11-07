import type { QueryClient } from "@tanstack/react-query";
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/lib/platform", () => ({
  isWindows: mocks.isWindows,
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

const renderControls = (app: AppId = "codex") => {
  const client = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={client}>
      <DesktopAppControls app={app} />
    </QueryClientProvider>,
  );
  return { ...view, client };
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

  it("非 Windows 平台不渲染按钮，也不发检测请求", () => {
    mocks.isWindows.mockReturnValue(false);
    const { container } = renderControls();

    expect(container).toBeEmptyDOMElement();
    expect(mocks.getStatus).not.toHaveBeenCalled();
  });

  it("不支持启动的 app 不渲染按钮", () => {
    const { container } = renderControls("claude");

    expect(container).toBeEmptyDOMElement();
    expect(mocks.getStatus).not.toHaveBeenCalled();
  });
});
