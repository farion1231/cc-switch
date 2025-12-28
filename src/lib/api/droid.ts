import { invoke } from "@tauri-apps/api/core";

export interface ConfigStatus {
  exists: boolean;
  path: string;
}

export const droidApi = {
  async getSettings(): Promise<Record<string, unknown>> {
    return await invoke("get_droid_settings");
  },

  async getConfigStatus(): Promise<ConfigStatus> {
    return await invoke("get_droid_config_status");
  },
};
