import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { Settings } from "@/types";
import {
  copilotVscodeApi,
  copilotVscodeStatusKey,
  type CopilotVscodeSyncStatus,
} from "@/lib/api/copilotVscode";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";

export function useCopilotVscodeSyncStatus(options: Settings["copilotVscode"]) {
  return useQuery({
    queryKey: [...copilotVscodeStatusKey, options],
    queryFn: copilotVscodeApi.status,
    enabled: options?.enabled === true,
    refetchOnMount: "always",
  });
}

/** Refreshes after CLI mutations, including tray and imported provider changes. */
export function useCopilotVscodeSyncNotifications(
  options: Settings["copilotVscode"],
) {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const { data } = useCopilotVscodeSyncStatus(options);
  const lastWarning = useRef<string | null>(null);

  useEffect(() => {
    if (!options?.enabled) return;
    const providersKey = ["providers", "copilot"] as const;
    let previousProviders = queryClient.getQueryData(providersKey);
    let explicitlyInvalidated = false;
    return queryClient.getQueryCache().subscribe((event) => {
      if (
        event.type !== "updated" ||
        event.query.queryKey.length !== providersKey.length ||
        event.query.queryKey[0] !== providersKey[0] ||
        event.query.queryKey[1] !== providersKey[1]
      ) {
        return;
      }
      if (event.action.type === "invalidate") {
        explicitlyInvalidated = true;
        return;
      }
      if (event.action.type !== "success") return;

      // React Query preserves the data reference for an unchanged poll.
      const currentProviders = event.query.state.data;
      const shouldRefresh =
        explicitlyInvalidated ||
        event.action.manual ||
        currentProviders !== previousProviders;
      explicitlyInvalidated = false;
      previousProviders = currentProviders;
      if (shouldRefresh) {
        void queryClient.invalidateQueries({
          queryKey: copilotVscodeStatusKey,
        });
      }
    });
  }, [options?.enabled, queryClient]);

  useEffect(() => {
    if (!options?.enabled || data?.pending === false) {
      if (lastWarning.current !== null) toast.dismiss("copilot-vscode-sync");
      lastWarning.current = null;
      return;
    }
    if (!data?.pending) return;
    const pendingDescription = t("settings.copilotVscode.pendingDescription");
    const detail = data.error || pendingDescription;
    if (detail === lastWarning.current) return;
    lastWarning.current = detail;
    const retry = async () => {
      let failure: string | undefined;
      try {
        await copilotVscodeApi.sync();
      } catch (error) {
        failure = extractErrorMessage(error);
      }
      await queryClient.invalidateQueries({ queryKey: copilotVscodeStatusKey });
      if (failure === undefined) return;

      const refreshedStatus = queryClient.getQueryData<CopilotVscodeSyncStatus>(
        [...copilotVscodeStatusKey, options],
      );
      if (refreshedStatus?.pending === false) return;
      // Keep this retry's error visible after the status query re-renders.
      lastWarning.current = refreshedStatus?.error || pendingDescription;
      toast.error(t("settings.copilotVscode.failed"), {
        id: "copilot-vscode-sync",
        description: failure,
        action: { label: t("common.retry"), onClick: retry },
      });
    };
    toast.warning(t("settings.copilotVscode.pending"), {
      id: "copilot-vscode-sync",
      description: detail,
      duration: 8000,
      closeButton: true,
      action: { label: t("common.retry"), onClick: retry },
    });
  }, [data, options?.enabled, queryClient, t]);
}
