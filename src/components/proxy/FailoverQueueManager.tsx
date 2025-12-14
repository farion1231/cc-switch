/**
 * 故障转移队列管理组件
 *
 * 允许用户管理代理模式下的故障转移队列，支持：
 * - 拖拽排序
 * - 添加/移除供应商
 * - 启用/禁用队列项
 */

import { useState, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { arrayMove, sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { toast } from "sonner";
import {
  GripVertical,
  Plus,
  Trash2,
  Loader2,
  Info,
  AlertTriangle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { FailoverQueueItem } from "@/types/proxy";
import type { AppId } from "@/lib/api";
import {
  useFailoverQueue,
  useAvailableProvidersForFailover,
  useAddToFailoverQueue,
  useRemoveFromFailoverQueue,
  useReorderFailoverQueue,
  useSetFailoverItemEnabled,
} from "@/lib/query/failover";

interface FailoverQueueManagerProps {
  appType: AppId;
  disabled?: boolean;
}

export function FailoverQueueManager({
  appType,
  disabled = false,
}: FailoverQueueManagerProps) {
  const { t } = useTranslation();
  const [selectedProviderId, setSelectedProviderId] = useState<string>("");

  // 查询数据
  const {
    data: queue,
    isLoading: isQueueLoading,
    error: queueError,
  } = useFailoverQueue(appType);
  const {
    data: availableProviders,
    isLoading: isProvidersLoading,
  } = useAvailableProvidersForFailover(appType);

  // Mutations
  const addToQueue = useAddToFailoverQueue();
  const removeFromQueue = useRemoveFromFailoverQueue();
  const reorderQueue = useReorderFailoverQueue();
  const setItemEnabled = useSetFailoverItemEnabled();

  // 拖拽配置
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  // 排序后的队列
  const sortedQueue = useMemo(() => {
    if (!queue) return [];
    return [...queue].sort((a, b) => a.queueOrder - b.queueOrder);
  }, [queue]);

  // 处理拖拽结束
  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id || !sortedQueue) return;

      const oldIndex = sortedQueue.findIndex(
        (item) => item.providerId === active.id,
      );
      const newIndex = sortedQueue.findIndex(
        (item) => item.providerId === over.id,
      );

      if (oldIndex === -1 || newIndex === -1) return;

      const reordered = arrayMove(sortedQueue, oldIndex, newIndex);
      const providerIds = reordered.map((item) => item.providerId);

      try {
        await reorderQueue.mutateAsync({ appType, providerIds });
        toast.success(
          t("proxy.failoverQueue.reorderSuccess", "队列顺序已更新"),
        );
      } catch (error) {
        toast.error(
          t("proxy.failoverQueue.reorderFailed", "更新顺序失败") +
            ": " +
            String(error),
        );
      }
    },
    [sortedQueue, appType, reorderQueue, t],
  );

  // 添加供应商到队列
  const handleAddProvider = async () => {
    if (!selectedProviderId) return;

    try {
      await addToQueue.mutateAsync({
        appType,
        providerId: selectedProviderId,
      });
      setSelectedProviderId("");
      toast.success(t("proxy.failoverQueue.addSuccess", "已添加到故障转移队列"));
    } catch (error) {
      toast.error(
        t("proxy.failoverQueue.addFailed", "添加失败") + ": " + String(error),
      );
    }
  };

  // 从队列移除供应商
  const handleRemoveProvider = async (providerId: string) => {
    try {
      await removeFromQueue.mutateAsync({ appType, providerId });
      toast.success(
        t("proxy.failoverQueue.removeSuccess", "已从故障转移队列移除"),
      );
    } catch (error) {
      toast.error(
        t("proxy.failoverQueue.removeFailed", "移除失败") + ": " + String(error),
      );
    }
  };

  // 切换启用状态
  const handleToggleEnabled = async (providerId: string, enabled: boolean) => {
    try {
      await setItemEnabled.mutateAsync({ appType, providerId, enabled });
    } catch (error) {
      toast.error(
        t("proxy.failoverQueue.toggleFailed", "状态更新失败") +
          ": " +
          String(error),
      );
    }
  };

  if (isQueueLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (queueError) {
    return (
      <Alert variant="destructive">
        <AlertTriangle className="h-4 w-4" />
        <AlertDescription>{String(queueError)}</AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-4">
      {/* 说明信息 */}
      <Alert className="border-blue-500/40 bg-blue-500/10">
        <Info className="h-4 w-4" />
        <AlertDescription className="text-sm">
          {t(
            "proxy.failoverQueue.info",
            "当前激活的供应商始终优先。当请求失败时，系统会按队列顺序依次尝试其他供应商。",
          )}
        </AlertDescription>
      </Alert>

      {/* 添加供应商 */}
      <div className="flex items-center gap-2">
        <Select
          value={selectedProviderId}
          onValueChange={setSelectedProviderId}
          disabled={disabled || isProvidersLoading}
        >
          <SelectTrigger className="flex-1">
            <SelectValue
              placeholder={t(
                "proxy.failoverQueue.selectProvider",
                "选择供应商添加到队列",
              )}
            />
          </SelectTrigger>
          <SelectContent>
            {availableProviders?.map((provider) => (
              <SelectItem key={provider.id} value={provider.id}>
                {provider.name}
              </SelectItem>
            ))}
            {(!availableProviders || availableProviders.length === 0) && (
              <div className="px-2 py-4 text-center text-sm text-muted-foreground">
                {t(
                  "proxy.failoverQueue.noAvailableProviders",
                  "没有可添加的供应商",
                )}
              </div>
            )}
          </SelectContent>
        </Select>
        <Button
          onClick={handleAddProvider}
          disabled={
            disabled || !selectedProviderId || addToQueue.isPending
          }
          size="icon"
          variant="outline"
        >
          {addToQueue.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Plus className="h-4 w-4" />
          )}
        </Button>
      </div>

      {/* 队列列表 */}
      {sortedQueue.length === 0 ? (
        <div className="rounded-lg border border-dashed border-muted-foreground/40 p-8 text-center">
          <p className="text-sm text-muted-foreground">
            {t(
              "proxy.failoverQueue.empty",
              "故障转移队列为空。添加供应商以启用自动故障转移。",
            )}
          </p>
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={handleDragEnd}
        >
          <SortableContext
            items={sortedQueue.map((item) => item.providerId)}
            strategy={verticalListSortingStrategy}
          >
            <div className="space-y-2">
              {sortedQueue.map((item, index) => (
                <SortableQueueItem
                  key={item.providerId}
                  item={item}
                  index={index}
                  disabled={disabled}
                  onToggleEnabled={handleToggleEnabled}
                  onRemove={handleRemoveProvider}
                  isRemoving={removeFromQueue.isPending}
                  isToggling={setItemEnabled.isPending}
                />
              ))}
            </div>
          </SortableContext>
        </DndContext>
      )}

      {/* 队列说明 */}
      {sortedQueue.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {t(
            "proxy.failoverQueue.dragHint",
            "拖拽供应商可调整故障转移顺序，序号越小优先级越高。",
          )}
        </p>
      )}
    </div>
  );
}

interface SortableQueueItemProps {
  item: FailoverQueueItem;
  index: number;
  disabled: boolean;
  onToggleEnabled: (providerId: string, enabled: boolean) => void;
  onRemove: (providerId: string) => void;
  isRemoving: boolean;
  isToggling: boolean;
}

function SortableQueueItem({
  item,
  index,
  disabled,
  onToggleEnabled,
  onRemove,
  isRemoving,
  isToggling,
}: SortableQueueItemProps) {
  const { t } = useTranslation();
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: item.providerId, disabled });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        "flex items-center gap-3 rounded-lg border bg-card p-3 transition-colors",
        isDragging && "opacity-50 shadow-lg",
        !item.enabled && "opacity-60",
      )}
    >
      {/* 拖拽手柄 */}
      <button
        type="button"
        className={cn(
          "cursor-grab touch-none text-muted-foreground hover:text-foreground",
          disabled && "cursor-not-allowed opacity-50",
        )}
        {...attributes}
        {...listeners}
        disabled={disabled}
        aria-label={t("provider.dragHandle", "拖拽排序")}
      >
        <GripVertical className="h-5 w-5" />
      </button>

      {/* 序号 */}
      <div className="flex h-6 w-6 items-center justify-center rounded-full bg-muted text-xs font-medium">
        {index + 1}
      </div>

      {/* 供应商名称 */}
      <div className="flex-1 min-w-0">
        <span
          className={cn(
            "text-sm font-medium truncate block",
            !item.enabled && "text-muted-foreground line-through",
          )}
        >
          {item.providerName}
        </span>
      </div>

      {/* 启用开关 */}
      <Switch
        checked={item.enabled}
        onCheckedChange={(checked) =>
          onToggleEnabled(item.providerId, checked)
        }
        disabled={disabled || isToggling}
        aria-label={t("proxy.failoverQueue.toggleEnabled", "启用/禁用")}
      />

      {/* 删除按钮 */}
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground hover:text-destructive"
        onClick={() => onRemove(item.providerId)}
        disabled={disabled || isRemoving}
        aria-label={t("common.delete", "删除")}
      >
        {isRemoving ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Trash2 className="h-4 w-4" />
        )}
      </Button>
    </div>
  );
}
