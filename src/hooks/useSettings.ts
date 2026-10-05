import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { useQueryClient } from "@tanstack/react-query";
import { providersApi, settingsApi } from "@/lib/api";
import { syncCurrentProvidersLiveSafe } from "@/utils/postChangeSync";
import {
  invalidatePiDirectoryCaches,
  useSettingsQuery,
  useSaveSettingsMutation,
} from "@/lib/query";
import type { Settings } from "@/types";
import {
  SETTINGS_DIRECTORY_FIELDS,
  useSettingsForm,
  type SettingsFormState,
} from "./useSettingsForm";
import {
  useDirectorySettings,
  type DirectoryAppId,
  type ResolvedDirectories,
} from "./useDirectorySettings";
import { useSettingsMetadata } from "./useSettingsMetadata";

interface SaveResult {
  requiresRestart: boolean;
}

export interface UseSettingsResult {
  settings: SettingsFormState | null;
  isLoading: boolean;
  isSaving: boolean;
  isPortable: boolean;
  appConfigDir?: string;
  /** 已保存的 CC Switch 数据目录（没覆盖时为空），用来判断有没有改动没保存 */
  initialAppConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  requiresRestart: boolean;
  updateSettings: (
    updates: Partial<SettingsFormState>,
    options?: { preservePending?: boolean },
  ) => void;
  updateDirectory: (app: DirectoryAppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: DirectoryAppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: DirectoryAppId) => Promise<void>;
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
  const queryClient = useQueryClient();
  const settingsSaveQueue = useRef<Promise<void>>(Promise.resolve());

  // 1️⃣ 表单状态管理
  const {
    settings,
    isLoading: isFormLoading,
    initialLanguage,
    updateSettings,
    resetSettings: resetForm,
    syncLanguage,
    trackPendingSettings,
  } = useSettingsForm();

  const persistSettings = useCallback(
    (
      patch: Partial<SettingsFormState>,
      onSaved?: (previous: Settings) => Promise<void>,
    ) => {
      const finishPending = trackPendingSettings(patch);
      const pending = settingsSaveQueue.current
        .catch(() => undefined)
        .then(async () => {
          // Queue edit intentions, not form snapshots. The cache contains only
          // acknowledged settings, including earlier explicit directory Saves.
          const previous =
            queryClient.getQueryData<Settings>(["settings"]) ?? data;
          if (!previous) throw new Error("Settings are not loaded");
          const { webdavSync: _webdav, s3Sync: _s3, ...saved } = previous;
          const payload: Settings = { ...saved, ...patch };
          await saveMutation.mutateAsync(payload);
          await onSaved?.(previous);
          return { payload, previous };
        })
        .finally(finishPending);
      settingsSaveQueue.current = pending.then(
        () => undefined,
        () => undefined,
      );
      return pending;
    },
    [data, queryClient, saveMutation, trackPendingSettings],
  );

  // 2️⃣ 目录管理
  const {
    appConfigDir,
    resolvedDirs,
    isLoading: isDirectoryLoading,
    initialAppConfigDir,
    commitAppConfigDir,
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
    resetAllDirectories({
      claude: sanitizeDir(data?.claudeConfigDir),
      codex: sanitizeDir(data?.codexConfigDir),
      gemini: sanitizeDir(data?.geminiConfigDir),
      grokbuild: sanitizeDir(data?.grokConfigDir),
      opencode: sanitizeDir(data?.opencodeConfigDir),
      openclaw: sanitizeDir(data?.openclawConfigDir),
      hermes: sanitizeDir(data?.hermesConfigDir),
      pi: sanitizeDir(data?.piConfigDir),
    });
    setRequiresRestart(false);
  }, [
    data,
    initialLanguage,
    resetForm,
    syncLanguage,
    resetAllDirectories,
    setRequiresRestart,
  ]);

  // 同步 Claude 插件集成配置到 ~/.claude/settings.json
  // 返回 true 表示已执行过 syncCurrentProvidersLiveSafe，调用方可跳过重复同步
  // prevEnabled 来自该次写入执行时的已确认缓存，避免 closure 中 data 滞后。
  const syncClaudePluginIfChanged = useCallback(
    async (
      enabled: boolean | undefined,
      prevEnabled: boolean | undefined,
    ): Promise<boolean> => {
      if (enabled === undefined || enabled === (prevEnabled ?? false)) {
        return false;
      }
      try {
        if (enabled) {
          const currentId = await providersApi.getCurrent("claude");
          let isOfficial = false;
          if (currentId) {
            const allProviders = await providersApi.getAll("claude");
            isOfficial = allProviders[currentId]?.category === "official";
          }
          await settingsApi.applyClaudePluginConfig({ official: isOfficial });
        } else {
          await settingsApi.applyClaudePluginConfig({ official: true });
        }

        const syncResult = await syncCurrentProvidersLiveSafe();
        if (!syncResult.ok) {
          console.warn(
            "[useSettings] Failed to sync providers after toggling Claude plugin",
            syncResult.error,
          );
          toast.error(
            t("notifications.syncClaudePluginFailed", {
              defaultValue: "同步 Claude 插件失败",
            }),
          );
        }
        return true;
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
        return false;
      }
    },
    [t],
  );

  // 即时保存设置（用于 General 标签页的实时更新）
  // 保存基础配置 + 独立的系统 API 调用（开机自启）
  const autoSaveSettings = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<SaveResult | null> => {
      if (!settings) return null;
      // Immediate preferences cannot commit directory or cloud-sync drafts.
      const { webdavSync: _webdav, s3Sync: _s3, ...patch } = updates;
      for (const field of SETTINGS_DIRECTORY_FIELDS) delete patch[field];

      try {
        await persistSettings(patch, async (previous) => {
          // 如果开机自启状态改变，调用系统 API
          if (
            patch.launchOnStartup !== undefined &&
            patch.launchOnStartup !== previous.launchOnStartup
          ) {
            try {
              await settingsApi.setAutoLaunch(patch.launchOnStartup);
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
            nextSkipClaudeOnboarding !==
              (previous.skipClaudeOnboarding ?? false)
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

          if (patch.enableClaudePluginIntegration !== undefined) {
            await syncClaudePluginIfChanged(
              patch.enableClaudePluginIntegration,
              previous.enableClaudePluginIntegration,
            );
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
        });

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
    [persistSettings, settings, syncClaudePluginIfChanged, t],
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
        const sanitizedGrokDir = sanitizeDir(mergedSettings.grokConfigDir);
        const sanitizedOpencodeDir = sanitizeDir(
          mergedSettings.opencodeConfigDir,
        );
        const sanitizedOpenclawDir = sanitizeDir(
          mergedSettings.openclawConfigDir,
        );
        const sanitizedPiDir = sanitizeDir(mergedSettings.piConfigDir);
        const previousAppDir = initialAppConfigDir;
        const previousClaudeDir = sanitizeDir(data?.claudeConfigDir);
        const previousCodexDir = sanitizeDir(data?.codexConfigDir);
        const previousGeminiDir = sanitizeDir(data?.geminiConfigDir);
        const previousGrokDir = sanitizeDir(data?.grokConfigDir);
        const previousOpencodeDir = sanitizeDir(data?.opencodeConfigDir);
        const previousOpenclawDir = sanitizeDir(data?.openclawConfigDir);
        const previousPiDir = sanitizeDir(data?.piConfigDir);
        const {
          webdavSync: _ignoredWebdavSync,
          s3Sync: _ignoredS3Sync,
          ...restSettings
        } = mergedSettings;

        const payload: Settings = {
          ...restSettings,
          claudeConfigDir: sanitizedClaudeDir,
          codexConfigDir: sanitizedCodexDir,
          geminiConfigDir: sanitizedGeminiDir,
          grokConfigDir: sanitizedGrokDir,
          opencodeConfigDir: sanitizedOpencodeDir,
          openclawConfigDir: sanitizedOpenclawDir,
          piConfigDir: sanitizedPiDir,
          language: mergedSettings.language,
        };

        const saved = queryClient.getQueryData<Settings>(["settings"]) ?? data;
        const patch: Partial<SettingsFormState> = Object.fromEntries(
          Object.entries(payload).filter(
            ([key, value]) =>
              SETTINGS_DIRECTORY_FIELDS.some((field) => field === key) ||
              !Object.is(value, saved?.[key as keyof Settings]),
          ),
        );
        const { payload: acknowledged, previous } =
          await persistSettings(patch);

        await settingsApi.setAppConfigDirOverride(sanitizedAppDir ?? null);
        // 基准值换成刚存的：设置页不卸载，下次比较和「需要重启」都只跟真正没保存的改动走
        commitAppConfigDir(sanitizedAppDir);

        // 只在开机自启状态真正改变时调用系统 API
        if (
          acknowledged.launchOnStartup !== undefined &&
          acknowledged.launchOnStartup !== previous.launchOnStartup
        ) {
          try {
            await settingsApi.setAutoLaunch(acknowledged.launchOnStartup);
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
        const prevSkipClaudeOnboarding = previous.skipClaudeOnboarding ?? false;
        const nextSkipClaudeOnboarding =
          acknowledged.skipClaudeOnboarding ?? false;
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

        const pluginSynced = await syncClaudePluginIfChanged(
          acknowledged.enableClaudePluginIntegration,
          previous.enableClaudePluginIntegration,
        );

        try {
          if (typeof window !== "undefined" && acknowledged.language) {
            window.localStorage.setItem("language", acknowledged.language);
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

        // 任一 app 的目录覆盖发生变化后，立即把当前状态投影到新的 live 目录。
        // 如果插件同步已经执行过 syncCurrentProvidersLiveSafe，则跳过避免重复
        const claudeDirChanged = sanitizedClaudeDir !== previousClaudeDir;
        const codexDirChanged = sanitizedCodexDir !== previousCodexDir;
        const geminiDirChanged = sanitizedGeminiDir !== previousGeminiDir;
        const grokDirChanged = sanitizedGrokDir !== previousGrokDir;
        const opencodeDirChanged = sanitizedOpencodeDir !== previousOpencodeDir;
        const openclawDirChanged = sanitizedOpenclawDir !== previousOpenclawDir;
        const piDirChanged = sanitizedPiDir !== previousPiDir;
        if (
          !pluginSynced &&
          (claudeDirChanged ||
            codexDirChanged ||
            geminiDirChanged ||
            grokDirChanged ||
            opencodeDirChanged ||
            openclawDirChanged)
        ) {
          const syncResult = await syncCurrentProvidersLiveSafe();
          if (!syncResult.ok) {
            console.warn(
              "[useSettings] Failed to sync current providers after directory change",
              syncResult.error,
            );
          }
        }
        if (piDirChanged) {
          await invalidatePiDirectoryCaches(queryClient);
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
      commitAppConfigDir,
      data,
      initialAppConfigDir,
      persistSettings,
      queryClient,
      settings,
      setRequiresRestart,
      syncClaudePluginIfChanged,
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
    initialAppConfigDir,
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
