import { invoke } from "@tauri-apps/api/core";

export interface PluginState {
  plugin_id: string;
  enabled: boolean;
  install_path: string;
  scope: string;
  version: string | null;
}

export const pluginsApi = {
  async list(): Promise<PluginState[]> {
    return await invoke("list_plugins");
  },

  async setEnabled(pluginId: string, enabled: boolean): Promise<boolean> {
    return await invoke("set_plugin_enabled", { pluginId, enabled });
  },
};
