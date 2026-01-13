import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import type { LogFilters } from "@/types/usage";

// Query keys
export const usageKeys = {
  all: ["usage"] as const,
  summary: (days: number) => [...usageKeys.all, "summary", days] as const,
  trends: (days: number) => [...usageKeys.all, "trends", days] as const,
  providerStats: () => [...usageKeys.all, "provider-stats"] as const,
  modelStats: () => [...usageKeys.all, "model-stats"] as const,
  logs: (filters: LogFilters, page: number, pageSize: number) =>
    [...usageKeys.all, "logs", filters, page, pageSize] as const,
  detail: (requestId: string) =>
    [...usageKeys.all, "detail", requestId] as const,
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
export function useUsageSummary(days: number) {
  return useQuery({
    queryKey: usageKeys.summary(days),
    queryFn: () => {
      const { startDate, endDate } = getWindow(days);
      return usageApi.getUsageSummary(startDate, endDate);
    },
    refetchInterval: 30000, // 每30秒自动刷新
    refetchIntervalInBackground: false, // 后台不刷新
  });
}

export function useUsageTrends(days: number) {
  return useQuery({
    queryKey: usageKeys.trends(days),
    queryFn: () => {
      const { startDate, endDate } = getWindow(days);
      return usageApi.getUsageTrends(startDate, endDate);
    },
    refetchInterval: 30000, // 每30秒自动刷新
    refetchIntervalInBackground: false,
  });
}

export function useProviderStats() {
  return useQuery({
    queryKey: usageKeys.providerStats(),
    queryFn: usageApi.getProviderStats,
    refetchInterval: 30000, // 每30秒自动刷新
    refetchIntervalInBackground: false,
  });
}

export function useModelStats() {
  return useQuery({
    queryKey: usageKeys.modelStats(),
    queryFn: usageApi.getModelStats,
    refetchInterval: 30000, // 每30秒自动刷新
    refetchIntervalInBackground: false,
  });
}

export function useRequestLogs(
  filters: LogFilters,
  page: number = 0,
  pageSize: number = 20,
) {
  return useQuery({
    queryKey: usageKeys.logs(filters, page, pageSize),
    queryFn: () => usageApi.getRequestLogs(filters, page, pageSize),
    refetchInterval: 30000, // 每30秒自动刷新
    refetchIntervalInBackground: false,
  });
}

export function useRequestDetail(requestId: string) {
  return useQuery({
    queryKey: usageKeys.detail(requestId),
    queryFn: () => usageApi.getRequestDetail(requestId),
    enabled: !!requestId,
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
