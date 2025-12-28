import { invoke } from "@tauri-apps/api/core";

export interface ConfigStatus {
  exists: boolean;
  path: string;
}

export const droidApi = {
  // 读取 settings.json (运行时配置)
  async getSettings(): Promise<Record<string, unknown>> {
    return await invoke("get_droid_settings");
  },

  // 获取配置状态 (检查 config.json)
  async getConfigStatus(): Promise<ConfigStatus> {
    return await invoke("get_droid_config_status");
  },

  // 读取 config.json (主配置文件)
  async getConfig(): Promise<Record<string, unknown>> {
    return await invoke("get_droid_config");
  },

  // 写入 config.json (主配置文件)
  async setConfig(config: Record<string, unknown>): Promise<boolean> {
    return await invoke("set_droid_config", { config });
  },

  // 清理 settings.json 以让新配置生效
  // 删除 customModels 空列表和 sessionDefaultSettings.model
  async cleanupSettings(): Promise<boolean> {
    return await invoke("cleanup_droid_settings");
  },

  // 获取 config.json 路径
  async getConfigPath(): Promise<string> {
    return await invoke("get_droid_config_path");
  },
};
