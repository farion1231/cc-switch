import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import type { Provider } from "@/types";
import { providersApi, settingsApi, type AppId } from "@/lib/api";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";
import { extractErrorMessage } from "@/utils/errorUtils";
import { deepClone } from "@/utils/deepClone";

type NewProviderInput = Omit<Provider, "id" | "createdAt"> & {
  providerKey?: string;
  addToLive?: boolean;
};

interface UseProviderWorkflowParams {
  activeApp: AppId;
  providers: Record<string, Provider>;
  addProvider: (provider: NewProviderInput) => Promise<unknown>;
  updateProvider: (provider: Provider, originalId?: string) => Promise<unknown>;
  deleteProvider: (id: string) => Promise<unknown>;
  refetch: () => Promise<unknown>;
}

const generateUniqueProviderCopyKey = (
  originalKey: string,
  existingKeys: string[],
): string => {
  const baseKey = `${originalKey}-copy`;

  if (!existingKeys.includes(baseKey)) {
    return baseKey;
  }

  let counter = 2;
  while (existingKeys.includes(`${baseKey}-${counter}`)) {
    counter++;
  }
  return `${baseKey}-${counter}`;
};

/**
 * 供应商工作流：编辑 / 删除确认 / 复制 / 打开终端 / 打开官网 / 导入刷新。
 * 从 App.tsx 抽离的业务编排层。
 */
export function useProviderWorkflow({
  activeApp,
  providers,
  addProvider,
  updateProvider,
  deleteProvider,
  refetch,
}: UseProviderWorkflowParams) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    provider: Provider;
    action: "remove" | "delete";
  } | null>(null);

  const handleEditProvider = useCallback(
    async ({
      provider,
      originalId,
    }: {
      provider: Provider;
      originalId?: string;
    }) => {
      await updateProvider(provider, originalId);
      setEditingProvider(null);
    },
    [updateProvider],
  );

  const handleConfirmAction = useCallback(async () => {
    if (!confirmAction) return;
    const { provider, action } = confirmAction;

    if (action === "remove") {
      // 仅从 live 配置移除（OpenCode/OpenClaw/Hermes 累加模式），不删数据库
      await providersApi.removeFromLiveConfig(provider.id, activeApp);
      if (activeApp === "opencode") {
        await queryClient.invalidateQueries({
          queryKey: ["opencodeLiveProviderIds"],
        });
      } else if (activeApp === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.liveProviderIds,
        });
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
      } else if (activeApp === "hermes") {
        await queryClient.invalidateQueries({
          queryKey: hermesKeys.liveProviderIds,
        });
      }
      toast.success(
        t("notifications.removeFromConfigSuccess", {
          defaultValue: "已从配置移除",
        }),
        { closeButton: true },
      );
    } else {
      await deleteProvider(provider.id);
    }
    setConfirmAction(null);
  }, [confirmAction, activeApp, queryClient, deleteProvider, t]);

  const handleDuplicateProvider = useCallback(
    async (provider: Provider) => {
      const newSortIndex =
        provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined;

      const duplicatedProvider: NewProviderInput = {
        name: `${provider.name} copy`,
        settingsConfig: deepClone(provider.settingsConfig),
        websiteUrl: provider.websiteUrl,
        category: provider.category,
        sortIndex: newSortIndex,
        meta: provider.meta ? deepClone(provider.meta) : undefined,
        icon: provider.icon,
        iconColor: provider.iconColor,
      };

      if (
        activeApp === "opencode" ||
        activeApp === "openclaw" ||
        activeApp === "hermes"
      ) {
        let liveProviderIds: string[] = [];
        try {
          liveProviderIds =
            activeApp === "opencode"
              ? await queryClient.ensureQueryData({
                  queryKey: ["opencodeLiveProviderIds"],
                  queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
                })
              : activeApp === "openclaw"
                ? await queryClient.ensureQueryData({
                    queryKey: openclawKeys.liveProviderIds,
                    queryFn: () => providersApi.getOpenClawLiveProviderIds(),
                  })
                : await queryClient.ensureQueryData({
                    queryKey: hermesKeys.liveProviderIds,
                    queryFn: () => providersApi.getHermesLiveProviderIds(),
                  });
        } catch (error) {
          console.error(
            "[App] Failed to load live provider IDs for duplication",
            error,
          );
          const errorMessage = extractErrorMessage(error);
          toast.error(
            t("provider.duplicateLiveIdsLoadFailed", {
              defaultValue: "读取配置中的供应商标识失败，请先修复配置后再试",
            }) + (errorMessage ? `: ${errorMessage}` : ""),
          );
          return;
        }
        const existingKeys = Array.from(
          new Set([...Object.keys(providers), ...liveProviderIds]),
        );
        duplicatedProvider.providerKey = generateUniqueProviderCopyKey(
          provider.id,
          existingKeys,
        );
        duplicatedProvider.addToLive = false;
      }

      if (provider.sortIndex !== undefined) {
        const updates = Object.values(providers)
          .filter(
            (p) =>
              p.sortIndex !== undefined &&
              p.sortIndex >= newSortIndex! &&
              p.id !== provider.id,
          )
          .map((p) => ({
            id: p.id,
            sortIndex: p.sortIndex! + 1,
          }));

        if (updates.length > 0) {
          try {
            await providersApi.updateSortOrder(updates, activeApp);
          } catch (error) {
            console.error("[App] Failed to update sort order", error);
            toast.error(
              t("provider.sortUpdateFailed", {
                defaultValue: "排序更新失败",
              }),
            );
            return;
          }
        }
      }

      await addProvider(duplicatedProvider);
    },
    [activeApp, addProvider, providers, queryClient, t],
  );

  const handleOpenTerminal = useCallback(
    async (provider: Provider) => {
      try {
        const selectedDir = await settingsApi.pickDirectory();
        if (!selectedDir) {
          return;
        }

        await providersApi.openTerminal(provider.id, activeApp, {
          cwd: selectedDir,
        });
        toast.success(
          t("provider.terminalOpened", {
            defaultValue: "终端已打开",
          }),
        );
      } catch (error) {
        console.error("[App] Failed to open terminal", error);
        const errorMessage = extractErrorMessage(error);
        toast.error(
          t("provider.terminalOpenFailed", {
            defaultValue: "打开终端失败",
          }) + (errorMessage ? `: ${errorMessage}` : ""),
        );
      }
    },
    [activeApp, t],
  );

  const handleOpenWebsite = useCallback(
    async (url: string) => {
      try {
        await settingsApi.openExternal(url);
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.openLinkFailed", {
            defaultValue: "链接打开失败",
          });
        toast.error(detail);
      }
    },
    [t],
  );

  const handleImportSuccess = useCallback(async () => {
    try {
      await queryClient.invalidateQueries({
        queryKey: ["providers"],
        refetchType: "all",
      });
      await queryClient.refetchQueries({
        queryKey: ["providers"],
        type: "all",
      });
    } catch (error) {
      console.error("[App] Failed to refresh providers after import", error);
      await refetch();
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  }, [queryClient, refetch]);

  return {
    editingProvider,
    setEditingProvider,
    usageProvider,
    setUsageProvider,
    confirmAction,
    setConfirmAction,
    handleEditProvider,
    handleConfirmAction,
    handleDuplicateProvider,
    handleOpenTerminal,
    handleOpenWebsite,
    handleImportSuccess,
  };
}
