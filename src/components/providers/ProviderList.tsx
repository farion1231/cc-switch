import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useMemo,
  useCallback,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { useDragSort } from "@/hooks/useDragSort";
import { useListControls } from "@/hooks/useListControls";
// import { useStreamCheck } from "@/hooks/useStreamCheck"; // 测试功能已隐藏
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderCardCompact } from "@/components/providers/ProviderCardCompact";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { ListToolbar } from "@/components/common/ListToolbar";
import {
  SearchOverlay,
  useSearchShortcut,
} from "@/components/common/SearchOverlay";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
} from "@/lib/query/failover";
import { useSettingsQuery } from "@/lib/query";

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
  const { sortedProviders, sensors, handleDragEnd } = useDragSort(
    providers,
    appId,
  );

  // 获取设置（搜索快捷键）
  const { data: settings } = useSettingsQuery();
  const searchShortcut = settings?.searchShortcut || "mod+k";

  // 列表控制（视图模式、排序、搜索、匿名模式）
  const {
    viewMode,
    setViewMode,
    sortField,
    setSortField,
    sortOrder,
    toggleSortOrder,
    isSearchOpen,
    openSearch,
    closeSearch,
    searchTerm,
    setSearchTerm,
    clearSearch,
    searchHistory,
    addToSearchHistory,
    clearSearchHistory,
    isAnonymousMode,
    toggleAnonymousMode,
  } = useListControls({ panelId: "providers" });

  // 搜索快捷键
  useSearchShortcut(openSearch, searchShortcut, {
    isOpen: isSearchOpen,
    onClose: closeSearch,
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

  // 流式健康检查 - 功能已隐藏
  // const { checkProvider, isChecking } = useStreamCheck(appId);

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

  // handleTest 功能已隐藏 - 供应商请求格式复杂难以统一测试
  // const handleTest = (provider: Provider) => {
  //   checkProvider(provider.id, provider.name);
  // };

  // 解析搜索语法前缀
  const parseSearchTerm = useCallback((term: string) => {
    const trimmed = term.trim().toLowerCase();
    // 支持的前缀: name:, url:
    const prefixMatch = trimmed.match(/^(name|url):(.*)$/);
    if (prefixMatch) {
      return {
        prefix: prefixMatch[1] as "name" | "url",
        keyword: prefixMatch[2].trim(),
      };
    }
    return { prefix: null, keyword: trimmed };
  }, []);

  // 提取用于高亮的关键词和前缀
  const { highlightKeyword, highlightField } = useMemo(() => {
    const { prefix, keyword } = parseSearchTerm(searchTerm);
    return {
      highlightKeyword: keyword,
      highlightField: prefix, // "name" | "tag" | "note" | "url" | null
    };
  }, [searchTerm, parseSearchTerm]);

  // 根据排序字段和顺序对供应商进行排序
  const sortedAndFilteredProviders = useMemo(() => {
    let result = [...sortedProviders];

    // 搜索过滤
    const { prefix, keyword } = parseSearchTerm(searchTerm);
    if (keyword) {
      result = result.filter((provider) => {
        if (prefix === "name") {
          return provider.name?.toLowerCase().includes(keyword);
        }
        if (prefix === "url") {
          return provider.websiteUrl?.toLowerCase().includes(keyword);
        }
        // 无前缀时搜索名称和 URL
        const fields = [provider.name, provider.websiteUrl];
        return fields.some((field) =>
          field?.toString().toLowerCase().includes(keyword),
        );
      });
    }

    // 排序
    result.sort((a, b) => {
      let comparison = 0;
      if (sortField === "name") {
        comparison = a.name.localeCompare(b.name);
      } else if (sortField === "createdAt") {
        const aTime = a.createdAt ? new Date(a.createdAt).getTime() : 0;
        const bTime = b.createdAt ? new Date(b.createdAt).getTime() : 0;
        comparison = aTime - bTime;
      } else {
        // custom: 使用 sortIndex 排序
        const aIndex = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
        const bIndex = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
        comparison = aIndex - bIndex;
      }
      return sortOrder === "asc" ? comparison : -comparison;
    });

    return result;
  }, [sortedProviders, searchTerm, sortField, sortOrder, parseSearchTerm]);

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

  const renderProviderList = () => (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={sortedAndFilteredProviders.map((provider) => provider.id)}
        strategy={verticalListSortingStrategy}
      >
        {viewMode === "list" ? (
          <div className="space-y-3">
            {sortedAndFilteredProviders.map((provider) => (
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
                // onTest 功能已隐藏 - 供应商请求格式复杂难以统一测试
                // onTest={appId !== "opencode" ? handleTest : undefined}
                isTesting={false} // isChecking(provider.id) - 测试功能已隐藏
                isProxyRunning={isProxyRunning}
                isProxyTakeover={isProxyTakeover}
                // 故障转移相关：联动状态
                isAutoFailoverEnabled={isFailoverModeActive}
                failoverPriority={getFailoverPriority(provider.id)}
                isInFailoverQueue={isInFailoverQueue(provider.id)}
                onToggleFailover={(enabled) =>
                  handleToggleFailover(provider.id, enabled)
                }
                activeProviderId={activeProviderId}
                isAnonymousMode={isAnonymousMode}
                highlightQuery={highlightKeyword}
                highlightField={highlightField}
              />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {sortedAndFilteredProviders.map((provider) => (
              <ProviderCardCompact
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
                isProxyRunning={isProxyRunning}
                isProxyTakeover={isProxyTakeover}
                isAutoFailoverEnabled={isFailoverModeActive}
                failoverPriority={getFailoverPriority(provider.id)}
                isInFailoverQueue={isInFailoverQueue(provider.id)}
                onToggleFailover={(enabled) =>
                  handleToggleFailover(provider.id, enabled)
                }
                activeProviderId={activeProviderId}
                isAnonymousMode={isAnonymousMode}
                highlightQuery={highlightKeyword}
                highlightField={highlightField}
              />
            ))}
          </div>
        )}
      </SortableContext>
    </DndContext>
  );

  return (
    <div className="space-y-4">
      {/* 工具栏 */}
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

      {/* 搜索覆盖层 */}
      <SearchOverlay
        isOpen={isSearchOpen}
        searchTerm={searchTerm}
        placeholder={t("provider.searchPlaceholder", {
          defaultValue: "Search name, notes, or URL...",
        })}
        scopeHint={t("provider.searchScopeHint", {
          defaultValue: "Matches provider name, notes, and URL.",
        })}
        resultCount={sortedAndFilteredProviders.length}
        totalCount={sortedProviders.length}
        searchHistory={searchHistory}
        onSearchChange={setSearchTerm}
        onClose={closeSearch}
        onClear={clearSearch}
        onSelectHistory={setSearchTerm}
        onClearHistory={clearSearchHistory}
        onSearchSubmit={addToSearchHistory}
      />

      {sortedAndFilteredProviders.length === 0 ? (
        <div className="px-6 py-8 text-sm text-center border border-dashed rounded-lg border-border text-muted-foreground">
          {t("provider.noSearchResults", {
            defaultValue: "No providers match your search.",
          })}
        </div>
      ) : (
        renderProviderList()
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
  // 匿名模式和搜索高亮
  isAnonymousMode?: boolean;
  highlightQuery?: string;
  highlightField?: "name" | "url" | null;
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
  isAnonymousMode,
  highlightQuery,
  highlightField,
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

  return (
    <div ref={setNodeRef} style={style}>
      <ProviderCard
        provider={provider}
        isCurrent={isCurrent}
        appId={appId}
        isInConfig={isInConfig}
        onSwitch={onSwitch}
        onEdit={onEdit}
        onDelete={onDelete}
        onRemoveFromConfig={onRemoveFromConfig}
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
        // 故障转移相关
        isAutoFailoverEnabled={isAutoFailoverEnabled}
        failoverPriority={failoverPriority}
        isInFailoverQueue={isInFailoverQueue}
        onToggleFailover={onToggleFailover}
        activeProviderId={activeProviderId}
        isAnonymousMode={isAnonymousMode}
        highlightQuery={highlightQuery}
        highlightField={highlightField}
      />
    </div>
  );
}
