import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { homeDir, join } from "@tauri-apps/api/path";
import { settingsApi, type AppId } from "@/lib/api";
import type { ConfigDirectorySet } from "@/types";
import { generateConfigDirectorySetId } from "@/utils/id";
import type { SettingsFormState } from "./useSettingsForm";

type DirectoryKey = "appConfig" | "claude" | "codex" | "gemini";

export interface ResolvedDirectories {
  appConfig: string;
  claude: string;
  codex: string;
  gemini: string;
}

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
  app: AppId,
): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    const folder =
      app === "claude" ? ".claude" : app === "codex" ? ".codex" : ".gemini";
    return await join(home, folder);
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
  isLoading: boolean;
  initialAppConfigDir?: string;
  configDirectorySets: ConfigDirectorySet[];
  updateDirectory: (app: AppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: AppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: AppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  addConfigDirectorySet: () => void;
  removeConfigDirectorySet: (setId: string) => void;
  updateConfigDirectorySet: (
    setId: string,
    updates: Partial<ConfigDirectorySet>,
  ) => void;
  updateConfigDirectorySetDirectory: (
    setId: string,
    app: AppId,
    value?: string,
  ) => void;
  browseConfigDirectorySet: (setId: string, app: AppId) => Promise<void>;
  resetConfigDirectorySet: (setId: string, app: AppId) => Promise<void>;
  resetAllDirectories: (
    claudeDir?: string,
    codexDir?: string,
    geminiDir?: string,
  ) => void;
}

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
  });
  const [isLoading, setIsLoading] = useState(true);

  const defaultsRef = useRef<ResolvedDirectories>({
    appConfig: "",
    claude: "",
    codex: "",
    gemini: "",
  });
  const initialAppConfigDirRef = useRef<string | undefined>(undefined);
  const getConfigSetName = useCallback(
    (position: number) =>
      position === 1
        ? t("settings.configSetDefaultName", { defaultValue: "默认环境" })
        : t("settings.configSetNameTemplate", {
            index: position,
            defaultValue: `配置组 ${position}`,
          }),
    [t],
  );

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
          defaultAppConfig,
          defaultClaudeDir,
          defaultCodexDir,
          defaultGeminiDir,
        ] = await Promise.all([
          settingsApi.getAppConfigDirOverride(),
          settingsApi.getConfigDir("claude"),
          settingsApi.getConfigDir("codex"),
          settingsApi.getConfigDir("gemini"),
          computeDefaultAppConfigDir(),
          computeDefaultConfigDir("claude"),
          computeDefaultConfigDir("codex"),
          computeDefaultConfigDir("gemini"),
        ]);

        if (!active) return;

        const normalizedOverride = sanitizeDir(overrideRaw ?? undefined);

        defaultsRef.current = {
          appConfig: defaultAppConfig ?? "",
          claude: defaultClaudeDir ?? "",
          codex: defaultCodexDir ?? "",
          gemini: defaultGeminiDir ?? "",
        };

        setAppConfigDir(normalizedOverride);
        initialAppConfigDirRef.current = normalizedOverride;

        setResolvedDirs({
          appConfig: normalizedOverride ?? defaultsRef.current.appConfig,
          claude: claudeDir || defaultsRef.current.claude,
          codex: codexDir || defaultsRef.current.codex,
          gemini: geminiDir || defaultsRef.current.gemini,
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

  const updateDirectoryState = useCallback(
    (key: DirectoryKey, value?: string) => {
      const sanitized = sanitizeDir(value);
      if (key === "appConfig") {
        setAppConfigDir(sanitized);
      } else {
        onUpdateSettings(
          key === "claude"
            ? { claudeConfigDir: sanitized }
            : key === "codex"
              ? { codexConfigDir: sanitized }
              : { geminiConfigDir: sanitized },
        );
      }

      setResolvedDirs((prev) => ({
        ...prev,
        [key]: sanitized ?? defaultsRef.current[key],
      }));
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
    (app: AppId, value?: string) => {
      updateDirectoryState(
        app === "claude" ? "claude" : app === "codex" ? "codex" : "gemini",
        value,
      );
    },
    [updateDirectoryState],
  );

  const browseDirectory = useCallback(
    async (app: AppId) => {
      const key: DirectoryKey =
        app === "claude" ? "claude" : app === "codex" ? "codex" : "gemini";
      const currentValue =
        key === "claude"
          ? (settings?.claudeConfigDir ?? resolvedDirs.claude)
          : key === "codex"
            ? (settings?.codexConfigDir ?? resolvedDirs.codex)
            : (settings?.geminiConfigDir ?? resolvedDirs.gemini);

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
    async (app: AppId) => {
      const key: DirectoryKey =
        app === "claude" ? "claude" : app === "codex" ? "codex" : "gemini";
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

  const resetAllDirectories = useCallback(
    (claudeDir?: string, codexDir?: string, geminiDir?: string) => {
      setAppConfigDir(initialAppConfigDirRef.current);
      setResolvedDirs({
        appConfig:
          initialAppConfigDirRef.current ?? defaultsRef.current.appConfig,
        claude: claudeDir ?? defaultsRef.current.claude,
        codex: codexDir ?? defaultsRef.current.codex,
        gemini: geminiDir ?? defaultsRef.current.gemini,
      });
    },
    [],
  );

  const addConfigDirectorySet = useCallback(() => {
    const currentSets = settings?.configDirectorySets ?? [];

    const nextSet: ConfigDirectorySet = {
      id: generateConfigDirectorySetId(),
      name: getConfigSetName(currentSets.length + 1),
      claudeConfigDir: undefined,
      codexConfigDir: undefined,
      geminiConfigDir: undefined,
    };

    onUpdateSettings({
      configDirectorySets: [...currentSets, nextSet],
    });
  }, [getConfigSetName, onUpdateSettings, settings?.configDirectorySets]);

  const removeConfigDirectorySet = useCallback(
    (setId: string) => {
      const currentSets = settings?.configDirectorySets ?? [];
      if (currentSets.length <= 1) return;

      const filtered = currentSets.filter((set, index) => {
        if (index === 0 && set.id === setId) {
          return true;
        }
        return set.id !== setId;
      });

      if (filtered.length === currentSets.length) return;

      onUpdateSettings({ configDirectorySets: filtered });
    },
    [onUpdateSettings, settings?.configDirectorySets],
  );

  const updateConfigDirectorySet = useCallback(
    (setId: string, updates: Partial<ConfigDirectorySet>) => {
      const currentSets = settings?.configDirectorySets ?? [];
      if (!currentSets.length) return;

      const trimmedName =
        typeof updates.name === "string"
          ? updates.name.trim().slice(0, 60)
          : undefined;

      const nextSets = currentSets.map((set, index) => {
        if (set.id !== setId) return set;
        const fallbackName = getConfigSetName(index + 1);

        return {
          ...set,
          ...(trimmedName !== undefined
            ? { name: trimmedName.length > 0 ? trimmedName : fallbackName }
            : {}),
          ...(Object.prototype.hasOwnProperty.call(updates, "claudeConfigDir")
            ? { claudeConfigDir: sanitizeDir(updates.claudeConfigDir) }
            : {}),
          ...(Object.prototype.hasOwnProperty.call(updates, "codexConfigDir")
            ? { codexConfigDir: sanitizeDir(updates.codexConfigDir) }
            : {}),
          ...(Object.prototype.hasOwnProperty.call(updates, "geminiConfigDir")
            ? { geminiConfigDir: sanitizeDir(updates.geminiConfigDir) }
            : {}),
        };
      });

      onUpdateSettings({ configDirectorySets: nextSets });
    },
    [getConfigSetName, onUpdateSettings, settings?.configDirectorySets],
  );

  const updateConfigDirectorySetDirectory = useCallback(
    (setId: string, app: AppId, value?: string) => {
      const primaryId = settings?.configDirectorySets?.[0]?.id;
      if (primaryId && setId === primaryId) {
        updateDirectory(app, value);
        return;
      }
      const key =
        app === "claude"
          ? "claudeConfigDir"
          : app === "codex"
            ? "codexConfigDir"
            : "geminiConfigDir";
      updateConfigDirectorySet(setId, { [key]: value } as Partial<
        ConfigDirectorySet
      >);
    },
    [settings?.configDirectorySets, updateConfigDirectorySet, updateDirectory],
  );

  const browseConfigDirectorySet = useCallback(
    async (setId: string, app: AppId) => {
      const primaryId = settings?.configDirectorySets?.[0]?.id;
      if (primaryId && setId === primaryId) {
        await browseDirectory(app);
        return;
      }

      const key =
        app === "claude"
          ? "claudeConfigDir"
          : app === "codex"
            ? "codexConfigDir"
            : "geminiConfigDir";
      const currentSets = settings?.configDirectorySets ?? [];
      const target = currentSets.find((set) => set.id === setId);
      const currentValue = target?.[key];

      try {
        const picked = await settingsApi.selectConfigDirectory(currentValue);
        const sanitized = sanitizeDir(picked ?? undefined);
        if (!sanitized) return;
        updateConfigDirectorySet(setId, { [key]: sanitized } as Partial<
          ConfigDirectorySet
        >);
      } catch (error) {
        console.error("[useDirectorySettings] Failed to pick directory", error);
        toast.error(
          t("settings.selectFileFailed", {
            defaultValue: "选择目录失败",
          }),
        );
      }
    },
    [
      browseDirectory,
      settings?.configDirectorySets,
      t,
      updateConfigDirectorySet,
    ],
  );

  const resetConfigDirectorySet = useCallback(
    async (setId: string, app: AppId) => {
      const primaryId = settings?.configDirectorySets?.[0]?.id;
      if (primaryId && setId === primaryId) {
        await resetDirectory(app);
        return;
      }
      const key =
        app === "claude"
          ? "claudeConfigDir"
          : app === "codex"
            ? "codexConfigDir"
            : "geminiConfigDir";
      updateConfigDirectorySet(setId, { [key]: undefined } as Partial<
        ConfigDirectorySet
      >);
    },
    [resetDirectory, settings?.configDirectorySets, updateConfigDirectorySet],
  );

  const configDirectorySets = settings?.configDirectorySets ?? [];

  return {
    appConfigDir,
    resolvedDirs,
    isLoading,
    initialAppConfigDir: initialAppConfigDirRef.current,
    configDirectorySets,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    addConfigDirectorySet,
    removeConfigDirectorySet,
    updateConfigDirectorySet,
    updateConfigDirectorySetDirectory,
    browseConfigDirectorySet,
    resetConfigDirectorySet,
    resetAllDirectories,
  };
}
