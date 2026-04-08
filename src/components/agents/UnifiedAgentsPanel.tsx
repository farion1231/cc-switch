import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, Trash2, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  type ImportAgentSelection,
  type AgentBackupEntry,
  useDeleteAgentBackup,
  useInstalledAgents,
  useAgentBackups,
  useRestoreAgentBackup,
  useToggleAgentApp,
  useUninstallAgent,
  useScanUnmanagedAgents,
  useImportAgentsFromApps,
  useInstallAgentsFromZip,
  type InstalledAgent,
} from "@/hooks/useAgents";
import type { AppId } from "@/lib/api/types";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi, agentsApi } from "@/lib/api";
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

interface UnifiedAgentsPanelProps {
  onOpenDiscovery: () => void;
  currentApp: AppId;
}

export interface UnifiedAgentsPanelHandle {
  openDiscovery: () => void;
  openImport: () => void;
  openInstallFromZip: () => void;
  openRestoreFromBackup: () => void;
}

function formatAgentBackupDate(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return Number.isNaN(date.getTime())
    ? String(unixSeconds)
    : date.toLocaleString();
}

const UnifiedAgentsPanel = React.forwardRef<
  UnifiedAgentsPanelHandle,
  UnifiedAgentsPanelProps
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

  const { data: agents, isLoading } = useInstalledAgents();
  const {
    data: agentBackups = [],
    refetch: refetchAgentBackups,
    isFetching: isFetchingAgentBackups,
  } = useAgentBackups();
  const deleteBackupMutation = useDeleteAgentBackup();
  const toggleAppMutation = useToggleAgentApp();
  const uninstallMutation = useUninstallAgent();
  const restoreBackupMutation = useRestoreAgentBackup();
  const { data: unmanagedAgents, refetch: scanUnmanaged } =
    useScanUnmanagedAgents();
  const importMutation = useImportAgentsFromApps();
  const installFromZipMutation = useInstallAgentsFromZip();

  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0, opencode: 0, openclaw: 0 };
    if (!agents) return counts;
    agents.forEach((agent) => {
      for (const app of MCP_SKILLS_APP_IDS) {
        if (agent.apps[app]) counts[app]++;
      }
    });
    return counts;
  }, [agents]);

  const handleToggleApp = async (id: string, app: AppId, enabled: boolean) => {
    try {
      await toggleAppMutation.mutateAsync({ id, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleUninstall = (agent: InstalledAgent) => {
    setConfirmDialog({
      isOpen: true,
      title: t("agents.uninstall"),
      message: t("agents.uninstallConfirm", { name: agent.name }),
      onConfirm: async () => {
        try {
          const result = await uninstallMutation.mutateAsync({
            id: agent.id,
            agentKey: agent.id,
          });
          setConfirmDialog(null);
          toast.success(t("agents.uninstallSuccess", { name: agent.name }), {
            description: result.backupPath
              ? t("agents.backup.location", { path: result.backupPath })
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
        toast.success(t("agents.noUnmanagedFound"), { closeButton: true });
        return;
      }
      setImportDialogOpen(true);
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleImport = async (imports: ImportAgentSelection[]) => {
    try {
      const imported = await importMutation.mutateAsync(imports);
      setImportDialogOpen(false);
      toast.success(t("agents.importSuccess", { count: imported.length }), {
        closeButton: true,
      });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const handleInstallFromZip = async () => {
    try {
      const filePath = await agentsApi.openZipFileDialog();
      if (!filePath) return;

      const installed = await installFromZipMutation.mutateAsync({
        filePath,
        currentApp,
      });

      if (installed.length === 0) {
        toast.info(t("agents.installFromZip.noAgentsFound"), {
          closeButton: true,
        });
      } else if (installed.length === 1) {
        toast.success(
          t("agents.installFromZip.successSingle", { name: installed[0].name }),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("agents.installFromZip.successMultiple", {
            count: installed.length,
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      toast.error(t("agents.installFailed"), { description: String(error) });
    }
  };

  const handleOpenRestoreFromBackup = async () => {
    setRestoreDialogOpen(true);
    try {
      await refetchAgentBackups();
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
        t("agents.restoreFromBackup.success", { name: restored.name }),
        {
          closeButton: true,
        },
      );
    } catch (error) {
      toast.error(t("agents.restoreFromBackup.failed"), {
        description: String(error),
      });
    }
  };

  const handleDeleteBackup = (backup: AgentBackupEntry) => {
    setConfirmDialog({
      isOpen: true,
      title: t("agents.restoreFromBackup.deleteConfirmTitle"),
      message: t("agents.restoreFromBackup.deleteConfirmMessage", {
        name: backup.agent.name,
      }),
      confirmText: t("agents.restoreFromBackup.delete"),
      variant: "destructive",
      onConfirm: async () => {
        try {
          await deleteBackupMutation.mutateAsync(backup.backupId);
          await refetchAgentBackups();
          setConfirmDialog(null);
          toast.success(
            t("agents.restoreFromBackup.deleteSuccess", {
              name: backup.agent.name,
            }),
            {
              closeButton: true,
            },
          );
        } catch (error) {
          toast.error(t("agents.restoreFromBackup.deleteFailed"), {
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
        totalLabel={t("agents.installed", { count: agents?.length || 0 })}
        counts={enabledCounts}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
        {isLoading ? (
          <div className="text-center py-12 text-muted-foreground">
            {t("agents.loading")}
          </div>
        ) : !agents || agents.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Sparkles size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("agents.noInstalled")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("agents.noInstalledDescription")}
            </p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <div className="rounded-xl border border-border-default overflow-hidden">
              {agents.map((agent, index) => (
                <InstalledAgentListItem
                  key={agent.id}
                  agent={agent}
                  onToggleApp={handleToggleApp}
                  onUninstall={() => handleUninstall(agent)}
                  isLast={index === agents.length - 1}
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

      {importDialogOpen && unmanagedAgents && (
        <ImportAgentsDialog
          agents={unmanagedAgents}
          onImport={handleImport}
          onClose={() => setImportDialogOpen(false)}
        />
      )}

      <RestoreAgentsDialog
        backups={agentBackups}
        isDeleting={deleteBackupMutation.isPending}
        isLoading={isFetchingAgentBackups}
        onDelete={handleDeleteBackup}
        isRestoring={restoreBackupMutation.isPending}
        onRestore={handleRestoreFromBackup}
        onClose={() => setRestoreDialogOpen(false)}
        open={restoreDialogOpen}
      />
    </div>
  );
});

UnifiedAgentsPanel.displayName = "UnifiedAgentsPanel";

interface InstalledAgentListItemProps {
  agent: InstalledAgent;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  isLast?: boolean;
}

const InstalledAgentListItem: React.FC<InstalledAgentListItemProps> = ({
  agent,
  onToggleApp,
  onUninstall,
  isLast,
}) => {
  const { t } = useTranslation();

  const openDocs = async () => {
    if (!agent.readmeUrl) return;
    try {
      await settingsApi.openExternal(agent.readmeUrl);
    } catch {
      // ignore
    }
  };

  const sourceLabel = useMemo(() => {
    if (agent.repoOwner && agent.repoName) {
      return `${agent.repoOwner}/${agent.repoName}`;
    }
    return t("agents.local");
  }, [agent.repoOwner, agent.repoName, t]);

  return (
    <ListItemRow isLast={isLast}>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="font-medium text-sm text-foreground truncate">
            {agent.name}
          </span>
          {agent.readmeUrl && (
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
        {agent.description && (
          <p
            className="text-xs text-muted-foreground truncate"
            title={agent.description}
          >
            {agent.description}
          </p>
        )}
      </div>

      <AppToggleGroup
        apps={agent.apps}
        onToggle={(app, enabled) => onToggleApp(agent.id, app, enabled)}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          onClick={onUninstall}
          title={t("agents.uninstall")}
        >
          <Trash2 size={14} />
        </Button>
      </div>
    </ListItemRow>
  );
};

interface ImportAgentsDialogProps {
  agents: Array<{
    directory: string;
    name: string;
    description?: string;
    foundIn: string[];
    path: string;
  }>;
  onImport: (imports: ImportAgentSelection[]) => void;
  onClose: () => void;
}

interface RestoreAgentsDialogProps {
  backups: AgentBackupEntry[];
  isDeleting: boolean;
  isLoading: boolean;
  isRestoring: boolean;
  onDelete: (backup: AgentBackupEntry) => void;
  onRestore: (backupId: string) => void;
  onClose: () => void;
  open: boolean;
}

const RestoreAgentsDialog: React.FC<RestoreAgentsDialogProps> = ({
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
          <DialogTitle>{t("agents.restoreFromBackup.title")}</DialogTitle>
          <DialogDescription>
            {t("agents.restoreFromBackup.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4">
          {isLoading ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("common.loading")}
            </div>
          ) : backups.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              {t("agents.restoreFromBackup.empty")}
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
                          {backup.agent.name}
                        </div>
                        <div className="rounded-md bg-muted px-2 py-0.5 text-[11px] text-muted-foreground">
                          {backup.agent.directory}
                        </div>
                      </div>
                      {backup.agent.description && (
                        <div className="mt-2 text-sm text-muted-foreground">
                          {backup.agent.description}
                        </div>
                      )}
                      <div className="mt-3 space-y-1.5 text-xs text-muted-foreground">
                        <div>
                          {t("agents.restoreFromBackup.createdAt")}:{" "}
                          {formatAgentBackupDate(backup.createdAt)}
                        </div>
                        <div className="break-all" title={backup.backupPath}>
                          {t("agents.restoreFromBackup.path")}:{" "}
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
                          ? t("agents.restoreFromBackup.restoring")
                          : t("agents.restoreFromBackup.restore")}
                      </Button>
                      <Button
                        type="button"
                        variant="destructive"
                        onClick={() => onDelete(backup)}
                        disabled={isRestoring || isDeleting}
                      >
                        {isDeleting
                          ? t("agents.restoreFromBackup.deleting")
                          : t("agents.restoreFromBackup.delete")}
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

const ImportAgentsDialog: React.FC<ImportAgentsDialogProps> = ({
  agents,
  onImport,
  onClose,
}) => {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(
    new Set(agents.map((a) => a.directory)),
  );
  const [selectedApps, setSelectedApps] = useState<
    Record<string, ImportAgentSelection["apps"]>
  >(() =>
    Object.fromEntries(
      agents.map((agent) => [
        agent.directory,
        {
          claude: agent.foundIn.includes("claude"),
          codex: agent.foundIn.includes("codex"),
          gemini: agent.foundIn.includes("gemini"),
          opencode: agent.foundIn.includes("opencode"),
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
          <h2 className="text-lg font-semibold mb-2">{t("agents.import")}</h2>
          <p className="text-sm text-muted-foreground mb-4">
            {t("agents.importDescription")}
          </p>

          <div className="flex-1 overflow-y-auto space-y-2 mb-4">
            {agents.map((agent) => (
              <div
                key={agent.directory}
                className="flex items-start gap-3 p-3 rounded-lg border hover:bg-muted"
              >
                <input
                  type="checkbox"
                  checked={selected.has(agent.directory)}
                  onChange={() => toggleSelect(agent.directory)}
                  className="mt-1"
                />
                <div className="flex-1 min-w-0">
                  <div className="font-medium">{agent.name}</div>
                  {agent.description && (
                    <div className="text-sm text-muted-foreground line-clamp-1">
                      {agent.description}
                    </div>
                  )}
                  <div className="mt-2">
                    <AppToggleGroup
                      apps={
                        selectedApps[agent.directory] ?? {
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
                          [agent.directory]: {
                            ...(prev[agent.directory] ?? {
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
                    title={agent.path}
                  >
                    {agent.path}
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
              {t("agents.importSelected", { count: selected.size })}
            </Button>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
};

export default UnifiedAgentsPanel;
