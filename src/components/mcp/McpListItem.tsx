import React from "react";
import { useTranslation } from "react-i18next";
import { Edit3, Trash2 } from "lucide-react";
import { McpServer } from "../../types";
import { mcpPresets } from "../../config/mcpPresets";
import { cardStyles, buttonStyles, cn } from "../../lib/styles";
import McpToggle from "./McpToggle";

interface McpListItemProps {
  id: string;
  server: McpServer;
  onToggle: (id: string, enabled: boolean) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

/**
 * MCP 列表项组件
 * 每个 MCP 占一行，左侧是 Toggle 开关，中间是名称和详细信息，右侧是编辑和删除按钮
 */
const McpListItem: React.FC<McpListItemProps> = ({
  id,
  server,
  onToggle,
  onEdit,
  onDelete,
}) => {
  const { t } = useTranslation();

  // 仅当显式为 true 时视为启用；避免 undefined 被误判为启用
  const enabled = server.enabled === true;
  const name = server.name || id;

  // 只显示 description，没有则留空
  const description = server.description || "";

  // 匹配预设元信息（用于展示文档链接等）
  const meta = mcpPresets.find((p) => p.id === id);
  const docsUrl = server.docs || meta?.docs;
  const homepageUrl = server.homepage || meta?.homepage;
  const tags = server.tags || meta?.tags;

  const openDocs = async () => {
    const url = docsUrl || homepageUrl;
    if (!url) return;
    try {
      await window.api.openExternal(url);
    } catch {
      // ignore
    }
  };

  return (
    <div className={cn(cardStyles.interactive, "!p-4 h-16")}>
      <div className="flex items-center gap-4 h-full">
        {/* 左侧：Toggle 开关 */}
        <div className="flex-shrink-0">
          <McpToggle
            enabled={enabled}
            onChange={(newEnabled) => onToggle(id, newEnabled)}
          />
        </div>

        {/* 中间：名称和详细信息 */}
        <div className="flex-1 min-w-0">
          <h3 className="font-medium text-gray-900 dark:text-gray-100 mb-1">
            {name}
          </h3>
          {description && (
            <p className="text-sm text-gray-500 dark:text-gray-400 truncate">
              {description}
            </p>
          )}
          {!description && tags && tags.length > 0 && (
            <p className="text-xs text-gray-400 dark:text-gray-500 truncate">
              {tags.join(", ")}
            </p>
          )}
          {/* 预设标记已移除 */}
        </div>

        {/* 右侧：操作按钮 */}
        <div className="flex items-center gap-2 flex-shrink-0">
          {docsUrl && (
            <button
              onClick={openDocs}
              className={buttonStyles.ghost}
              title={t("mcp.presets.docs")}
            >
              {t("mcp.presets.docs")}
            </button>
          )}
          <button
            onClick={() => onEdit(id)}
            className={buttonStyles.icon}
            title={t("common.edit")}
          >
            <Edit3 size={16} />
          </button>

          <button
            onClick={() => onDelete(id)}
            className={cn(
              buttonStyles.icon,
              "hover:text-red-500 hover:bg-red-100 dark:hover:text-red-400 dark:hover:bg-red-500/10",
            )}
            title={t("common.delete")}
          >
            <Trash2 size={16} />
          </button>
        </div>
      </div>
    </div>
  );
};

export default McpListItem;
