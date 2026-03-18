import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, Trash2, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
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
  type InstalledSkill,
} from "@/hooks/useSkills";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, skillsApi } from "@/lib/api";
import { toast } from "sonner";
import { MCP_SKILLS_APP_IDS } from "@/config/appConfig";
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

export interface UnifiedSkillsPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
  openInstallFromZip: () => void;
  openRestoreFromBackup: () => void;
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

  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0, opencode: 0, openclaw: 0 };
    if (!skills) return counts;
    skills.forEach((skill) => {
      for (const app of MCP_SKILLS_APP_IDS) {
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
          const result = await uninstallMutation.mutateAsync(skill.id);
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
  }));

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <AppCountBar
        totalLabel={t("skills.installed", { count: skills?.length || 0 })}
        counts={enabledCounts}
        appIds={MCP_SKILLS_APP_IDS}
      />

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
            <div className="rounded-xl border border-border-default overflow-hidden">
              {skills.map((skill, index) => (
                <InstalledSkillListItem
                  key={skill.id}
                  skill={skill}
                  onToggleApp={handleToggleApp}
                  onUninstall={() => handleUninstall(skill)}
                  isLast={index === skills.length - 1}
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
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  isLast?: boolean;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  onToggleApp,
  onUninstall,
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
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
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
                            }),
                            [app]: enabled,
                          },
                        }));
                      }}
                      appIds={MCP_SKILLS_APP_IDS}
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
            <Button variant="outline" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleImport} disabled={selected.size === 0}>
              {t("skills.importSelected", { count: selected.size })}
            </Button>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
};

export default UnifiedSkillsPanel;
