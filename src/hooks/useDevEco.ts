import { useQuery, type QueryClient } from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all DevEco-related queries.
 */
export const devecoKeys = {
  all: ["deveco"] as const,
  liveProviderIds: ["deveco", "liveProviderIds"] as const,
};

/**
 * Invalidate DevEco caches that change when a provider is added, switched,
 * deleted or removed from the native config.
 *
 * `liveProviderIds` is a query of its own, so refreshing the `providers` list
 * does not refresh it and it has no polling: without invalidation the cards keep
 * showing a stale "in config" badge (added stays un-added, removed stays added),
 * leaving the page unable to perform the opposite action until remounted.
 */
export function invalidateDevEcoProviderCaches(queryClient: QueryClient) {
  return queryClient.invalidateQueries({
    queryKey: devecoKeys.liveProviderIds,
  });
}

/**
 * Query live provider IDs from the deveco native config.
 * Used by ProviderList to show the "In Config" badge for additive-mode apps.
 */
export function useDevEcoLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: devecoKeys.liveProviderIds,
    queryFn: () => providersApi.getDevEcoLiveProviderIds(),
    enabled,
  });
}
