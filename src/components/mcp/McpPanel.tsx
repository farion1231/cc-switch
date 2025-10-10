import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { X, Plus, Server } from "lucide-react";
import { McpServer } from "../../types";
import McpListItem from "./McpListItem";
import McpFormModal from "./McpFormModal";
import { ConfirmDialog } from "../ConfirmDialog";
import { extractErrorMessage } from "../../utils/errorUtils";
import { mcpPresets } from "../../config/mcpPresets";
import McpToggle from "./McpToggle";
import { buttonStyles, cardStyles, cn } from "../../lib/styles";
import { AppType } from "../../lib/tauri-api";

interface McpPanelProps {
  onClose: () => void;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
  appType: AppType;
}

/**
 * MCP 管理面板
 * 采用与主界面一致的设计风格，右上角添加按钮，每个 MCP 占一行
 */
const McpPanel: React.FC<McpPanelProps> = ({ onClose, onNotify, appType }) => {
  const { t } = useTranslation();
  const [servers, setServers] = useState<Record<string, McpServer>>({});
  const [loading, setLoading] = useState(true);
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);

  const reload = async () => {
    setLoading(true);
    try {
      const cfg = await window.api.getMcpConfig(appType);
      setServers(cfg.servers || {});
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    const setup = async () => {
      try {
        // 初始化导入：按应用类型从对应客户端导入已有 MCP（设为 enabled=true）
        if (appType === "claude") {
          await window.api.importMcpFromClaude();
        } else if (appType === "codex") {
          await window.api.importMcpFromCodex();
        }

        // 读取现有 config.json 内容
        const cfg = await window.api.getMcpConfig(appType);
        const existing = cfg.servers || {};

        // 将预设落库为禁用（若缺失）
        const missing = mcpPresets.filter((p) => !existing[p.id]);
        for (const p of missing) {
          const seed: McpServer = {
            ...(p.server as McpServer),
            enabled: false,
            source: "preset",
          } as unknown as McpServer;
          await window.api.upsertMcpServerInConfig(appType, p.id, seed);
        }
      } catch (e) {
        console.warn("MCP 初始化导入/落库失败（忽略继续）", e);
      } finally {
        await reload();
      }
    };
    setup();
    // appType 改变时重新初始化
  }, [appType]);

  const handleToggle = async (id: string, enabled: boolean) => {
    try {
      const server = servers[id];
      if (!server) {
        const preset = mcpPresets.find((p) => p.id === id);
        if (!preset) return;
        await window.api.upsertMcpServerInConfig(
          appType,
          id,
          preset.server as McpServer,
        );
      }
      await window.api.setMcpEnabled(appType, id, enabled);
      await reload();
      onNotify?.(
        enabled ? t("mcp.msg.enabled") : t("mcp.msg.disabled"),
        "success",
        1500,
      );
    } catch (e: any) {
      const detail = extractErrorMessage(e);
      onNotify?.(
        detail || t("mcp.error.saveFailed"),
        "error",
        detail ? 6000 : 5000,
      );
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

  const handleDelete = (id: string) => {
    setConfirmDialog({
      isOpen: true,
      title: t("mcp.confirm.deleteTitle"),
      message: t("mcp.confirm.deleteMessage", { id }),
      onConfirm: async () => {
        try {
          await window.api.deleteMcpServerInConfig(appType, id);
          await reload();
          setConfirmDialog(null);
          onNotify?.(t("mcp.msg.deleted"), "success", 1500);
        } catch (e: any) {
          const detail = extractErrorMessage(e);
          onNotify?.(
            detail || t("mcp.error.deleteFailed"),
            "error",
            detail ? 6000 : 5000,
          );
        }
      },
    });
  };

  const handleSave = async (id: string, server: McpServer) => {
    try {
      await window.api.upsertMcpServerInConfig(appType, id, server);
      await reload();
      setIsFormOpen(false);
      setEditingId(null);
      onNotify?.(t("mcp.msg.saved"), "success", 1500);
    } catch (e: any) {
      const detail = extractErrorMessage(e);
      onNotify?.(
        detail || t("mcp.error.saveFailed"),
        "error",
        detail ? 6000 : 5000,
      );
      // 继续抛出错误，让表单层可以给到直观反馈（避免被更高层遮挡）
      throw e;
    }
  };

  const handleCloseForm = () => {
    setIsFormOpen(false);
    setEditingId(null);
  };

  const serverEntries = useMemo(() => Object.entries(servers), [servers]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Panel */}
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-lg max-w-3xl w-full mx-4 overflow-hidden flex flex-col max-h-[85vh] min-h-[600px]">
        {/* Header */}
        <div className="flex-shrink-0 flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-800">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {t("mcp.title")} ·{" "}
            {t(appType === "claude" ? "apps.claude" : "apps.codex")}
          </h3>

          <div className="flex items-center gap-3">
            <button
              onClick={handleAdd}
              className={`inline-flex items-center gap-2 ${buttonStyles.mcp}`}
            >
              <Plus size={16} />
              {t("mcp.add")}
            </button>
            <button
              onClick={onClose}
              className="p-1 text-gray-500 hover:text-gray-900 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
            >
              <X size={18} />
            </button>
          </div>
        </div>

        {/* Info Section */}
        <div className="flex-shrink-0 px-6 pt-4 pb-2">
          <div className="text-sm text-gray-500 dark:text-gray-400">
            {t("mcp.serverCount", { count: Object.keys(servers).length })}
          </div>
        </div>

        {/* Content - Scrollable */}
        <div className="flex-1 overflow-y-auto px-6 py-4">
          {loading ? (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              {t("mcp.loading")}
            </div>
          ) : (
            (() => {
              const notInstalledPresets = mcpPresets.filter(
                (p) => !servers[p.id],
              );
              const hasAny =
                serverEntries.length > 0 || notInstalledPresets.length > 0;
              if (!hasAny) {
                return (
                  <div className="text-center py-12">
                    <div className="w-16 h-16 mx-auto mb-4 bg-gray-100 dark:bg-gray-800 rounded-full flex items-center justify-center">
                      <Server
                        size={24}
                        className="text-gray-400 dark:text-gray-500"
                      />
                    </div>
                    <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
                      {t("mcp.empty")}
                    </h3>
                    <p className="text-gray-500 dark:text-gray-400 text-sm">
                      {t("mcp.emptyDescription")}
                    </p>
                  </div>
                );
              }

              return (
                <div className="space-y-3">
                  {/* 已安装 */}
                  {serverEntries.map(([id, server]) => (
                    <McpListItem
                      key={`installed-${id}`}
                      id={id}
                      server={server}
                      onToggle={handleToggle}
                      onEdit={handleEdit}
                      onDelete={handleDelete}
                    />
                  ))}

                  {/* 预设（未安装） */}
                  {notInstalledPresets.map((p) => {
                    return (
                      <div
                        key={`preset-${p.id}`}
                        className={cn(
                          cardStyles.interactive,
                          "!p-4 opacity-95",
                        )}
                      >
                        <div className="flex items-center gap-4">
                          <div className="flex-shrink-0">
                            <McpToggle
                              enabled={false}
                              onChange={(en) => handleToggle(p.id, en)}
                            />
                          </div>
                          <div className="flex-1 min-w-0">
                            <h3 className="font-medium text-gray-900 dark:text-gray-100 mb-1">
                              {p.id}
                            </h3>
                            {p.description && (
                              <p className="text-sm text-gray-500 dark:text-gray-400 truncate">
                                {p.description}
                              </p>
                            )}
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              );
            })()
          )}
        </div>
      </div>

      {/* Form Modal */}
      {isFormOpen && (
        <McpFormModal
          editingId={editingId || undefined}
          initialData={editingId ? servers[editingId] : undefined}
          existingIds={Object.keys(servers)}
          onSave={handleSave}
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
};

export default McpPanel;
