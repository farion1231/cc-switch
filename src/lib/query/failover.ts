import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { failoverApi } from "@/lib/api/failover";
import { toast } from "@/lib/toast";
import { useTranslation } from "react-i18next";
import { extractErrorMessage } from "@/utils/errorUtils";
import { proxyKeys } from "@/lib/query/proxy";
import { getAppLabel } from "@/config/appConfig";

// ========== 熔断器 Hooks ==========

/**
 * 获取供应商健康状态
 */
export function useProviderHealth(
  providerId: string,
  appType: string,
  enabled = true,
) {
  return useQuery({
    queryKey: ["providerHealth", providerId, appType],
    queryFn: () => failoverApi.getProviderHealth(providerId, appType),
    enabled: enabled && !!providerId && !!appType,
    refetchInterval: 5000, // 每 5 秒刷新一次
    retry: false,
  });
}

/**
 * 重置熔断器
 *
 * 重置后后端会检查是否应该切回优先级更高的供应商，
 * 因此需要同时刷新供应商列表和代理状态。
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
      // 刷新熔断器统计：重置会清空失败证据，不刷新的话界面上那几条
      // 证据会继续挂在一个已经被清掉的状态旁边
      queryClient.invalidateQueries({
        queryKey: [
          "circuitBreakerStats",
          variables.providerId,
          variables.appType,
        ],
      });
      // 刷新供应商列表（因为可能发生了自动恢复切换）
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
      // 刷新代理状态（更新 active_targets）
      queryClient.invalidateQueries({
        queryKey: proxyKeys.status,
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
 * 获取熔断器统计信息（含失败证据）
 *
 * `enabled=false` 时完全不发请求：代理没在跑的时候后端只会返回 null，
 * 让每个供应商卡片各打一次空转的调用没有意义。
 */
export function useCircuitBreakerStats(
  providerId: string,
  appType: string,
  enabled = true,
) {
  return useQuery({
    queryKey: ["circuitBreakerStats", providerId, appType],
    queryFn: () => failoverApi.getCircuitBreakerStats(providerId, appType),
    enabled: enabled && !!providerId && !!appType,
    refetchInterval: 5000, // 每 5 秒刷新一次
  });
}

/**
 * 当前在飞请求明细。
 *
 * 与熔断器统计不同，这个不需要 provider/app 参数：一次拿全部，前端按
 * app/模型筛选就地过滤。2s 轮询与代理状态一致。
 */
export function useInFlightRequests(enabled = true) {
  return useQuery({
    queryKey: ["inFlightRequests"],
    queryFn: () => failoverApi.getInFlightRequests(),
    enabled,
    refetchInterval: 2000,
  });
}

// ========== 故障转移队列 Hooks（新） ==========

/**
 * 获取故障转移队列
 */
export function useFailoverQueue(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["failoverQueue", appType],
    queryFn: () => failoverApi.getFailoverQueue(appType),
    enabled: enabled && !!appType,
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
  const { t } = useTranslation();

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
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
    },
    onError: (error: Error) => {
      toast.error(
        t("failover.queueAddFailed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    },
  });
}

/**
 * 从故障转移队列移除供应商
 */
export function useRemoveFromFailoverQueue() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

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
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
      // 清除该供应商的健康状态缓存（退出队列后不再需要健康监控）
      queryClient.invalidateQueries({
        queryKey: ["providerHealth", variables.providerId, variables.appType],
      });
      // 清除该供应商的熔断器统计缓存
      queryClient.invalidateQueries({
        queryKey: [
          "circuitBreakerStats",
          variables.providerId,
          variables.appType,
        ],
      });
    },
    onError: (error: Error) => {
      toast.error(
        t("failover.queueRemoveFailed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    },
  });
}

// ========== 自动故障转移开关 Hooks ==========

/**
 * 获取指定应用的自动故障转移开关状态
 */
export function useAutoFailoverEnabled(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["autoFailoverEnabled", appType],
    queryFn: () => failoverApi.getAutoFailoverEnabled(appType),
    enabled: enabled && !!appType,
    // 默认值为 false（与后端保持一致）
    placeholderData: false,
  });
}

/**
 * 设置指定应用的自动故障转移开关状态
 */
export function useSetAutoFailoverEnabled() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      failoverApi.setAutoFailoverEnabled(appType, enabled),

    // 乐观更新
    onMutate: async ({ appType, enabled }) => {
      await queryClient.cancelQueries({
        queryKey: ["autoFailoverEnabled", appType],
      });
      const previousValue = queryClient.getQueryData<boolean>([
        "autoFailoverEnabled",
        appType,
      ]);

      queryClient.setQueryData(["autoFailoverEnabled", appType], enabled);

      return { previousValue, appType };
    },

    onSuccess: (_data, variables) => {
      const appLabel = getAppLabel(variables.appType);

      toast.success(
        variables.enabled
          ? t("failover.enabled", {
              app: appLabel,
              defaultValue: `${appLabel} 故障转移已启用`,
            })
          : t("failover.disabled", {
              app: appLabel,
              defaultValue: `${appLabel} 故障转移已关闭`,
            }),
        { closeButton: true },
      );
    },

    // 错误时回滚
    onError: (error: Error, _variables, context) => {
      if (context?.previousValue !== undefined) {
        queryClient.setQueryData(
          ["autoFailoverEnabled", context.appType],
          context.previousValue,
        );
      }

      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("failover.toggleFailed", {
          detail,
          defaultValue: `操作失败: ${detail}`,
        }),
      );
    },

    // 无论成功失败，都重新获取
    onSettled: (_, __, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["autoFailoverEnabled", variables.appType],
      });
      // 启用/关闭故障转移可能触发：
      // - 立即切到队列 P1（当前供应商变化）
      // - 队列为空时自动把当前供应商加入队列（队列内容变化）
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForFailover", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: proxyKeys.status,
      });
    },
  });
}
