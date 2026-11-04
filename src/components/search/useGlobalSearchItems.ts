import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  useQueries,
  useQuery,
  type UseQueryResult,
} from "@tanstack/react-query";
import type { AppId } from "@/lib/api";
import { mcpApi } from "@/lib/api/mcp";
import { skillsApi } from "@/lib/api/skills";
import { promptsApi } from "@/lib/api";
import { providersQueryOptions } from "@/lib/query";
import { promptKeys } from "@/lib/query/prompts";
import {
  buildMcpItems,
  buildPageItems,
  buildPromptItems,
  buildProviderItems,
  buildSettingItems,
  buildSkillItems,
  type SearchItem,
} from "./searchIndex";

/** 模块级、引用稳定：useQueries 只在某条查询结果变了时才重新合并 */
function combineResults<T>(results: UseQueryResult<T>[]) {
  return {
    data: results.map((result) => result.data),
    loading: results.some(
      (result) => result.isPending && result.fetchStatus === "fetching",
    ),
  };
}

export interface GlobalSearchScope {
  /** 侧栏里显示的应用（按侧栏顺序） */
  apps: AppId[];
  /** 提示词页能选的应用 */
  promptApps: AppId[];
}

/**
 * 全局搜索的全部条目。条目数据读的是各页同一份查询缓存（同 queryKey），
 * 只在搜索框打开时才启用：已经缓存的直接用，没读过的才去读一次。
 */
export function useGlobalSearchItems(
  open: boolean,
  { apps, promptApps }: GlobalSearchScope,
): { items: SearchItem[]; loading: boolean } {
  const { t, i18n } = useTranslation();

  const providerQueries = useQueries({
    queries: apps.map((app) => ({
      ...providersQueryOptions(app),
      enabled: open,
    })),
    combine: combineResults,
  });
  const promptQueries = useQueries({
    queries: promptApps.map((app) => ({
      queryKey: promptKeys.list(app),
      queryFn: () => promptsApi.getPrompts(app),
      enabled: open,
    })),
    combine: combineResults,
  });
  const mcpQuery = useQuery({
    queryKey: ["mcp", "all"],
    queryFn: () => mcpApi.getAllServers(),
    enabled: open,
  });
  const skillsQuery = useQuery({
    queryKey: ["skills", "installed"],
    queryFn: () => skillsApi.getInstalled(),
    enabled: open,
    staleTime: Infinity,
  });

  const appsKey = apps.join(",");
  const staticItems = useMemo(
    () => [...buildPageItems(t, apps), ...buildSettingItems(t)],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, i18n.language, appsKey],
  );

  const items = useMemo(() => {
    const result = [...staticItems];
    apps.forEach((app, index) => {
      result.push(
        ...buildProviderItems(app, providerQueries.data[index]?.providers),
      );
    });
    result.push(...buildMcpItems(mcpQuery.data));
    result.push(...buildSkillItems(skillsQuery.data));
    promptApps.forEach((app, index) => {
      result.push(...buildPromptItems(app, promptQueries.data[index]));
    });
    return result;
  }, [
    staticItems,
    apps,
    promptApps,
    providerQueries.data,
    promptQueries.data,
    mcpQuery.data,
    skillsQuery.data,
  ]);

  const loading =
    open &&
    (providerQueries.loading ||
      promptQueries.loading ||
      [mcpQuery, skillsQuery].some(
        (query) => query.isPending && query.fetchStatus === "fetching",
      ));

  return { items, loading };
}
