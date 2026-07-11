import { useQuery, type QueryClient } from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all MimoCode-related queries.
 */
export const mimocodeKeys = {
  all: ["mimo"] as const,
  liveProviderIds: ["mimo", "liveProviderIds"] as const,
};

/**
 * Invalidate all MimoCode caches that may change when a provider is
 * added/updated/deleted/switched.
 */
export function invalidateMimocodeProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: mimocodeKeys.liveProviderIds }),
  ]);
}

export function useMimocodeLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: mimocodeKeys.liveProviderIds,
    queryFn: () => providersApi.getMiMoCodeLiveProviderIds(),
    enabled,
  });
}
