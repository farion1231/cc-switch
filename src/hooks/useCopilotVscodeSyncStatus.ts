import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { Settings } from "@/types";
import {
  copilotVscodeApi,
  copilotVscodeStatusKey,
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
    return queryClient.getQueryCache().subscribe((event) => {
      if (
        event.type === "updated" &&
        event.action.type === "success" &&
        event.query.queryKey[0] === "providers" &&
        event.query.queryKey[1] === "copilot"
      ) {
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
    const detail = data.error || t("settings.copilotVscode.pendingDescription");
    if (detail === lastWarning.current) return;
    lastWarning.current = detail;
    const retry = async () => {
      try {
        await copilotVscodeApi.sync();
      } catch (error) {
        toast.error(t("settings.copilotVscode.failed"), {
          id: "copilot-vscode-sync",
          description: extractErrorMessage(error),
          action: { label: t("common.retry"), onClick: retry },
        });
      } finally {
        await queryClient.invalidateQueries({
          queryKey: copilotVscodeStatusKey,
        });
      }
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
