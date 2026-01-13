import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
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
import { Edit3, Trash2 } from "lucide-react";
import { settingsApi } from "@/lib/api";
import { mcpPresets } from "@/config/mcpPresets";
import { toast } from "sonner";

interface UnifiedMcpPanelProps {
  onOpenChange: (open: boolean) => void;
}

/**
 * 统一 MCP 管理面板
 * v3.7.0 新架构：所有 MCP 服务器统一管理，每个服务器通过复选框控制应用到哪些客户端
 */
export interface UnifiedMcpPanelHandle {
  openAdd: () => void;
  openImport: () => void;
}

const UnifiedMcpPanel = React.forwardRef<
  UnifiedMcpPanelHandle,
  UnifiedMcpPanelProps
>(({ onOpenChange: _onOpenChange }, ref) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);

  // Queries and Mutations
  const { data: serversMap, isLoading } = useAllMcpServers();
  const toggleAppMutation = useToggleMcpApp();
  const deleteServerMutation = useDeleteMcpServer();
  const importMutation = useImportMcpFromApps();

  // Convert serversMap to array for easier rendering
  const serverEntries = useMemo((): Array<[string, McpServer]> => {
    if (!serversMap) return [];
    return Object.entries(serversMap);
  }, [serversMap]);

  // Count enabled servers per app
  const enabledCounts = useMemo(() => {
    const counts = { claude: 0, codex: 0, gemini: 0 };
    serverEntries.forEach(([_, server]) => {
      if (server.apps.claude) counts.claude++;
      if (server.apps.codex) counts.codex++;
      if (server.apps.gemini) counts.gemini++;
    });
    return counts;
  }, [serverEntries]);

  const handleToggleApp = async (
    serverId: string,
    app: AppId,
    enabled: boolean,
  ) => {
    try {
      await toggleAppMutation.mutateAsync({ serverId, app, enabled });
    } catch (error) {
      toast.error(t("common.error"), {
        description: String(error),
      });
    }
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
      toast.error(t("common.error"), {
        description: String(error),
      });
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
          toast.error(t("common.error"), {
            description: String(error),
          });
        }
      },
    });
  };

  const handleCloseForm = () => {
    setIsFormOpen(false);
    setEditingId(null);
  };

  return (
    <div className="mx-auto max-w-[56rem] px-6 flex flex-col h-[calc(100vh-8rem)] overflow-hidden">
      {/* Info Section */}
      <div className="flex-shrink-0 py-4 glass rounded-xl border border-white/10 mb-4 px-6">
        <div className="text-sm text-muted-foreground">
          {t("mcp.serverCount", { count: serverEntries.length })} ·{" "}
          {t("mcp.unifiedPanel.apps.claude")}: {enabledCounts.claude} ·{" "}
          {t("mcp.unifiedPanel.apps.codex")}: {enabledCounts.codex} ·{" "}
          {t("mcp.unifiedPanel.apps.gemini")}: {enabledCounts.gemini}
        </div>
      </div>

      {/* Content - Scrollable */}
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
          <div className="space-y-3">
            {serverEntries.map(([id, server]) => (
              <UnifiedMcpListItem
                key={id}
                id={id}
                server={server}
                onToggleApp={handleToggleApp}
                onEdit={handleEdit}
                onDelete={handleDelete}
              />
            ))}
          </div>
        )}
      </div>

      {/* Form Modal */}
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

      {/* Confirm Dialog */}
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

/**
 * 统一 MCP 列表项组件
 * 展示服务器名称、描述，以及三个应用的复选框
 */
interface UnifiedMcpListItemProps {
  id: string;
  server: McpServer;
  onToggleApp: (serverId: string, app: AppId, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

const UnifiedMcpListItem: React.FC<UnifiedMcpListItemProps> = ({
  id,
  server,
  onToggleApp,
  onEdit,
  onDelete,
}) => {
  const { t } = useTranslation();
  const name = server.name || id;
  const description = server.description || "";

  // 匹配预设元信息
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
    <div className="group relative flex items-center gap-4 p-4 rounded-xl border border-border-default bg-muted/50 hover:bg-muted hover:border-border-default/80 hover:shadow-sm transition-all duration-300">
      {/* 左侧：服务器信息 */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <h3 className="font-medium text-foreground">{name}</h3>
          {docsUrl && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={openDocs}
              title={t("mcp.presets.docs")}
            >
              {t("mcp.presets.docs")}
            </Button>
          )}
        </div>
        {description && (
          <p className="text-sm text-muted-foreground line-clamp-2">
            {description}
          </p>
        )}
        {!description && tags && tags.length > 0 && (
          <p className="text-xs text-muted-foreground/70 truncate">
            {tags.join(", ")}
          </p>
        )}
      </div>

      {/* 中间：应用开关 */}
      <div className="flex flex-col gap-2 flex-shrink-0 min-w-[120px]">
        <div className="flex items-center justify-between gap-3">
          <label
            htmlFor={`${id}-claude`}
            className="text-sm text-foreground/80 cursor-pointer"
          >
            {t("mcp.unifiedPanel.apps.claude")}
          </label>
          <Switch
            id={`${id}-claude`}
            checked={server.apps.claude}
            onCheckedChange={(checked: boolean) =>
              onToggleApp(id, "claude", checked)
            }
          />
        </div>

        <div className="flex items-center justify-between gap-3">
          <label
            htmlFor={`${id}-codex`}
            className="text-sm text-foreground/80 cursor-pointer"
          >
            {t("mcp.unifiedPanel.apps.codex")}
          </label>
          <Switch
            id={`${id}-codex`}
            checked={server.apps.codex}
            onCheckedChange={(checked: boolean) =>
              onToggleApp(id, "codex", checked)
            }
          />
        </div>

        <div className="flex items-center justify-between gap-3">
          <label
            htmlFor={`${id}-gemini`}
            className="text-sm text-foreground/80 cursor-pointer"
          >
            {t("mcp.unifiedPanel.apps.gemini")}
          </label>
          <Switch
            id={`${id}-gemini`}
            checked={server.apps.gemini}
            onCheckedChange={(checked: boolean) =>
              onToggleApp(id, "gemini", checked)
            }
          />
        </div>
      </div>

      {/* 右侧：操作按钮 */}
      <div className="flex items-center gap-2 flex-shrink-0">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={() => onEdit(id)}
          title={t("common.edit")}
        >
          <Edit3 size={16} />
        </Button>

        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={() => onDelete(id)}
          className="hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10"
          title={t("common.delete")}
        >
          <Trash2 size={16} />
        </Button>
      </div>
    </div>
  );
};

export default UnifiedMcpPanel;
