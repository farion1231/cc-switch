import { invoke } from "@tauri-apps/api/core";
import type { SessionMessage, SessionMeta } from "@/types";

export const sessionsApi = {
  async list(): Promise<SessionMeta[]> {
    return await invoke("list_sessions");
  },

  async getMessages(
    providerId: string,
    sourcePath: string,
  ): Promise<SessionMessage[]> {
    return await invoke("get_session_messages", { providerId, sourcePath });
  },

  async launchTerminal(options: {
    command: string;
    cwd?: string | null;
    customConfig?: string | null;
  }): Promise<boolean> {
    const { command, cwd, customConfig } = options;
    return await invoke("launch_session_terminal", {
      command,
      cwd,
      customConfig,
    });
  },

  async getAllAliases(): Promise<Record<string, string>> {
    return await invoke("get_all_session_aliases");
  },

  async setAlias(sessionKey: string, alias: string): Promise<void> {
    return await invoke("set_session_alias", { sessionKey, alias });
  },

  async deleteAlias(sessionKey: string): Promise<void> {
    return await invoke("delete_session_alias", { sessionKey });
  },
};
