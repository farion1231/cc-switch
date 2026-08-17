import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { classifierApi } from "@/lib/api/classifier";
import type { ClassifierConfig } from "@/types/proxy";

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
