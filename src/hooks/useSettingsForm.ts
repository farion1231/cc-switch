import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsQuery } from "@/lib/query";
import type { Settings } from "@/types";

type Language = "zh" | "zh-TW" | "en" | "ja";

export type SettingsFormState = Omit<Settings, "language"> & {
  language: Language;
};

const normalizeLanguage = (lang?: string | null): Language => {
  if (!lang) return "zh";
  const normalized = lang.toLowerCase().replace(/_/g, "-");

  if (normalized === "zh") {
    return "zh";
  }

  if (
    normalized === "zh-tw" ||
    normalized.startsWith("zh-hant") ||
    normalized.startsWith("zh-hk") ||
    normalized.startsWith("zh-mo")
  ) {
    return "zh-TW";
  }

  if (normalized === "en" || normalized === "ja") {
    return normalized;
  }

  if (normalized.startsWith("zh")) {
    return "zh";
  }

  return "zh";
};

const isSupportedLanguage = (lang?: string | null): boolean => {
  if (!lang) return false;
  const normalized = lang.toLowerCase().replace(/_/g, "-");
  return (
    normalized === "en" || normalized === "ja" || normalized.startsWith("zh")
  );
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

  const readPersistedLanguage = useCallback((): Language => {
    if (typeof window !== "undefined") {
      const stored = window.localStorage.getItem("language");
      if (isSupportedLanguage(stored)) {
        return normalizeLanguage(stored);
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
    const source =
      data ??
      (import.meta.env.VITE_BROWSER_PREVIEW === "true"
        ? ({
            language: readPersistedLanguage(),
            showInTray: true,
            minimizeToTrayOnClose: true,
            visibleApps: {
              claude: true,
              "claude-desktop": true,
              codex: true,
              gemini: true,
              grokbuild: true,
              opencode: true,
              openclaw: true,
              hermes: true,
              qodercli: true,
            },
            qodercliConfigDir: ".qoder",
          } as Settings)
        : null);
    if (!source) return;

    const normalizedLanguage = normalizeLanguage(
      source.language ?? readPersistedLanguage(),
    );

    const normalized: SettingsFormState = {
      ...source,
      showInTray: source.showInTray ?? true,
      minimizeToTrayOnClose: source.minimizeToTrayOnClose ?? true,
      useAppWindowControls: source.useAppWindowControls ?? false,
      enableClaudePluginIntegration:
        source.enableClaudePluginIntegration ?? false,
      silentStartup: source.silentStartup ?? false,
      skipClaudeOnboarding: source.skipClaudeOnboarding ?? false,
      preserveCodexOfficialAuthOnSwitch:
        source.preserveCodexOfficialAuthOnSwitch ?? false,
      unifyCodexSessionHistory: source.unifyCodexSessionHistory ?? false,
      claudeConfigDir: sanitizeDir(source.claudeConfigDir),
      codexConfigDir: sanitizeDir(source.codexConfigDir),
      geminiConfigDir: sanitizeDir(source.geminiConfigDir),
      grokConfigDir: sanitizeDir(source.grokConfigDir),
      opencodeConfigDir: sanitizeDir(source.opencodeConfigDir),
      openclawConfigDir: sanitizeDir(source.openclawConfigDir),
      language: normalizedLanguage,
    };

    setSettingsState(normalized);
    initialLanguageRef.current = normalizedLanguage;
    syncLanguage(normalizedLanguage);
  }, [data, isLoading, readPersistedLanguage, syncLanguage]);

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
        preserveCodexOfficialAuthOnSwitch:
          serverData.preserveCodexOfficialAuthOnSwitch ?? false,
        unifyCodexSessionHistory: serverData.unifyCodexSessionHistory ?? false,
        claudeConfigDir: sanitizeDir(serverData.claudeConfigDir),
        codexConfigDir: sanitizeDir(serverData.codexConfigDir),
        geminiConfigDir: sanitizeDir(serverData.geminiConfigDir),
        grokConfigDir: sanitizeDir(serverData.grokConfigDir),
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
