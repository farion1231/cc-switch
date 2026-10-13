// 跨设备用量同步（Usage Sync）相关类型，与后端 services::usage_sync 对应

export type UsageSyncBackend = "webdav" | "s3";

/** records = 逐请求用量记录；daily = 仅按天汇总（不共享请求 ID、时间戳、耗时） */
export type UsageSyncDetailLevel = "records" | "daily";

export interface UsageSyncStatus {
  lastSyncAt?: number;
  lastUploadAt?: number;
  lastError?: string;
  lastErrorAt?: number;
  lastErrorSource?: string;
  /** 另一个安装在用本机的设备身份上传，上传已暂停 */
  identityConflict: boolean;
}

export interface UsageSyncSettings {
  enabled: boolean;
  autoSync: boolean;
  backend?: UsageSyncBackend;
  detailLevel: UsageSyncDetailLevel;
  intervalMinutes: number;
  status: UsageSyncStatus;
}

export type RemoteDeviceStatus =
  | "ok"
  | "error"
  | "incompatible"
  | "missing"
  | "hidden";

export interface RemoteDeviceInfo {
  deviceId: string;
  deviceName: string;
  platform?: string | null;
  appVersion?: string | null;
  detailLevel: UsageSyncDetailLevel;
  /** 该设备最近一次上传（秒） */
  lastUploadAt?: number | null;
  /** 本机最近一次导入该设备数据（秒） */
  lastFetchedAt?: number | null;
  lastCheckedAt?: number | null;
  status: RemoteDeviceStatus;
  lastError?: string | null;
  firstDate?: string | null;
  lastDate?: string | null;
  detailFrom?: string | null;
  logRows: number;
  rollupRows: number;
}

export interface UsageSyncOverview {
  settings: UsageSyncSettings;
  availableBackends: { webdav: boolean; s3: boolean };
  remoteLocation?: string | null;
  /** false = 明文 HTTP 连接到非本机地址 */
  transportSecure?: boolean | null;
  thisDevice: {
    deviceId: string;
    deviceName: string;
    customName?: string | null;
  };
  devices: RemoteDeviceInfo[];
  syncing: boolean;
  staleAfterSecs: number;
}

export interface UsageSyncReport {
  uploadedPartitions: number;
  manifestWritten: boolean;
  uploadError?: string | null;
  identityConflict: boolean;
  devicesSeen: number;
  devicesUpdated: number;
  deviceErrors: { deviceId: string; error: string }[];
  downloadError?: string | null;
}

export type UsageSyncConflictResolution = "newIdentity" | "takeOver";

export interface UsageSyncPreferences {
  enabled: boolean;
  autoSync: boolean;
  backend?: UsageSyncBackend;
  detailLevel: UsageSyncDetailLevel;
  intervalMinutes: number;
}
