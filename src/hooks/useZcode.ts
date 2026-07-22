import { useQuery, type QueryClient } from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all ZCode-related queries.
 * Import this from any file that needs to invalidate ZCode caches.
 */
export const zcodeKeys = {
  all: ["zcode"] as const,
  liveProviderIds: ["zcode", "liveProviderIds"] as const,
};

/**
 * Invalidate all ZCode caches that may change when a provider is
 * added/updated/deleted/switched.
 */
export function invalidateZcodeProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: zcodeKeys.liveProviderIds }),
  ]);
}

// ============================================================
// Query hooks
// ============================================================

export function useZcodeLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: zcodeKeys.liveProviderIds,
    queryFn: () => providersApi.getZCodeLiveProviderIds(),
    enabled,
  });
}
