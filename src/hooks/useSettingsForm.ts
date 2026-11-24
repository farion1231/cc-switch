import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsQuery } from "@/lib/query";
import {
  DEFAULT_LANGUAGE,
  isLanguage,
  isTraditionalChineseTag,
  normalizeLanguageTag,
  type Language,
} from "@/i18n/languages";
import type { Settings } from "@/types";
import { sanitizeDirectoryFields } from "@/config/directoryApps";

export type SettingsFormState = Omit<Settings, "language"> & {
  language: Language;
};

const normalizeLanguage = (lang?: string | null): Language => {
  if (!lang) return DEFAULT_LANGUAGE;
  const normalized = normalizeLanguageTag(lang);
  if (isTraditionalChineseTag(normalized)) return "zh-TW";
  return isLanguage(normalized) ? normalized : DEFAULT_LANGUAGE;
};

const isSupportedLanguage = (lang?: string | null): boolean => {
  if (!lang) return false;
  const normalized = normalizeLanguageTag(lang);
  return isLanguage(normalized) || normalized.startsWith("zh-");
};

function normalizeSettings(
  data: Settings,
  readPersistedLanguage: () => Language,
): SettingsFormState {
  return {
    ...data,
    showInTray: data.showInTray ?? true,
    minimizeToTrayOnClose: data.minimizeToTrayOnClose ?? true,
    useAppWindowControls: data.useAppWindowControls ?? false,
    enableClaudePluginIntegration: data.enableClaudePluginIntegration ?? false,
    silentStartup: data.silentStartup ?? false,
    skipClaudeOnboarding: data.skipClaudeOnboarding ?? false,
    preserveCodexOfficialAuthOnSwitch:
      data.preserveCodexOfficialAuthOnSwitch ?? false,
    unifyCodexSessionHistory: data.unifyCodexSessionHistory ?? false,
    codexStackClassicTools: data.codexStackClassicTools ?? true,
    ...sanitizeDirectoryFields(data),
    language: normalizeLanguage(data.language ?? readPersistedLanguage()),
  };
}

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

  const readPersistedLanguage = useCallback((): Language => {
    if (typeof window !== "undefined") {
      const stored = window.localStorage.getItem("language");
      if (isSupportedLanguage(stored)) {
        return normalizeLanguage(stored);
      }
    }
    return normalizeLanguage(i18n.language);
  }, [i18n]);

  const savedSettings = useMemo(
    () => (data ? normalizeSettings(data, readPersistedLanguage) : null),
    [data, readPersistedLanguage],
  );
  const [settingsState, setSettingsState] = useState(savedSettings);
  const initialLanguageRef = useRef(
    savedSettings?.language ?? DEFAULT_LANGUAGE,
  );

  const syncLanguage = useCallback(
    (lang: Language) => {
      const current = normalizeLanguage(i18n.language);
      if (current !== lang) {
        void i18n.changeLanguage(lang);
      }
    },
    [i18n],
  );

  // 后续查询返回新设置时同步表单。
  useEffect(() => {
    if (!savedSettings) return;
    setSettingsState(savedSettings);
    initialLanguageRef.current = savedSettings.language;
    syncLanguage(savedSettings.language);
  }, [savedSettings, syncLanguage]);

  const updateSettings = useCallback(
    (updates: Partial<SettingsFormState>) => {
      setSettingsState((prev) => {
        const base =
          prev ??
          ({
            showInTray: true,
            minimizeToTrayOnClose: true,
            useAppWindowControls: false,
            enableClaudePluginIntegration: false,
            skipClaudeOnboarding: false,
            preserveCodexOfficialAuthOnSwitch: false,
            unifyCodexSessionHistory: false,
            codexStackClassicTools: true,
            language: readPersistedLanguage(),
          } as SettingsFormState);

        const next: SettingsFormState = {
          ...base,
          ...updates,
        };

        if (updates.language) {
          const normalized = normalizeLanguage(updates.language);
          next.language = normalized;
          syncLanguage(normalized);
        }

        return next;
      });
    },
    [readPersistedLanguage, syncLanguage],
  );

  const resetSettings = useCallback(
    (serverData: Settings | null) => {
      if (!serverData) return;

      setSettingsState(normalizeSettings(serverData, readPersistedLanguage));
      syncLanguage(initialLanguageRef.current);
    },
    [readPersistedLanguage, syncLanguage],
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
