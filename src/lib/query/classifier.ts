import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { classifierApi } from "@/lib/api/classifier";
import type { ClassifierConfig, ClassifierQueueItem } from "@/types/proxy";

const DEFAULT_CONFIG: ClassifierConfig = {
  enabled: false,
  forceThinkingOff: true,
};

/**
 * 获取分类器队列
 */
export function useClassifierQueue(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["classifierQueue", appType],
    queryFn: () => classifierApi.getClassifierQueue(appType),
    enabled: enabled && !!appType,
  });
}

/**
 * 获取可添加到分类器队列的供应商
 */
export function useAvailableProvidersForClassifier(appType: string) {
  return useQuery({
    queryKey: ["availableProvidersForClassifier", appType],
    queryFn: () => classifierApi.getAvailableProvidersForClassifier(appType),
    enabled: !!appType,
  });
}

/**
 * 读取分类器队列的两个开关
 */
export function useClassifierConfig(appType: string) {
  return useQuery({
    queryKey: ["classifierConfig", appType],
    queryFn: () => classifierApi.getClassifierConfig(appType),
    enabled: !!appType,
    placeholderData: DEFAULT_CONFIG,
  });
}

/**
 * 写入分类器队列的两个开关（乐观更新 + 失败回滚）
 *
 * 不失效 providers / proxyStatus：分类器队列不切换当前供应商，
 * 也不改变任何代理运行状态。
 */
export function useSetClassifierConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      config,
    }: {
      appType: string;
      config: ClassifierConfig;
    }) => classifierApi.setClassifierConfig(appType, config),
    onMutate: async ({ appType, config }) => {
      await queryClient.cancelQueries({
        queryKey: ["classifierConfig", appType],
      });
      const previous = queryClient.getQueryData<ClassifierConfig>([
        "classifierConfig",
        appType,
      ]);
      queryClient.setQueryData(["classifierConfig", appType], config);
      return { previous };
    },
    onError: (_error, variables, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          ["classifierConfig", variables.appType],
          context.previous,
        );
      }
    },
    onSettled: (_data, _error, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["classifierConfig", variables.appType],
      });
    },
  });
}

/**
 * 添加供应商到分类器队列
 */
export function useAddToClassifierQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => classifierApi.addToClassifierQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["classifierQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForClassifier", variables.appType],
      });
    },
  });
}

/**
 * 从分类器队列移除供应商
 */
export function useRemoveFromClassifierQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => classifierApi.removeFromClassifierQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["classifierQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["availableProvidersForClassifier", variables.appType],
      });
    },
  });
}

/**
 * 按拖拽结果重排分类器队列（乐观更新 + 失败回滚）
 *
 * 只碰 classifierQueue 这一个 key：分类器队列有自己的排序列，
 * 与首页 / 托盘 / 故障转移队列的顺序完全无关。
 */
export function useReorderClassifierQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerIds,
    }: {
      appType: string;
      providerIds: string[];
    }) => classifierApi.reorderClassifierQueue(appType, providerIds),
    onMutate: async ({ appType, providerIds }) => {
      const key = ["classifierQueue", appType];
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<ClassifierQueueItem[]>(key);

      if (previous) {
        const byId = new Map(previous.map((item) => [item.providerId, item]));
        // 只重排认识的 id，并把 providerIds 里没提到的成员留在末尾，
        // 与后端 reorder 的宽容语义保持一致
        const reordered = providerIds
          .map((id) => byId.get(id))
          .filter((item): item is ClassifierQueueItem => item !== undefined);
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
          ["classifierQueue", variables.appType],
          context.previous,
        );
      }
    },
    onSettled: (_data, _error, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["classifierQueue", variables.appType],
      });
    },
  });
}

/**
 * 设置队列条目的出站模型名覆写
 */
export function useSetClassifierModel() {
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
    }) => classifierApi.setClassifierModel(appType, providerId, model),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["classifierQueue", variables.appType],
      });
    },
  });
}
