import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { tpsTestProvider, type TpsTestResult } from "@/lib/api/model-test";
import type { AppId } from "@/lib/api";

export function useTpsTest(appId: AppId) {
  const { t } = useTranslation();
  const [testingIds, setTestingIds] = useState<Set<string>>(new Set());
  const [lastResultById, setLastResultById] = useState<
    Record<string, TpsTestResult>
  >({});

  const testProvider = useCallback(
    async (providerId: string, providerName: string) => {
      setTestingIds((prev) => new Set(prev).add(providerId));
      try {
        const result = await tpsTestProvider(appId, providerId);
        setLastResultById((prev) => ({ ...prev, [providerId]: result }));

        if (!result.success) {
          toast.error(
            t("tpsTest.failedToast", {
              name: providerName,
              error: result.message,
              defaultValue: `${providerName} TPS 测试失败: ${result.message}`,
            }),
            { closeButton: true },
          );
        }

        return result;
      } catch (e) {
        const message = String(e);
        toast.error(
          t("tpsTest.errorToast", {
            name: providerName,
            error: message,
            defaultValue: `${providerName} TPS 测试出错: ${message}`,
          }),
          { closeButton: true },
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

  const getLastResult = useCallback(
    (providerId: string) => lastResultById[providerId],
    [lastResultById],
  );

  return { testProvider, isTesting, getLastResult };
}

