import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { usageApi } from "@/lib/api/usage";
import type { LogFilters } from "@/types/usage";

const DEFAULT_REFETCH_INTERVAL_MS = 30000;

type UsageQueryOptions = {
  refetchInterval?: number | false;
  refetchIntervalInBackground?: boolean;
  refetchOnWindowFocus?: boolean;
};

type RequestLogsTimeMode = "rolling" | "fixed";

type RequestLogsQueryArgs = {
  filters: LogFilters;
  timeMode: RequestLogsTimeMode;
  page?: number;
  pageSize?: number;
  rollingWindowSeconds?: number;
  refreshCounter?: number; // 用于强制刷新
};

type RequestLogsKey = {
  timeMode: RequestLogsTimeMode;
  rollingWindowSeconds?: number;
  appType?: string;
  providerName?: string;
  providerId?: string;
  model?: string;
  statusCode?: number;
  statusFilter?: "success" | "error";
  startDate?: number;
  endDate?: number;
  refreshCounter?: number; // 用于强制刷新
};

// Query keys
export const usageKeys = {
  all: ["usage"] as const,
  summary: (days: number) => [...usageKeys.all, "summary", days] as const,
  trends: (days: number) => [...usageKeys.all, "trends", days] as const,
  providerStats: () => [...usageKeys.all, "provider-stats"] as const,
  modelStats: () => [...usageKeys.all, "model-stats"] as const,
  logs: (key: RequestLogsKey, page: number, pageSize: number) =>
    [
      ...usageKeys.all,
      "logs",
      key.timeMode,
      key.rollingWindowSeconds ?? 0,
      key.appType ?? "",
      key.providerName ?? "",
      key.providerId ?? "",
      key.model ?? "",
      key.statusCode ?? -1,
      key.statusFilter ?? "",
      key.startDate ?? 0,
      key.endDate ?? 0,
      key.refreshCounter ?? 0,
      page,
      pageSize,
    ] as const,
  detail: (requestId: string) =>
    [...usageKeys.all, "detail", requestId] as const,
  filters: (startDate?: number, endDate?: number) =>
    [...usageKeys.all, "filters", startDate, endDate] as const,
  pricing: () => [...usageKeys.all, "pricing"] as const,
  limits: (providerId: string, appType: string) =>
    [...usageKeys.all, "limits", providerId, appType] as const,
};

const getWindow = (days: number) => {
  const endDate = Math.floor(Date.now() / 1000);
  const startDate = endDate - days * 24 * 60 * 60;
  return { startDate, endDate };
};

// Hooks
export function useUsageSummary(days: number, options?: UsageQueryOptions) {
  return useQuery({
    queryKey: usageKeys.summary(days),
    queryFn: () => {
      const { startDate, endDate } = getWindow(days);
      return usageApi.getUsageSummary(startDate, endDate);
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS, // 每30秒自动刷新
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false, // 后台不刷新
  });
}

export function useUsageTrends(days: number, options?: UsageQueryOptions) {
  return useQuery({
    queryKey: usageKeys.trends(days),
    queryFn: () => {
      const { startDate, endDate } = getWindow(days);
      return usageApi.getUsageTrends(startDate, endDate);
    },
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useProviderStats(options?: UsageQueryOptions) {
  return useQuery({
    queryKey: usageKeys.providerStats(),
    queryFn: usageApi.getProviderStats,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS, // 每30秒自动刷新
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

export function useModelStats(options?: UsageQueryOptions) {
  return useQuery({
    queryKey: usageKeys.modelStats(),
    queryFn: usageApi.getModelStats,
    refetchInterval: options?.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options?.refetchIntervalInBackground ?? false,
  });
}

const getRollingRange = (windowSeconds: number) => {
  const endDate = Math.floor(Date.now() / 1000);
  const startDate = endDate - windowSeconds;
  return { startDate, endDate };
};

export function useRequestLogs({
  filters,
  timeMode,
  page = 0,
  pageSize = 20,
  rollingWindowSeconds = 24 * 60 * 60,
  refreshCounter = 0,
}: RequestLogsQueryArgs) {
  // 构建一个简单的查询键，确保 refreshCounter 变化时创建新查询
  const queryKey = useMemo(() => {
    const baseKey = [
      "usage",
      "logs",
      timeMode,
      rollingWindowSeconds,
      filters.appType || "",
      filters.providerId || "",
      filters.model || "",
      filters.statusFilter || "",
      filters.startDate || 0,
      filters.endDate || 0,
      page,
      pageSize,
    ];
    // 添加 refreshCounter 确保每次刷新都是新查询
    return [...baseKey, refreshCounter] as const;
  }, [
    timeMode,
    rollingWindowSeconds,
    filters.appType,
    filters.providerId,
    filters.model,
    filters.statusFilter,
    filters.startDate,
    filters.endDate,
    page,
    pageSize,
    refreshCounter,
  ]);

  return useQuery({
    queryKey,
    queryFn: async () => {
      const effectiveFilters =
        timeMode === "rolling"
          ? { ...filters, ...getRollingRange(rollingWindowSeconds) }
          : filters;
      const result = await usageApi.getRequestLogs(effectiveFilters, page, pageSize);
      return result;
    },
    staleTime: 0,
    gcTime: 1000 * 60 * 5,
    refetchOnMount: true,
    refetchOnWindowFocus: false,
    refetchInterval: false,
    networkMode: "always",
  });
}

export function useRequestDetail(requestId: string) {
  return useQuery({
    queryKey: usageKeys.detail(requestId),
    queryFn: () => usageApi.getRequestDetail(requestId),
    enabled: !!requestId,
  });
}

export function useAvailableFilters(startDate?: number, endDate?: number) {
  return useQuery({
    queryKey: usageKeys.filters(startDate, endDate),
    queryFn: () => usageApi.getAvailableFilters(startDate, endDate),
    staleTime: 0,
  });
}

export function useModelPricing() {
  return useQuery({
    queryKey: usageKeys.pricing(),
    queryFn: usageApi.getModelPricing,
  });
}

export function useProviderLimits(providerId: string, appType: string) {
  return useQuery({
    queryKey: usageKeys.limits(providerId, appType),
    queryFn: () => usageApi.checkProviderLimits(providerId, appType),
    enabled: !!providerId && !!appType,
  });
}

export function useUpdateModelPricing() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (params: {
      modelId: string;
      displayName: string;
      inputCost: string;
      outputCost: string;
      cacheReadCost: string;
      cacheCreationCost: string;
    }) =>
      usageApi.updateModelPricing(
        params.modelId,
        params.displayName,
        params.inputCost,
        params.outputCost,
        params.cacheReadCost,
        params.cacheCreationCost,
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.pricing() });
    },
  });
}

export function useDeleteModelPricing() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (modelId: string) => usageApi.deleteModelPricing(modelId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.pricing() });
    },
  });
}
