import { get, put, del } from "../web-client";
import type {
  DailyMemoryFileInfo,
  DailyMemorySearchResult,
} from "../workspace";

export type {
  DailyMemoryFileInfo,
  DailyMemorySearchResult,
} from "../workspace";

export const workspaceApi = {
  async readFile(filename: string): Promise<string | null> {
    return get<string | null>(
      `/workspace/file/${encodeURIComponent(filename)}`,
    );
  },

  async writeFile(filename: string, content: string): Promise<void> {
    return put<void>(`/workspace/file/${encodeURIComponent(filename)}`, {
      content,
    });
  },

  async listDailyMemoryFiles(): Promise<DailyMemoryFileInfo[]> {
    return get<DailyMemoryFileInfo[]>("/workspace/daily-memory");
  },

  async readDailyMemoryFile(filename: string): Promise<string | null> {
    return get<string | null>(
      `/workspace/daily-memory/${encodeURIComponent(filename)}`,
    );
  },

  async writeDailyMemoryFile(filename: string, content: string): Promise<void> {
    return put<void>(
      `/workspace/daily-memory/${encodeURIComponent(filename)}`,
      { content },
    );
  },

  async deleteDailyMemoryFile(filename: string): Promise<void> {
    return del<void>(`/workspace/daily-memory/${encodeURIComponent(filename)}`);
  },

  async searchDailyMemoryFiles(
    query: string,
  ): Promise<DailyMemorySearchResult[]> {
    return get<DailyMemorySearchResult[]>(
      `/workspace/daily-memory/search?query=${encodeURIComponent(query)}`,
    );
  },

  async openDirectory(subdir: "workspace" | "memory"): Promise<string> {
    return get<string>(`/workspace/directory?subdir=${subdir}`);
  },
};
