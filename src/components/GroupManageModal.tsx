import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ProviderGroup } from "../types";
import { X, Plus, Edit3, Trash2, FolderPlus } from "lucide-react";
import { cn, buttonStyles, cardStyles } from "../lib/styles";

interface GroupManageModalProps {
  isOpen: boolean;
  onClose: () => void;
  groups: Record<string, ProviderGroup>;
  groupsOrder: string[];
  onCreateGroup: (name: string, options?: { color?: string; icon?: string }) => Promise<void>;
  onUpdateGroup: (groupId: string, updates: Partial<ProviderGroup>) => Promise<void>;
  onDeleteGroup: (groupId: string) => Promise<void>;
}

const GROUP_COLORS = [
  { name: "蓝色", value: "#3b82f6", class: "bg-blue-500" },
  { name: "绿色", value: "#10b981", class: "bg-green-500" },
  { name: "黄色", value: "#f59e0b", class: "bg-yellow-500" },
  { name: "红色", value: "#ef4444", class: "bg-red-500" },
  { name: "紫色", value: "#8b5cf6", class: "bg-purple-500" },
  { name: "粉色", value: "#ec4899", class: "bg-pink-500" },
  { name: "灰色", value: "#6b7280", class: "bg-gray-500" },
];

export const GroupManageModal: React.FC<GroupManageModalProps> = ({
  isOpen,
  onClose,
  groups,
  groupsOrder,
  onCreateGroup,
  onUpdateGroup,
  onDeleteGroup,
}) => {
  const { t } = useTranslation();
  const [newGroupName, setNewGroupName] = useState("");
  const [selectedColor, setSelectedColor] = useState(GROUP_COLORS[0].value);
  const [editingGroupId, setEditingGroupId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");

  useEffect(() => {
    if (!isOpen) {
      setNewGroupName("");
      setEditingGroupId(null);
      setEditingName("");
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const handleCreateGroup = async () => {
    if (!newGroupName.trim()) return;

    await onCreateGroup(newGroupName.trim(), { color: selectedColor });
    setNewGroupName("");
    setSelectedColor(GROUP_COLORS[0].value);
  };

  const handleStartEdit = (group: ProviderGroup) => {
    setEditingGroupId(group.id);
    setEditingName(group.name);
  };

  const handleSaveEdit = async () => {
    if (!editingGroupId || !editingName.trim()) return;

    await onUpdateGroup(editingGroupId, { name: editingName.trim() });
    setEditingGroupId(null);
    setEditingName("");
  };

  const handleCancelEdit = () => {
    setEditingGroupId(null);
    setEditingName("");
  };

  const sortedGroups = groupsOrder
    .map((id) => groups[id])
    .filter((g) => g !== undefined);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-gray-900 rounded-lg shadow-xl w-full max-w-2xl max-h-[80vh] flex flex-col">
        {/* 头部 */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <FolderPlus className="h-6 w-6 text-blue-500" />
            <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
              分组管理
            </h2>
          </div>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
          >
            <X className="h-6 w-6" />
          </button>
        </div>

        {/* 内容 */}
        <div className="flex-1 overflow-y-auto p-6">
          {/* 创建新分组 */}
          <div className="mb-6 p-4 bg-blue-50 dark:bg-blue-900/20 rounded-lg border border-blue-200 dark:border-blue-800">
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              创建新分组
            </h3>
            <div className="flex flex-col gap-3">
              <input
                type="text"
                value={newGroupName}
                onChange={(e) => setNewGroupName(e.target.value)}
                onKeyPress={(e) => e.key === "Enter" && handleCreateGroup()}
                placeholder="输入分组名称..."
                className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
              />

              {/* 颜色选择 */}
              <div className="flex items-center gap-2">
                <span className="text-sm text-gray-600 dark:text-gray-400">颜色:</span>
                <div className="flex gap-2">
                  {GROUP_COLORS.map((color) => (
                    <button
                      key={color.value}
                      onClick={() => setSelectedColor(color.value)}
                      className={cn(
                        "w-8 h-8 rounded-full border-2 transition-all",
                        color.class,
                        selectedColor === color.value
                          ? "border-gray-900 dark:border-white scale-110"
                          : "border-transparent opacity-60 hover:opacity-100"
                      )}
                      title={color.name}
                    />
                  ))}
                </div>
              </div>

              <button
                onClick={handleCreateGroup}
                disabled={!newGroupName.trim()}
                className={cn(
                  buttonStyles.primary,
                  "w-full",
                  !newGroupName.trim() && "opacity-50 cursor-not-allowed"
                )}
              >
                <Plus className="h-4 w-4" />
                创建分组
              </button>
            </div>
          </div>

          {/* 分组列表 */}
          <div className="space-y-3">
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-2">
              现有分组 ({sortedGroups.length})
            </h3>

            {sortedGroups.length === 0 ? (
              <div className="text-center py-8 text-gray-500 dark:text-gray-400">
                暂无分组，请创建第一个分组
              </div>
            ) : (
              sortedGroups.map((group) => (
                <div
                  key={group.id}
                  className={cn(
                    cardStyles.base,
                    "flex items-center gap-3 p-4"
                  )}
                >
                  {/* 颜色标记 */}
                  {group.color && (
                    <div
                      className="w-4 h-4 rounded-full flex-shrink-0"
                      style={{ backgroundColor: group.color }}
                    />
                  )}

                  {/* 分组名称 */}
                  {editingGroupId === group.id ? (
                    <input
                      type="text"
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onKeyPress={(e) => {
                        if (e.key === "Enter") handleSaveEdit();
                        if (e.key === "Escape") handleCancelEdit();
                      }}
                      onBlur={handleSaveEdit}
                      autoFocus
                      className="flex-1 px-2 py-1 border border-blue-500 rounded bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 focus:outline-none"
                    />
                  ) : (
                    <span className="flex-1 text-gray-900 dark:text-gray-100 font-medium">
                      {group.name}
                    </span>
                  )}

                  {/* 供应商数量 */}
                  <span className="text-sm text-gray-500 dark:text-gray-400">
                    {group.providerIds.length} 个供应商
                  </span>

                  {/* 操作按钮 */}
                  <div className="flex items-center gap-2">
                    {editingGroupId !== group.id && (
                      <>
                        <button
                          onClick={() => handleStartEdit(group)}
                          className={buttonStyles.icon}
                          title="编辑"
                        >
                          <Edit3 className="h-4 w-4" />
                        </button>
                        <button
                          onClick={() => onDeleteGroup(group.id)}
                          className={cn(
                            buttonStyles.icon,
                            "text-gray-500 hover:text-red-500 hover:bg-red-100 dark:text-gray-400 dark:hover:text-red-400 dark:hover:bg-red-500/10"
                          )}
                          title="删除"
                        >
                          <Trash2 className="h-4 w-4" />
                        </button>
                      </>
                    )}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        {/* 底部 */}
        <div className="flex justify-end p-6 border-t border-gray-200 dark:border-gray-700">
          <button onClick={onClose} className={buttonStyles.secondary}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
};
