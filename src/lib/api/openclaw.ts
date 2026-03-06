import { invoke } from "@tauri-apps/api/core";
import type {
  OpenClawDefaultModel,
  OpenClawModelCatalogEntry,
  OpenClawAgentsDefaults,
  OpenClawEnvConfig,
  OpenClawToolsConfig,
} from "@/types";

/**
 * OpenClaw configuration API
 *
 * Manages ~/.openclaw/openclaw.json sections:
 * - agents.defaults (model, catalog)
 * - env (environment variables)
 * - tools (permissions)
 */
export const openclawApi = {
  // ============================================================
  // Agents Configuration
  // ============================================================

  /**
   * Get default model configuration (agents.defaults.model)
   */
  async getDefaultModel(): Promise<OpenClawDefaultModel | null> {
    return await invoke("get_openclaw_default_model");
  },

  /**
   * Set default model configuration (agents.defaults.model)
   */
  async setDefaultModel(model: OpenClawDefaultModel): Promise<void> {
    return await invoke("set_openclaw_default_model", { model });
  },

  /**
   * Get model catalog/allowlist (agents.defaults.models)
   */
  async getModelCatalog(): Promise<Record<
    string,
    OpenClawModelCatalogEntry
  > | null> {
    return await invoke("get_openclaw_model_catalog");
  },

  /**
   * Set model catalog/allowlist (agents.defaults.models)
   */
  async setModelCatalog(
    catalog: Record<string, OpenClawModelCatalogEntry>,
  ): Promise<void> {
    return await invoke("set_openclaw_model_catalog", { catalog });
  },

  /**
   * Get full agents.defaults config (all fields)
   */
  async getAgentsDefaults(): Promise<OpenClawAgentsDefaults | null> {
    return await invoke("get_openclaw_agents_defaults");
  },

  /**
   * Set full agents.defaults config (all fields)
   */
  async setAgentsDefaults(defaults: OpenClawAgentsDefaults): Promise<void> {
    return await invoke("set_openclaw_agents_defaults", { defaults });
  },

  // ============================================================
  // Env Configuration
  // ============================================================

  /**
   * Get env config (env section of openclaw.json)
   */
  async getEnv(): Promise<OpenClawEnvConfig> {
    return await invoke("get_openclaw_env");
  },

  /**
   * Set env config (env section of openclaw.json)
   */
  async setEnv(env: OpenClawEnvConfig): Promise<void> {
    return await invoke("set_openclaw_env", { env });
  },

  // ============================================================
  // Tools Configuration
  // ============================================================

  /**
   * Get tools config (tools section of openclaw.json)
   */
  async getTools(): Promise<OpenClawToolsConfig> {
    return await invoke("get_openclaw_tools");
  },

  /**
   * Set tools config (tools section of openclaw.json)
   */
  async setTools(tools: OpenClawToolsConfig): Promise<void> {
    return await invoke("set_openclaw_tools", { tools });
  },
};
