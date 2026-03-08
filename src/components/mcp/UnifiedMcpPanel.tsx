import React, { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Server,
  Edit3,
  Trash2,
  ExternalLink,
  GitCommitHorizontal,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  useAllMcpServers,
  useToggleMcpApp,
  useDeleteMcpServer,
  useImportMcpFromApps,
} from "@/hooks/useMcp";
import type { McpServer } from "@/types";
import type { AppId } from "@/lib/api/types";
import McpFormModal from "./McpFormModal";
import { ConfirmDialog } from "../ConfirmDialog";
import { settingsApi } from "@/lib/api";
import { mcpApi } from "@/lib/api/mcp";
import { mcpPresets } from "@/config/mcpPresets";
import { toast } from "sonner";
import { MCP_SKILLS_APP_IDS } from "@/config/appConfig";
import { AppCountBar } from "@/components/common/AppCountBar";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { ListItemRow } from "@/components/common/ListItemRow";

interface UnifiedMcpPanelProps {
  onOpenChange: (open: boolean) => void;
}

export interface UnifiedMcpPanelHandle {
  openAdd: () => void;
  openImport: () => void;
}

const UnifiedMcpPanel = React.forwardRef<
  UnifiedMcpPanelHandle,
  UnifiedMcpPanelProps
>(({ onOpenChange: _onOpenChange }, ref) => {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [isBatchDeleting, setIsBatchDeleting] = useState(false);
  const [isBatchTogglingApps, setIsBatchTogglingApps] = useState(false);
  const [batchModeEnabled, setBatchModeEnabled] = useState(false);
  const [pendingAppChanges, setPendingAppChanges] = useState<
    Record<string, Partial<Record<AppId, boolean>>>
  >({});
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);

  const { data: serversMap, isLoading } = useAllMcpServers();
  const toggleAppMutation = useToggleMcpApp();
  const deleteServerMutation = useDeleteMcpServer();
  const importMutation = useImportMcpFromApps();

  const serverEntries = useMemo((): Array<[string, McpServer]> => {
    if (!serversMap) return [];
    return Object.entries(serversMap);
  }, [serversMap]);

  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0, opencode: 0, openclaw: 0 };
    serverEntries.forEach(([_, server]) => {
      for (const app of MCP_SKILLS_APP_IDS) {
        if (server.apps[app]) counts[app]++;
      }
    });
    return counts;
  }, [serverEntries]);

  const selectedCount = useMemo(
    () => serverEntries.filter(([id]) => selected.has(id)).length,
    [selected, serverEntries],
  );

  const selectedServerIds = useMemo(
    () => serverEntries.filter(([id]) => selected.has(id)).map(([id]) => id),
    [selected, serverEntries],
  );

  const pendingChangeCount = useMemo(
    () =>
      Object.values(pendingAppChanges).reduce(
        (sum, appChanges) => sum + Object.keys(appChanges).length,
        0,
      ),
    [pendingAppChanges],
  );

  const getEffectiveApps = (server: McpServer): Record<AppId, boolean> => ({
    ...server.apps,
    ...pendingAppChanges[server.id],
  });

  const handleToggleApp = async (
    serverId: string,
    app: AppId,
    enabled: boolean,
  ) => {
    try {
      await toggleAppMutation.mutateAsync({ serverId, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const handleSelectAllVisible = () => {
    if (serverEntries.length === 0) return;
    setSelected((prev) => {
      const next = new Set(prev);
      serverEntries.forEach(([id]) => next.add(id));
      return next;
    });
  };

  const handleClearSelection = () => setSelected(new Set());

  const handleModeChange = (checked: boolean) => {
    if (!checked && pendingChangeCount > 0) {
      setConfirmDialog({
        isOpen: true,
        title: t("mcp.pendingDiscardTitle", {
          defaultValue: "Discard pending changes?",
        }),
        message: t("mcp.pendingDiscardMessage", {
          count: pendingChangeCount,
          defaultValue:
            "You have {{count}} pending app changes that have not been applied. Leaving batch mode will discard them.",
        }),
        onConfirm: () => {
          setPendingAppChanges({});
          setBatchModeEnabled(false);
          setConfirmDialog(null);
        },
      });
      return;
    }

    setBatchModeEnabled(checked);
  };

  const handleStageBatchToggle = (
    serverId: string,
    app: AppId,
    enabled: boolean,
  ) => {
    const targetIds =
      selectedServerIds.length === 0 || !selected.has(serverId)
        ? Array.from(new Set([...selectedServerIds, serverId]))
        : selectedServerIds;

    if (!selected.has(serverId)) {
      setSelected((prev) => new Set(prev).add(serverId));
    }

    setPendingAppChanges((prev) => {
      const next = { ...prev };
      targetIds.forEach((id) => {
        next[id] = {
          ...(next[id] ?? {}),
          [app]: enabled,
        };
      });
      return next;
    });
  };

  const handleAppToggle = async (
    server: McpServer,
    app: AppId,
    enabled: boolean,
  ) => {
    if (batchModeEnabled) {
      handleStageBatchToggle(server.id, app, enabled);
      return;
    }
    await handleToggleApp(server.id, app, enabled);
  };

  const handleApplyPendingChanges = () => {
    if (pendingChangeCount === 0) return;

    setConfirmDialog({
      isOpen: true,
      title: t("mcp.applyChangesTitle", {
        count: pendingChangeCount,
        defaultValue: `Apply changes (${pendingChangeCount})`,
      }),
      message: t("mcp.applyChangesMessage", {
        count: pendingChangeCount,
        defaultValue: `Apply ${pendingChangeCount} staged app changes now?`,
      }),
      onConfirm: async () => {
        setIsBatchTogglingApps(true);
        try {
          const operations = Object.entries(pendingAppChanges).flatMap(
            ([serverId, appChanges]) =>
              Object.entries(appChanges).map(([app, enabled]) => ({
                serverId,
                app: app as AppId,
                enabled: enabled as boolean,
              })),
          );

          const results = await Promise.allSettled(
            operations.map(({ serverId, app, enabled }) =>
              mcpApi.toggleApp(serverId, app, enabled),
            ),
          );

          const failedKeys = new Set<string>();
          results.forEach((result, index) => {
            if (result.status === "rejected") {
              const operation = operations[index];
              failedKeys.add(`${operation.serverId}:${operation.app}`);
            }
          });

          const nextPending: Record<
            string,
            Partial<Record<AppId, boolean>>
          > = {};
          Object.entries(pendingAppChanges).forEach(([id, appChanges]) => {
            const retainedEntries = Object.entries(appChanges).filter(([app]) =>
              failedKeys.has(`${id}:${app}`),
            );
            if (retainedEntries.length > 0) {
              nextPending[id] = Object.fromEntries(retainedEntries) as Partial<
                Record<AppId, boolean>
              >;
            }
          });

          const success = results.filter(
            (item) => item.status === "fulfilled",
          ).length;
          const failed = results.length - success;

          await queryClient.invalidateQueries({ queryKey: ["mcp", "all"] });

          setPendingAppChanges(nextPending);
          setConfirmDialog(null);

          if (failed === 0) {
            toast.success(
              t("mcp.applyChangesSuccess", {
                count: success,
                defaultValue: `Applied ${success} app changes`,
              }),
              { closeButton: true },
            );
          } else {
            const firstFailure = results.find(
              (item) => item.status === "rejected",
            ) as PromiseRejectedResult | undefined;
            toast.warning(
              t("mcp.applyChangesPartial", {
                success,
                failed,
                defaultValue: `Apply finished: ${success} succeeded, ${failed} failed`,
              }),
              {
                description: firstFailure?.reason
                  ? String(firstFailure.reason)
                  : undefined,
              },
            );
          }
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        } finally {
          setIsBatchTogglingApps(false);
        }
      },
    });
  };

  const handleEdit = (id: string) => {
    setEditingId(id);
    setIsFormOpen(true);
  };

  const handleAdd = () => {
    setEditingId(null);
    setIsFormOpen(true);
  };

  const handleImport = async () => {
    try {
      const count = await importMutation.mutateAsync();
      if (count === 0) {
        toast.success(t("mcp.unifiedPanel.noImportFound"), {
          closeButton: true,
        });
      } else {
        toast.success(t("mcp.unifiedPanel.importSuccess", { count }), {
          closeButton: true,
        });
      }
    } catch (error) {
      toast.error(t("common.error"), { description: String(error) });
    }
  };

  React.useImperativeHandle(ref, () => ({
    openAdd: handleAdd,
    openImport: handleImport,
  }));

  const handleDelete = (id: string) => {
    setConfirmDialog({
      isOpen: true,
      title: t("mcp.unifiedPanel.deleteServer"),
      message: t("mcp.unifiedPanel.deleteConfirm", { id }),
      onConfirm: async () => {
        try {
          await deleteServerMutation.mutateAsync(id);
          setConfirmDialog(null);
          toast.success(t("common.success"), { closeButton: true });
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        }
      },
    });
  };

  const handleBatchDelete = () => {
    const targets = serverEntries.filter(([id]) => selected.has(id));
    if (targets.length === 0) {
      toast.info(
        t("mcp.bulkDeleteNoop", {
          defaultValue: "没有可删除的已选 MCP",
        }),
      );
      return;
    }

    setConfirmDialog({
      isOpen: true,
      title: t("mcp.bulkDelete", {
        defaultValue: "删除已选（{{count}}）",
        count: targets.length,
      }),
      message: t("mcp.bulkDeleteConfirm", {
        defaultValue:
          "确定要删除已选的 {{count}} 个 MCP 服务器吗？此操作无法撤销。",
        count: targets.length,
      }),
      onConfirm: async () => {
        setIsBatchDeleting(true);
        try {
          const results = await Promise.allSettled(
            targets.map(([id]) => mcpApi.deleteUnifiedServer(id)),
          );
          const success = results.filter(
            (item) => item.status === "fulfilled",
          ).length;
          const failed = results.length - success;

          await queryClient.invalidateQueries({ queryKey: ["mcp", "all"] });

          setSelected(new Set());
          setConfirmDialog(null);

          if (failed === 0) {
            toast.success(
              t("mcp.bulkDeleteSuccess", {
                count: success,
                defaultValue: "已删除 {{count}} 个 MCP 服务器",
              }),
              { closeButton: true },
            );
          } else {
            const firstFailure = results.find(
              (item) => item.status === "rejected",
            ) as PromiseRejectedResult | undefined;
            toast.warning(
              t("mcp.bulkDeletePartial", {
                success,
                failed,
                defaultValue: "删除完成：成功 {{success}}，失败 {{failed}}",
              }),
              {
                description: firstFailure?.reason
                  ? String(firstFailure.reason)
                  : undefined,
              },
            );
          }
        } catch (error) {
          toast.error(t("common.error"), { description: String(error) });
        } finally {
          setIsBatchDeleting(false);
        }
      },
    });
  };

  const handleCloseForm = () => {
    setIsFormOpen(false);
    setEditingId(null);
  };

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      <AppCountBar
        totalLabel={t("mcp.serverCount", { count: serverEntries.length })}
        counts={enabledCounts}
        appIds={MCP_SKILLS_APP_IDS}
      />

      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-24">
        {isLoading ? (
          <div className="text-center py-12 text-muted-foreground">
            {t("mcp.loading")}
          </div>
        ) : serverEntries.length === 0 ? (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 bg-muted rounded-full flex items-center justify-center">
              <Server size={24} className="text-muted-foreground" />
            </div>
            <h3 className="text-lg font-medium text-foreground mb-2">
              {t("mcp.unifiedPanel.noServers")}
            </h3>
            <p className="text-muted-foreground text-sm">
              {t("mcp.emptyDescription")}
            </p>
          </div>
        ) : (
          <TooltipProvider delayDuration={300}>
            <>
              <div className="mb-4 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
                <div className="flex flex-wrap items-center gap-2">
                  <div className="mr-2 flex items-center gap-2 rounded-lg border border-border-default px-3 py-1.5">
                    <Label
                      htmlFor="mcp-batch-mode"
                      className="text-xs font-medium"
                    >
                      {t("mcp.batchMode", {
                        defaultValue: "Batch mode",
                      })}
                    </Label>
                    <Switch
                      id="mcp-batch-mode"
                      checked={batchModeEnabled}
                      onCheckedChange={handleModeChange}
                      disabled={isBatchTogglingApps}
                    />
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleSelectAllVisible}
                    disabled={serverEntries.length === 0}
                  >
                    {t("mcp.selectAllVisible", {
                      defaultValue: "Select All Visible",
                    })}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleClearSelection}
                    disabled={selectedCount === 0}
                  >
                    {t("mcp.clearSelection", {
                      defaultValue: "清空选择",
                    })}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleApplyPendingChanges}
                    disabled={
                      !batchModeEnabled ||
                      pendingChangeCount === 0 ||
                      isBatchTogglingApps
                    }
                  >
                    <GitCommitHorizontal size={14} />
                    {t("mcp.applyChanges", {
                      count: pendingChangeCount,
                      defaultValue: `Apply changes (${pendingChangeCount})`,
                    })}
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    onClick={handleBatchDelete}
                    disabled={
                      selectedCount === 0 ||
                      isBatchDeleting ||
                      isBatchTogglingApps
                    }
                  >
                    <Trash2 size={14} />
                    {t("mcp.bulkDelete", {
                      defaultValue: "删除已选（{{count}}）",
                      count: selectedCount,
                    })}
                  </Button>
                </div>
              </div>
              {batchModeEnabled && pendingChangeCount > 0 && (
                <div className="mb-4 rounded-lg border border-sky-200 bg-sky-50 px-3 py-2 text-xs text-sky-700 dark:border-sky-900/40 dark:bg-sky-950/20 dark:text-sky-300">
                  {t("mcp.pendingHint", {
                    count: pendingChangeCount,
                    defaultValue: `${pendingChangeCount} staged app changes are waiting to be applied.`,
                  })}
                </div>
              )}
              <div className="rounded-xl border border-border-default overflow-hidden">
                {serverEntries.map(([id, server], index) => (
                  <UnifiedMcpListItem
                    key={id}
                    id={id}
                    server={server}
                    apps={getEffectiveApps(server)}
                    pendingApps={pendingAppChanges[id]}
                    selected={selected.has(id)}
                    onToggleSelect={() => toggleSelect(id)}
                    onToggleApp={(app, enabled) =>
                      handleAppToggle(server, app, enabled)
                    }
                    onEdit={handleEdit}
                    onDelete={handleDelete}
                    disableAppToggle={isBatchTogglingApps}
                    isLast={index === serverEntries.length - 1}
                  />
                ))}
              </div>
            </>
          </TooltipProvider>
        )}
      </div>

      {isFormOpen && (
        <McpFormModal
          editingId={editingId || undefined}
          initialData={
            editingId && serversMap ? serversMap[editingId] : undefined
          }
          existingIds={serversMap ? Object.keys(serversMap) : []}
          defaultFormat="json"
          onSave={async () => {
            setIsFormOpen(false);
            setEditingId(null);
          }}
          onClose={handleCloseForm}
        />
      )}

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}
    </div>
  );
});

UnifiedMcpPanel.displayName = "UnifiedMcpPanel";

interface UnifiedMcpListItemProps {
  id: string;
  server: McpServer;
  apps: Record<AppId, boolean>;
  pendingApps?: Partial<Record<AppId, boolean>>;
  selected: boolean;
  onToggleSelect: () => void;
  onToggleApp: (app: AppId, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
  disableAppToggle?: boolean;
  isLast?: boolean;
}

const UnifiedMcpListItem: React.FC<UnifiedMcpListItemProps> = ({
  id,
  server,
  apps,
  pendingApps,
  selected,
  onToggleSelect,
  onToggleApp,
  onEdit,
  onDelete,
  disableAppToggle = false,
  isLast,
}) => {
  const { t } = useTranslation();
  const name = server.name || id;
  const description = server.description || "";

  const meta = mcpPresets.find((p) => p.id === id);
  const docsUrl = server.docs || meta?.docs;
  const homepageUrl = server.homepage || meta?.homepage;
  const tags = server.tags || meta?.tags;

  const openDocs = async () => {
    const url = docsUrl || homepageUrl;
    if (!url) return;
    try {
      await settingsApi.openExternal(url);
    } catch {
      // ignore
    }
  };

  return (
    <ListItemRow isLast={isLast}>
      <input
        type="checkbox"
        checked={selected}
        onChange={onToggleSelect}
        aria-label={t("mcp.selectServer", {
          defaultValue: "Select MCP server {{name}}",
          name,
        })}
        className="h-4 w-4 rounded border-border-default"
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="font-medium text-sm text-foreground truncate">
            {name}
          </span>
          {docsUrl && (
            <button
              type="button"
              onClick={openDocs}
              className="text-muted-foreground/60 hover:text-foreground flex-shrink-0"
              title={t("mcp.presets.docs")}
            >
              <ExternalLink size={12} />
            </button>
          )}
        </div>
        {description && (
          <p
            className="text-xs text-muted-foreground truncate"
            title={description}
          >
            {description}
          </p>
        )}
        {!description && tags && tags.length > 0 && (
          <p className="text-xs text-muted-foreground/60 truncate">
            {tags.join(", ")}
          </p>
        )}
      </div>

      <AppToggleGroup
        apps={apps}
        pendingApps={pendingApps}
        onToggle={onToggleApp}
        appIds={MCP_SKILLS_APP_IDS}
        disabled={disableAppToggle}
      />

      <div className="flex items-center gap-0.5 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => onEdit(id)}
          title={t("common.edit")}
        >
          <Edit3 size={14} />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7 hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          onClick={() => onDelete(id)}
          title={t("common.delete")}
        >
          <Trash2 size={14} />
        </Button>
      </div>
    </ListItemRow>
  );
};

export default UnifiedMcpPanel;
