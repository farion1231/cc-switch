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
  DialogFooter,
  DialogHeader,
  DialogTitle,
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
      className="flex items-center gap-2 rounded-md border border-border/40 bg-background px-3 py-2"
    >
      {/* 拖拽手柄 */}
      <button
        className="cursor-grab touch-none text-muted-foreground hover:text-foreground"
        {...attributes}
        {...listeners}
      >
        <GripVertical className="h-4 w-4" />
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
            className="h-7 text-sm"
            autoFocus
          />
        ) : (
          <span className="text-sm truncate block">{tag.name}</span>
        )}
      </div>

      {/* 操作按钮 */}
      <div className="flex items-center gap-1 shrink-0">
        {isEditing ? (
          <>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={onSaveEdit}
              title={t("common.save")}
            >
              <Check className="h-3.5 w-3.5 text-green-500" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
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
              className="h-7 w-7"
              onClick={() => onStartEdit(tag)}
              title={t("skills.tags.rename")}
            >
              <Pencil className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 text-destructive hover:text-destructive"
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
      toast.success(
        t("skills.tags.createSuccess", {
          defaultValue: "标签创建成功",
        }),
        { closeButton: true },
      );
    } catch (error) {
      console.error("[TagManagerDialog] 创建标签失败", error);
      toast.error(
        t("skills.tags.createFailed", {
          defaultValue: "标签创建失败",
        }),
      );
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
      toast.success(
        t("skills.tags.renameSuccess", {
          defaultValue: "标签重命名成功",
        }),
        { closeButton: true },
      );
    } catch (error) {
      console.error("[TagManagerDialog] 重命名标签失败", error);
      toast.error(
        t("skills.tags.renameFailed", {
          defaultValue: "标签重命名失败",
        }),
      );
    }
  }, [editingId, editingName, updateTag, t]);

  /** 删除标签 */
  const handleDelete = useCallback(
    async (tag: SkillTag) => {
      try {
        await deleteTag.mutateAsync(tag.id);
        setDeleteTarget(null);
        toast.success(
          t("skills.tags.deleteSuccess", {
            defaultValue: "标签删除成功",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("[TagManagerDialog] 删除标签失败", error);
        toast.error(
          t("skills.tags.deleteFailed", {
            defaultValue: "标签删除失败",
          }),
        );
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
        toast.success(
          t("skills.tags.reorderSuccess", {
            defaultValue: "排序已更新",
          }),
          { closeButton: true },
        );
      } catch (error) {
        console.error("[TagManagerDialog] 标签排序失败", error);
        toast.error(
          t("skills.tags.reorderFailed", {
            defaultValue: "排序更新失败",
          }),
        );
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
        <DialogContent className="sm:max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>{t("skills.tags.manage")}</DialogTitle>
            <DialogDescription className="sr-only">
              {t("skills.tags.manage")}
            </DialogDescription>
          </DialogHeader>

          {/* 标签列表 */}
          <div className="max-h-[50vh] overflow-y-auto [scrollbar-width:thin] space-y-2 py-2">
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
                  <div className="space-y-2">
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

          {/* 新建标签 */}
          <div className="flex items-center gap-2 pt-2 border-t">
            <Input
              value={newTagName}
              onChange={(e) => setNewTagName(e.target.value)}
              onKeyDown={handleNewTagKeyDown}
              placeholder={t("skills.tags.tagNamePlaceholder")}
              className="flex-1"
            />
            <Button
              onClick={handleCreate}
              disabled={!newTagName.trim() || createTag.isPending}
              size="sm"
            >
              <Plus className="h-4 w-4 mr-1" />
              {t("skills.tags.create")}
            </Button>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={onClose}>
              {t("common.close")}
            </Button>
          </DialogFooter>
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
