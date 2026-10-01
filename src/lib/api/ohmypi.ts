import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";

export interface OhMyPiCurrentState {
  enabledProviderIds: string[];
  defaultProviderId: string | null;
}

/** omp agent auto-discovery snapshot used by the enable-ohmypi guard. */
export interface OhMyPiAgentDiscoveryState {
  needsConfirmation: boolean;
  requiredProviderIds: string[];
  missingProviderIds: string[];
}

/** ` for confirm dialog listing. */
export interface OhMyPiAgentDiscoveryProvider {
  id: string;
  displayName: string;
}

export const ohmypiApi = {
  async getCurrentState(): Promise<OhMyPiCurrentState> {
    return await invoke("get_ohmypi_current_state");
  },

  async updateProviderUsageScript(
    id: string,
    usageScript: UsageScript,
  ): Promise<boolean> {
    return await invoke("update_ohmypi_provider_usage_script", {
      id,
      usageScript,
    });
  },

  async getAgentDiscoveryState(): Promise<OhMyPiAgentDiscoveryState> {
    return await invoke("get_ohmypi_agent_discovery_state");
  },

  async getAgentDiscoveryProviders(): Promise<OhMyPiAgentDiscoveryProvider[]> {
    return await invoke("get_ohmypi_agent_discovery_providers");
  },

  async disableAgentAutoDiscovery(): Promise<string[]> {
    return await invoke("disable_ohmypi_agent_auto_discovery");
  },
};
