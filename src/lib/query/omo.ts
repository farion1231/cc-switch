import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { omoApi, omoSlimApi } from "@/lib/api/omo";

// ── Factory ────────────────────────────────────────────────────

function createOmoQueryKeys(prefix: string) {
  return {
    all: [prefix] as const,
    currentProviderId: () => [prefix, "current-provider-id"] as const,
  };
}

function createOmoQueryHooks(
  variant: "omo" | "omo-slim",
  api: typeof omoApi | typeof omoSlimApi,
) {
  const keys = createOmoQueryKeys(variant);

  function invalidateAll(queryClient: ReturnType<typeof useQueryClient>) {
    queryClient.invalidateQueries({ queryKey: ["providers"] });
    queryClient.invalidateQueries({ queryKey: keys.currentProviderId() });
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
    useCurrentProviderId,
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

export const useCurrentOmoProviderId = omoHooks.useCurrentProviderId;
export const useReadOmoLocalFile = omoHooks.useReadLocalFile;
export const useDisableCurrentOmo = omoHooks.useDisableCurrent;

export const useCurrentOmoSlimProviderId = omoSlimHooks.useCurrentProviderId;
export const useReadOmoSlimLocalFile = omoSlimHooks.useReadLocalFile;
export const useDisableCurrentOmoSlim = omoSlimHooks.useDisableCurrent;
