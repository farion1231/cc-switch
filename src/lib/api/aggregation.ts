import { invoke } from "@tauri-apps/api/core";
import type { Provider, UniversalProvider } from "@/types";
import type { AppId } from "./types";

export interface AggregationAppProviderItem {
  appType: AppId;
  provider: Provider;
  baseUrl?: string;
  apiFormat: string;
  models: string[];
  isOfficial: boolean;
  canSync?: boolean;
  isCurrentInApp: boolean;
  inStack: boolean;
  syncedApps: string[];
}

export interface AggregationAppState {
  appType: AppId;
  supportsLocalProxy: boolean;
  supportsStackCatalog: boolean;
  additive: boolean;
  mode: "direct" | "route" | "stack" | "additive" | "launch";
  attached: boolean;
  routeProviderId?: string | null;
  stackMemberCount: number;
  syncedProviderCount: number;
  assignedProviderCount?: number;
}

export interface AggregationOverview {
  enabled: boolean;
  proxyRunning: boolean;
  proxyAddress: string;
  proxyPort: number;
  universalProviders: UniversalProvider[];
  appProviders: AggregationAppProviderItem[];
  appStates: AggregationAppState[];
}

export interface SyncAllResult {
  syncedApps: string[];
  warnings: string[];
}

export const aggregationApi = {
  async getOverview(): Promise<AggregationOverview> {
    return await invoke("get_aggregation_overview");
  },

  async setGlobalMode(enabled: boolean): Promise<AggregationOverview> {
    return await invoke("set_global_aggregation_mode", { enabled });
  },

  async saveAndSyncUniversal(
    provider: UniversalProvider,
  ): Promise<SyncAllResult> {
    return await invoke("save_and_sync_universal_provider", { provider });
  },

  async setProviderApps(
    sourceApp: string | null,
    providerId: string,
    targetApps: AppId[],
  ): Promise<SyncAllResult> {
    return await invoke("set_aggregation_provider_apps", {
      sourceApp,
      providerId,
      targetApps,
    });
  },

  async promoteAppProviderToUniversal(
    sourceApp: string,
    sourceProviderId: string,
    provider: UniversalProvider,
  ): Promise<SyncAllResult> {
    return await invoke("promote_app_provider_to_universal", {
      sourceApp,
      sourceProviderId,
      provider,
    });
  },

  async deleteProvider(
    sourceApp: string | null,
    providerId: string,
  ): Promise<SyncAllResult> {
    return await invoke("delete_aggregation_provider", {
      sourceApp,
      providerId,
    });
  },

  async setAppMode(
    appType: string,
    enabled: boolean,
    routeProviderId?: string | null,
  ): Promise<void> {
    return await invoke("set_aggregation_app_mode", {
      appType,
      enabled,
      routeProviderId: routeProviderId ?? null,
    });
  },
};
