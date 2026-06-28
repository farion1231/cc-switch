import { isTauri } from "@/lib/environment";

import {
  providersApi as tauriProvidersApi,
  universalProvidersApi as tauriUniversalProvidersApi,
} from "./providers";
import { settingsApi as tauriSettingsApi } from "./settings";
import { backupsApi as tauriBackupsApi } from "./settings";
import { mcpApi as tauriMcpApi } from "./mcp";
import { promptsApi as tauriPromptsApi } from "./prompts";
import { skillsApi as tauriSkillsApi } from "./skills";
import { usageApi } from "./usage";
import { vscodeApi } from "./vscode";
import { proxyApi as tauriProxyApi } from "./proxy";
import { openclawApi as tauriOpenclawApi } from "./openclaw";
import { sessionsApi as tauriSessionsApi } from "./sessions";
import { workspaceApi as tauriWorkspaceApi } from "./workspace";
import { workspaceApi as webWorkspaceApi } from "./web/workspace";
import * as tauriConfigApi from "./config";
import { authApi } from "./auth";

import {
  providersApi as webProvidersApi,
  universalProvidersApi as webUniversalProvidersApi,
} from "./web/providers";
import { settingsApi as webSettingsApi } from "./web/settings";
import { backupsApi as webBackupsApi } from "./web/settings";
import { mcpApi as webMcpApi } from "./web/mcp";
import * as webConfigApi from "./web/config";
import { promptsApi as webPromptsApi } from "./web/prompts";
import { skillsApi as webSkillsApi } from "./web/skills";
import { proxyApi as webProxyApi } from "./web/proxy";
import { sessionsApi as webSessionsApi } from "./web/sessions";
import { openclawApi as webOpenclawApi } from "./web/openclaw";

export type { AppId } from "./types";
export type { ProviderSwitchEvent } from "./providers";
export type { Prompt } from "./prompts";
export type { GitHubAccount } from "./copilot";
export type {
  ManagedAuthProvider,
  ManagedAuthStatus,
  ManagedAuthDeviceCodeResponse,
} from "./auth";

// Runtime API selection based on environment
// Desktop app uses Tauri APIs, Web uses HTTP APIs
export const providersApi = isTauri() ? tauriProvidersApi : webProvidersApi;
export const universalProvidersApi = isTauri()
  ? tauriUniversalProvidersApi
  : webUniversalProvidersApi;
export const settingsApi = isTauri() ? tauriSettingsApi : webSettingsApi;
export const backupsApi = isTauri() ? tauriBackupsApi : webBackupsApi;
export const mcpApi = isTauri() ? tauriMcpApi : webMcpApi;
export const configApi = isTauri() ? tauriConfigApi : webConfigApi;
export const promptsApi = isTauri() ? tauriPromptsApi : webPromptsApi;
export const skillsApi = isTauri() ? tauriSkillsApi : webSkillsApi;
export const proxyApi = isTauri() ? tauriProxyApi : webProxyApi;
export const sessionsApi = isTauri() ? tauriSessionsApi : webSessionsApi;
export const openclawApi = isTauri() ? tauriOpenclawApi : webOpenclawApi;
export { usageApi };
export { vscodeApi };
export const workspaceApi = isTauri() ? tauriWorkspaceApi : webWorkspaceApi;
export { authApi };
