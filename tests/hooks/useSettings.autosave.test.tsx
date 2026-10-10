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

describe("useSettings autosave debounce", () => {
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

  it("debounces rapid changes to directory fields", async () => {
    const { result } = renderHook(() => useSettings());

    // 快速连续调用 autoSaveSettings 3 次（模拟用户输入路径时逐字符编辑）
    act(() => {
      result.current.autoSaveSettings({ claudeConfigDir: "/home/u" });
      result.current.autoSaveSettings({ claudeConfigDir: "/home/us" });
      result.current.autoSaveSettings({ claudeConfigDir: "/home/user" });
    });

    // 前进 200ms（不到 300ms 防抖阈值）→ saveMutation 不应被调用
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(mutateAsyncMock).not.toHaveBeenCalled();

    // 再前进 150ms（总计 350ms > 300ms）→ saveMutation 应被调用 1 次
    await act(async () => {
      await vi.advanceTimersByTimeAsync(150);
    });

    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);

    // 验证保存的是最后一次调用的值
    const payload = mutateAsyncMock.mock.calls[0][0] as Settings;
    expect(payload.claudeConfigDir).toBe("/home/user");
  });

  it("flushes pending save immediately on flushAutoSaveSettings", async () => {
    const { result } = renderHook(() => useSettings());

    // 调用 autoSaveSettings 但不等待防抖完成
    act(() => {
      result.current.autoSaveSettings({ claudeConfigDir: "/home/user" });
    });

    // 前进 100ms（不到 300ms）→ 尚未触发保存
    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });
    expect(mutateAsyncMock).not.toHaveBeenCalled();

    // 模拟失焦：立即刷新待执行的保存
    let flushResult = false;
    act(() => {
      flushResult = result.current.flushAutoSaveSettings();
    });

    // 等待微任务队列刷新，让 executeAutoSave 完成
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);
    expect(flushResult).toBe(true);

    const payload = mutateAsyncMock.mock.calls[0][0] as Settings;
    expect(payload.claudeConfigDir).toBe("/home/user");
  });

  it("only saves the last value when multiple rapid calls are made", async () => {
    const { result } = renderHook(() => useSettings());

    // 模拟快速连续修改多个目录字段
    act(() => {
      result.current.autoSaveSettings({ claudeConfigDir: "/a" });
    });
    act(() => {
      result.current.autoSaveSettings({ codexConfigDir: "/b" });
    });
    act(() => {
      result.current.autoSaveSettings({
        claudeConfigDir: "/final",
        codexConfigDir: "/final-codex",
      });
    });

    // 前进超过 300ms
    await act(async () => {
      await vi.advanceTimersByTimeAsync(350);
    });

    // 只应保存 1 次（最后一次的值）
    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);

    const payload = mutateAsyncMock.mock.calls[0][0] as Settings;
    expect(payload.claudeConfigDir).toBe("/final");
    expect(payload.codexConfigDir).toBe("/final-codex");
  });

  it("returns false from flushAutoSaveSettings when no pending save exists", async () => {
    const { result } = renderHook(() => useSettings());

    let flushResult = true;
    act(() => {
      flushResult = result.current.flushAutoSaveSettings();
    });

    expect(flushResult).toBe(false);
    expect(mutateAsyncMock).not.toHaveBeenCalled();
  });

  it("resolves the returned promise after debounce fires", async () => {
    const { result } = renderHook(() => useSettings());

    let saveResult: { requiresRestart: boolean } | null = null;
    act(() => {
      result.current
        .autoSaveSettings({ claudeConfigDir: "/home/user" })
        .then((res) => {
          saveResult = res;
        });
    });

    // 前进超过 300ms 触发保存
    await act(async () => {
      await vi.advanceTimersByTimeAsync(350);
    });

    expect(saveResult).toEqual({ requiresRestart: false });
  });
});
