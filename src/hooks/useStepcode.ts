import { useQuery } from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Centralized query keys for all StepCode-related queries.
 * Import this from any file that needs to invalidate StepCode caches.
 */
export const stepcodeKeys = {
  all: ["stepcode"] as const,
  liveProviderIds: ["stepcode", "liveProviderIds"] as const,
};

/**
 * Query live provider IDs from the StepCode live config (~/.stepcode/models.json).
 * Used by ProviderList to show the "In Config" badge and by ProviderForm to
 * determine whether a provider key is locked.
 */
export function useStepcodeLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: stepcodeKeys.liveProviderIds,
    queryFn: () => providersApi.getStepcodeLiveProviderIds(),
    enabled,
  });
}
