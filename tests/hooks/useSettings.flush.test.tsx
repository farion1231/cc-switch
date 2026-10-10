import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useSettings } from "@/hooks/useSettings";
import type { Settings } from "@/types";

const mutateAsyncMock = vi.fn();
const useSettingsQueryMock = vi.fn();
const applyClaudePluginConfigMock = vi.fn();
const applyClaudeOnboardingSkipMock = vi.fn();
const clearClaudeOnboardingSkipMock = vi.fn();
const syncCurrentProvidersLiveMock = vi.fn();
const updateTrayMenuMock = vi.fn();
const getCurrentMock = vi.fn();
const getAllMock = vi.fn();
const getQueryDataMock = vi.fn();
const invalidatePiDirectoryCachesMock = vi.fn();
const toastErrorMock = vi.fn();
const toastSuccessMock = vi.fn();

let settingsFormMock: any;
let directorySettingsMock: any;
let metadataMock: any;
let serverSettings: Settings;

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("@/hooks/useSettingsForm", () => ({
  useSettingsForm: () => settingsFormMock,
}));

vi.mock("@/hooks/useDirectorySettings", () => ({
  useDirectorySettings: () => directorySettingsMock,
}));

vi.mock("@/hooks/useSettingsMetadata", () => ({
  useSettingsMetadata: () => metadataMock,
}));

vi.mock("@/lib/query", () => ({
  invalidatePiDirectoryCaches: (...args: unknown[]) =>
    invalidatePiDirectoryCachesMock(...args),
  useSettingsQuery: (...args: unknown[]) => useSettingsQueryMock(...args),
  useSaveSettingsMutation: () => ({
    mutateAsync: mutateAsyncMock,
    isPending: false,
  }),
}));

vi.mock("@tanstack/react-query", async () => {
  const actual = await vi.importActual<typeof import("@tanstack/react-query")>(
    "@tanstack/react-query",
  );
  return {
    ...actual,
    useQueryClient: () => ({
      getQueryData: (...args: unknown[]) => getQueryDataMock(...args),
    }),
  };
});

vi.mock("@/lib/api", () => ({
  settingsApi: {
    applyClaudePluginConfig: (...args: unknown[]) =>
      applyClaudePluginConfigMock(...args),
    applyClaudeOnboardingSkip: (...args: unknown[]) =>
      applyClaudeOnboardingSkipMock(...args),
    clearClaudeOnboardingSkip: (...args: unknown[]) =>
      clearClaudeOnboardingSkipMock(...args),
    syncCurrentProvidersLive: (...args: unknown[]) =>
      syncCurrentProvidersLiveMock(...args),
  },
  providersApi: {
    updateTrayMenu: (...args: unknown[]) => updateTrayMenuMock(...args),
    getCurrent: (...args: unknown[]) => getCurrentMock(...args),
    getAll: (...args: unknown[]) => getAllMock(...args),
  },
}));

const createSettingsFormMock = (overrides: Record<string, unknown> = {}) => ({
  settings: {
    showInTray: true,
    minimizeToTrayOnClose: true,
    enableClaudePluginIntegration: false,
    skipClaudeOnboarding: true,
    claudeConfigDir: "/claude",
    codexConfigDir: "/codex",
    geminiConfigDir: "/gemini",
    opencodeConfigDir: "/opencode",
    openclawConfigDir: "/openclaw",
    hermesConfigDir: "/hermes",
    piConfigDir: "/pi",
    language: "zh",
  },
  isLoading: false,
  initialLanguage: "zh",
  updateSettings: vi.fn(),
  resetSettings: vi.fn(),
  syncLanguage: vi.fn(),
  ...overrides,
});

const createDirectorySettingsMock = (
  overrides: Record<string, unknown> = {},
) => ({
  appConfigDir: undefined,
  resolvedDirs: {
    appConfig: "/home/mock/.cc-switch",
    claude: "/default/claude",
    codex: "/default/codex",
    gemini: "/default/gemini",
    opencode: "/default/opencode",
    openclaw: "/default/openclaw",
    hermes: "/default/hermes",
    pi: "/default/pi",
  },
  isLoading: false,
  initialAppConfigDir: undefined,
  commitAppConfigDir: vi.fn(),
  updateDirectory: vi.fn(),
  updateAppConfigDir: vi.fn(),
  browseDirectory: vi.fn(),
  browseAppConfigDir: vi.fn(),
  resetDirectory: vi.fn(),
  resetAppConfigDir: vi.fn(),
  resetAllDirectories: vi.fn(),
  ...overrides,
});

const createMetadataMock = (overrides: Record<string, unknown> = {}) => ({
  isPortable: false,
  requiresRestart: false,
  isLoading: false,
  acknowledgeRestart: vi.fn(),
  setRequiresRestart: vi.fn(),
  ...overrides,
});

describe("useSettings flushAutoSaveSettings", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mutateAsyncMock.mockReset();
    useSettingsQueryMock.mockReset();
    applyClaudePluginConfigMock.mockReset();
    applyClaudeOnboardingSkipMock.mockReset();
    clearClaudeOnboardingSkipMock.mockReset();
    syncCurrentProvidersLiveMock.mockReset();
    invalidatePiDirectoryCachesMock.mockReset();
    getCurrentMock.mockReset();
    getAllMock.mockReset();
    getQueryDataMock.mockReset();
    toastErrorMock.mockReset();
    toastSuccessMock.mockReset();
    window.localStorage.clear();

    serverSettings = {
      showInTray: true,
      minimizeToTrayOnClose: true,
      enableClaudePluginIntegration: false,
      skipClaudeOnboarding: true,
      claudeConfigDir: "/server/claude",
      codexConfigDir: "/server/codex",
      geminiConfigDir: "/server/gemini",
      opencodeConfigDir: "/server/opencode",
      openclawConfigDir: "/server/openclaw",
      hermesConfigDir: "/server/hermes",
      piConfigDir: "/server/pi",
      language: "zh",
    };

    useSettingsQueryMock.mockReturnValue({
      data: serverSettings,
      isLoading: false,
    });

    settingsFormMock = createSettingsFormMock({
      settings: {
        ...serverSettings,
        language: "zh",
      },
    });
    directorySettingsMock = createDirectorySettingsMock();
    metadataMock = createMetadataMock();

    mutateAsyncMock.mockResolvedValue(true);
    applyClaudePluginConfigMock.mockResolvedValue(true);
    applyClaudeOnboardingSkipMock.mockResolvedValue(true);
    clearClaudeOnboardingSkipMock.mockResolvedValue(true);
    syncCurrentProvidersLiveMock.mockResolvedValue({ ok: true });
    getCurrentMock.mockResolvedValue(null);
    getAllMock.mockResolvedValue({});
    getQueryDataMock.mockImplementation(() => serverSettings);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("flushes pending save on blur before debounce fires", async () => {
    const { result } = renderHook(() => useSettings());

    // 1. 调用 autoSaveSettings 启动防抖定时器（模拟用户在目录输入框中编辑）
    act(() => {
      result.current.autoSaveSettings({ claudeConfigDir: "/home/user" });
    });

    // 前进 100ms（不到 300ms 防抖阈值）→ saveMutation 不应被调用
    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });
    expect(mutateAsyncMock).not.toHaveBeenCalled();

    // 2. 模拟 blur 事件：在防抖触发前调用 flushAutoSaveSettings
    let flushResult = false;
    act(() => {
      flushResult = result.current.flushAutoSaveSettings();
    });

    // 等待微任务队列刷新，让 executeAutoSave 完成
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    // 3. 断言：saveMutation 被立即调用（不再等待防抖）
    expect(flushResult).toBe(true);
    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);

    const payload = mutateAsyncMock.mock.calls[0][0] as Settings;
    expect(payload.claudeConfigDir).toBe("/home/user");
  });

  it("returns false from flushAutoSaveSettings when no pending save exists", async () => {
    const { result } = renderHook(() => useSettings());

    // 没有调用 autoSaveSettings，直接调用 flush
    let flushResult = true;
    act(() => {
      flushResult = result.current.flushAutoSaveSettings();
    });

    // 断言：返回 false（无待保存的内容）
    expect(flushResult).toBe(false);
    expect(mutateAsyncMock).not.toHaveBeenCalled();
  });

  it("subsequent flushAutoSaveSettings returns false after already flushed", async () => {
    const { result } = renderHook(() => useSettings());

    // 启动防抖保存
    act(() => {
      result.current.autoSaveSettings({ claudeConfigDir: "/home/user" });
    });

    // 第一次 flush → 应成功
    let firstFlush = false;
    act(() => {
      firstFlush = result.current.flushAutoSaveSettings();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(firstFlush).toBe(true);
    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);

    // 第二次 flush → 应返回 false（无待保存）
    let secondFlush = true;
    act(() => {
      secondFlush = result.current.flushAutoSaveSettings();
    });

    expect(secondFlush).toBe(false);
    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);
  });

  it("resolves the pending promise after flush", async () => {
    const { result } = renderHook(() => useSettings());

    let saveResult: { requiresRestart: boolean } | null = null;
    act(() => {
      result.current
        .autoSaveSettings({ claudeConfigDir: "/home/user" })
        .then((res) => {
          saveResult = res;
        });
    });

    // 在防抖触发前 flush
    act(() => {
      result.current.flushAutoSaveSettings();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    // Promise 应已 resolve
    expect(saveResult).toEqual({ requiresRestart: false });
  });
});
