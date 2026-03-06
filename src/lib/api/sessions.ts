import { invoke } from "@tauri-apps/api/core";
import type {
  SessionExportFormat,
  SessionExportTarget,
  SessionMessage,
  SessionMeta,
} from "@/types";

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

  async saveExportFileDialog(options: {
    defaultName: string;
    format: SessionExportFormat;
  }): Promise<string | null> {
    const { defaultName, format } = options;
    return await invoke("save_session_export_file_dialog", {
      defaultName,
      format,
    });
  },

  async pickExportDirectoryDialog(): Promise<string | null> {
    return await invoke("pick_directory", { defaultPath: undefined });
  },

  async exportSingle(options: {
    providerId: string;
    sourcePath: string;
    format: SessionExportFormat;
    filePath: string;
    sessionId?: string;
    title?: string;
  }): Promise<boolean> {
    const { providerId, sourcePath, format, filePath, sessionId, title } = options;
    return await invoke("export_session_to_file", {
      providerId,
      sourcePath,
      format,
      filePath,
      sessionId,
      title,
    });
  },

  async exportBatch(options: {
    sessions: SessionExportTarget[];
    format: SessionExportFormat;
    directoryPath: string;
  }): Promise<number> {
    const { sessions, format, directoryPath } = options;
    return await invoke("export_sessions_to_directory", {
      sessions,
      format,
      directoryPath,
    });
  },
};
