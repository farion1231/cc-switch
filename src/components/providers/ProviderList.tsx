import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
  rectSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useMemo,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { useDragSort } from "@/hooks/useDragSort";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { useListControls } from "@/hooks/useListControls";
import { useSearchShortcut } from "@/components/common/SearchOverlay";
import { useSettingsQuery } from "@/lib/query";
import { cn } from "@/lib/utils";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderCardCompact } from "@/components/providers/ProviderCardCompact";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { ListToolbar } from "@/components/common/ListToolbar";
import { SearchOverlay } from "@/components/common/SearchOverlay";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
} from "@/lib/query/failover";
import { useCallback } from "react";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  /** OpenCode: remove from live config (not delete from database) */
  onRemoveFromConfig?: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onCreate?: () => void;
  isLoading?: boolean;
  isProxyRunning?: boolean; // 代理服务运行状态
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管）
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onOpenTerminal,
  onCreate,
  isLoading = false,
  isProxyRunning = false,
  isProxyTakeover = false,
  activeProviderId,
}: ProviderListProps) {
  const { t } = useTranslation();

  // List controls (view mode, search, sort)
  const panelId = `providers-${appId}`;
  const {
    viewMode,
    searchTerm,
    sortField,
    sortOrder,
    isSearchOpen,
    isAnonymousMode,
    setViewMode,
    setSearchTerm,
    setSortField,
    toggleSortOrder,
    openSearch,
    closeSearch,
    clearSearch,
    toggleAnonymousMode,
    filterItems,
    sortItems,
  } = useListControls({ panelId });

  // Keyboard shortcut for search (from settings or default Cmd/Ctrl+K)
  const { data: settings } = useSettingsQuery();
  const searchShortcut = settings?.searchShortcut || "mod+k";
  useSearchShortcut(openSearch, searchShortcut);

  // 计算当前显示的排序列表（用于拖动时作为基础）
  const currentDisplayedProviders = useMemo(() => {
    const providerList = Object.values(providers);
    if (sortField === "custom") {
      // 自定义排序：按 sortIndex 排序
      return [...providerList].sort((a, b) => {
        const indexA = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
        const indexB = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
        if (indexA !== indexB) return sortOrder === "asc" ? indexA - indexB : indexB - indexA;
        return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
      });
    } else if (sortField === "createdAt") {
      // 按创建时间排序
      return [...providerList].sort((a, b) => {
        const timeA = a.createdAt ?? 0;
        const timeB = b.createdAt ?? 0;
        return sortOrder === "asc" ? timeA - timeB : timeB - timeA;
      });
    } else {
      // 按名称排序
      return [...providerList].sort((a, b) => {
        const comparison = a.name.toLowerCase().localeCompare(b.name.toLowerCase());
        return sortOrder === "asc" ? comparison : -comparison;
      });
    }
  }, [providers, sortField, sortOrder]);

  // 拖动排序 hook - 传递当前显示列表和切换到自定义排序的回调
  const { sortedProviders, sensors, handleDragEnd } = useDragSort({
    providers,
    appId,
    displayedProviders: currentDisplayedProviders,
    sortField,
    onSwitchToCustomSort: () => setSortField("custom"),
  });

  // OpenCode: 查询 live 配置中的供应商 ID 列表，用于判断 isInConfig
  const { data: opencodeLiveIds } = useQuery({
    queryKey: ["opencodeLiveProviderIds"],
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
    enabled: appId === "opencode",
  });

  // OpenCode: 判断供应商是否已添加到 opencode.json
  const isProviderInConfig = useCallback(
    (providerId: string): boolean => {
      if (appId !== "opencode") return true; // 非 OpenCode 应用始终返回 true
      return opencodeLiveIds?.includes(providerId) ?? false;
    },
    [appId, opencodeLiveIds],
  );

  // 流式健康检查
  const { checkProvider, isChecking } = useStreamCheck(appId);

  // 故障转移相关
  const { data: isAutoFailoverEnabled } = useAutoFailoverEnabled(appId);
  const { data: failoverQueue } = useFailoverQueue(appId);
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();

  // 联动状态：只有当前应用开启代理接管且故障转移开启时才启用故障转移模式
  const isFailoverModeActive =
    isProxyTakeover === true && isAutoFailoverEnabled === true;

  // 计算供应商在故障转移队列中的优先级（基于 sortIndex 排序）
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

  // 判断供应商是否在故障转移队列中
  const isInFailoverQueue = useCallback(
    (providerId: string): boolean => {
      if (!isFailoverModeActive || !failoverQueue) return false;
      return failoverQueue.some((item) => item.providerId === providerId);
    },
    [isFailoverModeActive, failoverQueue],
  );

  // 切换供应商的故障转移队列状态
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

  const handleTest = (provider: Provider) => {
    checkProvider(provider.id, provider.name);
  };

  // Apply filtering and sorting
  const processedProviders = useMemo(() => {
    // 使用当前显示的排序列表，然后应用搜索过滤
    return filterItems(currentDisplayedProviders);
  }, [currentDisplayedProviders, filterItems]);

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
    return <ProviderEmptyState onCreate={onCreate} />;
  }

  const renderListView = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={processedProviders.map((provider) => provider.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className="space-y-3">
          {processedProviders.map((provider) => (
            <SortableProviderCard
              key={provider.id}
              provider={provider}
              isCurrent={provider.id === currentProviderId}
              appId={appId}
              isInConfig={isProviderInConfig(provider.id)}
              onSwitch={onSwitch}
              onEdit={onEdit}
              onDelete={onDelete}
              onRemoveFromConfig={onRemoveFromConfig}
              onDuplicate={onDuplicate}
              onConfigureUsage={onConfigureUsage}
              onOpenWebsite={onOpenWebsite}
              onOpenTerminal={onOpenTerminal}
              onTest={appId !== "opencode" ? handleTest : undefined}
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
              viewMode="list"
              isAnonymousMode={isAnonymousMode}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );

  const renderCardView = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={processedProviders.map((provider) => provider.id)}
        strategy={rectSortingStrategy}
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          {processedProviders.map((provider) => (
            <SortableProviderCard
              key={provider.id}
              provider={provider}
              isCurrent={provider.id === currentProviderId}
              appId={appId}
              isInConfig={isProviderInConfig(provider.id)}
              onSwitch={onSwitch}
              onEdit={onEdit}
              onDelete={onDelete}
              onRemoveFromConfig={onRemoveFromConfig}
              onDuplicate={onDuplicate}
              onConfigureUsage={onConfigureUsage}
              onOpenWebsite={onOpenWebsite}
              onOpenTerminal={onOpenTerminal}
              onTest={appId !== "opencode" ? handleTest : undefined}
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
              viewMode="card"
              isAnonymousMode={isAnonymousMode}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );

  return (
    <div className="mt-4 space-y-4">
      {/* Toolbar */}
      <ListToolbar
        viewMode={viewMode}
        sortField={sortField}
        sortOrder={sortOrder}
        isSearchOpen={isSearchOpen}
        isLoading={isLoading}
        isAnonymousMode={isAnonymousMode}
        onAnonymousModeToggle={toggleAnonymousMode}
        onViewModeChange={setViewMode}
        onSortFieldChange={setSortField}
        onSortOrderToggle={toggleSortOrder}
        onSearchOpen={openSearch}
      />

      {/* Search Overlay */}
      <SearchOverlay
        isOpen={isSearchOpen}
        searchTerm={searchTerm}
        placeholder={t("provider.searchPlaceholder", {
          defaultValue: "Search name, notes, or URL...",
        })}
        scopeHint={t("provider.searchScopeHint", {
          defaultValue: "Matches provider name, notes, and URL.",
        })}
        onSearchChange={setSearchTerm}
        onClose={closeSearch}
        onClear={clearSearch}
      />

      {/* Content */}
      {processedProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : viewMode === "card" ? (
        renderCardView()
      ) : (
        renderListView()
      )}
    </div>
  );
}

interface SortableProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig: boolean;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  /** OpenCode: remove from live config (not delete from database) */
  onRemoveFromConfig?: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onOpenTerminal?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  isTesting: boolean;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
  // 故障转移相关
  isAutoFailoverEnabled: boolean;
  failoverPriority?: number;
  isInFailoverQueue: boolean;
  onToggleFailover: (enabled: boolean) => void;
  activeProviderId?: string;
  viewMode: "list" | "card";
  // 匿名模式
  isAnonymousMode?: boolean;
}

function SortableProviderCard({
  provider,
  isCurrent,
  appId,
  isInConfig,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
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
  viewMode,
  isAnonymousMode,
}: SortableProviderCardProps) {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: provider.id });

  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  const dragHandleProps = {
    attributes,
    listeners,
    isDragging,
  };

  const commonProps = {
    provider,
    isCurrent,
    appId,
    isInConfig,
    onSwitch,
    onEdit,
    onDelete,
    onRemoveFromConfig,
    onDuplicate,
    onConfigureUsage: onConfigureUsage ? (item: Provider) => onConfigureUsage(item) : () => undefined,
    onOpenWebsite,
    onOpenTerminal,
    onTest,
    isTesting,
    isProxyRunning,
    isProxyTakeover,
    dragHandleProps,
    isAutoFailoverEnabled,
    failoverPriority,
    isInFailoverQueue,
    onToggleFailover,
    activeProviderId,
    isAnonymousMode,
  };

  return (
    <div ref={setNodeRef} style={style} className={cn(viewMode === "card" && "h-full", isDragging && "relative z-[100]")}>
      {viewMode === "card" ? (
        <ProviderCardCompact {...commonProps} />
      ) : (
        <ProviderCard {...commonProps} />
      )}
    </div>
  );
}
