import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openclawApi } from "@/lib/api/openclaw";
import { providersApi } from "@/lib/api/providers";
import type {
  OpenClawEnvConfig,
  OpenClawToolsConfig,
  OpenClawAgentsDefaults,
} from "@/types";

/**
 * Centralized query keys for all OpenClaw-related queries.
 * Import this from any file that needs to invalidate OpenClaw caches.
 */
export const openclawKeys = {
  all: ["openclaw"] as const,
  liveProviderIds: ["openclaw", "liveProviderIds"] as const,
  defaultModel: ["openclaw", "defaultModel"] as const,
  env: ["openclaw", "env"] as const,
  tools: ["openclaw", "tools"] as const,
  agentsDefaults: ["openclaw", "agentsDefaults"] as const,
};

// ============================================================
// Query hooks
// ============================================================

/**
 * Query live provider IDs from openclaw.json config.
 * Used by ProviderList to show "In Config" badge.
 */
export function useOpenClawLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: openclawKeys.liveProviderIds,
    queryFn: () => providersApi.getOpenClawLiveProviderIds(),
    enabled,
  });
}

/**
 * Query the default model from agents.defaults.model.
 * Used by ProviderList to show which provider is the default.
 */
export function useOpenClawDefaultModel(enabled: boolean) {
  return useQuery({
    queryKey: openclawKeys.defaultModel,
    queryFn: () => openclawApi.getDefaultModel(),
    enabled,
  });
}

/**
 * Query env section of openclaw.json.
 */
export function useOpenClawEnv() {
  return useQuery({
    queryKey: openclawKeys.env,
    queryFn: () => openclawApi.getEnv(),
    staleTime: 30_000,
  });
}

/**
 * Query tools section of openclaw.json.
 */
export function useOpenClawTools() {
  return useQuery({
    queryKey: openclawKeys.tools,
    queryFn: () => openclawApi.getTools(),
    staleTime: 30_000,
  });
}

/**
 * Query agents.defaults section of openclaw.json.
 */
export function useOpenClawAgentsDefaults() {
  return useQuery({
    queryKey: openclawKeys.agentsDefaults,
    queryFn: () => openclawApi.getAgentsDefaults(),
    staleTime: 30_000,
  });
}

// ============================================================
// Mutation hooks
// ============================================================

/**
 * Save env config. Invalidates env query on success.
 * Toast notifications are handled by the component.
 */
export function useSaveOpenClawEnv() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (env: OpenClawEnvConfig) => openclawApi.setEnv(env),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: openclawKeys.env });
    },
  });
}

/**
 * Save tools config. Invalidates tools query on success.
 * Toast notifications are handled by the component.
 */
export function useSaveOpenClawTools() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (tools: OpenClawToolsConfig) => openclawApi.setTools(tools),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: openclawKeys.tools });
    },
  });
}

/**
 * Save agents.defaults config. Invalidates both agentsDefaults and defaultModel
 * queries on success (since changing agents.defaults may affect the default model).
 * Toast notifications are handled by the component.
 */
export function useSaveOpenClawAgentsDefaults() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (defaults: OpenClawAgentsDefaults) =>
      openclawApi.setAgentsDefaults(defaults),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: openclawKeys.agentsDefaults });
      queryClient.invalidateQueries({ queryKey: openclawKeys.defaultModel });
    },
  });
}
