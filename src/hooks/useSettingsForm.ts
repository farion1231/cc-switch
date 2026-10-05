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

export const SETTINGS_DIRECTORY_FIELDS = [
  "claudeConfigDir",
  "codexConfigDir",
  "geminiConfigDir",
  "grokConfigDir",
  "opencodeConfigDir",
  "openclawConfigDir",
  "hermesConfigDir",
  "piConfigDir",
] as const;

export interface UseSettingsFormResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  initialLanguage: Language;
  updateSettings: (
    updates: Partial<SettingsFormState>,
    options?: { preservePending?: boolean },
  ) => void;
  resetSettings: (serverData: Settings | null) => void;
  readPersistedLanguage: () => Language;
  syncLanguage: (lang: Language) => void;
  trackPendingSettings: (updates: Partial<SettingsFormState>) => () => void;
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
  const savedSettingsRef = useRef<SettingsFormState | null>(null);
  const pendingSettingsRef = useRef<Array<Partial<SettingsFormState>>>([]);
  const [pendingVersion, setPendingVersion] = useState(0);

  const trackPendingSettings = useCallback(
    (updates: Partial<SettingsFormState>) => {
      const pending = { ...updates };
      // Directory edits have their own draft contract, including explicit Save.
      for (const field of SETTINGS_DIRECTORY_FIELDS) delete pending[field];
      pendingSettingsRef.current.push(pending);
      return () => {
        pendingSettingsRef.current = pendingSettingsRef.current.filter(
          (entry) => entry !== pending,
        );
        // Failure can leave query data unchanged. Reconcile the visible form
        // and language after removing the failed intention as well.
        setPendingVersion((version) => version + 1);
      };
    },
    [],
  );

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
      preserveCodexOfficialAuthOnSwitch:
        data.preserveCodexOfficialAuthOnSwitch ?? false,
      unifyCodexSessionHistory: data.unifyCodexSessionHistory ?? false,
      claudeConfigDir: sanitizeDir(data.claudeConfigDir),
      codexConfigDir: sanitizeDir(data.codexConfigDir),
      geminiConfigDir: sanitizeDir(data.geminiConfigDir),
      grokConfigDir: sanitizeDir(data.grokConfigDir),
      opencodeConfigDir: sanitizeDir(data.opencodeConfigDir),
      openclawConfigDir: sanitizeDir(data.openclawConfigDir),
      piConfigDir: sanitizeDir(data.piConfigDir),
      language: normalizedLanguage,
    };

    const previousSaved = savedSettingsRef.current;
    savedSettingsRef.current = normalized;
    setSettingsState((previous) => {
      if (!previous || !previousSaved) return normalized;
      const next = { ...normalized };
      // Immediate-save switches refetch settings. Keep each unsaved directory
      // edit (including reset-to-default) instead of replacing it on refetch.
      for (const field of SETTINGS_DIRECTORY_FIELDS) {
        if (
          sanitizeDir(previous[field]) !== sanitizeDir(previousSaved[field])
        ) {
          next[field] = previous[field];
        }
      }
      // Acknowledging an earlier write must not reset newer queued intentions,
      // even when a switch was toggled back to its original saved value.
      Object.assign(next, ...pendingSettingsRef.current);
      return next;
    });
    initialLanguageRef.current = normalizedLanguage;
    const pendingLanguage = Object.assign({}, ...pendingSettingsRef.current)
      .language as Language | undefined;
    syncLanguage(pendingLanguage ?? normalizedLanguage);
  }, [data, pendingVersion, readPersistedLanguage, syncLanguage]);

  const updateSettings = useCallback(
    (
      updates: Partial<SettingsFormState>,
      options?: { preservePending?: boolean },
    ) => {
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
        // New input always wins. Only a rollback must yield to newer queued
        // intentions; its caller captured previousValues before that input.
        if (options?.preservePending) {
          Object.assign(next, ...pendingSettingsRef.current);
        }

        if (next.language) {
          const normalized = normalizeLanguage(next.language);
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
        piConfigDir: sanitizeDir(serverData.piConfigDir),
        language: normalizedLanguage,
      };

      savedSettingsRef.current = normalized;
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
    trackPendingSettings,
  };
}
