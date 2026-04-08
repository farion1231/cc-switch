import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, Trash2, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  type ImportRuleSelection,
  type RuleBackupEntry,
  useDeleteRuleBackup,
  useInstalledRules,
  useRuleBackups,
  useRestoreRuleBackup,
  useToggleRuleApp,
  useUninstallRule,
  useScanUnmanagedRules,
  useImportRulesFromApps,
  useInstallRulesFromZip,
  type InstalledRule,
} from "@/hooks/useRules";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, rulesApi } from "@/lib/api";
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

interface UnifiedRulesPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
}

export interface UnifiedRulesPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
  openInstallFromZip: () => void;
  openRestoreFromBackup: () => void;
}

function formatRuleBackupDate(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return Number.isNaN(date.getTime())
    ? String(unixSeconds)
    : date.toLocaleString();
}

const UnifiedRulesPanel = React.forwardRef<
  UnifiedRulesPanelHandle,
  UnifiedRulesPanelProps
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

  const { data: rules, isLoading } = useInstalledRules();
  const {
    data: ruleBackups = [],
    refetch: refetchRuleBackups,
    isFetching: isFetchingRuleBackups,
  } = useRuleBackups();
  const deleteBackupMutation = useDeleteRuleBackup();
  const toggleAppMutation = useToggleRuleApp();
  const uninstallMutation = useUninstallRule();
  const restoreBackupMutation = useRestoreRuleBackup();
  const { data: unmanagedRules, refetch: scanUnmanaged } =
    useScanUnmanagedRules();
  const importMutation = useImportRulesFromApps();
  const installFromZipMutation = useInstallRulesFromZip();

  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0, opencode: 0, openclaw: 0 };
    if (!rules) return counts;
    rules.forEach((rule) => {
      for (const app of MCP_SKILLS_APP_IDS) {
        if (rule.apps[app]) counts[app]++;
      }
    });
    return counts;
  }, [rules]);

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleUninstall = (rule: InstalledRule) => {
    setConfirmDialog({
      isOpen: true,
      title: t("rules.uninstall"),
      message: t("rules.uninstallConfirm", { name: rule.name }),
      onConfirm: async () => {
        try {
          const result = await uninstallMutation.mutateAsync({
            id: rule.id,
            ruleKey: rule.id,
          });
          setConfirmDialog(null);
          toast.success(t("rules.uninstallSuccess", { name: rule.name }), {
            description: result.backupPath
              ? t("rules.backup.location", { path: result.backupPath })
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
        toast.success(t("rules.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleImport = async (imports: ImportRuleSelection[]) => {
    try {
      const imported = await importMutation.mutateAsync(imports);
      setImportDialogOpen(false);
      toast.success(t("rules.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleInstallFromZip = async () => {
    try {
      const filePath = await rulesApi.openZipFileDialog();
      if (!filePath) return;

      const installed = await installFromZipMutation.mutateAsync({
        filePath,
        currentApp,
      });

      if (installed.length === 0) {
        toast.info(t("rules.installFromZip.noRulesFound"), {
          closeButton: true,
        });
      } else if (installed.length === 1) {
        toast.success(
          t("rules.installFromZip.successSingle", { name: installed[0].name }),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("rules.installFromZip.successMultiple", {
            count: installed.length,
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      toast.error(t("rules.installFailed"), { description: String(error) });
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    setRestoreDialogOpen(true);
    try {
      await refetchRuleBackups();
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
        t("rules.restoreFromBackup.success", { name: restored.name }),
        {
          closeButton: true,
        },
      );
    } catch (error) {
      toast.error(t("rules.restoreFromBackup.failed"), {
        description: String(error),
      });
    }
  };

  const handleDeleteBackup = (backup: RuleBackupEntry) => {
    setConfirmDialog({
      isOpen: true,
      title: t("rules.restoreFromBackup.deleteConfirmTitle"),
      message: t("rules.restoreFromBackup.deleteConfirmMessage", {
        name: backup.rule.name,
      }),
      confirmText: t("rules.restoreFromBackup.delete"),
      variant: "destructive",
      onConfirm: async () => {
        try {
          await deleteBackupMutation.mutateAsync(backup.backupId);
          await refetchRuleBackups();
          setConfirmDialog(null);
          toast.success(
            t("rules.restoreFromBackup.deleteSuccess", {
              name: backup.rule.name,
            }),
            {
              closeButton: true,
            },
          );
        } catch (error) {
          toast.error(t("rules.restoreFromBackup.deleteFailed"), {
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
        totalLabel={t("rules.installed", { count: rules?.length || 0 })}
        counts={enabledCounts}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
        {isLoading ? (
          <div className="text-center py-12 text-muted-foreground">
            {t("rules.loading")}
          </div>
        ) : !rules || rules.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Sparkles size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("rules.noInstalled")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("rules.noInstalledDescription")}
            </p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <div className="rounded-xl border border-border-default overflow-hidden">
              {rules.map((rule, index) => (
                <InstalledRuleListItem
                  key={rule.id}
                  rule={rule}
                  onToggleApp={handleToggleApp}
                  onUninstall={() => handleUninstall(rule)}
                  isLast={index === rules.length - 1}
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

      {importDialogOpen && unmanagedRules && (
        <ImportRulesDialog
          rules={unmanagedRules}
          onImport={handleImport}
          onClose={() => setImportDialogOpen(false)}
        />
      )}

      <RestoreRulesDialog
        backups={ruleBackups}
        isDeleting={deleteBackupMutation.isPending}
        isLoading={isFetchingRuleBackups}
        onDelete={handleDeleteBackup}
        isRestoring={restoreBackupMutation.isPending}
        onRestore={handleRestoreFromBackup}
        onClose={() => setRestoreDialogOpen(false)}
        open={restoreDialogOpen}
      />
    </div>
  );
});

UnifiedRulesPanel.displayName = "UnifiedRulesPanel";

interface InstalledRuleListItemProps {
  rule: InstalledRule;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  isLast?: boolean;
}

const InstalledRuleListItem: React.FC<InstalledRuleListItemProps> = ({
  rule,
  onToggleApp,
  onUninstall,
  isLast,
}) => {
  const { t } = useTranslation();

  const openDocs = async () => {
    if (!rule.readmeUrl) return;
    try {
      await settingsApi.openExternal(rule.readmeUrl);
    } catch {
      // ignore
    }
  };

  const sourceLabel = useMemo(() => {
    if (rule.repoOwner && rule.repoName) {
      return `${rule.repoOwner}/${rule.repoName}`;
    }
    return t("rules.local");
  }, [rule.repoOwner, rule.repoName, t]);

  return (
    <ListItemRow isLast={isLast}>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="font-medium text-sm text-foreground truncate">
            {rule.name}
          </span>
          {rule.readmeUrl && (
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
        {rule.description && (
          <p
            className="text-xs text-muted-foreground truncate"
            title={rule.description}
          >
            {rule.description}
          </p>
        )}
      </div>

      <AppToggleGroup
        apps={rule.apps}
        onToggle={(app, enabled) => onToggleApp(rule.id, app, enabled)}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          onClick={onUninstall}
          title={t("rules.uninstall")}
        >
          <Trash2 size={14} />
        </Button>
      </div>
    </ListItemRow>
  );
};

interface ImportRulesDialogProps {
  rules: Array<{
    directory: string;
    name: string;
    description?: string;
    foundIn: string[];
    path: string;
  }>;
  onImport: (imports: ImportRuleSelection[]) => void;
  onClose: () => void;
}

interface RestoreRulesDialogProps {
  backups: RuleBackupEntry[];
  isDeleting: boolean;
  isLoading: boolean;
  isRestoring: boolean;
  onDelete: (backup: RuleBackupEntry) => void;
  onRestore: (backupId: string) => void;
  onClose: () => void;
  open: boolean;
}

const RestoreRulesDialog: React.FC<RestoreRulesDialogProps> = ({
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
          <DialogTitle>{t("rules.restoreFromBackup.title")}</DialogTitle>
          <DialogDescription>
            {t("rules.restoreFromBackup.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4">
          {isLoading ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("common.loading")}
            </div>
          ) : backups.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("rules.restoreFromBackup.empty")}
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
                          {backup.rule.name}
                        </div>
                        <div className="rounded-md bg-muted px-2 py-0.5 text-[11px] text-muted-foreground">
                          {backup.rule.directory}
                        </div>
                      </div>
                      {backup.rule.description && (
                        <div className="mt-2 text-sm text-muted-foreground">
                          {backup.rule.description}
                        </div>
                      )}
                      <div className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                        <div>
                          {t("rules.restoreFromBackup.createdAt")}:{" "}
                          {formatRuleBackupDate(backup.createdAt)}
                        </div>
                        <div className="break-all" title={backup.backupPath}>
                          {t("rules.restoreFromBackup.path")}:{" "}
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
                          ? t("rules.restoreFromBackup.restoring")
                          : t("rules.restoreFromBackup.restore")}
                      </Button>
                      <Button
                        type="button"
                        variant="destructive"
                        onClick={() => onDelete(backup)}
                        disabled={isRestoring || isDeleting}
                      >
                        {isDeleting
                          ? t("rules.restoreFromBackup.deleting")
                          : t("rules.restoreFromBackup.delete")}
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

const ImportRulesDialog: React.FC<ImportRulesDialogProps> = ({
  rules,
  onImport,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    new Set(rules.map((r) => r.directory)),
  );
  const [selectedApps, setSelectedApps] = useState<
    Record<string, ImportRuleSelection["apps"]>
  >(() =>
    Object.fromEntries(
      rules.map((rule) => [
        rule.directory,
        {
          claude: rule.foundIn.includes("claude"),
          codex: rule.foundIn.includes("codex"),
          gemini: rule.foundIn.includes("gemini"),
          opencode: rule.foundIn.includes("opencode"),
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
          <h2 className="text-lg font-semibold mb-2">{t("rules.import")}</h2>
          <p className="text-sm text-muted-foreground mb-4">
            {t("rules.importDescription")}
          </p>

          <div className="flex-1 overflow-y-auto space-y-2 mb-4">
            {rules.map((rule) => (
              <div
                key={rule.directory}
                className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted"
              >
                <input
                  type="checkbox"
                  checked={selected.has(rule.directory)}
                  onChange={() => toggleSelect(rule.directory)}
                  className="mt-1"
                />
                <div className="flex-1 min-w-0">
                  <div className="font-medium">{rule.name}</div>
                  {rule.description && (
                    <div className="text-sm text-muted-foreground line-clamp-1">
                      {rule.description}
                    </div>
                  )}
                  <div className="mt-2">
                    <AppToggleGroup
                      apps={
                        selectedApps[rule.directory] ?? {
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
                          [rule.directory]: {
                            ...(prev[rule.directory] ?? {
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
                    title={rule.path}
                  >
                    {rule.path}
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
              {t("rules.importSelected", { count: selected.size })}
            </Button>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
};

export default UnifiedRulesPanel;
