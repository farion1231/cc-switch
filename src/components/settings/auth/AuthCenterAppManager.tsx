import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  ArrowDown,
  ArrowUp,
  GripVertical,
  SlidersHorizontal,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Switch } from "@/components/ui/switch";
import type { AuthCenterTarget } from "@/lib/api/auth";
import {
  AUTH_CENTER_APPS,
  DEFAULT_AUTH_CENTER_PREFERENCES,
  updateAuthCenterPreferences,
} from "@/lib/authCenterPreferences";

interface AuthCenterAppManagerProps {
  apps: AuthCenterTarget[];
  visibleApps: AuthCenterTarget[];
  onVisibilityChange: (id: AuthCenterTarget, visible: boolean) => void;
}

export function AuthCenterAppManager({
  apps,
  visibleApps,
  onVisibilityChange,
}: AuthCenterAppManagerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const isVisible = (id: AuthCenterTarget) => visibleApps.includes(id);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const moveApp = (id: AuthCenterTarget, target: AuthCenterTarget) => {
    updateAuthCenterPreferences((current) => {
      const order = current.customOrder ? current.order : apps;
      return {
        ...current,
        customOrder: true,
        order: arrayMove(order, order.indexOf(id), order.indexOf(target)),
      };
    });
  };
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    const id = apps.find((app) => app === active.id);
    const target = apps.find((app) => app === over?.id);
    if (id && target && id !== target) moveApp(id, target);
  };

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <div className="flex items-center justify-between gap-3">
        <span className="text-caption text-fg-2">
          {t("authCenter.apps.summary", {
            defaultValue: "已显示 {{visible}} / {{total}} 个应用",
            visible: apps.filter(isVisible).length,
            total: apps.length,
          })}
        </span>
        <CollapsibleTrigger asChild>
          <Button type="button" variant="neutral" size="compact">
            <SlidersHorizontal className="h-3.5 w-3.5" />
            {t("authCenter.apps.manage", { defaultValue: "管理应用" })}
          </Button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent>
        <div className="mt-3 rounded-panel border border-border bg-surface p-3">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <p className="m-0 text-caption text-fg-2">
              {t("authCenter.apps.description", {
                defaultValue:
                  "拖动调整顺序，关闭开关可隐藏应用。已有账号和授权不受影响，可随时重新开启。",
              })}
            </p>
            <Button
              type="button"
              variant="quiet"
              size="compact"
              onClick={() =>
                updateAuthCenterPreferences(
                  () => DEFAULT_AUTH_CENTER_PREFERENCES,
                )
              }
            >
              {t("authCenter.apps.reset", { defaultValue: "恢复默认" })}
            </Button>
          </div>
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={onDragEnd}
          >
            <SortableContext
              items={apps}
              strategy={verticalListSortingStrategy}
            >
              <div className="flex flex-col gap-1">
                {apps.map((id, index) => (
                  <SortableApp
                    key={id}
                    id={id}
                    visible={isVisible(id)}
                    onVisibilityChange={(visible) =>
                      onVisibilityChange(id, visible)
                    }
                    onMoveUp={
                      index > 0 ? () => moveApp(id, apps[index - 1]) : undefined
                    }
                    onMoveDown={
                      index < apps.length - 1
                        ? () => moveApp(id, apps[index + 1])
                        : undefined
                    }
                  />
                ))}
              </div>
            </SortableContext>
          </DndContext>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function SortableApp({
  id,
  visible,
  onVisibilityChange,
  onMoveUp,
  onMoveDown,
}: {
  id: AuthCenterTarget;
  visible: boolean;
  onVisibilityChange: (visible: boolean) => void;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
}) {
  const { t } = useTranslation();
  const name = AUTH_CENTER_APPS.find((app) => app.id === id)!.name;
  const {
    setNodeRef,
    setActivatorNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });

  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
        position: "relative",
        zIndex: isDragging ? 1 : undefined,
      }}
      className="flex items-center gap-2 rounded-control bg-surface px-1 py-1.5"
    >
      <Button
        ref={setActivatorNodeRef}
        type="button"
        variant="ghost"
        size="icon-compact"
        className="touch-none cursor-grab active:cursor-grabbing"
        {...attributes}
        {...listeners}
        aria-label={t("authCenter.apps.drag", {
          defaultValue: "拖动调整 {{name}} 的顺序",
          name,
        })}
      >
        <GripVertical className="h-4 w-4" />
      </Button>
      <span className="min-w-0 flex-1 truncate text-body text-fg-1">
        {name}
      </span>
      <Button
        type="button"
        variant="ghost"
        size="icon-compact"
        disabled={!onMoveUp}
        onClick={onMoveUp}
        aria-label={t("authCenter.apps.moveUp", {
          defaultValue: "{{name}} 上移一位",
          name,
        })}
      >
        <ArrowUp className="h-3.5 w-3.5" />
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="icon-compact"
        disabled={!onMoveDown}
        onClick={onMoveDown}
        aria-label={t("authCenter.apps.moveDown", {
          defaultValue: "{{name}} 下移一位",
          name,
        })}
      >
        <ArrowDown className="h-3.5 w-3.5" />
      </Button>
      <Switch
        checked={visible}
        onCheckedChange={onVisibilityChange}
        aria-label={t("authCenter.apps.show", {
          defaultValue: "显示 {{name}}",
          name,
        })}
      />
    </div>
  );
}
