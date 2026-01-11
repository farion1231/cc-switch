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
    type ProxyTestResult,
    type UpstreamProxyStatus,
} from "@/lib/api/globalProxy";

/**
 * 获取全局代理 URL
 */
export function useGlobalProxyUrl() {
    return useQuery({
        queryKey: ["globalProxyUrl"],
        queryFn: getGlobalProxyUrl,
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
        onError: (error: Error) => {
            toast.error(
                t("settings.globalProxy.saveFailed", { error: error.message })
            );
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
                    t("settings.globalProxy.testSuccess", { latency: result.latencyMs })
                );
            } else {
                toast.error(
                    t("settings.globalProxy.testFailed", { error: result.error })
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
    });
}
