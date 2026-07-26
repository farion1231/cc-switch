import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { ManagedAppId, ProviderSwitchEvent } from "@/lib/api";
import { providersApi, settingsApi } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query";
import { useProviderActions } from "@/hooks/useProviderActions";
import { useLastValidValue } from "@/hooks/useLastValidValue";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";
import {
  useDisableCurrentOmo,
  useDisableCurrentOmoSlim,
} from "@/lib/query/omo";
import { deepClone } from "@/utils/deepClone";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { ProviderCatalogHandle } from "./ProviderCatalogHandle";
import { ProviderList } from "./ProviderList";
import { AddProviderDialog } from "./AddProviderDialog";
import { EditProviderDialog } from "./EditProviderDialog";
import UsageScriptModal from "@/components/UsageScriptModal";
import { ConfirmDialog } from "@/components/ConfirmDialog";

interface ManagedProviderWorkspaceProps {
  appId: ManagedAppId;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  activeProviderId?: string;
}

const generateUniqueProviderCopyKey = (
  originalKey: string,
  existingKeys: string[],
): string => {
  const baseKey = `${originalKey}-copy`;
  if (!existingKeys.includes(baseKey)) return baseKey;

  let counter = 2;
  while (existingKeys.includes(`${baseKey}-${counter}`)) counter++;
  return `${baseKey}-${counter}`;
};

export const ManagedProviderWorkspace = forwardRef<
  ProviderCatalogHandle,
  ManagedProviderWorkspaceProps
>(function ManagedProviderWorkspace(
  { appId, isProxyRunning, isProxyTakeover, activeProviderId },
  ref,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    provider: Provider;
    action: "remove" | "delete";
  } | null>(null);
  const effectiveEditingProvider = useLastValidValue(editingProvider);
  const effectiveUsageProvider = useLastValidValue(usageProvider);

  useImperativeHandle(ref, () => ({
    openCreate: () => setIsAddOpen(true),
  }));

  const { data, isLoading, refetch } = useProvidersQuery(appId, {
    isProxyRunning,
  });
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";
  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
  } = useProviderActions(appId, isProxyRunning, isProxyTakeover);

  const disableOmoMutation = useDisableCurrentOmo();
  const disableOmoSlimMutation = useDisableCurrentOmoSlim();

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;

    const setupListener = async () => {
      try {
        const off = await providersApi.onSwitched(
          async (event: ProviderSwitchEvent) => {
            if (event.appType === appId) await refetch();
          },
        );
        if (!active) {
          off();
          return;
        }
        unsubscribe = off;
      } catch (error) {
        console.error(
          "[ManagedProviderWorkspace] Failed to subscribe provider switch event",
          error,
        );
      }
    };

    void setupListener();
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [appId, refetch]);

  const handleDisableOmo = () => {
    disableOmoMutation.mutate(undefined, {
      onSuccess: () =>
        toast.success(t("omo.disabled", { defaultValue: "OMO 已停用" })),
      onError: (error: Error) =>
        toast.error(
          t("omo.disableFailed", {
            defaultValue: "停用 OMO 失败: {{error}}",
            error: extractErrorMessage(error),
          }),
        ),
    });
  };

  const handleDisableOmoSlim = () => {
    disableOmoSlimMutation.mutate(undefined, {
      onSuccess: () =>
        toast.success(t("omo.disabled", { defaultValue: "OMO 已停用" })),
      onError: (error: Error) =>
        toast.error(
          t("omo.disableFailed", {
            defaultValue: "停用 OMO 失败: {{error}}",
            error: extractErrorMessage(error),
          }),
        ),
    });
  };

  const handleOpenWebsite = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("notifications.openLinkFailed", {
            defaultValue: "链接打开失败",
          }),
      );
    }
  };

  const handleEditProvider = async ({
    provider,
    originalId,
  }: {
    provider: Provider;
    originalId?: string;
  }) => {
    await updateProvider(provider, originalId);
    setEditingProvider(null);
  };

  const handleConfirmAction = async () => {
    if (!confirmAction) return;
    const { provider, action } = confirmAction;

    if (action === "remove") {
      await providersApi.removeFromLiveConfig(provider.id, appId);
      if (appId === "opencode") {
        await queryClient.invalidateQueries({
          queryKey: ["opencodeLiveProviderIds"],
        });
      } else if (appId === "openclaw") {
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.liveProviderIds,
        });
        await queryClient.invalidateQueries({ queryKey: openclawKeys.health });
      } else if (appId === "hermes") {
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
  };

  const handleDuplicateProvider = async (provider: Provider) => {
    const newSortIndex =
      provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined;
    const duplicatedProvider: Omit<Provider, "id" | "createdAt"> & {
      providerKey?: string;
      addToLive?: boolean;
    } = {
      name: `${provider.name} copy`,
      settingsConfig: deepClone(provider.settingsConfig),
      websiteUrl: provider.websiteUrl,
      category: provider.category,
      sortIndex: newSortIndex,
      meta: provider.meta ? deepClone(provider.meta) : undefined,
      icon: provider.icon,
      iconColor: provider.iconColor,
    };

    if (["opencode", "openclaw", "hermes"].includes(appId)) {
      let liveProviderIds: string[];
      try {
        liveProviderIds =
          appId === "opencode"
            ? await queryClient.ensureQueryData({
                queryKey: ["opencodeLiveProviderIds"],
                queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
              })
            : appId === "openclaw"
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
          "[ManagedProviderWorkspace] Failed to load live provider IDs for duplication",
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
      duplicatedProvider.providerKey = generateUniqueProviderCopyKey(
        provider.id,
        Array.from(new Set([...Object.keys(providers), ...liveProviderIds])),
      );
      duplicatedProvider.addToLive = false;
    }

    if (provider.sortIndex !== undefined) {
      const updates = Object.values(providers)
        .filter(
          (item) =>
            item.sortIndex !== undefined &&
            item.sortIndex >= newSortIndex! &&
            item.id !== provider.id,
        )
        .map((item) => ({
          id: item.id,
          sortIndex: item.sortIndex! + 1,
        }));
      if (updates.length > 0) {
        try {
          await providersApi.updateSortOrder(updates, appId);
        } catch (error) {
          console.error(
            "[ManagedProviderWorkspace] Failed to update sort order",
            error,
          );
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
  };

  const handleOpenTerminal = async (provider: Provider) => {
    try {
      const selectedDir = await settingsApi.pickDirectory();
      if (!selectedDir) return;
      await providersApi.openTerminal(provider.id, appId, { cwd: selectedDir });
      toast.success(
        t("provider.terminalOpened", { defaultValue: "终端已打开" }),
      );
    } catch (error) {
      const errorMessage = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed", { defaultValue: "打开终端失败" }) +
          (errorMessage ? `: ${errorMessage}` : ""),
      );
    }
  };

  return (
    <>
      <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
        <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1">
          <AnimatePresence mode="wait">
            <motion.div
              key={appId}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.15 }}
              className="space-y-4"
            >
              <ProviderList
                providers={providers}
                currentProviderId={currentProviderId}
                appId={appId}
                isLoading={isLoading}
                isProxyRunning={isProxyRunning}
                isProxyTakeover={isProxyTakeover}
                activeProviderId={activeProviderId}
                onSwitch={switchProvider}
                onEdit={setEditingProvider}
                onDelete={(provider) =>
                  setConfirmAction({ provider, action: "delete" })
                }
                onRemoveFromConfig={
                  ["opencode", "openclaw", "hermes"].includes(appId)
                    ? (provider) =>
                        setConfirmAction({ provider, action: "remove" })
                    : undefined
                }
                onDisableOmo={
                  appId === "opencode" ? handleDisableOmo : undefined
                }
                onDisableOmoSlim={
                  appId === "opencode" ? handleDisableOmoSlim : undefined
                }
                onDuplicate={handleDuplicateProvider}
                onConfigureUsage={setUsageProvider}
                onOpenWebsite={handleOpenWebsite}
                onOpenTerminal={
                  appId === "claude" ? handleOpenTerminal : undefined
                }
                onCreate={() => setIsAddOpen(true)}
                onSetAsDefault={
                  appId === "openclaw"
                    ? setAsDefaultModel
                    : appId === "hermes"
                      ? switchProvider
                      : undefined
                }
              />
            </motion.div>
          </AnimatePresence>
        </div>
      </div>

      <AddProviderDialog
        open={isAddOpen}
        onOpenChange={setIsAddOpen}
        appId={appId}
        onSubmit={addProvider}
      />
      <EditProviderDialog
        open={Boolean(editingProvider)}
        provider={effectiveEditingProvider}
        onOpenChange={(open) => {
          if (!open) setEditingProvider(null);
        }}
        onSubmit={handleEditProvider}
        appId={appId}
        isProxyTakeover={isProxyTakeover}
      />
      {effectiveUsageProvider && (
        <UsageScriptModal
          key={effectiveUsageProvider.id}
          provider={effectiveUsageProvider}
          appId={appId}
          isOpen={Boolean(usageProvider)}
          onClose={() => setUsageProvider(null)}
          onSave={(script) => {
            if (usageProvider) void saveUsageScript(usageProvider, script);
          }}
        />
      )}
      <ConfirmDialog
        isOpen={Boolean(confirmAction)}
        title={
          confirmAction?.action === "remove"
            ? t("confirm.removeProvider")
            : t("confirm.deleteProvider")
        }
        message={
          confirmAction
            ? confirmAction.action === "remove"
              ? t("confirm.removeProviderMessage", {
                  name: confirmAction.provider.name,
                })
              : t("confirm.deleteProviderMessage", {
                  name: confirmAction.provider.name,
                })
            : ""
        }
        onConfirm={() => void handleConfirmAction()}
        onCancel={() => setConfirmAction(null)}
      />
    </>
  );
});
