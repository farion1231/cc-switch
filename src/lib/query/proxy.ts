import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { proxyApi } from "@/lib/api/proxy";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { GlobalProxyConfig, AppProxyConfig } from "@/types/proxy";

// ========== 代理服务器状态 Hooks ==========

/**
 * 获取代理服务器状态
 */
export function useProxyStatus() {
  return useQuery({
    queryKey: ["proxyStatus"],
    queryFn: () => proxyApi.getProxyStatus(),
    refetchInterval: 5000, // 每 5 秒刷新一次
  });
}

/**
 * 检查代理服务器是否运行
 */
export function useIsProxyRunning() {
  return useQuery({
    queryKey: ["proxyRunning"],
    queryFn: () => proxyApi.isProxyRunning(),
    refetchInterval: 2000,
  });
}

/**
 * 检查是否处于接管模式
 */
export function useIsLiveTakeoverActive() {
  return useQuery({
    queryKey: ["liveTakeoverActive"],
    queryFn: () => proxyApi.isLiveTakeoverActive(),
    refetchInterval: 2000,
  });
}

/**
 * 获取各应用接管状态
 */
export function useProxyTakeoverStatus() {
  return useQuery({
    queryKey: ["proxyTakeoverStatus"],
    queryFn: () => proxyApi.getProxyTakeoverStatus(),
    refetchInterval: 2000,
  });
}

// ========== 代理服务器控制 Hooks ==========

/**
 * 启动代理服务器
 */
export function useStartProxyServer() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.startProxyServer(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyRunning"] });
      queryClient.invalidateQueries({ queryKey: ["liveTakeoverActive"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
    },
  });
}

/**
 * 停止代理服务器
 */
export function useStopProxyServer() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.stopProxyWithRestore(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyRunning"] });
      queryClient.invalidateQueries({ queryKey: ["liveTakeoverActive"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
    },
  });
}

/**
 * 设置应用接管状态
 */
export function useSetProxyTakeoverForApp() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      proxyApi.setProxyTakeoverForApp(appType, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
      queryClient.invalidateQueries({ queryKey: ["liveTakeoverActive"] });
    },
  });
}

/**
 * 代理模式下切换供应商
 */
export function useSwitchProxyProvider() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => proxyApi.switchProxyProvider(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
    },
    onError: (error: Error) => {
      toast.error(t("proxy.switchFailed", { error: error.message }));
    },
  });
}

// ========== Legacy 代理配置 Hooks (兼容) ==========

/**
 * 获取代理配置（旧版）
 */
export function useProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const { data: config, isLoading } = useQuery({
    queryKey: ["proxyConfig"],
    queryFn: () => proxyApi.getProxyConfig(),
  });

  const updateMutation = useMutation({
    mutationFn: proxyApi.updateProxyConfig,
    onSuccess: () => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });

  return {
    config,
    isLoading,
    updateConfig: updateMutation.mutateAsync,
    isUpdating: updateMutation.isPending,
  };
}

// ========== v3+ 全局/应用级配置 Hooks ==========

/**
 * 获取全局代理配置
 */
export function useGlobalProxyConfig() {
  return useQuery({
    queryKey: ["globalProxyConfig"],
    queryFn: () => proxyApi.getGlobalProxyConfig(),
  });
}

/**
 * 更新全局代理配置
 */
export function useUpdateGlobalProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (config: GlobalProxyConfig) =>
      proxyApi.updateGlobalProxyConfig(config),
    onSuccess: () => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({ queryKey: ["globalProxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });
}

/**
 * 获取指定应用的代理配置
 */
export function useAppProxyConfig(appType: string) {
  return useQuery({
    queryKey: ["appProxyConfig", appType],
    queryFn: () => proxyApi.getProxyConfigForApp(appType),
    enabled: !!appType,
  });
}

/**
 * 更新指定应用的代理配置
 */
export function useUpdateAppProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (config: AppProxyConfig) =>
      proxyApi.updateProxyConfigForApp(config),
    onSuccess: (_, variables) => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({
        queryKey: ["appProxyConfig", variables.appType],
      });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["circuitBreakerConfig"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });
}
