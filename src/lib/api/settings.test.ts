import { beforeEach, describe, expect, it, vi } from "vitest";
import { settingsApi } from "./settings";
import { runtimeApi, type RuntimeInfo } from "./runtime";
import { invoke } from "./transport";

vi.mock("./runtime", () => ({
  runtimeApi: {
    getCached: vi.fn(),
  },
}));

vi.mock("./transport", () => ({
  invoke: vi.fn(),
  invokeLocal: vi.fn(),
}));

const getCachedMock = vi.mocked(runtimeApi.getCached);
const invokeMock = vi.mocked(invoke);

const runtimeInfo = (
  overrides: Partial<RuntimeInfo["backend"]["capabilities"]> = {},
): RuntimeInfo => ({
  client: { shell: "desktop", os: "windows" },
  backend: {
    os: "linux",
    headless: true,
    remote: true,
    capabilities: {
      readConfig: true,
      writeConfig: true,
      openLocalFolder: false,
      pickDirectory: false,
      serverDirectoryBrowse: true,
      appConfigDirOverride: false,
      saveFileDialog: false,
      openFileDialog: false,
      launchInteractiveTerminal: false,
      launchBackgroundProcess: false,
      autoLaunch: false,
      toolVersionCheck: false,
      windowControls: false,
      dragRegion: false,
      tray: false,
      ...overrides,
    },
  },
  relation: { coLocated: false },
});

describe("settingsApi runtime capability guards", () => {
  beforeEach(() => {
    getCachedMock.mockReset();
    invokeMock.mockReset();
  });

  it("does not invoke tool version checks when the backend does not advertise support", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo({ toolVersionCheck: false }));

    await expect(settingsApi.getToolVersions(["claude"])).resolves.toEqual([]);

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("invokes tool version checks when the backend advertises support", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo({ toolVersionCheck: true }));
    invokeMock.mockResolvedValue([
      {
        name: "claude",
        version: "1.0.0",
        latest_version: null,
        error: null,
        env_type: "linux",
        wsl_distro: null,
      },
    ]);

    await expect(settingsApi.getToolVersions(["claude"])).resolves.toEqual([
      {
        name: "claude",
        version: "1.0.0",
        latest_version: null,
        error: null,
        env_type: "linux",
        wsl_distro: null,
      },
    ]);
    expect(invokeMock).toHaveBeenCalledWith("get_tool_versions", {
      tools: ["claude"],
      wslShellByTool: undefined,
    });
  });
});
