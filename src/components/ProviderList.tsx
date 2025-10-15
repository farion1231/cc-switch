import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Provider, UsageScript } from "../types";
import { AppType } from "../lib/tauri-api";
import { Play, Edit3, Trash2, CheckCircle2, Users, Check, BarChart3, GripVertical } from "lucide-react";
import { buttonStyles, badgeStyles, cn } from "../lib/styles";
import UsageFooter from "./UsageFooter";
import UsageScriptModal from "./UsageScriptModal";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
// 不再在列表中显示分类徽章，避免造成困惑

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  onSwitch: (id: string) => void;
  onDelete: (id: string) => void;
  onEdit: (id: string) => void;
  appType: AppType;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
  onProvidersUpdated?: () => Promise<void>;
}

// Sortable Provider Item Component
interface SortableProviderItemProps {
  provider: Provider;
  isCurrent: boolean;
  apiUrl: string;
  onSwitch: (id: string) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
  onOpenUsageModal: (id: string) => void;
  onUrlClick: (url: string) => Promise<void>;
  appType: AppType;
  t: any;
}

const SortableProviderItem: React.FC<SortableProviderItemProps> = ({
  provider,
  isCurrent,
  apiUrl,
  onSwitch,
  onEdit,
  onDelete,
  onOpenUsageModal,
  onUrlClick,
  appType,
  t,
}) => {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    isDragging,
  } = useSortable({
    id: provider.id,
    animateLayoutChanges: () => false, // Disable layout animations
  });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition: 'none', // No transitions at all
    opacity: isDragging ? 0.5 : 1,
    zIndex: isDragging ? 1000 : undefined,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        // Base card styles without transitions that conflict with dragging
        "bg-white rounded-lg border p-4 dark:bg-gray-900",
        // Different border colors based on state
        isCurrent
          ? "border-blue-500 shadow-sm bg-blue-50 dark:border-blue-400 dark:bg-blue-400/10"
          : "border-gray-200 dark:border-gray-700",
        // Hover effects only when not dragging
        !isDragging && !isCurrent && "hover:border-gray-300 hover:shadow-sm dark:hover:border-gray-600",
        // Shadow during drag
        isDragging && "shadow-lg",
        // Only apply transition when not dragging to prevent conflicts
        !isDragging && "transition-[border-color,box-shadow] duration-200"
      )}
    >
      <div className="flex items-center justify-between">
        {/* Drag Handle */}
        <div
          {...attributes}
          {...listeners}
          className="cursor-grab active:cursor-grabbing p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded mr-2 transition-colors"
          title={t("provider.dragToReorder") || "拖拽以重新排序"}
        >
          <GripVertical size={20} className="text-gray-400" />
        </div>

        <div className="flex-1">
          <div className="flex items-center gap-3 mb-2">
            <h3 className="font-medium text-gray-900 dark:text-gray-100">
              {provider.name}
            </h3>
            <div
              className={cn(
                badgeStyles.success,
                !isCurrent && "invisible",
              )}
            >
              <CheckCircle2 size={12} />
              {t("provider.currentlyUsing")}
            </div>
          </div>

          <div className="flex items-center gap-2 text-sm">
            {provider.websiteUrl ? (
              <button
                onClick={(e) => {
                  e.preventDefault();
                  onUrlClick(provider.websiteUrl!);
                }}
                className="inline-flex items-center gap-1 text-blue-500 dark:text-blue-400 hover:opacity-90 transition-colors"
                title={t("providerForm.visitWebsite", {
                  url: provider.websiteUrl,
                })}
              >
                {provider.websiteUrl}
              </button>
            ) : (
              <span
                className="text-gray-500 dark:text-gray-400"
                title={apiUrl}
              >
                {apiUrl}
              </span>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2 ml-4">
          <button
            onClick={() => onSwitch(provider.id)}
            disabled={isCurrent}
            className={cn(
              "inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-[90px] justify-center whitespace-nowrap",
              isCurrent
                ? "bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-500 cursor-not-allowed"
                : "bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700",
            )}
          >
            {isCurrent ? <Check size={14} /> : <Play size={14} />}
            {isCurrent ? t("provider.inUse") : t("provider.enable")}
          </button>

          <button
            onClick={() => onEdit(provider.id)}
            className={buttonStyles.icon}
            title={t("provider.editProvider")}
          >
            <Edit3 size={16} />
          </button>

          <button
            onClick={() => onOpenUsageModal(provider.id)}
            className={buttonStyles.icon}
            title="配置用量查询"
          >
            <BarChart3 size={16} />
          </button>

          <button
            onClick={() => onDelete(provider.id)}
            disabled={isCurrent}
            className={cn(
              buttonStyles.icon,
              isCurrent
                ? "text-gray-400 cursor-not-allowed"
                : "text-gray-500 hover:text-red-500 hover:bg-red-100 dark:text-gray-400 dark:hover:text-red-400 dark:hover:bg-red-500/10",
            )}
            title={t("provider.deleteProvider")}
          >
            <Trash2 size={16} />
          </button>
        </div>
      </div>

      <UsageFooter
        providerId={provider.id}
        appType={appType}
        usageEnabled={provider.meta?.usage_script?.enabled || false}
      />
    </div>
  );
}

const ProviderList: React.FC<ProviderListProps> = ({
  providers,
  currentProviderId,
  onSwitch,
  onDelete,
  onEdit,
  appType,
  onNotify,
  onProvidersUpdated,
}) => {
  const { t, i18n } = useTranslation();
  const [usageModalProviderId, setUsageModalProviderId] = useState<string | null>(null);

  // Drag and drop sensors
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    })
  );

  // 提取API地址（兼容不同供应商配置：Claude env / Codex TOML）
  const getApiUrl = (provider: Provider): string => {
    try {
      const cfg = provider.settingsConfig;
      // Claude/Anthropic: 从 env 中读取
      if (cfg?.env?.ANTHROPIC_BASE_URL) {
        return cfg.env.ANTHROPIC_BASE_URL;
      }
      // Codex: 从 TOML 配置中解析 base_url
      if (typeof cfg?.config === "string" && cfg.config.includes("base_url")) {
        // 支持单/双引号
        const match = cfg.config.match(/base_url\s*=\s*(['"])([^'\"]+)\1/);
        if (match && match[2]) return match[2];
      }
      return t("provider.notConfigured");
    } catch {
      return t("provider.configError");
    }
  };

  const handleUrlClick = async (url: string) => {
    try {
      await window.api.openExternal(url);
    } catch (error) {
      console.error(t("console.openLinkFailed"), error);
      onNotify?.(
        `${t("console.openLinkFailed")}: ${String(error)}`,
        "error",
        4000,
      );
    }
  };

  // 列表页不再提供 Claude 插件按钮，统一在"设置"中控制

  // 处理用量配置保存
  const handleSaveUsageScript = async (providerId: string, script: UsageScript) => {
    try {
      const provider = providers[providerId];
      const updatedProvider = {
        ...provider,
        meta: {
          ...provider.meta,
          usage_script: script,
        },
      };
      await window.api.updateProvider(updatedProvider, appType);
      onNotify?.("用量查询配置已保存", "success", 2000);
      // 重新加载供应商列表,触发 UsageFooter 的 useEffect
      if (onProvidersUpdated) {
        await onProvidersUpdated();
      }
    } catch (error) {
      console.error("保存用量配置失败:", error);
      onNotify?.("保存失败", "error");
    }
  };

  // Sort providers
  const sortedProviders = React.useMemo(() => {
    return Object.values(providers).sort((a, b) => {
      // Priority 1: sortIndex
      if (a.sortIndex !== undefined && b.sortIndex !== undefined) {
        return a.sortIndex - b.sortIndex;
      }
      if (a.sortIndex !== undefined) return -1;
      if (b.sortIndex !== undefined) return 1;

      // Priority 2: createdAt
      const timeA = a.createdAt || 0;
      const timeB = b.createdAt || 0;
      if (timeA !== 0 && timeB !== 0) return timeA - timeB;
      if (timeA === 0 && timeB === 0) {
        // Priority 3: name
        const locale = i18n.language === "zh" ? "zh-CN" : "en-US";
        return a.name.localeCompare(b.name, locale);
      }
      return timeA === 0 ? -1 : 1;
    });
  }, [providers, i18n.language]);

  // Handle drag end - immediate refresh
  const handleDragEnd = React.useCallback(async (event: DragEndEvent) => {
    const { active, over } = event;

    if (!over || active.id === over.id) return;

    const oldIndex = sortedProviders.findIndex((p) => p.id === active.id);
    const newIndex = sortedProviders.findIndex((p) => p.id === over.id);

    if (oldIndex === -1 || newIndex === -1) return;

    // Calculate new sort order
    const reorderedProviders = arrayMove(sortedProviders, oldIndex, newIndex);
    const updates = reorderedProviders.map((provider, index) => ({
      id: provider.id,
      sortIndex: index,
    }));

    try {
      // Save to backend and refresh immediately
      await window.api.updateProvidersSortOrder(updates, appType);
      onProvidersUpdated?.();

      // Update tray menu to reflect new order
      await window.api.updateTrayMenu();
    } catch (error) {
      console.error("Failed to update sort order:", error);
      onNotify?.(t("provider.sortUpdateFailed") || "排序更新失败", "error");
    }
  }, [sortedProviders, appType, onProvidersUpdated, onNotify, t]);

  return (
    <div className="space-y-4">
      {sortedProviders.length === 0 ? (
        <div className="text-center py-12">
          <div className="w-16 h-16 mx-auto mb-4 bg-gray-100 rounded-full flex items-center justify-center">
            <Users size={24} className="text-gray-400" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            {t("provider.noProviders")}
          </h3>
          <p className="text-gray-500 dark:text-gray-400 text-sm">
            {t("provider.noProvidersDescription")}
          </p>
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={handleDragEnd}
          autoScroll={true}
        >
          <SortableContext
            items={sortedProviders.map((p) => p.id)}
            strategy={verticalListSortingStrategy}
          >
            <div className="space-y-3">
              {sortedProviders.map((provider) => {
                const isCurrent = provider.id === currentProviderId;
                const apiUrl = getApiUrl(provider);

                return (
                  <SortableProviderItem
                    key={provider.id}
                    provider={provider}
                    isCurrent={isCurrent}
                    apiUrl={apiUrl}
                    onSwitch={onSwitch}
                    onEdit={onEdit}
                    onDelete={onDelete}
                    onOpenUsageModal={setUsageModalProviderId}
                    onUrlClick={handleUrlClick}
                    appType={appType}
                    t={t}
                  />
                );
              })}
            </div>
          </SortableContext>
        </DndContext>
      )}

      {/* 用量配置模态框 */}
      {usageModalProviderId && providers[usageModalProviderId] && (
        <UsageScriptModal
          provider={providers[usageModalProviderId]}
          appType={appType!}
          onClose={() => setUsageModalProviderId(null)}
          onSave={(script) =>
            handleSaveUsageScript(usageModalProviderId, script)
          }
          onNotify={onNotify}
        />
      )}
    </div>
  );
};

export default ProviderList;
