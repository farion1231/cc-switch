/**
 * 代理服务状态管理 Hook
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type {
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
} from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 代理服务状态管理
 */
export function useProxyStatus() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  // 查询状态（自动轮询）
  const { data: status, isLoading } = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: () => invoke<ProxyStatus>("get_proxy_status"),
    // 仅在服务运行时轮询
    refetchInterval: (query) => (query.state.data?.running ? 2000 : false),
    // 保持之前的数据，避免闪烁
    placeholderData: (previousData) => previousData,
  });

  // 查询各应用接管状态
  const { data: takeoverStatus } = useQuery({
    queryKey: ["proxyTakeoverStatus"],
    queryFn: () => invoke<ProxyTakeoverStatus>("get_proxy_takeover_status"),
    placeholderData: (previousData) => previousData,
  });

  // 启动服务器（总开关：仅启动服务，不接管）
  const startProxyServerMutation = useMutation({
    mutationFn: () => invoke<ProxyServerInfo>("start_proxy_server"),
    onSuccess: (info) => {
      toast.success(
        t("proxy.server.started", {
          defaultValue: `Proxy server started - ${info.address}:${info.port}`,
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "Unknown error" });
      toast.error(
        t("proxy.server.startFailed", {
          defaultValue: `Failed to start proxy server: ${detail}`,
        }),
      );
    },
  });

  // 停止服务器（总开关关闭：强制恢复所有已接管的 Live 配置）
  const stopWithRestoreMutation = useMutation({
    mutationFn: () => invoke("stop_proxy_with_restore"),
    onSuccess: () => {
      toast.success(
        t("proxy.stoppedWithRestore", {
          defaultValue:
            "Proxy server stopped, all takeover configurations restored",
        }),
        { closeButton: true },
      );
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
      // 彻底删除所有供应商健康状态缓存（后端已清空数据库记录）
      queryClient.removeQueries({ queryKey: ["providerHealth"] });
      // 彻底删除所有熔断器统计缓存（代理停止后熔断器状态已重置）
      queryClient.removeQueries({ queryKey: ["circuitBreakerStats"] });
      // 注意：故障转移队列和开关状态会保留，不需要刷新
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "Unknown error" });
      toast.error(
        t("proxy.stopWithRestoreFailed", {
          defaultValue: `Stop failed: ${detail}`,
        }),
      );
    },
  });

  // 按应用开启/关闭接管
  const setTakeoverForAppMutation = useMutation({
    mutationFn: ({ appType, enabled }: { appType: string; enabled: boolean }) =>
      invoke("set_proxy_takeover_for_app", { appType, enabled }),
    onSuccess: (_data, variables) => {
      const appLabel =
        variables.appType === "claude"
          ? "Claude"
          : variables.appType === "codex"
            ? "Codex"
            : variables.appType === "gemini"
              ? "Gemini"
              : "OpenCode";

      toast.success(
        variables.enabled
          ? t("proxy.takeover.enabled", {
              defaultValue: `${appLabel} configuration taken over (requests will go through local proxy)`,
            })
          : t("proxy.takeover.disabled", {
              defaultValue: `${appLabel} configuration restored`,
            }),
        { closeButton: true },
      );

      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] });
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "Unknown error" });
      toast.error(
        t("proxy.takeover.failed", {
          defaultValue: `Operation failed: ${detail}`,
        }),
      );
    },
  });

  // 代理模式切换供应商（热切换）
  const switchProxyProviderMutation = useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => invoke("switch_proxy_provider", { appType, providerId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "Unknown error" });
      toast.error(
        t("proxy.switchFailed", {
          error: detail,
          defaultValue: `Switch failed: ${detail}`,
        }),
      );
    },
  });

  // 检查是否运行中
  const checkRunning = async () => {
    try {
      return await invoke<boolean>("is_proxy_running");
    } catch {
      return false;
    }
  };

  // 检查接管状态
  const checkTakeoverActive = async () => {
    try {
      return await invoke<boolean>("is_live_takeover_active");
    } catch {
      return false;
    }
  };

  return {
    status,
    isLoading,
    isRunning: status?.running || false,
    takeoverStatus,
    isTakeoverActive:
      takeoverStatus?.claude ||
      takeoverStatus?.codex ||
      takeoverStatus?.gemini ||
      false,

    // 启动/停止（总开关）
    startProxyServer: startProxyServerMutation.mutateAsync,
    stopWithRestore: stopWithRestoreMutation.mutateAsync,

    // 按应用接管开关
    setTakeoverForApp: setTakeoverForAppMutation.mutateAsync,

    // 代理模式下切换供应商
    switchProxyProvider: switchProxyProviderMutation.mutateAsync,

    // 状态检查
    checkRunning,
    checkTakeoverActive,

    // 加载状态
    isStarting: startProxyServerMutation.isPending,
    isStopping: stopWithRestoreMutation.isPending,
    isPending:
      startProxyServerMutation.isPending ||
      stopWithRestoreMutation.isPending ||
      setTakeoverForAppMutation.isPending,
  };
}
