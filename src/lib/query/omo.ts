import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { omoApi, omoSlimApi } from "@/lib/api/omo";
import * as configApi from "@/lib/api/config";
import type { OmoGlobalConfig } from "@/types/omo";

// ── Factory ────────────────────────────────────────────────────

function createOmoQueryKeys(prefix: string) {
  return {
    all: [prefix] as const,
    globalConfig: () => [prefix, "global-config"] as const,
    currentProviderId: () => [prefix, "current-provider-id"] as const,
    providerCount: () => [prefix, "provider-count"] as const,
  };
}

function createOmoQueryHooks(
  variant: "omo" | "omo-slim",
  api: typeof omoApi | typeof omoSlimApi,
) {
  const keys = createOmoQueryKeys(variant);
  const snippetKey = variant === "omo" ? "omo" : "omo_slim";

  function invalidateAll(queryClient: ReturnType<typeof useQueryClient>) {
    queryClient.invalidateQueries({ queryKey: keys.globalConfig() });
    queryClient.invalidateQueries({ queryKey: ["providers"] });
    queryClient.invalidateQueries({ queryKey: keys.currentProviderId() });
    queryClient.invalidateQueries({ queryKey: keys.providerCount() });
  }

  function useGlobalConfig(enabled = true) {
    return useQuery({
      queryKey: keys.globalConfig(),
      enabled,
      queryFn: async (): Promise<OmoGlobalConfig> => {
        const raw = await configApi.getCommonConfigSnippet(snippetKey);
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
            `[${variant}] invalid global config json, fallback to defaults`,
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

  function useCurrentProviderId(enabled = true) {
    return useQuery({
      queryKey: keys.currentProviderId(),
      queryFn:
        "getCurrentOmoProviderId" in api
          ? (api as typeof omoApi).getCurrentOmoProviderId
          : (api as typeof omoSlimApi).getCurrentProviderId,
      enabled,
    });
  }

  function useProviderCount(enabled = true) {
    return useQuery({
      queryKey: keys.providerCount(),
      queryFn:
        "getOmoProviderCount" in api
          ? (api as typeof omoApi).getOmoProviderCount
          : (api as typeof omoSlimApi).getProviderCount,
      enabled,
    });
  }

  function useSaveGlobalConfig() {
    const queryClient = useQueryClient();
    return useMutation({
      mutationFn: async (input: OmoGlobalConfig) => {
        const jsonStr = JSON.stringify(input);
        await configApi.setCommonConfigSnippet(snippetKey, jsonStr);
      },
      onSuccess: () => invalidateAll(queryClient),
    });
  }

  function useReadLocalFile() {
    return useMutation({
      mutationFn: () => api.readLocalFile(),
    });
  }

  function useDisableCurrent() {
    const queryClient = useQueryClient();
    return useMutation({
      mutationFn:
        "disableCurrentOmo" in api
          ? (api as typeof omoApi).disableCurrentOmo
          : (api as typeof omoSlimApi).disableCurrent,
      onSuccess: () => invalidateAll(queryClient),
    });
  }

  return {
    keys,
    useGlobalConfig,
    useCurrentProviderId,
    useProviderCount,
    useSaveGlobalConfig,
    useReadLocalFile,
    useDisableCurrent,
  };
}

// ── Instances ──────────────────────────────────────────────────

const omoHooks = createOmoQueryHooks("omo", omoApi);
const omoSlimHooks = createOmoQueryHooks("omo-slim", omoSlimApi);

// ── Backward-compatible exports ────────────────────────────────

export const omoKeys = omoHooks.keys;
export const omoSlimKeys = omoSlimHooks.keys;

export const useOmoGlobalConfig = omoHooks.useGlobalConfig;
export const useCurrentOmoProviderId = omoHooks.useCurrentProviderId;
export const useOmoProviderCount = omoHooks.useProviderCount;
export const useSaveOmoGlobalConfig = omoHooks.useSaveGlobalConfig;
export const useReadOmoLocalFile = omoHooks.useReadLocalFile;
export const useDisableCurrentOmo = omoHooks.useDisableCurrent;

export const useOmoSlimGlobalConfig = omoSlimHooks.useGlobalConfig;
export const useCurrentOmoSlimProviderId = omoSlimHooks.useCurrentProviderId;
export const useOmoSlimProviderCount = omoSlimHooks.useProviderCount;
export const useSaveOmoSlimGlobalConfig = omoSlimHooks.useSaveGlobalConfig;
export const useReadOmoSlimLocalFile = omoSlimHooks.useReadLocalFile;
export const useDisableCurrentOmoSlim = omoSlimHooks.useDisableCurrent;
