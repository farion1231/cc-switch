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
