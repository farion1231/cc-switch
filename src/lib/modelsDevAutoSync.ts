import { usageApi } from "@/lib/api/usage";
import {
  fetchModelsDevPricing,
  flattenModels,
  resolveModelsDevSelection,
  toModelPricing,
} from "@/lib/modelsDevPricing";
import type { ModelsDevSyncState } from "@/types/usage";
import {
  runtimeQueryScope,
  type RuntimeQueryScope,
} from "@/lib/runtime/queryScope";
import { RuntimeInvokeError } from "@/lib/runtime/invoke";

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
export const modelsDevSyncConfigQueryKey = (
  scope: RuntimeQueryScope = runtimeQueryScope(),
) => [...MODELS_DEV_SYNC_CONFIG_QUERY_KEY, ...scope] as const;
export const MODELS_DEV_STARTUP_SYNC_INTERVAL_MS = 6 * 60 * 60 * 1000;

const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

function sameRuntimeScope(expected: RuntimeQueryScope): boolean {
  const current = runtimeQueryScope();
  return (
    current.length === expected.length &&
    current.every((part, index) => part === expected[index])
  );
}

/**
 * models.dev 同步跨越网络下载和多次 RPC，必须固定在启动时的目标快照上。
 * 单次 appInvoke 只能防住请求执行期间的切换，无法阻止两次 RPC 之间换主机。
 */
function assertRuntimeScope(expected: RuntimeQueryScope): void {
  if (!sameRuntimeScope(expected)) {
    throw new RuntimeInvokeError(
      "REMOTE_TARGET_CHANGED",
      "运行目标已切换，已取消 models.dev 同步以避免跨主机写入",
    );
  }
}

export async function syncModelsDevPricing(
  state?: ModelsDevSyncState,
  force = false,
): Promise<ModelsDevSyncResult> {
  const operationScope = runtimeQueryScope();
  const initialState = state ?? (await usageApi.getModelsDevSyncConfig());
  const recentlySynced =
    initialState.config.lastSyncAt !== null &&
    Date.now() - initialState.config.lastSyncAt <
      MODELS_DEV_STARTUP_SYNC_INTERVAL_MS;
  if (!force && (!initialState.config.autoSyncEnabled || recentlySynced)) {
    return {
      skipped: true,
      selected: 0,
      imported: 0,
      changed: 0,
      syncedAt: initialState.config.lastSyncAt,
    };
  }

  try {
    const data = await fetchModelsDevPricing();
    assertRuntimeScope(operationScope);
    const latestState = await usageApi.getModelsDevSyncConfig();
    assertRuntimeScope(operationScope);
    if (!force && !latestState.config.autoSyncEnabled) {
      return {
        skipped: true,
        selected: 0,
        imported: 0,
        changed: 0,
        syncedAt: latestState.config.lastSyncAt,
      };
    }
    const selectedEntries = resolveModelsDevSelection(
      flattenModels(data),
      latestState.config,
    );
    const pricing = toModelPricing(selectedEntries);
    const changed = pricing.length
      ? await usageApi.updateModelPricingBatch(pricing)
      : 0;
    assertRuntimeScope(operationScope);
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
    // 目标已经切换时不能把旧操作的错误写到新主机；同一目标内的普通网络错误仍应持久化。
    if (sameRuntimeScope(operationScope)) {
      try {
        await usageApi.recordModelsDevSyncResult(null, errorMessage(error));
      } catch (saveError) {
        console.warn(
          "[models.dev] Failed to persist automatic sync error",
          saveError,
        );
      }
    }
    throw error;
  }
}

let startupSync: Promise<ModelsDevSyncResult> | null = null;

/** Run once per renderer and at most once per interval across WebView rebuilds. */
export function syncModelsDevPricingOnStartup(): Promise<ModelsDevSyncResult> {
  startupSync ??= syncModelsDevPricing();
  return startupSync;
}
