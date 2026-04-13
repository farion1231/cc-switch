import { invoke } from "@tauri-apps/api/core";

export interface InstalledEditor {
  id: string;
  name: string;
  installed: boolean;
  exePath?: string;
  source?: string;
}

export const editorsApi = {
  async listInstalledEditors(): Promise<InstalledEditor[]> {
    return await invoke("list_installed_editors");
  },
};

