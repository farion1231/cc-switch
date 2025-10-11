import { useMemo, useCallback, useState } from "react";
import { Provider, SortConfig, SortField, SortOrder } from "../types";

interface UseProviderSortOptions {
  providers: Provider[];
  defaultSortConfig?: SortConfig;
}

export const useProviderSort = ({
  providers,
  defaultSortConfig = { field: "createdAt", order: "asc" },
}: UseProviderSortOptions) => {
  const [sortConfig, setSortConfig] = useState<SortConfig>(defaultSortConfig);

  // 排序函数
  const sortProviders = useCallback(
    (providerList: Provider[], config: SortConfig): Provider[] => {
      const { field, order } = config;

      return [...providerList].sort((a, b) => {
        let comparison = 0;

        switch (field) {
          case "name":
            // 使用 localeCompare 支持中英文排序
            comparison = a.name.localeCompare(b.name, undefined, {
              sensitivity: "base",
            });
            break;

          case "id":
            comparison = a.id.localeCompare(b.id);
            break;

          case "createdAt":
            comparison = (a.createdAt || 0) - (b.createdAt || 0);
            break;

          case "lastUsed":
            comparison = (a.lastUsedAt || 0) - (b.lastUsedAt || 0);
            break;

          case "priority":
            comparison = (a.priority || 0) - (b.priority || 0);
            break;

          case "contractExpiry":
            comparison = (a.contractExpiry || 0) - (b.contractExpiry || 0);
            break;

          case "custom":
            comparison = (a.customOrder || 0) - (b.customOrder || 0);
            break;

          default:
            comparison = 0;
        }

        return order === "asc" ? comparison : -comparison;
      });
    },
    []
  );

  // 已排序的供应商列表
  const sortedProviders = useMemo(
    () => sortProviders(providers, sortConfig),
    [providers, sortConfig, sortProviders]
  );

  // 更改排序配置
  const changeSortConfig = useCallback(
    (field: SortField, order?: SortOrder) => {
      setSortConfig((prev) => {
        // 如果点击同一字段，切换排序顺序
        if (prev.field === field && !order) {
          return {
            field,
            order: prev.order === "asc" ? "desc" : "asc",
          };
        }

        // 否则使用新字段和指定顺序（默认升序）
        return {
          field,
          order: order || "asc",
        };
      });
    },
    []
  );

  // 切换排序顺序
  const toggleSortOrder = useCallback(() => {
    setSortConfig((prev) => ({
      ...prev,
      order: prev.order === "asc" ? "desc" : "asc",
    }));
  }, []);

  // 重置排序配置
  const resetSortConfig = useCallback(() => {
    setSortConfig(defaultSortConfig);
  }, [defaultSortConfig]);

  return {
    sortConfig,
    sortedProviders,
    changeSortConfig,
    toggleSortOrder,
    resetSortConfig,
    sortProviders, // 暴露排序函数供外部使用
  };
};

// 排序字段的显示名称映射
export const sortFieldLabels: Record<SortField, { zh: string; en: string }> = {
  name: { zh: "名称", en: "Name" },
  id: { zh: "供应商ID", en: "Provider ID" },
  createdAt: { zh: "创建时间", en: "Created At" },
  lastUsed: { zh: "最近使用", en: "Last Used" },
  priority: { zh: "优先级", en: "Priority" },
  contractExpiry: { zh: "合同到期日", en: "Contract Expiry" },
  custom: { zh: "自定义顺序", en: "Custom Order" },
};
