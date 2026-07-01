import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Loader2,
  FolderTree,
  Plus,
  Pencil,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  type ImportSkillSelection,
  type SkillBackupEntry,
  useDeleteSkillBackup,
  useInstalledSkills,
  useSkillGroups,
  useCreateSkillGroup,
  useRenameSkillGroup,
  useDeleteSkillGroup,
  useSetSkillGroupMembers,
  useBatchToggleSkillGroupApp,
  useSkillBackups,
  useRestoreSkillBackup,
  useToggleSkillApp,
  useUninstallSkill,
  useScanUnmanagedSkills,
  useImportSkillsFromApps,
  useInstallSkillsFromZip,
  useCheckSkillUpdates,
  useUpdateSkill,
  type InstalledSkill,
  type SkillUpdateInfo,
} from "@/hooks/useSkills";
import type { SkillAppId, SkillGroup } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { APP_ICON_MAP, SKILLS_APP_IDS } from "@/config/appConfig";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
}

type SkillGroupView = "all" | "source" | "manual";

const MANUAL_UNGROUPED_GROUP_ID = "manual:ungrouped";
const RIGHT_ALIGNED_APP_TOGGLE_SLOT_CLASS =
  "w-[164px] flex-shrink-0 flex items-center justify-end transition-transform";
const HOVER_ACTION_SLOT_CLASS =
  "absolute right-4 top-1/2 flex -translate-y-1/2 items-center gap-0.5 transition-opacity";
const SINGLE_ACTION_HOVER_SHIFT_CLASS = "group-hover:-translate-x-[40px]";
const DOUBLE_ACTION_SHIFT_CLASS = "-translate-x-[68px]";

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

function getSkillGroupDisplayName(
  t: ReturnType<typeof useTranslation>["t"],
  group: SkillGroup,
): string {
  if (group.id === MANUAL_UNGROUPED_GROUP_ID) {
    return t("skills.groups.auto.ungrouped");
  }

  if (group.kind !== "source") {
    return group.name;
  }

  if (group.id === "source:local") {
    return t("skills.groups.auto.local");
  }

  return group.name;
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
  const [selectedFilterApps, setSelectedFilterApps] = useState<Set<AppId>>(
    () => new Set(SKILLS_APP_IDS),
  );
  const [groupView, setGroupView] = useState<SkillGroupView>("all");
  const [collapsedGroupIds, setCollapsedGroupIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [memberDialogGroup, setMemberDialogGroup] = useState<SkillGroup | null>(
    null,
  );

  const { data: skills, isLoading } = useInstalledSkills();
  const { data: skillGroups = [], isLoading: isLoadingGroups } = useSkillGroups(
    groupView !== "all",
  );
  const {
    data: skillBackups = [],
    refetch: refetchSkillBackups,
    isFetching: isFetchingSkillBackups,
  } = useSkillBackups();
  const deleteBackupMutation = useDeleteSkillBackup();
  const toggleAppMutation = useToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const restoreBackupMutation = useRestoreSkillBackup();
  // enabled: true —— 进入 Skill 页面时自动静默扫描一次（绿点提示来源）
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills({ enabled: true });
  const importMutation = useImportSkillsFromApps();
  const installFromZipMutation = useInstallSkillsFromZip();
  const createGroupMutation = useCreateSkillGroup();
  const renameGroupMutation = useRenameSkillGroup();
  const deleteGroupMutation = useDeleteSkillGroup();
  const setGroupMembersMutation = useSetSkillGroupMembers();
  const batchToggleGroupMutation = useBatchToggleSkillGroupApp();
  const {
    data: skillUpdates,
    refetch: checkUpdates,
    isFetching: isCheckingUpdates,
  } = useCheckSkillUpdates();
  const updateSkillMutation = useUpdateSkill();
  const [isUpdatingAll, setIsUpdatingAll] = useState(false);

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

  const allFilterAppsSelected = useMemo(
    () => SKILLS_APP_IDS.every((app) => selectedFilterApps.has(app)),
    [selectedFilterApps],
  );

  const filteredSkills = useMemo(() => {
    if (!skills) return [];
    if (allFilterAppsSelected) return skills;
    if (selectedFilterApps.size === 0) return [];
    return skills.filter((skill) =>
      SKILLS_APP_IDS.some(
        (app) => selectedFilterApps.has(app) && Boolean(skill.apps[app]),
      ),
    );
  }, [allFilterAppsSelected, skills, selectedFilterApps]);

  const skillById = useMemo(() => {
    const map = new Map<string, InstalledSkill>();
    for (const skill of skills ?? []) {
      map.set(skill.id, skill);
    }
    return map;
  }, [skills]);

  const filteredSkillIds = useMemo(
    () => new Set(filteredSkills.map((skill) => skill.id)),
    [filteredSkills],
  );

  const visibleGroups = useMemo(() => {
    if (groupView === "all") return [];
    return skillGroups
      .filter((group) => group.kind === groupView)
      .map((group) => {
        const allSkills = group.memberSkillIds
          .map((id) => skillById.get(id))
          .filter((skill): skill is InstalledSkill => Boolean(skill));
        return {
          group,
          allSkills,
          skills: allSkills.filter(
            (skill) => allFilterAppsSelected || filteredSkillIds.has(skill.id),
          ),
        };
      })
      .filter(
        ({ group, allSkills }) =>
          allSkills.length > 0 || (group.kind === "manual" && group.editable),
      );
  }, [
    allFilterAppsSelected,
    filteredSkillIds,
    groupView,
    skillById,
    skillGroups,
  ]);

  const installedCountLabel = allFilterAppsSelected
    ? t("skills.installed", { count: skills?.length || 0 })
    : t("skills.installedFiltered", {
        count: filteredSkills.length,
        total: skills?.length || 0,
      });

  const handleToggleFilterApp = (app: AppId) => {
    setSelectedFilterApps((prev) => {
      const next = new Set(prev);
      if (next.has(app)) {
        next.delete(app);
      } else {
        next.add(app);
      }
      return next;
    });
  };

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleCreateGroup = async () => {
    const name = window.prompt(t("skills.groups.createPrompt"));
    const trimmedName = name?.trim();
    if (!trimmedName) return;
    try {
      await createGroupMutation.mutateAsync({
        name: trimmedName,
        skillIds: [],
      });
      setGroupView("manual");
      toast.success(t("skills.groups.createSuccess", { name: trimmedName }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleRenameGroup = async (group: SkillGroup) => {
    const name = window.prompt(t("skills.groups.renamePrompt"), group.name);
    const trimmedName = name?.trim();
    if (!trimmedName || trimmedName === group.name) return;
    try {
      await renameGroupMutation.mutateAsync({
        groupId: group.id,
        name: trimmedName,
      });
      toast.success(t("skills.groups.renameSuccess", { name: trimmedName }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleDeleteGroup = (group: SkillGroup) => {
    setConfirmDialog({
      isOpen: true,
      title: t("skills.groups.delete"),
      message: t("skills.groups.deleteConfirm", { name: group.name }),
      confirmText: t("skills.groups.delete"),
      variant: "destructive",
      onConfirm: async () => {
        try {
          await deleteGroupMutation.mutateAsync(group.id);
          setConfirmDialog(null);
          toast.success(
            t("skills.groups.deleteSuccess", { name: group.name }),
            {
              closeButton: true,
            },
          );
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        }
      },
    });
  };

  const handleSaveGroupMembers = async (
    group: SkillGroup,
    skillIds: string[],
  ) => {
    try {
      await setGroupMembersMutation.mutateAsync({
        groupId: group.id,
        skillIds,
      });
      setMemberDialogGroup(null);
      toast.success(t("skills.groups.membersSaved", { name: group.name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleBatchToggleGroupApp = async (
    group: SkillGroup,
    app: SkillAppId,
    enabled: boolean,
    skillIds: string[],
  ) => {
    try {
      const count = await batchToggleGroupMutation.mutateAsync({
        groupId: group.id,
        app,
        enabled,
        skillIds,
      });
      toast.success(
        t(
          enabled
            ? "skills.groups.enableSuccess"
            : "skills.groups.disableSuccess",
          {
            count,
            app: APP_ICON_MAP[app]?.label ?? app,
          },
        ),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleToggleGroupCollapsed = (groupId: string) => {
    setCollapsedGroupIds((prev) => {
      const next = new Set(prev);
      if (next.has(groupId)) {
        next.delete(groupId);
      } else {
        next.add(groupId);
      }
      return next;
    });
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
          totalLabel={installedCountLabel}
          counts={enabledCounts}
          appIds={SKILLS_APP_IDS}
          selectedApps={selectedFilterApps}
          onToggleApp={handleToggleFilterApp}
        />
        <div className="flex items-center gap-1.5">
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

      <div className="mt-3 mb-3 flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-1.5">
          {(["all", "source", "manual"] as SkillGroupView[]).map((view) => (
            <Button
              key={view}
              type="button"
              variant={groupView === view ? "secondary" : "ghost"}
              size="sm"
              className="h-7 text-xs gap-1"
              onClick={() => setGroupView(view)}
            >
              {view !== "all" && <FolderTree size={12} />}
              {t(`skills.groups.views.${view}`)}
            </Button>
          ))}
          {isLoadingGroups && (
            <Loader2 size={14} className="animate-spin text-muted-foreground" />
          )}
        </div>
        <div className="flex items-center gap-2">
          {groupView === "manual" && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 text-xs gap-1"
              onClick={handleCreateGroup}
              disabled={createGroupMutation.isPending}
            >
              <Plus size={12} />
              {t("skills.groups.create")}
            </Button>
          )}
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
        ) : groupView !== "all" ? (
          isLoadingGroups && skillGroups.length === 0 ? (
            <div className="text-center py-12 text-muted-foreground">
              {t("skills.loading")}
            </div>
          ) : visibleGroups.length === 0 ? (
            <div className="text-center py-12">
              <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
                <FolderTree size={24} className="text-muted-foreground" />
              </div>
              <h3 className="text-lg font-medium text-foreground mb-2">
                {t("skills.groups.noResults")}
              </h3>
              <p className="text-muted-foreground text-sm">
                {t("skills.groups.noResultsDescription")}
              </p>
            </div>
          ) : (
            <TooltipProvider delayDuration={300}>
              <div className="space-y-3">
                {visibleGroups.map(({ group, skills: groupSkills }) => (
                  <SkillGroupSection
                    key={group.id}
                    group={group}
                    skills={groupSkills}
                    updatesMap={updatesMap}
                    isCollapsed={collapsedGroupIds.has(group.id)}
                    isBulkPending={batchToggleGroupMutation.isPending}
                    isUpdatingSkillId={
                      updateSkillMutation.isPending
                        ? updateSkillMutation.variables
                        : undefined
                    }
                    onToggleCollapsed={handleToggleGroupCollapsed}
                    onBatchToggle={handleBatchToggleGroupApp}
                    onEditMembers={setMemberDialogGroup}
                    onRename={handleRenameGroup}
                    onDelete={handleDeleteGroup}
                    onToggleApp={handleToggleApp}
                    onUninstall={handleUninstall}
                    onUpdate={handleUpdateSkill}
                  />
                ))}
              </div>
            </TooltipProvider>
          )
        ) : filteredSkills.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Sparkles size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("skills.appFilter.noResults")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("skills.appFilter.noResultsDescription")}
            </p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <div className="rounded-xl border border-border-default overflow-hidden">
              {filteredSkills.map((skill, index) => (
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
                  isLast={index === filteredSkills.length - 1}
                />
              ))}
            </div>
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

      {memberDialogGroup && skills && (
        <SkillGroupMembersDialog
          group={memberDialogGroup}
          skills={skills}
          isSaving={setGroupMembersMutation.isPending}
          onSave={handleSaveGroupMembers}
          onClose={() => setMemberDialogGroup(null)}
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

interface SkillGroupSectionProps {
  group: SkillGroup;
  skills: InstalledSkill[];
  updatesMap: Record<string, SkillUpdateInfo>;
  isCollapsed: boolean;
  isBulkPending: boolean;
  isUpdatingSkillId?: string;
  onToggleCollapsed: (groupId: string) => void;
  onBatchToggle: (
    group: SkillGroup,
    app: SkillAppId,
    enabled: boolean,
    skillIds: string[],
  ) => void;
  onEditMembers: (group: SkillGroup) => void;
  onRename: (group: SkillGroup) => void;
  onDelete: (group: SkillGroup) => void;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: (skill: InstalledSkill) => void;
  onUpdate: (skill: InstalledSkill) => void;
}

const SkillGroupSection: React.FC<SkillGroupSectionProps> = ({
  group,
  skills,
  updatesMap,
  isCollapsed,
  isBulkPending,
  isUpdatingSkillId,
  onToggleCollapsed,
  onBatchToggle,
  onEditMembers,
  onRename,
  onDelete,
  onToggleApp,
  onUninstall,
  onUpdate,
}) => {
  const { t } = useTranslation();
  const groupName = getSkillGroupDisplayName(t, group);

  return (
    <div className="rounded-xl border border-border-default overflow-hidden bg-background/60">
      <div
        className={`group relative flex items-center gap-3 px-4 py-2.5 bg-muted/30 ${
          isCollapsed ? "" : "border-b border-border-default"
        }`}
      >
        <div className="min-w-0 flex flex-1 items-center gap-2">
          <button
            type="button"
            className="min-w-0 flex flex-1 items-center gap-2 text-left"
            onClick={() => onToggleCollapsed(group.id)}
            aria-expanded={!isCollapsed}
          >
            {isCollapsed ? (
              <ChevronRight
                size={14}
                className="text-muted-foreground shrink-0"
              />
            ) : (
              <ChevronDown
                size={14}
                className="text-muted-foreground shrink-0"
              />
            )}
            <FolderTree size={14} className="text-muted-foreground shrink-0" />
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="font-medium text-sm truncate">
                  {groupName}
                </span>
                <Badge variant="outline" className="h-5 text-[10px]">
                  {t(`skills.groups.kinds.${group.kind}`)}
                </Badge>
                <span className="text-xs text-muted-foreground">
                  {skills.length}/{group.count}
                </span>
              </div>
            </div>
          </button>

          {group.editable && (
            <div
              className={`flex items-center gap-1 flex-shrink-0 transition-transform ${SINGLE_ACTION_HOVER_SHIFT_CLASS}`}
            >
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 text-xs"
                onClick={() => onEditMembers(group)}
              >
                {t("skills.groups.members")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={() => onRename(group)}
                title={t("skills.groups.rename")}
              >
                <Pencil size={13} />
              </Button>
            </div>
          )}
        </div>

        <div
          className={`${RIGHT_ALIGNED_APP_TOGGLE_SLOT_CLASS} ${
            group.editable ? SINGLE_ACTION_HOVER_SHIFT_CLASS : ""
          }`}
          data-testid="group-toggle-slot"
          data-group-id={group.id}
        >
          <GroupAppToggleBar
            group={group}
            skills={skills}
            isPending={isBulkPending}
            onBatchToggle={onBatchToggle}
          />
        </div>
        <div
          className={`${HOVER_ACTION_SLOT_CLASS} ${
            group.editable
              ? "pointer-events-none opacity-0 group-hover:pointer-events-auto group-hover:opacity-100"
              : "pointer-events-none opacity-0"
          }`}
          data-testid="group-action-slot"
          data-group-id={group.id}
        >
          {group.editable && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 hover:text-red-500"
              onClick={() => onDelete(group)}
              title={t("skills.groups.delete")}
            >
              <Trash2 size={13} />
            </Button>
          )}
        </div>
      </div>

      {isCollapsed ? null : skills.length === 0 ? (
        <div className="px-4 py-6 text-center text-sm text-muted-foreground">
          {t("skills.groups.empty")}
        </div>
      ) : (
        skills.map((skill, index) => (
          <InstalledSkillListItem
            key={skill.id}
            skill={skill}
            hasUpdate={!!updatesMap[skill.id]}
            isUpdating={isUpdatingSkillId === skill.id}
            onToggleApp={onToggleApp}
            onUninstall={() => onUninstall(skill)}
            onUpdate={() => onUpdate(skill)}
            isLast={index === skills.length - 1}
          />
        ))
      )}
    </div>
  );
};

type GroupAppState = "enabled" | "disabled" | "mixed";

interface GroupAppToggleBarProps {
  group: SkillGroup;
  skills: InstalledSkill[];
  isPending: boolean;
  onBatchToggle: (
    group: SkillGroup,
    app: SkillAppId,
    enabled: boolean,
    skillIds: string[],
  ) => void;
}

const getGroupAppState = (
  skills: InstalledSkill[],
  app: SkillAppId,
): GroupAppState => {
  if (skills.length === 0) return "disabled";
  const enabledCount = skills.filter((skill) =>
    Boolean(skill.apps[app]),
  ).length;
  if (enabledCount === 0) return "disabled";
  if (enabledCount === skills.length) return "enabled";
  return "mixed";
};

const getGroupAppButtonClassName = (
  state: GroupAppState,
  activeClass: string,
): string => {
  const baseClass =
    "relative w-7 h-7 rounded-lg flex items-center justify-center transition-all disabled:cursor-not-allowed disabled:opacity-45";

  switch (state) {
    case "enabled":
      return `${baseClass} ${activeClass}`;
    case "mixed":
      return `${baseClass} ${activeClass} opacity-90 ring-2 ring-yellow-400/70 ring-offset-1 ring-offset-background`;
    case "disabled":
      return `${baseClass} opacity-35 hover:opacity-70`;
  }
};

const GroupAppToggleBar: React.FC<GroupAppToggleBarProps> = ({
  group,
  skills,
  isPending,
  onBatchToggle,
}) => {
  const { t } = useTranslation();
  const skillAppIds = SKILLS_APP_IDS as SkillAppId[];

  return (
    <div className="flex items-center gap-1.5 flex-shrink-0">
      {skillAppIds.map((app) => {
        const { label, icon, activeClass } = APP_ICON_MAP[app];
        const state = getGroupAppState(skills, app);
        const nextEnabled = state !== "enabled";
        const stateLabel = t(`skills.groups.appStates.${state}`);

        return (
          <Tooltip key={app}>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() =>
                  onBatchToggle(
                    group,
                    app,
                    nextEnabled,
                    skills.map((skill) => skill.id),
                  )
                }
                disabled={isPending || skills.length === 0}
                aria-label={t("skills.groups.toggleAppForGroup", {
                  app: label,
                  state: stateLabel,
                })}
                className={getGroupAppButtonClassName(state, activeClass)}
              >
                {icon}
                {state === "mixed" && (
                  <span className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full bg-yellow-400 ring-1 ring-background" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">
              <p>
                {label} · {stateLabel}
              </p>
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
};

interface SkillGroupMembersDialogProps {
  group: SkillGroup;
  skills: InstalledSkill[];
  isSaving: boolean;
  onSave: (group: SkillGroup, skillIds: string[]) => void;
  onClose: () => void;
}

const SkillGroupMembersDialog: React.FC<SkillGroupMembersDialogProps> = ({
  group,
  skills,
  isSaving,
  onSave,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selectedIds, setSelectedIds] = useState<Set<string>>(
    () => new Set(group.memberSkillIds),
  );
  const [query, setQuery] = useState("");

  const visibleSkills = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return skills;
    return skills.filter((skill) =>
      [skill.name, skill.description, skill.directory]
        .filter((value): value is string => Boolean(value))
        .some((value) => value.toLowerCase().includes(normalized)),
    );
  }, [query, skills]);
  const visibleSkillIds = useMemo(
    () => visibleSkills.map((skill) => skill.id),
    [visibleSkills],
  );
  const allVisibleSkillsSelected =
    visibleSkillIds.length > 0 &&
    visibleSkillIds.every((id) => selectedIds.has(id));

  const toggleSkill = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const toggleAllSkillsSelected = () => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (allVisibleSkillsSelected) {
        for (const id of visibleSkillIds) {
          next.delete(id);
        }
      } else {
        for (const id of visibleSkillIds) {
          next.add(id);
        }
      }
      return next;
    });
  };

  return (
    <Dialog open onOpenChange={(nextOpen) => !nextOpen && onClose()}>
      <DialogContent
        className="max-w-2xl max-h-[85vh] flex flex-col"
        zIndex="alert"
      >
        <DialogHeader>
          <DialogTitle>{t("skills.groups.editMembersTitle")}</DialogTitle>
          <DialogDescription>
            {t("skills.groups.editMembersDescription", { name: group.name })}
          </DialogDescription>
        </DialogHeader>

        <div className="px-6 pb-3">
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("skills.groups.searchMembers")}
          />
        </div>

        <div className="flex-1 overflow-y-auto px-6 pb-4 space-y-2">
          {visibleSkills.map((skill) => (
            <label
              key={skill.id}
              className="flex items-start gap-3 rounded-lg border border-border-default p-3 hover:bg-muted/50"
            >
              <input
                type="checkbox"
                checked={selectedIds.has(skill.id)}
                onChange={() => toggleSkill(skill.id)}
                className="mt-1"
              />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium truncate">{skill.name}</div>
                {skill.description && (
                  <div className="text-xs text-muted-foreground truncate">
                    {skill.description}
                  </div>
                )}
                <div className="text-[11px] text-muted-foreground/60">
                  {skill.directory}
                </div>
              </div>
            </label>
          ))}
        </div>

        <DialogFooter className="sm:justify-between">
          <Button
            type="button"
            variant="outline"
            onClick={toggleAllSkillsSelected}
            disabled={visibleSkillIds.length === 0 || isSaving}
          >
            {allVisibleSkillsSelected
              ? t("skills.groups.clearAll")
              : t("skills.groups.selectAll")}
          </Button>
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={isSaving}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              onClick={() => onSave(group, Array.from(selectedIds))}
              disabled={isSaving}
            >
              {t("skills.groups.saveMembers", { count: selectedIds.size })}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

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
  const actionButtonsAlwaysVisible = Boolean(hasUpdate);
  const toggleShiftClass = actionButtonsAlwaysVisible
    ? DOUBLE_ACTION_SHIFT_CLASS
    : SINGLE_ACTION_HOVER_SHIFT_CLASS;

  const openDocs = async () => {
    if (!skill.readmeUrl) return;
    try {
      await settingsApi.openExternal(skill.readmeUrl);
    } catch {
      // ignore
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

      <div
        className={`${RIGHT_ALIGNED_APP_TOGGLE_SLOT_CLASS} ${toggleShiftClass}`}
        data-testid="skill-toggle-slot"
        data-skill-id={skill.id}
      >
        <AppToggleGroup
          apps={skill.apps}
          onToggle={(app, enabled) => onToggleApp(skill.id, app, enabled)}
          appIds={SKILLS_APP_IDS}
        />
      </div>

      <div
        className={`${HOVER_ACTION_SLOT_CLASS} ${
          actionButtonsAlwaysVisible
            ? "opacity-100"
            : "pointer-events-none opacity-0 group-hover:pointer-events-auto group-hover:opacity-100"
        }`}
        data-testid="skill-action-slot"
        data-skill-id={skill.id}
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

export default UnifiedSkillsPanel;
