import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Loader2,
  Star,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { TooltipProvider } from "@/components/ui/tooltip";
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
  useSetSkillPin,
  type InstalledSkill,
  type SkillUpdateInfo,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { SKILLS_APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import { cn } from "@/lib/utils";
import { SkillsToolbar } from "./SkillsToolbar";
import { SkillsBulkActionBar } from "./SkillsBulkActionBar";
import {
  useSkillsFilterSort,
  LOCAL_SOURCE_KEY,
  ALL_GROUP_KEY,
} from "./useSkillsFilterSort";
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

  const { data: skills, isLoading } = useInstalledSkills();
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
  const setPinMutation = useSetSkillPin();
  const [isUpdatingAll, setIsUpdatingAll] = useState(false);
  const [isBulkWorking, setIsBulkWorking] = useState(false);

  const updatesMap = useMemo(() => {
    const map: Record<string, SkillUpdateInfo> = {};
    if (skillUpdates) {
      for (const u of skillUpdates) {
        map[u.id] = u;
      }
    }
    return map;
  }, [skillUpdates]);

  // Skills 工具栏：搜索/过滤/排序/分组/多选状态
  const toolbar = useSkillsFilterSort(skills ?? [], updatesMap, SKILLS_APP_IDS);

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

  const handleTogglePin = (skill: InstalledSkill) => {
    setPinMutation.mutate({ id: skill.id, pinned: !skill.pinnedAt });
  };

  const buildSkillKey = (skill: InstalledSkill): string => {
    const installName =
      skill.directory.split(/[/\\]/).pop()?.toLowerCase() ||
      skill.directory.toLowerCase();
    return `${installName}:${skill.repoOwner?.toLowerCase() || ""}:${skill.repoName?.toLowerCase() || ""}`;
  };

  const getSelectedSkills = (): InstalledSkill[] => {
    if (!skills) return [];
    return skills.filter((s) => toolbar.state.selectedIds.has(s.id));
  };

  const handleBulkUninstall = () => {
    const selected = getSelectedSkills();
    if (selected.length === 0) return;
    setConfirmDialog({
      isOpen: true,
      title: t("skills.bulk.confirmUninstallTitle"),
      message: t("skills.bulk.confirmUninstallMessage", {
        count: selected.length,
      }),
      variant: "destructive",
      confirmText: t("skills.uninstall"),
      onConfirm: async () => {
        setConfirmDialog(null);
        setIsBulkWorking(true);
        let okCount = 0;
        for (const skill of selected) {
          try {
            await uninstallMutation.mutateAsync({
              id: skill.id,
              skillKey: buildSkillKey(skill),
            });
            okCount++;
          } catch (error) {
            toast.error(t("skills.uninstallFailed"), {
              description: `${skill.name}: ${String(error)}`,
            });
          }
        }
        setIsBulkWorking(false);
        toolbar.exitSelectionMode();
        if (okCount > 0) {
          toast.success(t("skills.bulk.uninstallSuccess", { count: okCount }), {
            closeButton: true,
          });
        }
      },
    });
  };

  const handleBulkToggleApp = async (app: AppId, enabled: boolean) => {
    const selected = getSelectedSkills();
    if (selected.length === 0) return;
    setIsBulkWorking(true);
    let okCount = 0;
    for (const skill of selected) {
      try {
        await toggleAppMutation.mutateAsync({ id: skill.id, app, enabled });
        okCount++;
      } catch (error) {
        toast.error(t("common.error"), {
          description: `${skill.name}: ${String(error)}`,
        });
      }
    }
    setIsBulkWorking(false);
    if (okCount > 0) {
      toast.success(
        t(
          enabled ? "skills.bulk.enableSuccess" : "skills.bulk.disableSuccess",
          {
            count: okCount,
            app: APP_ICON_MAP[app]?.label ?? app,
          },
        ),
        { closeButton: true },
      );
    }
  };

  const handleBulkUpdateAvailable = async () => {
    const selected = getSelectedSkills().filter((s) => updatesMap[s.id]);
    if (selected.length === 0) return;
    setIsBulkWorking(true);
    let okCount = 0;
    for (const skill of selected) {
      try {
        await updateSkillMutation.mutateAsync(skill.id);
        okCount++;
      } catch (error) {
        toast.error(t("skills.updateFailed"), {
          description: `${skill.name}: ${String(error)}`,
        });
      }
    }
    setIsBulkWorking(false);
    toolbar.exitSelectionMode();
    if (okCount > 0) {
      toast.success(t("skills.updateAllSuccess", { count: okCount }), {
        closeButton: true,
      });
    }
  };

  const selectedHasUpdate = useMemo(() => {
    return getSelectedSkills().filter((s) => updatesMap[s.id]).length;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [toolbar.state.selectedIds, skills, updatesMap]);

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
    <div className="px-6 flex flex-col flex-1 min-h-0">
      {/* 粘性顶部：标题/计数 chips/更新操作 + 工具栏 + 多选条 */}
      <div className="sticky top-0 z-20 -mx-6 px-6 border-b border-border-default bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80">
        {/* 紧凑 header：标题 + App 计数 chips（点击切换过滤）+ 更新操作 */}
        <div className="flex items-center justify-between gap-3 py-3">
          <div className="flex items-center gap-2 flex-wrap min-w-0">
            <span className="text-sm font-medium whitespace-nowrap text-foreground">
              {t("skills.installed", { count: skills?.length || 0 })}
            </span>
            {SKILLS_APP_IDS.length > 0 && (
              <span className="h-4 w-px bg-border" />
            )}
            {SKILLS_APP_IDS.map((app) => {
              const active = toolbar.state.filterApps.has(app);
              const cfg = APP_ICON_MAP[app];
              return (
                <button
                  key={app}
                  type="button"
                  onClick={() => toolbar.toggleApp(app)}
                  aria-pressed={active}
                  className={cn(
                    "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    cfg.badgeClass,
                    active
                      ? "ring-2 ring-primary ring-offset-1 ring-offset-background"
                      : "opacity-80 hover:opacity-100",
                  )}
                >
                  <span className="opacity-75">{cfg.label}:</span>
                  <span className="font-bold">{enabledCounts[app] ?? 0}</span>
                </button>
              );
            })}
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            {skillUpdates && skillUpdates.length > 0 && (
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
                  : t("skills.updateAll", { count: skillUpdates.length })}
              </Button>
            )}
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

        {skills && skills.length > 0 && (
          <SkillsToolbar
            searchQuery={toolbar.state.searchQuery}
            onSearchChange={toolbar.setSearchQuery}
            sourceOptions={toolbar.sourceOptions}
            filterSources={toolbar.state.filterSources}
            onToggleSource={toolbar.toggleSource}
            filterUpdateOnly={toolbar.state.filterUpdateOnly}
            onToggleUpdateOnly={() =>
              toolbar.setFilterUpdateOnly(!toolbar.state.filterUpdateOnly)
            }
            updateAvailableCount={skillUpdates?.length ?? 0}
            sortKey={toolbar.state.sortKey}
            onSortChange={toolbar.setSortKey}
            groupKey={toolbar.state.groupKey}
            onGroupChange={toolbar.setGroupKey}
            selectionMode={toolbar.state.selectionMode}
            onToggleSelectionMode={() =>
              toolbar.state.selectionMode
                ? toolbar.exitSelectionMode()
                : toolbar.enterSelectionMode()
            }
            hasFilters={toolbar.hasFilters}
            onClearFilters={toolbar.clearFilters}
            total={toolbar.total}
            filteredCount={toolbar.filteredCount}
          />
        )}

        {/* 多选条：与 toolbar 同处 sticky 顶部组合，避免 top-offset 计算 */}
        {toolbar.state.selectionMode && (
          <SkillsBulkActionBar
            selectedCount={toolbar.state.selectedIds.size}
            selectedHasUpdate={selectedHasUpdate}
            appIds={SKILLS_APP_IDS}
            onUninstall={handleBulkUninstall}
            onToggleApp={handleBulkToggleApp}
            onUpdateAvailable={handleBulkUpdateAvailable}
            onCancel={toolbar.exitSelectionMode}
            onSelectAll={() => {
              if (
                toolbar.state.selectedIds.size === toolbar.filteredCount &&
                toolbar.filteredCount > 0
              ) {
                toolbar.clearSelection();
              } else {
                toolbar.selectAllVisible();
              }
            }}
            totalVisible={toolbar.filteredCount}
            isWorking={isBulkWorking}
          />
        )}
      </div>
      {/* 列表区（main 是 scroll container，此处不再独立滚动） */}
      <div className="pt-3 pb-24">
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
        ) : toolbar.filteredCount === 0 ? (
          <div className="text-center py-12">
            <p className="text-sm text-muted-foreground">
              {t("skills.noResults")}
            </p>
            {toolbar.hasFilters && (
              <Button
                type="button"
                variant="link"
                onClick={toolbar.clearFilters}
                className="mt-2 h-auto p-0 text-sm"
              >
                {t("skills.toolbar.clearFilters")}
              </Button>
            )}
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <div className="flex flex-col gap-3">
              {toolbar.groups.map((group) => {
                if (group.items.length === 0) return null;
                const showHeader =
                  toolbar.state.groupKey !== "none" &&
                  group.key !== ALL_GROUP_KEY;
                const collapsed = toolbar.state.collapsed.has(group.key);
                const groupLabel =
                  toolbar.state.groupKey === "app"
                    ? (APP_ICON_MAP[group.key as AppId]?.label ?? group.label)
                    : group.key === LOCAL_SOURCE_KEY
                      ? t("skills.local")
                      : group.label;
                return (
                  <div key={group.key}>
                    {showHeader && (
                      <button
                        type="button"
                        onClick={() => toolbar.toggleCollapsed(group.key)}
                        className="w-full flex items-center gap-1.5 px-2 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground"
                      >
                        {collapsed ? (
                          <ChevronRight size={14} />
                        ) : (
                          <ChevronDown size={14} />
                        )}
                        <span>{groupLabel}</span>
                        <span className="text-muted-foreground/60">
                          ({group.items.length})
                        </span>
                      </button>
                    )}
                    {!collapsed && (
                      <div className="rounded-xl border border-border-default overflow-hidden">
                        {group.items.map((skill, index) => (
                          <InstalledSkillListItem
                            key={`${group.key}:${skill.id}`}
                            skill={skill}
                            hasUpdate={!!updatesMap[skill.id]}
                            isUpdating={
                              updateSkillMutation.isPending &&
                              updateSkillMutation.variables === skill.id
                            }
                            onToggleApp={handleToggleApp}
                            onUninstall={() => handleUninstall(skill)}
                            onUpdate={() => handleUpdateSkill(skill)}
                            onTogglePin={() => handleTogglePin(skill)}
                            isLast={index === group.items.length - 1}
                            selectionMode={toolbar.state.selectionMode}
                            isSelected={toolbar.state.selectedIds.has(skill.id)}
                            onToggleSelect={() =>
                              toolbar.toggleSelected(skill.id)
                            }
                          />
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
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
  onTogglePin?: () => void;
  isLast?: boolean;
  selectionMode?: boolean;
  isSelected?: boolean;
  onToggleSelect?: () => void;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  hasUpdate,
  isUpdating,
  onToggleApp,
  onUninstall,
  onUpdate,
  onTogglePin,
  isLast,
  selectionMode,
  isSelected,
  onToggleSelect,
}) => {
  const { t } = useTranslation();
  const isPinned = !!skill.pinnedAt;

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
      {selectionMode ? (
        <Checkbox
          checked={!!isSelected}
          onCheckedChange={() => onToggleSelect?.()}
          aria-label={t("skills.bulk.select")}
          className="shrink-0"
        />
      ) : (
        <button
          type="button"
          onClick={onTogglePin}
          title={isPinned ? t("skills.pin.unpin") : t("skills.pin.pin")}
          className={cn(
            "shrink-0 grid place-content-center w-6 h-6 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-opacity",
            isPinned
              ? "opacity-100 text-amber-500 hover:text-amber-600"
              : "opacity-0 group-hover:opacity-100",
          )}
        >
          <Star size={14} className={isPinned ? "fill-current" : ""} />
        </button>
      )}

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

      <AppToggleGroup
        apps={skill.apps}
        onToggle={(app, enabled) => onToggleApp(skill.id, app, enabled)}
        appIds={SKILLS_APP_IDS}
      />

      {!selectionMode && (
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
      )}
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
    <Dialog
      open
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !isImporting) onClose();
      }}
    >
      <DialogContent
        className="max-w-2xl max-h-[85vh] flex flex-col"
        zIndex="alert"
      >
        <DialogHeader>
          <DialogTitle>{t("skills.import")}</DialogTitle>
          <DialogDescription>{t("skills.importDescription")}</DialogDescription>
        </DialogHeader>

        <TooltipProvider delayDuration={300}>
          <div className="flex-1 overflow-y-auto px-6 py-4 space-y-2">
            {skills.map((skill) => (
              <div
                key={skill.directory}
                className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted"
              >
                <Checkbox
                  checked={selected.has(skill.directory)}
                  onCheckedChange={() => toggleSelect(skill.directory)}
                  aria-label={skill.name}
                  className="mt-1 shrink-0"
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
        </TooltipProvider>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={isImporting}>
            {t("common.cancel")}
          </Button>
          <Button
            onClick={handleImport}
            disabled={selected.size === 0 || isImporting}
          >
            {t("skills.importSelected", { count: selected.size })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

export default UnifiedSkillsPanel;
