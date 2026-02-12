import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { omoApi } from "@/lib/api/omo";
import * as configApi from "@/lib/api/config";
import type { OmoGlobalConfig } from "@/types/omo";

export const omoKeys = {
  all: ["omo"] as const,
  globalConfig: () => [...omoKeys.all, "global-config"] as const,
  currentProviderId: () => [...omoKeys.all, "current-provider-id"] as const,
  providerCount: () => [...omoKeys.all, "provider-count"] as const,
};

function invalidateOmoQueries(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: omoKeys.globalConfig() });
  queryClient.invalidateQueries({ queryKey: ["providers"] });
  queryClient.invalidateQueries({ queryKey: omoKeys.currentProviderId() });
  queryClient.invalidateQueries({ queryKey: omoKeys.providerCount() });
}

export function useOmoGlobalConfig(enabled = true) {
  return useQuery({
    queryKey: omoKeys.globalConfig(),
    enabled,
    queryFn: async (): Promise<OmoGlobalConfig> => {
      const raw = await configApi.getCommonConfigSnippet("omo");
      if (!raw) {
        return {
          id: "global",
          disabledAgents: [],
          disabledMcps: [],
          disabledHooks: [],
          disabledSkills: [],
          updatedAt: new Date().toISOString(),
        };
      }
      try {
        return JSON.parse(raw) as OmoGlobalConfig;
      } catch (error) {
        console.warn(
          "[omo] invalid global config json, fallback to defaults",
          error,
        );
        return {
          id: "global",
          disabledAgents: [],
          disabledMcps: [],
          disabledHooks: [],
          disabledSkills: [],
          updatedAt: new Date().toISOString(),
        };
      }
    },
  });
}

export function useCurrentOmoProviderId(enabled = true) {
  return useQuery({
    queryKey: omoKeys.currentProviderId(),
    queryFn: omoApi.getCurrentOmoProviderId,
    enabled,
  });
}

export function useOmoProviderCount(enabled = true) {
  return useQuery({
    queryKey: omoKeys.providerCount(),
    queryFn: omoApi.getOmoProviderCount,
    enabled,
  });
}

export function useSaveOmoGlobalConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: OmoGlobalConfig) => {
      const jsonStr = JSON.stringify(input);
      await configApi.setCommonConfigSnippet("omo", jsonStr);
    },
    onSuccess: () => invalidateOmoQueries(queryClient),
  });
}

export function useReadOmoLocalFile() {
  return useMutation({
    mutationFn: () => omoApi.readLocalFile(),
  });
}

export function useDisableCurrentOmo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => omoApi.disableCurrentOmo(),
    onSuccess: () => invalidateOmoQueries(queryClient),
  });
}
