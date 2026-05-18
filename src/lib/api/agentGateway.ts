import { invoke } from "@tauri-apps/api/core";

export type AgentRuntimeKind =
  | "claude_code"
  | "codex"
  | "opencode"
  | "open_claw"
  | "gemini";

export type AgentStatus =
  | "created"
  | "launching"
  | "running"
  | "stopping"
  | "stopped"
  | "failed"
  | "exited"
  | "killed";

export type LaunchStrategy =
  | "windows_terminal"
  | "power_shell_window"
  | "background_process";

export type AgentPermissionMode =
  | "default"
  | "plan"
  | "accept_edits"
  | "auto"
  | "dont_ask"
  | "bypass_permissions";

export type AgentProviderMode =
  | "selected_provider"
  | "current_cc_switch_provider";

export interface ProviderSnapshotRequest {
  providerId?: string | null;
  providerMode?: AgentProviderMode | null;
}

export interface ProviderRuntimeSnapshot {
  providerId: string;
  providerName: string;
  providerType: string;
  appType: string;
  baseUrl: string;
  redactedBaseUrl: string;
  authTokenPresent: boolean;
  apiFormat?: string | null;
  upstreamModels: string[];
  defaultUpstreamModel?: string | null;
  redactedSettingsConfigJson: string;
  providerConfigHash?: string | null;
}

export interface LaunchAgentRequest {
  name: string;
  runtime: AgentRuntimeKind;
  providerId: string;
  providerMode?: AgentProviderMode | null;
  model?: string | null;
  claudeEntryModel?: string | null;
  upstreamProviderModel?: string | null;
  runProfileId?: string | null;
  cwd?: string | null;
  sessionId?: string | null;
  launchStrategy?: LaunchStrategy | null;
  permissionMode?: AgentPermissionMode | null;
}

export interface RestartAgentRequest {
  launchStrategy?: LaunchStrategy | null;
  permissionMode?: AgentPermissionMode | null;
}

export interface AgentInstance {
  id: string;
  name: string;
  runtime: AgentRuntimeKind;
  providerId: string;
  providerName?: string | null;
  model?: string | null;
  launchMode: "new" | "resume";
  runProfileId: string;
  port: number;
  cwd?: string | null;
  pid?: number | null;
  windowTitle?: string | null;
  sessionId?: string | null;
  status: AgentStatus;
  createdAt: string;
  startedAt?: string | null;
  stoppedAt?: string | null;
  lastError?: string | null;
  deletedAt?: string | null;
}

export interface AgentLog {
  id: string;
  agentId: string;
  level: string;
  event: string;
  message?: string | null;
  payloadJson?: string | null;
  createdAt: string;
}

export interface RunProfile {
  id: string;
  name: string;
  runtime: AgentRuntimeKind;
  kind: string;
  args: string[];
  env: Array<[string, string]>;
  allowCustomProfiles: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface CleanupReport {
  failedAgents: number;
  releasedPorts: number;
}

export interface AgentCommandError {
  code: string;
  message: string;
  suggestion: string;
  details?: string | null;
}

export const formatAgentError = (error: unknown): string => {
  if (error && typeof error === "object") {
    const maybeError = error as Partial<AgentCommandError>;
    if (maybeError.code && maybeError.message) {
      return `${maybeError.code}: ${maybeError.message}${
        maybeError.suggestion ? ` ${maybeError.suggestion}` : ""
      }${maybeError.details ? ` Details: ${maybeError.details}` : ""}`;
    }
  }
  return error instanceof Error ? error.message : String(error);
};

export const agentGatewayApi = {
  async launchAgent(req: LaunchAgentRequest): Promise<AgentInstance> {
    return await invoke("agent_gateway_launch_agent", { req });
  },

  async previewProviderSnapshot(
    req: ProviderSnapshotRequest,
  ): Promise<ProviderRuntimeSnapshot> {
    return await invoke("agent_gateway_preview_provider_snapshot", { req });
  },

  async stopAgent(agentId: string): Promise<void> {
    return await invoke("agent_gateway_stop_agent", { agentId });
  },

  async killAgent(agentId: string): Promise<void> {
    return await invoke("agent_gateway_kill_agent", { agentId });
  },

  async deleteAgent(agentId: string): Promise<void> {
    return await invoke("agent_gateway_delete_agent", { agentId });
  },

  async restartAgent(
    agentId: string,
    req: RestartAgentRequest,
  ): Promise<AgentInstance> {
    return await invoke("agent_gateway_restart_agent", { agentId, req });
  },

  async listAgents(): Promise<AgentInstance[]> {
    return await invoke("agent_gateway_list_agents");
  },

  async getAgent(agentId: string): Promise<AgentInstance> {
    return await invoke("agent_gateway_get_agent", { agentId });
  },

  async syncStatus(): Promise<AgentInstance[]> {
    return await invoke("agent_gateway_sync_status");
  },

  async getLogs(agentId: string, limit = 100): Promise<AgentLog[]> {
    return await invoke("agent_gateway_get_logs", { agentId, limit });
  },

  async listRunProfiles(): Promise<RunProfile[]> {
    return await invoke("agent_gateway_list_run_profiles");
  },

  async cleanupStale(): Promise<CleanupReport> {
    return await invoke("agent_gateway_cleanup_stale");
  },
};
