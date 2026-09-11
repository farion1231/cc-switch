import { useQuery, type QueryClient } from "@tanstack/react-query";
import { workbuddyApi, workbuddyKeys } from "@/lib/api/workbuddy";

/**
 * Query/invalidation helpers for WorkBuddy's model configuration.
 *
 * WorkBuddy is an *additive* app: one CC Switch provider maps to one gateway
 * (baseUrl + apiKey) inside `~/.workbuddy/models.json`. These hooks mirror the
 * Hermes/OpenClaw surface so cache invalidation stays consistent.
 */

/**
 * Invalidate every WorkBuddy cache that can change when a provider is
 * added/updated/deleted/switched. Runs in parallel to avoid sequential refetches.
 */
export function invalidateWorkbuddyProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: workbuddyKeys.liveProviderIds }),
    queryClient.invalidateQueries({ queryKey: workbuddyKeys.configHealth }),
  ]);
}

/**
 * Gateway-aggregated provider ids currently present in models.json,
 * used to decide whether a DB provider is already applied to the live config.
 */
export function useWorkbuddyLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: workbuddyKeys.liveProviderIds,
    queryFn: () => workbuddyApi.getLiveProviderIds(),
    enabled,
  });
}

/**
 * Human-readable problems detected in models.json. Empty array means healthy.
 */
export function useWorkbuddyConfigHealth(enabled: boolean) {
  return useQuery({
    queryKey: workbuddyKeys.configHealth,
    queryFn: () => workbuddyApi.scanConfigHealth(),
    staleTime: 60_000,
    enabled,
  });
}
