import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter, type DragEndEvent } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { useDragSort } from "@/hooks/useDragSort";
import {
  useOpenClawLiveProviderIds,
  useOpenClawDefaultModel,
} from "@/hooks/useOpenClaw";
import {
  useHermesLiveProviderIds,
  useHermesModelConfig,
} from "@/hooks/useHermes";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  ProviderManagementToolbar,
  type ProviderViewMode,
} from "@/components/providers/ProviderManagementToolbar";
import { ProviderCompactRow } from "@/components/providers/ProviderCompactRow";
import {
  ProviderConfigDrawer,
  type ProviderConfigDrawerState,
} from "@/components/providers/ProviderConfigDrawer";
import {
  buildProviderGroupSortUpdates,
  buildProviderGroups,
  type ProviderDisplayGroup,
} from "@/lib/provider-management/providerGrouping";
import {
  applyGroupCommonConfig,
  type GroupCommonConfigKey,
} from "@/lib/provider-management/providerGroupCommonConfig";
import { extractProviderSummary } from "@/lib/provider-management/providerSummary";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
} from "@/lib/query/failover";
import {
  useCurrentOmoProviderId,
  useCurrentOmoSlimProviderId,
} from "@/lib/query/omo";
import { isTextEditableTarget } from "@/utils/domUtils";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (provider: Provider) => void | Promise<void>;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onBatchDelete?: (providers: Provider[]) => void | Promise<void>;
  onRemoveFromConfig?: (provider: Provider) => void;
  onBatchRemoveFromConfig?: (providers: Provider[]) => void | Promise<void>;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onCreate?: () => void;
  isLoading?: boolean;
  isProxyRunning?: boolean;
  isProxyTakeover?: boolean;
  activeProviderId?: string;
  onSetAsDefault?: (provider: Provider) => void;
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onBatchDelete,
  onRemoveFromConfig,
  onBatchRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onCreate,
  isLoading = false,
  isProxyRunning = false,
  isProxyTakeover = false,
  activeProviderId,
  onSetAsDefault,
}: ProviderListProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const { sortedProviders, sensors } = useDragSort(providers, appId);

  const { data: opencodeLiveIds } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });

  const { data: openclawLiveIds } = useOpenClawLiveProviderIds(
    appId === "openclaw",
  );
  const { data: hermesLiveIds } = useHermesLiveProviderIds(appId === "hermes");
  const { data: hermesModelConfig } = useHermesModelConfig(appId === "hermes");
  const hermesCurrentProviderId = hermesModelConfig?.provider;

  const isProviderInConfig = useCallback(
    (providerId: string): boolean => {
      if (appId === "opencode") {
        return opencodeLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "openclaw") {
        return openclawLiveIds?.includes(providerId) ?? false;
      }
      if (appId === "hermes") {
        return hermesLiveIds?.includes(providerId) ?? false;
      }
      return true;
    },
    [appId, opencodeLiveIds, openclawLiveIds, hermesLiveIds],
  );

  const { data: openclawDefaultModel } = useOpenClawDefaultModel(
    appId === "openclaw",
  );

  const isProviderDefaultModel = useCallback(
    (providerId: string): boolean => {
      if (appId !== "openclaw" || !openclawDefaultModel?.primary) return false;
      return openclawDefaultModel.primary.startsWith(providerId + "/");
    },
    [appId, openclawDefaultModel],
  );

  const { data: isAutoFailoverEnabled } = useAutoFailoverEnabled(appId);
  const { data: failoverQueue } = useFailoverQueue(appId);
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();

  const isFailoverModeActive =
    isProxyTakeover === true && isAutoFailoverEnabled === true;

  const isOpenCode = appId === "opencode";
  const { data: currentOmoId } = useCurrentOmoProviderId(isOpenCode);
  const { data: currentOmoSlimId } = useCurrentOmoSlimProviderId(isOpenCode);

  const getFailoverPriority = useCallback(
    (providerId: string): number | undefined => {
      if (!isFailoverModeActive || !failoverQueue) return undefined;
      const index = failoverQueue.findIndex(
        (item) => item.providerId === providerId,
      );
      return index >= 0 ? index + 1 : undefined;
    },
    [isFailoverModeActive, failoverQueue],
  );

  const isInFailoverQueue = useCallback(
    (providerId: string): boolean => {
      if (!isFailoverModeActive || !failoverQueue) return false;
      return failoverQueue.some((item) => item.providerId === providerId);
    },
    [isFailoverModeActive, failoverQueue],
  );

  const handleToggleFailover = useCallback(
    (providerId: string, enabled: boolean) => {
      if (enabled) {
        addToQueue.mutate({ appType: appId, providerId });
      } else {
        removeFromQueue.mutate({ appType: appId, providerId });
      }
    },
    [appId, addToQueue, removeFromQueue],
  );

  const [searchTerm, setSearchTerm] = useState("");
  const [viewMode, setViewMode] = useState<ProviderViewMode>("cards");
  const [selectedProviderIds, setSelectedProviderIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [expandedGroupIds, setExpandedGroupIds] = useState<Set<string>>(
    () => new Set(),
  );
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [showBatchDeleteConfirm, setShowBatchDeleteConfirm] = useState(false);

  const { data: claudeDesktopStatus } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    enabled: appId === "claude-desktop",
    refetchInterval: appId === "claude-desktop" ? 5000 : false,
  });

  const runProviderTests = useCallback(
    (providersToTest: Provider[]) => {
      providersToTest.forEach((provider) =>
        checkProvider(provider.id, provider.name),
      );
    },
    [checkProvider],
  );

  const handleTestProviders = useCallback(
    (providersToTest: Provider[]) => {
      if (providersToTest.length === 0) return;
      runProviderTests(providersToTest);
    },
    [runProviderTests],
  );

  const handleTest = useCallback(
    (provider: Provider) => {
      handleTestProviders([provider]);
    },
    [handleTestProviders],
  );

  const importMutation = useMutation({
    mutationFn: async (): Promise<boolean> => {
      if (appId === "opencode") {
        const count = await providersApi.importOpenCodeFromLive();
        return count > 0;
      }
      if (appId === "openclaw") {
        const count = await providersApi.importOpenClawFromLive();
        return count > 0;
      }
      if (appId === "hermes") {
        const count = await providersApi.importHermesFromLive();
        return count > 0;
      }
      if (appId === "claude-desktop") {
        const count = await providersApi.importClaudeDesktopFromClaude();
        return count > 0;
      }
      return providersApi.importDefault(appId);
    },
    onSuccess: (imported) => {
      if (imported) {
        queryClient.invalidateQueries({ queryKey: ["providers", appId] });
        if (appId === "claude-desktop") {
          queryClient.invalidateQueries({ queryKey: ["claudeDesktopStatus"] });
        }
        toast.success(t("provider.importCurrentDescription"));
      } else {
        toast.info(t("provider.noProviders"));
      }
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const updateProviderMutation = useMutation({
    mutationFn: async ({
      provider,
      originalId,
    }: {
      provider: Provider;
      originalId: string;
    }) => providersApi.update(provider, appId, originalId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["providers", appId] });
      toast.success(
        t("provider.management.groupConfigSaved", {
          defaultValue: "Provider config updated",
        }),
      );
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;

      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && key === "f") {
        if (isTextEditableTarget(document.activeElement)) return;
        event.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
        return;
      }

      if (key === "escape" && searchTerm) {
        setSearchTerm("");
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [searchTerm]);

  const filteredProviders = useMemo(() => {
    const keywords = searchTerm
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    if (keywords.length === 0) return sortedProviders;

    return sortedProviders.filter((provider) => {
      const haystack = extractProviderSummary(provider, appId)
        .searchText.join(" ")
        .toLowerCase();
      return keywords.every((keyword) => haystack.includes(keyword));
    });
  }, [appId, searchTerm, sortedProviders]);

  const providerGroups = useMemo(
    () => buildProviderGroups(filteredProviders, appId),
    [appId, filteredProviders],
  );
  const allProviderGroups = useMemo(
    () => buildProviderGroups(sortedProviders, appId),
    [appId, sortedProviders],
  );

  useEffect(() => {
    const visibleIds = new Set(
      filteredProviders.map((provider) => provider.id),
    );
    setSelectedProviderIds((previous) => {
      const next = new Set(
        Array.from(previous).filter((providerId) => visibleIds.has(providerId)),
      );
      return next.size === previous.size ? previous : next;
    });
  }, [filteredProviders]);

  useEffect(() => {
    const visibleGroupIds = new Set(providerGroups.map((group) => group.id));
    setExpandedGroupIds((previous) => {
      const next = new Set(
        Array.from(previous).filter((groupId) => visibleGroupIds.has(groupId)),
      );
      return next.size === previous.size ? previous : next;
    });
  }, [providerGroups]);

  const selectedProviders = useMemo(
    () =>
      filteredProviders.filter((provider) =>
        selectedProviderIds.has(provider.id),
      ),
    [filteredProviders, selectedProviderIds],
  );

  const handleProviderSelectedChange = useCallback(
    (providerId: string, selected: boolean) => {
      setSelectedProviderIds((previous) => {
        const next = new Set(previous);
        if (selected) {
          next.add(providerId);
        } else {
          next.delete(providerId);
        }
        return next;
      });
    },
    [],
  );

  const clearSelection = useCallback(() => {
    setSelectedProviderIds(new Set());
  }, []);

  const handleBatchTest = useCallback(() => {
    handleTestProviders(selectedProviders);
  }, [handleTestProviders, selectedProviders]);

  const handleGroupDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const updates = buildProviderGroupSortUpdates(
        allProviderGroups,
        event.active.id,
        event.over?.id,
        providerGroups,
      );
      if (!updates.length) return;

      try {
        await providersApi.updateSortOrder(updates, appId);
        await queryClient.invalidateQueries({
          queryKey: ["providers", appId],
        });
        await queryClient.invalidateQueries({
          queryKey: ["failoverQueue", appId],
        });

        try {
          await providersApi.updateTrayMenu();
        } catch (trayError) {
          console.error("Failed to update tray menu after sort", trayError);
        }

        toast.success(
          t("provider.sortUpdated", {
            defaultValue: "Sort order updated",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("Failed to update provider sort order", error);
        toast.error(
          t("provider.sortUpdateFailed", {
            defaultValue: "Failed to update sort order",
          }),
        );
      }
    },
    [allProviderGroups, appId, providerGroups, queryClient, t],
  );

  const toggleGroupDrawer = useCallback((groupId: string) => {
    setExpandedGroupIds((previous) => {
      const next = new Set(previous);
      if (next.has(groupId)) {
        next.delete(groupId);
      } else {
        next.add(groupId);
      }
      return next;
    });
  }, []);

  const isConfigBatchProvider = useCallback(
    (provider: Provider) =>
      appId === "openclaw" ||
      appId === "hermes" ||
      (appId === "opencode" &&
        provider.category !== "omo" &&
        provider.category !== "omo-slim"),
    [appId],
  );

  const getProviderDisplayState = useCallback(
    (
      provider: Provider,
    ): ProviderConfigDrawerState & {
      failoverPriority?: number;
    } => {
      const isOmo = provider.category === "omo";
      const isOmoSlim = provider.category === "omo-slim";
      const isOmoCurrent = isOmo && provider.id === (currentOmoId || "");
      const isOmoSlimCurrent =
        isOmoSlim && provider.id === (currentOmoSlimId || "");
      const isHermesCurrent =
        appId === "hermes" && hermesCurrentProviderId === provider.id;
      const isDefaultModel =
        appId === "hermes"
          ? isHermesCurrent
          : isProviderDefaultModel(provider.id);
      const isCurrent = isOmo
        ? isOmoCurrent
        : isOmoSlim
          ? isOmoSlimCurrent
          : appId === "hermes"
            ? isHermesCurrent
            : provider.id === currentProviderId;

      return {
        isOmo,
        isOmoSlim,
        isCurrent,
        isDefaultModel,
        isInConfig: isProviderInConfig(provider.id),
        failoverPriority: getFailoverPriority(provider.id),
        isInFailoverQueue: isInFailoverQueue(provider.id),
      };
    },
    [
      appId,
      currentOmoId,
      currentOmoSlimId,
      currentProviderId,
      getFailoverPriority,
      hermesCurrentProviderId,
      isInFailoverQueue,
      isProviderDefaultModel,
      isProviderInConfig,
    ],
  );

  const getGroupDisplayProvider = useCallback(
    (group: ProviderDisplayGroup) => {
      const activeProvider = group.providers.find((provider) => {
        const state = getProviderDisplayState(provider);
        return (
          provider.id === activeProviderId ||
          state.isCurrent ||
          Boolean(state.isDefaultModel)
        );
      });
      if (activeProvider) return activeProvider;

      return (
        group.providers.find(
          (provider) => getProviderDisplayState(provider).isInConfig,
        ) ?? group.primaryProvider
      );
    },
    [activeProviderId, getProviderDisplayState],
  );

  const selectedProvidersToAddToConfig = useMemo(
    () =>
      selectedProviders.filter(
        (provider) =>
          isConfigBatchProvider(provider) && !isProviderInConfig(provider.id),
      ),
    [isConfigBatchProvider, isProviderInConfig, selectedProviders],
  );

  const selectedProvidersToRemoveFromConfig = useMemo(
    () =>
      selectedProviders.filter((provider) => {
        if (!onRemoveFromConfig || !isConfigBatchProvider(provider)) {
          return false;
        }
        const state = getProviderDisplayState(provider);
        return state.isInConfig && !state.isDefaultModel;
      }),
    [
      getProviderDisplayState,
      isConfigBatchProvider,
      onRemoveFromConfig,
      selectedProviders,
    ],
  );

  const selectedProvidersToAddToFailover = useMemo(
    () =>
      selectedProviders.filter((provider) => {
        const state = getProviderDisplayState(provider);
        return (
          isFailoverModeActive &&
          !state.isOmo &&
          !state.isOmoSlim &&
          !state.isInFailoverQueue
        );
      }),
    [getProviderDisplayState, isFailoverModeActive, selectedProviders],
  );

  const selectedProvidersToRemoveFromFailover = useMemo(
    () =>
      selectedProviders.filter((provider) => {
        const state = getProviderDisplayState(provider);
        return (
          isFailoverModeActive &&
          !state.isOmo &&
          !state.isOmoSlim &&
          state.isInFailoverQueue
        );
      }),
    [getProviderDisplayState, isFailoverModeActive, selectedProviders],
  );

  const handleBatchAddToConfig = useCallback(async () => {
    try {
      for (const provider of selectedProvidersToAddToConfig) {
        await onSwitch(provider);
      }
    } finally {
      clearSelection();
    }
  }, [clearSelection, onSwitch, selectedProvidersToAddToConfig]);

  const handleBatchRemoveFromConfig = useCallback(() => {
    if (!onBatchRemoveFromConfig) return;
    void onBatchRemoveFromConfig(selectedProvidersToRemoveFromConfig);
    clearSelection();
  }, [
    clearSelection,
    onBatchRemoveFromConfig,
    selectedProvidersToRemoveFromConfig,
  ]);

  const handleBatchAddToFailover = useCallback(() => {
    selectedProvidersToAddToFailover.forEach((provider) =>
      handleToggleFailover(provider.id, true),
    );
    clearSelection();
  }, [clearSelection, handleToggleFailover, selectedProvidersToAddToFailover]);

  const handleBatchRemoveFromFailover = useCallback(() => {
    selectedProvidersToRemoveFromFailover.forEach((provider) =>
      handleToggleFailover(provider.id, false),
    );
    clearSelection();
  }, [
    clearSelection,
    handleToggleFailover,
    selectedProvidersToRemoveFromFailover,
  ]);

  const handleConfirmBatchDelete = useCallback(() => {
    if (!onBatchDelete) return;
    void onBatchDelete(selectedProviders);
    setShowBatchDeleteConfirm(false);
    clearSelection();
  }, [clearSelection, onBatchDelete, selectedProviders]);

  const handleApplyGroupCommonConfig = useCallback(
    (
      provider: Provider,
      sourceProvider: Provider,
      keys: GroupCommonConfigKey[],
    ) => {
      const updatedProvider = applyGroupCommonConfig(
        provider,
        sourceProvider,
        appId,
        keys,
      );
      updateProviderMutation.mutate({
        provider: updatedProvider,
        originalId: provider.id,
      });
    },
    [appId, updateProviderMutation],
  );

  const claudeDesktopStatusMessages = useMemo(() => {
    if (appId !== "claude-desktop" || !claudeDesktopStatus) return [];

    const messages: string[] = [];
    if (!claudeDesktopStatus.supported) {
      messages.push(
        t("claudeDesktop.statusUnsupported", {
          defaultValue:
            "Claude Desktop 3P config writing is not supported on this platform.",
        }),
      );
      return messages;
    }

    if (claudeDesktopStatus.staleRawModels) {
      messages.push(
        t("claudeDesktop.statusStaleRawModels", {
          defaultValue:
            "Claude Desktop profile contains non-claude model names. Switching the current provider again can repair it.",
        }),
      );
    }
    if (claudeDesktopStatus.missingRouteMappings) {
      messages.push(
        t("claudeDesktop.statusMissingRouteMappings", {
          defaultValue:
            "This provider enables model mapping but has no valid route mappings. Edit the provider and add at least one mapping.",
        }),
      );
    }
    if (
      claudeDesktopStatus.mode === "proxy" &&
      !claudeDesktopStatus.gatewayTokenConfigured
    ) {
      messages.push(
        t("claudeDesktop.statusGatewayTokenMissing", {
          defaultValue:
            "The local routing token has not been generated. Switching this provider again will write a new local token.",
        }),
      );
    }

    const expected = claudeDesktopStatus.expectedBaseUrl?.replace(/\/+$/, "");
    const actual = claudeDesktopStatus.actualBaseUrl?.replace(/\/+$/, "");
    if (expected && actual && expected !== actual) {
      messages.push(
        t("claudeDesktop.statusBaseUrlMismatch", {
          expected,
          actual,
          defaultValue:
            "Claude Desktop profile points to {{actual}}, but should point to {{expected}}. Switching the current provider again can repair it.",
        }),
      );
    }

    return messages;
  }, [appId, claudeDesktopStatus, t]);

  if (isLoading) {
    return (
      <div className="space-y-3">
        {[0, 1, 2].map((index) => (
          <div
            key={index}
            className="w-full border border-dashed rounded-lg h-28 border-muted-foreground/40 bg-muted/40"
          />
        ))}
      </div>
    );
  }

  if (sortedProviders.length === 0) {
    return (
      <ProviderEmptyState
        appId={appId}
        onCreate={onCreate}
        onImport={() => importMutation.mutate()}
      />
    );
  }

  const renderGroupDrawer = (group: ProviderDisplayGroup) => {
    if (!group.isGrouped || !expandedGroupIds.has(group.id)) return null;
    const sourceProvider = getGroupDisplayProvider(group);

    return (
      <ProviderConfigDrawer
        groupId={group.id}
        groupLabel={group.label}
        providers={group.providers}
        primaryProvider={sourceProvider}
        appId={appId}
        getProviderState={getProviderDisplayState}
        onSwitch={onSwitch}
        onEdit={onEdit}
        onDelete={onDelete}
        onDuplicate={onDuplicate}
        onRemoveFromConfig={onRemoveFromConfig}
        onDisableOmo={onDisableOmo}
        onDisableOmoSlim={onDisableOmoSlim}
        onConfigureUsage={onConfigureUsage}
        onOpenTerminal={onOpenTerminal}
        onTest={handleTest}
        isTesting={isChecking}
        isProxyTakeover={isProxyTakeover}
        isAutoFailoverEnabled={isFailoverModeActive}
        onToggleFailover={handleToggleFailover}
        onSetAsDefault={onSetAsDefault}
        onApplyGroupCommonConfig={(provider, keys) =>
          handleApplyGroupCommonConfig(provider, sourceProvider, keys)
        }
      />
    );
  };

  const renderProviderList = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleGroupDragEnd}
    >
      <SortableContext
        items={providerGroups.map((group) => group.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className="space-y-3">
          {providerGroups.map((group) => {
            const provider = getGroupDisplayProvider(group);
            const state = getProviderDisplayState(provider);
            const isDrawerOpen = expandedGroupIds.has(group.id);

            return (
              <div key={group.id} className="space-y-2">
                <SortableProviderCard
                  sortableId={group.id}
                  provider={provider}
                  isCurrent={state.isCurrent}
                  isSelected={selectedProviderIds.has(provider.id)}
                  onSelectedChange={(selected) =>
                    handleProviderSelectedChange(provider.id, selected)
                  }
                  groupCount={group.providers.length}
                  isDrawerOpen={isDrawerOpen}
                  onToggleDrawer={
                    group.isGrouped
                      ? () => toggleGroupDrawer(group.id)
                      : undefined
                  }
                  appId={appId}
                  isInConfig={state.isInConfig}
                  isOmo={state.isOmo}
                  isOmoSlim={state.isOmoSlim}
                  onSwitch={onSwitch}
                  onEdit={onEdit}
                  onDelete={onDelete}
                  onRemoveFromConfig={onRemoveFromConfig}
                  onDisableOmo={onDisableOmo}
                  onDisableOmoSlim={onDisableOmoSlim}
                  onDuplicate={onDuplicate}
                  onConfigureUsage={onConfigureUsage}
                  onOpenWebsite={onOpenWebsite}
                  onOpenTerminal={onOpenTerminal}
                  onTest={handleTest}
                  isTesting={isChecking(provider.id)}
                  isProxyRunning={isProxyRunning}
                  isProxyTakeover={isProxyTakeover}
                  isAutoFailoverEnabled={isFailoverModeActive}
                  failoverPriority={state.failoverPriority}
                  isInFailoverQueue={state.isInFailoverQueue}
                  onToggleFailover={(enabled) =>
                    handleToggleFailover(provider.id, enabled)
                  }
                  activeProviderId={activeProviderId}
                  isDefaultModel={state.isDefaultModel}
                  onSetAsDefault={
                    onSetAsDefault ? () => onSetAsDefault(provider) : undefined
                  }
                />
                {renderGroupDrawer(group)}
              </div>
            );
          })}
        </div>
      </SortableContext>
    </DndContext>
  );

  const renderCompactList = () => (
    <div className="overflow-hidden rounded-lg border border-border">
      {providerGroups.map((group) => {
        const provider = getGroupDisplayProvider(group);
        const state = getProviderDisplayState(provider);
        const isDrawerOpen = expandedGroupIds.has(group.id);

        return (
          <div key={group.id}>
            <ProviderCompactRow
              provider={provider}
              summary={extractProviderSummary(provider, appId)}
              appId={appId}
              isCurrent={state.isCurrent}
              isInConfig={state.isInConfig}
              isSelected={selectedProviderIds.has(provider.id)}
              onSelectedChange={(selected) =>
                handleProviderSelectedChange(provider.id, selected)
              }
              isDrawerOpen={isDrawerOpen}
              onToggleDrawer={
                group.isGrouped ? () => toggleGroupDrawer(group.id) : undefined
              }
              groupCount={group.providers.length}
              isOmo={state.isOmo}
              isOmoSlim={state.isOmoSlim}
              onSwitch={() => onSwitch(provider)}
              onEdit={() => onEdit(provider)}
              onDelete={() => onDelete(provider)}
              onRemoveFromConfig={
                onRemoveFromConfig
                  ? () => onRemoveFromConfig(provider)
                  : undefined
              }
              onDuplicate={() => onDuplicate(provider)}
              onConfigureUsage={
                onConfigureUsage ? () => onConfigureUsage(provider) : undefined
              }
              onOpenTerminal={
                onOpenTerminal ? () => onOpenTerminal(provider) : undefined
              }
              onTest={() => handleTest(provider)}
              isTesting={isChecking(provider.id)}
              isProxyTakeover={isProxyTakeover}
              isAutoFailoverEnabled={isFailoverModeActive}
              failoverPriority={state.failoverPriority}
              isInFailoverQueue={state.isInFailoverQueue}
              onToggleFailover={(enabled) =>
                handleToggleFailover(provider.id, enabled)
              }
              isDefaultModel={state.isDefaultModel}
              onSetAsDefault={
                onSetAsDefault ? () => onSetAsDefault(provider) : undefined
              }
            />
            {isDrawerOpen && (
              <div className="border-b border-border bg-card px-3 py-3">
                {renderGroupDrawer(group)}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );

  return (
    <div className="mt-4 space-y-4">
      {claudeDesktopStatusMessages.length > 0 && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200">
          <div className="flex items-center gap-2 font-medium">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            {t("claudeDesktop.statusTitle", {
              defaultValue: "Claude Desktop config needs attention",
            })}
          </div>
          <ul className="mt-2 space-y-1 text-xs leading-relaxed">
            {claudeDesktopStatusMessages.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        </div>
      )}

      <ProviderManagementToolbar
        searchTerm={searchTerm}
        onSearchTermChange={setSearchTerm}
        searchInputRef={searchInputRef}
        visibleCount={filteredProviders.length}
        totalCount={sortedProviders.length}
        selectedCount={selectedProviderIds.size}
        viewMode={viewMode}
        onViewModeChange={setViewMode}
        onClearSelection={clearSelection}
        onBatchTest={selectedProviders.length ? handleBatchTest : undefined}
        onBatchAddToConfig={
          selectedProvidersToAddToConfig.length
            ? handleBatchAddToConfig
            : undefined
        }
        onBatchRemoveFromConfig={
          onBatchRemoveFromConfig && selectedProvidersToRemoveFromConfig.length
            ? handleBatchRemoveFromConfig
            : undefined
        }
        onBatchAddToFailover={
          selectedProvidersToAddToFailover.length
            ? handleBatchAddToFailover
            : undefined
        }
        onBatchRemoveFromFailover={
          selectedProvidersToRemoveFromFailover.length
            ? handleBatchRemoveFromFailover
            : undefined
        }
        onBatchDelete={
          onBatchDelete && selectedProviders.length
            ? () => setShowBatchDeleteConfirm(true)
            : undefined
        }
      />

      {filteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : viewMode === "compact" ? (
        renderCompactList()
      ) : (
        renderProviderList()
      )}

      <ConfirmDialog
        isOpen={showBatchDeleteConfirm}
        title={t("provider.management.batchDeleteConfirmTitle", {
          defaultValue: "Delete selected providers",
        })}
        message={t("provider.management.batchDeleteConfirmMessage", {
          count: selectedProviders.length,
          defaultValue:
            "Delete {{count}} selected providers? This action cannot be undone.",
        })}
        confirmText={t("provider.management.batchDeleteConfirmAction", {
          defaultValue: "Delete selected providers",
        })}
        onConfirm={handleConfirmBatchDelete}
        onCancel={() => setShowBatchDeleteConfirm(false)}
      />
    </div>
  );
}

interface SortableProviderCardProps {
  sortableId: string;
  provider: Provider;
  isCurrent: boolean;
  isSelected: boolean;
  onSelectedChange: (selected: boolean) => void;
  groupCount: number;
  isDrawerOpen: boolean;
  onToggleDrawer?: () => void;
  appId: AppId;
  isInConfig: boolean;
  isOmo: boolean;
  isOmoSlim: boolean;
  onSwitch: (provider: Provider) => void | Promise<void>;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting: boolean;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  isAutoFailoverEnabled: boolean;
  failoverPriority?: number;
  isInFailoverQueue: boolean;
  onToggleFailover: (enabled: boolean) => void;
  activeProviderId?: string;
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
}

function SortableProviderCard({
  sortableId,
  provider,
  isCurrent,
  isSelected,
  onSelectedChange,
  groupCount,
  isDrawerOpen,
  onToggleDrawer,
  appId,
  isInConfig,
  isOmo,
  isOmoSlim,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onTest,
  isTesting,
  isProxyRunning,
  isProxyTakeover,
  isAutoFailoverEnabled,
  failoverPriority,
  isInFailoverQueue,
  onToggleFailover,
  activeProviderId,
  isDefaultModel,
  onSetAsDefault,
}: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: sortableId });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div ref={setNodeRef} style={style}>
      <ProviderCard
        provider={provider}
        isCurrent={isCurrent}
        isSelected={isSelected}
        onSelectedChange={onSelectedChange}
        groupCount={groupCount}
        isDrawerOpen={isDrawerOpen}
        onToggleDrawer={onToggleDrawer}
        appId={appId}
        isInConfig={isInConfig}
        isOmo={isOmo}
        isOmoSlim={isOmoSlim}
        onSwitch={onSwitch}
        onEdit={onEdit}
        onDelete={onDelete}
        onRemoveFromConfig={onRemoveFromConfig}
        onDisableOmo={onDisableOmo}
        onDisableOmoSlim={onDisableOmoSlim}
        onDuplicate={onDuplicate}
        onConfigureUsage={
          onConfigureUsage ? (item) => onConfigureUsage(item) : () => undefined
        }
        onOpenWebsite={onOpenWebsite}
        onOpenTerminal={onOpenTerminal}
        onTest={onTest}
        isTesting={isTesting}
        isProxyRunning={isProxyRunning}
        isProxyTakeover={isProxyTakeover}
        dragHandleProps={{
          attributes,
          listeners,
          isDragging,
        }}
        isAutoFailoverEnabled={isAutoFailoverEnabled}
        failoverPriority={failoverPriority}
        isInFailoverQueue={isInFailoverQueue}
        onToggleFailover={onToggleFailover}
        activeProviderId={activeProviderId}
        isDefaultModel={isDefaultModel}
        onSetAsDefault={onSetAsDefault}
      />
    </div>
  );
}
