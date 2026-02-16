import { invoke } from "@tauri-apps/api/core";

export const workspaceApi = {
  async readFile(filename: string): Promise<string | null> {
    return invoke<string | null>("read_workspace_file", { filename });
  },

  async writeFile(filename: string, content: string): Promise<void> {
    return invoke<void>("write_workspace_file", { filename, content });
  },
};
