/**
 * 全局出站代理 React Hooks
 *
 * 提供获取、设置和测试全局代理的 React Query hooks。
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  getGlobalProxyUrl,
  setGlobalProxyUrl,
  testProxyUrl,
  getUpstreamProxyStatus,
  scanLocalProxies,
  getFollowSystemProxy,
  setFollowSystemProxy,
  type ProxyTestResult,
  type UpstreamProxyStatus,
  type DetectedProxy,
} from "@/lib/api/globalProxy";

/**
 * 获取全局代理 URL
 */
export function useGlobalProxyUrl() {
  return useQuery({
    queryKey: ["globalProxyUrl"],
    queryFn: getGlobalProxyUrl,
    staleTime: 30 * 1000, // 30秒内不重新获取，避免展开时闪烁
  });
}

/**
 * 设置全局代理 URL
 */
export function useSetGlobalProxyUrl() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: setGlobalProxyUrl,
    onSuccess: () => {
      toast.success(t("settings.globalProxy.saved"));
      queryClient.invalidateQueries({ queryKey: ["globalProxyUrl"] });
      queryClient.invalidateQueries({ queryKey: ["upstreamProxyStatus"] });
    },
    onError: (error: unknown) => {
      const message =
        error instanceof Error
          ? error.message
          : typeof error === "string"
            ? error
            : "Unknown error";
      toast.error(t("settings.globalProxy.saveFailed", { error: message }));
    },
  });
}

/**
 * 是否跟随系统代理
 */
export function useFollowSystemProxy() {
  return useQuery({
    queryKey: ["followSystemProxy"],
    queryFn: getFollowSystemProxy,
    staleTime: 30 * 1000,
  });
}

/**
 * 设置是否跟随系统代理（后端立即重建客户端，无需重启）
 */
export function useSetFollowSystemProxy() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: setFollowSystemProxy,
    onSuccess: () => {
      toast.success(t("settings.globalProxy.saved"));
      queryClient.invalidateQueries({ queryKey: ["followSystemProxy"] });
      queryClient.invalidateQueries({ queryKey: ["upstreamProxyStatus"] });
    },
    onError: (error: unknown) => {
      const message =
        error instanceof Error
          ? error.message
          : typeof error === "string"
            ? error
            : "Unknown error";
      toast.error(t("settings.globalProxy.saveFailed", { error: message }));
    },
  });
}

/**
 * 测试代理连接
 */
export function useTestProxy() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: testProxyUrl,
    onSuccess: (result: ProxyTestResult) => {
      if (result.success) {
        toast.success(
          t("settings.globalProxy.testSuccess", { latency: result.latencyMs }),
        );
      } else {
        toast.error(
          t("settings.globalProxy.testFailed", { error: result.error }),
        );
      }
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });
}

/**
 * 获取当前出站代理状态
 */
export function useUpstreamProxyStatus() {
  return useQuery<UpstreamProxyStatus>({
    queryKey: ["upstreamProxyStatus"],
    queryFn: getUpstreamProxyStatus,
    // 这条查询会读一次系统代理配置并对代理端口做 TCP 探测，不该每次窗口聚焦都跑一遍；
    // 超过 30s 后再聚焦会重新探测，状态块上另有手动刷新，保存代理地址和切换跟随开关也会显式失效它
    staleTime: 30 * 1000,
  });
}

/**
 * 扫描本地代理
 */
export function useScanProxies() {
  const { t } = useTranslation();

  return useMutation({
    mutationFn: scanLocalProxies,
    onError: (error: Error) => {
      toast.error(
        t("settings.globalProxy.scanFailed", { error: error.message }),
      );
    },
  });
}

export type { DetectedProxy };
