/**
 * 代理服务状态管理 Hook
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { ProxyStatus, ProxyServerInfo } from "@/types/proxy";
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

  // 查询接管状态
  const { data: isTakeoverActive } = useQuery({
    queryKey: ["proxyTakeoverActive"],
    queryFn: () => invoke<boolean>("is_live_takeover_active"),
  });

  // 启动服务器（带 Live 配置接管）
  const startWithTakeoverMutation = useMutation({
    mutationFn: () => invoke<ProxyServerInfo>("start_proxy_with_takeover"),
    onSuccess: (info) => {
      toast.success(
        t("proxy.startedWithTakeover", {
          defaultValue: `代理模式已启用 - ${info.address}:${info.port}`,
        }),
      );
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverActive"] });
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || "未知错误";
      toast.error(
        t("proxy.startWithTakeoverFailed", {
          defaultValue: `启动失败: ${detail}`,
        }),
      );
    },
  });

  // 停止服务器（恢复 Live 配置）
  const stopWithRestoreMutation = useMutation({
    mutationFn: () => invoke("stop_proxy_with_restore"),
    onSuccess: () => {
      toast.success(
        t("proxy.stoppedWithRestore", {
          defaultValue: "代理模式已关闭，配置已恢复",
        }),
      );
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["proxyTakeoverActive"] });
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || "未知错误";
      toast.error(
        t("proxy.stopWithRestoreFailed", {
          defaultValue: `停止失败: ${detail}`,
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
      const detail = extractErrorMessage(error) || "未知错误";
      toast.error(`切换失败: ${detail}`);
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
    isTakeoverActive: isTakeoverActive || false,

    // 启动/停止（接管模式）
    startWithTakeover: startWithTakeoverMutation.mutateAsync,
    stopWithRestore: stopWithRestoreMutation.mutateAsync,

    // 代理模式下切换供应商
    switchProxyProvider: switchProxyProviderMutation.mutateAsync,

    // 状态检查
    checkRunning,
    checkTakeoverActive,

    // 加载状态
    isStarting: startWithTakeoverMutation.isPending,
    isStopping: stopWithRestoreMutation.isPending,
    isPending:
      startWithTakeoverMutation.isPending || stopWithRestoreMutation.isPending,
  };
}
