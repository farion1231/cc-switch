import {
  type QueryClient,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { ohmypiApi } from "@/lib/api/ohmypi";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

export const ohmypiKeys = {
  all: ["ohmypi"] as const,
  currentState: ["ohmypi", "currentState"] as const,
  agentDiscoveryState: ["ohmypi", "agentDiscoveryState"] as const,
};

export const invalidateOhMyPiProviderCaches = async (
  queryClient: QueryClient,
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ohmypiKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: ["providers", "ohmypi"] }),
  ]);
};

export const invalidateOhMyPiDirectoryCaches = async (
  queryClient: QueryClient,
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ohmypiKeys.all }),
    queryClient.invalidateQueries({ queryKey: ["providers", "ohmypi"] }),
    queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
    queryClient.invalidateQueries({ queryKey: ["sessions"] }),
  ]);
};

export function useOhMyPiCurrentState(enabled = true) {
  return useQuery({
    queryKey: ohmypiKeys.currentState,
    queryFn: () => ohmypiApi.getCurrentState(),
    enabled,
  });
}

/** Whether omp still auto-discovers MCP/skills from other agents. */
export function useOhMyPiAgentDiscoveryState() {
  return useQuery({
    queryKey: ohmypiKeys.agentDiscoveryState,
    queryFn: () => ohmypiApi.getAgentDiscoveryState(),
  });
}

/** Disable omp's auto-discovery of the 12 other-agent sources. Returns the
 * updated `disabledProviders`. On success, invalidates the discovery-state
 * query and toasts that it takes effect on the next omp session / `/reload-plugins`. */
export function useDisableOhMyPiAgentAutoDiscovery() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  return useMutation({
    mutationFn: () => ohmypiApi.disableAgentAutoDiscovery(),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ohmypiKeys.agentDiscoveryState,
      });
      toast.success(t("ohmypi.autoDiscovery.disabled.toast"));
    },
    onError: (error: unknown) => {
      toast.error(t("ohmypi.autoDiscovery.disableFailed.toast"));
      console.error("disable_ohmypi_agent_auto_discovery failed", error);
    },
  });
}
