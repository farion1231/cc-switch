import {
  useQuery,
  type QueryClient,
} from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all Kimi-related queries.
 * Import this from any file that needs to invalidate Kimi caches.
 */
export const kimiKeys = {
  all: ["kimi"] as const,
  liveProviderIds: ["kimi", "liveProviderIds"] as const,
  defaultModel: ["kimi", "defaultModel"] as const,
  currentProviderId: ["kimi", "currentProviderId"] as const,
};

/**
 * Invalidate all Kimi caches that may change when a provider is
 * added/updated/deleted/switched. Runs invalidations in parallel so the
 * caller doesn't await three sequential refetches.
 */
export function invalidateKimiProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: kimiKeys.liveProviderIds }),
    queryClient.invalidateQueries({ queryKey: kimiKeys.defaultModel }),
    queryClient.invalidateQueries({ queryKey: kimiKeys.currentProviderId }),
  ]);
}

// ============================================================
// Query hooks
// ============================================================

export function useKimiLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: kimiKeys.liveProviderIds,
    queryFn: () => providersApi.getKimiLiveProviderIds(),
    enabled,
  });
}

export function useKimiDefaultModel(enabled: boolean) {
  return useQuery({
    queryKey: kimiKeys.defaultModel,
    queryFn: () => providersApi.getKimiDefaultModel(),
    enabled,
  });
}

export function useKimiCurrentProviderId(enabled: boolean) {
  return useQuery({
    queryKey: kimiKeys.currentProviderId,
    queryFn: () => providersApi.getKimiCurrentProviderId(),
    enabled,
  });
}
