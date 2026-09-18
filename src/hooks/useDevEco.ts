import { useQuery } from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all DevEco-related queries.
 */
export const devecoKeys = {
  all: ["deveco"] as const,
  liveProviderIds: ["deveco", "liveProviderIds"] as const,
};

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
