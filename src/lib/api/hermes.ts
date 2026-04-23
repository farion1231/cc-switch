import { invoke } from "@tauri-apps/api/core";
import type {
  HermesHealthWarning,
  HermesMemoryKind,
  HermesMemoryLimits,
  HermesModelConfig,
} from "@/types";

/**
 * Hermes Agent configuration API (CC Switch side).
 *
 * CC Switch intentionally keeps its Hermes surface minimal — deep configuration
 * (model, agent behavior, env vars, skills, cron, logs, analytics) lives in
 * the Hermes Web UI at http://127.0.0.1:9119. CC Switch only reads the `model`
 * section to highlight the active provider, scans config health, and launches
 * the Hermes Web UI for everything else. Writes to `model` happen implicitly
 * via `apply_switch_defaults` when the user switches providers.
 */
export const hermesApi = {
  async getModelConfig(): Promise<HermesModelConfig | null> {
    return await invoke("get_hermes_model_config");
  },

  async scanHealth(): Promise<HermesHealthWarning[]> {
    return await invoke("scan_hermes_config_health");
  },

  /**
   * Probe the local Hermes Web UI and open it in the system browser.
   * Optional `path` lets callers deep-link to specific pages like `/config`.
   */
  async openWebUI(path?: string): Promise<void> {
    await invoke("open_hermes_web_ui", { path: path ?? null });
  },

  /** Open the preferred terminal and run `hermes dashboard` (non-blocking). */
  async launchDashboard(): Promise<void> {
    await invoke("launch_hermes_dashboard");
  },

  /**
   * Read one of Hermes' memory blobs (`MEMORY.md` or `USER.md`). Returns an
   * empty string when the file hasn't been created yet.
   */
  async getMemory(kind: HermesMemoryKind): Promise<string> {
    return await invoke("get_hermes_memory", { kind });
  },

  /** Atomically overwrite a Hermes memory file. */
  async setMemory(kind: HermesMemoryKind, content: string): Promise<void> {
    await invoke("set_hermes_memory", { kind, content });
  },

  /**
   * Character budgets + enable flags for both memory blobs, read from
   * config.yaml with Hermes defaults as fallback.
   */
  async getMemoryLimits(): Promise<HermesMemoryLimits> {
    return await invoke("get_hermes_memory_limits");
  },

  /**
   * Toggle the on/off flag for one memory blob. Other fields in the `memory:`
   * section (budgets, external provider config) are preserved.
   */
  async setMemoryEnabled(
    kind: HermesMemoryKind,
    enabled: boolean,
  ): Promise<void> {
    await invoke("set_hermes_memory_enabled", { kind, enabled });
  },
};
