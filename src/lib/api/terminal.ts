import { invoke } from "@tauri-apps/api/core";

export interface WtProfile {
  guid: string;
  name: string;
}

export const terminalApi = {
  async getWtProfiles(): Promise<WtProfile[]> {
    return await invoke("get_windows_terminal_profiles");
  },
};
