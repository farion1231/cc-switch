import { invoke } from "@tauri-apps/api/core";

/**
 * WorkBuddy custom-model configuration API (CC Switch side).
 *
 * WorkBuddy stores all custom models in a single flat JSON array at
 * `~/.workbuddy/models.json` (a file the user often symlinks to a cloud drive
 * for sync). CC Switch treats WorkBuddy as an *additive* app — one CC Switch
 * "provider" maps to one gateway (base_url + api_key) and owns the set of
 * models declared under it. Writes flatten every provider's models back into
 * models.json while preserving the symlink.
 *
 * Query keys mirror the hermes surface so react-query invalidation stays
 * consistent across additive apps.
 */
export const workbuddyKeys = {
  liveProviderIds: ["workbuddy", "liveProviderIds"] as const,
  liveProvider: (providerId: string) =>
    ["workbuddy", "liveProvider", providerId] as const,
  configHealth: ["workbuddy", "configHealth"] as const,
};

export const workbuddyApi = {
  /**
   * Import providers already present in the live models.json into the DB.
   * Returns the number of providers imported/updated.
   */
  async importProvidersFromLive(): Promise<number> {
    return await invoke("import_workbuddy_providers_from_live");
  },

  /**
   * List the gateway-aggregated provider ids currently in models.json
   * (e.g. `openrouter`, `deepseek`).
   */
  async getLiveProviderIds(): Promise<string[]> {
    return await invoke("get_workbuddy_live_provider_ids");
  },

  /**
   * Read a single aggregated provider's config snippet
   * (`{ baseUrl, apiKey, models }`) or null when it no longer exists.
   */
  async getLiveProvider(providerId: string): Promise<unknown | null> {
    return await invoke("get_workbuddy_live_provider", { providerId });
  },

  /**
   * Lightweight health scan of models.json. Returns a list of human-readable
   * problems; an empty array means healthy.
   */
  async scanConfigHealth(): Promise<string[]> {
    return await invoke("scan_workbuddy_config_health");
  },
};
