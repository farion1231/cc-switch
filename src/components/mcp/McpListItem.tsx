import React from "react";
import { useTranslation } from "react-i18next";
import { Edit3, Trash2 } from "lucide-react";
import { McpServer } from "../../types";
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

  // 只显示 description，没有则留空
  const description = (server as any).description || "";

  return (
    <div className={cn(cardStyles.interactive, "!p-4")}>
      <div className="flex items-center gap-4">
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
            {id}
          </h3>
          {description && (
            <p className="text-sm text-gray-500 dark:text-gray-400 truncate">
              {description}
            </p>
          )}
        </div>

        {/* 右侧：操作按钮 */}
        <div className="flex items-center gap-2 flex-shrink-0">
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
