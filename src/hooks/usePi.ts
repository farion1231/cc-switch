import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { piApi } from "@/lib/api/pi";
import type { PiProviderConfig } from "@/types";

/**
 * Centralized query keys for all Pi-related queries.
 */
export const piKeys = {
  all: ["pi"] as const,
  providers: ["pi", "providers"] as const,
  provider: (name: string) => ["pi", "provider", name] as const,
  activeProvider: ["pi", "activeProvider"] as const,
  health: ["pi", "health"] as const,
  dir: ["pi", "dir"] as const,
  configPath: ["pi", "configPath"] as const,
  liveProviderIds: ["pi", "liveProviderIds"] as const,
};

/**
 * Invalidate all Pi caches that may change when a provider is
 * added/updated/deleted/switched.
 */
export function invalidatePiProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: piKeys.liveProviderIds }),
    queryClient.invalidateQueries({ queryKey: piKeys.providers }),
    queryClient.invalidateQueries({ queryKey: piKeys.activeProvider }),
    queryClient.invalidateQueries({ queryKey: piKeys.health }),
  ]);
}

// ============================================================
// Query hooks
// ============================================================

/**
 * Query live provider IDs from Pi config.
 * Used by ProviderList to show "In Config" badge.
 */
export function usePiLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: piKeys.liveProviderIds,
    queryFn: () => piApi.getLiveProviderIds(),
    enabled,
  });
}

/**
 * Query all providers from Pi config.
 */
export function usePiProviders() {
  return useQuery({
    queryKey: piKeys.providers,
    queryFn: () => piApi.getProviders(),
    staleTime: 30_000,
  });
}

/**
 * Query a single Pi provider by name.
 */
export function usePiProvider(providerName: string | null) {
  return useQuery({
    queryKey: piKeys.provider(providerName ?? ""),
    queryFn: () => piApi.getProvider(providerName!),
    enabled: !!providerName,
    staleTime: 30_000,
  });
}

/**
 * Query the active Pi provider name.
 */
export function usePiActiveProvider(enabled = true) {
  return useQuery({
    queryKey: piKeys.activeProvider,
    queryFn: () => piApi.getActiveProvider(),
    staleTime: 30_000,
    enabled,
  });
}

/**
 * Query Pi config health.
 */
export function usePiHealth(enabled: boolean) {
  return useQuery({
    queryKey: piKeys.health,
    queryFn: () => piApi.scanHealth(),
    staleTime: 30_000,
    enabled,
  });
}

/**
 * Query Pi config directory.
 */
export function usePiDir() {
  return useQuery({
    queryKey: piKeys.dir,
    queryFn: () => piApi.getDir(),
    staleTime: Infinity,
  });
}

/**
 * Query Pi config file path.
 */
export function usePiConfigPath() {
  return useQuery({
    queryKey: piKeys.configPath,
    queryFn: () => piApi.getConfigPath(),
    staleTime: Infinity,
  });
}

// ============================================================
// Mutation hooks
// ============================================================

/**
 * Import providers from live Pi config into CC Switch.
 */
export function useImportPiFromLive() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => piApi.importFromLive(),
    onSuccess: async () => {
      await invalidatePiProviderCaches(queryClient);
    },
  });
}

/**
 * Upsert a Pi provider. Invalidates providers and health on success.
 */
export function useSetPiProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      providerName,
      providerConfig,
    }: {
      providerName: string;
      providerConfig: PiProviderConfig;
    }) => piApi.setProvider(providerName, providerConfig),
    onSuccess: async () => {
      await invalidatePiProviderCaches(queryClient);
    },
  });
}

/**
 * Remove a Pi provider. Invalidates providers and health on success.
 */
export function useRemovePiProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (providerName: string) => piApi.removeProvider(providerName),
    onSuccess: async () => {
      await invalidatePiProviderCaches(queryClient);
    },
  });
}

/**
 * Set the active Pi provider. Invalidates activeProvider and health on success.
 */
export function useSetPiActiveProvider() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (providerName: string) =>
      piApi.setActiveProvider(providerName),
    onSuccess: async () => {
      await invalidatePiProviderCaches(queryClient);
    },
  });
}
