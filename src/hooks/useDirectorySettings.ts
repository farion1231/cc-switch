import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { homeDir, join } from "@tauri-apps/api/path";
import { settingsApi, type AppId } from "@/lib/api";
import type { SettingsFormState } from "./useSettingsForm";
import type { ConfigDirProfile } from "@/types";

type AppDirectoryKey =
  | "claude"
  | "codex"
  | "gemini"
  | "opencode"
  | "openclaw"
  | "hermes";
type DirectoryKey = "appConfig" | AppDirectoryKey;

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
  AppId,
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
  isLoading: boolean;
  initialAppConfigDir?: string;
  updateDirectory: (app: AppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: AppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: AppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  resetAllDirectories: (overrides?: ResolvedAppDirectoryOverrides) => void;
  // Profile management
  profiles: ConfigDirProfile[];
  activeProfileId: string | undefined;
  getActiveProfile: () => ConfigDirProfile | undefined;
  getActiveProfileDirectoryValues: () => {
    claude?: string;
    codex?: string;
    gemini?: string;
    opencode?: string;
    openclaw?: string;
    hermes?: string;
  } | null;
  createProfile: (name: string) => Promise<ConfigDirProfile>;
  updateProfile: (profile: ConfigDirProfile) => Promise<void>;
  deleteProfile: (id: string) => Promise<void>;
  switchProfile: (id: string) => Promise<void>;
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
  const [isLoading, setIsLoading] = useState(true);

  // Profile state
  const [profiles, setProfiles] = useState<ConfigDirProfile[]>([]);
  const [activeProfileId, setActiveProfileId] = useState<string | undefined>(
    settings?.activeConfigDirProfileId,
  );

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

  // Load profile data
  useEffect(() => {
    settingsApi
      .getConfigDirProfiles()
      .then((profiles) => {
        setProfiles(profiles);
      })
      .catch(() => {
        // Silently ignore if profiles API is not available yet
      });
  }, []);

  // Load active profile ID from settings
  useEffect(() => {
    if (settings?.activeConfigDirProfileId) {
      setActiveProfileId(settings.activeConfigDirProfileId);
    }
  }, [settings?.activeConfigDirProfileId]);

  const updateDirectoryState = useCallback(
    (key: DirectoryKey, value?: string) => {
      const sanitized = sanitizeDir(value);
      if (key === "appConfig") {
        setAppConfigDir(sanitized);
      } else {
        // 如果有 active profile，更新 profile 内部字段
        if (activeProfileId) {
          const activeProfile = profiles.find((p) => p.id === activeProfileId);
          if (activeProfile) {
            const profileKey =
              DIRECTORY_KEY_TO_SETTINGS_FIELD[key].replace(
                "ConfigDir",
                "",
              ) as keyof ConfigDirProfile;
            const updatedProfile: ConfigDirProfile = {
              ...activeProfile,
              [profileKey]: sanitized,
            };
            // 保存到后端
            settingsApi.upsertConfigDirProfile(updatedProfile).catch((err) => {
              console.error("[useDirectorySettings] Failed to update profile", err);
            });
            // 更新本地状态
            setProfiles((prev) =>
              prev.map((p) => (p.id === activeProfileId ? updatedProfile : p)),
            );
            // 同步到 settings
            onUpdateSettings({
              configDirProfiles: profiles.map((p) =>
                p.id === activeProfileId ? updatedProfile : p,
              ),
            });
          }
        } else {
          // 没有 active profile，更新顶层 settings 字段（向后兼容）
          onUpdateSettings({
            [DIRECTORY_KEY_TO_SETTINGS_FIELD[key]]: sanitized,
          });
        }
      }

      setResolvedDirs((prev) => {
        const next = sanitized ?? defaultsRef.current[key];
        // Same-ref early-return: unchanged value shouldn't cascade renders
        // through the settings tree.
        if (prev[key] === next) return prev;
        return { ...prev, [key]: next };
      });
    },
    [onUpdateSettings, activeProfileId, profiles],
  );

  const updateAppConfigDir = useCallback(
    (value?: string) => {
      updateDirectoryState("appConfig", value);
    },
    [updateDirectoryState],
  );

  const updateDirectory = useCallback(
    (app: AppId, value?: string) => {
      updateDirectoryState(APP_DIRECTORY_META[app].key, value);
    },
    [updateDirectoryState],
  );

  const browseDirectory = useCallback(
    async (app: AppId) => {
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

  const getActiveProfile = useCallback(() => {
    return profiles.find((p) => p.id === activeProfileId);
  }, [profiles, activeProfileId]);

  const getActiveProfileDirectoryValues = useCallback(() => {
    const activeProfile = profiles.find((p) => p.id === activeProfileId);
    if (!activeProfile) {
      return null;
    }
    return {
      claude: activeProfile.claude,
      codex: activeProfile.codex,
      gemini: activeProfile.gemini,
      opencode: activeProfile.opencode,
      openclaw: activeProfile.openclaw,
      hermes: activeProfile.hermes,
    };
  }, [activeProfileId, profiles]);

  const createProfile = useCallback(async (name: string) => {
    const id = `profile-${Date.now()}`;
    const profile: ConfigDirProfile = { id, name };
    await settingsApi.upsertConfigDirProfile(profile);
    await settingsApi.setActiveConfigDirProfile(id);
    setProfiles((prev) => [...prev, profile]);
    setActiveProfileId(id);
    // 同步到 settings，让父组件知道 active profile 变化了
    onUpdateSettings({
      configDirProfiles: [...profiles, profile],
      activeConfigDirProfileId: id,
    });
    return profile;
  }, [profiles, onUpdateSettings]);

  const updateProfile = useCallback(async (profile: ConfigDirProfile) => {
    await settingsApi.upsertConfigDirProfile(profile);
    const newProfiles = profiles.map((p) => (p.id === profile.id ? profile : p));
    setProfiles(newProfiles);
    // 同步到 settings
    onUpdateSettings({
      configDirProfiles: newProfiles,
    });
  }, [profiles, onUpdateSettings]);

  const deleteProfile = useCallback(
    async (id: string) => {
      await settingsApi.deleteConfigDirProfile(id);
      const newProfiles = profiles.filter((p) => p.id !== id);
      setProfiles(newProfiles);
      // 同步到 settings
      onUpdateSettings({
        configDirProfiles: newProfiles,
      });
      if (activeProfileId === id) {
        // 从后端重新获取实际的 activeProfileId（后端会自动选择第一个剩余 profile）
        const actualActiveProfile = await settingsApi.getActiveConfigDirProfile();
        const actualActiveId = actualActiveProfile?.id;
        setActiveProfileId(actualActiveId);
        onUpdateSettings({
          activeConfigDirProfileId: actualActiveId,
        });
      }
    },
    [activeProfileId, profiles, onUpdateSettings],
  );

  const switchProfile = useCallback(
    async (id: string) => {
      await settingsApi.setActiveConfigDirProfile(id);
      setActiveProfileId(id);
      // 同步到 settings
      onUpdateSettings({
        activeConfigDirProfileId: id,
      });
      // Reload resolved directories after switching
      const newActive = profiles.find((p) => p.id === id);
      if (newActive) {
        const newResolvedDirs: ResolvedDirectories = {
          ...defaultsRef.current,
          appConfig: appConfigDir || defaultsRef.current.appConfig,
        };
        for (const [appId, meta] of Object.entries(APP_DIRECTORY_META)) {
          const dirPath = (newActive as unknown as Record<
            string,
            string | undefined
          >)[meta.key];
          if (dirPath) {
            newResolvedDirs[appId as AppId] = dirPath;
          } else {
            const fallback =
              (await settingsApi.getConfigDir(appId as AppId)) ||
              defaultsRef.current[meta.key];
            newResolvedDirs[appId as AppId] = fallback;
          }
        }
        setResolvedDirs(newResolvedDirs);
      }
    },
    [profiles, appConfigDir, onUpdateSettings],
  );

  return {
    appConfigDir,
    resolvedDirs,
    isLoading,
    initialAppConfigDir: initialAppConfigDirRef.current,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    resetAllDirectories,
    // Profile management
    profiles,
    activeProfileId,
    getActiveProfile,
    getActiveProfileDirectoryValues,
    createProfile,
    updateProfile,
    deleteProfile,
    switchProfile,
  };
}
