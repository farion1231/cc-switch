import { usageApi } from "@/lib/api/usage";
import {
  fetchModelsDevPricing,
  flattenModels,
  resolveModelsDevSelection,
  toModelPricing,
} from "@/lib/modelsDevPricing";
import type { ModelsDevSyncState } from "@/types/usage";

export interface ModelsDevSyncResult {
  skipped: boolean;
  selected: number;
  imported: number;
  changed: number;
  syncedAt: number | null;
}

export const MODELS_DEV_SYNC_CONFIG_QUERY_KEY = [
  "models-dev-sync-config",
] as const;

const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

export async function syncModelsDevPricing(
  state?: ModelsDevSyncState,
  force = false,
): Promise<ModelsDevSyncResult> {
  const currentState = state ?? (await usageApi.getModelsDevSyncConfig());
  if (!force && !currentState.config.autoSyncEnabled) {
    return {
      skipped: true,
      selected: 0,
      imported: 0,
      changed: 0,
      syncedAt: currentState.config.lastSyncAt,
    };
  }

  try {
    const data = await fetchModelsDevPricing();
    const selectedEntries = resolveModelsDevSelection(
      flattenModels(data),
      currentState.config,
    );
    const pricing = toModelPricing(selectedEntries);
    const changed = pricing.length
      ? await usageApi.updateModelPricingBatch(pricing)
      : 0;
    const syncedAt = Date.now();
    await usageApi.recordModelsDevSyncResult(syncedAt, null);
    return {
      skipped: false,
      selected: selectedEntries.length,
      imported: pricing.length,
      changed,
      syncedAt,
    };
  } catch (error) {
    try {
      await usageApi.recordModelsDevSyncResult(null, errorMessage(error));
    } catch (saveError) {
      console.warn(
        "[models.dev] Failed to persist automatic sync error",
        saveError,
      );
    }
    throw error;
  }
}

let startupSync: Promise<ModelsDevSyncResult> | null = null;

/** Run at most once per renderer lifetime, including React StrictMode. */
export function syncModelsDevPricingOnStartup(): Promise<ModelsDevSyncResult> {
  startupSync ??= syncModelsDevPricing();
  return startupSync;
}
