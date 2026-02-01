import { invoke } from "@tauri-apps/api/core";
import type { SessionMessage, SessionMeta } from "@/types";

export const sessionsApi = {
  async list(): Promise<SessionMeta[]> {
    return await invoke("list_sessions");
  },

  async getMessages(
    providerId: string,
    sourcePath: string
  ): Promise<SessionMessage[]> {
    return await invoke("get_session_messages", { providerId, sourcePath });
  },

  async launchTerminal(options: {
    target: string;
    command: string;
    cwd?: string | null;
    customConfig?: string | null;
  }): Promise<boolean> {
    const { target, command, cwd, customConfig } = options;
    return await invoke("launch_session_terminal", {
      target,
      command,
      cwd,
      customConfig,
    });
  },
};
