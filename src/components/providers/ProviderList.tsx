import {
  AlertTriangle,
  CheckSquare,
  Search,
  Trash2,
  X,
} from "lucide-react";
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
import { usePiLiveProviderIds, usePiActiveProvider } from "@/hooks/usePi";
import { piApi } from "@/lib/api/pi";
import { piKeys } from "@/hooks/usePi";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
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
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { isTextEditableTarget } from "@/utils/domUtils";
import { isHermesReadOnlyProvider } from "@/config/hermesProviderPresets";
import { extractErrorMessage } from "@/utils/errorUtils";
import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useCallback,
  type CSSProperties,
} from "react";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onCreate?: () => void;
  onDeleteMany?: (providers: Provider[]) => Promise<void>;
  isLoading?: boolean;
  isProxyRunning?: boolean; // 代理服务运行状态
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管）
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
  onSetAsDefault?: (provider: Provider) => void; // OpenClaw: set as default model
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
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
  onCreate,
  onDeleteMany,
  isLoading = false,
  isProxyRunning = false,
  isProxyTakeover = false,
  activeProviderId,
  onSetAsDefault,
}: ProviderListProps) {
  const { t } = useTranslation();
  const { checkProvider, isChecking } = useStreamCheck(appId);
  const { sortedProviders, sensors, handleDragEnd } = useDragSort(
    providers,
    appId,
  );

  const { data: opencodeLiveIds } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });

  // OpenClaw: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: openclawLiveIds } = useOpenClawLiveProviderIds(
    appId === "openclaw",
  );

  // Hermes: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: hermesLiveIds } = useHermesLiveProviderIds(appId === "hermes");

  // Pi: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: piLiveIds } = usePiLiveProviderIds(appId === "pi");

  // Hermes: 读取当前 model.provider，用于判断哪个供应商是"当前激活"（高亮）
  const { data: hermesModelConfig } = useHermesModelConfig(appId === "hermes");
  const hermesCurrentProviderId = hermesModelConfig?.provider;

  // Pi: 读取当前 active provider，用于判断哪个供应商是"当前激活"（高亮）
  const { data: piActiveProviderId } = usePiActiveProvider(appId === "pi");

  // 判断供应商是否已添加到配置（累加模式应用：OpenCode/OpenClaw/Hermes）
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
      if (appId === "pi") {
        return piLiveIds?.includes(providerId) ?? false;
      }
      return true; // 其他应用始终返回 true
    },
    [appId, opencodeLiveIds, openclawLiveIds, hermesLiveIds, piLiveIds],
  );

  // OpenClaw: query default model to determine which provider is default
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

  // 故障转移相关
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
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  const [isBatchDeleting, setIsBatchDeleting] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const { data: claudeDesktopStatus } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    enabled: appId === "claude-desktop",
    refetchInterval: appId === "claude-desktop" ? 5000 : false,
  });

  // 连通性检查不发真实请求、无封号/计费风险，直接执行（无需确认弹窗）。
  const handleTest = useCallback(
    (provider: Provider) => {
      checkProvider(provider.id, provider.name);
    },
    [checkProvider],
  );

  // Import current live config as default provider
  const queryClient = useQueryClient();
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
      if (appId === "pi") {
        const count = await piApi.importFromLive();
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
        if (appId === "pi") {
          queryClient.invalidateQueries({ queryKey: piKeys.liveProviderIds });
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

  useEffect(() => {
    setSearchTerm("");
    setSelectionMode(false);
    setSelectedIds(new Set());
    setBatchDeleteOpen(false);
  }, [appId]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;

      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && key === "f") {
        // 正在输入框/可编辑区域中时不抢占 Ctrl+F（例如添加供应商表单里
        // ProviderPresetSelector 的搜索框），避免与其同名快捷键冲突。
        if (isTextEditableTarget(document.activeElement)) return;
        event.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
        return;
      }

      if (key === "escape" && selectionMode) {
        setSelectionMode(false);
        setSelectedIds(new Set());
      }
    };

    globalThis.addEventListener("keydown", handleKeyDown);
    return () => globalThis.removeEventListener("keydown", handleKeyDown);
  }, [selectionMode]);

  const resolveIsCurrent = useCallback(
    (provider: Provider): boolean => {
      const isOmo = provider.category === "omo";
      const isOmoSlim = provider.category === "omo-slim";
      if (isOmo) return provider.id === (currentOmoId || "");
      if (isOmoSlim) return provider.id === (currentOmoSlimId || "");
      if (appId === "hermes") return hermesCurrentProviderId === provider.id;
      if (appId === "pi") return piActiveProviderId === provider.id;
      return provider.id === currentProviderId;
    },
    [
      appId,
      currentOmoId,
      currentOmoSlimId,
      hermesCurrentProviderId,
      piActiveProviderId,
      currentProviderId,
    ],
  );

  const isProviderDeletable = useCallback(
    (provider: Provider): boolean => {
      if (
        appId === "hermes" &&
        isHermesReadOnlyProvider(provider.settingsConfig)
      ) {
        return false;
      }
      const isOmo =
        provider.category === "omo" || provider.category === "omo-slim";
      const isAdditiveMode =
        (appId === "opencode" && !isOmo) ||
        appId === "openclaw" ||
        appId === "hermes" ||
        appId === "pi";
      if (isOmo || isAdditiveMode) return true;
      return !resolveIsCurrent(provider);
    },
    [appId, resolveIsCurrent],
  );

  const filteredProviders = useMemo(() => {
    const keyword = searchTerm.trim().toLowerCase();
    if (!keyword) return sortedProviders;
    return sortedProviders.filter((provider) => {
      const fields = [provider.name, provider.notes, provider.websiteUrl];
      return fields.some((field) =>
        field?.toString().toLowerCase().includes(keyword),
      );
    });
  }, [searchTerm, sortedProviders]);

  const deletableFilteredProviders = useMemo(
    () => filteredProviders.filter(isProviderDeletable),
    [filteredProviders, isProviderDeletable],
  );

  const selectedProviders = useMemo(
    () =>
      filteredProviders.filter(
        (provider) =>
          selectedIds.has(provider.id) && isProviderDeletable(provider),
      ),
    [filteredProviders, selectedIds, isProviderDeletable],
  );

  useEffect(() => {
    setSelectedIds((prev) => {
      if (prev.size === 0) return prev;
      const visibleIds = new Set(filteredProviders.map((p) => p.id));
      const next = new Set([...prev].filter((id) => visibleIds.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [filteredProviders]);

  const allDeletableSelected =
    deletableFilteredProviders.length > 0 &&
    deletableFilteredProviders.every((provider) =>
      selectedIds.has(provider.id),
    );

  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelectedIds(new Set());
    setBatchDeleteOpen(false);
  }, []);

  const toggleSelectAll = useCallback(() => {
    if (allDeletableSelected) {
      setSelectedIds(new Set());
      return;
    }
    setSelectedIds(new Set(deletableFilteredProviders.map((p) => p.id)));
  }, [allDeletableSelected, deletableFilteredProviders]);

  const handleBatchDeleteConfirm = useCallback(async () => {
    if (!onDeleteMany || selectedProviders.length === 0) return;
    setIsBatchDeleting(true);
    try {
      await onDeleteMany(selectedProviders);
      toast.success(
        t("provider.batchDeleteSuccess", {
          count: selectedProviders.length,
          defaultValue: `已删除 ${selectedProviders.length} 个供应商`,
        }),
        { closeButton: true },
      );
      exitSelectionMode();
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("provider.batchDeleteRequestFailed", {
            defaultValue: "批量删除失败，请稍后重试",
          }),
      );
    } finally {
      setIsBatchDeleting(false);
      setBatchDeleteOpen(false);
    }
  }, [onDeleteMany, selectedProviders, t, exitSelectionMode]);

  const claudeDesktopStatusMessages = useMemo(() => {
    if (appId !== "claude-desktop" || !claudeDesktopStatus) return [];

    const messages: string[] = [];
    if (!claudeDesktopStatus.supported) {
      messages.push(
        t("claudeDesktop.statusUnsupported", {
          defaultValue: "当前平台暂不支持 Claude Desktop 3P 配置写入。",
        }),
      );
      return messages;
    }

    if (claudeDesktopStatus.staleRawModels) {
      messages.push(
        t("claudeDesktop.statusStaleRawModels", {
          defaultValue:
            "Claude Desktop profile 中存在非 claude-* 模型名，新版 Claude Desktop 可能拒绝加载；重新切换当前供应商可修复。",
        }),
      );
    }
    if (claudeDesktopStatus.missingRouteMappings) {
      messages.push(
        t("claudeDesktop.statusMissingRouteMappings", {
          defaultValue:
            "当前供应商启用了模型映射，但没有有效路由；请编辑供应商并补全至少一个模型映射。",
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
            "当前本地路由 token 尚未生成；重新切换该供应商会写入新的本地 token。",
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
            "Claude Desktop profile 指向的地址与当前供应商不一致；当前为 {{actual}}，应为 {{expected}}。重新切换当前供应商可修复。",
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
            className="w-full border border-dashed rounded-2xl h-28 border-white/30 dark:border-white/15 glass-pill"
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

  const renderProviderList = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={filteredProviders.map((provider) => provider.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className="space-y-3">
          {filteredProviders.map((provider) => {
            const isOmo = provider.category === "omo";
            const isOmoSlim = provider.category === "omo-slim";
            return (
              <SortableProviderCard
                key={provider.id}
                provider={provider}
                isCurrent={resolveIsCurrent(provider)}
                appId={appId}
                isInConfig={isProviderInConfig(provider.id)}
                isOmo={isOmo}
                isOmoSlim={isOmoSlim}
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
                failoverPriority={getFailoverPriority(provider.id)}
                isInFailoverQueue={isInFailoverQueue(provider.id)}
                onToggleFailover={(enabled) =>
                  handleToggleFailover(provider.id, enabled)
                }
                activeProviderId={activeProviderId}
                // OpenClaw: default model / Hermes: model.provider === provider.id
                isDefaultModel={
                  appId === "hermes" || appId === "pi"
                    ? resolveIsCurrent(provider)
                    : isProviderDefaultModel(provider.id)
                }
                onSetAsDefault={
                  onSetAsDefault ? () => onSetAsDefault(provider) : undefined
                }
                selectionMode={selectionMode}
                selected={selectedIds.has(provider.id)}
                selectable={isProviderDeletable(provider)}
                onSelectChange={(next) => {
                  setSelectedIds((prev) => {
                    const copy = new Set(prev);
                    if (next) copy.add(provider.id);
                    else copy.delete(provider.id);
                    return copy;
                  });
                }}
              />
            );
          })}
        </div>
      </SortableContext>
    </DndContext>
  );

  return (
    <div className="mt-4 space-y-4">
      {claudeDesktopStatusMessages.length > 0 && (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200">
          <div className="flex items-center gap-2 font-medium">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            {t("claudeDesktop.statusTitle", {
              defaultValue: "Claude Desktop 配置需要检查",
            })}
          </div>
          <ul className="mt-2 space-y-1 text-xs leading-relaxed">
            {claudeDesktopStatusMessages.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        </div>
      )}

      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={searchInputRef}
            value={searchTerm}
            onChange={(event) => setSearchTerm(event.target.value)}
            placeholder={t("provider.searchPlaceholder", {
              defaultValue: "搜索供应商名称、备注或网址…",
            })}
            aria-label={t("provider.searchAriaLabel", {
              defaultValue: "搜索供应商",
            })}
            className="h-9 pl-9 pr-9"
          />
          {searchTerm ? (
            <Button
              variant="ghost"
              size="icon"
              className="absolute right-1 top-1/2 h-7 w-7 -translate-y-1/2"
              onClick={() => setSearchTerm("")}
              aria-label={t("common.clear", { defaultValue: "清除" })}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          {selectionMode ? (
            <>
              <Button
                variant="outline"
                size="sm"
                onClick={toggleSelectAll}
                disabled={deletableFilteredProviders.length === 0}
              >
                {allDeletableSelected
                  ? t("provider.deselectAll", { defaultValue: "取消全选" })
                  : t("provider.selectAll", { defaultValue: "全选" })}
              </Button>
              <Button
                variant="destructive"
                size="sm"
                disabled={selectedProviders.length === 0 || isBatchDeleting}
                onClick={() => setBatchDeleteOpen(true)}
              >
                <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                {t("provider.batchDelete", {
                  count: selectedProviders.length,
                  defaultValue: `一键删除${selectedProviders.length > 0 ? ` (${selectedProviders.length})` : ""}`,
                })}
              </Button>
              <Button variant="ghost" size="sm" onClick={exitSelectionMode}>
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
            </>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setSelectionMode(true)}
              disabled={deletableFilteredProviders.length === 0}
              title={t("provider.batchManageTooltip", {
                defaultValue: "批量选择并一键删除",
              })}
            >
              <CheckSquare className="mr-1.5 h-3.5 w-3.5" />
              {t("provider.batchManage", { defaultValue: "批量管理" })}
            </Button>
          )}
        </div>
      </div>

      {(searchTerm.trim() || selectionMode) && (
        <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground px-0.5">
          <span>
            {searchTerm.trim()
              ? t("provider.searchResultCount", {
                  count: filteredProviders.length,
                  total: sortedProviders.length,
                  defaultValue: `找到 ${filteredProviders.length} / ${sortedProviders.length} 个供应商`,
                })
              : t("provider.batchManageHint", {
                  defaultValue: "勾选后可一键删除；当前使用中的供应商不可删除。",
                })}
          </span>
          {selectionMode && (
            <span>
              {t("provider.selectedCount", {
                count: selectedProviders.length,
                defaultValue: `已选 ${selectedProviders.length} 个`,
              })}
            </span>
          )}
        </div>
      )}

      {filteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-2xl border-border text-muted-foreground glass-panel">
          {t("provider.noSearchResults", {
            defaultValue: "没有符合搜索条件的供应商。",
          })}
        </div>
      ) : (
        renderProviderList()
      )}

      <ConfirmDialog
        isOpen={batchDeleteOpen}
        title={t("provider.batchDeleteConfirmTitle", {
          defaultValue: "一键删除供应商",
        })}
        message={t("provider.batchDeleteConfirmMessage", {
          count: selectedProviders.length,
          defaultValue: `确定删除已选中的 ${selectedProviders.length} 个供应商吗？此操作无法撤销。`,
        })}
        confirmText={
          isBatchDeleting
            ? t("common.deleting", { defaultValue: "删除中..." })
            : t("provider.batchDeleteConfirmAction", {
                defaultValue: "一键删除",
              })
        }
        onConfirm={() => {
          void handleBatchDeleteConfirm();
        }}
        onCancel={() => {
          if (!isBatchDeleting) setBatchDeleteOpen(false);
        }}
      />
    </div>
  );
}

interface SortableProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig: boolean;
  isOmo: boolean;
  isOmoSlim: boolean;
  onSwitch: (provider: Provider) => void;
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
  // OpenClaw: default model
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
  selectionMode?: boolean;
  selected?: boolean;
  selectable?: boolean;
  onSelectChange?: (selected: boolean) => void;
}

function SortableProviderCard({
  provider,
  isCurrent,
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
  selectionMode,
  selected,
  selectable,
  onSelectChange,
}: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: provider.id, disabled: selectionMode });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div ref={setNodeRef} style={style}>
      <ProviderCard
        provider={provider}
        isCurrent={isCurrent}
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
        // OpenClaw: default model
        isDefaultModel={isDefaultModel}
        onSetAsDefault={onSetAsDefault}
        selectionMode={selectionMode}
        selected={selected}
        selectable={selectable}
        onSelectChange={onSelectChange}
      />
    </div>
  );
}
