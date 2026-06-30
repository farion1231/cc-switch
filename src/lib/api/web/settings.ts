import { get, post, put, del, connectWebSocket } from "../web-client";
import type {
  Settings,
  WebDavSyncSettings,
  S3SyncSettings,
  RemoteSnapshotInfo,
} from "@/types";
import type { AppId } from "./types";
import type {
  BackupEntry,
  ToolInstallationReport,
  RectifierConfig,
  OptimizerConfig,
  LogConfig,
} from "../settings";
import type { CodexUnifyHistoryRestoreResult } from "../settings";

export interface ConfigTransferResult {
  success: boolean;
  message: string;
  filePath?: string;
  backupId?: string;
}

export interface WebDavTestResult {
  success: boolean;
  message?: string;
}

export interface WebDavSyncResult {
  status: string;
}

export const settingsApi = {
  async get(): Promise<Settings> {
    return get("/settings");
  },

  async save(settings: Settings): Promise<boolean> {
    return put("/settings", settings);
  },

  async restart(): Promise<boolean> {
    console.warn("restart_app not available in web mode");
    return true;
  },

  async checkUpdates(): Promise<void> {
    console.warn("check_for_updates not available in web mode");
  },

  async installUpdateAndRestart(): Promise<boolean> {
    console.warn("install_update_and_restart not available in web mode");
    return false;
  },

  async hasCodexUnifyHistoryBackup(): Promise<boolean> {
    console.warn("has_codex_unify_history_backup not available in web mode");
    return false;
  },

  async restoreCodexUnifiedHistory(): Promise<CodexUnifyHistoryRestoreResult> {
    console.warn("restore_codex_unified_history not available in web mode");
    return { restoredJsonlFiles: 0, restoredStateRows: 0 };
  },

  async isPortable(): Promise<boolean> {
    return false;
  },

  async getConfigDir(appId: AppId): Promise<string> {
    try {
      const response = await get<{ path: string }>(
        `/settings/config-dir?app=${appId}`,
      );
      return typeof response?.path === "string" ? response.path : "";
    } catch (error) {
      console.warn(
        "getConfigDir endpoint unavailable in web mode, using default path fallback",
        error,
      );
      return "";
    }
  },

  async openConfigFolder(_appId: AppId): Promise<void> {
    console.warn("open_config_folder not available in web mode");
  },

  async selectConfigDirectory(defaultPath?: string): Promise<string | null> {
    console.warn("pick_directory not available in web mode");
    return defaultPath || null;
  },

  async pickDirectory(defaultPath?: string): Promise<string | null> {
    return this.selectConfigDirectory(defaultPath);
  },

  async getClaudeCodeConfigPath(): Promise<string> {
    const response = await get<{ path: string }>("/settings/claude-code-path");
    return response.path;
  },

  async getAppConfigPath(): Promise<string> {
    const response = await get<{ path: string }>("/settings/app-config-path");
    return response.path;
  },

  async openAppConfigFolder(): Promise<void> {
    console.warn("open_app_config_folder not available in web mode");
  },

  async getAppConfigDirOverride(): Promise<string | null> {
    try {
      const response = await get<{ path: string | null }>(
        "/settings/app-config-dir-override",
      );
      return response?.path ?? null;
    } catch (error) {
      console.warn(
        "getAppConfigDirOverride endpoint unavailable in web mode, using null override",
        error,
      );
      return null;
    }
  },

  async setAppConfigDirOverride(path: string | null): Promise<boolean> {
    return post("/settings/app-config-dir-override", { path });
  },

  async applyClaudePluginConfig(options: {
    official: boolean;
  }): Promise<boolean> {
    return post("/settings/apply-claude-plugin", options);
  },

  async applyClaudeOnboardingSkip(): Promise<boolean> {
    return post("/settings/claude-onboarding-skip", {});
  },

  async clearClaudeOnboardingSkip(): Promise<boolean> {
    return del("/settings/claude-onboarding-skip");
  },

  async saveFileDialog(defaultName: string): Promise<string | null> {
    console.warn("save_file_dialog not available in web mode");
    return defaultName;
  },

  async openFileDialog(): Promise<string | null> {
    console.warn("open_file_dialog not available in web mode");
    return null;
  },

  async exportConfigToFile(filePath: string): Promise<ConfigTransferResult> {
    return post("/settings/export", { filePath });
  },

  async importConfigFromFile(filePath: string): Promise<ConfigTransferResult> {
    return post("/settings/import", { filePath });
  },

  async webdavTestConnection(
    settings: WebDavSyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return post("/settings/webdav/test", { settings, preserveEmptyPassword });
  },

  async webdavSyncUpload(): Promise<WebDavSyncResult> {
    return post("/settings/webdav/upload", {});
  },

  async webdavSyncDownload(): Promise<WebDavSyncResult> {
    return post("/settings/webdav/download", {});
  },

  async webdavSyncSaveSettings(
    settings: WebDavSyncSettings,
    passwordTouched = false,
  ): Promise<{ success: boolean }> {
    return post("/settings/webdav/settings", { settings, passwordTouched });
  },

  async webdavSyncFetchRemoteInfo(): Promise<
    RemoteSnapshotInfo | { empty: true }
  > {
    return get("/settings/webdav/remote-info");
  },

  // ===== S3 Sync API =====

  async s3TestConnection(
    settings: S3SyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return post("/settings/s3/test", { settings, preserveEmptyPassword });
  },

  async s3SyncUpload(): Promise<WebDavSyncResult> {
    return post("/settings/s3/upload", {});
  },

  async s3SyncDownload(): Promise<WebDavSyncResult> {
    return post("/settings/s3/download", {});
  },

  async s3SyncSaveSettings(
    settings: S3SyncSettings,
    passwordTouched: boolean,
  ): Promise<{ success: boolean }> {
    return post("/settings/s3/settings", { settings, passwordTouched });
  },

  async s3SyncFetchRemoteInfo(): Promise<RemoteSnapshotInfo | { empty: true }> {
    return get("/settings/s3/remote-info");
  },

  async syncCurrentProvidersLive(): Promise<void> {
    await post("/settings/sync-providers-live", {});
  },

  async openExternal(url: string): Promise<void> {
    window.open(url, "_blank", "noopener,noreferrer");
  },

  async setAutoLaunch(enabled: boolean): Promise<boolean> {
    console.warn("set_auto_launch not available in web mode");
    return enabled;
  },

  async getAutoLaunchStatus(): Promise<boolean> {
    return false;
  },

  async getToolVersions(
    tools?: string[],
    wslShellByTool?: Record<
      string,
      { wslShell?: string | null; wslShellFlag?: string | null }
    >,
  ): Promise<
    Array<{
      name: string;
      version: string | null;
      latest_version: string | null;
      error: string | null;
      installed_but_broken: boolean;
      env_type: "windows" | "wsl" | "macos" | "linux" | "unknown";
      wsl_distro: string | null;
    }>
  > {
    const query = tools?.length
      ? `?tools=${encodeURIComponent(tools.join(","))}`
      : "";
    return post(`/settings/tool-versions${query}`, { tools, wslShellByTool });
  },

  async getRectifierConfig(): Promise<RectifierConfig> {
    return get("/settings/rectifier-config");
  },

  async setRectifierConfig(config: RectifierConfig): Promise<boolean> {
    return put("/settings/rectifier-config", config);
  },

  async getOptimizerConfig(): Promise<OptimizerConfig> {
    return get("/settings/optimizer-config");
  },

  async setOptimizerConfig(config: OptimizerConfig): Promise<boolean> {
    return put("/settings/optimizer-config", config);
  },

  async getLogConfig(): Promise<LogConfig> {
    return get("/settings/log-config");
  },

  async setLogConfig(config: LogConfig): Promise<boolean> {
    return put("/settings/log-config", config);
  },

  onWebDavSyncStatusUpdated(
    callback: (status: { status: string; error?: string }) => void,
  ): () => void {
    return connectWebSocket((data: any) => {
      if (data.event === "webdav-sync-status-updated") {
        callback(data.data);
      }
    });
  },

  async probeToolInstallations(
    tools: string[],
  ): Promise<ToolInstallationReport[]> {
    return post("/settings/probe-tool-installations", { tools });
  },

  async runToolLifecycleAction(
    tools: string[],
    action: "install" | "update",
    wslShellByTool?: Record<
      string,
      { wslShell?: string | null; wslShellFlag?: string | null }
    >,
  ): Promise<void> {
    await post("/settings/tool-lifecycle", { tools, action, wslShellByTool });
  },
};

export const backupsApi = {
  async createDbBackup(): Promise<string> {
    return post("/settings/backups", {});
  },

  async listDbBackups(): Promise<BackupEntry[]> {
    return get("/settings/backups");
  },

  async restoreDbBackup(filename: string): Promise<string> {
    return post(
      `/settings/backups/${encodeURIComponent(filename)}/restore`,
      {},
    );
  },

  async renameDbBackup(oldFilename: string, newName: string): Promise<string> {
    return put(`/settings/backups/${encodeURIComponent(oldFilename)}`, {
      newName,
    });
  },

  async deleteDbBackup(filename: string): Promise<void> {
    await del(`/settings/backups/${encodeURIComponent(filename)}`);
  },
};
