import { useCallback } from "react";
import { useQuery, type QueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { hermesApi } from "@/lib/api/hermes";
import { providersApi } from "@/lib/api/providers";
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
 * Returns a handler that probes the local Hermes Web UI, opens it in the
 * system browser, and surfaces a localized toast on failure. Callers only
 * need to wire the returned function to a click handler.
 */
export function useOpenHermesWebUI() {
  const { t } = useTranslation();
  return useCallback(
    async (path?: string) => {
      try {
        await hermesApi.openWebUI(path);
      } catch (error) {
        const detail = extractErrorMessage(error);
        if (detail === HERMES_WEB_OFFLINE_ERROR) {
          toast.error(t("hermes.webui.offline"));
        } else {
          toast.error(t("hermes.webui.openFailed"), {
            description: detail || undefined,
          });
        }
      }
    },
    [t],
  );
}
