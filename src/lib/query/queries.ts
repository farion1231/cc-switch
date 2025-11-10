import {
  useQuery,
  type UseQueryResult,
  keepPreviousData,
} from "@tanstack/react-query";
import { providersApi, settingsApi, usageApi, type AppId } from "@/lib/api";
import type { Provider, Settings, UsageResult } from "@/types";

const sortProviders = (
  providers: Record<string, Provider>,
): Record<string, Provider> => {
  const sortedEntries = Object.values(providers)
    .sort((a, b) => {
      const indexA = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
      const indexB = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
      if (indexA !== indexB) {
        return indexA - indexB;
      }

      const timeA = a.createdAt ?? 0;
      const timeB = b.createdAt ?? 0;
      if (timeA === timeB) {
        return a.name.localeCompare(b.name, "zh-CN");
      }
      return timeA - timeB;
    })
    .map((provider) => [provider.id, provider] as const);

  return Object.fromEntries(sortedEntries);
};

export interface ProvidersQueryData {
  providers: Record<string, Provider>;
  currentProviderId: string;
}

export const useProvidersQuery = (
  appId: AppId,
): UseQueryResult<ProvidersQueryData> => {
  return useQuery({
    queryKey: ["providers", appId],
    placeholderData: keepPreviousData,
    queryFn: async () => {
      let providers: Record<string, Provider> = {};
      let currentProviderId = "";

      try {
        providers = await providersApi.getAll(appId);
      } catch (error) {
        console.error("获取供应商列表失败:", error);
      }

      try {
        currentProviderId = await providersApi.getCurrent(appId);
      } catch (error) {
        console.error("获取当前供应商失败:", error);
      }

      if (Object.keys(providers).length === 0) {
        try {
          const success = await providersApi.importDefault(appId);
          if (success) {
            providers = await providersApi.getAll(appId);
            currentProviderId = await providersApi.getCurrent(appId);
          }
        } catch (error) {
          console.error("导入默认配置失败:", error);
        }
      }

      return {
        providers: sortProviders(providers),
        currentProviderId,
      };
    },
  });
};

export const useSettingsQuery = (): UseQueryResult<Settings> => {
  return useQuery({
    queryKey: ["settings"],
    queryFn: async () => settingsApi.get(),
  });
};

export interface UseUsageQueryOptions {
  enabled?: boolean;
  autoQueryInterval?: number; // 自动查询间隔（分钟），0 表示禁用
}

export const useUsageQuery = (
  providerId: string,
  appId: AppId,
  options?: UseUsageQueryOptions,
) => {
  const { enabled = true, autoQueryInterval = 0 } = options || {};

  const query = useQuery<UsageResult>({
    queryKey: ["usage", providerId, appId],
    queryFn: async () => usageApi.query(providerId, appId),
    enabled: enabled && !!providerId,
    refetchInterval:
      autoQueryInterval > 0
        ? Math.max(autoQueryInterval, 1) * 60 * 1000 // 最小1分钟
        : false,
    refetchIntervalInBackground: true, // 后台也继续定时查询
    refetchOnWindowFocus: false,
    retry: false,
    staleTime: 0, // 不使用缓存策略，确保 refetchInterval 准确执行
  });

  return {
    ...query,
    lastQueriedAt: query.dataUpdatedAt || null,
  };
};
