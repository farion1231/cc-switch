import { invoke } from "@tauri-apps/api/core";

export interface DeviceUsageSettings {
  deviceId: string;
  deviceName: string;
  enabled: boolean;
  lastSyncAt: number | null;
  lastError: string | null;
}
export interface DeviceUsageRow {
  date: string;
  appType: string;
  model: string;
  verified: boolean;
  requests: number;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cost: string;
}
export interface DeviceUsageReport {
  settings: DeviceUsageSettings;
  devices: {
    deviceId: string;
    deviceName: string;
    capturedAt: number;
    local: boolean;
  }[];
  rows: DeviceUsageRow[];
  totals: DeviceUsageRow[];
  conflictingEvents: number;
}
export type DeviceUsageStatus = Pick<DeviceUsageReport, "settings" | "devices">;
export const deviceUsageApi = {
  status: () => invoke<DeviceUsageStatus>("get_usage_sync_status"),
  report: (
    scope: string,
    start?: string,
    end?: string,
    app?: string,
    model?: string,
  ) =>
    invoke<DeviceUsageReport>("get_device_usage", {
      scope,
      start,
      end,
      app,
      model,
    }),
  configure: (enabled: boolean, name: string) =>
    invoke<DeviceUsageSettings>("configure_usage_sync", { enabled, name }),
  sync: () => invoke<void>("sync_device_usage"),
};
