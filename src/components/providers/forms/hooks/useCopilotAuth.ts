import { useState, useCallback, useRef, useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { copilotApi, settingsApi } from "@/lib/api";
import type { CopilotAuthStatus, CopilotDeviceCodeResponse } from "@/lib/api";

/**
 * OAuth 轮询状态
 */
type PollingState = "idle" | "polling" | "success" | "error";

/**
 * Copilot OAuth 认证 Hook
 *
 * 管理 GitHub Copilot OAuth 设备码流程的状态和操作。
 */
export function useCopilotAuth() {
  const queryClient = useQueryClient();

  // 轮询状态
  const [pollingState, setPollingState] = useState<PollingState>("idle");
  const [deviceCode, setDeviceCode] =
    useState<CopilotDeviceCodeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  // 轮询计时器
  const pollingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(
    null,
  );
  const pollingTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 获取认证状态
  const {
    data: authStatus,
    isLoading: isLoadingStatus,
    refetch: refetchStatus,
  } = useQuery<CopilotAuthStatus>({
    queryKey: ["copilot-auth-status"],
    queryFn: copilotApi.copilotGetAuthStatus,
    staleTime: 30000, // 30 秒
  });

  // 停止轮询
  const stopPolling = useCallback(() => {
    if (pollingIntervalRef.current) {
      clearInterval(pollingIntervalRef.current);
      pollingIntervalRef.current = null;
    }
    if (pollingTimeoutRef.current) {
      clearTimeout(pollingTimeoutRef.current);
      pollingTimeoutRef.current = null;
    }
  }, []);

  // 清理（组件卸载时）
  useEffect(() => {
    return () => {
      stopPolling();
    };
  }, [stopPolling]);

  // 启动设备码流程
  const startDeviceFlowMutation = useMutation({
    mutationFn: copilotApi.copilotStartDeviceFlow,
    onSuccess: async (response) => {
      setDeviceCode(response);
      setPollingState("polling");
      setError(null);

      // 自动复制用户码到剪贴板
      try {
        await navigator.clipboard.writeText(response.user_code);
        console.log("[CopilotAuth] 用户码已复制到剪贴板:", response.user_code);
      } catch (e) {
        console.error("Failed to copy user code:", e);
      }

      // 打开系统浏览器
      try {
        await settingsApi.openExternal(response.verification_uri);
      } catch (e) {
        console.error("Failed to open browser:", e);
      }

      // 开始轮询 - GitHub 推荐至少 5 秒间隔，这里使用更保守的 8 秒
      const interval = Math.max((response.interval || 5) + 3, 8) * 1000;
      const expiresAt = Date.now() + response.expires_in * 1000;

      // 轮询函数
      const pollOnce = async () => {
        // 检查是否过期
        if (Date.now() > expiresAt) {
          stopPolling();
          setPollingState("error");
          setError("Device code expired. Please try again.");
          return;
        }

        try {
          const success = await copilotApi.copilotPollForAuth(
            response.device_code,
          );
          if (success) {
            stopPolling();
            setPollingState("success");
            // 刷新认证状态
            await refetchStatus();
            // 使相关查询失效
            queryClient.invalidateQueries({
              queryKey: ["copilot-auth-status"],
            });
          }
        } catch (e) {
          const errorMessage = e instanceof Error ? e.message : String(e);
          // 如果不是"等待中"的错误，则停止轮询
          if (
            !errorMessage.includes("pending") &&
            !errorMessage.includes("slow_down")
          ) {
            stopPolling();
            setPollingState("error");
            setError(errorMessage);
          }
        }
      };

      // 立即执行一次轮询
      pollOnce();

      // 定时轮询
      pollingIntervalRef.current = setInterval(pollOnce, interval);

      // 设置过期超时
      pollingTimeoutRef.current = setTimeout(() => {
        stopPolling();
        setPollingState("error");
        setError("Device code expired. Please try again.");
      }, response.expires_in * 1000);
    },
    onError: (e) => {
      setPollingState("error");
      setError(e instanceof Error ? e.message : String(e));
    },
  });

  // 注销
  const logoutMutation = useMutation({
    mutationFn: copilotApi.copilotLogout,
    onSuccess: () => {
      // 刷新认证状态
      refetchStatus();
      queryClient.invalidateQueries({ queryKey: ["copilot-auth-status"] });
    },
  });

  // 启动认证
  const startAuth = useCallback(() => {
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
    stopPolling();
    startDeviceFlowMutation.mutate();
  }, [startDeviceFlowMutation, stopPolling]);

  // 取消认证
  const cancelAuth = useCallback(() => {
    stopPolling();
    setPollingState("idle");
    setDeviceCode(null);
    setError(null);
  }, [stopPolling]);

  // 注销
  const logout = useCallback(() => {
    logoutMutation.mutate();
  }, [logoutMutation]);

  return {
    // 状态
    authStatus,
    isLoadingStatus,
    isAuthenticated: authStatus?.authenticated ?? false,
    username: authStatus?.username ?? null,

    // 轮询状态
    pollingState,
    deviceCode,
    error,
    isPolling: pollingState === "polling",

    // 操作
    startAuth,
    cancelAuth,
    logout,
    refetchStatus,
  };
}
