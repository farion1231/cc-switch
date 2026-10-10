import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { proxyApi } from "@/lib/api/proxy";
import { toast } from "@/lib/toast";
import { useTranslation } from "react-i18next";
import type {
  AppModeView,
  GlobalProxyConfig,
  AppProxyConfig,
  ProxyStackWriteError,
  ProxyTakeoverStatus,
} from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";
import { getAppLabel } from "@/config/appConfig";

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

/**
 * 应用的模式状态（直连 / 路由 / 聚合、路由目标、直连那家）。放在 ["providers", appId] 前缀下：
 * 进出模式、切换供应商时随供应商列表一起失效。
 */
export function useAppMode(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["providers", appType, "mode"] as const,
    queryFn: () => proxyApi.getAppMode(appType),
    enabled,
    placeholderData: (previous: AppModeView | undefined) => previous,
  });
}

/**
 * 直连供应商（路由模式下退出路由时写回的那家）。
 * 放在 ["providers", appId] 前缀下：切换、编辑供应商时随供应商列表一起失效。
 */
export function useDirectProviderId(appType: string, enabled: boolean) {
  return useQuery({
    queryKey: ["providers", appType, "direct"] as const,
    queryFn: () => proxyApi.getDirectProvider(appType),
    enabled,
  });
}

/**
 * Stack 模型名单。放在 ["providers", appId] 前缀下：编辑、删除供应商时随列表一起失效。
 */
export function useProxyStack(appType: string, enabled: boolean) {
  return useQuery({
    queryKey: ["providers", appType, "stack"] as const,
    queryFn: () => proxyApi.getProxyStack(appType),
    enabled,
  });
}

function isStackWriteError(error: unknown): error is ProxyStackWriteError {
  return (
    typeof error === "object" &&
    error !== null &&
    typeof (error as ProxyStackWriteError).partial === "boolean"
  );
}

/**
 * 把一家加入或移出 Stack 模型。Claude Code 运行中就会读到改过的 settings.json，Codex 只在
 * 启动时读模型目录，成功后提示重启。失败分两种：
 * 什么都没改（弹后端的错误），已部分写入（下次操作或重启 CC Switch 时补完）。两种都按
 * 后端的状态重新显示，不在前端假设名单不变。
 */
export function useSetProxyStackMember() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
      enabled,
    }: {
      appType: string;
      providerId: string;
      enabled: boolean;
    }) => proxyApi.setProxyStackMember(appType, providerId, enabled),
    onSuccess: (notice, variables) => {
      toast.success(
        t(
          variables.appType === "codex"
            ? "provider.stackSaved"
            : "provider.stackSavedLive",
          { client: getAppLabel(variables.appType) },
        ),
        {
          description: variables.enabled
            ? t("provider.stackReselectHint")
            : undefined,
          closeButton: true,
        },
      );
      if (notice) {
        toast.warning(t(`provider.${notice}`), { closeButton: true });
      }
    },
    onError: (error: unknown) => {
      if (isStackWriteError(error) && error.partial) {
        toast.warning(t("provider.stackPartial"), {
          description: error.message,
          closeButton: true,
        });
        return;
      }
      toast.error(
        t("provider.stackFailed", { error: extractErrorMessage(error) }),
      );
    },
    onSettled: (_data, _error, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
    },
  });
}

/**
 * Codex 聚合的模型被路由供应商自己的模型目录挡住（routeOwnsCatalog）时，改用 CC Switch
 * 生成的目录。客户端只在启动时读模型目录，成功后提示重启；还剩别的提示照样弹出。
 */
export function useAdoptCodexStackCatalog() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => proxyApi.adoptCodexStackCatalog(),
    onSuccess: (notice) => {
      toast.success(t("provider.adoptCatalogDone"), { closeButton: true });
      if (notice) {
        toast.warning(t(`provider.${notice}`), { closeButton: true });
      }
    },
    onError: (error: unknown) => {
      toast.error(
        t("provider.adoptCatalogFailed", {
          error: extractErrorMessage(error),
        }),
      );
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    },
  });
}

/**
 * 重启 Codex 的托管守护进程，让它重读模型目录。结束后重新查 Stack 名单：重启成功时
 * 「还在用旧模型列表」的提示随之消失。
 */
export function useRestartCodexAppServerDaemon() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: () => proxyApi.restartCodexAppServerDaemon(),
    onSuccess: (outcome) => {
      if (outcome === "notRunning") {
        toast.info(t("proxy.stackMode.codexStale.notRunning"), {
          closeButton: true,
        });
        return;
      }
      toast.success(t("proxy.stackMode.codexStale.restarted"), {
        closeButton: true,
      });
    },
    onError: (error: unknown) => {
      toast.error(
        t("proxy.stackMode.codexStale.failed", {
          error: extractErrorMessage(error),
        }),
      );
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    },
  });
}

/**
 * 关掉看不到进程时出的「Codex 可能还在用旧的」提示：后端记下现在这份，之后再变才提示。
 */
export function useAcknowledgeCodexStaleClients() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => proxyApi.acknowledgeCodexStaleClients(),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: ["providers", "codex"] });
    },
  });
}

// ========== 代理服务器控制 Hooks ==========

/**
 * 设置应用接管状态
 */
export function useSetProxyTakeoverForApp() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      enabled,
      stack = false,
    }: {
      appType: string;
      enabled: boolean;
      stack?: boolean;
    }) => proxyApi.setProxyTakeoverForApp(appType, enabled, stack),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus });
      // 进出路由模式会改「当前」显示的供应商（路由模式下是路由到的那家）。
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
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
