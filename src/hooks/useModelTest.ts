import { useState, useCallback } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { testProviderModel, type ModelTestResult } from "@/lib/api/model-test";
import type { AppId } from "@/lib/api";

export function useModelTest(appId: AppId) {
  const { t } = useTranslation();
  const [testingIds, setTestingIds] = useState<Set<string>>(new Set());

  const testProvider = useCallback(
    async (
      providerId: string,
      providerName: string,
    ): Promise<ModelTestResult | null> => {
      setTestingIds((prev) => new Set(prev).add(providerId));

      try {
        const result = await testProviderModel(appId, providerId);

        if (result.success) {
          toast.success(
            t("modelTest.success", {
              name: providerName,
              time: result.responseTimeMs,
              defaultValue: `${providerName} 测试成功 (${result.responseTimeMs}ms)`,
            }),
          );
        } else {
          toast.error(
            t("modelTest.failed", {
              name: providerName,
              error: result.message,
              defaultValue: `${providerName} 测试失败: ${result.message}`,
            }),
          );
        }

        return result;
      } catch (e) {
        toast.error(
          t("modelTest.error", {
            name: providerName,
            error: String(e),
            defaultValue: `${providerName} 测试出错: ${String(e)}`,
          }),
        );
        return null;
      } finally {
        setTestingIds((prev) => {
          const next = new Set(prev);
          next.delete(providerId);
          return next;
        });
      }
    },
    [appId, t],
  );

  const isTesting = useCallback(
    (providerId: string) => testingIds.has(providerId),
    [testingIds],
  );

  return { testProvider, isTesting };
}
