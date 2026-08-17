import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Sparkles,
  Trash2,
  ExternalLink,
  RefreshCw,
  Loader2,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  type ImportSkillSelection,
  type SkillBackupEntry,
  useDeleteSkillBackup,
  useInstalledSkills,
  useSkillBackups,
  useRestoreSkillBackup,
  useBulkToggleSkillApp,
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
import type { AppId } from "@/lib/api/types";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const DEFAULT_SKILL_APPS = {
  claude: false,
  codex: false,
  gemini: false,
  grokbuild: false,
  opencode: false,
  openclaw: false,
  hermes: false,
  cursor: false,
  pi: false,
} satisfies ImportSkillSelection["apps"];
const IMPORT_SKILLS_APP_IDS = SKILLS_APP_IDS.filter((app) => app !== "pi");

interface UnifiedSkillsPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
  onInteractionBlockedChange?: (blocked: boolean) => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
  onCheckUpdatesStateChange?: (state: SkillsCheckUpdatesState) => void;
}

export interface SkillsCheckUpdatesState {
  isChecking: boolean;
  hasSkills: boolean;
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
>((props, ref) => {
  const {
    onOpenDiscovery,
    currentApp,
    onInteractionBlockedChange,
    onNavigationBlockedChange,
    onCheckUpdatesStateChange,
  } = props;
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
  const [searchQuery, setSearchQuery] = useState("");
  const [writePending, setWritePending] = useState(false);
  const writeLockRef = React.useRef(false);
  const checkUpdatesLockRef = React.useRef(false);

  const { data: skills, isLoading } = useInstalledSkills();
  const {
    data: skillBackups = [],
    refetch: refetchSkillBackups,
    isFetching: isFetchingSkillBackups,
  } = useSkillBackups();
  const deleteBackupMutation = useDeleteSkillBackup();
  const toggleAppMutation = useToggleSkillApp();
  const bulkToggleAppMutation = useBulkToggleSkillApp();
  const uninstallMutation = useUninstallSkill();
  const restoreBackupMutation = useRestoreSkillBackup();
  // enabled: true —— 进入 Skill 页面时自动静默扫描一次（绿点提示来源）
  const { data: unmanagedSkills, refetch: scanUnmanaged } =
    useScanUnmanagedSkills({ enabled: true });
  const importMutation = useImportSkillsFromApps();
  const installFromZipMutation = useInstallSkillsFromZip();
  const {
    data: skillUpdates,
    refetch: checkUpdates,
    isFetching: isCheckingUpdates,
  } = useCheckSkillUpdates();
  const updateSkillMutation = useUpdateSkill();
  const [isUpdatingAll, setIsUpdatingAll] = useState(false);
  const visibleSkillAppIds =
    currentApp === "pi" ? SKILLS_APP_IDS : IMPORT_SKILLS_APP_IDS;

  const mutationPending =
    deleteBackupMutation.isPending ||
    toggleAppMutation.isPending ||
    bulkToggleAppMutation.isPending ||
    uninstallMutation.isPending ||
    restoreBackupMutation.isPending ||
    importMutation.isPending ||
    installFromZipMutation.isPending ||
    updateSkillMutation.isPending ||
    isUpdatingAll;
  const dialogOpen =
    importDialogOpen || restoreDialogOpen || confirmDialog !== null;
  const navigationBlocked = writePending || mutationPending || dialogOpen;
  const interactionBlocked = navigationBlocked || isCheckingUpdates;

  React.useEffect(() => {
    onInteractionBlockedChange?.(interactionBlocked);
  }, [interactionBlocked, onInteractionBlockedChange]);

  React.useEffect(() => {
    onNavigationBlockedChange?.(navigationBlocked);
  }, [navigationBlocked, onNavigationBlockedChange]);

  React.useEffect(
    () => () => {
      onInteractionBlockedChange?.(false);
      onNavigationBlockedChange?.(false);
    },
    [onInteractionBlockedChange, onNavigationBlockedChange],
  );

  const hasSkills = (skills?.length ?? 0) > 0;

  React.useEffect(() => {
    onCheckUpdatesStateChange?.({
      isChecking: isCheckingUpdates,
      hasSkills,
    });
  }, [hasSkills, isCheckingUpdates, onCheckUpdatesStateChange]);

  React.useEffect(
    () => () =>
      onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: false }),
    [onCheckUpdatesStateChange],
  );

  const beginWrite = (allowOpenDialog = false) => {
    if (
      checkUpdatesLockRef.current ||
      isCheckingUpdates ||
      writeLockRef.current ||
      mutationPending ||
      (!allowOpenDialog && dialogOpen)
    ) {
      return false;
    }
    writeLockRef.current = true;
    setWritePending(true);
    return true;
  };

  const endWrite = () => {
    writeLockRef.current = false;
    setWritePending(false);
  };

  const applicableSkillUpdates = useMemo(() => {
    const installedIds = new Set((skills ?? []).map((skill) => skill.id));
    return (skillUpdates ?? []).filter((update) => installedIds.has(update.id));
  }, [skillUpdates, skills]);

  const updatesMap = useMemo(() => {
    const map: Record<string, SkillUpdateInfo> = {};
    for (const update of applicableSkillUpdates) {
      map[update.id] = update;
    }
    return map;
  }, [applicableSkillUpdates]);

  const enabledCounts = useMemo(() => {
    const counts = {
      claude: 0,
      "claude-desktop": 0,
      codex: 0,
      gemini: 0,
      grokbuild: 0,
      opencode: 0,
      openclaw: 0,
      hermes: 0,
      cursor: 0,
      pi: 0,
    };
    if (!skills) return counts;
    skills.forEach((skill) => {
      for (const app of SKILLS_APP_IDS) {
        if (skill.apps[app]) {
          counts[app]++;
        }
      }
    });
    return counts;
  }, [skills]);

  const filteredSkills = useMemo(() => {
    if (!skills) return [];

    const query = searchQuery.trim().toLocaleLowerCase();
    if (!query) return skills;

    return skills.filter((skill) => {
      const searchableValues = [
        skill.name,
        skill.id,
        skill.description,
        skill.directory,
        skill.repoOwner,
        skill.repoName,
        skill.repoOwner && skill.repoName
          ? `${skill.repoOwner}/${skill.repoName}`
          : undefined,
      ];

      return searchableValues.some((value) =>
        value?.toLocaleLowerCase().includes(query),
      );
    });
  }, [searchQuery, skills]);

  const pendingApp = bulkToggleAppMutation.isPending
    ? bulkToggleAppMutation.variables?.app
    : toggleAppMutation.isPending
      ? toggleAppMutation.variables?.app
      : null;

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    if (!beginWrite()) return;

    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleToggleAll = async (app: AppId, enabled: boolean) => {
    if (!skills || !beginWrite()) return;

    const ids = skills
      .filter((skill) => Boolean(skill.apps[app]) !== enabled)
      .map((skill) => skill.id);
    if (ids.length === 0) {
      endWrite();
      return;
    }

    try {
      const result = await bulkToggleAppMutation.mutateAsync({
        ids,
        app,
        enabled,
      });
      if (result.failed.length > 0) {
        toast.error(
          t("common.bulkToggleFailed", { count: result.failed.length }),
          { description: String(result.failed[0].error) },
        );
      }
    } catch (error) {
      toast.error(t("common.bulkToggleFailed", { count: ids.length }), {
        description: String(error),
      });
    } finally {
      endWrite();
    }
  };

  const handleUninstall = (skill: InstalledSkill) => {
    if (
      checkUpdatesLockRef.current ||
      writeLockRef.current ||
      interactionBlocked
    ) {
      return;
    }
    setConfirmDialog({
      isOpen: true,
      title: t("skills.uninstall"),
      message: t("skills.uninstallConfirm", { name: skill.name }),
      onConfirm: async () => {
        if (!beginWrite(true)) return;
        try {
          const result = await uninstallMutation.mutateAsync(skill.id);
          setConfirmDialog(null);
          const piCleanupIncomplete =
            result.piCleanupIncomplete || Boolean(result.preservedPiPath);
          const toastOptions = {
            description: result.preservedPiPath
              ? t("skills.uninstallPiPreserved", {
                  path: result.preservedPiPath,
                })
              : result.piCleanupIncomplete
                ? t("skills.uninstallPiCleanupIncomplete")
                : result.backupPath
                  ? t("skills.backup.location", { path: result.backupPath })
                  : undefined,
            closeButton: true,
          };
          if (piCleanupIncomplete) {
            toast.warning(
              t("skills.uninstallSuccess", { name: skill.name }),
              toastOptions,
            );
          } else {
            toast.success(
              t("skills.uninstallSuccess", { name: skill.name }),
              toastOptions,
            );
          }
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        } finally {
          endWrite();
        }
      },
    });
  };

  const handleOpenImport = async () => {
    if (!beginWrite()) return;
    try {
      const result = await scanUnmanaged();
      if (!result.data || result.data.length === 0) {
        toast.success(t("skills.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleImport = async (imports: ImportSkillSelection[]) => {
    if (!beginWrite(true)) return;
    try {
      const imported = await importMutation.mutateAsync(imports);
      setImportDialogOpen(false);
      toast.success(t("skills.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleInstallFromZip = async () => {
    if (!beginWrite()) return;
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
          t("skills.installFromZip.successSingle", {
            name: installed[0].name,
          }),
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
    } finally {
      endWrite();
    }
  };

  const handleCheckUpdates = async () => {
    if (
      checkUpdatesLockRef.current ||
      writeLockRef.current ||
      interactionBlocked
    ) {
      return;
    }
    checkUpdatesLockRef.current = true;
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
    } finally {
      checkUpdatesLockRef.current = false;
    }
  };

  const handleUpdateSkill = async (skill: InstalledSkill) => {
    if (!beginWrite()) return;
    try {
      const updated = await updateSkillMutation.mutateAsync(skill.id);
      toast.success(t("skills.updateSuccess", { name: updated.name }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("skills.updateFailed"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleUpdateAll = async () => {
    if (applicableSkillUpdates.length === 0 || !beginWrite()) {
      return;
    }
    setIsUpdatingAll(true);
    let successCount = 0;
    try {
      for (const update of applicableSkillUpdates) {
        try {
          await updateSkillMutation.mutateAsync(update.id);
          successCount++;
        } catch (error) {
          toast.error(t("skills.updateFailed"), {
            description: `${update.name}: ${String(error)}`,
          });
        }
      }
    } finally {
      setIsUpdatingAll(false);
      endWrite();
    }
    if (successCount > 0) {
      toast.success(t("skills.updateAllSuccess", { count: successCount }), {
        closeButton: true,
      });
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    if (!beginWrite()) return;
    setRestoreDialogOpen(true);
    try {
      await refetchSkillBackups({ throwOnError: true });
    } catch (error) {
      setRestoreDialogOpen(false);
      toast.error(t("common.error"), { description: String(error) });
    } finally {
      endWrite();
    }
  };

  const handleRestoreFromBackup = async (backupId: string) => {
    if (!beginWrite(true)) return;
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
    } finally {
      endWrite();
    }
  };

  const handleDeleteBackup = (backup: SkillBackupEntry) => {
    if (checkUpdatesLockRef.current || writeLockRef.current) return;
    setConfirmDialog({
      isOpen: true,
      title: t("skills.restoreFromBackup.deleteConfirmTitle"),
      message: t("skills.restoreFromBackup.deleteConfirmMessage", {
        name: backup.skill.name,
      }),
      confirmText: t("skills.restoreFromBackup.delete"),
      variant: "destructive",
      onConfirm: async () => {
        if (!beginWrite(true)) return;
        try {
          let deleteSucceeded = false;
          let deleteError: unknown;
          try {
            await deleteBackupMutation.mutateAsync(backup.backupId);
            deleteSucceeded = true;
          } catch (error) {
            deleteError = error;
          }

          // The backups query is disabled by default, so invalidation alone
          // does not fetch authoritative data. Explicitly refresh after both
          // success and failure (remove_dir_all may have made partial progress).
          let refreshedBackups: SkillBackupEntry[] | undefined;
          try {
            const result = await refetchSkillBackups({ throwOnError: true });
            refreshedBackups = result.data;
          } catch (error) {
            // A refresh failure must not turn a completed deletion into a false
            // "delete failed" report, or replace the original deletion error.
            console.error(
              "Failed to refresh Skill backups after deletion:",
              error,
            );
          }

          if (!deleteSucceeded) {
            // remove_dir_all may finish removing the directory but still
            // report an error. If the authoritative refresh confirms that the
            // item is gone, close the now-stale confirmation dialog.
            if (
              refreshedBackups &&
              !refreshedBackups.some(
                (entry) => entry.backupId === backup.backupId,
              )
            ) {
              setConfirmDialog(null);
            }
            toast.error(t("skills.restoreFromBackup.deleteFailed"), {
              description: String(deleteError),
            });
          } else {
            setConfirmDialog(null);
            toast.success(
              t("skills.restoreFromBackup.deleteSuccess", {
                name: backup.skill.name,
              }),
              {
                closeButton: true,
              },
            );
          }
        } finally {
          endWrite();
        }
      },
    });
  };

  React.useImperativeHandle(ref, () => ({
    openDiscovery: () => {
      if (
        !checkUpdatesLockRef.current &&
        !writeLockRef.current &&
        !interactionBlocked
      ) {
        onOpenDiscovery();
      }
    },
    openImport: handleOpenImport,
    openInstallFromZip: handleInstallFromZip,
    openRestoreFromBackup: handleOpenRestoreFromBackup,
    checkUpdates: handleCheckUpdates,
  }));

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <div className="flex items-center justify-between gap-2">
        <div className="min-w-0 flex-1">
          <AppCountBar
            totalLabel={t("skills.installed", { count: skills?.length || 0 })}
            counts={enabledCounts}
            appIds={visibleSkillAppIds}
            totalCount={skills?.length ?? 0}
            onToggleAll={handleToggleAll}
            pendingApp={pendingApp}
            disabled={interactionBlocked}
          />
        </div>
        <div
          className="mb-4 overflow-hidden transition-all duration-300 ease-out"
          style={{
            maxWidth: applicableSkillUpdates.length > 0 ? "200px" : "0px",
            opacity: applicableSkillUpdates.length > 0 ? 1 : 0,
          }}
        >
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1 whitespace-nowrap disabled:opacity-100"
            onClick={handleUpdateAll}
            disabled={interactionBlocked}
          >
            {isUpdatingAll ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <RefreshCw size={12} />
            )}
            {isUpdatingAll
              ? t("skills.updatingAll")
              : t("skills.updateAll", {
                  count: applicableSkillUpdates.length,
                })}
          </Button>
        </div>
      </div>

      <ManagementListSearch
        value={searchQuery}
        onValueChange={setSearchQuery}
        placeholder={t("skills.installedSearchPlaceholder")}
        ariaLabel={t("skills.installedSearchAriaLabel")}
        clearLabel={t("common.clear")}
      />

      <ScrollArea className="-mr-3 flex-1 min-h-0" type="auto">
        <div className="pb-24 pr-3">
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
          ) : filteredSkills.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-center text-muted-foreground">
              <Search className="mb-4 h-10 w-10 opacity-40" />
              <p className="text-sm">{t("skills.noInstalledSearchResults")}</p>
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
                    actionsDisabled={interactionBlocked}
                    appIds={visibleSkillAppIds}
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
      </ScrollArea>

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          confirmText={confirmDialog.confirmText}
          variant={confirmDialog.variant}
          zIndex="top"
          pending={writePending}
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
  appIds: AppId[];
  hasUpdate?: boolean;
  isUpdating?: boolean;
  actionsDisabled?: boolean;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  onUpdate?: () => void;
  isLast?: boolean;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  appIds,
  hasUpdate,
  isUpdating,
  actionsDisabled,
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

      <AppToggleGroup
        apps={skill.apps}
        onToggle={(app, enabled) => onToggleApp(skill.id, app, enabled)}
        appIds={appIds}
        disabled={actionsDisabled}
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
            className={cn(
              "h-7 w-7 hover:text-blue-500 hover:bg-blue-100 dark:hover:text-blue-400 dark:hover:bg-blue-500/10",
              actionsDisabled && !isUpdating && "disabled:opacity-100",
            )}
            onClick={onUpdate}
            disabled={actionsDisabled || isUpdating}
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
          className="h-7 w-7 hover:text-red-500 hover:bg-red-100 disabled:opacity-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          onClick={onUninstall}
          disabled={actionsDisabled}
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
  const actionPending = isRestoring || isDeleting;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => !nextOpen && !actionPending && onClose()}
    >
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
          <Button
            type="button"
            variant="outline"
            onClick={onClose}
            disabled={actionPending}
          >
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
          ...DEFAULT_SKILL_APPS,
          ...Object.fromEntries(
            SKILLS_APP_IDS.map((app) => [app, skill.foundIn.includes(app)]),
          ),
        } satisfies ImportSkillSelection["apps"],
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
        apps: selectedApps[directory] ?? { ...DEFAULT_SKILL_APPS },
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
                          ...DEFAULT_SKILL_APPS,
                        }
                      }
                      onToggle={(app, enabled) => {
                        setSelectedApps((prev) => ({
                          ...prev,
                          [skill.directory]: {
                            ...(prev[skill.directory] ?? {
                              ...DEFAULT_SKILL_APPS,
                            }),
                            [app]: enabled,
                          },
                        }));
                      }}
                      appIds={IMPORT_SKILLS_APP_IDS}
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
