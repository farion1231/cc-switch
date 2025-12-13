import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useDirectorySettings } from "@/hooks/useDirectorySettings";
import type { SettingsFormState } from "@/hooks/useSettingsForm";

const getAppConfigDirOverrideMock = vi.hoisted(() => vi.fn());
const getConfigDirMock = vi.hoisted(() => vi.fn());
const selectConfigDirectoryMock = vi.hoisted(() => vi.fn());
const setAppConfigDirOverrideMock = vi.hoisted(() => vi.fn());
const homeDirMock = vi.hoisted(() => vi.fn<() => Promise<string>>());
const joinMock = vi.hoisted(() =>
  vi.fn(async (...segments: string[]) => segments.join("/")),
);
const toastErrorMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getAppConfigDirOverride: getAppConfigDirOverrideMock,
    getConfigDir: getConfigDirMock,
    selectConfigDirectory: selectConfigDirectoryMock,
    setAppConfigDirOverride: setAppConfigDirOverrideMock,
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: homeDirMock,
  join: joinMock,
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      (options?.defaultValue as string) ?? key,
  }),
}));

const createSettings = (
  overrides: Partial<SettingsFormState> = {},
): SettingsFormState => ({
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableClaudePluginIntegration: false,
  claudeConfigDir: "/claude/custom",
  codexConfigDir: "/codex/custom",
  geminiConfigDir: "/gemini/custom",
  configDirectorySets: [
    {
      id: "primary",
      name: "默认环境",
      claudeConfigDir: "/claude/custom",
      codexConfigDir: "/codex/custom",
      geminiConfigDir: "/gemini/custom",
    },
  ],
  language: "zh",
  ...overrides,
});

describe("useDirectorySettings", () => {
  const onUpdateSettings = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();

    homeDirMock.mockResolvedValue("/home/mock");
    joinMock.mockImplementation(async (...segments: string[]) =>
      segments.join("/"),
    );

    getAppConfigDirOverrideMock.mockResolvedValue(null);
    getConfigDirMock.mockImplementation(async (app: string) =>
      app === "claude" ? "/remote/claude" : "/remote/codex",
    );
    selectConfigDirectoryMock.mockReset();
  });

  it("initializes directories using overrides and remote defaults", async () => {
    getAppConfigDirOverrideMock.mockResolvedValue("  /override/app  ");

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.appConfigDir).toBe("/override/app");
    expect(result.current.resolvedDirs).toEqual({
      appConfig: "/override/app",
      claude: "/remote/claude",
      codex: "/remote/codex",
      gemini: "/remote/codex", // Gemini 使用 codex 作为默认
    });
  });

  it("updates claude directory when browsing succeeds", async () => {
    selectConfigDirectoryMock.mockResolvedValue("/picked/claude");

    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({ claudeConfigDir: undefined }),
        onUpdateSettings,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("claude");
    });

    expect(selectConfigDirectoryMock).toHaveBeenCalledWith("/remote/claude");
    expect(onUpdateSettings).toHaveBeenCalledWith({
      claudeConfigDir: "/picked/claude",
    });
    expect(result.current.resolvedDirs.claude).toBe("/picked/claude");
  });

  it("reports error when directory selection fails", async () => {
    selectConfigDirectoryMock.mockResolvedValue(null);

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("codex");
    });

    expect(result.current.resolvedDirs.codex).toBe("/remote/codex");
    expect(onUpdateSettings).not.toHaveBeenCalledWith({
      codexConfigDir: expect.anything(),
    });
    expect(selectConfigDirectoryMock).toHaveBeenCalled();

    selectConfigDirectoryMock.mockRejectedValue(new Error("dialog failed"));
    toastErrorMock.mockClear();

    await act(async () => {
      await result.current.browseDirectory("codex");
    });

    expect(toastErrorMock).toHaveBeenCalled();
  });

  it("warns when directory selection promise rejects", async () => {
    selectConfigDirectoryMock.mockRejectedValue(new Error("dialog failed"));

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("codex");
    });

    expect(toastErrorMock).toHaveBeenCalled();
    expect(onUpdateSettings).not.toHaveBeenCalledWith({
      codexConfigDir: expect.anything(),
    });
  });

  it("updates app config directory via browseAppConfigDir", async () => {
    selectConfigDirectoryMock.mockResolvedValue("  /new/app  ");

    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings(),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseAppConfigDir();
    });

    expect(result.current.appConfigDir).toBe("/new/app");
    expect(selectConfigDirectoryMock).toHaveBeenCalledWith(
      "/home/mock/.cc-switch",
    );
  });

  it("resets directories to computed defaults", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({
          claudeConfigDir: "/custom/claude",
          codexConfigDir: "/custom/codex",
        }),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.resetDirectory("claude");
      await result.current.resetDirectory("codex");
      await result.current.resetAppConfigDir();
    });

    expect(onUpdateSettings).toHaveBeenCalledWith({
      claudeConfigDir: undefined,
    });
    expect(onUpdateSettings).toHaveBeenCalledWith({
      codexConfigDir: undefined,
    });
    expect(result.current.resolvedDirs.claude).toBe("/home/mock/.claude");
    expect(result.current.resolvedDirs.codex).toBe("/home/mock/.codex");
    expect(result.current.resolvedDirs.appConfig).toBe("/home/mock/.cc-switch");
  });

  it("resetAllDirectories applies provided resolved values", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    act(() => {
      result.current.resetAllDirectories("/server/claude", "/server/codex");
    });

    expect(result.current.resolvedDirs.claude).toBe("/server/claude");
    expect(result.current.resolvedDirs.codex).toBe("/server/codex");
  });

  it("adds a new config directory set", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings(),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    act(() => {
      result.current.addConfigDirectorySet();
    });

    const lastCall = onUpdateSettings.mock.calls.at(-1)?.[0];
    expect(lastCall?.configDirectorySets).toHaveLength(2);
  });

  it("updates secondary set directories via helper", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({
          configDirectorySets: [
            {
              id: "primary",
              name: "默认环境",
              claudeConfigDir: "/claude/custom",
              codexConfigDir: "/codex/custom",
              geminiConfigDir: "/gemini/custom",
            },
            {
              id: "wsl",
              name: "WSL",
              claudeConfigDir: "/wsl/claude",
              codexConfigDir: "/wsl/codex",
              geminiConfigDir: "/wsl/gemini",
            },
          ],
        }),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    act(() => {
      result.current.updateConfigDirectorySetDirectory(
        "wsl",
        "codex",
        "  /wsl/codex-new ",
      );
    });

    const lastCall = onUpdateSettings.mock.calls.at(-1)?.[0];
    expect(lastCall?.configDirectorySets).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "wsl",
          codexConfigDir: "/wsl/codex-new",
        }),
      ]),
    );
  });

  it("browses directories for non-primary set", async () => {
    selectConfigDirectoryMock.mockResolvedValue("/picked/wsl");
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({
          configDirectorySets: [
            {
              id: "primary",
              name: "默认环境",
              claudeConfigDir: "/claude/custom",
              codexConfigDir: "/codex/custom",
              geminiConfigDir: "/gemini/custom",
            },
            {
              id: "wsl",
              name: "WSL",
              claudeConfigDir: "/wsl/claude",
              codexConfigDir: "/wsl/codex",
              geminiConfigDir: "/wsl/gemini",
            },
          ],
        }),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseConfigDirectorySet("wsl", "claude");
    });

    expect(selectConfigDirectoryMock).toHaveBeenCalledWith("/wsl/claude");
    const lastCall = onUpdateSettings.mock.calls.at(-1)?.[0];
    expect(lastCall?.configDirectorySets).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "wsl",
          claudeConfigDir: "/picked/wsl",
        }),
      ]),
    );
  });

  it("resets directories on secondary set without touching primary", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({
          configDirectorySets: [
            {
              id: "primary",
              name: "默认环境",
              claudeConfigDir: "/claude/custom",
              codexConfigDir: "/codex/custom",
              geminiConfigDir: "/gemini/custom",
            },
            {
              id: "wsl",
              name: "WSL",
              claudeConfigDir: "/wsl/claude",
              codexConfigDir: "/wsl/codex",
              geminiConfigDir: "/wsl/gemini",
            },
          ],
        }),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.resetConfigDirectorySet("wsl", "claude");
    });

    const lastCall = onUpdateSettings.mock.calls.at(-1)?.[0];
    expect(lastCall?.configDirectorySets).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "wsl",
          claudeConfigDir: undefined,
        }),
      ]),
    );
  });
});
