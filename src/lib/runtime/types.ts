export type RuntimeConnectionStatus =
  | "local"
  | "connecting"
  | "online"
  | "reconnecting"
  | "offline"
  | "incompatible";

export interface RuntimeSnapshot {
  status: RuntimeConnectionStatus;
  generation: number;
  activeTargetId?: string;
  errorCode?: string;
  errorMessage?: string;
}

export interface RemoteTargetConfig {
  id: string;
  name: string;
  hostAlias: string;
  username?: string;
  port?: number;
  identityFile?: string;
}

/** 本机 OpenSSH 配置中发现的具体 Host；保存前不分配 CC Switch 目标 ID。 */
export interface DiscoveredSshTarget {
  name: string;
  hostAlias: string;
  hostname?: string;
  username?: string;
  port?: number;
  identityFile?: string;
}

export interface RemotePlatform {
  os: string;
  architecture: string;
}
