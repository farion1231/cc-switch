import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { auxiliaryApi } from "@/lib/api/auxiliary";
import type { AuxiliaryConfig, AuxiliaryQueueItem } from "@/types/proxy";

const DEFAULT_CONFIG: AuxiliaryConfig = {
  enabled: false,
};

/**
 * 获取辅助请求队列
 */
export function useAuxiliaryQueue(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["auxiliaryQueue", appType],
    queryFn: () => auxiliaryApi.getAuxiliaryQueue(appType),
    enabled: enabled && !!appType,
  });
}

/**
 * 获取可添加到辅助请求队列的供应商
 */
export function useAvailableProvidersForAuxiliary(appType: string) {
  return useQuery({
    queryKey: ["availableProvidersForAuxiliary", appType],
    queryFn: () => auxiliaryApi.getAvailableProvidersForAuxiliary(appType),
    enabled: !!appType,
  });
}

/**
 * 读取辅助请求队列的两个开关
 */
export function useAuxiliaryConfig(appType: string) {
  return useQuery({
    queryKey: ["auxiliaryConfig", appType],
    queryFn: () => auxiliaryApi.getAuxiliaryConfig(appType),
    enabled: !!appType,
    placeholderData: DEFAULT_CONFIG,
  });
}

/**
 * 写入辅助请求队列的两个开关（乐观更新 + 失败回滚）
 *
 * 不失效 providers / proxyStatus：辅助请求队列不切换当前供应商，
 * 也不改变任何代理运行状态。
 */
export function useSetAuxiliaryConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      config,
    }: {
      appType: string;
      config: AuxiliaryConfig;
    }) => auxiliaryApi.setAuxiliaryConfig(appType, config),
    onMutate: async ({ appType, config }) => {
      await queryClient.cancelQueries({
        queryKey: ["auxiliaryConfig", appType],
      });
      const previous = queryClient.getQueryData<AuxiliaryConfig>([
        "auxiliaryConfig",
        appType,
      ]);
      queryClient.setQueryData(["auxiliaryConfig", appType], config);
      return { previous };
    },
    onError: (_error, variables, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          ["auxiliaryConfig", variables.appType],
          context.previous,
        );
      }
    },
    onSettled: (_data, _error, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["auxiliaryConfig", variables.appType],
      });
    },
  });
}

/**
 * 添加供应商到辅助请求队列
 */
export function useAddToAuxiliaryQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => auxiliaryApi.addToAuxiliaryQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["auxiliaryQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForAuxiliary", variables.appType],
      });
    },
  });
}

/**
 * 从辅助请求队列移除供应商
 */
export function useRemoveFromAuxiliaryQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => auxiliaryApi.removeFromAuxiliaryQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["auxiliaryQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForAuxiliary", variables.appType],
      });
    },
  });
}

/**
 * 按拖拽结果重排辅助请求队列（乐观更新 + 失败回滚）
 *
 * 只碰 auxiliaryQueue 这一个 key：辅助请求队列有自己的排序列，
 * 与首页 / 托盘 / 故障转移队列的顺序完全无关。
 */
export function useReorderAuxiliaryQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerIds,
    }: {
      appType: string;
      providerIds: string[];
    }) => auxiliaryApi.reorderAuxiliaryQueue(appType, providerIds),
    onMutate: async ({ appType, providerIds }) => {
      const key = ["auxiliaryQueue", appType];
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<AuxiliaryQueueItem[]>(key);

      if (previous) {
        const byId = new Map(previous.map((item) => [item.providerId, item]));
        // 只重排认识的 id，并把 providerIds 里没提到的成员留在末尾，
        // 与后端 reorder 的宽容语义保持一致
        const reordered = providerIds
          .map((id) => byId.get(id))
          .filter((item): item is AuxiliaryQueueItem => item !== undefined);
        const named = new Set(providerIds);
        queryClient.setQueryData(key, [
          ...reordered,
          ...previous.filter((item) => !named.has(item.providerId)),
        ]);
      }

      return { previous };
    },
    onError: (_error, variables, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          ["auxiliaryQueue", variables.appType],
          context.previous,
        );
      }
    },
    onSettled: (_data, _error, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["auxiliaryQueue", variables.appType],
      });
    },
  });
}

/**
 * 设置队列条目的出站模型名覆写
 */
export function useSetAuxiliaryModel() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
      model,
    }: {
      appType: string;
      providerId: string;
      model: string | null;
    }) => auxiliaryApi.setAuxiliaryModel(appType, providerId, model),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["auxiliaryQueue", variables.appType],
      });
    },
  });
}
