import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsQuery } from "@/lib/query";
import type { ConfigDirectorySet, Settings } from "@/types";
import { generateConfigDirectorySetId } from "@/utils/id";

type Language = "zh" | "en" | "ja";

export type SettingsFormState = Omit<Settings, "language"> & {
  language: Language;
};

const normalizeLanguage = (lang?: string | null): Language => {
  if (!lang) return "zh";
  const normalized = lang.toLowerCase();
  return normalized === "en" || normalized === "ja" ? normalized : "zh";
};

const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

const clampSetName = (
  value: string | undefined | null,
  fallbackName: string,
  index: number,
): string => {
  const trimmed = value?.trim();
  const base =
    trimmed && trimmed.length > 0
      ? trimmed
      : index === 0
        ? fallbackName
        : `${fallbackName} ${index + 1}`;
  return base.length > 60 ? base.slice(0, 60) : base;
};

const normalizeConfigDirectorySets = (
  sets: ConfigDirectorySet[] | undefined,
  fallbackName: string,
): ConfigDirectorySet[] => {
  if (!sets || sets.length === 0) {
    return [
      {
        id: generateConfigDirectorySetId(),
        name: fallbackName,
      },
    ];
  }

  return sets.map((set, index) => ({
    ...set,
    id:
      typeof set.id === "string" && set.id.trim().length > 0
        ? set.id
        : generateConfigDirectorySetId(),
    name: clampSetName(set.name, fallbackName, index),
    claudeConfigDir: sanitizeDir(set.claudeConfigDir),
    codexConfigDir: sanitizeDir(set.codexConfigDir),
    geminiConfigDir: sanitizeDir(set.geminiConfigDir),
  }));
};

const syncPrimaryDirectorySet = (
  sets: ConfigDirectorySet[],
  directories: Pick<ConfigDirectorySet, "claudeConfigDir" | "codexConfigDir" | "geminiConfigDir">,
  fallbackName: string,
): ConfigDirectorySet[] => {
  if (!sets.length) {
    return [
      {
        id: generateConfigDirectorySetId(),
        name: fallbackName,
        ...directories,
      },
    ];
  }

  const [primary, ...rest] = sets;
  const syncedPrimary: ConfigDirectorySet = {
    ...primary,
    claudeConfigDir: directories.claudeConfigDir,
    codexConfigDir: directories.codexConfigDir,
    geminiConfigDir: directories.geminiConfigDir,
  };

  return [syncedPrimary, ...rest];
};

const applyConfigSetSync = (
  state: SettingsFormState,
  defaultSetName: string,
): SettingsFormState => {
  const directories = {
    claudeConfigDir: sanitizeDir(state.claudeConfigDir),
    codexConfigDir: sanitizeDir(state.codexConfigDir),
    geminiConfigDir: sanitizeDir(state.geminiConfigDir),
  };

  const normalizedSets = normalizeConfigDirectorySets(
    state.configDirectorySets,
    defaultSetName,
  );
  const syncedSets = syncPrimaryDirectorySet(
    normalizedSets,
    directories,
    defaultSetName,
  );
  const activeId =
    state.activeConfigDirectorySetId &&
    syncedSets.some((set) => set.id === state.activeConfigDirectorySetId)
      ? state.activeConfigDirectorySetId
      : syncedSets[0]?.id;

  return {
    ...state,
    claudeConfigDir: directories.claudeConfigDir,
    codexConfigDir: directories.codexConfigDir,
    geminiConfigDir: directories.geminiConfigDir,
    configDirectorySets: syncedSets,
    activeConfigDirectorySetId: activeId,
  };
};

const buildNormalizedSettingsState = (
  data: Settings,
  fallbackLanguage: Language,
  defaultSetName: string,
): SettingsFormState => {
  const normalizedLanguage = normalizeLanguage(data.language ?? fallbackLanguage);

  const base: SettingsFormState = {
    ...data,
    showInTray: data.showInTray ?? true,
    minimizeToTrayOnClose: data.minimizeToTrayOnClose ?? true,
    enableClaudePluginIntegration: data.enableClaudePluginIntegration ?? false,
    claudeConfigDir: sanitizeDir(data.claudeConfigDir),
    codexConfigDir: sanitizeDir(data.codexConfigDir),
    geminiConfigDir: sanitizeDir(data.geminiConfigDir),
    language: normalizedLanguage,
    configDirectorySets: data.configDirectorySets ?? [],
    activeConfigDirectorySetId: data.activeConfigDirectorySetId,
  };

  return applyConfigSetSync(base, defaultSetName);
};

export interface UseSettingsFormResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  initialLanguage: Language;
  updateSettings: (updates: Partial<SettingsFormState>) => void;
  resetSettings: (serverData: Settings | null) => void;
  readPersistedLanguage: () => Language;
  syncLanguage: (lang: Language) => void;
}

/**
 * useSettingsForm - 表单状态管理
 * 负责：
 * - 表单数据状态
 * - 表单字段更新
 * - 语言同步
 * - 表单重置
 */
export function useSettingsForm(): UseSettingsFormResult {
  const { i18n } = useTranslation();
  const { data, isLoading } = useSettingsQuery();

  const [settingsState, setSettingsState] = useState<SettingsFormState | null>(
    null,
  );

  const initialLanguageRef = useRef<Language>("zh");

  const getDefaultConfigSetName = useCallback(
    () => i18n.t("settings.configSetDefaultName", { defaultValue: "默认环境" }),
    [i18n],
  );

  const readPersistedLanguage = useCallback((): Language => {
    if (typeof window !== "undefined") {
      const stored = window.localStorage.getItem("language");
      if (stored === "en" || stored === "zh" || stored === "ja") {
        return stored as Language;
      }
    }
    return normalizeLanguage(i18n.language);
  }, [i18n]);

  const syncLanguage = useCallback(
    (lang: Language) => {
      const current = normalizeLanguage(i18n.language);
      if (current !== lang) {
        void i18n.changeLanguage(lang);
      }
    },
    [i18n],
  );

  // 初始化设置数据
  useEffect(() => {
    if (!data) return;

    const defaultSetName = getDefaultConfigSetName();
    const normalized = buildNormalizedSettingsState(
      data,
      data.language ?? readPersistedLanguage(),
      defaultSetName,
    );
    setSettingsState(normalized);
    initialLanguageRef.current = normalized.language;
    syncLanguage(normalized.language);
  }, [data, getDefaultConfigSetName, readPersistedLanguage, syncLanguage]);

  const updateSettings = useCallback(
    (updates: Partial<SettingsFormState>) => {
      setSettingsState((prev) => {
        const base =
          prev ??
          ({
            showInTray: true,
            minimizeToTrayOnClose: true,
            enableClaudePluginIntegration: false,
            skipClaudeOnboarding: true,
            language: readPersistedLanguage(),
            configDirectorySets: [],
          } as SettingsFormState);

        const merged: SettingsFormState = {
          ...base,
          ...updates,
        };

        if (updates.language) {
          const normalized = normalizeLanguage(updates.language);
          merged.language = normalized;
          syncLanguage(normalized);
        }

        return applyConfigSetSync(merged, getDefaultConfigSetName());
      });
    },
    [getDefaultConfigSetName, readPersistedLanguage, syncLanguage],
  );

  const resetSettings = useCallback(
    (serverData: Settings | null) => {
      if (!serverData) return;

      const defaultSetName = getDefaultConfigSetName();
      const normalized = buildNormalizedSettingsState(
        serverData,
        serverData.language ?? readPersistedLanguage(),
        defaultSetName,
      );
      setSettingsState(normalized);
      syncLanguage(initialLanguageRef.current);
    },
    [getDefaultConfigSetName, readPersistedLanguage, syncLanguage],
  );

  return {
    settings: settingsState,
    isLoading,
    initialLanguage: initialLanguageRef.current,
    updateSettings,
    resetSettings,
    readPersistedLanguage,
    syncLanguage,
  };
}
