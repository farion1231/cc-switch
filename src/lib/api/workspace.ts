import { invoke } from "@tauri-apps/api/core";

export interface DailyMemoryFileInfo {
  filename: string;
  date: string;
  sizeBytes: number;
  modifiedAt: number;
  preview: string;
}

export const workspaceApi = {
  async readFile(filename: string): Promise<string | null> {
    return invoke<string | null>("read_workspace_file", { filename });
  },

  async writeFile(filename: string, content: string): Promise<void> {
    return invoke<void>("write_workspace_file", { filename, content });
  },

  async listDailyMemoryFiles(): Promise<DailyMemoryFileInfo[]> {
    return invoke<DailyMemoryFileInfo[]>("list_daily_memory_files");
  },

  async readDailyMemoryFile(filename: string): Promise<string | null> {
    return invoke<string | null>("read_daily_memory_file", { filename });
  },

  async writeDailyMemoryFile(filename: string, content: string): Promise<void> {
    return invoke<void>("write_daily_memory_file", { filename, content });
  },

  async deleteDailyMemoryFile(filename: string): Promise<void> {
    return invoke<void>("delete_daily_memory_file", { filename });
  },
};
