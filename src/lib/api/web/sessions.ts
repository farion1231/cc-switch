import { get, post } from "../web-client";
import type { SessionMeta, SessionMessage } from "@/types";
import type { DeleteSessionOptions, DeleteSessionResult } from "../sessions";

export const sessionsApi = {
  async list(): Promise<SessionMeta[]> {
    return get("/sessions");
  },

  async getMessages(
    providerId: string,
    sourcePath: string,
  ): Promise<SessionMessage[]> {
    return get(
      `/sessions/messages?providerId=${encodeURIComponent(providerId)}&sourcePath=${encodeURIComponent(sourcePath)}`,
    );
  },

  async delete(options: DeleteSessionOptions): Promise<boolean> {
    return post("/sessions/delete", {
      providerId: options.providerId,
      sessionId: options.sessionId,
      sourcePath: options.sourcePath,
    });
  },

  async deleteMany(
    items: DeleteSessionOptions[],
  ): Promise<DeleteSessionResult[]> {
    return Promise.all(
      items.map(async (item) => {
        try {
          await this.delete(item);
          return { ...item, success: true };
        } catch (error) {
          return {
            ...item,
            success: false,
            error: error instanceof Error ? error.message : String(error),
          };
        }
      }),
    );
  },

  async launchTerminal(_options: {
    command: string;
    cwd?: string | null;
    customConfig?: string | null;
  }): Promise<boolean> {
    // Launching a local terminal is inherently desktop-only; the web client
    // has no host shell access. The UI hides this action in web mode.
    console.warn("launch_session_terminal not available in web mode");
    return false;
  },
};
