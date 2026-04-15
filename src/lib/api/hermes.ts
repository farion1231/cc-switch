import { invoke } from "@tauri-apps/api/core";
import type {
  HermesModelConfig,
  HermesAgentConfig,
  HermesEnvConfig,
  HermesHealthWarning,
  HermesWriteOutcome,
} from "@/types";

/**
 * Hermes Agent configuration API
 *
 * Manages Hermes config sections:
 * - model (model selection and provider)
 * - agent (agent behavior)
 * - env (environment variables)
 */
export const hermesApi = {
  // ============================================================
  // Model Configuration
  // ============================================================

  /**
   * Get model configuration
   */
  async getModelConfig(): Promise<HermesModelConfig | null> {
    return await invoke("get_hermes_model_config");
  },

  /**
   * Set model configuration
   */
  async setModelConfig(config: HermesModelConfig): Promise<HermesWriteOutcome> {
    return await invoke("set_hermes_model_config", { config });
  },

  // ============================================================
  // Agent Configuration
  // ============================================================

  /**
   * Get agent configuration
   */
  async getAgentConfig(): Promise<HermesAgentConfig | null> {
    return await invoke("get_hermes_agent_config");
  },

  /**
   * Set agent configuration
   */
  async setAgentConfig(config: HermesAgentConfig): Promise<HermesWriteOutcome> {
    return await invoke("set_hermes_agent_config", { config });
  },

  // ============================================================
  // Env Configuration
  // ============================================================

  /**
   * Get env configuration (.env file)
   */
  async getEnv(): Promise<HermesEnvConfig> {
    return await invoke("get_hermes_env");
  },

  /**
   * Set env configuration (.env file)
   */
  async setEnv(env: HermesEnvConfig): Promise<HermesWriteOutcome> {
    return await invoke("set_hermes_env", { env });
  },

  // ============================================================
  // Health
  // ============================================================

  /**
   * Scan config health and return warnings
   */
  async scanHealth(): Promise<HermesHealthWarning[]> {
    return await invoke("scan_hermes_config_health");
  },

  /**
   * Get live provider config by ID
   */
  async getLiveProvider(
    providerId: string,
  ): Promise<Record<string, unknown> | null> {
    return await invoke("get_hermes_live_provider", { providerId });
  },
};
