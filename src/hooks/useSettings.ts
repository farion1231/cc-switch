import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, settingsApi, type AppId } from "@/lib/api";
import { syncCurrentProvidersLiveSafe } from "@/utils/postChangeSync";
import { useSettingsQuery, useSaveSettingsMutation } from "@/lib/query";
import type { Settings } from "@/types";
import { useSettingsForm, type SettingsFormState } from "./useSettingsForm";
import {
  useDirectorySettings,
  type ResolvedDirectories,
} from "./useDirectorySettings";
import { useSettingsMetadata } from "./useSettingsMetadata";

type Language = "zh" | "en" | "ja";

interface SaveResult {
  requiresRestart: boolean;
}

export interface UseSettingsResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  isSaving: boolean;
  isPortable: boolean;
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  requiresRestart: boolean;
  updateSettings: (updates: Partial<SettingsFormState>) => void;
  updateDirectory: (app: AppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: AppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: AppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  saveSettings: (
    overrides?: Partial<SettingsFormState>,
    options?: { silent?: boolean },
  ) => Promise<SaveResult | null>;
  autoSaveSettings: (
    updates: Partial<SettingsFormState>,
  ) => Promise<SaveResult | null>;
  resetSettings: () => void;
  acknowledgeRestart: () => void;
}

export type { SettingsFormState, ResolvedDirectories };

const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

/**
 * useSettings - 组合层
 * 负责：
 * - 组合 useSettingsForm、useDirectorySettings、useSettingsMetadata
 * - 保存设置逻辑
 * - 重置设置逻辑
 */
export function useSettings(): UseSettingsResult {
  const { t } = useTranslation();
  const { data } = useSettingsQuery();
  const saveMutation = useSaveSettingsMutation();

  // 1️⃣ 表单状态管理
  const {
    settings,
    isLoading: isFormLoading,
    initialLanguage,
    updateSettings,
    resetSettings: resetForm,
    syncLanguage,
  } = useSettingsForm();

  // 2️⃣ 目录管理
  const {
    appConfigDir,
    resolvedDirs,
    isLoading: isDirectoryLoading,
    initialAppConfigDir,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    resetAllDirectories,
  } = useDirectorySettings({
    settings,
    onUpdateSettings: updateSettings,
  });

  // 3️⃣ 元数据管理
  const {
    isPortable,
    requiresRestart,
    isLoading: isMetadataLoading,
    acknowledgeRestart,
    setRequiresRestart,
  } = useSettingsMetadata();

  // 重置设置
  const resetSettings = useCallback(() => {
    resetForm(data ?? null);
    syncLanguage(initialLanguage);
    resetAllDirectories(
      sanitizeDir(data?.claudeConfigDir),
      sanitizeDir(data?.codexConfigDir),
      sanitizeDir(data?.geminiConfigDir),
    );
    setRequiresRestart(false);
  }, [
    data,
    initialLanguage,
    resetForm,
    syncLanguage,
    resetAllDirectories,
    setRequiresRestart,
  ]);

  // 即时保存设置（用于 General 标签页的实时更新）
  // 保存基础配置 + 独立的系统 API 调用（开机自启）
  const autoSaveSettings = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<SaveResult | null> => {
      const mergedSettings = settings ? { ...settings, ...updates } : null;
      if (!mergedSettings) return null;

      try {
        const sanitizedClaudeDir = sanitizeDir(mergedSettings.claudeConfigDir);
        const sanitizedCodexDir = sanitizeDir(mergedSettings.codexConfigDir);
        const sanitizedGeminiDir = sanitizeDir(mergedSettings.geminiConfigDir);

        const payload: Settings = {
          ...mergedSettings,
          claudeConfigDir: sanitizedClaudeDir,
          codexConfigDir: sanitizedCodexDir,
          geminiConfigDir: sanitizedGeminiDir,
          language: mergedSettings.language,
        };

        // 保存到配置文件
        await saveMutation.mutateAsync(payload);

        // 如果开机自启状态改变，调用系统 API
        if (
          payload.launchOnStartup !== undefined &&
          payload.launchOnStartup !== data?.launchOnStartup
        ) {
          try {
            await settingsApi.setAutoLaunch(payload.launchOnStartup);
          } catch (error) {
            console.error("Failed to update auto-launch:", error);
            toast.error(
              t("settings.autoLaunchFailed", {
                defaultValue: "设置开机自启失败",
              }),
            );
          }
        }

        // Claude Code 初次安装确认：开=写入 hasCompletedOnboarding=true；关=删除该字段
        // 仅在本次更新包含 skipClaudeOnboarding 时触发，避免其它自动保存误触发
        const nextSkipClaudeOnboarding = updates.skipClaudeOnboarding;
        if (
          nextSkipClaudeOnboarding !== undefined &&
          nextSkipClaudeOnboarding !== (data?.skipClaudeOnboarding ?? false)
        ) {
          try {
            if (nextSkipClaudeOnboarding) {
              await settingsApi.applyClaudeOnboardingSkip();
            } else {
              await settingsApi.clearClaudeOnboardingSkip();
            }
          } catch (error) {
            console.warn(
              "[useSettings] Failed to sync Claude onboarding skip",
              error,
            );
            toast.error(
              nextSkipClaudeOnboarding
                ? t("notifications.skipClaudeOnboardingFailed", {
                    defaultValue: "跳过 Claude Code 初次安装确认失败",
                  })
                : t("notifications.clearClaudeOnboardingSkipFailed", {
                    defaultValue: "恢复 Claude Code 初次安装确认失败",
                  }),
            );
          }
        }

        // 持久化语言偏好
        try {
          if (typeof window !== "undefined" && updates.language) {
            window.localStorage.setItem("language", updates.language);
          }
        } catch (error) {
          console.warn(
            "[useSettings] Failed to persist language preference",
            error,
          );
        }

        // 更新托盘菜单
        try {
          await providersApi.updateTrayMenu();
        } catch (error) {
          console.warn("[useSettings] Failed to refresh tray menu", error);
        }

        return { requiresRestart: false };
      } catch (error) {
        console.error("[useSettings] Failed to auto-save settings", error);
        toast.error(
          t("notifications.settingsSaveFailed", {
            defaultValue: "保存设置失败: {{error}}",
            error: (error as Error)?.message ?? String(error),
          }),
        );
        throw error;
      }
    },
    [data, saveMutation, settings, t],
  );

  // 完整保存设置（用于 Advanced 标签页的手动保存）
  // 包含所有系统 API 调用和完整的验证流程
  const saveSettings = useCallback(
    async (
      overrides?: Partial<SettingsFormState>,
      options?: { silent?: boolean },
    ): Promise<SaveResult | null> => {
      const mergedSettings = settings ? { ...settings, ...overrides } : null;
      if (!mergedSettings) return null;
      try {
        const sanitizedAppDir = sanitizeDir(appConfigDir);
        const sanitizedClaudeDir = sanitizeDir(mergedSettings.claudeConfigDir);
        const sanitizedCodexDir = sanitizeDir(mergedSettings.codexConfigDir);
        const sanitizedGeminiDir = sanitizeDir(mergedSettings.geminiConfigDir);
        const previousAppDir = initialAppConfigDir;
        const previousClaudeDir = sanitizeDir(data?.claudeConfigDir);
        const previousCodexDir = sanitizeDir(data?.codexConfigDir);
        const previousGeminiDir = sanitizeDir(data?.geminiConfigDir);

        const payload: Settings = {
          ...mergedSettings,
          claudeConfigDir: sanitizedClaudeDir,
          codexConfigDir: sanitizedCodexDir,
          geminiConfigDir: sanitizedGeminiDir,
          language: mergedSettings.language,
        };

        await saveMutation.mutateAsync(payload);

        await settingsApi.setAppConfigDirOverride(sanitizedAppDir ?? null);

        // 只在开机自启状态真正改变时调用系统 API
        if (
          payload.launchOnStartup !== undefined &&
          payload.launchOnStartup !== data?.launchOnStartup
        ) {
          try {
            await settingsApi.setAutoLaunch(payload.launchOnStartup);
          } catch (error) {
            console.error("Failed to update auto-launch:", error);
            toast.error(
              t("settings.autoLaunchFailed", {
                defaultValue: "设置开机自启失败",
              }),
            );
          }
        }

        // Claude Code 初次安装确认：开=写入 hasCompletedOnboarding=true；关=删除该字段
        const prevSkipClaudeOnboarding = data?.skipClaudeOnboarding ?? false;
        const nextSkipClaudeOnboarding = payload.skipClaudeOnboarding ?? false;
        if (nextSkipClaudeOnboarding !== prevSkipClaudeOnboarding) {
          try {
            if (nextSkipClaudeOnboarding) {
              await settingsApi.applyClaudeOnboardingSkip();
            } else {
              await settingsApi.clearClaudeOnboardingSkip();
            }
          } catch (error) {
            console.warn(
              "[useSettings] Failed to sync Claude onboarding skip",
              error,
            );
            toast.error(
              nextSkipClaudeOnboarding
                ? t("notifications.skipClaudeOnboardingFailed", {
                    defaultValue: "跳过 Claude Code 初次安装确认失败",
                  })
                : t("notifications.clearClaudeOnboardingSkipFailed", {
                    defaultValue: "恢复 Claude Code 初次安装确认失败",
                  }),
            );
          }
        }

        // 只在 Claude 插件集成状态真正改变时调用系统 API
        if (
          payload.enableClaudePluginIntegration !== undefined &&
          payload.enableClaudePluginIntegration !==
            data?.enableClaudePluginIntegration
        ) {
          try {
            if (payload.enableClaudePluginIntegration) {
              await settingsApi.applyClaudePluginConfig({ official: false });
            } else {
              await settingsApi.applyClaudePluginConfig({ official: true });
            }
          } catch (error) {
            console.warn(
              "[useSettings] Failed to sync Claude plugin config",
              error,
            );
            toast.error(
              t("notifications.syncClaudePluginFailed", {
                defaultValue: "同步 Claude 插件失败",
              }),
            );
          }
        }

        try {
          if (typeof window !== "undefined") {
            window.localStorage.setItem(
              "language",
              payload.language as Language,
            );
          }
        } catch (error) {
          console.warn(
            "[useSettings] Failed to persist language preference",
            error,
          );
        }

        try {
          await providersApi.updateTrayMenu();
        } catch (error) {
          console.warn("[useSettings] Failed to refresh tray menu", error);
        }

        // 如果 Claude/Codex/Gemini 的目录覆盖发生变化，则立即将“当前使用的供应商”写回对应应用的 live 配置
        const claudeDirChanged = sanitizedClaudeDir !== previousClaudeDir;
        const codexDirChanged = sanitizedCodexDir !== previousCodexDir;
        const geminiDirChanged = sanitizedGeminiDir !== previousGeminiDir;
        if (claudeDirChanged || codexDirChanged || geminiDirChanged) {
          const syncResult = await syncCurrentProvidersLiveSafe();
          if (!syncResult.ok) {
            console.warn(
              "[useSettings] Failed to sync current providers after directory change",
              syncResult.error,
            );
          }
        }

        const appDirChanged = sanitizedAppDir !== (previousAppDir ?? undefined);
        setRequiresRestart(appDirChanged);

        if (!options?.silent) {
          toast.success(
            t("notifications.settingsSaved", {
              defaultValue: "设置已保存",
            }),
            { closeButton: true },
          );
        }

        return { requiresRestart: appDirChanged };
      } catch (error) {
        console.error("[useSettings] Failed to save settings", error);
        toast.error(
          t("notifications.settingsSaveFailed", {
            defaultValue: "保存设置失败: {{error}}",
            error: (error as Error)?.message ?? String(error),
          }),
        );
        throw error;
      }
    },
    [
      appConfigDir,
      data,
      initialAppConfigDir,
      saveMutation,
      settings,
      setRequiresRestart,
      t,
    ],
  );

  const isLoading = useMemo(
    () => isFormLoading || isDirectoryLoading || isMetadataLoading,
    [isFormLoading, isDirectoryLoading, isMetadataLoading],
  );

  return {
    settings,
    isLoading,
    isSaving: saveMutation.isPending,
    isPortable,
    appConfigDir,
    resolvedDirs,
    requiresRestart,
    updateSettings,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    saveSettings,
    autoSaveSettings,
    resetSettings,
    acknowledgeRestart,
  };
}
