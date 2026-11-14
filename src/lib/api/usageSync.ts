import { invoke } from "@tauri-apps/api/core";
import type {
  UsageSyncConflictResolution,
  UsageSyncOverview,
  UsageSyncPreferences,
  UsageSyncReport,
  UsageSyncSettings,
} from "@/types/usageSync";

export const usageSyncApi = {
  getOverview: async (): Promise<UsageSyncOverview> =>
    invoke("usage_sync_get_overview"),

  saveSettings: async (
    preferences: UsageSyncPreferences,
  ): Promise<UsageSyncSettings> =>
    invoke("usage_sync_save_settings", { ...preferences }),

  runNow: async (): Promise<UsageSyncReport> => invoke("usage_sync_run_now"),

  /** 空字符串 / null 恢复系统主机名 */
  setDeviceName: async (name: string | null): Promise<void> =>
    invoke("usage_sync_set_device_name", { name }),

  setDeviceHidden: async (deviceId: string, hidden: boolean): Promise<void> =>
    invoke("usage_sync_set_device_hidden", { deviceId, hidden }),

  resolveConflict: async (
    resolution: UsageSyncConflictResolution,
  ): Promise<void> => invoke("usage_sync_resolve_conflict", { resolution }),
};
