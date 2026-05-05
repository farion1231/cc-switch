import { beforeEach, describe, expect, it, vi } from "vitest";
import { providersApi } from "./providers";
import { runtimeApi, type RuntimeInfo } from "./runtime";
import { invoke } from "./transport";

vi.mock("./runtime", () => ({
  runtimeApi: {
    getCached: vi.fn(),
  },
}));

vi.mock("./transport", () => ({
  invoke: vi.fn(),
}));

vi.mock("./events", () => ({
  listenBackendEvent: vi.fn(),
}));

const getCachedMock = vi.mocked(runtimeApi.getCached);
const invokeMock = vi.mocked(invoke);

const runtimeInfo = (
  launchInteractiveTerminal: boolean,
  coLocated: boolean,
): RuntimeInfo => ({
  client: { shell: "desktop", os: "windows" },
  backend: {
    os: "linux",
    headless: !coLocated,
    remote: !coLocated,
    capabilities: {
      readConfig: true,
      writeConfig: true,
      openLocalFolder: coLocated,
      pickDirectory: coLocated,
      serverDirectoryBrowse: true,
      appConfigDirOverride: coLocated,
      saveFileDialog: coLocated,
      openFileDialog: coLocated,
      launchInteractiveTerminal,
      launchBackgroundProcess: false,
      autoLaunch: coLocated,
      toolVersionCheck: coLocated,
      windowControls: coLocated,
      dragRegion: coLocated,
      tray: coLocated,
    },
  },
  relation: { coLocated },
});

describe("providersApi runtime capability guards", () => {
  beforeEach(() => {
    getCachedMock.mockReset();
    invokeMock.mockReset();
  });

  it("does not open provider terminals when the client and backend are not colocated", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(true, false));

    await expect(
      providersApi.openTerminal("provider-1", "claude"),
    ).resolves.toBe(false);

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("opens provider terminals when an interactive terminal is available locally", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(true, true));
    invokeMock.mockResolvedValue(true);

    await expect(
      providersApi.openTerminal("provider-1", "claude", { cwd: "/tmp" }),
    ).resolves.toBe(true);

    expect(invokeMock).toHaveBeenCalledWith("open_provider_terminal", {
      providerId: "provider-1",
      app: "claude",
      cwd: "/tmp",
    });
  });
});
