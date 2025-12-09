import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { failoverApi } from "@/lib/api/failover";

/**
 * 获取代理目标列表
 */
export function useProxyTargets(appType: string) {
  return useQuery({
    queryKey: ["proxyTargets", appType],
    queryFn: () => failoverApi.getProxyTargets(appType),
    enabled: !!appType,
  });
}

/**
 * 获取供应商健康状态
 */
export function useProviderHealth(providerId: string, appType: string) {
  return useQuery({
    queryKey: ["providerHealth", providerId, appType],
    queryFn: () => failoverApi.getProviderHealth(providerId, appType),
    enabled: !!providerId && !!appType,
    refetchInterval: 5000, // 每 5 秒刷新一次
    retry: false,
  });
}

/**
 * 设置代理目标
 */
export function useSetProxyTarget() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      providerId,
      appType,
      enabled,
    }: {
      providerId: string;
      appType: string;
      enabled: boolean;
    }) => failoverApi.setProxyTarget(providerId, appType, enabled),
    onSuccess: (_, variables) => {
      // 刷新代理目标列表
      queryClient.invalidateQueries({
        queryKey: ["proxyTargets", variables.appType],
      });
      // 刷新供应商列表
      queryClient.invalidateQueries({ queryKey: ["providers"] });
      // 刷新健康状态
      queryClient.invalidateQueries({
        queryKey: ["providerHealth", variables.providerId, variables.appType],
      });
    },
  });
}

/**
 * 重置熔断器
 */
export function useResetCircuitBreaker() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      providerId,
      appType,
    }: {
      providerId: string;
      appType: string;
    }) => failoverApi.resetCircuitBreaker(providerId, appType),
    onSuccess: (_, variables) => {
      // 刷新健康状态
      queryClient.invalidateQueries({
        queryKey: ["providerHealth", variables.providerId, variables.appType],
      });
    },
  });
}

/**
 * 获取熔断器配置
 */
export function useCircuitBreakerConfig() {
  return useQuery({
    queryKey: ["circuitBreakerConfig"],
    queryFn: () => failoverApi.getCircuitBreakerConfig(),
  });
}

/**
 * 更新熔断器配置
 */
export function useUpdateCircuitBreakerConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: failoverApi.updateCircuitBreakerConfig,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["circuitBreakerConfig"] });
    },
  });
}

/**
 * 获取熔断器统计信息
 */
export function useCircuitBreakerStats(providerId: string, appType: string) {
  return useQuery({
    queryKey: ["circuitBreakerStats", providerId, appType],
    queryFn: () => failoverApi.getCircuitBreakerStats(providerId, appType),
    enabled: !!providerId && !!appType,
    refetchInterval: 5000, // 每 5 秒刷新一次
  });
}
