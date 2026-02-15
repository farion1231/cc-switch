import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { providersApi, settingsApi, openclawApi, type AppId } from "@/lib/api";
import { proxyApi } from "@/lib/api/proxy";
import type {
  Provider,
  UsageScript,
  OpenClawProviderConfig,
  OpenClawDefaultModel,
} from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import {
  useAddProviderMutation,
  useUpdateProviderMutation,
  useDeleteProviderMutation,
  useSwitchProviderMutation,
} from "@/lib/query";
import { extractErrorMessage } from "@/utils/errorUtils";
import { extractProviderBaseUrl } from "@/utils/providerBaseUrl";
import { openclawKeys } from "@/hooks/useOpenClaw";

interface UseProviderActionsOptions {
  /** 代理服务是否正在运行 */
  isProxyRunning?: boolean;
  /** 当前应用的代理接管是否激活 */
  isTakeoverActive?: boolean;
}

/**
 * Hook for managing provider actions (add, update, delete, switch)
 * Extracts business logic from App.tsx
 */
export function useProviderActions(
  activeApp: AppId,
  options: UseProviderActionsOptions = {},
) {
  const { isProxyRunning = false, isTakeoverActive = false } = options;
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const addProviderMutation = useAddProviderMutation(activeApp);
  const updateProviderMutation = useUpdateProviderMutation(activeApp);
  const deleteProviderMutation = useDeleteProviderMutation(activeApp);
  const switchProviderMutation = useSwitchProviderMutation(activeApp);

  // Claude 插件同步逻辑
  const syncClaudePlugin = useCallback(
    async (provider: Provider) => {
      if (activeApp !== "claude") return;

      try {
        const settings = await settingsApi.get();
        if (!settings?.enableClaudePluginIntegration) {
          return;
        }

        const isOfficial = provider.category === "official";
        await settingsApi.applyClaudePluginConfig({ official: isOfficial });

        // 静默执行，不显示成功通知
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.syncClaudePluginFailed", {
            defaultValue: "同步 Claude 插件失败",
          });
        toast.error(detail, { duration: 4200 });
      }
    },
    [activeApp, t],
  );

  // 添加供应商
  const addProvider = useCallback(
    async (
      provider: Omit<Provider, "id"> & {
        providerKey?: string;
        suggestedDefaults?: OpenClawSuggestedDefaults;
      },
    ) => {
      await addProviderMutation.mutateAsync(provider);

      // OpenClaw: register models to allowlist after adding provider
      if (activeApp === "openclaw" && provider.suggestedDefaults) {
        const { model, modelCatalog } = provider.suggestedDefaults;
        let modelsRegistered = false;

        try {
          // 1. Merge model catalog (allowlist)
          if (modelCatalog && Object.keys(modelCatalog).length > 0) {
            const existingCatalog = (await openclawApi.getModelCatalog()) || {};
            const mergedCatalog = { ...existingCatalog, ...modelCatalog };
            await openclawApi.setModelCatalog(mergedCatalog);
            modelsRegistered = true;
          }

          // 2. Set default model (only if not already set)
          if (model) {
            const existingDefault = await openclawApi.getDefaultModel();
            if (!existingDefault?.primary) {
              await openclawApi.setDefaultModel(model);
            }
          }

          // Show success toast if models were registered
          if (modelsRegistered) {
            toast.success(
              t("notifications.openclawModelsRegistered", {
                defaultValue: "模型已注册到 /model 列表",
              }),
              { closeButton: true },
            );
          }
        } catch (error) {
          // Log warning but don't block main flow - provider config is already saved
          console.warn(
            "[OpenClaw] Failed to register models to allowlist:",
            error,
          );
        }
      }
    },
    [addProviderMutation, activeApp, t],
  );

  // 更新供应商
  const updateProvider = useCallback(
    async (provider: Provider) => {
      await updateProviderMutation.mutateAsync(provider);

      // 更新托盘菜单（失败不影响主操作）
      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after updating provider",
          trayError,
        );
      }
    },
    [updateProviderMutation],
  );

  // 切换供应商
  const switchProvider = useCallback(
    async (provider: Provider) => {
      // 官方供应商不需要检查
      if (provider.category === "official") {
        try {
          await switchProviderMutation.mutateAsync(provider.id);
          await syncClaudePlugin(provider);
          const isMultiProviderApp =
            activeApp === "opencode" || activeApp === "openclaw";
          const messageKey = isMultiProviderApp
            ? "notifications.addToConfigSuccess"
            : "notifications.switchSuccess";
          const defaultMessage = isMultiProviderApp
            ? "已添加到配置"
            : "切换成功！";
          toast.success(
            t(messageKey, { defaultValue: defaultMessage }),
            {
              closeButton: true,
            },
          );
        } catch {
          // 错误提示由 mutation 处理
        }
        return;
      }

      // 提取 base URL 和 API 格式
      const baseUrl = extractProviderBaseUrl(provider, activeApp);
      const apiFormat = provider.meta?.apiFormat;

      // 调用后端 API 检查是否需要代理（统一由后端控制）
      let proxyRequirement: string | null = null;
      let proxyRequirementCheckFailed = false;
      if (baseUrl || apiFormat) {
        try {
          proxyRequirement = await proxyApi.checkProxyRequirement(
            activeApp,
            baseUrl || "",
            apiFormat,
          );
        } catch (error) {
          console.error("Failed to check proxy requirement:", error);
          proxyRequirementCheckFailed = true;
        }
      }

      // 如果需要代理但代理未激活，阻止切换并提示
      if (proxyRequirement && !(isProxyRunning && isTakeoverActive)) {
        let message: string;

        if (proxyRequirement === "openai_chat_format") {
          message = t("notifications.openAIChatFormatRequiresProxy", {
            defaultValue:
              "此供应商使用 OpenAI Chat 格式，需要开启代理服务进行格式转换才能正常使用。请先开启代理并接管当前应用。",
          });
        } else if (proxyRequirement === "full_url") {
          // 用户填了全链接（如 /v1/messages 结尾）
          message = t("notifications.fullUrlRequiresProxy", {
            defaultValue:
              "此供应商配置了完整 API 路径，直连模式下客户端可能会重复追加路径。请先开启代理并接管当前应用。",
          });
        } else {
          // url_mismatch: 直连地址和代理地址不匹配
          message = t("notifications.urlMismatchRequiresProxy", {
            defaultValue:
              "此供应商的请求地址配置与 API 格式不匹配，直连模式下无法正常工作。请先开启代理并接管当前应用。",
          });
        }

        toast.warning(message, {
          duration: 6000,
          closeButton: true,
        });
        return; // 阻止切换
      }

      try {
        await switchProviderMutation.mutateAsync(provider.id);
        await syncClaudePlugin(provider);

        const isMultiProviderApp =
          activeApp === "opencode" || activeApp === "openclaw";
        const messageKey = isMultiProviderApp
          ? "notifications.addToConfigSuccess"
          : "notifications.switchSuccess";
        const defaultMessage = isMultiProviderApp ? "已添加到配置" : "切换成功！";

        if (proxyRequirementCheckFailed && baseUrl) {
          toast.success(
            t("notifications.switchAppliedUnverified", {
              defaultValue: "切换已应用（未验证直连兼容性）",
            }),
            {
              description: t("notifications.switchAppliedUnverifiedDesc", {
                defaultValue:
                  "未能验证该端点是否需要代理。如果切换后无法正常使用，请开启代理并接管当前应用。",
              }),
              closeButton: true,
              duration: 6000,
            },
          );
        } else {
          toast.success(t(messageKey, { defaultValue: defaultMessage }), {
            closeButton: true,
          });
        }
      } catch {
        // 错误提示由 mutation 处理
      }
    },
    [
      switchProviderMutation,
      syncClaudePlugin,
      activeApp,
      t,
      isProxyRunning,
      isTakeoverActive,
    ],
  );

  // 删除供应商
  const deleteProvider = useCallback(
    async (id: string) => {
      await deleteProviderMutation.mutateAsync(id);
    },
    [deleteProviderMutation],
  );

  // 保存用量脚本
  const saveUsageScript = useCallback(
    async (provider: Provider, script: UsageScript) => {
      try {
        const updatedProvider: Provider = {
          ...provider,
          meta: {
            ...provider.meta,
            usage_script: script,
          },
        };

        await providersApi.update(updatedProvider, activeApp);
        await queryClient.invalidateQueries({
          queryKey: ["providers", activeApp],
        });
        // 🔧 保存用量脚本后，也应该失效该 provider 的用量查询缓存
        // 这样主页列表会使用新配置重新查询，而不是使用测试时的缓存
        await queryClient.invalidateQueries({
          queryKey: ["usage", provider.id, activeApp],
        });
        toast.success(
          t("provider.usageSaved", {
            defaultValue: "用量查询配置已保存",
          }),
          { closeButton: true },
        );
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("provider.usageSaveFailed", {
            defaultValue: "用量查询配置保存失败",
          });
        toast.error(detail);
      }
    },
    [activeApp, queryClient, t],
  );

  // Set provider as default model (OpenClaw only)
  const setAsDefaultModel = useCallback(
    async (provider: Provider) => {
      const config = provider.settingsConfig as OpenClawProviderConfig;
      if (!config.models || config.models.length === 0) {
        toast.error(
          t("notifications.openclawNoModels", {
            defaultValue: "该供应商没有配置模型",
          }),
        );
        return;
      }

      const model: OpenClawDefaultModel = {
        primary: `${provider.id}/${config.models[0].id}`,
        fallbacks: config.models.slice(1).map((m) => `${provider.id}/${m.id}`),
      };

      try {
        await openclawApi.setDefaultModel(model);
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.defaultModel,
        });
        toast.success(
          t("notifications.openclawDefaultModelSet", {
            defaultValue: "已设为默认模型",
          }),
          { closeButton: true },
        );
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.openclawDefaultModelSetFailed", {
            defaultValue: "设置默认模型失败",
          });
        toast.error(detail);
      }
    },
    [queryClient, t],
  );

  return {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
    isLoading:
      addProviderMutation.isPending ||
      updateProviderMutation.isPending ||
      deleteProviderMutation.isPending ||
      switchProviderMutation.isPending,
  };
}
