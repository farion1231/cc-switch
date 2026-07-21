import { invoke } from "@tauri-apps/api/core";
import type {
  PiProviderConfig,
  PiHealthWarning,
  PiWriteOutcome,
} from "@/types";

/**
 * Pi Coding Agent configuration API
 *
 * Manages ~/.pi/agent/models.json provider sections.
 * Active provider selection lives in settings.json via active provider commands.
 */
export const piApi = {
  // ============================================================
  // Directory & Path
  // ============================================================

  /**
   * Get Pi config directory.
   */
  async getDir(): Promise<string> {
    return await invoke("get_pi_dir");
  },

  /**
   * Get Pi config file path.
   */
  async getConfigPath(): Promise<string> {
    return await invoke("get_pi_config_path");
  },

  // ============================================================
  // Provider CRUD
  // ============================================================

  /**
   * Get all providers from Pi config.
   */
  async getProviders(): Promise<Record<string, PiProviderConfig>> {
    return await invoke("get_pi_providers");
  },

  /**
   * Get a single Pi provider by name.
   */
  async getProvider(
    providerName: string,
  ): Promise<PiProviderConfig | null> {
    return await invoke("get_pi_provider", { providerName });
  },

  /**
   * Upsert a Pi provider.
   */
  async setProvider(
    providerName: string,
    providerConfig: PiProviderConfig,
  ): Promise<PiWriteOutcome> {
    return await invoke("set_pi_provider", { providerName, providerConfig });
  },

  /**
   * Remove a Pi provider.
   */
  async removeProvider(
    providerName: string,
  ): Promise<PiWriteOutcome> {
    return await invoke("remove_pi_provider", { providerName });
  },

  // ============================================================
  // Active Provider
  // ============================================================

  /**
   * Get the active Pi provider name.
   */
  async getActiveProvider(): Promise<string | null> {
    return await invoke("get_pi_active_provider");
  },

  /**
   * Set the active Pi provider name.
   */
  async setActiveProvider(
    providerName: string,
  ): Promise<PiWriteOutcome> {
    return await invoke("set_pi_active_provider", { providerName });
  },

  // ============================================================
  // Live Config
  // ============================================================

  /**
   * Import providers from live Pi config into CC Switch database.
   */
  async importFromLive(): Promise<number> {
    return await invoke("import_pi_providers_from_live");
  },

  /**
   * Get provider IDs present in live Pi config.
   */
  async getLiveProviderIds(): Promise<string[]> {
    return await invoke("get_pi_live_provider_ids");
  },

  /**
   * Get a single provider from live Pi config.
   */
  async getLiveProvider(
    providerId: string,
  ): Promise<PiProviderConfig | null> {
    return await invoke("get_pi_live_provider", { providerId });
  },

  // ============================================================
  // Health
  // ============================================================

  /**
   * Scan Pi config for known configuration hazards.
   */
  async scanHealth(): Promise<PiHealthWarning[]> {
    return await invoke("scan_pi_config_health");
  },
};
