import { invoke } from "@tauri-apps/api/core";

export type LanWebMode = "off" | "local" | "lan";

export interface LanWebStatus {
  mode: LanWebMode;
  port: number;
  allowControl: boolean;
  remoteControlApps: string[];
  installedRemoteApps: string[];
  running: boolean;
  listenAddress: string | null;
  localUrl: string | null;
  lanUrls: string[];
  lanHosts: string[];
  token: string;
  error: string | null;
}

export interface LanWebConfigUpdate {
  mode: LanWebMode;
  port: number;
  allowControl: boolean;
  remoteControlApps: string[];
}

export const lanWebApi = {
  getStatus: (): Promise<LanWebStatus> => invoke("get_lan_web_status"),

  updateConfig: (config: LanWebConfigUpdate): Promise<LanWebStatus> =>
    invoke("update_lan_web_config", { config }),

  regenerateToken: (): Promise<LanWebStatus> =>
    invoke("regenerate_lan_web_token"),
};
