import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useConfigSets } from "@/hooks/useConfigSets";
import type { Settings } from "@/types";

const useSettingsQueryMock = vi.fn();
const mutateAsyncMock = vi.fn();
const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const syncCurrentProvidersLiveSafeMock = vi.fn();
const setQueryDataMock = vi.fn();
const settingsApiGetMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, any>) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    setQueryData: (...args: unknown[]) => setQueryDataMock(...args),
  }),
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: (...args: unknown[]) => useSettingsQueryMock(...args),
  useSaveSettingsMutation: () => ({
    mutateAsync: mutateAsyncMock,
    isPending: false,
  }),
}));

vi.mock("@/utils/postChangeSync", () => ({
  syncCurrentProvidersLiveSafe: (...args: unknown[]) =>
    syncCurrentProvidersLiveSafeMock(...args),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    get: (...args: unknown[]) => settingsApiGetMock(...args),
  },
}));

const baseSettings: Settings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableClaudePluginIntegration: false,
  claudeConfigDir: "/win/claude",
  codexConfigDir: "/win/codex",
  geminiConfigDir: "/win/gemini",
  currentProviderClaude: "claude-win",
  currentProviderCodex: "codex-win",
  currentProviderGemini: "gemini-win",
};

describe("useConfigSets", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mutateAsyncMock.mockResolvedValue(true);
    syncCurrentProvidersLiveSafeMock.mockResolvedValue({ ok: true });
    settingsApiGetMock.mockResolvedValue(undefined);
  });

  it("returns empty config list when settings are not loaded", () => {
    useSettingsQueryMock.mockReturnValue({ data: undefined });

    const { result } = renderHook(() => useConfigSets());

    expect(result.current.configSets).toEqual([]);
    expect(result.current.hasMultipleSets).toBe(false);
  });

  it("returns fallback config set when none defined", () => {
    const settings: Settings = {
      ...baseSettings,
      configDirectorySets: [],
      activeConfigDirectorySetId: undefined,
    };
    useSettingsQueryMock.mockReturnValue({ data: settings });
    settingsApiGetMock.mockResolvedValue(settings);

    const { result } = renderHook(() => useConfigSets());

    expect(result.current.configSets).toHaveLength(1);
    expect(result.current.configSets[0]).toMatchObject({
      claudeConfigDir: "/win/claude",
      currentProviderClaude: "claude-win",
    });
  });

  it("activates selected config set and reorders list", async () => {
    const settings: Settings = {
      ...baseSettings,
      configDirectorySets: [
        {
          id: "windows",
          name: "Windows",
          claudeConfigDir: "/win/claude",
          codexConfigDir: "/win/codex",
          geminiConfigDir: "/win/gemini",
          currentProviderClaude: "claude-win",
          currentProviderCodex: "codex-win",
          currentProviderGemini: "gemini-win",
        },
        {
          id: "wsl",
          name: "WSL",
          claudeConfigDir: "/wsl/claude",
          codexConfigDir: "/wsl/codex",
          geminiConfigDir: "/wsl/gemini",
          currentProviderClaude: "claude-wsl",
          currentProviderCodex: "codex-wsl",
          currentProviderGemini: "gemini-wsl",
        },
      ],
      activeConfigDirectorySetId: "windows",
    };
    useSettingsQueryMock.mockReturnValue({ data: settings });
    settingsApiGetMock.mockResolvedValue(settings);

    const { result } = renderHook(() => useConfigSets());

    await act(async () => {
      const success = await result.current.activateConfigSet("wsl");
      expect(success).toBe(true);
    });

    expect(mutateAsyncMock).toHaveBeenCalledWith(
      expect.objectContaining({
        claudeConfigDir: "/wsl/claude",
        codexConfigDir: "/wsl/codex",
        geminiConfigDir: "/wsl/gemini",
        activeConfigDirectorySetId: "wsl",
        currentProviderClaude: "claude-wsl",
        currentProviderCodex: "codex-wsl",
        currentProviderGemini: "gemini-wsl",
        configDirectorySets: [
          expect.objectContaining({
            id: "wsl",
            currentProviderClaude: "claude-wsl",
          }),
          expect.objectContaining({
            id: "windows",
            currentProviderClaude: "claude-win",
          }),
        ],
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalled();
    expect(syncCurrentProvidersLiveSafeMock).toHaveBeenCalledTimes(1);
  });

  it("uses latest fetched settings when cached data is stale", async () => {
    const staleSettings: Settings = {
      ...baseSettings,
      configDirectorySets: [
        {
          id: "windows",
          name: "Windows",
          claudeConfigDir: "/win/claude",
          codexConfigDir: "/win/codex",
          geminiConfigDir: "/win/gemini",
          currentProviderClaude: "claude-win",
        },
        {
          id: "wsl",
          name: "WSL",
          claudeConfigDir: "/wsl/claude",
          codexConfigDir: "/wsl/codex",
          geminiConfigDir: "/wsl/gemini",
          currentProviderClaude: "claude-old",
        },
      ],
      activeConfigDirectorySetId: "windows",
    };
    const freshSettings: Settings = {
      ...staleSettings,
      configDirectorySets: [
        staleSettings.configDirectorySets![0],
        {
          ...staleSettings.configDirectorySets![1],
          currentProviderClaude: "claude-wsl-new",
        },
      ],
    };

    useSettingsQueryMock.mockReturnValue({ data: staleSettings });
    settingsApiGetMock.mockResolvedValue(freshSettings);

    const { result } = renderHook(() => useConfigSets());

    await act(async () => {
      const success = await result.current.activateConfigSet("wsl");
      expect(success).toBe(true);
    });

    expect(settingsApiGetMock).toHaveBeenCalled();
    expect(mutateAsyncMock).toHaveBeenCalledWith(
      expect.objectContaining({
        currentProviderClaude: "claude-wsl-new",
      }),
    );
  });
});
