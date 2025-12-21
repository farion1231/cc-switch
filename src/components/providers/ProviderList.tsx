import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useMemo, useState, type CSSProperties } from "react";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { useDragSort } from "@/hooks/useDragSort";
import { useStreamCheck } from "@/hooks/useStreamCheck";
import { ProviderCard } from "@/components/providers/ProviderCard";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { Input } from "@/components/ui/input";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  appId: AppId;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onCreate?: () => void;
  isLoading?: boolean;
  isProxyRunning?: boolean; // 代理服务运行状态
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管）
}

export function ProviderList({
  providers,
  currentProviderId,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onCreate,
  isLoading = false,
  isProxyRunning = false, // 默认值为 false
  isProxyTakeover = false, // 默认值为 false
}: ProviderListProps) {
  const { t } = useTranslation();
  const { sortedProviders, sensors, handleDragEnd } = useDragSort(
    providers,
    appId
  );

  // 流式健康检查
  const { checkProvider, isChecking } = useStreamCheck(appId);

  const handleTest = (provider: Provider) => {
    checkProvider(provider.id, provider.name);
  };

  const [searchTerm, setSearchTerm] = useState("");

  const filteredProviders = useMemo(() => {
    const keyword = searchTerm.trim().toLowerCase();
    if (!keyword) return sortedProviders;
    return sortedProviders.filter((provider) => {
      const fields = [provider.name, provider.notes, provider.websiteUrl];
      return fields.some((field) =>
        field?.toString().toLowerCase().includes(keyword)
      );
    });
  }, [searchTerm, sortedProviders]);

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
        items={filteredProviders.map((provider) => provider.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className="space-y-3">
          {filteredProviders.map((provider) => (
            <SortableProviderCard
              key={provider.id}
              provider={provider}
              isCurrent={provider.id === currentProviderId}
              appId={appId}
              onSwitch={onSwitch}
              onEdit={onEdit}
              onDelete={onDelete}
              onDuplicate={onDuplicate}
              onConfigureUsage={onConfigureUsage}
              onOpenWebsite={onOpenWebsite}
              onTest={handleTest}
              isTesting={isChecking(provider.id)}
              isProxyRunning={isProxyRunning}
              isProxyTakeover={isProxyTakeover}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );

  return (
    <div className="mt-4 space-y-4">
      <div className="relative w-full sm:max-w-xs">
        <Search className="absolute w-4 h-4 -translate-y-1/2 pointer-events-none left-3 top-1/2 text-muted-foreground" />
        <Input
          value={searchTerm}
          onChange={(event) => setSearchTerm(event.target.value)}
          placeholder={t("provider.searchPlaceholder", {
            defaultValue: "Search name, notes, or URL...",
          })}
          aria-label={t("provider.searchAriaLabel", {
            defaultValue: "Search providers",
          })}
          className="pl-9"
        />
      </div>
      {filteredProviders.length === 0 ? (
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
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onConfigureUsage?: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onTest: (provider: Provider) => void;
  isTesting: boolean;
  isProxyRunning: boolean;
  isProxyTakeover: boolean;
}

function SortableProviderCard({
  provider,
  isCurrent,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onDuplicate,
  onConfigureUsage,
  onOpenWebsite,
  onTest,
  isTesting,
  isProxyRunning,
  isProxyTakeover,
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
        onSwitch={onSwitch}
        onEdit={onEdit}
        onDelete={onDelete}
        onDuplicate={onDuplicate}
        onConfigureUsage={
          onConfigureUsage ? (item) => onConfigureUsage(item) : () => undefined
        }
        onOpenWebsite={onOpenWebsite}
        onTest={onTest}
        isTesting={isTesting}
        isProxyRunning={isProxyRunning}
        isProxyTakeover={isProxyTakeover}
        dragHandleProps={{
          attributes,
          listeners,
          isDragging,
        }}
      />
    </div>
  );
}
