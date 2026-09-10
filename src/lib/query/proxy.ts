import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { proxyApi } from "@/lib/api/proxy";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type {
  GlobalProxyConfig,
  AppProxyConfig,
  ProxyTakeoverStatus,
} from "@/types/proxy";

export const proxyKeys = {
  status: ["proxyStatus"] as const,
  takeoverStatus: ["proxyTakeoverStatus"] as const,
  globalConfig: ["globalProxyConfig"] as const,
  appConfig: (appType: string) => ["appProxyConfig", appType] as const,
};

// ========== 代理服务器状态 Hooks ==========

/**
 * 获取代理服务器状态
 */
export function useProxyStatusQuery() {
  return useQuery({
    queryKey: proxyKeys.status,
    queryFn: () => proxyApi.getProxyStatus(),
    // 仅在服务运行时轮询
    refetchInterval: (query) => (query.state.data?.running ? 2000 : false),
    // 保持之前的数据，避免闪烁
    placeholderData: (previousData) => previousData,
  });
}

/**
 * 获取各应用接管状态
 */
export function useProxyTakeoverStatus(poll = true) {
  return useQuery({
    queryKey: proxyKeys.takeoverStatus,
    queryFn: () => proxyApi.getProxyTakeoverStatus(),
    refetchInterval: poll ? 2000 : false,
    ...(poll
      ? {}
      : {
          placeholderData: (previousData: ProxyTakeoverStatus | undefined) =>
            previousData,
        }),
  });
}

// ========== 代理服务器控制 Hooks ==========

/**
 * 设置应用接管状态
 */
export function useSetProxyTakeoverForApp() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      proxyApi.setProxyTakeoverForApp(appType, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus });
    },
  });
}

// ========== 公网路由 Hooks ==========

/**
 * 获取公网路由状态（隧道运行时轮询，用于拿到公网地址）
 */
export function usePublicRouteStatus() {
  return useQuery({
    queryKey: ["publicRouteStatus"],
    queryFn: () => proxyApi.getPublicRouteStatus(),
    // 启用期间持续轮询，等待隧道就绪（公网地址出现）
    refetchInterval: (query) => (query.state.data?.enabled ? 2000 : false),
    placeholderData: (previousData) => previousData,
  });
}

/**
 * 保存公网路由隧道配置（模式 + 命名隧道参数）
 */
export function useSetPublicRouteTunnelConfig() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (config: {
      mode: string;
      namedTunnel?: string | null;
      namedHostname?: string | null;
    }) => proxyApi.setPublicRouteTunnelConfig(config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["publicRouteStatus"] });
    },
  });
}

/**
 * 列出 Cloudflare 账户下已有的命名隧道
 */
export function useListNamedTunnels(enabled: boolean) {
  return useQuery({
    queryKey: ["namedTunnels"],
    queryFn: () => proxyApi.listNamedTunnels(),
    enabled,
    retry: false,
    staleTime: 60_000,
  });
}

/**
 * 启用公网路由
 */
export function useEnablePublicRoute() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.enablePublicRoute(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["publicRouteStatus"] });
    },
  });
}

/**
 * 禁用公网路由
 */
export function useDisablePublicRoute() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.disablePublicRoute(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["publicRouteStatus"] });
    },
  });
}

/**
 * 重新生成公网路由隧道鉴权密钥（ccsk-*）。
 * 返回最新状态（含新 key）；Cursor 的 key 无法自动写入，前端负责复制并引导重新粘贴。
 */
export function useRegeneratePublicRouteApiKey() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.regeneratePublicRouteApiKey(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["publicRouteStatus"] });
    },
  });
}

// ========== v3+ 全局/应用级配置 Hooks ==========

/**
 * 获取全局代理配置
 */
export function useGlobalProxyConfig() {
  return useQuery({
    queryKey: proxyKeys.globalConfig,
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
      queryClient.invalidateQueries({ queryKey: proxyKeys.globalConfig });
      queryClient.invalidateQueries({ queryKey: proxyKeys.status });
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
    queryKey: proxyKeys.appConfig(appType),
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
        queryKey: proxyKeys.appConfig(variables.appType),
      });
      queryClient.invalidateQueries({
        queryKey: ["autoFailoverEnabled", variables.appType],
      });
      queryClient.invalidateQueries({ queryKey: ["circuitBreakerConfig"] });
      queryClient.invalidateQueries({ queryKey: proxyKeys.status });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", { error: error.message }),
      );
    },
  });
}
