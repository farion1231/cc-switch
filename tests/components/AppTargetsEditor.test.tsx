import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { AppTargetsEditor } from "@/components/settings/AppTargetsEditor";
import type {
  AppTargetsView,
  ToolInstall,
  WslInventory,
} from "@/lib/api/appTargets";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.target ? `${key}:${options.target}` : key,
  }),
}));

const inspectMock = vi.fn();
const saveMock = vi.fn();
vi.mock("@/lib/api/appTargets", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/appTargets")>();
  return {
    ...actual,
    appTargetsApi: {
      inspect: (...args: unknown[]) => inspectMock(...args),
      save: (...args: unknown[]) => saveMock(...args),
      overview: vi.fn(),
      sync: vi.fn(),
    },
  };
});

const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));

const inventory: WslInventory = {
  hostWindows: true,
  wslAvailable: true,
  distros: [
    {
      distro: "Ubuntu",
      available: true,
      running: true,
      home: "/home/me",
      cached: false,
    },
  ],
};

let installs: Record<string, Record<string, ToolInstall>> = {};
const claudeInstalled = () => ({
  Ubuntu: {
    claude: {
      status: "installed" as const,
      version: "2.1.0",
      detectedAt: 1,
      cached: false,
    },
  },
});

const legacyView: AppTargetsView = {
  app: "claude",
  source: "legacy",
  config: {
    primary: "windows",
    windows: { enabled: true, dir: null },
    wsl: [],
  },
  resolvedDirs: {
    windows: "C:\\Users\\me\\.claude",
    "wsl:Ubuntu": "\\\\wsl.localhost\\Ubuntu\\home\\me\\.claude",
  },
  records: {},
};

function renderEditor(view: AppTargetsView = legacyView) {
  const client = new QueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return render(
    <AppTargetsEditor
      app="claude"
      appName="Claude Code"
      view={view}
      inventory={inventory}
      installs={installs}
      detecting={false}
      onDetect={vi.fn()}
      renderBlock={(actions, body) => (
        <div>
          {actions}
          {body}
        </div>
      )}
    />,
    { wrapper },
  );
}

describe("AppTargetsEditor", () => {
  beforeEach(() => {
    installs = claudeInstalled();
    inspectMock.mockReset();
    saveMock.mockReset();
    Object.values(toastMock).forEach((fn) => fn.mockReset());
  });

  it("only lists WSL distros where the client is installed", () => {
    installs = {
      Ubuntu: {
        claude: { status: "notInstalled", detectedAt: 1, cached: false },
      },
    };
    renderEditor();
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
  });

  it("lists detected distros unchecked and keeps the last target", () => {
    renderEditor();
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes).toHaveLength(2);
    expect(boxes[0]).toBeChecked();
    expect(boxes[1]).not.toBeChecked();

    // 取消最后一个：保持选择并提示
    fireEvent.click(boxes[0]);
    expect(boxes[0]).toBeChecked();
    expect(screen.getByText("appTargets.keepOne")).toBeInTheDocument();
    // 未改动时保存 / 撤销都不可用
    expect(screen.getByText("common.save").closest("button")).toBeDisabled();
  });

  it("has no primary picker and derives the primary environment", async () => {
    inspectMock.mockResolvedValue({
      errors: [],
      resolvedDirs: {},
      conflicts: [],
      primaryTarget: "wsl:Ubuntu",
    });
    saveMock.mockResolvedValue({
      saved: true,
      needsResolution: false,
      errors: [],
      resolvedDirs: {},
      conflicts: [],
      primaryTarget: "wsl:Ubuntu",
      imported: [],
      targets: [],
    });
    renderEditor();
    const boxes = screen.getAllByRole("checkbox");
    fireEvent.click(boxes[1]);
    expect(screen.queryAllByRole("radio")).toHaveLength(0);
    // 取消 Windows：主环境自动落到 Ubuntu，不再提示用户选择
    fireEvent.click(boxes[0]);
    await act(async () => {
      fireEvent.click(screen.getByText("common.save"));
    });
    await waitFor(() => expect(saveMock).toHaveBeenCalled());
    expect(saveMock.mock.calls[0][1].primary).toBe("wsl:Ubuntu");
  });

  it("offers import or overwrite (no defer) for the new primary", async () => {
    inspectMock.mockResolvedValue({
      errors: [],
      resolvedDirs: {},
      primaryTarget: "wsl:Ubuntu",
      conflicts: [
        { target: "wsl:Ubuntu", dir: "x", existingHost: "other.example" },
      ],
    });
    renderEditor();
    const boxes = screen.getAllByRole("checkbox");
    fireEvent.click(boxes[1]);
    fireEvent.click(boxes[0]);
    await act(async () => {
      fireEvent.click(screen.getByText("common.save"));
    });
    await screen.findByText(/appTargets.conflict.title/);
    expect(
      screen.getByRole("radio", { name: /^appTargets.conflict.import / }),
    ).toBeChecked();
    expect(
      screen.queryByRole("radio", { name: /^appTargets.conflict.defer / }),
    ).not.toBeInTheDocument();
  });

  it("wraps truncated names and paths in the shared hover tip", () => {
    renderEditor();
    const pill = screen.getAllByText("WSL · Ubuntu")[0];
    // HoverTip（Radix Tooltip）的触发器带 data-state
    expect(pill.closest("[data-state]")).not.toBeNull();
    const input = screen.getAllByRole("textbox")[1];
    expect(input.closest("[data-state]")).not.toBeNull();
  });

  it("asks per conflict and cancelling saves nothing", async () => {
    inspectMock.mockResolvedValue({
      errors: [],
      resolvedDirs: {},
      primaryTarget: "windows",
      conflicts: [
        {
          target: "wsl:Ubuntu",
          dir: "\\\\wsl.localhost\\Ubuntu\\home\\me\\.claude",
          existingHost: "other.example",
          incomingProviderName: "A",
        },
      ],
    });
    renderEditor();
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    await act(async () => {
      fireEvent.click(screen.getByText("common.save"));
    });
    await screen.findByText(/appTargets.conflict.title/);
    // 默认「暂不写入」
    expect(
      screen.getByRole("radio", { name: /^appTargets.conflict.defer / }),
    ).toBeChecked();
    await act(async () => {
      fireEvent.click(screen.getByText("common.cancel"));
    });
    expect(saveMock).not.toHaveBeenCalled();
    expect(toastMock.info).toHaveBeenCalledWith("appTargets.toast.cancelled");
  });

  it("sends the chosen resolution and reports unsynced targets", async () => {
    inspectMock.mockResolvedValue({
      errors: [],
      resolvedDirs: {},
      primaryTarget: "windows",
      conflicts: [
        { target: "wsl:Ubuntu", dir: "x", incomingProviderName: "A" },
      ],
    });
    saveMock.mockResolvedValue({
      saved: true,
      needsResolution: false,
      errors: [],
      resolvedDirs: {},
      conflicts: [],
      imported: [],
      targets: [
        { target: "wsl:Ubuntu", status: "error", at: 1, detail: "boom" },
      ],
    });
    renderEditor();
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    await act(async () => {
      fireEvent.click(screen.getByText("common.save"));
    });
    await screen.findByText(/appTargets.conflict.title/);
    fireEvent.click(
      screen.getByRole("radio", { name: /^appTargets.conflict.overwrite / }),
    );
    await act(async () => {
      fireEvent.click(screen.getByText("appTargets.conflict.confirm"));
    });
    await waitFor(() => expect(saveMock).toHaveBeenCalled());
    const [app, config, resolutions] = saveMock.mock.calls[0];
    expect(app).toBe("claude");
    expect(config.wsl).toEqual([
      { distro: "Ubuntu", enabled: true, dir: null },
    ]);
    expect(resolutions).toEqual({ "wsl:Ubuntu": "overwrite" });
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalled());
  });

  it("shows backend field errors on the row", async () => {
    inspectMock.mockResolvedValue({
      errors: [{ target: "wsl:Ubuntu", code: "wslDistroMismatch" }],
      resolvedDirs: {},
      conflicts: [],
    });
    renderEditor();
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    await act(async () => {
      fireEvent.click(screen.getByText("common.save"));
    });
    expect(
      await screen.findByText("appTargets.errors.wslDistroMismatch"),
    ).toBeInTheDocument();
    expect(saveMock).not.toHaveBeenCalled();
  });
});
