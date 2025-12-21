import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { failoverApi } from "@/lib/api/failover";

// ========== 熔断器 Hooks ==========

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

// ========== 故障转移队列 Hooks（新） ==========

/**
 * 获取故障转移队列
 */
export function useFailoverQueue(appType: string) {
  return useQuery({
    queryKey: ["failoverQueue", appType],
    queryFn: () => failoverApi.getFailoverQueue(appType),
    enabled: !!appType,
  });
}

/**
 * 获取可添加到队列的供应商
 */
export function useAvailableProvidersForFailover(appType: string) {
  return useQuery({
    queryKey: ["availableProvidersForFailover", appType],
    queryFn: () => failoverApi.getAvailableProvidersForFailover(appType),
    enabled: !!appType,
  });
}

/**
 * 添加供应商到故障转移队列
 */
export function useAddToFailoverQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => failoverApi.addToFailoverQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForFailover", variables.appType],
      });
    },
  });
}

/**
 * 从故障转移队列移除供应商
 */
export function useRemoveFromFailoverQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => failoverApi.removeFromFailoverQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForFailover", variables.appType],
      });
    },
  });
}

/**
 * 重新排序故障转移队列
 */
export function useReorderFailoverQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerIds,
    }: {
      appType: string;
      providerIds: string[];
    }) => failoverApi.reorderFailoverQueue(appType, providerIds),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
    },
  });
}

/**
 * 设置故障转移队列项的启用状态
 * 使用乐观更新(Optimistic Update)以提供即时反馈
 */
export function useSetFailoverItemEnabled() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
      enabled,
    }: {
      appType: string;
      providerId: string;
      enabled: boolean;
    }) => failoverApi.setFailoverItemEnabled(appType, providerId, enabled),

    // 乐观更新：立即更新缓存中的数据
    onMutate: async (variables) => {
      // 取消正在进行的查询，防止覆盖乐观更新
      await queryClient.cancelQueries({
        queryKey: ["failoverQueue", variables.appType],
      });

      // 保存之前的数据以便回滚
      const previousQueue = queryClient.getQueryData<
        import("@/types/proxy").FailoverQueueItem[]
      >(["failoverQueue", variables.appType]);

      // 乐观地更新缓存
      if (previousQueue) {
        queryClient.setQueryData<import("@/types/proxy").FailoverQueueItem[]>(
          ["failoverQueue", variables.appType],
          previousQueue.map((item) =>
            item.providerId === variables.providerId
              ? { ...item, enabled: variables.enabled }
              : item,
          ),
        );
      }

      // 返回上下文供 onError 使用
      return { previousQueue };
    },

    // 错误时回滚
    onError: (_error, variables, context) => {
      if (context?.previousQueue) {
        queryClient.setQueryData(
          ["failoverQueue", variables.appType],
          context.previousQueue,
        );
      }
    },

    // 无论成功失败，都重新获取最新数据以确保一致性
    onSettled: (_, __, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
    },
  });
}

// ========== 自动故障转移总开关 Hooks ==========

/**
 * 获取自动故障转移总开关状态
 */
export function useAutoFailoverEnabled() {
  return useQuery({
    queryKey: ["autoFailoverEnabled"],
    queryFn: () => failoverApi.getAutoFailoverEnabled(),
    // 默认值为 false（与后端保持一致）
    placeholderData: false,
  });
}

/**
 * 设置自动故障转移总开关状态
 */
export function useSetAutoFailoverEnabled() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (enabled: boolean) =>
      failoverApi.setAutoFailoverEnabled(enabled),

    // 乐观更新
    onMutate: async (enabled) => {
      await queryClient.cancelQueries({ queryKey: ["autoFailoverEnabled"] });
      const previousValue = queryClient.getQueryData<boolean>([
        "autoFailoverEnabled",
      ]);

      queryClient.setQueryData(["autoFailoverEnabled"], enabled);

      return { previousValue };
    },

    // 错误时回滚
    onError: (_error, _enabled, context) => {
      if (context?.previousValue !== undefined) {
        queryClient.setQueryData(
          ["autoFailoverEnabled"],
          context.previousValue,
        );
      }
    },

    // 无论成功失败，都重新获取
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["autoFailoverEnabled"] });
    },
  });
}
