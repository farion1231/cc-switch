/**
 * 代理服务状态管理 Hook
 */

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { proxyApi } from "@/lib/api/proxy";
import {
  proxyKeys,
  useProxyStatusQuery,
  useProxyTakeoverStatus,
} from "@/lib/query/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";
import { getAppLabel } from "@/config/appConfig";

/**
 * 代理服务状态管理
 */
export function useProxyStatus() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  // 查询状态（自动轮询）
  const { data: status, isPending: isProxyStatusPending } =
    useProxyStatusQuery();

  // 查询各应用接管状态
  const { data: takeoverStatus, isPending: isTakeoverStatusPending } =
    useProxyTakeoverStatus(false);

  // 启动服务器（总开关：仅启动服务，不接管）
  const startProxyServerMutation = useMutation({
    mutationFn: () => proxyApi.startProxyServer(),
    onSuccess: (info) => {
      toast.success(
        t("proxy.server.started", {
          address: info.address,
          port: info.port,
          defaultValue: `代理服务已启动 - ${info.address}:${info.port}`,
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: proxyKeys.status });
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.server.startFailed", {
          detail,
          defaultValue: `启动代理服务失败: ${detail}`,
        }),
      );
    },
  });

  // 停止服务器（仅停止服务，不改写/恢复其它应用接管状态）
  const stopProxyServerMutation = useMutation({
    mutationFn: () => proxyApi.stopProxyServer(),
    onSuccess: () => {
      toast.success(
        t("proxy.server.stopped", {
          defaultValue: "代理服务已停止",
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: proxyKeys.status });
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.server.stopFailed", {
          detail,
          defaultValue: `停止代理服务失败: ${detail}`,
        }),
      );
    },
  });

  // 停止服务器（总开关关闭：强制恢复所有已接管的 Live 配置）
  const stopWithRestoreMutation = useMutation({
    mutationFn: () => proxyApi.stopProxyWithRestore(),
    onSuccess: () => {
      toast.success(
        t("proxy.stoppedWithRestore", {
          defaultValue: "代理服务已关闭，已恢复所有接管配置",
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: proxyKeys.status });
      queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus });
      // 彻底删除所有供应商健康状态缓存（后端已清空数据库记录）
      queryClient.removeQueries({ queryKey: ["providerHealth"] });
      // 彻底删除所有熔断器统计缓存（代理停止后熔断器状态已重置）
      queryClient.removeQueries({ queryKey: ["circuitBreakerStats"] });
      // 注意：故障转移队列和开关状态会保留，不需要刷新
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.stopWithRestoreFailed", {
          detail,
          defaultValue: `停止失败: ${detail}`,
        }),
      );
    },
  });

  // 按应用开启/关闭接管
  const setTakeoverForAppMutation = useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      proxyApi.setProxyTakeoverForApp(appType, enabled),
    onSuccess: (_data, variables) => {
      const appLabel = getAppLabel(variables.appType);

      toast.success(
        variables.enabled
          ? t("proxy.takeover.enabled", {
              app: appLabel,
              defaultValue: `已接管 ${appLabel} 配置（请求将走本地代理）`,
            })
          : t("proxy.takeover.disabled", {
              app: appLabel,
              defaultValue: `已恢复 ${appLabel} 配置`,
            }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: proxyKeys.status });
      queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus });
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "未知错误" });
      toast.error(
        t("proxy.takeover.failed", {
          detail,
          defaultValue: `操作失败: ${detail}`,
        }),
      );
    },
  });

  return {
    status,
    isRunning: status?.running || false,
    takeoverStatus,
    isTakeoverActive:
      takeoverStatus?.claude ||
      takeoverStatus?.codex ||
      takeoverStatus?.gemini ||
      takeoverStatus?.grokbuild ||
      takeoverStatus?.cursor ||
      false,
    isInitialStatusPending: isProxyStatusPending || isTakeoverStatusPending,

    // 启动/停止（总开关）
    startProxyServer: startProxyServerMutation.mutateAsync,
    stopProxyServer: stopProxyServerMutation.mutateAsync,
    stopWithRestore: stopWithRestoreMutation.mutateAsync,

    // 按应用接管开关
    setTakeoverForApp: setTakeoverForAppMutation.mutateAsync,

    // 加载状态
    isStarting: startProxyServerMutation.isPending,
    isStoppingServer: stopProxyServerMutation.isPending,
    isPending:
      startProxyServerMutation.isPending ||
      stopProxyServerMutation.isPending ||
      stopWithRestoreMutation.isPending ||
      setTakeoverForAppMutation.isPending,
  };
}
