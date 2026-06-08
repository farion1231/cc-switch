import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Pencil, Trash2, GripVertical, Check, X } from "lucide-react";
import { toast } from "sonner";
import { CSS } from "@dnd-kit/utilities";
import { DndContext, closestCenter } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
  arrayMove,
} from "@dnd-kit/sortable";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogClose,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useSkillTags,
  useCreateTag,
  useUpdateTag,
  useDeleteTag,
  useReorderTags,
} from "@/hooks/useSkills";
import type { SkillTag } from "@/hooks/useSkills";

interface TagManagerDialogProps {
  open: boolean;
  onClose: () => void;
}

/** 可排序的标签行 */
function SortableTagRow({
  tag,
  editingId,
  editingName,
  onStartEdit,
  onCancelEdit,
  onEditNameChange,
  onSaveEdit,
  onDelete,
}: {
  tag: SkillTag;
  editingId: number | null;
  editingName: string;
  onStartEdit: (tag: SkillTag) => void;
  onCancelEdit: () => void;
  onEditNameChange: (name: string) => void;
  onSaveEdit: () => void;
  onDelete: (tag: SkillTag) => void;
}) {
  const { t } = useTranslation();
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: tag.id });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };

  const isEditing = editingId === tag.id;

  return (
    <div
      ref={setNodeRef}
      style={style}
      className="group flex min-h-12 items-center gap-3 rounded-md border border-transparent px-3 py-2 transition-colors hover:border-border-default hover:bg-muted/35"
    >
      {/* 拖拽手柄 */}
      <button
        className="cursor-grab touch-none text-muted-foreground/70 transition-colors hover:text-foreground active:cursor-grabbing"
        {...attributes}
        {...listeners}
      >
        <GripVertical className="h-4 w-4" aria-hidden="true" />
      </button>

      {/* 标签名 / 编辑输入框 */}
      <div className="flex-1 min-w-0">
        {isEditing ? (
          <Input
            value={editingName}
            onChange={(e) => onEditNameChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSaveEdit();
              if (e.key === "Escape") onCancelEdit();
            }}
            className="h-8 text-sm"
            autoFocus
          />
        ) : (
          <span className="block truncate text-sm font-medium">{tag.name}</span>
        )}
      </div>

      {/* 操作按钮 */}
      <div className="flex items-center gap-1 shrink-0">
        {isEditing ? (
          <>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 cursor-pointer"
              onClick={onSaveEdit}
              title={t("common.save")}
            >
              <Check className="h-3.5 w-3.5 text-green-500" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 cursor-pointer"
              onClick={onCancelEdit}
              title={t("common.cancel")}
            >
              <X className="h-3.5 w-3.5 text-muted-foreground" />
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 cursor-pointer opacity-70 transition-opacity group-hover:opacity-100"
              onClick={() => onStartEdit(tag)}
              title={t("skills.tags.rename")}
            >
              <Pencil className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 cursor-pointer opacity-70 transition-opacity hover:bg-red-500/10 hover:text-red-500 group-hover:opacity-100"
              onClick={() => onDelete(tag)}
              title={t("skills.tags.delete")}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </>
        )}
      </div>
    </div>
  );
}

export function TagManagerDialog({ open, onClose }: TagManagerDialogProps) {
  const { t } = useTranslation();
  const { data: tags } = useSkillTags();
  const createTag = useCreateTag();
  const updateTag = useUpdateTag();
  const deleteTag = useDeleteTag();
  const reorderTags = useReorderTags();

  // 新建标签输入
  const [newTagName, setNewTagName] = useState("");

  // 内联编辑状态
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingName, setEditingName] = useState("");

  // 删除确认
  const [deleteTarget, setDeleteTarget] = useState<SkillTag | null>(null);

  // dnd-kit 传感器
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  /** 创建标签 */
  const handleCreate = useCallback(async () => {
    const name = newTagName.trim();
    if (!name) return;

    try {
      await createTag.mutateAsync(name);
      setNewTagName("");
      toast.success(t("skills.tags.createSuccess"), { closeButton: true });
    } catch (error) {
      console.error("[TagManagerDialog] 创建标签失败", error);
      toast.error(t("skills.tags.createFailed"));
    }
  }, [newTagName, createTag, t]);

  /** 开始编辑 */
  const handleStartEdit = useCallback((tag: SkillTag) => {
    setEditingId(tag.id);
    setEditingName(tag.name);
  }, []);

  /** 取消编辑 */
  const handleCancelEdit = useCallback(() => {
    setEditingId(null);
    setEditingName("");
  }, []);

  /** 保存编辑 */
  const handleSaveEdit = useCallback(async () => {
    if (editingId === null) return;
    const name = editingName.trim();
    if (!name) return;

    try {
      await updateTag.mutateAsync({ id: editingId, name });
      setEditingId(null);
      setEditingName("");
      toast.success(t("skills.tags.renameSuccess"), { closeButton: true });
    } catch (error) {
      console.error("[TagManagerDialog] 重命名标签失败", error);
      toast.error(t("skills.tags.renameFailed"));
    }
  }, [editingId, editingName, updateTag, t]);

  /** 删除标签 */
  const handleDelete = useCallback(
    async (tag: SkillTag) => {
      try {
        await deleteTag.mutateAsync(tag.id);
        setDeleteTarget(null);
        toast.success(t("skills.tags.deleteSuccess"), { closeButton: true });
      } catch (error) {
        console.error("[TagManagerDialog] 删除标签失败", error);
        toast.error(t("skills.tags.deleteFailed"));
      }
    },
    [deleteTag, t],
  );

  /** 拖拽排序结束 */
  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id || !tags) return;

      const oldIndex = tags.findIndex((tag) => tag.id === active.id);
      const newIndex = tags.findIndex((tag) => tag.id === over.id);
      if (oldIndex === -1 || newIndex === -1) return;

      const reordered = arrayMove(tags, oldIndex, newIndex);
      const orderedIds = reordered.map((tag) => tag.id);

      try {
        await reorderTags.mutateAsync(orderedIds);
        toast.success(t("skills.tags.reorderSuccess"), { closeButton: true });
      } catch (error) {
        console.error("[TagManagerDialog] 标签排序失败", error);
        toast.error(t("skills.tags.reorderFailed"));
      }
    },
    [tags, reorderTags, t],
  );

  /** 新建输入回车提交 */
  const handleNewTagKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") handleCreate();
    },
    [handleCreate],
  );

  const sortedTags = tags
    ? [...tags].sort((a, b) => a.sort_index - b.sort_index)
    : [];

  return (
    <>
      <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
        <DialogContent
          className="gap-0 overflow-hidden p-0 sm:max-w-[520px]"
          zIndex="alert"
        >
          <DialogHeader className="border-b border-border-default bg-background px-6 py-4">
            <div className="flex items-center justify-between gap-4">
              <div className="flex min-w-0 items-center gap-3">
                <DialogTitle>{t("skills.tags.manage")}</DialogTitle>
                <span className="rounded-full border border-border-default bg-muted/40 px-2 py-0.5 text-xs text-muted-foreground">
                  {t("skills.tags.count", { count: sortedTags.length })}
                </span>
              </div>
              <DialogClose asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0 cursor-pointer"
                  title={t("common.close")}
                >
                  <X className="h-4 w-4" />
                </Button>
              </DialogClose>
            </div>
            <DialogDescription className="sr-only">
              {t("skills.tags.manage")}
            </DialogDescription>
          </DialogHeader>

          {/* 新建标签 */}
          <div className="border-b border-border-default bg-muted/10 px-6 py-4">
            <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
              <Input
                value={newTagName}
                onChange={(e) => setNewTagName(e.target.value)}
                onKeyDown={handleNewTagKeyDown}
                placeholder={t("skills.tags.tagNamePlaceholder")}
                className="h-10"
              />
              <Button
                onClick={handleCreate}
                disabled={!newTagName.trim() || createTag.isPending}
                className="h-10 px-4"
              >
                <Plus className="h-4 w-4" />
                {t("skills.tags.create")}
              </Button>
            </div>
          </div>

          {/* 标签列表 */}
          <div className="max-h-[52vh] overflow-y-auto p-3 [scrollbar-width:thin]">
            {sortedTags.length === 0 ? (
              <div className="py-8 text-center text-sm text-muted-foreground">
                {t("skills.tags.noTags")}
              </div>
            ) : (
              <DndContext
                sensors={sensors}
                collisionDetection={closestCenter}
                onDragEnd={handleDragEnd}
              >
                <SortableContext
                  items={sortedTags.map((tag) => tag.id)}
                  strategy={verticalListSortingStrategy}
                >
                  <div className="space-y-1">
                    {sortedTags.map((tag) => (
                      <SortableTagRow
                        key={tag.id}
                        tag={tag}
                        editingId={editingId}
                        editingName={editingName}
                        onStartEdit={handleStartEdit}
                        onCancelEdit={handleCancelEdit}
                        onEditNameChange={setEditingName}
                        onSaveEdit={handleSaveEdit}
                        onDelete={setDeleteTarget}
                      />
                    ))}
                  </div>
                </SortableContext>
              </DndContext>
            )}
          </div>
        </DialogContent>
      </Dialog>

      {/* 删除确认弹窗 */}
      <ConfirmDialog
        isOpen={!!deleteTarget}
        title={t("skills.tags.delete")}
        message={t("skills.tags.deleteConfirm", {
          name: deleteTarget?.name ?? "",
          defaultValue: `确定要删除标签「${deleteTarget?.name ?? ""}」吗？`,
        })}
        onConfirm={() => deleteTarget && void handleDelete(deleteTarget)}
        onCancel={() => setDeleteTarget(null)}
      />
    </>
  );
}
