import { invoke } from "@tauri-apps/api/core";

/// Wire format matches the Rust `WorkspaceSyncSettings` (camelCase serde).
export interface WorkspaceSyncSettings {
  enabled: boolean;
  transport: "webdav" | "s3";
  providers: string[];
  remoteRoot: string;
  profile: string;
  status?: {
    lastSyncAt?: number;
    lastError?: string;
    lastErrorSource?: string;
  };
}

export interface WorkspaceScanPreviewItem {
  provider: string;
  installed: boolean;
  itemCount: number;
}

export interface WorkspaceConflictReport {
  provider: string;
  logicalId: string;
  resolution: string;
  conflictPath?: string | null;
}

export interface WorkspaceSyncReport {
  snapshotId: string;
  providersScanned: number;
  itemsTotal: number;
  archiveBytes: number;
  filesWritten: number;
  conflicts: WorkspaceConflictReport[];
}

export interface WorkspaceRemoteInfo {
  empty?: boolean;
  snapshotId?: string;
  deviceName?: string;
  updatedAt?: number;
  sizeBytes?: number;
}

export const workspaceSyncApi = {
  async getSettings(): Promise<WorkspaceSyncSettings> {
    return invoke<WorkspaceSyncSettings>("workspace_sync_get_settings");
  },

  async saveSettings(
    settings: WorkspaceSyncSettings,
  ): Promise<{ success: boolean }> {
    return invoke<{ success: boolean }>("workspace_sync_save_settings", {
      settings,
    });
  },

  async scanPreview(): Promise<{ providers: WorkspaceScanPreviewItem[] }> {
    return invoke<{ providers: WorkspaceScanPreviewItem[] }>(
      "workspace_sync_scan_preview",
    );
  },

  /// The one-button sync: pull remote union → merge with local → deploy →
  /// upload the merged union back.
  async sync(): Promise<WorkspaceSyncReport> {
    return invoke<WorkspaceSyncReport>("workspace_sync_run");
  },

  async fetchRemoteInfo(): Promise<WorkspaceRemoteInfo> {
    return invoke<WorkspaceRemoteInfo>("workspace_sync_fetch_remote_info");
  },
};
