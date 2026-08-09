import {
  useQuery,
  type QueryClient,
} from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all Pi-related queries.
 * Import this from any file that needs to invalidate Pi caches.
 */
export const piKeys = {
  all: ["pi"] as const,
  liveProviderIds: ["pi", "liveProviderIds"] as const,
};

/**
 * Invalidate all Pi caches that may change when a provider is
 * added/updated/deleted/added-to-config.
 */
export function invalidatePiProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: piKeys.liveProviderIds }),
  ]);
}

// ============================================================
// Query hooks
// ============================================================

export function usePiLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: piKeys.liveProviderIds,
    queryFn: () => providersApi.getPiLiveProviderIds(),
    enabled,
  });
}
