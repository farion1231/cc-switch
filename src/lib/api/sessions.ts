import { invoke } from "@tauri-apps/api/core";
import type { SessionMessage, SessionMeta } from "@/types";

export interface DeleteSessionOptions {
  providerId: string;
  sessionId: string;
  sourcePath: string;
}

export interface DeleteSessionResult extends DeleteSessionOptions {
  success: boolean;
  error?: string;
}

export type SessionExportFormat = "markdown" | "html" | "text" | "raw";

export interface ExportSessionOptions {
  providerId: string;
  sessionId: string;
  sourcePath: string;
  format: SessionExportFormat;
  outputPath: string;
}

export interface RenderSessionExportOptions {
  providerId: string;
  sessionId: string;
  sourcePath: string;
  format: Exclude<SessionExportFormat, "raw">;
}

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

  async delete(options: DeleteSessionOptions): Promise<boolean> {
    const { providerId, sessionId, sourcePath } = options;
    return await invoke("delete_session", {
      providerId,
      sessionId,
      sourcePath,
    });
  },

  async deleteMany(
    items: DeleteSessionOptions[],
  ): Promise<DeleteSessionResult[]> {
    return await invoke("delete_sessions", { items });
  },

  async export(options: ExportSessionOptions): Promise<boolean> {
    return await invoke("export_session", { request: options });
  },

  async renderExport(options: RenderSessionExportOptions): Promise<string> {
    return await invoke("render_session_export", { request: options });
  },

  async saveExportFileDialog(options: {
    defaultName: string;
    filterName: string;
    extensions: string[];
  }): Promise<string | null> {
    return await invoke("save_file_dialog_with_filter", options);
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
};
