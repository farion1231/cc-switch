import { useState, useCallback } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  streamCheckProvider,
  type StreamCheckResult,
} from "@/lib/api/connectivity-check";
import type { AppId } from "@/lib/api";

/**
 * 供应商连通性检查。
 *
 * 只探测 base_url 是否可达（任何 HTTP 响应都算可达），不发真实大模型请求。
 * 刻意 **不** 重置故障转移熔断器——可达 ≠ 配置正确，一个端口通但鉴权废的供应商
 * 不应被误判为"健康"而切回线上。熔断器只由真实转发流量驱动（见 proxy/forwarder.rs）。
 */
export function useStreamCheck(appId: AppId) {
  const { t } = useTranslation();
  const [checkingIds, setCheckingIds] = useState<Set<string>>(new Set());

  const checkProvider = useCallback(
    async (
      providerId: string,
      providerName: string,
    ): Promise<StreamCheckResult | null> => {
      setCheckingIds((prev) => new Set(prev).add(providerId));

      try {
        const result = await streamCheckProvider(appId, providerId);

        if (result.status === "operational") {
          toast.success(
            t("streamCheck.reachable", {
              providerName: providerName,
              responseTimeMs: result.responseTimeMs,
              defaultValue: `${providerName} 连通正常 (${result.responseTimeMs}ms)`,
            }),
            { closeButton: true },
          );
        } else if (result.status === "degraded") {
          toast.warning(
            t("streamCheck.reachableSlow", {
              providerName: providerName,
              responseTimeMs: result.responseTimeMs,
              defaultValue: `${providerName} 连通但较慢 (${result.responseTimeMs}ms)`,
            }),
          );
        } else {
          // 仅当无法建立连接或真实调用失败时才会到这里
          const isApiError = result.message.includes("HTTP ");
          let hint = t("streamCheck.unreachableHint", {
            defaultValue: "无法建立连接（DNS / 连接 / TLS / 超时）。请检查 base_url 与网络。",
          });
          
          if (isApiError) {
            if (result.message.includes("401") || result.message.toLowerCase().includes("key")) {
              hint = t("streamCheck.authErrorHint", { defaultValue: "认证失败，请检查您的 API 密钥 (API Key) 配置。" });
            } else if (result.message.includes("429") || result.message.toLowerCase().includes("quota") || result.message.toLowerCase().includes("limit")) {
              hint = t("streamCheck.quotaErrorHint", { defaultValue: "频控受限或额度用尽，请检查账户可用额度。" });
            } else if (result.message.includes("404") || result.message.toLowerCase().includes("model")) {
              hint = t("streamCheck.modelErrorHint", { defaultValue: "请求模型不存在，或当前渠道未开通该模型访问权限。" });
            } else {
              hint = t("streamCheck.apiErrorHint", { defaultValue: "大模型接口调用失败，请根据具体返回的 HTTP 错误进行排查。" });
            }
          }

          toast.error(
            t("streamCheck.unreachable", {
              providerName: providerName,
              message: result.message,
              defaultValue: `${providerName} 无法连通: ${result.message}`,
            }),
            {
              description: hint,
              duration: 8000,
              closeButton: true,
            },
          );
        }

        return result;
      } catch (e) {
        toast.error(
          t("streamCheck.error", {
            providerName: providerName,
            error: String(e),
            defaultValue: `${providerName} 检查出错: ${String(e)}`,
          }),
        );
        return null;
      } finally {
        setCheckingIds((prev) => {
          const next = new Set(prev);
          next.delete(providerId);
          return next;
        });
      }
    },
    [appId, t],
  );

  const isChecking = useCallback(
    (providerId: string) => checkingIds.has(providerId),
    [checkingIds],
  );

  return { checkProvider, isChecking };
}
