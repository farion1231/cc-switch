import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api/settings";
import type { Settings } from "@/types";

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
  // 用户手动操作过 keepConversationHistory 后，异步初始化读取不再覆盖
  const userTouchedTranscriptRef = useRef(false);

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

    const normalizedLanguage = normalizeLanguage(
      data.language ?? readPersistedLanguage(),
    );

    const normalized: SettingsFormState = {
      ...data,
      showInTray: data.showInTray ?? true,
      minimizeToTrayOnClose: data.minimizeToTrayOnClose ?? true,
      useAppWindowControls: data.useAppWindowControls ?? false,
      enableClaudePluginIntegration:
        data.enableClaudePluginIntegration ?? false,
      silentStartup: data.silentStartup ?? false,
      skipClaudeOnboarding: data.skipClaudeOnboarding ?? false,
      keepConversationHistory: data.keepConversationHistory ?? false,
      claudeConfigDir: sanitizeDir(data.claudeConfigDir),
      codexConfigDir: sanitizeDir(data.codexConfigDir),
      geminiConfigDir: sanitizeDir(data.geminiConfigDir),
      opencodeConfigDir: sanitizeDir(data.opencodeConfigDir),
      openclawConfigDir: sanitizeDir(data.openclawConfigDir),
      language: normalizedLanguage,
    };

    setSettingsState(normalized);
    initialLanguageRef.current = normalizedLanguage;
    syncLanguage(normalizedLanguage);

    // 从 ~/.claude/settings.json 读取实际的 transcript protection 状态并同步到表单
    // 仅在用户未手动操作过 toggle 时才覆盖，避免异步结果回滚用户选择
    settingsApi.getTranscriptProtection().then((isProtected) => {
      if (userTouchedTranscriptRef.current) return;
      setSettingsState((prev) => {
        if (!prev || prev.keepConversationHistory === isProtected) return prev;
        return { ...prev, keepConversationHistory: isProtected };
      });
    }).catch((err) => {
      console.warn("[useSettingsForm] Failed to read transcript protection state", err);
    });
  }, [data, readPersistedLanguage, syncLanguage]);

  const updateSettings = useCallback(
    (updates: Partial<SettingsFormState>) => {
      if (updates.keepConversationHistory !== undefined) {
        userTouchedTranscriptRef.current = true;
      }
      setSettingsState((prev) => {
        const base =
          prev ??
          ({
            showInTray: true,
            minimizeToTrayOnClose: true,
            useAppWindowControls: false,
            enableClaudePluginIntegration: false,
            skipClaudeOnboarding: false,
            keepConversationHistory: false,
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

      const normalizedLanguage = normalizeLanguage(
        serverData.language ?? readPersistedLanguage(),
      );

      const normalized: SettingsFormState = {
        ...serverData,
        showInTray: serverData.showInTray ?? true,
        minimizeToTrayOnClose: serverData.minimizeToTrayOnClose ?? true,
        useAppWindowControls: serverData.useAppWindowControls ?? false,
        enableClaudePluginIntegration:
          serverData.enableClaudePluginIntegration ?? false,
        silentStartup: serverData.silentStartup ?? false,
        skipClaudeOnboarding: serverData.skipClaudeOnboarding ?? false,
        keepConversationHistory: serverData.keepConversationHistory ?? false,
        claudeConfigDir: sanitizeDir(serverData.claudeConfigDir),
        codexConfigDir: sanitizeDir(serverData.codexConfigDir),
        geminiConfigDir: sanitizeDir(serverData.geminiConfigDir),
        opencodeConfigDir: sanitizeDir(serverData.opencodeConfigDir),
        openclawConfigDir: sanitizeDir(serverData.openclawConfigDir),
        language: normalizedLanguage,
      };

      setSettingsState(normalized);
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
