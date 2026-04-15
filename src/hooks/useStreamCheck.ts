import { useState, useCallback } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  streamCheckProvider,
  type StreamCheckResult,
} from "@/lib/api/model-test";
import type { AppId } from "@/lib/api";
import { useResetCircuitBreaker } from "@/lib/query/failover";

export function useStreamCheck(appId: AppId) {
  const { t } = useTranslation();
  const [checkingIds, setCheckingIds] = useState<Set<string>>(new Set());
  const resetCircuitBreaker = useResetCircuitBreaker();

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
            t("streamCheck.operational", {
              providerName: providerName,
              responseTimeMs: result.responseTimeMs,
              defaultValue: `${providerName} 运行正常 (${result.responseTimeMs}ms)`,
            }),
            { closeButton: true },
          );

          // 测试通过后重置熔断器状态
          resetCircuitBreaker.mutate({ providerId, appType: appId });
        } else if (result.status === "degraded") {
          toast.warning(
            t("streamCheck.degraded", {
              providerName: providerName,
              responseTimeMs: result.responseTimeMs,
              defaultValue: `${providerName} 响应较慢 (${result.responseTimeMs}ms)`,
            }),
          );

          // 降级状态也重置熔断器，因为至少能通信
          resetCircuitBreaker.mutate({ providerId, appType: appId });
        } else {
          const httpStatus = result.httpStatus;
          const hintKey = httpStatus
            ? `streamCheck.httpHint.${httpStatus >= 500 ? "5xx" : httpStatus}`
            : null;
          const description =
            (hintKey ? t(hintKey, { defaultValue: "" }) : "") || undefined;

          // 401/403/400 = 检查被拒（供应商可能正常）；429/5xx = 临时问题
          const isProbeRejection =
            httpStatus != null &&
            ([401, 403, 400, 429].includes(httpStatus) || httpStatus >= 500);

          if (isProbeRejection) {
            toast.warning(
              t("streamCheck.rejected", {
                providerName: providerName,
                message: result.message,
                defaultValue: `${providerName} 检查被拒: ${result.message}`,
              }),
              { description, duration: 8000, closeButton: true },
            );
          } else {
            toast.error(
              t("streamCheck.failed", {
                providerName: providerName,
                message: result.message,
                defaultValue: `${providerName} 检查失败: ${result.message}`,
              }),
              { description, duration: 8000, closeButton: true },
            );
          }
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
    [appId, t, resetCircuitBreaker],
  );

  const isChecking = useCallback(
    (providerId: string) => checkingIds.has(providerId),
    [checkingIds],
  );

  return { checkProvider, isChecking };
}
