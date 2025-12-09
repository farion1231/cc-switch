/**
 * 代理服务状态管理 Hook
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import type { ProxyStatus, ProxyServerInfo } from "@/types/proxy";

/**
 * 代理服务状态管理
 */
export function useProxyStatus() {
  const queryClient = useQueryClient();

  // 查询状态（自动轮询）
  const { data: status, isLoading } = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: () => invoke<ProxyStatus>("get_proxy_status"),
    // 仅在服务运行时轮询
    refetchInterval: (query) => (query.state.data?.running ? 2000 : false),
    // 保持之前的数据，避免闪烁
    placeholderData: (previousData) => previousData,
  });

  // 启动服务器
  const startMutation = useMutation({
    mutationFn: () => invoke<ProxyServerInfo>("start_proxy_server"),
    onSuccess: (info) => {
      toast.success(`代理服务已启动 - ${info.address}:${info.port}`);
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(`启动失败: ${error.message}`);
    },
  });

  // 停止服务器
  const stopMutation = useMutation({
    mutationFn: () => invoke("stop_proxy_server"),
    onSuccess: () => {
      toast.success("代理服务已停止");
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(`停止失败: ${error.message}`);
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

  return {
    status,
    isLoading,
    isRunning: status?.running || false,
    start: startMutation.mutateAsync,
    stop: stopMutation.mutateAsync,
    checkRunning,
    isStarting: startMutation.isPending,
    isStopping: stopMutation.isPending,
    isPending: startMutation.isPending || stopMutation.isPending,
  };
}
