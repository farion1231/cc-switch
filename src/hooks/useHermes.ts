import { useCallback } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { hermesApi } from "@/lib/api/hermes";
import { providersApi } from "@/lib/api/providers";
import type { HermesMemoryKind } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * Error code returned by the Rust `open_hermes_web_ui` command when probing
 * `/api/status` fails. Must match the string constant in
 * `src-tauri/src/commands/hermes.rs`.
 */
export const HERMES_WEB_OFFLINE_ERROR = "hermes_web_offline";

/**
 * Centralized query keys for all Hermes-related queries.
 * Import this from any file that needs to invalidate Hermes caches.
 */
export const hermesKeys = {
  all: ["hermes"] as const,
  liveProviderIds: ["hermes", "liveProviderIds"] as const,
  modelConfig: ["hermes", "modelConfig"] as const,
  memory: (kind: HermesMemoryKind) => ["hermes", "memory", kind] as const,
  memoryLimits: ["hermes", "memoryLimits"] as const,
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
  ]);
}

// ============================================================
// Query hooks
// ============================================================

export function useHermesLiveProviderIds(enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.liveProviderIds,
    queryFn: () => providersApi.getHermesLiveProviderIds(),
    enabled,
  });
}

export function useHermesModelConfig(enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.modelConfig,
    queryFn: () => hermesApi.getModelConfig(),
    enabled,
  });
}

export function useHermesMemory(kind: HermesMemoryKind, enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.memory(kind),
    queryFn: () => hermesApi.getMemory(kind),
    enabled,
  });
}

export function useHermesMemoryLimits(enabled: boolean) {
  return useQuery({
    queryKey: hermesKeys.memoryLimits,
    queryFn: () => hermesApi.getMemoryLimits(),
    staleTime: 60_000,
    enabled,
  });
}

// ============================================================
// Mutation hooks
// ============================================================

/**
 * Save a Hermes memory file atomically and refresh the corresponding query.
 * Error toasts are emitted here so caller components don't need their own
 * try/catch; success toasts are intentionally left to the caller (to pick
 * the right localized message per tab).
 */
export function useSaveHermesMemory() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  return useMutation({
    mutationFn: ({
      kind,
      content,
    }: {
      kind: HermesMemoryKind;
      content: string;
    }) => hermesApi.setMemory(kind, content),
    onSuccess: async (_data, variables) => {
      await queryClient.invalidateQueries({
        queryKey: hermesKeys.memory(variables.kind),
      });
    },
    onError: (error) => {
      toast.error(t("hermes.memory.saveFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    },
  });
}

/**
 * Toggle one memory blob's on/off flag in Hermes' `config.yaml`. Invalidates
 * the limits query so the switch UI and disabled banner update immediately.
 */
export function useToggleHermesMemoryEnabled() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  return useMutation({
    mutationFn: ({
      kind,
      enabled,
    }: {
      kind: HermesMemoryKind;
      enabled: boolean;
    }) => hermesApi.setMemoryEnabled(kind, enabled),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: hermesKeys.memoryLimits,
      });
    },
    onError: (error) => {
      toast.error(t("hermes.memory.toggleFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    },
  });
}

/**
 * Returns a handler that probes the local Hermes Web UI, opens it in the
 * system browser, and surfaces a localized toast on failure. When
 * `onOffline` is provided, it replaces the default offline toast —
 * callers can use this to open a launch-dashboard confirm dialog instead.
 */
export function useOpenHermesWebUI(onOffline?: () => void) {
  const { t } = useTranslation();
  return useCallback(
    async (path?: string) => {
      try {
        await hermesApi.openWebUI(path);
      } catch (error) {
        const detail = extractErrorMessage(error);
        if (detail === HERMES_WEB_OFFLINE_ERROR) {
          if (onOffline) {
            onOffline();
          } else {
            toast.error(t("hermes.webui.offline"));
          }
        } else {
          toast.error(t("hermes.webui.openFailed"), {
            description: detail || undefined,
          });
        }
      }
    },
    [t, onOffline],
  );
}
