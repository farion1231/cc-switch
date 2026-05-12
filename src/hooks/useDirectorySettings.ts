import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { homeDir, join } from "@tauri-apps/api/path";
import { settingsApi, type AppId } from "@/lib/api";
import type { SettingsFormState } from "./useSettingsForm";

export type DirectoryAppId = Exclude<AppId, "claude-desktop">;
type AppDirectoryKey =
  | "claude"
  | "codex"
  | "codexWsl"
  | "gemini"
  | "geminiWsl"
  | "opencode"
  | "opencodeWsl"
  | "openclaw"
  | "openclawWsl"
  | "hermes";
type DirectoryKey = "appConfig" | AppDirectoryKey | "claudeWsl";

export interface ResolvedDirectories {
  appConfig: string;
  claude: string;
  codex: string;
  gemini: string;
  opencode: string;
  openclaw: string;
  hermes: string;
}

// Single source of truth for per-app directory metadata.
const APP_DIRECTORY_META: Record<
  DirectoryAppId,
  { key: AppDirectoryKey; defaultFolder: string }
> = {
  claude: { key: "claude", defaultFolder: ".claude" },
  codex: { key: "codex", defaultFolder: ".codex" },
  gemini: { key: "gemini", defaultFolder: ".gemini" },
  opencode: { key: "opencode", defaultFolder: ".config/opencode" },
  openclaw: { key: "openclaw", defaultFolder: ".openclaw" },
  hermes: { key: "hermes", defaultFolder: ".hermes" },
};

const DIRECTORY_KEY_TO_SETTINGS_FIELD: Record<
  AppDirectoryKey,
  keyof SettingsFormState
> = {
  claude: "claudeConfigDir",
  codex: "codexConfigDir",
  gemini: "geminiConfigDir",
  opencode: "opencodeConfigDir",
  openclaw: "openclawConfigDir",
  hermes: "hermesConfigDir",
};

export interface CliDetectionItem {
  app: AppId;
  native: {
    envType: "windows" | "wsl" | "macos" | "linux" | "unknown";
    executablePath?: string | null;
    configDir: string;
    configExists: boolean;
  };
  wsl?: {
    envType: "windows" | "wsl" | "macos" | "linux" | "unknown";
    distro: string;
    executablePath?: string | null;
    configDir: string;
    configExists: boolean;
  } | null;
}

export interface CliDetectionMap {
  claude?: CliDetectionItem;
  codex?: CliDetectionItem;
  gemini?: CliDetectionItem;
  opencode?: CliDetectionItem;
}

const isCliDetectableApp = (app: string): app is keyof CliDetectionMap =>
  app === "claude" || app === "codex" || app === "gemini" || app === "opencode";

const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

const computeDefaultAppConfigDir = async (): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    return await join(home, ".cc-switch");
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to resolve default app config dir",
      error,
    );
    return undefined;
  }
};

const computeDefaultConfigDir = async (
  app: DirectoryAppId,
): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    return await join(home, APP_DIRECTORY_META[app].defaultFolder);
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to resolve default config dir",
      error,
    );
    return undefined;
  }
};

export interface UseDirectorySettingsProps {
  settings: SettingsFormState | null;
  onUpdateSettings: (updates: Partial<SettingsFormState>) => void;
}

export interface UseDirectorySettingsResult {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  cliDetections: CliDetectionMap;
  cliDetectionMeta: {
    isLoading: boolean;
    wslInstalled: boolean;
    wslDistro?: string;
  };
  isLoading: boolean;
  initialAppConfigDir?: string;
  updateDirectory: (app: DirectoryAppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: DirectoryAppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  browseClaudeWslDirectory: () => Promise<void>;
  updateClaudeWslDirectory: (value?: string) => void;
  resetDirectory: (app: AppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  resetClaudeWslDirectory: () => Promise<void>;
  resetAllDirectories: (overrides?: ResolvedAppDirectoryOverrides) => void;
  updateWslDirectory: (app: AppId, value?: string) => void;
  browseWslDirectory: (app: AppId) => Promise<void>;
  resetWslDirectory: (app: AppId) => Promise<void>;
}

export type ResolvedAppDirectoryOverrides = Partial<
  Record<AppDirectoryKey, string | undefined>
>;

/**
 * useDirectorySettings - 目录管理
 * 负责：
 * - appConfigDir 状态
 * - resolvedDirs 状态
 * - 目录选择（browse）
 * - 目录重置
 * - 默认值计算
 */
export function useDirectorySettings({
  settings,
  onUpdateSettings,
}: UseDirectorySettingsProps): UseDirectorySettingsResult {
  const { t } = useTranslation();

  const [appConfigDir, setAppConfigDir] = useState<string | undefined>(
    undefined,
  );
  const [resolvedDirs, setResolvedDirs] = useState<ResolvedDirectories>({
    appConfig: "",
    claude: "",
    codex: "",
    gemini: "",
    opencode: "",
    openclaw: "",
    hermes: "",
  });
  const [cliDetections, setCliDetections] = useState<CliDetectionMap>({});
  const [cliDetectionMeta, setCliDetectionMeta] = useState<{
    isLoading: boolean;
    wslInstalled: boolean;
    wslDistro?: string;
  }>({
    isLoading: true,
    wslInstalled: false,
  });
  const [isLoading, setIsLoading] = useState(true);

  const defaultsRef = useRef<ResolvedDirectories>({
    appConfig: "",
    claude: "",
    codex: "",
    gemini: "",
    opencode: "",
    openclaw: "",
    hermes: "",
  });
  const initialAppConfigDirRef = useRef<string | undefined>(undefined);

  // 加载目录信息
  useEffect(() => {
    let active = true;
    setIsLoading(true);

    const load = async () => {
      try {
        const [
          overrideRaw,
          claudeDir,
          codexDir,
          geminiDir,
          opencodeDir,
          openclawDir,
          hermesDir,
          defaultAppConfig,
          defaultClaudeDir,
          defaultCodexDir,
          defaultGeminiDir,
          defaultOpencodeDir,
          defaultOpenclawDir,
          defaultHermesDir,
        ] = await Promise.all([
          settingsApi.getAppConfigDirOverride(),
          settingsApi.getConfigDir("claude"),
          settingsApi.getConfigDir("codex"),
          settingsApi.getConfigDir("gemini"),
          settingsApi.getConfigDir("opencode"),
          settingsApi.getConfigDir("openclaw"),
          settingsApi.getConfigDir("hermes"),
          computeDefaultAppConfigDir(),
          computeDefaultConfigDir("claude"),
          computeDefaultConfigDir("codex"),
          computeDefaultConfigDir("gemini"),
          computeDefaultConfigDir("opencode"),
          computeDefaultConfigDir("openclaw"),
          computeDefaultConfigDir("hermes"),
        ]);

        if (!active) return;

        const normalizedOverride = sanitizeDir(overrideRaw ?? undefined);

        defaultsRef.current = {
          appConfig: defaultAppConfig ?? "",
          claude: defaultClaudeDir ?? "",
          codex: defaultCodexDir ?? "",
          gemini: defaultGeminiDir ?? "",
          opencode: defaultOpencodeDir ?? "",
          openclaw: defaultOpenclawDir ?? "",
          hermes: defaultHermesDir ?? "",
        };

        setAppConfigDir(normalizedOverride);
        initialAppConfigDirRef.current = normalizedOverride;

        setResolvedDirs({
          appConfig: normalizedOverride ?? defaultsRef.current.appConfig,
          claude: claudeDir || defaultsRef.current.claude,
          codex: codexDir || defaultsRef.current.codex,
          gemini: geminiDir || defaultsRef.current.gemini,
          opencode: opencodeDir || defaultsRef.current.opencode,
          openclaw: openclawDir || defaultsRef.current.openclaw,
          hermes: hermesDir || defaultsRef.current.hermes,
        });
      } catch (error) {
        console.error(
          "[useDirectorySettings] Failed to load directory info",
          error,
        );
      } finally {
        if (active) {
          setIsLoading(false);
        }
      }
    };

    void load();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;

    const loadCliDetections = async () => {
      try {
        const summary = await settingsApi.detectCliTools();
        if (!active) return;

        const mapped = summary.tools.reduce<CliDetectionMap>((acc, item) => {
          if (isCliDetectableApp(item.app)) {
            acc[item.app] = {
              app: item.app,
              native: item.native,
              wsl: item.wsl,
            };
          }
          return acc;
        }, {});

        setCliDetections(mapped);
        setCliDetectionMeta({
          isLoading: false,
          wslInstalled: summary.wslInstalled,
          wslDistro: summary.wslDistro ?? undefined,
        });
      } catch (error) {
        console.error(
          "[useDirectorySettings] Failed to load CLI detection info",
          error,
        );
        if (!active) return;
        setCliDetectionMeta({
          isLoading: false,
          wslInstalled: false,
        });
      }
    };

    void loadCliDetections();
    return () => {
      active = false;
    };
  }, []);

  const normalizedCliDetections = useMemo(() => cliDetections, [cliDetections]);

  const updateDirectoryState = useCallback(
    (key: DirectoryKey, value?: string) => {
      const sanitized = sanitizeDir(value);
      if (key === "appConfig") {
        setAppConfigDir(sanitized);
      } else if (key === "claudeWsl") {
        onUpdateSettings({ claudeConfigDirWsl: sanitized });
      } else if (key === "codexWsl") {
        onUpdateSettings({ codexConfigDirWsl: sanitized });
      } else if (key === "geminiWsl") {
        onUpdateSettings({ geminiConfigDirWsl: sanitized });
      } else if (key === "opencodeWsl") {
        onUpdateSettings({ opencodeConfigDirWsl: sanitized });
      } else if (key === "openclawWsl") {
        onUpdateSettings({ openclawConfigDirWsl: sanitized });
      } else {
        onUpdateSettings({
          [DIRECTORY_KEY_TO_SETTINGS_FIELD[key]]: sanitized,
        });
      }

      if (
        key === "claudeWsl" ||
        key === "codexWsl" ||
        key === "geminiWsl" ||
        key === "opencodeWsl" ||
        key === "openclawWsl"
      ) {
        return;
      }

      setResolvedDirs((prev) => {
        const next = sanitized ?? defaultsRef.current[key];
        // Same-ref early-return: unchanged value shouldn't cascade renders
        // through the settings tree.
        if (prev[key] === next) return prev;
        return { ...prev, [key]: next };
      });
    },
    [onUpdateSettings],
  );

  const updateAppConfigDir = useCallback(
    (value?: string) => {
      updateDirectoryState("appConfig", value);
    },
    [updateDirectoryState],
  );

  const updateDirectory = useCallback(
    (app: DirectoryAppId, value?: string) => {
      updateDirectoryState(APP_DIRECTORY_META[app].key, value);
    },
    [updateDirectoryState],
  );

  const browseDirectory = useCallback(
    async (app: DirectoryAppId) => {
      const key = APP_DIRECTORY_META[app].key;
      const settingsField = DIRECTORY_KEY_TO_SETTINGS_FIELD[key];
      const currentValue =
        (settings?.[settingsField] as string | undefined) ?? resolvedDirs[key];

      try {
        const picked = await settingsApi.selectConfigDirectory(currentValue);
        const sanitized = sanitizeDir(picked ?? undefined);
        if (!sanitized) return;
        updateDirectoryState(key, sanitized);
      } catch (error) {
        console.error("[useDirectorySettings] Failed to pick directory", error);
        toast.error(
          t("settings.selectFileFailed", {
            defaultValue: "选择目录失败",
          }),
        );
      }
    },
    [settings, resolvedDirs, t, updateDirectoryState],
  );

  const browseClaudeWslDirectory = useCallback(async () => {
    const currentValue = settings?.claudeConfigDirWsl ?? undefined;
    try {
      const picked = await settingsApi.selectConfigDirectory(currentValue);
      const sanitized = sanitizeDir(picked ?? undefined);
      if (!sanitized) return;
      updateDirectoryState("claudeWsl", sanitized);
    } catch (error) {
      console.error("[useDirectorySettings] Failed to pick WSL directory", error);
      toast.error(
        t("settings.selectFileFailed", {
          defaultValue: "选择目录失败",
        }),
      );
    }
  }, [settings, t, updateDirectoryState]);

  const browseAppConfigDir = useCallback(async () => {
    const currentValue = appConfigDir ?? resolvedDirs.appConfig;
    try {
      const picked = await settingsApi.selectConfigDirectory(currentValue);
      const sanitized = sanitizeDir(picked ?? undefined);
      if (!sanitized) return;
      updateDirectoryState("appConfig", sanitized);
    } catch (error) {
      console.error(
        "[useDirectorySettings] Failed to pick app config directory",
        error,
      );
      toast.error(
        t("settings.selectFileFailed", {
          defaultValue: "选择目录失败",
        }),
      );
    }
  }, [appConfigDir, resolvedDirs.appConfig, t, updateDirectoryState]);

  const resetDirectory = useCallback(
    async (app: DirectoryAppId) => {
      const key = APP_DIRECTORY_META[app].key;
      if (!defaultsRef.current[key]) {
        const fallback = await computeDefaultConfigDir(app);
        if (fallback) {
          defaultsRef.current = {
            ...defaultsRef.current,
            [key]: fallback,
          };
        }
      }
      updateDirectoryState(key, undefined);
    },
    [updateDirectoryState],
  );

  const resetAppConfigDir = useCallback(async () => {
    if (!defaultsRef.current.appConfig) {
      const fallback = await computeDefaultAppConfigDir();
      if (fallback) {
        defaultsRef.current = {
          ...defaultsRef.current,
          appConfig: fallback,
        };
      }
    }
    updateDirectoryState("appConfig", undefined);
  }, [updateDirectoryState]);

  const updateClaudeWslDirectory = useCallback(
    (value?: string) => {
      updateDirectoryState("claudeWsl", value);
    },
    [updateDirectoryState],
  );

  const resetClaudeWslDirectory = useCallback(async () => {
    updateDirectoryState("claudeWsl", undefined);
  }, [updateDirectoryState]);

  const wslKeyForApp = useCallback((app: AppId): DirectoryKey => {
    switch (app) {
      case "codex":
        return "codexWsl";
      case "gemini":
        return "geminiWsl";
      case "opencode":
        return "opencodeWsl";
      case "openclaw":
        return "openclawWsl";
      default:
        return "codexWsl";
    }
  }, []);

  const updateWslDirectory = useCallback(
    (app: AppId, value?: string) => {
      updateDirectoryState(wslKeyForApp(app), value);
    },
    [updateDirectoryState, wslKeyForApp],
  );

  const browseWslDirectory = useCallback(
    async (app: AppId) => {
      const settingsKeyMap: Record<AppId, keyof SettingsFormState | undefined> = {
        claude: "claudeConfigDirWsl",
        codex: "codexConfigDirWsl",
        gemini: "geminiConfigDirWsl",
        opencode: "opencodeConfigDirWsl",
        openclaw: "openclawConfigDirWsl",
      };
      const settingsKey = settingsKeyMap[app];
      const currentValue = settingsKey
        ? (settings?.[settingsKey] as string | undefined)
        : undefined;
      try {
        const picked = await settingsApi.selectConfigDirectory(currentValue);
        const sanitized = sanitizeDir(picked ?? undefined);
        if (!sanitized) return;
        updateWslDirectory(app, sanitized);
      } catch (error) {
        console.error(
          "[useDirectorySettings] Failed to pick WSL directory",
          error,
        );
        toast.error(
          t("settings.selectFileFailed", {
            defaultValue: "选择目录失败",
          }),
        );
      }
    },
    [settings, t, updateWslDirectory],
  );

  const resetWslDirectory = useCallback(
    async (app: AppId) => {
      updateWslDirectory(app, undefined);
    },
    [updateWslDirectory],
  );

  const resetAllDirectories = useCallback(
    (overrides?: ResolvedAppDirectoryOverrides) => {
      setAppConfigDir(initialAppConfigDirRef.current);
      setResolvedDirs({
        appConfig:
          initialAppConfigDirRef.current ?? defaultsRef.current.appConfig,
        claude: overrides?.claude ?? defaultsRef.current.claude,
        codex: overrides?.codex ?? defaultsRef.current.codex,
        gemini: overrides?.gemini ?? defaultsRef.current.gemini,
        opencode: overrides?.opencode ?? defaultsRef.current.opencode,
        openclaw: overrides?.openclaw ?? defaultsRef.current.openclaw,
        hermes: overrides?.hermes ?? defaultsRef.current.hermes,
      });
    },
    [],
  );

  return {
    appConfigDir,
    resolvedDirs,
    cliDetections: normalizedCliDetections,
    cliDetectionMeta,
    isLoading,
    initialAppConfigDir: initialAppConfigDirRef.current,
    updateDirectory,
    updateAppConfigDir,
    updateClaudeWslDirectory,
    browseDirectory,
    browseAppConfigDir,
    browseClaudeWslDirectory,
    resetDirectory,
    resetAppConfigDir,
    resetClaudeWslDirectory,
    updateWslDirectory,
    browseWslDirectory,
    resetWslDirectory,
    resetAllDirectories,
  };
}
