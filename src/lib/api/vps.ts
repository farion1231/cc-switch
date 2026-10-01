import { invoke } from "@tauri-apps/api/core";
import { SKILLS_APP_IDS } from "@/config/appConfig";
import type { SkillApps } from "./skills";

export type VpsAuthMethod = "password" | "privateKey" | "certificate";

export interface VpsServer {
  id: string;
  name: string;
  purpose: string;
  host: string;
  port: number;
  user: string;
  identityFile?: string;
  authMethod?: VpsAuthMethod;
  certificateFile?: string;
  apps: SkillApps;
}

export type VpsConnectionStatus =
  | "success"
  | "hostKeyConfirmationRequired"
  | "hostKeyChanged"
  | "targetChanged"
  | "authenticationFailed"
  | "credentialsUnavailable"
  | "passwordRequired"
  | "sshNotFound"
  | "timeout"
  | "cancelled"
  | "failed";

export interface VpsConnectionResult {
  status: VpsConnectionStatus;
  fingerprint?: string;
  confirmationToken?: string;
  message?: string;
}

export function emptyVpsApps(): SkillApps {
  return Object.fromEntries(
    SKILLS_APP_IDS.map((app) => [app, false]),
  ) as unknown as SkillApps;
}

function serverPayload(server: VpsServer) {
  return {
    id: server.id,
    name: server.name,
    purpose: server.purpose,
    host: server.host,
    port: server.port,
    user: server.user,
    ...(server.identityFile !== undefined && {
      identityFile: server.identityFile,
    }),
    ...(server.authMethod !== undefined && { authMethod: server.authMethod }),
    ...(server.certificateFile !== undefined && {
      certificateFile: server.certificateFile,
    }),
    apps: Object.fromEntries(
      SKILLS_APP_IDS.map((app) => [app, Boolean(server.apps[app])]),
    ),
  };
}

export const vpsApi = {
  getServers: () => invoke<VpsServer[]>("get_vps_servers"),
  saveServer: (server: VpsServer, password?: string) =>
    invoke<VpsServer[]>("save_vps_server", {
      server: serverPayload(server),
      ...(password !== undefined && { password }),
    }),
  deleteServer: (id: string) =>
    invoke<VpsServer[]>("delete_vps_server", { id }),
  testConnection: (server: VpsServer, requestId: string, password?: string) =>
    invoke<VpsConnectionResult>("test_vps_connection", {
      server: serverPayload(server),
      requestId,
      ...(password !== undefined && { password }),
    }),
  cancelConnectionTest: (requestId: string) =>
    invoke<void>("cancel_vps_connection_test", { requestId }),
  confirmHostKey: (confirmationToken: string) =>
    invoke<void>("confirm_vps_host_key", { confirmationToken }),
};
