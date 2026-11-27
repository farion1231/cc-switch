import { useState, useCallback } from "react";
import { toast } from "@/lib/toast";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import {
  streamCheckProvider,
  type NetworkErrorKind,
  type StreamCheckResult,
} from "@/lib/api/connectivity-check";
import type { AppId } from "@/lib/api";

/** 按连不上的原因挑一句能照着做的提示；认不出原因时退回通用提示。 */
function unreachableHint(t: TFunction, kind?: NetworkErrorKind): string {
  switch (kind) {
    case "blocked":
      return t("streamCheck.hint.blocked");
    case "refused":
      return t("streamCheck.hint.refused");
    case "timeout":
      return t("streamCheck.hint.timeout");
    case "dns":
      return t("streamCheck.hint.dns");
    case "tls":
      return t("streamCheck.hint.tls");
    default:
      return t("streamCheck.unreachableHint");
  }
}

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
          // 仅当无法建立连接（DNS / 连接被拒 / TLS / 超时）才会到这里
          toast.error(
            t("streamCheck.unreachable", {
              providerName: providerName,
              message: result.message,
              defaultValue: `${providerName} 无法连通: ${result.message}`,
            }),
            {
              description: unreachableHint(t, result.errorKind),
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
