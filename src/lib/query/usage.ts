import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { usageApi } from '@/lib/api/usage';
import type { LogFilters } from '@/types/usage';

// Query keys
export const usageKeys = {
  all: ['usage'] as const,
  summary: (startDate?: number, endDate?: number) =>
    [...usageKeys.all, 'summary', startDate, endDate] as const,
  trends: (days: number) => [...usageKeys.all, 'trends', days] as const,
  providerStats: () => [...usageKeys.all, 'provider-stats'] as const,
  modelStats: () => [...usageKeys.all, 'model-stats'] as const,
  logs: (filters: LogFilters, limit: number, offset: number) =>
    [...usageKeys.all, 'logs', filters, limit, offset] as const,
  detail: (requestId: string) =>
    [...usageKeys.all, 'detail', requestId] as const,
  pricing: () => [...usageKeys.all, 'pricing'] as const,
  limits: (providerId: string, appType: string) =>
    [...usageKeys.all, 'limits', providerId, appType] as const,
};

// Hooks
export function useUsageSummary(startDate?: number, endDate?: number) {
  return useQuery({
    queryKey: usageKeys.summary(startDate, endDate),
    queryFn: () => usageApi.getUsageSummary(startDate, endDate),
  });
}

export function useUsageTrends(days: number) {
  return useQuery({
    queryKey: usageKeys.trends(days),
    queryFn: () => usageApi.getUsageTrends(days),
  });
}

export function useProviderStats() {
  return useQuery({
    queryKey: usageKeys.providerStats(),
    queryFn: usageApi.getProviderStats,
  });
}

export function useModelStats() {
  return useQuery({
    queryKey: usageKeys.modelStats(),
    queryFn: usageApi.getModelStats,
  });
}

export function useRequestLogs(
  filters: LogFilters,
  limit: number = 50,
  offset: number = 0
) {
  return useQuery({
    queryKey: usageKeys.logs(filters, limit, offset),
    queryFn: () => usageApi.getRequestLogs(filters, limit, offset),
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
        params.cacheCreationCost
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: usageKeys.pricing() });
    },
  });
}
