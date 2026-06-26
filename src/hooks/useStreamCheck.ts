import { useState, useCallback } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  streamCheckProvider,
  type StreamCheckResult,
} from "@/lib/api/model-test";
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

  const getFailureDescription = useCallback(
    (result: StreamCheckResult) => {
      if (typeof result.httpStatus === "number") {
        const httpHintKey =
          result.httpStatus >= 500
            ? "streamCheck.httpHint.5xx"
            : `streamCheck.httpHint.${result.httpStatus}`;
        const httpHint = t(httpHintKey, { defaultValue: "" });
        if (httpHint && httpHint !== httpHintKey) {
          return httpHint;
        }

        return t("streamCheck.connectivityNote", {
          defaultValue:
            "Connectivity only confirms reachability. Authentication or model configuration may still need adjustment.",
        });
      }

      return t("streamCheck.unreachableHint", {
        defaultValue:
          "Could not establish a connection (DNS / connect / TLS / timeout). Check the base_url and your network.",
      });
    },
    [t],
  );

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
          toast.error(
            t("streamCheck.failed", {
              providerName: providerName,
              message: result.message,
              defaultValue: `${providerName} check failed: ${result.message}`,
            }),
            {
              description: getFailureDescription(result),
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
    [appId, getFailureDescription, t],
  );

  const isChecking = useCallback(
    (providerId: string) => checkingIds.has(providerId),
    [checkingIds],
  );

  return { checkProvider, isChecking };
}
