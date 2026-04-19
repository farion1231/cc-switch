import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { hermesApi } from "@/lib/api/hermes";
import { providersApi } from "@/lib/api/providers";
import type {
  HermesEnvConfig,
  HermesAgentConfig,
  HermesModelConfig,
} from "@/types";

/**
 * Centralized query keys for all Hermes-related queries.
 * Import this from any file that needs to invalidate Hermes caches.
 */
export const hermesKeys = {
  all: ["hermes"] as const,
  liveProviderIds: ["hermes", "liveProviderIds"] as const,
  modelConfig: ["hermes", "modelConfig"] as const,
  agentConfig: ["hermes", "agentConfig"] as const,
  env: ["hermes", "env"] as const,
  health: ["hermes", "health"] as const,
};

/**
 * Invalidate all Hermes caches that may change when a provider is
 * added/updated/deleted/switched. Runs invalidations in parallel so the
 * caller doesn't await three sequential refetches.
 */
export function invalidateHermesProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: hermesKeys.liveProviderIds }),
    queryClient.invalidateQueries({ queryKey: hermesKeys.modelConfig }),
    queryClient.invalidateQueries({ queryKey: hermesKeys.health }),
  ]);
}

// ============================================================
// Query hooks
// ============================================================

/**
 * Query live provider IDs from Hermes config.
 * Used by ProviderList to show "In Config" badge.
 */
export function useHermesLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.liveProviderIds,
    queryFn: () => providersApi.getHermesLiveProviderIds(),
    enabled,
  });
}

/**
 * Query model configuration.
 */
export function useHermesModelConfig(enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.modelConfig,
    queryFn: () => hermesApi.getModelConfig(),
    enabled,
  });
}

/**
 * Query agent configuration.
 */
export function useHermesAgentConfig(enabled = true) {
  return useQuery({
    queryKey: hermesKeys.agentConfig,
    queryFn: () => hermesApi.getAgentConfig(),
    staleTime: 30_000,
    enabled,
  });
}

/**
 * Query env configuration.
 */
export function useHermesEnv(enabled = true) {
  return useQuery({
    queryKey: hermesKeys.env,
    queryFn: () => hermesApi.getEnv(),
    staleTime: 30_000,
    enabled,
  });
}

/**
 * Query config health warnings.
 */
export function useHermesHealth(enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.health,
    queryFn: () => hermesApi.scanHealth(),
    staleTime: 30_000,
    enabled,
  });
}

// ============================================================
// Mutation hooks
// ============================================================

/**
 * Save model config. Invalidates modelConfig and health queries on success.
 * Toast notifications are handled by the component.
 */
export function useSaveHermesModelConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (config: HermesModelConfig) => hermesApi.setModelConfig(config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: hermesKeys.modelConfig });
      queryClient.invalidateQueries({ queryKey: hermesKeys.health });
    },
  });
}

/**
 * Save agent config. Invalidates agentConfig and health queries on success.
 * Toast notifications are handled by the component.
 */
export function useSaveHermesAgentConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (config: HermesAgentConfig) => hermesApi.setAgentConfig(config),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: hermesKeys.agentConfig });
      queryClient.invalidateQueries({ queryKey: hermesKeys.health });
    },
  });
}

/**
 * Save env config. Invalidates env and health queries on success.
 * Toast notifications are handled by the component.
 */
export function useSaveHermesEnv() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (env: HermesEnvConfig) => hermesApi.setEnv(env),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: hermesKeys.env });
      queryClient.invalidateQueries({ queryKey: hermesKeys.health });
    },
  });
}
