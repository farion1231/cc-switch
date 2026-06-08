import React, {
  useMemo,
  useState,
  useCallback,
  useRef,
  useEffect,
} from "react";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Loader2,
  Tag,
  LayoutList,
  Layers,
  Settings2,
  ChevronDown,
  ChevronRight,
  GripVertical,
  Check,
  X,
  FolderOpen,
} from "lucide-react";
import {
  DndContext,
  DragOverlay,
  closestCenter,
  PointerSensor,
  useDroppable,
  useSensor,
  useSensors,
  type DragStartEvent,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
  arrayMove,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Input } from "@/components/ui/input";
import {
  type ImportSkillSelection,
  type SkillBackupEntry,
  useDeleteSkillBackup,
  useInstalledSkills,
  useSkillBackups,
  useRestoreSkillBackup,
  useToggleSkillApp,
  useUninstallSkill,
  useScanUnmanagedSkills,
  useImportSkillsFromApps,
  useInstallSkillsFromZip,
  useCheckSkillUpdates,
  useUpdateSkill,
  useSkillTags,
  useAllTagAssignments,
  useSetSkillTags,
  useUpdateTag,
  useReorderTags,
  type InstalledSkill,
  type SkillUpdateInfo,
  type SkillTag,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import { TagManagerDialog } from "@/components/skills/TagManagerDialog";
import { TagAssignPopover } from "@/components/skills/TagAssignPopover";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const UNTAGGED_GROUP_KEY = -1;
const UNTAGGED_DROP_ID = "drop-untagged";

export function getSkillDropTargetTagIds(
  overId: string,
  tagAssignments: [string, number][],
): number[] | null {
  if (overId === UNTAGGED_DROP_ID) {
    return [];
  }

  if (overId.startsWith("drop-group:")) {
    const tagId = parseInt(overId.replace("drop-group:", ""), 10);
    if (isNaN(tagId)) return null;
    return tagId === UNTAGGED_GROUP_KEY ? [] : [tagId];
  }

  if (overId.startsWith("group:")) {
    const tagId = parseInt(overId.replace("group:", ""), 10);
    if (isNaN(tagId)) return null;
    return tagId === UNTAGGED_GROUP_KEY ? [] : [tagId];
  }

  if (overId.startsWith("skill:")) {
    const overSkillId = overId.replace("skill:", "");
    const assignment = tagAssignments.find(([sid]) => sid === overSkillId);
    return assignment ? [assignment[1]] : [];
  }

  return null;
}

function hasSameTagAssignment(currentTagIds: number[], targetTagIds: number[]) {
  return (
    currentTagIds.length === targetTagIds.length &&
    targetTagIds.every((tagId) => currentTagIds.includes(tagId))
  );
}

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
}

export interface UnifiedSkillsPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
  openInstallFromZip: () => void;
  openRestoreFromBackup: () => void;
  checkUpdates: () => void;
}

function formatSkillBackupDate(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return Number.isNaN(date.getTime())
    ? String(unixSeconds)
    : date.toLocaleString();
}

const UnifiedSkillsPanel = React.forwardRef<
  UnifiedSkillsPanelHandle,
  UnifiedSkillsPanelProps
>(({ onOpenDiscovery, currentApp }, ref) => {
  const { t } = useTranslation();
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    confirmText?: string;
    variant?: "destructive" | "info";
    onConfirm: () => void;
  } | null>(null);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [restoreDialogOpen, setRestoreDialogOpen] = useState(false);
  const [viewMode, setViewMode] = useState<"list" | "grouped">("list");
  const [tagManagerOpen, setTagManagerOpen] = useState(false);
  const [collapsedTags, setCollapsedTags] = useState<Set<number>>(new Set());

  const { data: skills, isLoading } = useInstalledSkills();
  const { data: tags = [] } = useSkillTags();
  const { data: tagAssignments = [] } = useAllTagAssignments();
  const {
    data: skillBackups = [],
    refetch: refetchSkillBackups,
    isFetching: isFetchingSkillBackups,
  } = useSkillBackups();
  const deleteBackupMutation = useDeleteSkillBackup();
  const toggleAppMutation = useToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const restoreBackupMutation = useRestoreSkillBackup();
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills();
  const importMutation = useImportSkillsFromApps();
  const installFromZipMutation = useInstallSkillsFromZip();
  const {
    data: skillUpdates,
    refetch: checkUpdates,
    isFetching: isCheckingUpdates,
  } = useCheckSkillUpdates();
  const updateSkillMutation = useUpdateSkill();
  const setSkillTagsMutation = useSetSkillTags();
  const updateTagMutation = useUpdateTag();
  const reorderTagsMutation = useReorderTags();
  const [isUpdatingAll, setIsUpdatingAll] = useState(false);

  // 拖拽状态
  const [activeDragSkill, setActiveDragSkill] = useState<InstalledSkill | null>(
    null,
  );
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

  const updatesMap = useMemo(() => {
    const map: Record<string, SkillUpdateInfo> = {};
    if (skillUpdates) {
      for (const u of skillUpdates) {
        map[u.id] = u;
      }
    }
    return map;
  }, [skillUpdates]);

  const enabledCounts = useMemo(() => {
    const counts = {
      claude: 0,
      "claude-desktop": 0,
      codex: 0,
      gemini: 0,
      opencode: 0,
      openclaw: 0,
      hermes: 0,
    };
    if (!skills) return counts;
    skills.forEach((skill) => {
      for (const app of SKILLS_APP_IDS) {
        if (skill.apps[app]) counts[app]++;
      }
    });
    return counts;
  }, [skills]);

  // 按标签分组的 skills
  const groupedSkills = useMemo(() => {
    if (!skills) return [];
    const groups: {
      tag: SkillTag | null;
      tagId: number | null;
      skills: InstalledSkill[];
    }[] = [];
    const assignedSkillIds = new Set<string>();

    // 预构建 tagId -> Set<skillId> 映射，避免 O(n*m) 查找
    const tagSkillMap = new Map<number, Set<string>>();
    for (const [sid, tid] of tagAssignments) {
      let set = tagSkillMap.get(tid);
      if (!set) {
        set = new Set();
        tagSkillMap.set(tid, set);
      }
      set.add(sid);
    }

    // 按 tag 分组（保留空 tag 作为 drop target）
    for (const tag of tags) {
      const skillIdSet = tagSkillMap.get(tag.id);
      const groupSkills = skillIdSet
        ? skills.filter((s) => skillIdSet.has(s.id))
        : [];
      groups.push({ tag, tagId: tag.id, skills: groupSkills });
      groupSkills.forEach((s) => assignedSkillIds.add(s.id));
    }

    // 未分组
    const ungrouped = skills.filter((s) => !assignedSkillIds.has(s.id));
    if (ungrouped.length > 0) {
      groups.push({ tag: null, tagId: null, skills: ungrouped });
    }

    return groups;
  }, [skills, tags, tagAssignments]);

  const toggleTagCollapse = useCallback((tagId: number) => {
    setCollapsedTags((prev) => {
      const next = new Set(prev);
      if (next.has(tagId)) {
        next.delete(tagId);
      } else {
        next.add(tagId);
      }
      return next;
    });
  }, []);

  // 分组名称内联编辑状态
  const [editingTagId, setEditingTagId] = useState<number | null>(null);
  const [editingTagName, setEditingTagName] = useState("");
  const editInputRef = useRef<HTMLInputElement>(null!);

  useEffect(() => {
    if (editingTagId !== null && editInputRef.current) {
      editInputRef.current.focus();
      editInputRef.current.select();
    }
  }, [editingTagId]);

  const handleStartEditTag = useCallback((tag: SkillTag) => {
    setEditingTagId(tag.id);
    setEditingTagName(tag.name);
  }, []);

  const handleSaveEditTag = useCallback(async () => {
    if (editingTagId === null) return;
    const trimmed = editingTagName.trim();
    if (!trimmed) {
      setEditingTagId(null);
      return;
    }
    try {
      await updateTagMutation.mutateAsync({ id: editingTagId, name: trimmed });
      toast.success(t("skills.tags.renameSuccess"), { closeButton: true });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
    setEditingTagId(null);
  }, [editingTagId, editingTagName, updateTagMutation, t]);

  const handleCancelEditTag = useCallback(() => {
    setEditingTagId(null);
  }, []);

  // 拖拽开始
  const handleDragStart = useCallback(
    (event: DragStartEvent) => {
      const { active } = event;
      const activeId = active.id as string;
      // 判断是 skill 拖拽还是 group 拖拽
      if (activeId.startsWith("skill:")) {
        const skillId = activeId.replace("skill:", "");
        const skill = skills?.find((s) => s.id === skillId);
        if (skill) setActiveDragSkill(skill);
      }
    },
    [skills],
  );

  // 拖拽结束
  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      setActiveDragSkill(null);
      if (!over) return;

      const activeId = active.id as string;
      const overId = over.id as string;

      // 分组排序（group header 拖拽）
      if (
        activeId.startsWith("group:") &&
        overId.startsWith("group:") &&
        activeId !== overId
      ) {
        const activeTagId = parseInt(activeId.replace("group:", ""), 10);
        const overTagId = parseInt(overId.replace("group:", ""), 10);
        const tagIds = tags.map((t) => t.id);
        const oldIndex = tagIds.indexOf(activeTagId);
        const newIndex = tagIds.indexOf(overTagId);
        if (oldIndex !== -1 && newIndex !== -1) {
          const newOrder = arrayMove(tagIds, oldIndex, newIndex);
          try {
            await reorderTagsMutation.mutateAsync(newOrder);
          } catch (error) {
            toast.error(t("common.error"), { description: String(error) });
          }
        }
        return;
      }

      // 技能拖拽到不同分组
      if (activeId.startsWith("skill:")) {
        const skillId = activeId.replace("skill:", "");
        const targetTagIds = getSkillDropTargetTagIds(overId, tagAssignments);
        if (targetTagIds === null) return;

        // 获取该 skill 当前的标签
        const currentTagIds = tagAssignments
          .filter(([sid]) => sid === skillId)
          .map(([, tid]) => tid);

        // 如果已经在目标分组中，不处理
        if (hasSameTagAssignment(currentTagIds, targetTagIds)) return;

        // 分配新标签（替换现有分配）
        try {
          await setSkillTagsMutation.mutateAsync({
            skillId,
            tagIds: targetTagIds,
          });
          toast.success(t("skills.tags.moveSuccess"), { closeButton: true });
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        }
      }
    },
    [tags, tagAssignments, reorderTagsMutation, setSkillTagsMutation, t],
  );

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleUninstall = (skill: InstalledSkill) => {
    setConfirmDialog({
      isOpen: true,
      title: t("skills.uninstall"),
      message: t("skills.uninstallConfirm", { name: skill.name }),
      onConfirm: async () => {
        try {
          // 构建 skillKey 用于更新 discoverable 缓存
          const installName =
            skill.directory.split(/[/\\]/).pop()?.toLowerCase() ||
            skill.directory.toLowerCase();
          const skillKey = `${installName}:${skill.repoOwner?.toLowerCase() || ""}:${skill.repoName?.toLowerCase() || ""}`;

          const result = await uninstallMutation.mutateAsync({
            id: skill.id,
            skillKey,
          });
          setConfirmDialog(null);
          toast.success(t("skills.uninstallSuccess", { name: skill.name }), {
            description: result.backupPath
              ? t("skills.backup.location", { path: result.backupPath })
              : undefined,
            closeButton: true,
          });
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        }
      },
    });
  };

  const handleOpenImport = async () => {
    try {
      const result = await scanUnmanaged();
      if (!result.data || result.data.length === 0) {
        toast.success(t("skills.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleImport = async (imports: ImportSkillSelection[]) => {
    try {
      const imported = await importMutation.mutateAsync(imports);
      setImportDialogOpen(false);
      toast.success(t("skills.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleInstallFromZip = async () => {
    try {
      const filePath = await skillsApi.openZipFileDialog();
      if (!filePath) return;

      const installed = await installFromZipMutation.mutateAsync({
        filePath,
        currentApp,
      });

      if (installed.length === 0) {
        toast.info(t("skills.installFromZip.noSkillsFound"), {
          closeButton: true,
        });
      } else if (installed.length === 1) {
        toast.success(
          t("skills.installFromZip.successSingle", { name: installed[0].name }),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("skills.installFromZip.successMultiple", {
            count: installed.length,
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      toast.error(t("skills.installFailed"), { description: String(error) });
    }
  };

  const handleCheckUpdates = async () => {
    try {
      const result = await checkUpdates();
      const updates = result.data || [];
      if (updates.length === 0) {
        toast.success(t("skills.noUpdates"), { closeButton: true });
      } else {
        toast.info(t("skills.updatesFound", { count: updates.length }), {
          closeButton: true,
        });
      }
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleUpdateSkill = async (skill: InstalledSkill) => {
    try {
      const updated = await updateSkillMutation.mutateAsync(skill.id);
      toast.success(t("skills.updateSuccess", { name: updated.name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("skills.updateFailed"), { description: String(error) });
    }
  };

  const handleUpdateAll = async () => {
    if (!skillUpdates || skillUpdates.length === 0) return;
    setIsUpdatingAll(true);
    let successCount = 0;
    for (const update of skillUpdates) {
      try {
        await updateSkillMutation.mutateAsync(update.id);
        successCount++;
      } catch (error) {
        toast.error(t("skills.updateFailed"), {
          description: `${update.name}: ${String(error)}`,
        });
      }
    }
    setIsUpdatingAll(false);
    if (successCount > 0) {
      toast.success(t("skills.updateAllSuccess", { count: successCount }), {
        closeButton: true,
      });
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    setRestoreDialogOpen(true);
    try {
      await refetchSkillBackups();
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleRestoreFromBackup = async (backupId: string) => {
    try {
      const restored = await restoreBackupMutation.mutateAsync({
        backupId,
        currentApp,
      });
      setRestoreDialogOpen(false);
      toast.success(
        t("skills.restoreFromBackup.success", { name: restored.name }),
        {
          closeButton: true,
        },
      );
    } catch (error) {
      toast.error(t("skills.restoreFromBackup.failed"), {
        description: String(error),
      });
    }
  };

  const handleDeleteBackup = (backup: SkillBackupEntry) => {
    setConfirmDialog({
      isOpen: true,
      title: t("skills.restoreFromBackup.deleteConfirmTitle"),
      message: t("skills.restoreFromBackup.deleteConfirmMessage", {
        name: backup.skill.name,
      }),
      confirmText: t("skills.restoreFromBackup.delete"),
      variant: "destructive",
      onConfirm: async () => {
        try {
          await deleteBackupMutation.mutateAsync(backup.backupId);
          await refetchSkillBackups();
          setConfirmDialog(null);
          toast.success(
            t("skills.restoreFromBackup.deleteSuccess", {
              name: backup.skill.name,
            }),
            {
              closeButton: true,
            },
          );
        } catch (error) {
          toast.error(t("skills.restoreFromBackup.deleteFailed"), {
            description: String(error),
          });
        }
      },
    });
  };

  React.useImperativeHandle(ref, () => ({
    openDiscovery: onOpenDiscovery,
    openImport: handleOpenImport,
    openInstallFromZip: handleInstallFromZip,
    openRestoreFromBackup: handleOpenRestoreFromBackup,
    checkUpdates: handleCheckUpdates,
  }));

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <div className="flex items-center justify-between">
        <AppCountBar
          totalLabel={t("skills.installed", { count: skills?.length || 0 })}
          counts={enabledCounts}
          appIds={SKILLS_APP_IDS}
        />
        <div className="flex items-center gap-1.5">
          {/* 视图切换 */}
          <div className="flex items-center rounded-md border border-border-default">
            <Button
              type="button"
              variant={viewMode === "list" ? "secondary" : "ghost"}
              size="sm"
              className="h-7 px-2 text-xs rounded-r-none"
              onClick={() => setViewMode("list")}
              title={t("skills.tags.listView")}
            >
              <LayoutList size={12} />
            </Button>
            <Button
              type="button"
              variant={viewMode === "grouped" ? "secondary" : "ghost"}
              size="sm"
              className="h-7 px-2 text-xs rounded-l-none"
              onClick={() => setViewMode("grouped")}
              title={t("skills.tags.groupedView")}
            >
              <Layers size={12} />
            </Button>
          </div>
          {/* 标签管理入口 */}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={() => setTagManagerOpen(true)}
          >
            <Tag size={12} />
            <Settings2 size={10} />
          </Button>
          <div
            className="transition-all duration-300 ease-out overflow-hidden"
            style={{
              maxWidth:
                skillUpdates && skillUpdates.length > 0 ? "200px" : "0px",
              opacity: skillUpdates && skillUpdates.length > 0 ? 1 : 0,
            }}
          >
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 text-xs gap-1 whitespace-nowrap"
              onClick={handleUpdateAll}
              disabled={isUpdatingAll || updateSkillMutation.isPending}
            >
              {isUpdatingAll ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <RefreshCw size={12} />
              )}
              {isUpdatingAll
                ? t("skills.updatingAll")
                : t("skills.updateAll", { count: skillUpdates?.length ?? 0 })}
            </Button>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={handleCheckUpdates}
            disabled={isCheckingUpdates || !skills || skills.length === 0}
          >
            {isCheckingUpdates ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <RefreshCw size={12} />
            )}
            {isCheckingUpdates
              ? t("skills.checkingUpdates")
              : t("skills.checkUpdates")}
          </Button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
        {isLoading ? (
          <div className="text-center py-12 text-muted-foreground">
            {t("skills.loading")}
          </div>
        ) : !skills || skills.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Sparkles size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("skills.noInstalled")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("skills.noInstalledDescription")}
            </p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            {viewMode === "grouped" && groupedSkills.length > 0 ? (
              /* 分组视图（支持拖拽排序 + 技能拖拽分组 + 内联编辑） */
              <DndContext
                sensors={sensors}
                collisionDetection={closestCenter}
                onDragStart={handleDragStart}
                onDragEnd={handleDragEnd}
              >
                <SortableContext
                  items={groupedSkills
                    .filter((g) => g.tagId !== null)
                    .map((g) => `group:${g.tagId}`)}
                  strategy={verticalListSortingStrategy}
                >
                  <div className="space-y-3">
                    {groupedSkills.map((group) => {
                      const tagKey = group.tagId ?? UNTAGGED_GROUP_KEY;
                      const isCollapsed = collapsedTags.has(tagKey);
                      return (
                        <DroppableGroup
                          key={tagKey}
                          tagId={group.tagId}
                          isUntagged={group.tagId === null}
                        >
                          {group.tagId !== null ? (
                            <SortableGroupHeader
                              tag={group.tag}
                              tagId={group.tagId}
                              skillCount={group.skills.length}
                              isCollapsed={isCollapsed}
                              isEditing={editingTagId === group.tagId}
                              editingName={editingTagName}
                              editInputRef={editInputRef}
                              onToggleCollapse={() =>
                                toggleTagCollapse(group.tagId!)
                              }
                              onStartEdit={() =>
                                group.tag && handleStartEditTag(group.tag)
                              }
                              onEditingNameChange={setEditingTagName}
                              onSaveEdit={handleSaveEditTag}
                              onCancelEdit={handleCancelEditTag}
                            />
                          ) : (
                            <StaticGroupHeader
                              tag={group.tag}
                              tagId={group.tagId}
                              skillCount={group.skills.length}
                              isCollapsed={isCollapsed}
                              isEditing={false}
                              editingName={editingTagName}
                              editInputRef={editInputRef}
                              onToggleCollapse={() => {}}
                              onStartEdit={() => {}}
                              onEditingNameChange={setEditingTagName}
                              onSaveEdit={handleSaveEditTag}
                              onCancelEdit={handleCancelEditTag}
                            />
                          )}
                          {!isCollapsed && (
                            <div>
                              {group.skills.map((skill, index) => (
                                <DraggableSkillRow key={skill.id} skill={skill}>
                                  <InstalledSkillListItem
                                    skill={skill}
                                    hasUpdate={!!updatesMap[skill.id]}
                                    isUpdating={
                                      updateSkillMutation.isPending &&
                                      updateSkillMutation.variables === skill.id
                                    }
                                    onToggleApp={handleToggleApp}
                                    onUninstall={() => handleUninstall(skill)}
                                    onUpdate={() => handleUpdateSkill(skill)}
                                    isLast={index === group.skills.length - 1}
                                  />
                                </DraggableSkillRow>
                              ))}
                            </div>
                          )}
                        </DroppableGroup>
                      );
                    })}
                  </div>
                </SortableContext>
                <DragOverlay>
                  {activeDragSkill && (
                    <div className="rounded-lg border border-primary bg-background px-4 py-2 shadow-lg opacity-90">
                      <span className="text-sm font-medium">
                        {activeDragSkill.name}
                      </span>
                    </div>
                  )}
                </DragOverlay>
              </DndContext>
            ) : (
              /* 列表视图 */
              <div className="rounded-xl border border-border-default overflow-hidden">
                {skills.map((skill, index) => (
                  <InstalledSkillListItem
                    key={skill.id}
                    skill={skill}
                    hasUpdate={!!updatesMap[skill.id]}
                    isUpdating={
                      updateSkillMutation.isPending &&
                      updateSkillMutation.variables === skill.id
                    }
                    onToggleApp={handleToggleApp}
                    onUninstall={() => handleUninstall(skill)}
                    onUpdate={() => handleUpdateSkill(skill)}
                    isLast={index === skills.length - 1}
                  />
                ))}
              </div>
            )}
          </TooltipProvider>
        )}
      </div>

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          confirmText={confirmDialog.confirmText}
          variant={confirmDialog.variant}
          zIndex="top"
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}

      {importDialogOpen && unmanagedSkills && (
        <ImportSkillsDialog
          skills={unmanagedSkills}
          isImporting={importMutation.isPending}
          onImport={handleImport}
          onClose={() => setImportDialogOpen(false)}
        />
      )}

      {/* 标签管理弹窗 */}
      {tagManagerOpen && (
        <TagManagerDialog
          open={tagManagerOpen}
          onClose={() => setTagManagerOpen(false)}
        />
      )}

      <RestoreSkillsDialog
        backups={skillBackups}
        isDeleting={deleteBackupMutation.isPending}
        isLoading={isFetchingSkillBackups}
        onDelete={handleDeleteBackup}
        isRestoring={restoreBackupMutation.isPending}
        onRestore={handleRestoreFromBackup}
        onClose={() => setRestoreDialogOpen(false)}
        open={restoreDialogOpen}
      />
    </div>
  );
});

UnifiedSkillsPanel.displayName = "UnifiedSkillsPanel";

interface InstalledSkillListItemProps {
  skill: InstalledSkill;
  hasUpdate?: boolean;
  isUpdating?: boolean;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  onUpdate?: () => void;
  isLast?: boolean;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  hasUpdate,
  isUpdating,
  onToggleApp,
  onUninstall,
  onUpdate,
  isLast,
}) => {
  const { t } = useTranslation();

  const openDocs = async () => {
    if (!skill.readmeUrl) return;
    try {
      await settingsApi.openExternal(skill.readmeUrl);
    } catch {
      // ignore
    }
  };

  const openInExplorer = async () => {
    try {
      await skillsApi.openDirectory(skill.directory);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const sourceLabel = useMemo(() => {
    if (skill.repoOwner && skill.repoName) {
      return `${skill.repoOwner}/${skill.repoName}`;
    }
    return t("skills.local");
  }, [skill.repoOwner, skill.repoName, t]);

  return (
    <ListItemRow isLast={isLast}>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="font-medium text-sm text-foreground truncate">
            {skill.name}
          </span>
          {skill.readmeUrl && (
            <button
              type="button"
              onClick={openDocs}
              className="text-muted-foreground/60 hover:text-foreground flex-shrink-0"
            >
              <ExternalLink size={12} />
            </button>
          )}
          <button
            type="button"
            onClick={openInExplorer}
            className="text-muted-foreground/60 hover:text-foreground flex-shrink-0"
            title={t("skills.openDirectory")}
          >
            <FolderOpen size={12} />
          </button>
          <TagAssignPopover skillId={skill.id} />
          <span className="text-xs text-muted-foreground/50 flex-shrink-0">
            {sourceLabel}
          </span>
          {hasUpdate && (
            <Badge
              variant="outline"
              className="shrink-0 text-[10px] px-1.5 py-0 h-4 border-amber-500 text-amber-600 dark:text-amber-400"
            >
              {t("skills.updateAvailable")}
            </Badge>
          )}
        </div>
        {skill.description && (
          <p
            className="text-xs text-muted-foreground truncate"
            title={skill.description}
          >
            {skill.description}
          </p>
        )}
      </div>

      <AppToggleGroup
        apps={skill.apps}
        onToggle={(app, enabled) => onToggleApp(skill.id, app, enabled)}
        appIds={SKILLS_APP_IDS}
      />

      <div
        className="flex-shrink-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity"
        style={hasUpdate ? { opacity: 1 } : undefined}
      >
        {hasUpdate && onUpdate && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 hover:text-blue-500 hover:bg-blue-100 dark:hover:text-blue-400 dark:hover:bg-blue-500/10"
            onClick={onUpdate}
            disabled={isUpdating}
            title={t("skills.update")}
          >
            {isUpdating ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <RefreshCw size={14} />
            )}
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          onClick={onUninstall}
          title={t("skills.uninstall")}
        >
          <Trash2 size={14} />
        </Button>
      </div>
    </ListItemRow>
  );
};

interface ImportSkillsDialogProps {
  skills: Array<{
    directory: string;
    name: string;
    description?: string;
    foundIn: string[];
    path: string;
  }>;
  isImporting: boolean;
  onImport: (imports: ImportSkillSelection[]) => void;
  onClose: () => void;
}

interface RestoreSkillsDialogProps {
  backups: SkillBackupEntry[];
  isDeleting: boolean;
  isLoading: boolean;
  isRestoring: boolean;
  onDelete: (backup: SkillBackupEntry) => void;
  onRestore: (backupId: string) => void;
  onClose: () => void;
  open: boolean;
}

const RestoreSkillsDialog: React.FC<RestoreSkillsDialogProps> = ({
  backups,
  isDeleting,
  isLoading,
  isRestoring,
  onDelete,
  onRestore,
  onClose,
  open,
}) => {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent
        className="max-w-2xl max-h-[85vh] flex flex-col"
        zIndex="alert"
      >
        <DialogHeader>
          <DialogTitle>{t("skills.restoreFromBackup.title")}</DialogTitle>
          <DialogDescription>
            {t("skills.restoreFromBackup.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4">
          {isLoading ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("common.loading")}
            </div>
          ) : backups.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("skills.restoreFromBackup.empty")}
            </div>
          ) : (
            <div className="space-y-3">
              {backups.map((backup) => (
                <div
                  key={backup.backupId}
                  className="rounded-xl border border-border-default bg-background/70 p-4 shadow-sm"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <div className="font-medium text-sm text-foreground">
                          {backup.skill.name}
                        </div>
                        <div className="rounded-md bg-muted px-2 py-0.5 text-[11px] text-muted-foreground">
                          {backup.skill.directory}
                        </div>
                      </div>
                      {backup.skill.description && (
                        <div className="mt-2 text-sm text-muted-foreground">
                          {backup.skill.description}
                        </div>
                      )}
                      <div className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                        <div>
                          {t("skills.restoreFromBackup.createdAt")}:{" "}
                          {formatSkillBackupDate(backup.createdAt)}
                        </div>
                        <div className="break-all" title={backup.backupPath}>
                          {t("skills.restoreFromBackup.path")}:{" "}
                          {backup.backupPath}
                        </div>
                      </div>
                    </div>

                    <div className="flex flex-col gap-2 sm:min-w-28">
                      <Button
                        type="button"
                        variant="outline"
                        onClick={() => onRestore(backup.backupId)}
                        disabled={isRestoring || isDeleting}
                      >
                        {isRestoring
                          ? t("skills.restoreFromBackup.restoring")
                          : t("skills.restoreFromBackup.restore")}
                      </Button>
                      <Button
                        type="button"
                        variant="destructive"
                        onClick={() => onDelete(backup)}
                        disabled={isRestoring || isDeleting}
                      >
                        {isDeleting
                          ? t("skills.restoreFromBackup.deleting")
                          : t("skills.restoreFromBackup.delete")}
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

const ImportSkillsDialog: React.FC<ImportSkillsDialogProps> = ({
  skills,
  isImporting,
  onImport,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    new Set(skills.map((s) => s.directory)),
  );
  const [selectedApps, setSelectedApps] = useState<
    Record<string, ImportSkillSelection["apps"]>
  >(() =>
    Object.fromEntries(
      skills.map((skill) => [
        skill.directory,
        {
          claude: skill.foundIn.includes("claude"),
          codex: skill.foundIn.includes("codex"),
          gemini: skill.foundIn.includes("gemini"),
          opencode: skill.foundIn.includes("opencode"),
          openclaw: false,
          hermes: skill.foundIn.includes("hermes"),
        },
      ]),
    ),
  );

  const toggleSelect = (directory: string) => {
    const newSelected = new Set(selected);
    if (newSelected.has(directory)) {
      newSelected.delete(directory);
    } else {
      newSelected.add(directory);
    }
    setSelected(newSelected);
  };

  const handleImport = () => {
    onImport(
      Array.from(selected).map((directory) => ({
        directory,
        apps: selectedApps[directory] ?? {
          claude: false,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      })),
    );
  };

  return (
    <TooltipProvider delayDuration={300}>
      <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
        <div className="bg-background rounded-xl p-6 max-w-lg w-full mx-4 shadow-xl max-h-[80vh] flex flex-col">
          <h2 className="text-lg font-semibold mb-2">{t("skills.import")}</h2>
          <p className="text-sm text-muted-foreground mb-4">
            {t("skills.importDescription")}
          </p>

          <div className="flex-1 overflow-y-auto space-y-2 mb-4">
            {skills.map((skill) => (
              <div
                key={skill.directory}
                className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted"
              >
                <input
                  type="checkbox"
                  checked={selected.has(skill.directory)}
                  onChange={() => toggleSelect(skill.directory)}
                  className="mt-1"
                />
                <div className="flex-1 min-w-0">
                  <div className="font-medium">{skill.name}</div>
                  {skill.description && (
                    <div className="text-sm text-muted-foreground line-clamp-1">
                      {skill.description}
                    </div>
                  )}
                  <div className="mt-2">
                    <AppToggleGroup
                      apps={
                        selectedApps[skill.directory] ?? {
                          claude: false,
                          codex: false,
                          gemini: false,
                          opencode: false,
                          openclaw: false,
                          hermes: false,
                        }
                      }
                      onToggle={(app, enabled) => {
                        setSelectedApps((prev) => ({
                          ...prev,
                          [skill.directory]: {
                            ...(prev[skill.directory] ?? {
                              claude: false,
                              codex: false,
                              gemini: false,
                              opencode: false,
                              openclaw: false,
                              hermes: false,
                            }),
                            [app]: enabled,
                          },
                        }));
                      }}
                      appIds={SKILLS_APP_IDS}
                    />
                  </div>
                  <div
                    className="text-xs text-muted-foreground/50 mt-1 truncate"
                    title={skill.path}
                  >
                    {skill.path}
                  </div>
                </div>
              </div>
            ))}
          </div>

          <div className="flex justify-end gap-3">
            <Button variant="outline" onClick={onClose} disabled={isImporting}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={handleImport}
              disabled={selected.size === 0 || isImporting}
            >
              {t("skills.importSelected", { count: selected.size })}
            </Button>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
};

// ========== 分组视图辅助组件 ==========

/** 可放置技能的分组容器 */
const DroppableGroup: React.FC<{
  tagId: number | null;
  isUntagged: boolean;
  children: React.ReactNode;
}> = ({ tagId, isUntagged, children }) => {
  if (isUntagged) {
    return <UntaggedDroppableGroup>{children}</UntaggedDroppableGroup>;
  }

  return (
    <SortableDroppableGroup tagId={tagId!}>{children}</SortableDroppableGroup>
  );
};

/** 可放置的未分组容器 */
const UntaggedDroppableGroup: React.FC<{
  children: React.ReactNode;
}> = ({ children }) => {
  const { setNodeRef, isOver } = useDroppable({
    id: UNTAGGED_DROP_ID,
  });

  return (
    <div
      ref={setNodeRef}
      className={`rounded-xl border overflow-hidden transition-colors ${
        isOver ? "border-primary/60 bg-primary/5" : "border-border-default"
      }`}
      data-group-id="untagged"
    >
      {children}
    </div>
  );
};

/** 可放置的分组容器 */
const SortableDroppableGroup: React.FC<{
  tagId: number;
  children: React.ReactNode;
}> = ({ tagId, children }) => {
  const { setNodeRef, isOver } = useDroppable({
    id: `drop-group:${tagId}`,
  });

  return (
    <div
      ref={setNodeRef}
      className={`rounded-xl border overflow-hidden transition-colors ${
        isOver ? "border-primary/60 bg-primary/5" : "border-border-default"
      }`}
      data-group-id={tagId}
    >
      {children}
    </div>
  );
};

/** 分组头部 Props */
interface GroupHeaderProps {
  tag: SkillTag | null;
  tagId: number | null;
  skillCount: number;
  isCollapsed: boolean;
  isEditing: boolean;
  editingName: string;
  editInputRef: React.RefObject<HTMLInputElement>;
  onToggleCollapse: () => void;
  onStartEdit: () => void;
  onEditingNameChange: (name: string) => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
}

/** 分组头部内容（编辑/显示逻辑） */
const GroupHeaderContent: React.FC<GroupHeaderProps> = ({
  tag,
  tagId,
  skillCount,
  isCollapsed,
  isEditing,
  editingName,
  editInputRef,
  onToggleCollapse,
  onStartEdit,
  onEditingNameChange,
  onSaveEdit,
  onCancelEdit,
}) => {
  const { t } = useTranslation();
  const isCancellingRef = useRef(false);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onSaveEdit();
    } else if (e.key === "Escape") {
      isCancellingRef.current = true;
      onCancelEdit();
    }
  };

  const handleBlur = () => {
    // 延迟检查，让 mousedown 事件先触发
    requestAnimationFrame(() => {
      if (!isCancellingRef.current) {
        onSaveEdit();
      }
      isCancellingRef.current = false;
    });
  };

  const handleCancelMouseDown = (e: React.MouseEvent) => {
    // 阻止 Input 失焦
    e.preventDefault();
    isCancellingRef.current = true;
    onCancelEdit();
  };

  return (
    <>
      {/* 展开/折叠 */}
      <button
        type="button"
        className="flex-shrink-0"
        onClick={onToggleCollapse}
      >
        {tagId !== null ? (
          isCollapsed ? (
            <ChevronRight size={14} />
          ) : (
            <ChevronDown size={14} />
          )
        ) : (
          <span className="w-[14px]" />
        )}
      </button>

      <Tag size={14} className="text-muted-foreground flex-shrink-0" />

      {/* 分组名称：显示态 vs 编辑态 */}
      {isEditing ? (
        <div className="flex items-center gap-1 flex-1 min-w-0">
          <Input
            ref={editInputRef}
            value={editingName}
            onChange={(e) => onEditingNameChange(e.target.value)}
            onKeyDown={handleKeyDown}
            onBlur={handleBlur}
            className="h-6 text-sm py-0 px-1"
          />
          <button
            type="button"
            onClick={onSaveEdit}
            className="text-green-500 hover:text-green-600 flex-shrink-0"
          >
            <Check size={14} />
          </button>
          <button
            type="button"
            onMouseDown={handleCancelMouseDown}
            className="text-muted-foreground hover:text-foreground flex-shrink-0"
          >
            <X size={14} />
          </button>
        </div>
      ) : (
        <span
          className="text-sm font-medium truncate"
          onDoubleClick={tagId !== null ? onStartEdit : undefined}
          title={tag ? t("skills.tags.doubleClickToEdit") : undefined}
        >
          {tag ? tag.name : t("skills.tags.untagged")}
        </span>
      )}

      <Badge
        variant="secondary"
        className="ml-auto text-[10px] px-1.5 py-0 h-4 flex-shrink-0"
      >
        {skillCount}
      </Badge>
    </>
  );
};

/** 可排序的分组头部（有 tagId，支持拖拽排序） */
const SortableGroupHeader: React.FC<GroupHeaderProps> = ({
  tagId,
  ...props
}) => {
  const {
    setNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: `group:${tagId}` });

  return (
    <div
      ref={setNodeRef}
      className="flex items-center gap-2 px-4 py-2 bg-muted/50 hover:bg-muted/70 transition-colors"
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.5 : 1,
      }}
    >
      {/* 拖拽手柄 */}
      <button
        type="button"
        className="cursor-grab active:cursor-grabbing text-muted-foreground/40 hover:text-muted-foreground flex-shrink-0"
        {...attributes}
        {...listeners}
      >
        <GripVertical size={14} />
      </button>
      <GroupHeaderContent tagId={tagId} {...props} />
    </div>
  );
};

/** 不可排序的分组头部（未分组区域） */
const StaticGroupHeader: React.FC<GroupHeaderProps> = ({ tagId, ...props }) => {
  return (
    <div className="flex items-center gap-2 px-4 py-2 bg-muted/50 hover:bg-muted/70 transition-colors">
      <GroupHeaderContent tagId={tagId} {...props} />
    </div>
  );
};

/** 可拖拽的技能行 */
const DraggableSkillRow: React.FC<{
  skill: InstalledSkill;
  children: React.ReactNode;
}> = ({ skill, children }) => {
  const { attributes, listeners, setNodeRef, isDragging } = useSortable({
    id: `skill:${skill.id}`,
  });

  return (
    <div
      ref={setNodeRef}
      className="relative group"
      style={{ opacity: isDragging ? 0.4 : 1 }}
    >
      {/* 拖拽手柄覆盖在技能行左侧 */}
      <div
        className="absolute left-0 top-0 bottom-0 w-6 flex items-center justify-center cursor-grab active:cursor-grabbing opacity-0 group-hover:opacity-100 transition-opacity z-10"
        {...attributes}
        {...listeners}
      >
        <GripVertical size={12} className="text-muted-foreground/50" />
      </div>
      <div className="pl-5">{children}</div>
    </div>
  );
};

export default UnifiedSkillsPanel;
