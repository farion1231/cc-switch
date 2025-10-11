import { useState, useEffect, useCallback } from "react";
import { ProviderGroup, Provider } from "../types";
import { AppType } from "../lib/tauri-api";

interface UseProviderGroupsOptions {
  appType: AppType;
  providers: Record<string, Provider>;
  onNotify?: (message: string, type: "success" | "error", duration?: number) => void;
}

export const useProviderGroups = ({
  appType,
  providers,
  onNotify,
}: UseProviderGroupsOptions) => {
  const [groups, setGroups] = useState<Record<string, ProviderGroup>>({});
  const [groupsOrder, setGroupsOrder] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);

  // 加载分组
  const loadGroups = useCallback(async () => {
    setLoading(true);
    try {
      const loadedGroups = await window.api.getGroups(appType);
      setGroups(loadedGroups);

      // 按 order 字段排序分组
      const sortedIds = Object.values(loadedGroups)
        .sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
        .map((g) => g.id);
      setGroupsOrder(sortedIds);
    } catch (error) {
      console.error("加载分组失败:", error);
      onNotify?.("加载分组失败", "error");
    } finally {
      setLoading(false);
    }
  }, [appType, onNotify]);

  useEffect(() => {
    loadGroups();
  }, [loadGroups]);

  // 创建分组
  const createGroup = useCallback(
    async (
      name: string,
      options?: {
        color?: string;
        icon?: string;
        parentId?: string;
      }
    ): Promise<ProviderGroup | null> => {
      try {
        const newGroup = await window.api.createGroup(
          {
            name,
            color: options?.color,
            icon: options?.icon,
            parentId: options?.parentId,
            providerIds: [],
            order: groupsOrder.length,
          },
          appType
        );

        setGroups((prev) => ({ ...prev, [newGroup.id]: newGroup }));
        setGroupsOrder((prev) => [...prev, newGroup.id]);

        onNotify?.(`分组"${name}"创建成功`, "success");
        return newGroup;
      } catch (error) {
        console.error("创建分组失败:", error);
        onNotify?.("创建分组失败", "error");
        return null;
      }
    },
    [appType, groupsOrder.length, onNotify]
  );

  // 更新分组
  const updateGroup = useCallback(
    async (groupId: string, updates: Partial<ProviderGroup>): Promise<boolean> => {
      try {
        const group = groups[groupId];
        if (!group) {
          throw new Error("分组不存在");
        }

        const updatedGroup: ProviderGroup = {
          ...group,
          ...updates,
          updatedAt: Date.now(),
        };

        const success = await window.api.updateGroup(updatedGroup, appType);
        if (success) {
          setGroups((prev) => ({ ...prev, [groupId]: updatedGroup }));
          onNotify?.("分组更新成功", "success");
        }
        return success;
      } catch (error) {
        console.error("更新分组失败:", error);
        onNotify?.("更新分组失败", "error");
        return false;
      }
    },
    [appType, groups, onNotify]
  );

  // 删除分组
  const deleteGroup = useCallback(
    async (groupId: string): Promise<boolean> => {
      try {
        const group = groups[groupId];
        if (!group) {
          throw new Error("分组不存在");
        }

        // 确认删除
        const confirmed = window.confirm(
          `确定要删除分组"${group.name}"吗？分组内的供应商将被移至"未分组"。`
        );
        if (!confirmed) return false;

        const success = await window.api.deleteGroup(groupId, appType);
        if (success) {
          setGroups((prev) => {
            const newGroups = { ...prev };
            delete newGroups[groupId];
            return newGroups;
          });
          setGroupsOrder((prev) => prev.filter((id) => id !== groupId));
          onNotify?.("分组删除成功", "success");
        }
        return success;
      } catch (error) {
        console.error("删除分组失败:", error);
        onNotify?.("删除分组失败", "error");
        return false;
      }
    },
    [appType, groups, onNotify]
  );

  // 切换分组折叠状态
  const toggleGroupCollapsed = useCallback(
    async (groupId: string): Promise<void> => {
      const group = groups[groupId];
      if (!group) return;

      await updateGroup(groupId, { collapsed: !group.collapsed });
    },
    [groups, updateGroup]
  );

  // 添加供应商到分组
  const addProviderToGroup = useCallback(
    async (providerId: string, groupId: string): Promise<boolean> => {
      try {
        const success = await window.api.addProviderToGroup(
          providerId,
          groupId,
          appType
        );

        if (success) {
          setGroups((prev) => {
            const group = prev[groupId];
            if (!group) return prev;

            return {
              ...prev,
              [groupId]: {
                ...group,
                providerIds: [...group.providerIds, providerId],
                updatedAt: Date.now(),
              },
            };
          });

          const providerName = providers[providerId]?.name || providerId;
          const groupName = groups[groupId]?.name || groupId;
          onNotify?.(
            `已将"${providerName}"添加到分组"${groupName}"`,
            "success"
          );
        }
        return success;
      } catch (error) {
        console.error("添加供应商到分组失败:", error);
        onNotify?.("添加供应商到分组失败", "error");
        return false;
      }
    },
    [appType, providers, groups, onNotify]
  );

  // 从分组移除供应商
  const removeProviderFromGroup = useCallback(
    async (providerId: string): Promise<boolean> => {
      try {
        const success = await window.api.removeProviderFromGroup(
          providerId,
          appType
        );

        if (success) {
          setGroups((prev) => {
            const newGroups = { ...prev };
            Object.keys(newGroups).forEach((groupId) => {
              const group = newGroups[groupId];
              if (group.providerIds.includes(providerId)) {
                newGroups[groupId] = {
                  ...group,
                  providerIds: group.providerIds.filter(
                    (id) => id !== providerId
                  ),
                  updatedAt: Date.now(),
                };
              }
            });
            return newGroups;
          });

          const providerName = providers[providerId]?.name || providerId;
          onNotify?.(`已将"${providerName}"移出分组`, "success");
        }
        return success;
      } catch (error) {
        console.error("从分组移除供应商失败:", error);
        onNotify?.("从分组移除供应商失败", "error");
        return false;
      }
    },
    [appType, providers, onNotify]
  );

  // 更新分组顺序
  const updateGroupsOrderState = useCallback(
    async (newOrder: string[]): Promise<boolean> => {
      try {
        const success = await window.api.updateGroupsOrder(newOrder, appType);
        if (success) {
          setGroupsOrder(newOrder);
          onNotify?.("分组顺序已更新", "success", 2000);
        }
        return success;
      } catch (error) {
        console.error("更新分组顺序失败:", error);
        onNotify?.("更新分组顺序失败", "error");
        return false;
      }
    },
    [appType, onNotify]
  );

  // 获取未分组的供应商
  const getUngroupedProviders = useCallback((): Provider[] => {
    const groupedProviderIds = new Set<string>();
    Object.values(groups).forEach((group) => {
      group.providerIds.forEach((id) => groupedProviderIds.add(id));
    });

    return Object.values(providers).filter(
      (provider) => !groupedProviderIds.has(provider.id)
    );
  }, [providers, groups]);

  // 按分组获取供应商
  const getProvidersByGroup = useCallback(
    (groupId: string): Provider[] => {
      const group = groups[groupId];
      if (!group) return [];

      return group.providerIds
        .map((id) => providers[id])
        .filter((p) => p !== undefined);
    },
    [groups, providers]
  );

  return {
    groups,
    groupsOrder,
    loading,
    createGroup,
    updateGroup,
    deleteGroup,
    toggleGroupCollapsed,
    addProviderToGroup,
    removeProviderFromGroup,
    updateGroupsOrder: updateGroupsOrderState,
    getUngroupedProviders,
    getProvidersByGroup,
    reloadGroups: loadGroups,
  };
};
