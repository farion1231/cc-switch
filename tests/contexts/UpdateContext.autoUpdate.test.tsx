import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";

const mocks = vi.hoisted(() => ({
  status: {
    supported: true,
    enabled: false,
    downloading: false,
    ready: null as { version: string } | null,
    routingInUse: false,
  },
  checkForUpdate: vi.fn(),
  downloadAppUpdate: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    // 每次返回新对象，和真实 IPC 一样
    getAutoUpdateStatus: vi.fn(async () => ({
      ...mocks.status,
      ready: mocks.status.ready ? { ...mocks.status.ready } : null,
    })),
    downloadAppUpdate: mocks.downloadAppUpdate,
  },
}));

vi.mock("@/lib/updater", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/updater")>()),
  checkForUpdate: mocks.checkForUpdate,
}));

let ctx: ReturnType<typeof useUpdate>;
function Probe() {
  ctx = useUpdate();
  return (
    <div data-testid="probe">
      {String(ctx.hasUpdate)}:{ctx.updateInfo?.availableVersion ?? "-"}
    </div>
  );
}

describe("UpdateProvider auto update", () => {
  beforeEach(() => {
    vi.useRealTimers();
    mocks.status.ready = null;
    mocks.status.enabled = false;
    mocks.status.downloading = false;
    mocks.checkForUpdate.mockReset();
    mocks.downloadAppUpdate.mockReset();
    mocks.downloadAppUpdate.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  const available = (version: string) => ({
    status: "available",
    info: { currentVersion: "4.0.0", availableVersion: version },
  });

  it("starts the background download as soon as the startup check finds a new version", async () => {
    // 开发构建不主动下载，这里按正式构建测
    vi.stubEnv("DEV", false);
    mocks.status.enabled = true;
    mocks.checkForUpdate.mockResolvedValue(available("4.1.0"));

    render(
      <UpdateProvider>
        <Probe />
      </UpdateProvider>,
    );

    // 启动检查在挂载 1 秒后才跑
    await waitFor(
      () => expect(mocks.downloadAppUpdate).toHaveBeenCalledTimes(1),
      { timeout: 3000 },
    );
    // 再查一次同一版本不会重复触发下载
    await act(async () => {
      await ctx.checkUpdate();
      await ctx.refreshAutoUpdate();
    });
    expect(mocks.downloadAppUpdate).toHaveBeenCalledTimes(1);
  });

  it("does not download when the toggle is off", async () => {
    mocks.checkForUpdate.mockResolvedValue(available("4.1.0"));

    render(
      <UpdateProvider>
        <Probe />
      </UpdateProvider>,
    );

    await waitFor(
      () => expect(screen.getByTestId("probe")).toHaveTextContent("true:4.1.0"),
      { timeout: 3000 },
    );
    expect(mocks.downloadAppUpdate).not.toHaveBeenCalled();
  });

  it("treats a downloaded package as an available update even when the online check fails", async () => {
    mocks.status.ready = { version: "4.1.0" };
    mocks.checkForUpdate.mockRejectedValue(new Error("offline"));

    render(
      <UpdateProvider>
        <Probe />
      </UpdateProvider>,
    );

    await waitFor(() =>
      expect(screen.getByTestId("probe")).toHaveTextContent("true:4.1.0"),
    );
  });

  it("rechecks online at most once per downloaded version while the prompt polls", async () => {
    mocks.status.ready = { version: "4.1.0" };
    mocks.checkForUpdate.mockRejectedValue(new Error("offline"));

    render(
      <UpdateProvider>
        <Probe />
      </UpdateProvider>,
    );
    await waitFor(() => expect(ctx.readyUpdate?.version).toBe("4.1.0"));
    // 启动检查（1 秒后）之外，补查只该发生一次
    await waitFor(() => expect(mocks.checkForUpdate).toHaveBeenCalled());
    const callsAfterFirstRecheck = mocks.checkForUpdate.mock.calls.length;

    for (let i = 0; i < 3; i += 1) {
      await act(async () => {
        await ctx.refreshAutoUpdate();
      });
    }
    expect(mocks.checkForUpdate.mock.calls.length).toBe(callsAfterFirstRecheck);
  });
});
