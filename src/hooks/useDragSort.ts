import { useCallback, useMemo } from "react";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { arrayMove, sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import { providersApi, type AppId } from "@/lib/api";

export interface UseDragSortOptions {
  providers: Record<string, Provider>;
  appId: AppId;
  /** 当前显示的排序列表（可能是按时间、名称等排序后的结果） */
  displayedProviders?: Provider[];
  /** 当前排序字段，用于判断是否需要切换到自定义排序 */
  sortField?: string;
  /** 切换到自定义排序的回调 */
  onSwitchToCustomSort?: () => void;
}

export function useDragSort(
  providersOrOptions: Record<string, Provider> | UseDragSortOptions,
  appIdParam?: AppId,
) {
  // 兼容旧的调用方式
  const options: UseDragSortOptions =
    appIdParam !== undefined
      ? { providers: providersOrOptions as Record<string, Provider>, appId: appIdParam }
      : (providersOrOptions as UseDragSortOptions);

  const {
    providers,
    appId,
    displayedProviders,
    sortField,
    onSwitchToCustomSort,
  } = options;

  const queryClient = useQueryClient();
  const { t, i18n } = useTranslation();

  const sortedProviders = useMemo(() => {
    const locale = i18n.language === "zh" ? "zh-CN" : "en-US";
    return Object.values(providers).sort((a, b) => {
      if (a.sortIndex !== undefined && b.sortIndex !== undefined) {
        return a.sortIndex - b.sortIndex;
      }
      if (a.sortIndex !== undefined) return -1;
      if (b.sortIndex !== undefined) return 1;

      const timeA = a.createdAt ?? 0;
      const timeB = b.createdAt ?? 0;
      if (timeA && timeB && timeA !== timeB) {
        return timeA - timeB;
      }

      return a.name.localeCompare(b.name, locale);
    });
  }, [providers, i18n.language]);

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) {
        return;
      }

      // 如果当前不是自定义排序，使用显示的排序列表作为基础
      // 这样拖动后会保留当前显示顺序并应用拖动变化
      const baseList =
        sortField !== "custom" && displayedProviders
          ? displayedProviders
          : sortedProviders;

      const oldIndex = baseList.findIndex(
        (provider) => provider.id === active.id,
      );
      const newIndex = baseList.findIndex(
        (provider) => provider.id === over.id,
      );

      if (oldIndex === -1 || newIndex === -1) {
        return;
      }

      const reordered = arrayMove(baseList, oldIndex, newIndex);
      const updates = reordered.map((provider, index) => ({
        id: provider.id,
        sortIndex: index,
      }));

      try {
        await providersApi.updateSortOrder(updates, appId);
        await queryClient.invalidateQueries({
          queryKey: ["providers", appId],
        });

        // 刷新故障转移队列（因为队列顺序依赖 sort_index）
        await queryClient.invalidateQueries({
          queryKey: ["failoverQueue", appId],
        });

        // 更新托盘菜单以反映新的排序（失败不影响主操作）
        try {
          await providersApi.updateTrayMenu();
        } catch (trayError) {
          console.error("Failed to update tray menu after sort", trayError);
          // 托盘菜单更新失败不影响排序成功
        }

        // 如果当前不是自定义排序，自动切换到自定义排序模式
        if (sortField !== "custom" && onSwitchToCustomSort) {
          onSwitchToCustomSort();
        }

        toast.success(
          t("provider.sortUpdated", {
            defaultValue: "排序已更新",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("Failed to update provider sort order", error);
        toast.error(
          t("provider.sortUpdateFailed", {
            defaultValue: "排序更新失败",
          }),
        );
      }
    },
    [sortedProviders, displayedProviders, sortField, onSwitchToCustomSort, appId, queryClient, t],
  );

  return {
    sortedProviders,
    sensors,
    handleDragEnd,
  };
}
