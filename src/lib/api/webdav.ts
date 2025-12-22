import { invoke } from "@tauri-apps/api/core";

export interface WebDavConfig {
  url: string;
  username: string;
  password: string;
  remote_path: string;
}

export interface WebDavResult {
  success: boolean;
  message: string;
  filename?: string;
  backupId?: string;
}

export const webdavApi = {
  async saveConfig(config: WebDavConfig): Promise<WebDavResult> {
    return await invoke("save_webdav_config", { config });
  },

  async getConfig(): Promise<WebDavConfig | null> {
    return await invoke("get_webdav_config");
  },

  async testConnection(config: WebDavConfig): Promise<WebDavResult> {
    return await invoke("test_webdav_connection", { config });
  },

  async exportToWebDav(config: WebDavConfig): Promise<WebDavResult> {
    return await invoke("export_config_to_webdav", { config });
  },

  async importFromWebDav(
    config: WebDavConfig,
    filename: string,
  ): Promise<WebDavResult> {
    return await invoke("import_config_from_webdav", {
      config,
      filename,
    });
  },

  async listBackups(config: WebDavConfig): Promise<string[]> {
    return await invoke("list_webdav_backups", { config });
  },

  async deleteBackup(
    config: WebDavConfig,
    filename: string,
  ): Promise<WebDavResult> {
    return await invoke("delete_webdav_backup", {
      config,
      filename,
    });
  },
};
